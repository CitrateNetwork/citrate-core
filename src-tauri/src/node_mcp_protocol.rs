//! HUP-S4.2 — the citrate-node MCP protocol core: JSON-RPC 2.0 dispatch for the MCP base protocol
//! (`initialize`, `ping`, `tools/*`, `resources/*`), independent of the transport.
//!
//! The data comes from a [`NodeBackend`]. Production uses the app-backed backend in
//! `node_mcp_live.rs`; tests use fixtures. The dispatcher owns the rules that must hold whatever
//! the backend is: argument validation, the read-only RPC allowlist, the precompile allowlist, and
//! the routing of every write into the [`ApprovalInbox`] (signatures through the ceremony).
//!
//! Two protocol eras are served side by side:
//! - **session era** (2025-06-18, 2025-03-26, 2024-11-05): `initialize` opens a session;
//! - **stateless era** (2026-07-28): every request carries its protocol version, client info and
//!   client capabilities in `params._meta`; there is no `initialize` (and no `ping`);
//!   `server/discover` describes the server; every result carries `resultType`, and list/read
//!   results carry the `ttlMs` / `cacheScope` caching hints.
//!
//! **MCP Tasks** (the `io.modelcontextprotocol/tasks` extension, SEP-2663): a client that
//! declares the extension in its per-request capabilities gets a task handle (`resultType:
//! "task"`) from every write tool instead of a plain pending reply. The task IS the approval
//! request: `tasks/get` reports `working` while the member decides and `completed` with the tool
//! result once they have; `tasks/cancel` withdraws a request that is still pending. A task never
//! needs input from the client (`input_required` is never used): the member decides in the app.

use crate::node_mcp_approvals::{
    ApprovalInbox, CeremonyToClose, McpAction, McpRequest, RequestKind,
};
use crate::node_mcp_tools::{self as tools, ToolKind};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// The protocol versions this server speaks, newest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// The stateless protocol revision this server also speaks (per-request `_meta`, no session).
pub const STATELESS_PROTOCOL_VERSION: &str = "2026-07-28";

/// Every version, newest first, as `server/discover` lists them.
pub fn all_protocol_versions() -> Vec<&'static str> {
    let mut v = vec![STATELESS_PROTOCOL_VERSION];
    v.extend_from_slice(SUPPORTED_PROTOCOL_VERSIONS);
    v
}

/// `_meta` keys of the stateless era.
pub const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
pub const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
pub const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
pub const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
/// The MCP Tasks extension id.
pub const TASKS_EXTENSION: &str = "io.modelcontextprotocol/tasks";

/// How long a task handle stays meaningful, from creation (a pending request expires after 15
/// minutes; a decided one is kept for polling for at least as long again). Pending owner sign-off.
pub const TASK_TTL_MS: u64 = 2 * crate::node_mcp_approvals::REQUEST_TTL_MS;
/// Suggested polling interval for a task (a person is deciding; there is no point polling faster).
pub const TASK_POLL_MS: u64 = 5_000;
/// Caching hint for results that never change while the app runs (tool catalog, templates).
const STATIC_TTL_MS: u64 = 60 * 60 * 1000;

/// JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// MCP: resource not found (session era; the stateless era uses INVALID_PARAMS).
pub const RESOURCE_NOT_FOUND: i64 = -32002;
/// MCP 2026-07-28: a mirrored HTTP header does not match the body.
pub const HEADER_MISMATCH: i64 = -32020;
/// MCP 2026-07-28: the requested protocol version is not supported.
pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// The read-only JSON-RPC methods the backend may be asked to run. Nothing that sends, signs or
/// mutates node state is on this list.
pub const READ_RPC_METHODS: &[&str] = &[
    "eth_chainId",
    "eth_blockNumber",
    "net_peerCount",
    "eth_getBalance",
    "eth_call",
    "eth_estimateGas",
    "eth_getLogs",
    "eth_getTransactionReceipt",
    "citrate_getDagStats",
];

/// A chain read and where it came from (`local-node` or `public-rpc`).
#[derive(Debug, Clone, PartialEq)]
pub struct RpcRead {
    pub value: Value,
    pub source: String,
}

/// A transaction proposal that opened a ceremony.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedSignature {
    pub ceremony_id: String,
    /// The ceremony's own decoded view (serialized `CeremonyView`).
    pub ceremony: Value,
}

/// Where the node's data comes from. Every method returns an honest error when its source is not
/// available (node stopped, vault locked, daemon down); none fabricates a value.
pub trait NodeBackend: Send + Sync {
    /// `NodeStatus` (state, peers, height, syncPct).
    fn node_status(&self) -> Result<Value, String>;
    /// Run one read-only JSON-RPC method. Only called with a method in [`READ_RPC_METHODS`].
    fn rpc_read(&self, method: &str, params: Value) -> Result<RpcRead, String>;
    /// This member's PUBLIC wallet address (never key material).
    fn wallet_address(&self) -> Result<String, String>;
    /// Search a shared memory tenant (only tenants in `tools::MEMORY_TENANTS` are passed).
    fn memory_search(&self, tenant: &str, query: &str, limit: usize) -> Result<Value, String>;
    /// The member's groups.
    fn groups(&self) -> Result<Value, String>;
    fn cluster_status(&self, group: &str) -> Result<Value, String>;
    fn cluster_peers(&self, group: &str) -> Result<Value, String>;
    /// Outstanding invites for a group, WITHOUT tokens or links.
    fn invites(&self, group: &str) -> Result<Value, String>;
    /// Open a SignatureCeremony for a transaction from this member's wallet. Signs nothing.
    fn propose_transaction(
        &self,
        origin: &str,
        to: &str,
        value_wei: u128,
        data: &str,
    ) -> Result<ProposedSignature, String>;
    /// Close a ceremony that will never be approved (expired / token revoked).
    fn close_ceremony(&self, ceremony_id: &str);
    /// HUP-S6.5: a deploy-gas top-up for this member's wallet, under the member's faucet budget
    /// (`crate::faucet`). Signs nothing. Backends without a faucet say so.
    fn faucet_request(&self, _origin: &str, _initcode_hash: &str) -> Result<Value, String> {
        Err("The in-app faucet is not available from this server.".to_string())
    }
    /// The member's linked devices and revoked device addresses (public data only).
    fn devices(&self) -> Result<Value, String>;
    /// The files this node keeps (the pinning file store).
    fn pins(&self) -> Result<Value, String>;
    /// Open a contract-creation SignatureCeremony for `bytecode ++ constructor_args`, refused
    /// unless the deploy gate is READY for exactly those bytes. Signs nothing.
    fn propose_deploy(
        &self,
        origin: &str,
        bytecode: &str,
        constructor_args: &str,
        value_wei: u128,
        gas: Option<u64>,
    ) -> Result<ProposedSignature, String>;
    /// `Ok` when an anchor pass could raise approval cards now (AnchorRegistry in the address
    /// book and nightly anchoring on); otherwise the honest reason.
    fn anchor_ready(&self) -> Result<(), String>;
    /// The ABI registry entry for a contract on 40204: CitrateScan's verified-source record
    /// (match status, name, compiler, ABI), without the source text. Never a guessed ABI.
    fn contract_abi(&self, address: &str) -> Result<Value, String>;
    /// HUP-S1.1: the Hermes sidecar's open sessions (`GET /sessions`).
    fn hermes_sessions(&self) -> Result<Value, String> {
        Err(crate::node_mcp_hermes::HERMES_UNAVAILABLE.to_string())
    }
    /// HUP-S1.1: a Hermes session's events after `after`, waiting up to `wait_ms` for one.
    fn hermes_events(&self, session: &str, after: u64, wait_ms: u64) -> Result<Value, String> {
        let _ = (session, after, wait_ms);
        Err(crate::node_mcp_hermes::HERMES_UNAVAILABLE.to_string())
    }
}

