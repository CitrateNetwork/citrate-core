//! HUP-S4.2 — the citrate-node MCP protocol core: JSON-RPC 2.0 dispatch for the MCP base protocol
//! (`initialize`, `ping`, `tools/*`, `resources/*`), independent of the transport.
//!
//! The data comes from a [`NodeBackend`]. Production uses the app-backed backend in
//! `node_mcp_live.rs`; tests use fixtures. The dispatcher owns the rules that must hold whatever
//! the backend is: argument validation, the read-only RPC allowlist, the precompile allowlist, and
//! the routing of every write into the [`ApprovalInbox`] (signatures through the ceremony).

use crate::node_mcp_approvals::{
    ApprovalInbox, CeremonyToClose, McpAction, McpRequest, RequestKind,
};
use crate::node_mcp_tools::{self as tools, ToolKind};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// The protocol versions this server speaks, newest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// JSON-RPC error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// MCP: resource not found.
pub const RESOURCE_NOT_FOUND: i64 = -32002;

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
}

/// Who is calling (resolved by the transport from the connect token + the session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerCtx {
    pub token_id: String,
    pub token_label: String,
    /// `clientInfo.name` from `initialize`, if the client sent one (shown as-is, never trusted).
    pub client_name: Option<String>,
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
            "capabilities": {
                "tools": {"listChanged": false},
                "resources": {"subscribe": false, "listChanged": false},
            },
            "serverInfo": {
                "name": "citrate-node",
                "title": "Citrate node",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": "Tools and resources for this member's Citrate node (chain 40204). Read tools answer at once. Write tools (tx_propose, cluster_join, cluster_share, invite_create, invite_revoke) only create a request: the member must approve it in Citrate Core, and nothing is signed or changed until they do. Poll request_status with the returned id.",
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
        let reply = match method {
            "initialize" => {
                let v = params.get("protocolVersion").and_then(Value::as_str);
                rpc_result(&id, Self::initialize_result(v))
            }
            "ping" => rpc_result(&id, json!({})),
            "tools/list" => rpc_result(
                &id,
                json!({"tools": tools::TOOLS.iter().map(tools::tool_json).collect::<Vec<_>>()}),
            ),
            "tools/call" => return Some(self.tools_call(ctx, &id, &params)),
            "resources/list" => rpc_result(&id, json!({"resources": resources_list()})),
            "resources/templates/list" => {
                rpc_result(&id, json!({"resourceTemplates": resource_templates()}))
            }
            "resources/read" => match self.resources_read(&params) {
                Ok(v) => rpc_result(&id, v),
                Err((code, m)) => rpc_error(&id, code, &m),
            },
            _ => rpc_error(
                &id,
                METHOD_NOT_FOUND,
                &format!("method not found: {method}"),
            ),
        };
        let ok = reply.get("error").is_none();
        self.log_call(ctx, method, None, ok);
        Some(reply)
    }

    fn tools_call(&self, ctx: &CallerCtx, id: &Value, params: &Value) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            self.log_call(ctx, "tools/call", None, false);
            return rpc_error(id, INVALID_PARAMS, "tools/call needs a tool name");
        };
        let Some(def) = tools::tool(name) else {
            self.log_call(ctx, "tools/call", Some(name), false);
            return rpc_error(id, INVALID_PARAMS, &format!("unknown tool: {name}"));
        };
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let outcome =
            tools::reject_unknown(&args, &(def.input_schema)()).and_then(|_| match def.kind {
                ToolKind::Read => self.read_tool(ctx, name, &args),
                ToolKind::Signature => self.signature_tool(ctx, name, &args),
                ToolKind::Action => self.action_tool(ctx, name, &args),
            });
        self.log_call(ctx, "tools/call", Some(name), outcome.is_ok());
        let result = match outcome {
            Ok(v) => tool_ok(v),
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
            "precompile_call" => {
                let address = tools::parse_address(tools::arg_str(args, "address")?)?;
                let p = tools::precompile_at(&address)
                    .ok_or("that address is not in the precompile table (see precompile_table)")?;
                let data = tools::parse_data(tools::arg_str(args, "data")?)?;
                let r = self.rpc("eth_call", json!([{"to": address, "data": data}, "latest"]))?;
                Ok(json!({"precompile": p.name, "result": r.value, "source": r.source}))
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
                found
                    .map(|r| request_view(&r))
                    .ok_or_else(|| format!("no request {id} for this client"))
            }
            other => Err(format!("unknown read tool {other}")),
        }
    }

    fn pending_reply(req: &McpRequest) -> Value {
        json!({
            "requestId": req.id,
            "state": "pending",
            "summary": req.summary,
            "next": "The member must approve this in Citrate Core (Settings, API endpoints & keys, Node MCP server). Nothing happens until they do. Poll request_status with this requestId.",
        })
    }

    fn signature_tool(&self, ctx: &CallerCtx, name: &str, args: &Value) -> Result<Value, String> {
        if name != "tx_propose" {
            return Err(format!("unknown signature tool {name}"));
        }
        let to = tools::parse_address(tools::arg_str(args, "to")?)?;
        let value = match tools::arg_opt_str(args, "value_wei")? {
            Some(v) => tools::parse_wei(v)?,
            None => 0,
        };
        let data = match tools::arg_opt_str(args, "data")? {
            Some(d) => tools::parse_data(d)?,
            None => "0x".to_string(),
        };
        // Check for room BEFORE opening a ceremony, so a full inbox never orphans one.
        let (room, close) = self.inbox.has_room(self.now());
        self.close(close);
        if !room {
            return Err("Too many requests are waiting for the member. Ask them to review the requests in Citrate Core first.".to_string());
        }
        let origin = ctx.origin();
        let proposed = self
            .backend
            .propose_transaction(&origin, &to, value, &data)?;
        let summary = proposed
            .ceremony
            .get("decoded")
            .and_then(|d| d.get("action"))
            .and_then(Value::as_str)
            .map(|a| format!("Sign and send a transaction: {a}"))
            .unwrap_or_else(|| "Sign and send a transaction".to_string());
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
                Ok(Self::pending_reply(&req))
            }
            Err(e) => {
                // Lost a race for the last slot: close the ceremony we just opened.
                self.backend.close_ceremony(&proposed.ceremony_id);
                Err(e)
            }
        }
    }

    fn action_tool(&self, ctx: &CallerCtx, name: &str, args: &Value) -> Result<Value, String> {
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
            other => return Err(format!("unknown action tool {other}")),
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
        Ok(Self::pending_reply(&req))
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
            _ => match parse_memory_uri(uri) {
                Some((tenant, q)) => self.backend.memory_search(&tenant, &q, 10),
                None => return Err((RESOURCE_NOT_FOUND, format!("resource not found: {uri}"))),
            },
        }
        .map_err(|m| (INVALID_PARAMS, m))?;
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
        Ok(json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": text}]}))
    }
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
    ])
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
        "note": "Registries not listed here (for example AgentSBT, BenchmarkRegistry, CapsuleRegistry, AnchorRegistry) are not deployed on 40204 yet.",
    }))
}