/// Who is calling (resolved by the transport from the connect token + the session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerCtx {
    pub token_id: String,
    pub token_label: String,
    /// `clientInfo.name` from `initialize`, if the client sent one (shown as-is, never trusted).
    pub client_name: Option<String>,
    /// The connect token is limited to the read tools (Hermes's own token): write tools are not
    /// listed and a call to one is refused before anything is queued.
    pub read_only: bool,
}

impl CallerCtx {
    /// The origin shown to the member in approval cards and ceremonies.
    pub fn origin(&self) -> String {
        let clean = |s: &str| -> String {
            s.chars()
                .filter(|c| !tools::is_unsafe_display_char(*c))
                .take(48)
                .collect::<String>()
        };
        match &self.client_name {
            Some(n) if !n.trim().is_empty() => {
                format!("mcp:{} via {}", clean(&self.token_label), clean(n))
            }
            _ => format!("mcp:{}", clean(&self.token_label)),
        }
    }
}

/// One line in the recent-calls log the member sees in Settings.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallLogEntry {
    pub at_ms: u64,
    pub token_id: String,
    pub method: String,
    pub tool: Option<String>,
    pub ok: bool,
}

const CALL_LOG_CAP: usize = 100;

/// The server core: backend + inbox + call log.
pub struct McpCore {
    backend: Arc<dyn NodeBackend>,
    inbox: Arc<ApprovalInbox>,
    calls: Mutex<std::collections::VecDeque<CallLogEntry>>,
    clock: fn() -> u64,
}

/// Wall-clock Unix ms.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// A `tools/call` result carrying structured JSON (wrapped in an object when it is not one).
fn tool_ok(value: Value) -> Value {
    let structured = if value.is_object() {
        value
    } else {
        json!({ "result": value })
    };
    let text = serde_json::to_string_pretty(&structured).unwrap_or_else(|_| structured.to_string());
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": false,
    })
}

/// A `tools/call` result reporting a tool-level failure (the model sees it and can recover).
fn tool_err(message: &str) -> Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true,
    })
}

/// The shape a client sees for one of its requests.
pub fn request_view(r: &McpRequest) -> Value {
    serde_json::to_value(r)
        .map(|mut v| {
            if let Some(o) = v.as_object_mut() {
                // The token id is the member's bookkeeping; the client already knows who it is.
                o.remove("tokenId");
            }
            v
        })
        .unwrap_or(Value::Null)
}

/// Hex quantity (`0x1a`) → u64.
fn hex_u64(v: &Value) -> Option<u64> {
    v.as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .and_then(|h| u64::from_str_radix(h, 16).ok())
}

/// Hex quantity → u128 rendered as a decimal string.
fn hex_u128_dec(v: &Value) -> Option<String> {
    v.as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .and_then(|h| u128::from_str_radix(h, 16).ok())
        .map(|n| n.to_string())
}

impl McpCore {
    pub fn new(backend: Arc<dyn NodeBackend>, inbox: Arc<ApprovalInbox>) -> Self {
        McpCore {
            backend,
            inbox,
            calls: Mutex::new(Default::default()),
            clock: now_ms,
        }
    }

    /// Tests: a fixed clock.
    #[cfg(test)]
    pub fn with_clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = clock;
        self
    }

    pub fn inbox(&self) -> &Arc<ApprovalInbox> {
        &self.inbox
    }

    pub fn backend(&self) -> &Arc<dyn NodeBackend> {
        &self.backend
    }

    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// The recent calls, newest first.
    pub fn recent_calls(&self) -> Vec<CallLogEntry> {
        let g = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        g.iter().rev().cloned().collect()
    }

    fn log_call(&self, ctx: &CallerCtx, method: &str, tool: Option<&str>, ok: bool) {
        let mut g = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() >= CALL_LOG_CAP {
            g.pop_front();
        }
        g.push_back(CallLogEntry {
            at_ms: self.now(),
            token_id: ctx.token_id.clone(),
            method: method.to_string(),
            tool: tool.map(str::to_string),
            ok,
        });
    }

    fn close(&self, ceremonies: Vec<CeremonyToClose>) {
        for c in ceremonies {
            self.backend.close_ceremony(&c.0);
        }
    }

    /// Negotiate a protocol version: the client's if we speak it, else our newest.
    pub fn negotiate(requested: Option<&str>) -> &'static str {
        requested
            .and_then(|r| {
                SUPPORTED_PROTOCOL_VERSIONS
                    .iter()
                    .find(|v| **v == r)
                    .copied()
            })
            .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0])
    }

    /// The `initialize` result.
    pub fn initialize_result(requested: Option<&str>) -> Value {
        json!({
            "protocolVersion": Self::negotiate(requested),
            "capabilities": Self::capabilities(),
            "serverInfo": Self::server_info(),
            "instructions": INSTRUCTIONS,
        })
    }

    /// Dispatch one JSON-RPC message. Returns `None` for notifications and responses (nothing to
    /// send back). `msg` must be a single object; the transport rejects batches.
    pub fn dispatch(&self, ctx: &CallerCtx, msg: &Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(rpc_error(
                &Value::Null,
                INVALID_REQUEST,
                "expected a JSON-RPC object",
            ));
        };
        let method = obj.get("method").and_then(Value::as_str);
        let id = obj.get("id").cloned();
        let (Some(method), Some(id)) = (method, id) else {
            // A notification (method, no id) or a response (id, no method): nothing to answer.
            return None;
        };
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(rpc_error(&id, INVALID_REQUEST, "jsonrpc must be \"2.0\""));
        }
        let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
        let stateless = match meta_protocol_version(&params) {
            None => false,
            Some(v) if v == STATELESS_PROTOCOL_VERSION => true,
            Some(v) => {
                self.log_call(ctx, method, None, false);
                return Some(unsupported_version(&id, v));
            }
        };
        let tasks = client_declares_tasks(&params);
        let reply = match method {
            "initialize" if !stateless => {
                let v = params.get("protocolVersion").and_then(Value::as_str);
                rpc_result(&id, Self::initialize_result(v))
            }
            "ping" if !stateless => rpc_result(&id, json!({})),
            "server/discover" => rpc_result(&id, Self::discover_result()),
            "tools/list" => rpc_result(
                &id,
                json!({"tools": tools::all_tools()
                    .filter(|t| !ctx.read_only || t.kind == ToolKind::Read)
                    .map(tools::tool_json)
                    .collect::<Vec<_>>()}),
            ),
            "tools/call" => {
                // tools_call logs the call itself (with the tool name).
                let r = self.tools_call(ctx, &id, &params, tasks);
                return Some(if stateless { decorate(r, method) } else { r });
            }
            "tasks/get" | "tasks/update" | "tasks/cancel" => {
                self.tasks_method(ctx, &id, method, &params)
            }
            "resources/list" => rpc_result(&id, json!({"resources": resources_list()})),
            "resources/templates/list" => {
                rpc_result(&id, json!({"resourceTemplates": resource_templates()}))
            }
            "resources/read" => match self.resources_read(&params) {
                Ok(v) => rpc_result(&id, v),
                Err((code, m)) => {
                    let code = if stateless && code == RESOURCE_NOT_FOUND {
                        INVALID_PARAMS
                    } else {
                        code
                    };
                    rpc_error(&id, code, &m)
                }
            },
            _ => rpc_error(
                &id,
                METHOD_NOT_FOUND,
                &format!("method not found: {method}"),
            ),
        };
        let ok = reply.get("error").is_none();
        self.log_call(ctx, method, None, ok);
        Some(if stateless {
            decorate(reply, method)
        } else {
            reply
        })
    }

    /// The server's capabilities (both eras). The Tasks extension is advertised; it is used only
    /// for a request whose client declares it.
    fn capabilities() -> Value {
        json!({
            "tools": {"listChanged": false},
            "resources": {"subscribe": false, "listChanged": false},
            "extensions": {TASKS_EXTENSION: {}},
        })
    }

    fn server_info() -> Value {
        json!({
            "name": "citrate-node",
            "title": "Citrate node",
            "version": env!("CARGO_PKG_VERSION"),
        })
    }

    /// The `server/discover` result (MCP 2026-07-28).
    pub fn discover_result() -> Value {
        json!({
            "resultType": "complete",
            "supportedVersions": all_protocol_versions(),
            "capabilities": Self::capabilities(),
            "instructions": INSTRUCTIONS,
            "ttlMs": STATIC_TTL_MS,
            "cacheScope": "public",
            "_meta": {META_SERVER_INFO: Self::server_info()},
        })
    }

    /// `tasks/get`, `tasks/update`, `tasks/cancel`. A task is one of this client's own write
    /// requests; another token's request is indistinguishable from an unknown id.
    fn tasks_method(&self, ctx: &CallerCtx, id: &Value, method: &str, params: &Value) -> Value {
        let Some(task_id) = params.get("taskId").and_then(Value::as_str) else {
            return rpc_error(id, INVALID_PARAMS, &format!("{method} needs a taskId"));
        };
        let unknown = || rpc_error(id, INVALID_PARAMS, &format!("unknown task {task_id}"));
        match method {
            "tasks/get" => {
                let (found, close) = self.inbox.status_for(task_id, &ctx.token_id, self.now());
                self.close(close);
                match found {
                    Some(r) => {
                        let mut v = self.task_view_confirmed(&r);
                        v["resultType"] = json!("complete");
                        rpc_result(id, v)
                    }
                    None => unknown(),
                }
            }
            "tasks/cancel" => {
                let (found, close) =
                    self.inbox
                        .cancel_by_client(task_id, &ctx.token_id, self.now());
                self.close(close);
                match found {
                    Some(_) => rpc_result(id, json!({"resultType": "complete"})),
                    None => unknown(),
                }
            }
            _ => {
                // tasks/update: this server never asks a client for input (the member decides in
                // the app), so there is never an outstanding inputRequest; responses are ignored.
                let (found, close) = self.inbox.status_for(task_id, &ctx.token_id, self.now());
                self.close(close);
                match found {
                    Some(_) => rpc_result(id, json!({"resultType": "complete"})),
                    None => unknown(),
                }
            }
        }
    }

    fn tools_call(&self, ctx: &CallerCtx, id: &Value, params: &Value, tasks: bool) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            self.log_call(ctx, "tools/call", None, false);
            return rpc_error(id, INVALID_PARAMS, "tools/call needs a tool name");
        };
        let Some(def) = tools::tool(name) else {
            self.log_call(ctx, "tools/call", Some(name), false);
            return rpc_error(id, INVALID_PARAMS, &format!("unknown tool: {name}"));
        };
        if ctx.read_only && def.kind != ToolKind::Read {
            self.log_call(ctx, "tools/call", Some(name), false);
            return rpc_result(
                id,
                tool_err(&format!(
                    "{name} is a write tool, and this connect token is limited to the read tools."
                )),
            );
        }
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let outcome =
            tools::reject_unknown(&args, &(def.input_schema)()).and_then(|_| match def.kind {
                ToolKind::Read => self.read_tool(ctx, name, &args).map(Reply::Value),
                ToolKind::Signature => self.signature_tool(ctx, name, &args).map(Reply::Request),
                ToolKind::Action => self.action_tool(ctx, name, &args).map(Reply::Request),
                ToolKind::Budgeted => self.budgeted_tool(ctx, name, &args).map(Reply::Value),
            });
        self.log_call(ctx, "tools/call", Some(name), outcome.is_ok());
        let result = match outcome {
            Ok(Reply::Value(v)) => tool_ok(v),
            // The client declared MCP Tasks: hand back the request as a task handle.
            Ok(Reply::Request(req)) if tasks => {
                let mut t = task_view(&req);
                t["resultType"] = json!("task");
                t
            }
            Ok(Reply::Request(req)) => tool_ok(Self::pending_reply(&req)),
            Err(m) => tool_err(&m),
        };
        rpc_result(id, result)
    }

    fn rpc(&self, method: &str, params: Value) -> Result<RpcRead, String> {
        if !READ_RPC_METHODS.contains(&method) {
            return Err(format!("{method} is not a read-only method"));
        }
        self.backend.rpc_read(method, params)
    }

    fn chain_head(&self) -> Result<Value, String> {
        let head = self.rpc("eth_blockNumber", json!([]))?;
        let chain = self.rpc("eth_chainId", json!([]))?;
        Ok(json!({
            "chainId": hex_u64(&chain.value),
            "height": hex_u64(&head.value),
            "source": head.source,
        }))
    }

    fn balance_of(&self, address: &str) -> Result<Value, String> {
        let r = self.rpc("eth_getBalance", json!([address, "latest"]))?;
        let wei = hex_u128_dec(&r.value).ok_or("the node returned a malformed balance")?;
        Ok(json!({"address": address, "balanceWei": wei, "source": r.source}))
    }

    fn call_object(args: &Value) -> Result<Value, String> {
        let to = tools::parse_address(tools::arg_str(args, "to")?)?;
        let mut call = json!({"to": to});
        if let Some(d) = tools::arg_opt_str(args, "data")? {
            call["data"] = json!(tools::parse_data(d)?);
        }
        if let Some(f) = tools::arg_opt_str(args, "from")? {
            call["from"] = json!(tools::parse_address(f)?);
        }
        if let Some(v) = tools::arg_opt_str(args, "value_wei")? {
            call["value"] = json!(format!("0x{:x}", tools::parse_wei(v)?));
        }
        Ok(call)
    }

    fn read_tool(&self, ctx: &CallerCtx, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "node_status" => self.backend.node_status(),
            "chain_head" => self.chain_head(),
            "get_balance" => {
                let a = tools::parse_address(tools::arg_str(args, "address")?)?;
                self.balance_of(&a)
            }
            "chain_call" => {
                let call = Self::call_object(args)?;
                let r = self.rpc("eth_call", json!([call, "latest"]))?;
                Ok(json!({"result": r.value, "source": r.source}))
            }
            "estimate_gas" => {
                let call = Self::call_object(args)?;
                let r = self.rpc("eth_estimateGas", json!([call]))?;
                Ok(json!({"gas": hex_u64(&r.value), "source": r.source}))
            }
            "get_logs" => {
                let address = tools::parse_address(tools::arg_str(args, "address")?)?;
                let from = args
                    .get("from_block")
                    .and_then(Value::as_u64)
                    .ok_or("from_block must be a block number")?;
                let to = args
                    .get("to_block")
                    .and_then(Value::as_u64)
                    .ok_or("to_block must be a block number")?;
                if to < from {
                    return Err("to_block is before from_block".to_string());
                }
                if to - from >= tools::MAX_LOG_RANGE {
                    return Err(format!(
                        "block range is limited to {} blocks",
                        tools::MAX_LOG_RANGE
                    ));
                }
                let mut topics = Vec::new();
                if let Some(t) = args.get("topics") {
                    let arr = t.as_array().ok_or("topics must be an array")?;
                    if arr.len() > 4 {
                        return Err("at most 4 topics".to_string());
                    }
                    for x in arr {
                        topics.push(match x {
                            Value::Null => Value::Null,
                            Value::String(s) => json!(tools::parse_topic(s)?),
                            _ => return Err("each topic is a hex string or null".to_string()),
                        });
                    }
                }
                let filter = json!({
                    "address": address,
                    "topics": topics,
                    "fromBlock": format!("0x{from:x}"),
                    "toBlock": format!("0x{to:x}"),
                });
                let r = self.rpc("eth_getLogs", json!([filter]))?;
                Ok(json!({"logs": r.value, "source": r.source}))
            }
            "precompile_table" => Ok(tools::precompile_table()),
            "dag_stats" => {
                let r = self.rpc("citrate_getDagStats", json!([]))?;
                dag_stats_view(&r)
            }
            "devices_list" => self.backend.devices(),
            "pins_list" => self.backend.pins(),
            "precompile_call" => {
                let address = tools::parse_address(tools::arg_str(args, "address")?)?;
                let p = tools::precompile_at(&address)
                    .ok_or("that address is not in the precompile table (see precompile_table)")?;
                let data = tools::parse_data(tools::arg_str(args, "data")?)?;
                let r = self.rpc("eth_call", json!([{"to": address, "data": data}, "latest"]))?;
                Ok(json!({"precompile": p.name, "result": r.value, "source": r.source}))
            }
            "ed25519_verify" => {
                let data = tools::ed25519_verify_input(
                    tools::arg_str(args, "public_key")?,
                    tools::arg_str(args, "signature")?,
                    tools::arg_str(args, "message")?,
                )?;
                let address = tools::precompile_address(0x0120);
                let r = self.rpc("eth_call", json!([{"to": address, "data": data}, "latest"]))?;
                // The precompile answers a 32-byte word: ...01 valid, all zero invalid.
                let word = r.value.as_str().unwrap_or("");
                let valid = match word.strip_prefix("0x") {
                    Some(h) if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) => {
                        h[..63].chars().all(|c| c == '0') && h.ends_with('1')
                    }
                    _ => {
                        return Err(format!(
                            "the precompile returned {word:?}, not a 32-byte word (is ED25519_VERIFY active on this chain?)"
                        ))
                    }
                };
                Ok(
                    json!({"precompile": "ED25519_VERIFY", "address": address, "valid": valid, "source": r.source}),
                )
            }
            "wallet_info" => {
                let a = self.backend.wallet_address()?;
                let mut v = self.balance_of(&a)?;
                v["note"] =
                    json!("Public information only. This server never returns key material.");
                Ok(v)
            }
            "address_book" => address_book(),
            "memory_search" => {
                let q = tools::arg_str(args, "query")?.trim();
                if q.is_empty() || q.chars().count() > tools::MAX_QUERY_CHARS {
                    return Err(format!(
                        "query must be 1 to {} characters",
                        tools::MAX_QUERY_CHARS
                    ));
                }
                let tenant = tools::arg_opt_str(args, "tenant")?.unwrap_or("citrate-docs");
                if !tools::MEMORY_TENANTS.contains(&tenant) {
                    return Err(format!(
                        "tenant must be one of {:?}; personal memory is not available over MCP",
                        tools::MEMORY_TENANTS
                    ));
                }
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10);
                let limit = limit.clamp(1, 25) as usize;
                self.backend.memory_search(tenant, q, limit)
            }
            "groups_list" => self.backend.groups(),
            "cluster_status" => {
                let g = tools::parse_group(tools::arg_str(args, "group")?)?;
                self.backend.cluster_status(&g)
            }
            "cluster_peers" => {
                let g = tools::parse_group(tools::arg_str(args, "group")?)?;
                self.backend.cluster_peers(&g)
            }
            "invites_list" => {
                let g = tools::parse_group(tools::arg_str(args, "group")?)?;
                self.backend.invites(&g)
            }
            "request_status" => {
                let id = tools::arg_str(args, "id")?;
                let (found, close) = self.inbox.status_for(id, &ctx.token_id, self.now());
                self.close(close);
                let r = found.ok_or_else(|| format!("no request {id} for this client"))?;
                let mut v = request_view(&r);
                if let Some(c) = self.confirmation(&r) {
                    v["confirmation"] = c.view();
                }
                Ok(v)
            }
            other => crate::node_mcp_hermes::read_tool(
                &crate::node_mcp_hermes::ViaBackend(self.backend.as_ref()),
                other,
                args,
            )
            .unwrap_or_else(|| Err(format!("unknown read tool {other}"))),
        }
    }

    /// HUP-S6.5: tools that run inside a member-granted budget. Core decides the recipient, the
    /// need and the timing; the caller only names the deploy.
    fn budgeted_tool(&self, ctx: &CallerCtx, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "faucet_request" => {
                let h = tools::arg_str(args, "initcode_hash")?;
                let hex_ok = h
                    .strip_prefix("0x")
                    .is_some_and(|x| x.len() == 64 && x.chars().all(|c| c.is_ascii_hexdigit()));
                if !hex_ok {
                    return Err("initcode_hash must be a 0x-prefixed 32-byte hash".to_string());
                }
                self.backend
                    .faucet_request(&ctx.origin(), &h.to_ascii_lowercase())
            }
            other => Err(format!("unknown budgeted tool {other}")),
        }
    }

    /// Deploy-and-confirm (and tx-and-confirm): for an APPROVED signature request whose result
    /// carries a transaction hash, what the chain says about it now (`eth_getTransactionReceipt`,
    /// a read). `None` for every other request.
    fn confirmation(&self, r: &McpRequest) -> Option<Confirmation> {
        let (
            RequestKind::Signature { .. },
            crate::node_mcp_approvals::RequestState::Approved { result },
        ) = (&r.kind, &r.state)
        else {
            return None;
        };
        let hash = result
            .get("txHash")
            .and_then(Value::as_str)
            .and_then(|h| tools::parse_topic(h).ok())?;
        Some(match self.rpc("eth_getTransactionReceipt", json!([hash])) {
            Err(e) => Confirmation::Unreadable { hash, error: e },
            Ok(read) if read.value.is_null() => Confirmation::Waiting { hash },
            Ok(read) => match receipt_view(&read.value, &read.source) {
                Some(rc) if rc["status"] == json!("reverted") => Confirmation::Reverted(rc),
                Some(rc) => Confirmation::Included(rc),
                None => Confirmation::Unreadable {
                    hash,
                    error: "the node returned a malformed receipt".to_string(),
                },
            },
        })
    }

    /// [`task_view`], with an approved transaction confirmed against the chain: the task stays
    /// `working` until the transaction is in a block, then completes with its receipt (or with an
    /// `isError` result when it reverted).
    fn task_view_confirmed(&self, r: &McpRequest) -> Value {
        let mut v = task_view(r);
        let Some(c) = self.confirmation(r) else {
            return v;
        };
        let approved = match &r.state {
            crate::node_mcp_approvals::RequestState::Approved { result } => result.clone(),
            _ => return v,
        };
        match c {
            Confirmation::Waiting { hash } => {
                v["status"] = json!("working");
                v["statusMessage"] = json!(format!(
                    "Approved by the member and sent ({hash}); waiting for the chain to include it."
                ));
                if let Some(o) = v.as_object_mut() {
                    o.remove("result");
                }
            }
            Confirmation::Unreadable { hash, error } => {
                v["status"] = json!("working");
                v["statusMessage"] = json!(format!(
                    "Approved by the member and sent ({hash}); its receipt could not be read yet ({error})."
                ));
                if let Some(o) = v.as_object_mut() {
                    o.remove("result");
                }
            }
            Confirmation::Included(rc) => {
                let block = rc.get("blockNumber").cloned().unwrap_or(Value::Null);
                v["statusMessage"] = json!(format!(
                    "Approved by the member, sent, and included in block {block}."
                ));
                let mut out = approved;
                out["receipt"] = rc;
                v["result"] = with_complete(tool_ok(out));
            }
            Confirmation::Reverted(rc) => {
                let block = rc.get("blockNumber").cloned().unwrap_or(Value::Null);
                v["statusMessage"] = json!(format!(
                    "Approved by the member and sent, but it reverted in block {block}."
                ));
                v["result"] = with_complete(tool_err(&format!(
                    "The transaction was included in block {block} but reverted, so nothing changed on chain."
                )));
            }
        }
        v
    }

    fn pending_reply(req: &McpRequest) -> Value {
        json!({
            "requestId": req.id,
            "state": "pending",
            "summary": req.summary,
            "next": "The member must approve this in Citrate Core (Settings, API endpoints & keys, Node MCP server). Nothing happens until they do. Poll request_status with this requestId.",
        })
    }

    fn signature_tool(
        &self,
        ctx: &CallerCtx,
        name: &str,
        args: &Value,
    ) -> Result<McpRequest, String> {
        let value = match tools::arg_opt_str(args, "value_wei")? {
            Some(v) => tools::parse_wei(v)?,
            None => 0,
        };
        // Parse everything before anything is opened.
        enum Proposal {
            Tx {
                to: String,
                data: String,
            },
            Deploy {
                bytecode: String,
                args: String,
                gas: Option<u64>,
            },
        }
        let proposal = match name {
            "tx_propose" => Proposal::Tx {
                to: tools::parse_address(tools::arg_str(args, "to")?)?,
                data: match tools::arg_opt_str(args, "data")? {
                    Some(d) => tools::parse_data(d)?,
                    None => "0x".to_string(),
                },
            },
            "deploy_propose" => {
                let bytecode = tools::parse_data(tools::arg_str(args, "bytecode")?)?;
                if bytecode.len() <= 2 {
                    return Err("bytecode is empty".to_string());
                }
                let cargs = match tools::arg_opt_str(args, "constructor_args")? {
                    Some(a) => tools::parse_data(a)?,
                    None => "0x".to_string(),
                };
                if bytecode.len() - 2 + cargs.len() - 2 > tools::MAX_DATA_HEX {
                    return Err(
                        "bytecode and constructor arguments are larger than 64 KiB".to_string()
                    );
                }
                let gas = match args.get("gas") {
                    None | Some(Value::Null) => None,
                    Some(g) => {
                        let g = g.as_u64().ok_or("gas must be a whole number")?;
                        if !(21_000..=tools::MAX_DEPLOY_GAS).contains(&g) {
                            return Err(format!(
                                "gas must be between 21,000 and {}",
                                tools::MAX_DEPLOY_GAS
                            ));
                        }
                        Some(g)
                    }
                };
                Proposal::Deploy {
                    bytecode,
                    args: cargs,
                    gas,
                }
            }
            other => return Err(format!("unknown signature tool {other}")),
        };
        // Check for room BEFORE opening a ceremony, so a full inbox never orphans one.
        let (room, close) = self.inbox.has_room(self.now());
        self.close(close);
        if !room {
            return Err("Too many requests are waiting for the member. Ask them to review the requests in Citrate Core first.".to_string());
        }
        let origin = ctx.origin();
        let (proposed, summary) = match proposal {
            Proposal::Tx { to, data } => {
                let p = self
                    .backend
                    .propose_transaction(&origin, &to, value, &data)?;
                let s = p
                    .ceremony
                    .get("decoded")
                    .and_then(|d| d.get("action"))
                    .and_then(Value::as_str)
                    .map(|a| format!("Sign and send a transaction: {a}"))
                    .unwrap_or_else(|| "Sign and send a transaction".to_string());
                (p, s)
            }
            Proposal::Deploy {
                bytecode,
                args,
                gas,
            } => {
                let p = self
                    .backend
                    .propose_deploy(&origin, &bytecode, &args, value, gas)?;
                let hash = p
                    .ceremony
                    .get("gate")
                    .and_then(|g| g.get("initcodeHash"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string();
                let s = format!(
                    "Sign and send a contract deploy (deploy gate READY for init code {hash})"
                );
                (p, s)
            }
        };
        let kind = RequestKind::Signature {
            ceremony_id: proposed.ceremony_id.clone(),
            ceremony: proposed.ceremony,
        };
        match self
            .inbox
            .submit(&ctx.token_id, &origin, kind, summary, self.now())
        {
            Ok((req, close)) => {
                self.close(close);
                Ok(req)
            }
            Err(e) => {
                // Lost a race for the last slot: close the ceremony we just opened.
                self.backend.close_ceremony(&proposed.ceremony_id);
                Err(e)
            }
        }
    }

    fn action_tool(&self, ctx: &CallerCtx, name: &str, args: &Value) -> Result<McpRequest, String> {
        let action = match name {
            "cluster_join" => McpAction::ClusterJoin {
                group: tools::parse_group(tools::arg_str(args, "group")?)?,
            },
            "cluster_share" => McpAction::ClusterShare {
                group: tools::parse_group(tools::arg_str(args, "group")?)?,
                cid: tools::parse_cid(tools::arg_str(args, "cid")?)?,
            },
            "invite_create" => McpAction::InviteCreate {
                group: tools::parse_group(tools::arg_str(args, "group")?)?,
                for_handle: tools::parse_label(tools::arg_str(args, "for_handle")?, "for_handle")?,
            },
            "invite_revoke" => McpAction::InviteRevoke {
                group: tools::parse_group(tools::arg_str(args, "group")?)?,
                invite_id: {
                    let i = tools::arg_str(args, "invite_id")?;
                    if i.len() != 16 || !i.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err("invite_id must be an id from invites_list".to_string());
                    }
                    i.to_ascii_lowercase()
                },
            },
            "pin_add" => McpAction::PinAdd {
                cid: tools::parse_cid(tools::arg_str(args, "cid")?)?,
            },
            "anchor_propose" => {
                // Refuse at once rather than queue a request that can only fail.
                self.backend.anchor_ready()?;
                McpAction::AnchorPropose
            }
            other => match crate::node_mcp_hermes::action_for(other, args) {
                Some(a) => a?,
                None => return Err(format!("unknown action tool {other}")),
            },
        };
        let summary = action.summary();
        let (req, close) = self.inbox.submit(
            &ctx.token_id,
            &ctx.origin(),
            RequestKind::Action { action },
            summary,
            self.now(),
        )?;
        self.close(close);
        Ok(req)
    }

    fn resources_read(&self, params: &Value) -> Result<Value, (i64, String)> {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "resources/read needs a uri".to_string()))?;
        let value = match uri {
            "citrate://node/status" => self.backend.node_status(),
            "citrate://chain/head" => self.chain_head(),
            "citrate://wallet" => self
                .backend
                .wallet_address()
                .and_then(|a| self.balance_of(&a)),
            "citrate://addresses" => address_book(),
            "citrate://precompiles" => Ok(tools::precompile_table()),
            _ => match (parse_memory_uri(uri), parse_abi_uri(uri)) {
                (Some((tenant, q)), _) => self.backend.memory_search(&tenant, &q, 10),
                (None, Some(address)) => self.backend.contract_abi(&address),
                (None, None) => {
                    return Err((RESOURCE_NOT_FOUND, format!("resource not found: {uri}")))
                }
            },
        }
        .map_err(|m| (INVALID_PARAMS, m))?;
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Ok(json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": text}]}))
    }
}

/// What the chain says about an approved transaction.
#[derive(Debug, Clone, PartialEq)]
pub enum Confirmation {
    /// Sent; no receipt yet.
    Waiting { hash: String },
    /// In a block and succeeded (the receipt view).
    Included(Value),
    /// In a block and reverted (the receipt view).
    Reverted(Value),
    /// The receipt could not be read (node and public RPC both failed, or a malformed answer).
    Unreadable { hash: String, error: String },
}

impl Confirmation {
    /// The `confirmation` field `request_status` adds to an approved transaction.
    pub fn view(&self) -> Value {
        match self {
            Confirmation::Waiting { hash } => json!({"state": "waiting", "txHash": hash}),
            Confirmation::Included(rc) => json!({"state": "included", "receipt": rc}),
            Confirmation::Reverted(rc) => json!({"state": "reverted", "receipt": rc}),
            Confirmation::Unreadable { hash, error } => {
                json!({"state": "unknown", "txHash": hash, "error": error})
            }
        }
    }
}

/// The parts of a transaction receipt a client needs: status, block, created contract (a
/// deploy), gas used. `None` when the receipt is malformed.
pub fn receipt_view(rc: &Value, source: &str) -> Option<Value> {
    let hash = rc
        .get("transactionHash")
        .and_then(Value::as_str)
        .and_then(|h| tools::parse_topic(h).ok())?;
    let status = match rc.get("status").and_then(Value::as_str) {
        Some("0x1") => "success",
        Some("0x0") => "reverted",
        _ => return None,
    };
    let block = rc.get("blockNumber").and_then(hex_u64)?;
    let contract = rc
        .get("contractAddress")
        .and_then(Value::as_str)
        .and_then(|a| tools::parse_address(a).ok());
    Some(json!({
        "transactionHash": hash,
        "status": status,
        "blockNumber": block,
        "contractAddress": contract,
        "gasUsed": rc.get("gasUsed").and_then(hex_u64),
        "source": source,
    }))
}

/// What a tool call produced: an immediate value (read tools) or a queued request (write tools).
enum Reply {
    Value(Value),
    Request(McpRequest),
}

/// The server's instructions text (both eras).
pub const INSTRUCTIONS: &str = "Tools and resources for this member's Citrate node (chain 40204). Read tools answer at once. Write tools (tx_propose, deploy_propose, pin_add, anchor_propose, cluster_join, cluster_share, invite_create, invite_revoke) only create a request: the member must approve it in Citrate Core, and nothing is signed or changed until they do. Poll request_status with the returned id, or, if you declared the io.modelcontextprotocol/tasks extension, poll tasks/get with the returned taskId.";

/// The protocol version a stateless-era request names in `params._meta`, if any.
pub fn meta_protocol_version(params: &Value) -> Option<&str> {
    params
        .get("_meta")
        .and_then(|m| m.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str)
}

/// The client name a stateless-era request names in `params._meta` (display only, never trusted).
pub fn meta_client_name(params: &Value) -> Option<String> {
    params
        .get("_meta")
        .and_then(|m| m.get(META_CLIENT_INFO))
        .and_then(|c| c.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Whether this request's client declared the MCP Tasks extension in its capabilities. The
/// server never returns a task to a client that did not (SEP-2663).
pub fn client_declares_tasks(params: &Value) -> bool {
    params
        .get("_meta")
        .and_then(|m| m.get(META_CLIENT_CAPABILITIES))
        .and_then(|c| c.get("extensions"))
        .and_then(|e| e.get(TASKS_EXTENSION))
        .is_some_and(Value::is_object)
}

/// The `UnsupportedProtocolVersion` error (MCP 2026-07-28).
pub fn unsupported_version(id: &Value, requested: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {
        "code": UNSUPPORTED_PROTOCOL_VERSION,
        "message": "Unsupported protocol version",
        "data": {"supported": all_protocol_versions(), "requested": requested},
    }})
}

/// Stateless-era result decoration: `resultType` (kept when already set, e.g. `"task"`), the
/// server's identity in `_meta`, and caching hints on list/read results.
fn decorate(mut reply: Value, method: &str) -> Value {
    let Some(result) = reply.get_mut("result").and_then(Value::as_object_mut) else {
        return reply;
    };
    result
        .entry("resultType")
        .or_insert_with(|| json!("complete"));
    let meta = result.entry("_meta").or_insert_with(|| json!({}));
    if let Some(m) = meta.as_object_mut() {
        m.insert(META_SERVER_INFO.to_string(), McpCore::server_info());
    }
    let cache = match method {
        "tools/list" | "resources/list" | "resources/templates/list" => {
            Some((STATIC_TTL_MS, "private"))
        }
        // Live node data: always re-read.
        "resources/read" => Some((0, "private")),
        _ => None,
    };
    if let Some((ttl, scope)) = cache {
        result.insert("ttlMs".into(), json!(ttl));
        result.insert("cacheScope".into(), json!(scope));
    }
    reply
}

/// An ISO 8601 UTC timestamp for a Unix-ms time.
fn iso(ms: u64) -> String {
    crate::google_workspace::rfc3339(ms / 1000)
}

/// One approval request as an MCP task (SEP-2663 `DetailedTask`). The member's decision maps to:
/// pending/running -> `working`; approved -> `completed` with the tool result; failed, rejected or
/// expired -> `completed` with an `isError` tool result (these are tool-level outcomes, not
/// JSON-RPC errors); withdrawn by the client -> `cancelled`.
pub fn task_view(r: &McpRequest) -> Value {
    use crate::node_mcp_approvals::{RequestState, CANCELLED_BY_CLIENT};
    let (status, message, extra): (&str, String, Option<(&str, Value)>) = match &r.state {
        RequestState::Pending => (
            "working",
            "Waiting for the member to approve or reject this in Citrate Core.".to_string(),
            None,
        ),
        RequestState::Running => (
            "working",
            "The member approved this; it is running now.".to_string(),
            None,
        ),
        RequestState::Approved { result } => (
            "completed",
            "Approved by the member and done.".to_string(),
            Some(("result", with_complete(tool_ok(result.clone())))),
        ),
        RequestState::Failed { error } => (
            "completed",
            "Approved by the member, but it failed.".to_string(),
            Some((
                "result",
                with_complete(tool_err(&format!("It failed after approval: {error}"))),
            )),
        ),
        RequestState::Rejected { reason } if reason == CANCELLED_BY_CLIENT => {
            ("cancelled", "Withdrawn by the client.".to_string(), None)
        }
        RequestState::Rejected { reason } => (
            "completed",
            format!("Not done: {reason}."),
            Some((
                "result",
                with_complete(tool_err(&format!(
                    "The member did not approve this ({reason})."
                ))),
            )),
        ),
        RequestState::Expired => (
            "completed",
            "Expired before the member decided.".to_string(),
            Some((
                "result",
                with_complete(tool_err("The request expired before the member decided.")),
            )),
        ),
    };
    let mut v = json!({
        "taskId": r.id,
        "status": status,
        "statusMessage": message,
        "createdAt": iso(r.created_ms),
        "lastUpdatedAt": iso(r.decided_ms.unwrap_or(r.created_ms)),
        "ttlMs": TASK_TTL_MS,
        "pollIntervalMs": TASK_POLL_MS,
    });
    if let Some((k, val)) = extra {
        v[k] = val;
    }
    v
}

fn with_complete(mut v: Value) -> Value {
    v["resultType"] = json!("complete");
    v
}

/// The `dag_stats` answer: what the node reports exactly (tips, height, highest blue score,
/// GhostDAG parameters). The node's blue/red block counts are a fixed-ratio estimate, not a
/// count, so they are left out rather than passed on as fact.
pub fn dag_stats_view(r: &RpcRead) -> Result<Value, String> {
    let v = &r.value;
    let height = v
        .get("height")
        .and_then(Value::as_u64)
        .ok_or("the node returned malformed DAG statistics")?;
    let tips: Vec<Value> = v
        .get("currentTips")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|t| tools::parse_topic(t).is_ok())
                .take(32)
                .map(|t| json!(t.to_ascii_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "height": height,
        "tipsCount": v.get("tipsCount").and_then(Value::as_u64),
        "tips": tips,
        "maxBlueScore": v.get("maxBlueScore").and_then(Value::as_u64),
        "ghostdagParams": v.get("ghostdagParams").filter(|p| p.is_object()).cloned(),
        "source": r.source,
        "note": "Blue and red block counts are not reported: the node estimates them from the height instead of counting them.",
    }))
}

/// The static resources.
pub fn resources_list() -> Value {
    json!([
        {"uri": "citrate://node/status", "name": "node-status", "title": "Node status", "mimeType": "application/json",
         "description": "State, peers, head height and sync percentage of this node."},
        {"uri": "citrate://chain/head", "name": "chain-head", "title": "Chain head", "mimeType": "application/json",
         "description": "Chain id and head block number, and whether they came from this node or the public RPC."},
        {"uri": "citrate://wallet", "name": "wallet", "title": "Wallet (public)", "mimeType": "application/json",
         "description": "This member's public wallet address and SALT balance."},
        {"uri": "citrate://addresses", "name": "addresses", "title": "Address book", "mimeType": "application/json",
         "description": "Deployed 40204 contract addresses this build ships with."},
        {"uri": "citrate://precompiles", "name": "precompiles", "title": "Precompile table", "mimeType": "application/json",
         "description": "Citrate precompiles callable read-only with precompile_call."},
    ])
}

/// The resource templates (memory search).
pub fn resource_templates() -> Value {
    json!([
        {"uriTemplate": "citrate://memory/{tenant}/search?q={query}", "name": "memory-search", "title": "Memory search",
         "mimeType": "application/json",
         "description": "Search a shared knowledge graph: tenant is citrate-docs or chain-state."},
        {"uriTemplate": "citrate://contract/{address}/abi", "name": "contract-abi", "title": "Contract ABI (ABI registry)",
         "mimeType": "application/json",
         "description": "A 40204 contract's ABI from CitrateScan's verified sources, with the match status (verified, partial match, or not verified). No ABI is guessed."},
    ])
}

/// Parse `citrate://contract/<0x address>/abi` into the lowercased address.
pub fn parse_abi_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("citrate://contract/")?;
    let address = rest.strip_suffix("/abi")?;
    tools::parse_address(address).ok()
}

/// Parse `citrate://memory/<tenant>/search?q=<urlencoded query>`; only shared tenants.
pub fn parse_memory_uri(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix("citrate://memory/")?;
    let (tenant, query) = rest.split_once("/search?q=")?;
    if !tools::MEMORY_TENANTS.contains(&tenant) {
        return None;
    }
    let q = url::form_urlencoded::parse(format!("q={query}").as_bytes())
        .find(|(k, _)| k == "q")
        .map(|(_, v)| v.into_owned())?;
    let q = q.trim().to_string();
    if q.is_empty() || q.chars().count() > tools::MAX_QUERY_CHARS {
        return None;
    }
    Some((tenant.to_string(), q))
}

/// The embedded 40204 address book (`src-tauri/addresses/40204.json`, generated from the chain).
pub fn address_book() -> Result<Value, String> {
    let book: Value = serde_json::from_str(include_str!("../addresses/40204.json"))
        .map_err(|e| format!("embedded address book unreadable: {e}"))?;
    Ok(json!({
        "chainId": book.get("chainId"),
        "genesisHash": book.get("genesisHash"),
        "addresses": book.get("addresses"),
        "note": "Only the contracts this app reads are listed. A contract missing here is not in this app's address book; it may still be deployed (citrate-chain contracts/addresses/40204.json is the full book).",
    }))
}
