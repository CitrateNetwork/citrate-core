// HUP-S4.2 / HUP-S8.5 — citrate-node MCP server tests.
//
// The protocol, token, inbox and transport rules are tested against a fixture data source (a
// test-only `NodeBackend` with canned chain answers). The transport tests speak real HTTP over
// real loopback sockets with a small JSON-RPC client, and the stdio shim is driven through its
// production ureq transport against the real server.

use super::*;
use crate::node_mcp_approvals::{RequestState, MAX_PENDING, REQUEST_TTL_MS};
use crate::node_mcp_http::{run_stdio_shim, UreqShimTransport};
use crate::node_mcp_protocol::{CallerCtx, ProposedSignature, RpcRead};
use std::io::{Read as _, Write as _};
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Fixture data source
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Fixture {
    rpc_calls: Mutex<Vec<(String, Value)>>,
    proposals: Mutex<Vec<(String, String, u128, String)>>,
    closed: Mutex<Vec<String>>,
    next: AtomicU64,
    /// (origin, bytecode, constructor args, value, gas) for each deploy proposal that opened.
    deploys: Mutex<Vec<DeployCall>>,
    /// `Some(reason)` = anchoring is not ready (the reason is returned verbatim).
    anchor_not_ready: Mutex<Option<String>>,
    /// The `eth_getTransactionReceipt` answer (`None` = no receipt yet, JSON null).
    receipt: Mutex<Option<Value>>,
    /// The ED25519_VERIFY precompile's answer word (`None` = the generic eth_call answer).
    ed25519_word: Mutex<Option<String>>,
    /// Addresses asked for an ABI registry entry.
    abi_lookups: Mutex<Vec<String>>,
}

/// (origin, bytecode, constructor args, value, gas) of one deploy proposal.
type DeployCall = (String, String, String, u128, Option<u64>);

/// The bytecode the fixture's deploy gate treats as NOT READY.
const NOT_READY_CODE: &str = "0xdead";

impl NodeBackend for Fixture {
    fn node_status(&self) -> Result<Value, String> {
        Ok(json!({"state": "running", "peers": 3, "height": 16, "syncPct": 100.0}))
    }
    fn rpc_read(&self, method: &str, params: Value) -> Result<RpcRead, String> {
        self.rpc_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((method.to_string(), params.clone()));
        let value = match method {
            "eth_blockNumber" => json!("0x10"),
            "eth_chainId" => json!("0x9d0c"),
            "eth_getBalance" => json!("0xde0b6b3a7640000"),
            "eth_call" => {
                let to = params[0]["to"].as_str().unwrap_or_default().to_string();
                match self
                    .ed25519_word
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                {
                    Some(w) if to == crate::node_mcp_tools::precompile_address(0x0120) => json!(w),
                    _ => json!("0x01"),
                }
            }
            "eth_getTransactionReceipt" => self
                .receipt
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .unwrap_or(Value::Null),
            "eth_estimateGas" => json!("0x5208"),
            "eth_getLogs" => json!([]),
            "net_peerCount" => json!("0x3"),
            "citrate_getDagStats" => json!({
                "totalBlocks": 92156, "blueBlocks": 87548, "redBlocks": 4608, "tipsCount": 1,
                "maxBlueScore": 92156, "height": 92156,
                "currentTips": ["0xF36B109288455DF4A80CF87AB2209E95183235AB2D3A6E96514EF27EAFAA2B52", "not-a-hash"],
                "ghostdagParams": {"k": 18, "maxParents": 10, "maxBlueScoreDiff": 1000, "pruningWindow": 100000, "finalityDepth": 100}
            }),
            other => return Err(format!("fixture: unexpected {other}")),
        };
        Ok(RpcRead {
            value,
            source: "local-node".into(),
        })
    }
    fn wallet_address(&self) -> Result<String, String> {
        Ok("0x00000000000000000000000000000000000000aa".into())
    }
    fn memory_search(&self, tenant: &str, query: &str, limit: usize) -> Result<Value, String> {
        Ok(json!({"tenant": tenant, "query": query, "limit": limit, "hits": []}))
    }
    fn groups(&self) -> Result<Value, String> {
        Ok(json!({"groups": [{"id": "grp_a", "name": "Alpha"}]}))
    }
    fn cluster_status(&self, group: &str) -> Result<Value, String> {
        Ok(json!({"groupId": group, "online": 1, "total": 2, "sharedFiles": []}))
    }
    fn cluster_peers(&self, _group: &str) -> Result<Value, String> {
        Ok(json!({"peers": [{"address": "0xabc", "online": true}]}))
    }
    fn invites(&self, _group: &str) -> Result<Value, String> {
        Ok(json!({"invites": []}))
    }
    fn propose_transaction(
        &self,
        origin: &str,
        to: &str,
        value_wei: u128,
        data: &str,
    ) -> Result<ProposedSignature, String> {
        self.proposals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((origin.into(), to.into(), value_wei, data.into()));
        let id = (self.next.fetch_add(1, Ordering::SeqCst) + 1).to_string();
        Ok(ProposedSignature {
            ceremony_id: id.clone(),
            ceremony: json!({"id": id, "origin": origin, "decoded": {"action": "Transfer", "cost": "0", "destination": to}, "requiresRawAck": false}),
        })
    }
    fn close_ceremony(&self, ceremony_id: &str) {
        self.closed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(ceremony_id.into());
    }
    fn devices(&self) -> Result<Value, String> {
        Ok(json!({"thisDevice": "0x00000000000000000000000000000000000000d1",
            "links": [{"device": "0x00000000000000000000000000000000000000d1", "label": "Studio Mac", "index": 0, "thisDevice": true}],
            "revoked": []}))
    }
    fn pins(&self) -> Result<Value, String> {
        Ok(json!({"pins": [{"cid": "bafyfixture", "sizeBytes": 12, "bondSalt": "", "pinState": "pinned", "addedAt": 1}]}))
    }
    fn propose_deploy(
        &self,
        origin: &str,
        bytecode: &str,
        constructor_args: &str,
        value_wei: u128,
        gas: Option<u64>,
    ) -> Result<ProposedSignature, String> {
        if bytecode == NOT_READY_CODE {
            return Err("Deploy refused: the D-4 deploy gate is NOT READY for bytecode 0xabc. Failing: medusa: an invariant broke.".into());
        }
        self.deploys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((origin.into(), bytecode.into(), constructor_args.into(), value_wei, gas));
        let id = (self.next.fetch_add(1, Ordering::SeqCst) + 1).to_string();
        Ok(ProposedSignature {
            ceremony_id: id.clone(),
            ceremony: json!({"id": id, "origin": origin, "decoded": {"action": "contract creation", "cost": "0", "destination": "new contract"},
                "requiresRawAck": false, "gate": {"initcodeHash": "0x1234", "verdict": "ready"}}),
        })
    }
    fn contract_abi(&self, address: &str) -> Result<Value, String> {
        self.abi_lookups
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(address.to_string());
        Ok(json!({"address": address, "status": "verified", "contractName": "Fixture",
            "abi": [{"type": "function", "name": "ping", "inputs": [], "outputs": []}],
            "sourceAvailable": true, "from": "CitrateScan verified sources"}))
    }
    fn anchor_ready(&self) -> Result<(), String> {
        match self
            .anchor_not_ready
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }
}

fn ctx(token: &str) -> CallerCtx {
    CallerCtx {
        token_id: token.into(),
        token_label: "Claude Code".into(),
        client_name: Some("claude-code".into()),
        read_only: false,
    }
}

fn core_with(f: Arc<Fixture>) -> McpCore {
    McpCore::new(f, Arc::new(ApprovalInbox::new()))
}

fn call(core: &McpCore, c: &CallerCtx, name: &str, args: Value) -> Value {
    core.dispatch(
        c,
        &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": args}}),
    )
    .expect("a request gets a reply")
}

fn is_tool_error(reply: &Value) -> bool {
    reply["result"]["isError"] == json!(true)
}

// ---------------------------------------------------------------------------
// Connect tokens
// ---------------------------------------------------------------------------

#[test]
fn token_issue_then_verify_accepts_only_the_exact_token() {
    let s = TokenStore::in_memory();
    let t = s.issue("Claude Code", 1).expect("issue");
    assert!(t.connect_token.starts_with("cnmcp_"));
    assert_eq!(t.connect_token.len(), 6 + 64);
    let ok = s.verify(&t.connect_token, 2).expect("verifies");
    assert_eq!(ok.id, t.id);
    assert_eq!(ok.label, "Claude Code");
    // One flipped character, a wrong prefix, a truncation: all refused.
    let mut flipped = t.connect_token.clone().into_bytes();
    let last = flipped.len() - 1;
    flipped[last] = if flipped[last] == b'0' { b'1' } else { b'0' };
    assert!(s
        .verify(&String::from_utf8(flipped).unwrap_or_default(), 2)
        .is_none());
    assert!(s
        .verify(&t.connect_token.replacen("cnmcp_", "cnmcx_", 1), 2)
        .is_none());
    assert!(s.verify(&t.connect_token[..60], 2).is_none());
    assert!(s.verify("", 2).is_none());
}

#[test]
fn token_store_persists_only_the_hash_with_private_permissions() {
    let dir = std::env::temp_dir().join(format!("n4-node-mcp-tok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("tokens.json");
    let s = TokenStore::load(path.clone());
    let t = s.issue("Cursor", 5).expect("issue");
    let on_disk = std::fs::read_to_string(&path).expect("written");
    assert!(
        !on_disk.contains(&t.connect_token),
        "plaintext must never be stored"
    );
    assert!(
        !on_disk.contains(&t.connect_token[6..]),
        "nor the random part"
    );
    let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(
        t.connect_token.as_bytes(),
    ));
    assert!(on_disk.contains(&digest), "the SHA-256 is what persists");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    // A fresh load (next app launch) still accepts the token; revoke persists too.
    let again = TokenStore::load(path.clone());
    assert!(again.verify(&t.connect_token, 6).is_some());
    assert!(again.revoke(&t.id).expect("revoke"));
    assert!(TokenStore::load(path.clone())
        .verify(&t.connect_token, 7)
        .is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_token_file_fails_closed() {
    let dir = std::env::temp_dir().join(format!("n4-node-mcp-bad-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tokens.json");
    std::fs::write(&path, b"{not json").expect("write");
    let s = TokenStore::load(path);
    assert!(s.list().is_empty());
    assert!(s.verify(&format!("cnmcp_{}", "0".repeat(64)), 1).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn token_labels_are_validated_and_count_is_capped() {
    let s = TokenStore::in_memory();
    assert!(s.issue("   ", 1).is_err());
    assert!(s.issue("bad\nlabel", 1).is_err());
    assert!(s.issue(&"x".repeat(49), 1).is_err());
    for i in 0..crate::node_mcp_token::MAX_TOKENS {
        s.issue(&format!("client {i}"), 1).expect("under the cap");
    }
    assert!(s.issue("one too many", 1).is_err());
    let first = s.list()[0].id.clone();
    assert!(s.revoke(&first).expect("revoke"));
    assert!(!s.revoke(&first).expect("second revoke is a no-op"));
    assert!(s.issue("now fits", 1).is_ok());
}

#[test]
fn token_views_carry_no_secret_material() {
    let s = TokenStore::in_memory();
    let t = s.issue("Claude Code", 1).expect("issue");
    s.verify(&t.connect_token, 9);
    let v = serde_json::to_value(s.list()).expect("json");
    let text = v.to_string();
    assert!(!text.contains(&t.connect_token));
    assert!(!text.contains("sha256"));
    assert_eq!(v[0]["lastUsedMs"], json!(9));
}

// ---------------------------------------------------------------------------
// MCP base protocol
// ---------------------------------------------------------------------------

#[test]
fn initialize_negotiates_the_protocol_version() {
    let core = core_with(Arc::new(Fixture::default()));
    let c = ctx("t1");
    let r = core
        .dispatch(&c, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "x", "version": "1"}}}))
        .expect("reply");
    assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(r["result"]["serverInfo"]["name"], "citrate-node");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert!(r["result"]["capabilities"]["resources"].is_object());
    let r = core
        .dispatch(&c, &json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}))
        .expect("reply");
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn notifications_get_no_reply_and_bad_envelopes_are_refused() {
    let core = core_with(Arc::new(Fixture::default()));
    let c = ctx("t1");
    assert!(core
        .dispatch(
            &c,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .is_none());
    let r = core
        .dispatch(&c, &json!({"id": 3, "method": "ping"}))
        .expect("reply");
    assert_eq!(r["error"]["code"], json!(-32600));
    let r = core
        .dispatch(
            &c,
            &json!({"jsonrpc": "2.0", "id": 4, "method": "prompts/list"}),
        )
        .expect("reply");
    assert_eq!(r["error"]["code"], json!(-32601));
    let r = core
        .dispatch(&c, &json!({"jsonrpc": "2.0", "id": 5, "method": "ping"}))
        .expect("reply");
    assert_eq!(r["result"], json!({}));
}

#[test]
fn tools_list_carries_annotations_and_strict_schemas() {
    let core = core_with(Arc::new(Fixture::default()));
    let r = core
        .dispatch(
            &ctx("t"),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )
        .expect("reply");
    let tools = r["result"]["tools"].as_array().expect("array").clone();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for want in [
        "node_status",
        "chain_head",
        "get_balance",
        "chain_call",
        "estimate_gas",
        "get_logs",
        "precompile_table",
        "precompile_call",
        "wallet_info",
        "address_book",
        "memory_search",
        "groups_list",
        "cluster_status",
        "cluster_peers",
        "invites_list",
        "request_status",
        "tx_propose",
        "cluster_join",
        "cluster_share",
        "invite_create",
        "invite_revoke",
        "dag_stats",
        "devices_list",
        "pins_list",
        "deploy_propose",
        "pin_add",
        "anchor_propose",
    ] {
        assert!(names.contains(&want), "missing tool {want}");
    }
    for t in &tools {
        let schema = &t["inputSchema"];
        assert_eq!(schema["type"], "object", "{}", t["name"]);
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{}",
            t["name"]
        );
        let read_only = t["annotations"]["readOnlyHint"] == json!(true);
        let name = t["name"].as_str().unwrap_or_default();
        let is_write = [
            "tx_propose",
            "deploy_propose",
            "pin_add",
            "anchor_propose",
            "cluster_join",
            "cluster_share",
            "invite_create",
            "invite_revoke",
            "faucet_request",
            "hermes_session_send",
            "hermes_session_stop",
        ]
        .contains(&name);
        assert_eq!(
            read_only, !is_write,
            "{name}: readOnlyHint must match the tool kind"
        );
        // HUP-S6.5: the budgeted faucet tool changes state but destroys nothing.
        if is_write && name != "faucet_request" {
            assert_eq!(t["annotations"]["destructiveHint"], json!(true), "{name}");
        }
    }
}

#[test]
fn read_tools_answer_from_the_backend_and_report_their_source() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    let r = call(&core, &c, "node_status", json!({}));
    assert_eq!(r["result"]["structuredContent"]["peers"], json!(3));
    let r = call(&core, &c, "chain_head", json!({}));
    assert_eq!(r["result"]["structuredContent"]["chainId"], json!(40204));
    assert_eq!(r["result"]["structuredContent"]["height"], json!(16));
    assert_eq!(r["result"]["structuredContent"]["source"], "local-node");
    let r = call(
        &core,
        &c,
        "get_balance",
        json!({"address": "0x00000000000000000000000000000000000000AA"}),
    );
    assert_eq!(
        r["result"]["structuredContent"]["balanceWei"],
        "1000000000000000000"
    );
    let r = call(&core, &c, "wallet_info", json!({}));
    assert_eq!(
        r["result"]["structuredContent"]["address"],
        "0x00000000000000000000000000000000000000aa"
    );
    let text = r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(text.contains("balanceWei"));
}

#[test]
fn only_read_only_rpc_methods_ever_reach_the_backend() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    for (name, args) in [
        ("chain_head", json!({})),
        (
            "chain_call",
            json!({"to": "0x0000000000000000000000000000000000000001", "data": "0x1234"}),
        ),
        (
            "estimate_gas",
            json!({"to": "0x0000000000000000000000000000000000000001", "value_wei": "5"}),
        ),
        (
            "get_logs",
            json!({"address": "0x0000000000000000000000000000000000000001", "from_block": 1, "to_block": 2}),
        ),
        (
            "precompile_call",
            json!({"address": "0x0000000000000000000000000000000000000120", "data": "0x"}),
        ),
        (
            "tx_propose",
            json!({"to": "0x0000000000000000000000000000000000000002", "value_wei": "1"}),
        ),
    ] {
        let r = call(&core, &c, name, args);
        assert!(!is_tool_error(&r), "{name}: {r}");
    }
    let methods: Vec<String> = f
        .rpc_calls
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(m, _)| m.clone())
        .collect();
    assert!(!methods.is_empty());
    for m in &methods {
        assert!(
            crate::node_mcp_protocol::READ_RPC_METHODS.contains(&m.as_str()),
            "{m}"
        );
        assert!(!m.contains("send") && !m.contains("sign"), "{m}");
    }
}

#[test]
fn arguments_are_validated_before_the_node_is_touched() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    for (name, args) in [
        ("get_balance", json!({"address": "0x123"})),
        (
            "chain_call",
            json!({"to": "0x0000000000000000000000000000000000000001", "data": "0x123"}),
        ),
        (
            "chain_call",
            json!({"to": "0x0000000000000000000000000000000000000001", "data": "1234"}),
        ),
        (
            "chain_call",
            json!({"to": "0x0000000000000000000000000000000000000001", "extra": 1}),
        ),
        (
            "estimate_gas",
            json!({"to": "0x0000000000000000000000000000000000000001", "value_wei": "-1"}),
        ),
        (
            "get_logs",
            json!({"address": "0x0000000000000000000000000000000000000001", "from_block": 10, "to_block": 5}),
        ),
        (
            "get_logs",
            json!({"address": "0x0000000000000000000000000000000000000001", "from_block": 0, "to_block": 5000}),
        ),
        (
            "get_logs",
            json!({"address": "0x0000000000000000000000000000000000000001", "from_block": 0, "to_block": 1, "topics": ["0x12"]}),
        ),
        (
            "precompile_call",
            json!({"address": "0x0000000000000000000000000000000000000001", "data": "0x"}),
        ),
        (
            "precompile_call",
            json!({"address": "0x0000000000000000000000000000000000000100", "data": "0x"}),
        ),
        ("tx_propose", json!({"to": "nope"})),
        (
            "tx_propose",
            json!({"to": "0x0000000000000000000000000000000000000002", "data": "0xzz"}),
        ),
    ] {
        let r = call(&core, &c, name, args.clone());
        assert!(is_tool_error(&r), "{name} {args} should be refused: {r}");
    }
    assert!(f
        .rpc_calls
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty());
    assert!(f
        .proposals
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty());
    // The widest allowed log range passes.
    let r = call(
        &core,
        &c,
        "get_logs",
        json!({"address": "0x0000000000000000000000000000000000000001", "from_block": 0, "to_block": 4999}),
    );
    assert!(!is_tool_error(&r));
}

#[test]
fn an_unknown_tool_is_a_protocol_error() {
    let core = core_with(Arc::new(Fixture::default()));
    let r = call(&core, &ctx("t"), "eth_sendRawTransaction", json!({}));
    assert_eq!(r["error"]["code"], json!(-32602));
}

#[test]
fn precompile_call_targets_the_table_address() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let r = call(
        &core,
        &ctx("t"),
        "precompile_call",
        json!({"address": "0x0000000000000000000000000000000000000120", "data": "0xAB"}),
    );
    assert_eq!(
        r["result"]["structuredContent"]["precompile"],
        "ED25519_VERIFY"
    );
    let calls = f
        .rpc_calls
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert_eq!(calls[0].0, "eth_call");
    assert_eq!(
        calls[0].1[0]["to"],
        "0x0000000000000000000000000000000000000120"
    );
    assert_eq!(calls[0].1[0]["data"], "0xab");
    let table = call(&core, &ctx("t"), "precompile_table", json!({}));
    let rows = table["result"]["structuredContent"]["precompiles"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(rows.len(), crate::node_mcp_tools::PRECOMPILES.len());
    assert!(rows
        .iter()
        .any(|r| r["short"] == "0x0110" && r["name"] == "BELNAP_AGGREGATE"));
}

#[test]
fn memory_search_never_reaches_personal_memory() {
    let core = core_with(Arc::new(Fixture::default()));
    let c = ctx("t");
    let r = call(
        &core,
        &c,
        "memory_search",
        json!({"query": "staking", "tenant": "personal"}),
    );
    assert!(is_tool_error(&r));
    let r = call(&core, &c, "memory_search", json!({"query": "staking"}));
    assert_eq!(r["result"]["structuredContent"]["tenant"], "citrate-docs");
    let r = call(
        &core,
        &c,
        "memory_search",
        json!({"query": "x", "tenant": "chain-state", "limit": 500}),
    );
    assert_eq!(r["result"]["structuredContent"]["limit"], json!(25));
    let r = call(&core, &c, "memory_search", json!({"query": "  "}));
    assert!(is_tool_error(&r));
}

#[test]
fn resources_list_read_and_templates() {
    let core = core_with(Arc::new(Fixture::default()));
    let c = ctx("t");
    let r = core
        .dispatch(
            &c,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "resources/list"}),
        )
        .expect("reply");
    let uris: Vec<String> = r["result"]["resources"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|x| x["uri"].as_str().map(str::to_string))
        .collect();
    for u in [
        "citrate://node/status",
        "citrate://chain/head",
        "citrate://wallet",
        "citrate://addresses",
        "citrate://precompiles",
    ] {
        assert!(uris.iter().any(|x| x == u), "{u}");
        let rr = core
            .dispatch(&c, &json!({"jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": {"uri": u}}))
            .expect("reply");
        assert_eq!(rr["result"]["contents"][0]["uri"], u, "{rr}");
        assert_eq!(rr["result"]["contents"][0]["mimeType"], "application/json");
    }
    let rr = core
        .dispatch(&c, &json!({"jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": {"uri": "citrate://memory/citrate-docs/search?q=paraconsensus%20round"}}))
        .expect("reply");
    let text = rr["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(text.contains("paraconsensus round"), "{text}");
    for bad in [
        "citrate://memory/personal/search?q=x",
        "citrate://nope",
        "file:///etc/passwd",
    ] {
        let rr = core
            .dispatch(&c, &json!({"jsonrpc": "2.0", "id": 4, "method": "resources/read", "params": {"uri": bad}}))
            .expect("reply");
        assert_eq!(rr["error"]["code"], json!(-32002), "{bad}");
    }
    let t = core
        .dispatch(
            &c,
            &json!({"jsonrpc": "2.0", "id": 5, "method": "resources/templates/list"}),
        )
        .expect("reply");
    assert!(t["result"]["resourceTemplates"][0]["uriTemplate"]
        .as_str()
        .unwrap_or_default()
        .starts_with("citrate://memory/"));
    let book = core
        .dispatch(&c, &json!({"jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": {"uri": "citrate://addresses"}}))
        .expect("reply");
    assert!(book["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("ValidatorRegistry"));
}

// ---------------------------------------------------------------------------
// Writes: the ceremony route and the approval inbox
// ---------------------------------------------------------------------------

#[test]
fn tx_propose_opens_a_ceremony_and_only_returns_a_pending_request() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("tokA");
    let r = call(
        &core,
        &c,
        "tx_propose",
        json!({"to": "0x0000000000000000000000000000000000000002", "value_wei": "42", "data": "0xA9059CBB"}),
    );
    let sc = &r["result"]["structuredContent"];
    assert_eq!(sc["state"], "pending");
    let rid = sc["requestId"].as_str().unwrap_or_default().to_string();
    assert!(rid.starts_with("mcpr-"));
    assert!(sc.get("sigHex").is_none() && sc.get("txHash").is_none());
    let p = f
        .proposals
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    assert_eq!(p.len(), 1);
    assert_eq!(
        p[0].0, "mcp:Claude Code via claude-code",
        "origin is shown verbatim to the member"
    );
    assert_eq!(p[0].1, "0x0000000000000000000000000000000000000002");
    assert_eq!(p[0].2, 42);
    assert_eq!(p[0].3, "0xa9059cbb");
    // The owning client sees it pending; another token cannot see it at all.
    let s = call(&core, &c, "request_status", json!({"id": rid}));
    assert_eq!(s["result"]["structuredContent"]["state"], "pending");
    assert_eq!(s["result"]["structuredContent"]["kind"], "signature");
    assert!(s["result"]["structuredContent"].get("tokenId").is_none());
    let other = call(&core, &ctx("tokB"), "request_status", json!({"id": rid}));
    assert!(is_tool_error(&other));
}

#[test]
fn cluster_and_invite_writes_queue_for_approval_and_run_nothing() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    for (name, args) in [
        ("cluster_join", json!({"group": "grp_a"})),
        (
            "cluster_share",
            json!({"group": "grp_a", "cid": "bafybeigdyrzt5"}),
        ),
        (
            "invite_create",
            json!({"group": "grp_a", "for_handle": "@ana"}),
        ),
        (
            "invite_revoke",
            json!({"group": "grp_a", "invite_id": "0123456789abcdef"}),
        ),
    ] {
        let r = call(&core, &c, name, args);
        assert_eq!(
            r["result"]["structuredContent"]["state"], "pending",
            "{name}: {r}"
        );
    }
    let (list, _) = core.inbox().list(core.now());
    assert_eq!(list.len(), 4);
    assert!(list.iter().all(|r| r.state == RequestState::Pending));
    assert!(list
        .iter()
        .any(|r| r.summary.contains("The agent will receive the invite link")));
    // Bad input queues nothing.
    for (name, args) in [
        ("cluster_join", json!({"group": "has space"})),
        ("cluster_share", json!({"group": "grp_a", "cid": "../etc"})),
        ("invite_create", json!({"group": "grp_a", "for_handle": ""})),
        (
            "invite_revoke",
            json!({"group": "grp_a", "invite_id": "short"}),
        ),
    ] {
        assert!(is_tool_error(&call(&core, &c, name, args)), "{name}");
    }
    assert_eq!(core.inbox().list(core.now()).0.len(), 4);
    assert!(f
        .proposals
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty());
}

#[test]
fn a_flood_of_writes_is_capped_before_any_ceremony_opens() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    for _ in 0..MAX_PENDING {
        let r = call(
            &core,
            &c,
            "tx_propose",
            json!({"to": "0x0000000000000000000000000000000000000002"}),
        );
        assert!(!is_tool_error(&r));
    }
    let r = call(
        &core,
        &c,
        "tx_propose",
        json!({"to": "0x0000000000000000000000000000000000000002"}),
    );
    assert!(is_tool_error(&r));
    assert_eq!(
        f.proposals.lock().unwrap_or_else(|e| e.into_inner()).len(),
        MAX_PENDING,
        "the refused request must not have opened a ceremony"
    );
    let r = call(&core, &c, "cluster_join", json!({"group": "g"}));
    assert!(is_tool_error(&r));
}

static EXPIRY_CLOCK: AtomicU64 = AtomicU64::new(1_000);
fn expiry_clock() -> u64 {
    EXPIRY_CLOCK.load(Ordering::SeqCst)
}

#[test]
fn pending_requests_expire_and_their_ceremonies_close() {
    let f = Arc::new(Fixture::default());
    let core = McpCore::new(f.clone(), Arc::new(ApprovalInbox::new())).with_clock(expiry_clock);
    let c = ctx("t");
    let r = call(
        &core,
        &c,
        "tx_propose",
        json!({"to": "0x0000000000000000000000000000000000000002"}),
    );
    let rid = r["result"]["structuredContent"]["requestId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    EXPIRY_CLOCK.store(1_000 + REQUEST_TTL_MS - 1, Ordering::SeqCst);
    let s = call(&core, &c, "request_status", json!({"id": rid}));
    assert_eq!(s["result"]["structuredContent"]["state"], "pending");
    EXPIRY_CLOCK.store(1_000 + REQUEST_TTL_MS, Ordering::SeqCst);
    let s = call(&core, &c, "request_status", json!({"id": rid}));
    assert_eq!(s["result"]["structuredContent"]["state"], "expired");
    assert_eq!(
        f.closed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["1"]
    );
    assert!(
        core.inbox()
            .begin_decision(&rid, true, expiry_clock())
            .is_err(),
        "an expired request cannot be approved"
    );
}

#[test]
fn a_request_is_decided_at_most_once() {
    let inbox = ApprovalInbox::new();
    let (req, _) = inbox
        .submit(
            "t",
            "mcp:x",
            RequestKind::Action {
                action: McpAction::ClusterJoin { group: "g".into() },
            },
            "join".into(),
            1,
        )
        .expect("submit");
    let (running, _) = inbox
        .begin_decision(&req.id, true, 2)
        .expect("first decision");
    assert_eq!(running.state, RequestState::Running);
    assert!(inbox.begin_decision(&req.id, true, 3).is_err());
    assert!(inbox.begin_decision(&req.id, false, 3).is_err());
    let done = inbox
        .finish(&req.id, Ok(json!({"joined": "g"})), 4)
        .expect("finish");
    assert_eq!(
        done.state,
        RequestState::Approved {
            result: json!({"joined": "g"})
        }
    );
    assert!(
        inbox.finish(&req.id, Ok(json!(1)), 5).is_none(),
        "finish only applies to a running request"
    );
    let (rej, _) = inbox
        .submit(
            "t",
            "mcp:x",
            RequestKind::Action {
                action: McpAction::ClusterJoin { group: "g".into() },
            },
            "join".into(),
            6,
        )
        .expect("submit");
    let (r, _) = inbox.begin_decision(&rej.id, false, 7).expect("reject");
    assert!(matches!(r.state, RequestState::Rejected { .. }));
}

#[test]
fn revoking_a_token_closes_its_pending_requests_and_ceremonies() {
    let f = Arc::new(Fixture::default());
    let state = NodeMcpState::new(f.clone(), TokenStore::in_memory(), None);
    let a = state.create_token("A").expect("issue");
    let b = state.create_token("B").expect("issue");
    let core = state.shared().core.clone();
    call(
        &core,
        &ctx(&a.id),
        "tx_propose",
        json!({"to": "0x0000000000000000000000000000000000000002"}),
    );
    call(&core, &ctx(&a.id), "cluster_join", json!({"group": "g"}));
    call(&core, &ctx(&b.id), "cluster_join", json!({"group": "g"}));
    assert!(state.revoke_token(&a.id).expect("revoke"));
    let reqs = state.requests();
    let a_states: Vec<&RequestState> = reqs
        .iter()
        .filter(|r| r.token_id == a.id)
        .map(|r| &r.state)
        .collect();
    assert_eq!(a_states.len(), 2);
    assert!(a_states
        .iter()
        .all(|s| matches!(s, RequestState::Rejected { .. })));
    assert!(reqs
        .iter()
        .any(|r| r.token_id == b.id && r.state == RequestState::Pending));
    assert_eq!(
        f.closed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_slice(),
        ["1"]
    );
    assert!(state.shared().tokens.verify(&a.connect_token, 1).is_none());
}

#[test]
fn the_ceremony_origin_strips_control_characters() {
    let c = CallerCtx {
        token_id: "t".into(),
        token_label: "Claude Code".into(),
        client_name: Some("evil\u{1b}[2Jname\nApproved".into()),
        read_only: false,
    };
    let o = c.origin();
    assert!(!o.chars().any(|ch| ch.is_control()), "{o:?}");
    assert!(o.starts_with("mcp:Claude Code via "));
    let none = CallerCtx {
        client_name: None,
        ..c
    };
    assert_eq!(none.origin(), "mcp:Claude Code");
}

// ---------------------------------------------------------------------------
// Transport: real HTTP over loopback
// ---------------------------------------------------------------------------

struct Served {
    state: NodeMcpState,
    port: u16,
    token: String,
}

fn serve() -> Served {
    let state = NodeMcpState::new(Arc::new(Fixture::default()), TokenStore::in_memory(), None);
    let token = state
        .create_token("Claude Code")
        .expect("issue")
        .connect_token;
    let port = state.start_on(0).expect("bind loopback");
    Served { state, port, token }
}

/// A minimal HTTP/1.1 client: one request per connection, read to EOF.
fn http(
    port: u16,
    method: &str,
    headers: &[(&str, String)],
    body: &str,
) -> (u16, HashMap<String, String>, String) {
    use std::collections::HashMap as _HM;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();
    let mut req = format!(
        "{method} /mcp HTTP/1.1\r\nContent-Length: {}\r\n",
        body.len()
    );
    let has_host = headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host"));
    if !has_host {
        req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).expect("write");
    let mut raw = String::new();
    s.read_to_string(&mut raw).expect("read");
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let mut h: _HM<String, String> = _HM::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            h.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    (status, h, body.to_string())
}

use std::collections::HashMap;

fn auth(t: &str) -> (&'static str, String) {
    ("Authorization", format!("Bearer {t}"))
}

fn json_ct() -> (&'static str, String) {
    ("Content-Type", "application/json".to_string())
}

fn init_body() -> String {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test-client", "version": "0"}}}).to_string()
}

fn open_session(sv: &Served) -> String {
    let (st, h, _) = http(sv.port, "POST", &[auth(&sv.token), json_ct()], &init_body());
    assert_eq!(st, 200);
    h.get("mcp-session-id").cloned().expect("session id")
}

#[test]
fn http_requires_a_valid_connect_token() {
    let sv = serve();
    let (st, h, _) = http(sv.port, "POST", &[json_ct()], &init_body());
    assert_eq!(st, 401);
    assert!(h
        .get("www-authenticate")
        .is_some_and(|v| v.starts_with("Bearer")));
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[auth(&format!("cnmcp_{}", "a".repeat(64))), json_ct()],
        &init_body(),
    );
    assert_eq!(st, 401);
    let (st, _, body) = http(sv.port, "POST", &[auth(&sv.token), json_ct()], &init_body());
    assert_eq!(st, 200, "{body}");
    sv.state.stop();
}

#[test]
fn http_refuses_foreign_hosts_and_origins_and_sends_no_cors() {
    let sv = serve();
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[("Host", "evil.example".into()), auth(&sv.token), json_ct()],
        &init_body(),
    );
    assert_eq!(st, 421, "DNS-rebinding guard");
    let (st, h, _) = http(
        sv.port,
        "POST",
        &[
            ("Origin", "https://evil.example".into()),
            auth(&sv.token),
            json_ct(),
        ],
        &init_body(),
    );
    assert_eq!(st, 403);
    assert!(h.keys().all(|k| !k.starts_with("access-control-")));
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[("Origin", "null".into()), auth(&sv.token), json_ct()],
        &init_body(),
    );
    assert_eq!(st, 403);
    let (st, h, _) = http(
        sv.port,
        "POST",
        &[
            ("Origin", format!("http://127.0.0.1:{}", sv.port)),
            auth(&sv.token),
            json_ct(),
        ],
        &init_body(),
    );
    assert_eq!(st, 200);
    assert!(h.keys().all(|k| !k.starts_with("access-control-")));
    sv.state.stop();
}

#[test]
fn http_sessions_are_required_and_bound_to_their_token() {
    let sv = serve();
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}).to_string();
    let (st, _, _) = http(sv.port, "POST", &[auth(&sv.token), json_ct()], &list);
    assert_eq!(st, 400, "no session id");
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[
            auth(&sv.token),
            json_ct(),
            ("Mcp-Session-Id", "deadbeef".into()),
        ],
        &list,
    );
    assert_eq!(st, 404, "unknown session");
    let sid = open_session(&sv);
    let other = sv.state.create_token("Other").expect("issue").connect_token;
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[auth(&other), json_ct(), ("Mcp-Session-Id", sid.clone())],
        &list,
    );
    assert_eq!(st, 404, "a session cannot be used with another token");
    let (st, _, body) = http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid.clone())],
        &list,
    );
    assert_eq!(st, 200);
    let v: Value = serde_json::from_str(&body).expect("json");
    assert!(v["result"]["tools"]
        .as_array()
        .is_some_and(|t| t.len() >= 21));
    // DELETE ends the session.
    let (st, _, _) = http(
        sv.port,
        "DELETE",
        &[auth(&sv.token), ("Mcp-Session-Id", sid.clone())],
        "",
    );
    assert_eq!(st, 204);
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid)],
        &list,
    );
    assert_eq!(st, 404);
    sv.state.stop();
}

#[test]
fn http_methods_batches_notifications_and_limits() {
    let sv = serve();
    let sid = open_session(&sv);
    let (st, h, _) = http(
        sv.port,
        "GET",
        &[auth(&sv.token), ("Mcp-Session-Id", sid.clone())],
        "",
    );
    assert_eq!(st, 405);
    assert_eq!(h.get("allow").map(String::as_str), Some("POST, DELETE"));
    let (st, _, body) = http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid.clone())],
        "[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]",
    );
    assert_eq!(st, 400);
    assert!(body.contains("-32600"));
    let (st, _, body) = http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid.clone())],
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}",
    );
    assert_eq!(st, 202);
    assert!(body.is_empty());
    let (st, _, body) = http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid.clone())],
        "{nope",
    );
    assert_eq!(st, 400);
    assert!(body.contains("-32700"));
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[
            auth(&sv.token),
            ("Content-Type", "text/plain".into()),
            ("Mcp-Session-Id", sid.clone()),
        ],
        "{}",
    );
    assert_eq!(st, 415);
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[
            auth(&sv.token),
            json_ct(),
            ("Mcp-Session-Id", sid.clone()),
            ("MCP-Protocol-Version", "1999-01-01".into()),
        ],
        "{}",
    );
    assert_eq!(st, 400);
    // Oversized body: the server refuses on Content-Length before reading it.
    let mut s = std::net::TcpStream::connect(("127.0.0.1", sv.port)).expect("connect");
    let head = format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\n\r\n", sv.port, sv.token, 2 * 1024 * 1024);
    s.write_all(head.as_bytes()).expect("write");
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    assert!(raw.starts_with("HTTP/1.1 413"), "{raw}");
    let (st, _, _) = http(sv.port, "POST", &[auth(&sv.token), json_ct()], "");
    assert_eq!(st, 400);
    sv.state.stop();
}

#[test]
fn a_real_client_flow_over_http_and_revocation_takes_effect_at_once() {
    let sv = serve();
    let sid = open_session(&sv);
    let h = |body: Value| {
        http(
            sv.port,
            "POST",
            &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid.clone())],
            &body.to_string(),
        )
    };
    let (st, _, _) = h(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert_eq!(st, 202);
    let (_, _, body) = h(
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "chain_head", "arguments": {}}}),
    );
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["result"]["structuredContent"]["chainId"], json!(40204));
    let (_, _, body) = h(
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "tx_propose", "arguments": {"to": "0x0000000000000000000000000000000000000002"}}}),
    );
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["result"]["structuredContent"]["state"], "pending");
    // The member sees it in Settings, with the origin the token label + client name produce.
    let reqs = sv.state.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].origin, "mcp:Claude Code via test-client");
    // The recent-calls log records it.
    assert!(sv
        .state
        .status()
        .recent_calls
        .iter()
        .any(|c| c.tool.as_deref() == Some("tx_propose")));
    // Revoke: the next request is refused and the pending request is closed.
    let id = sv.state.status().tokens[0].id.clone();
    assert!(sv.state.revoke_token(&id).expect("revoke"));
    let (st, _, _) = h(json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}));
    assert_eq!(st, 401);
    assert!(matches!(
        sv.state.requests()[0].state,
        RequestState::Rejected { .. }
    ));
    sv.state.stop();
}

#[test]
fn server_start_is_idempotent_and_stop_frees_the_port() {
    let sv = serve();
    assert_eq!(
        sv.state.start_on(0).expect("again"),
        sv.port,
        "a second start reuses the listener"
    );
    assert!(sv.state.is_running());
    sv.state.stop();
    assert!(!sv.state.is_running());
    // The port is free again (rebind succeeds).
    let l = std::net::TcpListener::bind(("127.0.0.1", sv.port)).expect("port released");
    drop(l);
}

#[test]
fn the_server_binds_loopback_only() {
    let sv = serve();
    let running = sv.state.server.lock().unwrap_or_else(|e| e.into_inner());
    let addr = running.as_ref().map(|s| s.addr).expect("running");
    assert!(addr.ip().is_loopback());
    drop(running);
    sv.state.stop();
}

// ---------------------------------------------------------------------------
// stdio shim, through its production ureq transport, against the real server
// ---------------------------------------------------------------------------

#[test]
fn stdio_shim_forwards_lines_keeps_the_session_and_skips_notifications() {
    let sv = serve();
    let input = format!(
        "{}\n{}\n\n{}\n{}\n",
        init_body(),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "node_status", "arguments": {}}}),
    );
    let transport = UreqShimTransport {
        url: crate::node_mcp_http::endpoint_url(sv.port),
    };
    let mut out: Vec<u8> = Vec::new();
    let code = run_stdio_shim(std::io::Cursor::new(input), &mut out, &transport, &sv.token);
    assert_eq!(code, 0);
    let text = String::from_utf8(out).expect("utf8");
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is one JSON message"))
        .collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert_eq!(lines[0]["id"], json!(1));
    assert_eq!(lines[1]["id"], json!(2));
    assert!(
        lines[1]["result"]["tools"].is_array(),
        "the session from initialize was reused"
    );
    assert_eq!(lines[2]["result"]["structuredContent"]["state"], "running");
    sv.state.stop();
}

#[test]
fn stdio_shim_reports_a_stopped_server_and_a_bad_token_as_jsonrpc_errors() {
    let sv = serve();
    let transport = UreqShimTransport {
        url: crate::node_mcp_http::endpoint_url(sv.port),
    };
    let mut out: Vec<u8> = Vec::new();
    run_stdio_shim(
        std::io::Cursor::new(format!("{}\n", init_body())),
        &mut out,
        &transport,
        "cnmcp_wrong",
    );
    let v: Value =
        serde_json::from_str(String::from_utf8(out).unwrap_or_default().trim()).expect("json");
    assert_eq!(v["id"], json!(1));
    // A token the server does not hold: the server cannot prove itself for it, so the token is
    // never sent.
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("was not sent"),
        "{v}"
    );
    sv.state.stop();
    let mut out: Vec<u8> = Vec::new();
    run_stdio_shim(
        std::io::Cursor::new(format!("{}\n", init_body())),
        &mut out,
        &transport,
        &sv.token,
    );
    let v: Value =
        serde_json::from_str(String::from_utf8(out).unwrap_or_default().trim()).expect("json");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("not running"));
}

// ---------------------------------------------------------------------------
// Settings state
// ---------------------------------------------------------------------------

#[test]
fn the_server_is_off_by_default_and_the_switch_persists() {
    let dir = std::env::temp_dir().join(format!("n4-node-mcp-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let st = NodeMcpState::new(
        Arc::new(Fixture::default()),
        TokenStore::in_memory(),
        Some(dir.clone()),
    );
    assert!(!st.config().enabled, "off unless the member turns it on");
    assert!(!st.is_running());
    let status = st.status();
    assert!(!status.running);
    assert!(status.endpoint.starts_with("http://127.0.0.1:"));
    // Persist the switch without binding the fixed port in a test: write via set_enabled(false)
    // then flip the file the way set_enabled(true) does.
    st.set_enabled(false).expect("save");
    assert!(!st.config().enabled);
    let mut cfg = st.config();
    cfg.enabled = true;
    cfg.port = Some(0);
    st.save_config(&cfg).expect("save");
    let again = NodeMcpState::new(
        Arc::new(Fixture::default()),
        TokenStore::in_memory(),
        Some(dir.clone()),
    );
    assert!(again.config().enabled);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_and_requests_serialize_without_secrets() {
    let sv = serve();
    let sid = open_session(&sv);
    let body = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "invite_create", "arguments": {"group": "grp_a", "for_handle": "@ana"}}});
    http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid)],
        &body.to_string(),
    );
    let s = serde_json::to_string(&sv.state.status()).expect("json");
    let r = serde_json::to_string(&sv.state.requests()).expect("json");
    assert!(!s.contains(&sv.token) && !r.contains(&sv.token));
    assert!(r.contains("\"action\":\"invite_create\""), "{r}");
    sv.state.stop();
}

// ---------------------------------------------------------------------------
// Demo (manual): real chain reads through the real server
// ---------------------------------------------------------------------------

/// A data source whose chain reads hit the live public 40204 RPC (no app needed). Everything else
/// says honestly that this harness has no app behind it.
struct PublicChainOnly;

impl NodeBackend for PublicChainOnly {
    fn node_status(&self) -> Result<Value, String> {
        Err("demo harness: no supervised node in this process".into())
    }
    fn rpc_read(&self, method: &str, params: Value) -> Result<RpcRead, String> {
        use crate::rpc::RpcTransport;
        let client = crate::rpc::RpcClient::with_transport(crate::rpc::HttpTransport::citrate());
        let resp = client
            .transport()
            .call(client.build_request(method, params))
            .map_err(|e| e.to_string())?;
        if let Some(e) = resp.get("error") {
            return Err(e.to_string());
        }
        Ok(RpcRead {
            value: resp.get("result").cloned().unwrap_or(Value::Null),
            source: "public-rpc".into(),
        })
    }
    fn wallet_address(&self) -> Result<String, String> {
        Err("demo harness: no wallet".into())
    }
    fn memory_search(&self, _: &str, _: &str, _: usize) -> Result<Value, String> {
        Err("demo harness: no memory daemon".into())
    }
    fn groups(&self) -> Result<Value, String> {
        Err("demo harness: no comms daemon".into())
    }
    fn cluster_status(&self, _: &str) -> Result<Value, String> {
        Err("demo harness: no cluster daemon".into())
    }
    fn cluster_peers(&self, _: &str) -> Result<Value, String> {
        Err("demo harness: no cluster daemon".into())
    }
    fn invites(&self, _: &str) -> Result<Value, String> {
        Err("demo harness: no invites".into())
    }
    fn propose_transaction(
        &self,
        _: &str,
        _: &str,
        _: u128,
        _: &str,
    ) -> Result<ProposedSignature, String> {
        Err("demo harness: no ceremony".into())
    }
    fn close_ceremony(&self, _: &str) {}
    fn devices(&self) -> Result<Value, String> {
        Err("demo harness: no device links".into())
    }
    fn pins(&self) -> Result<Value, String> {
        Err("demo harness: no IPFS daemon".into())
    }
    fn propose_deploy(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u128,
        _: Option<u64>,
    ) -> Result<ProposedSignature, String> {
        Err("demo harness: no deploy gate or ceremony".into())
    }
    fn anchor_ready(&self) -> Result<(), String> {
        Err("demo harness: AnchorRegistry is not in this build's address book".into())
    }
    fn contract_abi(&self, address: &str) -> Result<Value, String> {
        // The same public source as the app (CitrateScan's verified sources).
        crate::node_mcp_live::abi_registry_entry(address)
    }
}

/// Run with `cargo test --lib node_mcp_demo -- --ignored --nocapture`. Prints a real
/// `tools/list` and a real `chain_head` call against the public 40204 RPC. With
/// `NODE_MCP_DEMO_HOLD_SECS` and `NODE_MCP_DEMO_TOKEN_FILE` set, it also keeps serving on
/// `NODE_MCP_DEMO_PORT` (default 47299) so an external client (curl, the stdio shim, Claude Code)
/// can connect.
#[test]
#[ignore = "manual demo: needs network access to rpc.citrate.ai"]
fn node_mcp_demo() {
    let state = NodeMcpState::new(Arc::new(PublicChainOnly), TokenStore::in_memory(), None);
    let token = state.create_token("demo").expect("issue").connect_token;
    let port: u16 = std::env::var("NODE_MCP_DEMO_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(47299);
    let port = state.start_on(port).expect("bind");
    let sv = Served { state, port, token };
    let sid = open_session(&sv);
    let post = |body: Value| {
        println!(">>> {body}");
        let (st, _, b) = http(
            sv.port,
            "POST",
            &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid.clone())],
            &body.to_string(),
        );
        println!("<<< HTTP {st} {b}\n");
        b
    };
    let list = post(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert!(list.contains("tx_propose"));
    let head = post(
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "chain_head", "arguments": {}}}),
    );
    assert!(head.contains("40204"), "{head}");
    if let (Ok(secs), Ok(file)) = (
        std::env::var("NODE_MCP_DEMO_HOLD_SECS"),
        std::env::var("NODE_MCP_DEMO_TOKEN_FILE"),
    ) {
        crate::node_mcp_token::write_private(std::path::Path::new(&file), sv.token.as_bytes())
            .expect("token file");
        // With NODE_MCP_DEMO_RO_TOKEN_FILE, also issue the read-only token core gives Hermes.
        if let Ok(ro_file) = std::env::var("NODE_MCP_DEMO_RO_TOKEN_FILE") {
            let ro = sv.state.hermes_token(None).expect("issue");
            crate::node_mcp_token::write_private(
                std::path::Path::new(&ro_file),
                ro.as_bytes(),
            )
            .expect("read-only token file");
        }
        println!(
            "serving on {} for {secs}s",
            crate::node_mcp_http::endpoint_url(sv.port)
        );
        std::thread::sleep(std::time::Duration::from_secs(secs.parse().unwrap_or(60)));
    }
    sv.state.stop();
}

#[test]
fn the_stdio_shim_sends_the_token_to_loopback_only() {
    use crate::node_mcp_http::shim_url_is_loopback as ok;
    assert!(ok("http://127.0.0.1:47204/mcp"));
    assert!(ok("http://localhost:47204/mcp"));
    assert!(ok("http://[::1]:47204/mcp"));
    assert!(!ok("https://127.0.0.1:47204/mcp"));
    assert!(!ok("http://127.0.0.1:47204@evil.example/mcp"));
    assert!(!ok("http://localhost.evil.example/mcp"));
    assert!(!ok("http://10.0.0.5:47204/mcp"));
    assert!(!ok("not a url"));
}

#[test]
fn the_stdio_shim_endpoint_resolves_to_loopback_or_refuses() {
    use crate::node_mcp_http::resolve_shim_url as r;
    assert_eq!(r(None, None).as_deref(), Ok("http://127.0.0.1:47204/mcp"));
    assert_eq!(
        r(None, Some("5000".into())).as_deref(),
        Ok("http://127.0.0.1:5000/mcp")
    );
    assert!(r(None, Some("nope".into())).is_err());
    assert_eq!(
        r(Some("http://localhost:9/mcp".into()), None).as_deref(),
        Ok("http://localhost:9/mcp")
    );
    assert!(r(Some("https://example.com/mcp".into()), None).is_err());
    assert!(r(
        Some("http://127.0.0.1:1@evil.example/mcp".into()),
        Some("1".into())
    )
    .is_err());
    assert_eq!(
        r(Some("  ".into()), None).as_deref(),
        Ok("http://127.0.0.1:47204/mcp")
    );
}

// ---------------------------------------------------------------------------
// Review hardening (adversarial review of HUP-S4.2 + S8.5)
// ---------------------------------------------------------------------------

#[test]
fn a_chunked_body_is_refused_before_it_is_read() {
    let raw = b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
    let r = crate::node_mcp_http::read_request(&mut &raw[..]);
    assert_eq!(r.map(|_| 0).unwrap_or_else(|e| e.status), 411);
}

/// A reader that trickles one header byte per read and never finishes the header.
struct Trickle;
impl std::io::Read for Trickle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::thread::sleep(std::time::Duration::from_millis(5));
        buf[0] = b'a';
        Ok(1)
    }
}

#[test]
fn a_trickled_request_hits_a_whole_request_deadline() {
    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_millis(60);
    let r = crate::node_mcp_http::read_request_by(&mut Trickle, deadline);
    assert_eq!(r.map(|_| 0).unwrap_or_else(|e| e.status), 408);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "the deadline bounds the whole request, not each read"
    );
}

#[test]
fn delete_cannot_end_another_tokens_session() {
    let sv = serve();
    let sid = open_session(&sv);
    let other = sv.state.create_token("Other").expect("issue").connect_token;
    let (st, _, _) = http(
        sv.port,
        "DELETE",
        &[auth(&other), ("Mcp-Session-Id", sid.clone())],
        "",
    );
    assert_eq!(st, 404, "another token cannot end this session");
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}).to_string();
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct(), ("Mcp-Session-Id", sid)],
        &list,
    );
    assert_eq!(st, 200, "the owner's session is still open");
    sv.state.stop();
}

#[test]
fn bidi_controls_never_reach_approval_text() {
    let rlo = "pay\u{202E}evil";
    assert!(crate::node_mcp_token::normalize_label(rlo).is_err());
    assert!(crate::node_mcp_tools::parse_label(rlo, "for_handle").is_err());
    assert!(crate::node_mcp_tools::parse_label("a\u{2066}b", "for_handle").is_err());
    let c = CallerCtx {
        token_id: "t".into(),
        token_label: "Claude Code".into(),
        client_name: Some("x\u{202E}\u{2067}\u{200F}y".into()),
        read_only: false,
    };
    assert_eq!(c.origin(), "mcp:Claude Code via xy");
}

// ---------------------------------------------------------------------------
// HUP n5: the rest of the planset's tool list (dag_stats, devices_list, pins_list,
// deploy_propose, pin_add, anchor_propose), MCP Tasks, and the 2026-07-28 stateless era.
// ---------------------------------------------------------------------------

use crate::node_mcp_protocol::{
    HEADER_MISMATCH, INVALID_PARAMS, METHOD_NOT_FOUND, META_CLIENT_CAPABILITIES, META_CLIENT_INFO, META_PROTOCOL_VERSION,
    STATELESS_PROTOCOL_VERSION, TASKS_EXTENSION, TASK_POLL_MS, TASK_TTL_MS,
    UNSUPPORTED_PROTOCOL_VERSION,
};

/// `_meta` for a client that declares the MCP Tasks extension (session era: no version key).
fn tasks_meta() -> Value {
    json!({ META_CLIENT_CAPABILITIES: {"extensions": {TASKS_EXTENSION: {}}} })
}

/// `_meta` for a stateless-era (2026-07-28) request.
fn stateless_meta(tasks: bool) -> Value {
    let mut caps = json!({});
    if tasks {
        caps = json!({"extensions": {TASKS_EXTENSION: {}}});
    }
    json!({
        META_PROTOCOL_VERSION: STATELESS_PROTOCOL_VERSION,
        META_CLIENT_INFO: {"name": "stateless-client", "version": "1"},
        META_CLIENT_CAPABILITIES: caps,
    })
}

fn call_meta(core: &McpCore, c: &CallerCtx, name: &str, args: Value, meta: Value) -> Value {
    core.dispatch(
        c,
        &json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": name, "arguments": args, "_meta": meta}}),
    )
    .expect("a request gets a reply")
}

fn rpc(core: &McpCore, c: &CallerCtx, method: &str, params: Value) -> Value {
    core.dispatch(
        c,
        &json!({"jsonrpc": "2.0", "id": 11, "method": method, "params": params}),
    )
    .expect("a request gets a reply")
}

#[test]
fn dag_stats_reports_what_the_node_counts_and_drops_its_estimates() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let r = call(&core, &ctx("t"), "dag_stats", json!({}));
    assert!(!is_tool_error(&r), "{r}");
    let v = &r["result"]["structuredContent"];
    assert_eq!(v["height"], json!(92156));
    assert_eq!(v["maxBlueScore"], json!(92156));
    assert_eq!(v["tipsCount"], json!(1));
    assert_eq!(v["ghostdagParams"]["k"], json!(18));
    assert_eq!(v["source"], json!("local-node"));
    // Only well-formed 32-byte tips pass, lowercased.
    assert_eq!(
        v["tips"],
        json!(["0xf36b109288455df4a80cf87ab2209e95183235ab2d3a6e96514ef27eafaa2b52"])
    );
    // The node's 95% blue/red split is an estimate, not a count: never passed on as fact.
    for k in ["blueBlocks", "redBlocks", "totalBlocks"] {
        assert!(v.get(k).is_none(), "{k} must not be reported");
    }
    let calls = f.rpc_calls.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(calls.last().map(|c| c.0.as_str()), Some("citrate_getDagStats"));
}

#[test]
fn dag_stats_refuses_a_malformed_node_answer() {
    let bad = RpcRead {
        value: json!({"tips": []}),
        source: "local-node".into(),
    };
    assert!(crate::node_mcp_protocol::dag_stats_view(&bad).is_err());
}

#[test]
fn devices_and_pins_are_read_tools_from_the_backend() {
    let core = core_with(Arc::new(Fixture::default()));
    let d = call(&core, &ctx("t"), "devices_list", json!({}));
    assert_eq!(
        d["result"]["structuredContent"]["links"][0]["label"],
        json!("Studio Mac")
    );
    let p = call(&core, &ctx("t"), "pins_list", json!({}));
    assert_eq!(
        p["result"]["structuredContent"]["pins"][0]["cid"],
        json!("bafyfixture")
    );
    assert!(is_tool_error(&call(
        &core,
        &ctx("t"),
        "pins_list",
        json!({"extra": 1})
    )));
    // Read tools never queue anything.
    assert_eq!(core.inbox().list(1).0.len(), 0);
}

#[test]
fn deploy_propose_opens_a_gated_ceremony_and_only_returns_a_pending_request() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let r = call(
        &core,
        &ctx("tok1"),
        "deploy_propose",
        json!({"bytecode": "0x6080AB", "constructor_args": "0x01", "value_wei": "5", "gas": 3000000}),
    );
    assert!(!is_tool_error(&r), "{r}");
    let v = &r["result"]["structuredContent"];
    assert_eq!(v["state"], json!("pending"));
    assert!(v["summary"].as_str().unwrap_or_default().contains("deploy gate READY"));
    let deploys = f.deploys.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(deploys.len(), 1);
    assert_eq!(deploys[0].0, "mcp:Claude Code via claude-code");
    assert_eq!(deploys[0].1, "0x6080ab");
    assert_eq!(deploys[0].2, "0x01");
    assert_eq!(deploys[0].3, 5);
    assert_eq!(deploys[0].4, Some(3_000_000));
    let (reqs, _) = core.inbox().list(1);
    assert!(matches!(reqs[0].kind, RequestKind::Signature { .. }));
}

#[test]
fn deploy_propose_is_refused_at_once_when_the_gate_is_not_ready() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let r = call(
        &core,
        &ctx("t"),
        "deploy_propose",
        json!({"bytecode": NOT_READY_CODE}),
    );
    assert!(is_tool_error(&r));
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("NOT READY"));
    assert_eq!(core.inbox().list(1).0.len(), 0, "nothing queued");
}

#[test]
fn deploy_propose_arguments_are_validated_before_the_gate_is_asked() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    for args in [
        json!({}),
        json!({"bytecode": "0x"}),
        json!({"bytecode": "6080"}),
        json!({"bytecode": "0x608"}),
        json!({"bytecode": "0x6080", "gas": 20999}),
        json!({"bytecode": "0x6080", "gas": 30_000_001u64}),
        json!({"bytecode": "0x6080", "gas": "lots"}),
        json!({"bytecode": "0x6080", "value_wei": "-1"}),
        json!({"bytecode": "0x6080", "to": "0x00000000000000000000000000000000000000aa"}),
        json!({"bytecode": format!("0x{}", "ab".repeat(40_000)), "constructor_args": format!("0x{}", "cd".repeat(30_000))}),
    ] {
        let r = call(&core, &ctx("t"), "deploy_propose", args.clone());
        assert!(is_tool_error(&r), "accepted {args}");
    }
    assert!(f.deploys.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
}

#[test]
fn pin_add_queues_a_local_pin_and_runs_nothing() {
    let core = core_with(Arc::new(Fixture::default()));
    let r = call(&core, &ctx("t"), "pin_add", json!({"cid": "bafybeigdyrzt"}));
    assert!(!is_tool_error(&r), "{r}");
    let (reqs, _) = core.inbox().list(1);
    assert_eq!(reqs.len(), 1);
    match &reqs[0].kind {
        RequestKind::Action {
            action: McpAction::PinAdd { cid },
        } => assert_eq!(cid, "bafybeigdyrzt"),
        other => panic!("unexpected {other:?}"),
    }
    assert!(reqs[0].summary.contains("no storage bond"));
    for bad in [json!({"cid": "../etc"}), json!({"cid": ""}), json!({})] {
        assert!(is_tool_error(&call(&core, &ctx("t"), "pin_add", bad)));
    }
}

#[test]
fn anchor_propose_is_refused_while_anchoring_is_not_ready() {
    let f = Arc::new(Fixture::default());
    *f.anchor_not_ready.lock().unwrap_or_else(|e| e.into_inner()) =
        Some("Nightly anchoring is off.".into());
    let core = core_with(f.clone());
    let r = call(&core, &ctx("t"), "anchor_propose", json!({}));
    assert!(is_tool_error(&r));
    assert_eq!(
        r["result"]["content"][0]["text"],
        json!("Nightly anchoring is off.")
    );
    assert_eq!(core.inbox().list(1).0.len(), 0);
    *f.anchor_not_ready.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let ok = call(&core, &ctx("t"), "anchor_propose", json!({}));
    assert!(!is_tool_error(&ok), "{ok}");
    let (reqs, _) = core.inbox().list(1);
    assert!(matches!(
        reqs[0].kind,
        RequestKind::Action {
            action: McpAction::AnchorPropose
        }
    ));
}

// ---- MCP Tasks -------------------------------------------------------------------------------

#[test]
fn a_write_returns_a_task_only_to_a_client_that_declared_the_extension() {
    let core = core_with(Arc::new(Fixture::default())).with_clock(|| 1_759_312_800_000);
    let plain = call(
        &core,
        &ctx("t"),
        "cluster_join",
        json!({"group": "grp_a"}),
    );
    assert!(plain["result"].get("resultType").is_none());
    assert_eq!(plain["result"]["structuredContent"]["state"], json!("pending"));

    let task = call_meta(
        &core,
        &ctx("t"),
        "cluster_join",
        json!({"group": "grp_a"}),
        tasks_meta(),
    );
    let t = &task["result"];
    assert_eq!(t["resultType"], json!("task"));
    assert_eq!(t["status"], json!("working"));
    assert_eq!(t["taskId"], json!("mcpr-2"));
    assert_eq!(t["ttlMs"], json!(TASK_TTL_MS));
    assert_eq!(t["pollIntervalMs"], json!(TASK_POLL_MS));
    assert_eq!(t["createdAt"], json!("2025-10-01T10:00:00Z"));
    assert!(t.get("content").is_none(), "a task is not a CallToolResult");

    // A read tool never becomes a task, even when the client declared the extension.
    let read = call_meta(&core, &ctx("t"), "chain_head", json!({}), tasks_meta());
    assert!(read["result"].get("taskId").is_none());
    assert_eq!(read["result"]["isError"], json!(false));
    // A refused write is a tool error, not a task.
    let bad = call_meta(&core, &ctx("t"), "pin_add", json!({"cid": "../x"}), tasks_meta());
    assert!(is_tool_error(&bad));
}

#[test]
fn tasks_get_follows_the_members_decision() {
    let core = core_with(Arc::new(Fixture::default()));
    let c = ctx("t");
    let mk = |core: &McpCore| -> String {
        let r = call_meta(core, &c, "pin_add", json!({"cid": "bafyabc"}), tasks_meta());
        r["result"]["taskId"].as_str().unwrap_or_default().to_string()
    };
    // Approved and done: completed with the tool result.
    let a = mk(&core);
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": a}));
    assert_eq!(g["result"]["status"], json!("working"));
    assert_eq!(g["result"]["resultType"], json!("complete"));
    core.inbox().begin_decision(&a, true, 2).expect("approve");
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": a}));
    assert_eq!(g["result"]["status"], json!("working"));
    assert!(g["result"]["statusMessage"].as_str().unwrap_or_default().contains("running"));
    core.inbox().finish(&a, Ok(json!({"pinned": "bafyabc"})), 3);
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": a}));
    assert_eq!(g["result"]["status"], json!("completed"));
    assert_eq!(g["result"]["result"]["isError"], json!(false));
    assert_eq!(
        g["result"]["result"]["structuredContent"]["pinned"],
        json!("bafyabc")
    );
    // Rejected by the member: completed with an isError tool result (not a JSON-RPC failure).
    let b = mk(&core);
    core.inbox().begin_decision(&b, false, 4).expect("reject");
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": b}));
    assert_eq!(g["result"]["status"], json!("completed"));
    assert_eq!(g["result"]["result"]["isError"], json!(true));
    // Failed after approval: completed, isError, carrying the error.
    let d = mk(&core);
    core.inbox().begin_decision(&d, true, 5).expect("approve");
    core.inbox().finish(&d, Err("kubo is not running".into()), 6);
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": d}));
    assert_eq!(g["result"]["status"], json!("completed"));
    assert!(g["result"]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("kubo is not running"));
}

static TASK_CLOCK: AtomicU64 = AtomicU64::new(0);
fn task_clock() -> u64 {
    TASK_CLOCK.load(Ordering::SeqCst)
}

#[test]
fn tasks_expire_with_their_request() {
    let core = core_with(Arc::new(Fixture::default())).with_clock(task_clock);
    let c = ctx("t");
    let r = call_meta(&core, &c, "pin_add", json!({"cid": "bafyabc"}), tasks_meta());
    let id = r["result"]["taskId"].as_str().unwrap_or_default().to_string();
    TASK_CLOCK.store(REQUEST_TTL_MS + 1, Ordering::SeqCst);
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id}));
    assert_eq!(g["result"]["status"], json!("completed"));
    assert_eq!(g["result"]["result"]["isError"], json!(true));
}

#[test]
fn tasks_cancel_withdraws_a_pending_request_and_closes_its_ceremony() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    let r = call_meta(
        &core,
        &c,
        "tx_propose",
        json!({"to": "0x00000000000000000000000000000000000000bb"}),
        tasks_meta(),
    );
    let id = r["result"]["taskId"].as_str().unwrap_or_default().to_string();
    let ack = rpc(&core, &c, "tasks/cancel", json!({"taskId": id}));
    assert_eq!(ack["result"], json!({"resultType": "complete"}));
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id}));
    assert_eq!(g["result"]["status"], json!("cancelled"));
    assert_eq!(
        f.closed.lock().unwrap_or_else(|e| e.into_inner()).as_slice(),
        ["1"]
    );
    // The member can no longer approve it.
    assert!(core.inbox().begin_decision(&id, true, 9).is_err());
    // Cancelling a decided task is acknowledged and changes nothing (cooperative).
    let r2 = call_meta(&core, &c, "pin_add", json!({"cid": "bafyabc"}), tasks_meta());
    let id2 = r2["result"]["taskId"].as_str().unwrap_or_default().to_string();
    core.inbox().begin_decision(&id2, true, 2).expect("approve");
    let ack = rpc(&core, &c, "tasks/cancel", json!({"taskId": id2}));
    assert!(ack.get("error").is_none());
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id2}));
    assert_eq!(g["result"]["status"], json!("working"));
}

#[test]
fn a_client_sees_and_cancels_only_its_own_tasks() {
    let core = core_with(Arc::new(Fixture::default()));
    let r = call_meta(
        &core,
        &ctx("owner"),
        "pin_add",
        json!({"cid": "bafyabc"}),
        tasks_meta(),
    );
    let id = r["result"]["taskId"].as_str().unwrap_or_default().to_string();
    for m in ["tasks/get", "tasks/cancel", "tasks/update"] {
        let e = rpc(&core, &ctx("other"), m, json!({"taskId": id, "inputResponses": {}}));
        assert_eq!(e["error"]["code"], json!(INVALID_PARAMS), "{m}");
    }
    let g = rpc(&core, &ctx("owner"), "tasks/get", json!({"taskId": id}));
    assert_eq!(g["result"]["status"], json!("working"), "other's cancel had no effect");
    // tasks/update is acknowledged (this server never asks for input) and changes nothing.
    let u = rpc(
        &core,
        &ctx("owner"),
        "tasks/update",
        json!({"taskId": id, "inputResponses": {"k": {}}}),
    );
    assert_eq!(u["result"], json!({"resultType": "complete"}));
    let missing = rpc(&core, &ctx("owner"), "tasks/get", json!({}));
    assert_eq!(missing["error"]["code"], json!(INVALID_PARAMS));
}

// ---- The stateless era (2026-07-28) -----------------------------------------------------------

#[test]
fn server_discover_lists_both_eras_and_the_tasks_extension() {
    let core = core_with(Arc::new(Fixture::default()));
    let d = rpc(
        &core,
        &ctx("t"),
        "server/discover",
        json!({"_meta": stateless_meta(false)}),
    );
    let r = &d["result"];
    assert_eq!(r["resultType"], json!("complete"));
    assert_eq!(
        r["supportedVersions"],
        json!(["2026-07-28", "2025-06-18", "2025-03-26", "2024-11-05"])
    );
    assert!(r["capabilities"]["extensions"][TASKS_EXTENSION].is_object());
    assert!(r["capabilities"]["tools"].is_object());
    assert_eq!(
        r["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        json!("citrate-node")
    );
    assert!(r["ttlMs"].is_u64() && r["cacheScope"].is_string());
    assert!(r["instructions"].as_str().unwrap_or_default().contains("deploy_propose"));
}

#[test]
fn stateless_results_carry_result_type_server_info_and_cache_hints() {
    let core = core_with(Arc::new(Fixture::default()));
    let c = ctx("t");
    let l = rpc(&core, &c, "tools/list", json!({"_meta": stateless_meta(false)}));
    assert_eq!(l["result"]["resultType"], json!("complete"));
    assert_eq!(l["result"]["cacheScope"], json!("private"));
    assert!(l["result"]["ttlMs"].as_u64().unwrap_or(0) > 0);
    assert_eq!(
        l["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        json!("citrate-node")
    );
    let rr = rpc(
        &core,
        &c,
        "resources/read",
        json!({"uri": "citrate://chain/head", "_meta": stateless_meta(false)}),
    );
    assert_eq!(rr["result"]["ttlMs"], json!(0));
    let call = call_meta(&core, &c, "chain_head", json!({}), stateless_meta(false));
    assert_eq!(call["result"]["resultType"], json!("complete"));
    assert!(call["result"].get("ttlMs").is_none());
    let task = call_meta(&core, &c, "pin_add", json!({"cid": "bafyabc"}), stateless_meta(true));
    assert_eq!(task["result"]["resultType"], json!("task"), "decoration keeps \"task\"");
    // The session era is unchanged: no resultType on a plain result.
    let legacy = rpc(&core, &c, "tools/list", json!({}));
    assert!(legacy["result"].get("resultType").is_none());
}

#[test]
fn stateless_requests_have_no_initialize_or_ping_and_check_the_version() {
    let core = core_with(Arc::new(Fixture::default()));
    let c = ctx("t");
    for m in ["initialize", "ping"] {
        let e = rpc(&core, &c, m, json!({"_meta": stateless_meta(false)}));
        assert_eq!(e["error"]["code"], json!(METHOD_NOT_FOUND), "{m}");
    }
    let e = rpc(
        &core,
        &c,
        "tools/list",
        json!({"_meta": {META_PROTOCOL_VERSION: "1900-01-01"}}),
    );
    assert_eq!(e["error"]["code"], json!(UNSUPPORTED_PROTOCOL_VERSION));
    assert_eq!(e["error"]["data"]["requested"], json!("1900-01-01"));
    assert_eq!(e["error"]["data"]["supported"][0], json!("2026-07-28"));
    // Resource not found is INVALID_PARAMS in the stateless era, -32002 in the session era.
    let nf = rpc(
        &core,
        &c,
        "resources/read",
        json!({"uri": "citrate://nope", "_meta": stateless_meta(false)}),
    );
    assert_eq!(nf["error"]["code"], json!(INVALID_PARAMS));
    let nf_legacy = rpc(&core, &c, "resources/read", json!({"uri": "citrate://nope"}));
    assert_eq!(
        nf_legacy["error"]["code"],
        json!(crate::node_mcp_protocol::RESOURCE_NOT_FOUND)
    );
}

#[test]
fn header_values_round_trip_through_the_base64_sentinel() {
    use crate::node_mcp_http::{decode_header_value as dec, encode_header_value as enc};
    assert_eq!(enc("tools/call"), "tools/call");
    for v in ["Hello, 世界", " padded ", "line1\nline2", "=?base64?literal?=", ""] {
        let e = enc(v);
        assert!(e.starts_with("=?base64?"), "{v:?} -> {e}");
        assert_eq!(dec(&e).as_deref(), Some(v));
    }
    assert_eq!(dec("=?base64?!!!?=").as_deref(), None);
}

fn stateless_headers(token: &str, method: &str, name: Option<&str>) -> Vec<(&'static str, String)> {
    let mut h = vec![
        auth(token),
        json_ct(),
        ("MCP-Protocol-Version", STATELESS_PROTOCOL_VERSION.to_string()),
        ("Mcp-Method", method.to_string()),
    ];
    if let Some(n) = name {
        h.push((
            "Mcp-Name",
            crate::node_mcp_http::encode_header_value(n),
        ));
    }
    h
}

#[test]
fn http_stateless_requests_need_no_session_but_do_need_the_token_and_headers() {
    let sv = serve();
    let body = |method: &str, extra: Value| {
        let mut params = json!({"_meta": stateless_meta(true)});
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                p.insert(k.clone(), v.clone());
            }
        }
        json!({"jsonrpc": "2.0", "id": 5, "method": method, "params": params}).to_string()
    };
    // discover and a read tool, no session.
    let (st, _, b) = http(
        sv.port,
        "POST",
        &stateless_headers(&sv.token, "server/discover", None),
        &body("server/discover", json!({})),
    );
    assert_eq!(st, 200, "{b}");
    assert!(b.contains("2026-07-28"));
    let (st, _, b) = http(
        sv.port,
        "POST",
        &stateless_headers(&sv.token, "tools/call", Some("chain_head")),
        &body("tools/call", json!({"name": "chain_head", "arguments": {}})),
    );
    assert_eq!(st, 200, "{b}");
    assert!(b.contains("\"resultType\":\"complete\""), "{b}");
    // A write becomes a task; tasks/get finds it with the same token.
    let (st, _, b) = http(
        sv.port,
        "POST",
        &stateless_headers(&sv.token, "tools/call", Some("pin_add")),
        &body("tools/call", json!({"name": "pin_add", "arguments": {"cid": "bafyabc"}})),
    );
    assert_eq!(st, 200, "{b}");
    let v: Value = serde_json::from_str(&b).unwrap_or(Value::Null);
    assert_eq!(v["result"]["resultType"], json!("task"));
    let tid = v["result"]["taskId"].as_str().unwrap_or_default().to_string();
    let (st, _, b) = http(
        sv.port,
        "POST",
        &stateless_headers(&sv.token, "tasks/get", None),
        &body("tasks/get", json!({"taskId": tid})),
    );
    assert_eq!(st, 200, "{b}");
    assert!(b.contains("\"status\":\"working\""), "{b}");
    // The approval card shows the client name from _meta.
    let reqs = sv.state.requests();
    assert!(reqs[0].origin.contains("stateless-client"), "{}", reqs[0].origin);

    // No token: 401, whatever the era.
    let mut no_tok = stateless_headers(&sv.token, "tools/list", None);
    no_tok.remove(0);
    let (st, _, _) = http(sv.port, "POST", &no_tok, &body("tools/list", json!({})));
    assert_eq!(st, 401);
    // Missing or mismatched mirrored headers: 400 HeaderMismatch.
    let mut missing = stateless_headers(&sv.token, "tools/list", None);
    missing.retain(|(k, _)| *k != "Mcp-Method");
    for (headers, b) in [
        (missing, body("tools/list", json!({}))),
        (
            stateless_headers(&sv.token, "tools/list", None),
            body("tools/call", json!({"name": "chain_head", "arguments": {}})),
        ),
        (
            stateless_headers(&sv.token, "tools/call", Some("node_status")),
            body("tools/call", json!({"name": "chain_head", "arguments": {}})),
        ),
        (
            stateless_headers(&sv.token, "tools/call", None),
            body("tools/call", json!({"name": "chain_head", "arguments": {}})),
        ),
    ] {
        let (st, _, rb) = http(sv.port, "POST", &headers, &b);
        assert_eq!(st, 400, "{rb}");
        assert!(rb.contains(&HEADER_MISMATCH.to_string()), "{rb}");
    }
    let mut wrong_version = stateless_headers(&sv.token, "tools/list", None);
    wrong_version[2].1 = "2025-06-18".into();
    let (st, _, rb) = http(sv.port, "POST", &wrong_version, &body("tools/list", json!({})));
    assert_eq!(st, 400);
    assert!(rb.contains(&HEADER_MISMATCH.to_string()), "{rb}");
    // An unknown method is 404 with a JSON-RPC body; an unsupported version is 400 + -32022.
    let (st, _, rb) = http(
        sv.port,
        "POST",
        &stateless_headers(&sv.token, "nope/nope", None),
        &body("nope/nope", json!({})),
    );
    assert_eq!(st, 404);
    assert!(rb.contains("-32601"), "{rb}");
    let mut old = stateless_headers(&sv.token, "tools/list", None);
    old[2].1 = "1900-01-01".into();
    let (st, _, rb) = http(
        sv.port,
        "POST",
        &old,
        &json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list", "params": {"_meta": {META_PROTOCOL_VERSION: "1900-01-01"}}}).to_string(),
    );
    assert_eq!(st, 400);
    assert!(rb.contains("-32022"), "{rb}");
    // A session-era request still needs its session.
    let (st, _, _) = http(
        sv.port,
        "POST",
        &[auth(&sv.token), json_ct()],
        &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list"}).to_string(),
    );
    assert_eq!(st, 400);
    sv.state.stop();
}

#[test]
fn the_stdio_shim_carries_stateless_requests_and_their_errors() {
    let sv = serve();
    let transport = UreqShimTransport {
        url: crate::node_mcp_http::endpoint_url(sv.port),
    };
    let meta = stateless_meta(true);
    let input = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "pin_add", "arguments": {"cid": "bafyabc"}, "_meta": meta}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "nope/nope", "params": {"_meta": meta}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n");
    let mut out = Vec::new();
    let code = run_stdio_shim(input.as_bytes(), &mut out, &transport, &sv.token);
    assert_eq!(code, 0);
    let lines: Vec<Value> = String::from_utf8(out)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["result"]["supportedVersions"][0], json!("2026-07-28"));
    assert_eq!(lines[1]["result"]["resultType"], json!("task"));
    // The server's own JSON-RPC error passes through (not rewrapped as an HTTP failure).
    assert_eq!(lines[2]["error"]["code"], json!(METHOD_NOT_FOUND));
    assert_eq!(lines[2]["id"], json!(3));
    sv.state.stop();
}

#[test]
fn hermes_gets_at_most_one_live_token_and_losing_the_switch_revokes_it() {
    let sv = serve();
    let label = crate::hermes_mcp::HERMES_NODE_TOKEN_LABEL;
    let first = sv.state.hermes_token(None).expect("issue");
    // No token in Hermes's file (or one that is not live): a new one replaces the earlier one.
    let second = sv.state.hermes_token(None).expect("issue");
    let hermes: Vec<_> = sv
        .state
        .status()
        .tokens
        .into_iter()
        .filter(|t| t.label == label)
        .collect();
    assert_eq!(hermes.len(), 1, "the earlier Hermes token was revoked");
    assert!(hermes[0].read_only);
    // The revoked token is refused at once; the new one works; the member's token is untouched.
    let (st, _, _) = http(sv.port, "POST", &[auth(&first), json_ct()], &init_body());
    assert_eq!(st, 401);
    let (st, _, _) = http(sv.port, "POST", &[auth(&second), json_ct()], &init_body());
    assert_eq!(st, 200);
    assert!(sv.state.status().tokens.iter().any(|t| t.label == "Claude Code"));
    // Switch off: no Hermes token remains.
    sv.state.revoke_hermes_tokens();
    assert!(!sv.state.status().tokens.iter().any(|t| t.label == label));
    sv.state.stop();
}

// ---------------------------------------------------------------------------
// fan-out 6: read-only token scope, deploy-and-confirm, ABI registry, Ed25519 helper
// ---------------------------------------------------------------------------

fn read_only_ctx(token: &str) -> CallerCtx {
    CallerCtx {
        token_id: token.into(),
        token_label: crate::hermes_mcp::HERMES_NODE_TOKEN_LABEL.into(),
        client_name: Some("agent-mcp-host".into()),
        read_only: true,
    }
}

#[test]
fn a_read_only_token_is_offered_only_read_tools() {
    let core = core_with(Arc::new(Fixture::default()));
    let r = rpc(&core, &read_only_ctx("h"), "tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().cloned().unwrap_or_default();
    assert!(!tools.is_empty());
    for t in &tools {
        assert_eq!(
            t["annotations"]["readOnlyHint"],
            json!(true),
            "a write tool was listed for a read-only token: {}",
            t["name"]
        );
    }
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for w in ["tx_propose", "deploy_propose", "pin_add", "anchor_propose", "cluster_join", "invite_create"] {
        assert!(!names.contains(&w), "{w} listed");
    }
    // A full token still sees every tool.
    let full = rpc(&core, &ctx("m"), "tools/list", json!({}));
    assert_eq!(
        full["result"]["tools"].as_array().map(Vec::len),
        Some(crate::node_mcp_tools::all_tools().count())
    );
}

#[test]
fn a_read_only_token_cannot_call_a_write_tool_and_nothing_is_queued() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = read_only_ctx("h");
    let tx = call(
        &core,
        &c,
        "tx_propose",
        json!({"to": "0x0000000000000000000000000000000000000002"}),
    );
    assert!(is_tool_error(&tx), "{tx}");
    assert!(tx["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("limited to the read tools"));
    let pin = call_meta(&core, &c, "pin_add", json!({"cid": "bafyabc"}), tasks_meta());
    assert!(is_tool_error(&pin), "{pin}");
    assert!(pin["result"].get("taskId").is_none());
    let dep = call(&core, &c, "deploy_propose", json!({"bytecode": "0x6000"}));
    assert!(is_tool_error(&dep));
    assert!(f.proposals.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    assert!(f.deploys.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    assert!(core.inbox().list(1).0.is_empty(), "no request was queued");
    // Read tools still answer.
    let head = call(&core, &c, "chain_head", json!({}));
    assert!(!is_tool_error(&head), "{head}");
}

#[test]
fn a_read_only_scope_persists_and_older_records_load_as_full_tokens() {
    let dir = std::env::temp_dir().join(format!("n6-node-mcp-scope-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("tokens.json");
    let s = TokenStore::load(path.clone());
    let ro = s.issue_scoped("Hermes (built-in)", true, 1).expect("issue");
    let full = s.issue("Cursor", 2).expect("issue");
    let again = TokenStore::load(path.clone());
    assert!(again.verify(&ro.connect_token, 3).map(|a| a.read_only) == Some(true));
    assert!(again.verify(&full.connect_token, 3).map(|a| a.read_only) == Some(false));
    let views = again.list();
    assert!(views.iter().any(|v| v.label == "Hermes (built-in)" && v.read_only));
    // A record written before scopes existed (no readOnly field) is a full token.
    let legacy = r#"[{"id":"0011aabb","label":"Old","sha256":"00","createdMs":1}]"#;
    std::fs::write(&path, legacy).expect("write");
    let old = TokenStore::load(path.clone());
    assert_eq!(old.list().len(), 1);
    assert!(!old.list()[0].read_only);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn members_cannot_issue_the_reserved_hermes_label_and_hermes_revocation_spares_member_tokens() {
    let sv = serve();
    for l in ["Hermes (built-in)", "  hermes (BUILT-IN) "] {
        let e = sv.state.create_token(l).expect_err("reserved");
        assert!(e.contains("reserved"), "{e}");
    }
    let _ = sv.state.hermes_token(None).expect("issue");
    sv.state.revoke_hermes_tokens();
    // The member's "Claude Code" token survives revoking Hermes's tokens.
    assert!(sv.state.status().tokens.iter().any(|t| t.label == "Claude Code"));
    sv.state.stop();
}

#[test]
fn hermes_token_over_http_is_read_only_whatever_the_client_asks() {
    let sv = serve();
    let label = crate::hermes_mcp::HERMES_NODE_TOKEN_LABEL;
    let h = sv.state.hermes_token(None).expect("issue");
    assert!(sv
        .state
        .status()
        .tokens
        .iter()
        .any(|t| t.label == label && t.read_only));
    let (st, hdrs, _) = http(sv.port, "POST", &[auth(&h), json_ct()], &init_body());
    assert_eq!(st, 200);
    let sid = hdrs.get("mcp-session-id").cloned().unwrap_or_default();
    let (st, _, body) = http(
        sv.port,
        "POST",
        &[auth(&h), json_ct(), ("Mcp-Session-Id", sid.clone())],
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "pin_add", "arguments": {"cid": "bafyabc"}}}).to_string(),
    );
    assert_eq!(st, 200);
    assert!(body.contains("limited to the read tools"), "{body}");
    assert_eq!(sv.state.status().pending_requests, 0);
    let (_, _, list) = http(
        sv.port,
        "POST",
        &[auth(&h), json_ct(), ("Mcp-Session-Id", sid)],
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}).to_string(),
    );
    assert!(!list.contains("\"tx_propose\"") && list.contains("\"chain_head\""), "{list}");
    sv.state.stop();
}

/// An approved tx_propose whose ceremony returned `tx_hash`.
fn approved_tx(core: &McpCore, c: &CallerCtx, tx_hash: &str) -> String {
    let r = call_meta(
        core,
        c,
        "tx_propose",
        json!({"to": "0x0000000000000000000000000000000000000002"}),
        tasks_meta(),
    );
    let id = r["result"]["taskId"].as_str().unwrap_or_default().to_string();
    core.inbox().begin_decision(&id, true, 2).expect("approve");
    core.inbox()
        .finish(&id, Ok(json!({"txHash": tx_hash, "blockNumber": null})), 3);
    id
}

const TX_HASH: &str = "0x8f1c1b6a1f5d3b0d6a7a6ab8a5d0f0f2b8e8f1d3c3b2a1908172635445362718";

#[test]
fn an_approved_transaction_task_completes_only_once_it_is_in_a_block() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    let id = approved_tx(&core, &c, TX_HASH);
    // Sent, no receipt yet: still working, no result.
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id}));
    assert_eq!(g["result"]["status"], json!("working"), "{g}");
    assert!(g["result"].get("result").is_none());
    assert!(g["result"]["statusMessage"]
        .as_str()
        .unwrap_or_default()
        .contains("waiting for the chain"));
    let st = call(&core, &c, "request_status", json!({"id": id}));
    assert_eq!(
        st["result"]["structuredContent"]["confirmation"]["state"],
        json!("waiting")
    );
    // Included (a deploy: the receipt names the new contract).
    *f.receipt.lock().unwrap_or_else(|e| e.into_inner()) = Some(json!({
        "transactionHash": TX_HASH, "status": "0x1", "blockNumber": "0x11",
        "contractAddress": "0x00000000000000000000000000000000000000C0", "gasUsed": "0x5208"
    }));
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id}));
    assert_eq!(g["result"]["status"], json!("completed"), "{g}");
    let sc = &g["result"]["result"]["structuredContent"];
    assert_eq!(sc["txHash"], json!(TX_HASH));
    assert_eq!(sc["receipt"]["blockNumber"], json!(17));
    assert_eq!(sc["receipt"]["status"], json!("success"));
    assert_eq!(
        sc["receipt"]["contractAddress"],
        json!("0x00000000000000000000000000000000000000c0")
    );
    // The receipt was read with the read-only method, for exactly this hash.
    let calls = f.rpc_calls.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(calls
        .iter()
        .any(|(m, p)| m == "eth_getTransactionReceipt" && p == &json!([TX_HASH])));
}

#[test]
fn a_reverted_transaction_task_completes_as_a_tool_error() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    let id = approved_tx(&core, &c, TX_HASH);
    *f.receipt.lock().unwrap_or_else(|e| e.into_inner()) = Some(json!({
        "transactionHash": TX_HASH, "status": "0x0", "blockNumber": "0x12", "gasUsed": "0x1"
    }));
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id}));
    assert_eq!(g["result"]["status"], json!("completed"));
    assert_eq!(g["result"]["result"]["isError"], json!(true));
    assert!(g["result"]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("reverted"));
    let st = call(&core, &c, "request_status", json!({"id": id}));
    assert_eq!(
        st["result"]["structuredContent"]["confirmation"]["state"],
        json!("reverted")
    );
}

#[test]
fn a_malformed_receipt_or_hash_is_never_reported_as_included() {
    assert!(crate::node_mcp_protocol::receipt_view(&json!({"status": "0x1"}), "x").is_none());
    assert!(crate::node_mcp_protocol::receipt_view(
        &json!({"transactionHash": TX_HASH, "status": "0x7", "blockNumber": "0x1"}),
        "x"
    )
    .is_none());
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    *f.receipt.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(json!({"transactionHash": TX_HASH, "status": "0x1"}));
    let id = approved_tx(&core, &c, TX_HASH);
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id}));
    assert_eq!(g["result"]["status"], json!("working"));
    // A result with no usable hash is left as the plain approved result (no chain read).
    let id2 = approved_tx(&core, &c, "not-a-hash");
    let before = f.rpc_calls.lock().unwrap_or_else(|e| e.into_inner()).len();
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": id2}));
    assert_eq!(g["result"]["status"], json!("completed"));
    assert_eq!(f.rpc_calls.lock().unwrap_or_else(|e| e.into_inner()).len(), before);
    // Action requests are never confirmed against the chain.
    let p = call_meta(&core, &c, "pin_add", json!({"cid": "bafyabc"}), tasks_meta());
    let pid = p["result"]["taskId"].as_str().unwrap_or_default().to_string();
    core.inbox().begin_decision(&pid, true, 2).expect("approve");
    core.inbox()
        .finish(&pid, Ok(json!({"txHash": TX_HASH})), 3);
    let g = rpc(&core, &c, "tasks/get", json!({"taskId": pid}));
    assert_eq!(g["result"]["status"], json!("completed"));
    assert!(g["result"]["result"]["structuredContent"].get("receipt").is_none());
}

#[test]
fn the_abi_registry_resource_reads_verified_sources_for_a_valid_address_only() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    let t = rpc(&core, &c, "resources/templates/list", json!({}));
    let templates: Vec<String> = t["result"]["resourceTemplates"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|x| x["uriTemplate"].as_str().map(str::to_string))
        .collect();
    assert!(templates.contains(&"citrate://contract/{address}/abi".to_string()));
    let r = rpc(
        &core,
        &c,
        "resources/read",
        json!({"uri": "citrate://contract/0x00000000000000000000000000000000000000AB/abi"}),
    );
    let text = r["result"]["contents"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("\"ping\"") && text.contains("verified"), "{r}");
    assert_eq!(
        f.abi_lookups.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        vec!["0x00000000000000000000000000000000000000ab".to_string()]
    );
    for bad in [
        "citrate://contract/0x1234/abi",
        "citrate://contract/0x00000000000000000000000000000000000000AB/source",
        "citrate://contract//abi",
    ] {
        let r = rpc(&core, &c, "resources/read", json!({"uri": bad}));
        assert_eq!(r["error"]["code"], json!(crate::node_mcp_protocol::RESOURCE_NOT_FOUND), "{bad}");
    }
    assert_eq!(f.abi_lookups.lock().unwrap_or_else(|e| e.into_inner()).len(), 1);
}

#[test]
fn the_abi_registry_entry_drops_the_source_text() {
    let vs = crate::contract_reader::VerifiedSource {
        status: crate::contract_reader::SourceStatus::Partial,
        is_contract: true,
        code_size: Some(10),
        contract_name: Some("Thing".into()),
        compiler_version: Some("0.8.30".into()),
        abi: Some(json!([])),
        source: Some("contract Thing {}".into()),
        note: Some("partial match".into()),
    };
    let v = crate::node_mcp_live::abi_entry_view("0xab", &vs);
    assert!(v.get("source").is_none());
    assert_eq!(v["sourceAvailable"], json!(true));
    assert_eq!(v["status"], json!("partial"));
    assert_eq!(v["address"], json!("0xab"));
}

#[test]
fn ed25519_verify_encodes_the_precompile_input_exactly() {
    let pk = format!("0x{}", "AB".repeat(32));
    let sig = format!("0x{}", "cd".repeat(64));
    let input = crate::node_mcp_tools::ed25519_verify_input(&pk, &sig, "0x68656C6C6F").expect("ok");
    assert_eq!(
        input,
        format!("0x{}{}68656c6c6f", "ab".repeat(32), "cd".repeat(64))
    );
    // Empty message is allowed (the precompile's minimum input is pubkey + signature).
    assert!(crate::node_mcp_tools::ed25519_verify_input(&pk, &sig, "0x").is_ok());
    // Wrong lengths and too-long messages are refused before any call.
    assert!(crate::node_mcp_tools::ed25519_verify_input("0xab", &sig, "0x").is_err());
    assert!(crate::node_mcp_tools::ed25519_verify_input(&pk, &format!("0x{}", "cd".repeat(63)), "0x").is_err());
    let long = format!("0x{}", "00".repeat(crate::node_mcp_tools::ED25519_MAX_MESSAGE_BYTES + 1));
    assert!(crate::node_mcp_tools::ed25519_verify_input(&pk, &sig, &long).is_err());
    let max = format!("0x{}", "00".repeat(crate::node_mcp_tools::ED25519_MAX_MESSAGE_BYTES));
    assert!(crate::node_mcp_tools::ed25519_verify_input(&pk, &sig, &max).is_ok());
}

#[test]
fn ed25519_verify_calls_0x0120_and_reads_its_answer_word() {
    let f = Arc::new(Fixture::default());
    let core = core_with(f.clone());
    let c = ctx("t");
    let args = json!({"public_key": format!("0x{}", "11".repeat(32)), "signature": format!("0x{}", "22".repeat(64)), "message": "0x01"});
    *f.ed25519_word.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("0x{}1", "0".repeat(63)));
    let ok = call(&core, &c, "ed25519_verify", args.clone());
    assert_eq!(ok["result"]["structuredContent"]["valid"], json!(true), "{ok}");
    let calls = f.rpc_calls.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (m, p) = calls.last().cloned().unwrap_or_default();
    assert_eq!(m, "eth_call");
    assert_eq!(p[0]["to"], json!(crate::node_mcp_tools::precompile_address(0x0120)));
    assert_eq!(
        p[0]["data"],
        json!(format!("0x{}{}01", "11".repeat(32), "22".repeat(64)))
    );
    *f.ed25519_word.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("0x{}", "0".repeat(64)));
    let no = call(&core, &c, "ed25519_verify", args.clone());
    assert_eq!(no["result"]["structuredContent"]["valid"], json!(false));
    // Not a 32-byte word (precompile inactive or a different answer): an error, never "valid".
    *f.ed25519_word.lock().unwrap_or_else(|e| e.into_inner()) = Some("0x01".into());
    let bad = call(&core, &c, "ed25519_verify", args);
    assert!(is_tool_error(&bad), "{bad}");
}

// ---------------------------------------------------------------------------
// the shim sends the connect token only to a server that proves it is Citrate Core
// ---------------------------------------------------------------------------

/// Something else listening on the port: answers every request, records every header it saw.
struct Squatter(std::sync::Mutex<Vec<Vec<(String, String)>>>);

impl crate::node_mcp_http::ShimTransport for Squatter {
    fn post(
        &self,
        _body: &str,
        headers: &[(String, String)],
    ) -> Result<(u16, HashMap<String, String>, String), String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(headers.to_vec());
        let mut h = HashMap::new();
        // Even an answer that looks like an identity proof is not one.
        h.insert(
            crate::node_mcp_http::IDENTITY_HEADER.to_string(),
            "00".repeat(32),
        );
        Ok((200, h, json!({"jsonrpc": "2.0", "id": 1, "result": {}}).to_string()))
    }
}

#[test]
fn the_shim_never_sends_the_token_to_a_server_that_cannot_prove_it_is_core() {
    let squatter = Squatter(std::sync::Mutex::new(Vec::new()));
    let token = format!("{}{}", crate::node_mcp_token::TOKEN_PREFIX, "ab".repeat(32));
    let mut out: Vec<u8> = Vec::new();
    let input = format!("{}\n{}\n", init_body(), json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    run_stdio_shim(std::io::Cursor::new(input), &mut out, &squatter, &token);
    let seen = squatter.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(!seen.is_empty(), "the shim asked the server to prove itself");
    for hs in &seen {
        for (k, v) in hs {
            assert!(!k.eq_ignore_ascii_case("authorization"), "token sent: {v}");
            assert!(!v.contains(&token));
        }
    }
    let text = String::from_utf8(out).unwrap_or_default();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.contains("was not sent"), "{text}");
}

/// Core answers the first request, then quits and another program takes the port.
struct CoreThenSquatter {
    token: String,
    authorized_answered: std::sync::Mutex<usize>,
    after_switch: std::sync::Mutex<Vec<Vec<(String, String)>>>,
}

impl crate::node_mcp_http::ShimTransport for CoreThenSquatter {
    fn post(
        &self,
        _body: &str,
        headers: &[(String, String)],
    ) -> Result<(u16, HashMap<String, String>, String), String> {
        let mut answered = self.authorized_answered.lock().unwrap_or_else(|e| e.into_inner());
        let mut h = HashMap::new();
        if *answered >= 1 {
            // Core is gone: whatever holds the port now records what it is sent.
            self.after_switch
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(headers.to_vec());
            h.insert(
                crate::node_mcp_http::IDENTITY_HEADER.to_string(),
                "00".repeat(32),
            );
            return Ok((200, h, json!({"jsonrpc": "2.0", "id": 2, "result": {}}).to_string()));
        }
        let challenge = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(crate::node_mcp_http::IDENTITY_CHALLENGE_HEADER));
        if let Some((_, ch)) = challenge {
            let mut nonce = [0u8; 32];
            hex::decode_to_slice(ch, &mut nonce).map_err(|e| e.to_string())?;
            h.insert(
                crate::node_mcp_http::IDENTITY_HEADER.to_string(),
                crate::node_mcp_token::identity_proof_for(&self.token, &nonce),
            );
            return Ok((204, h, String::new()));
        }
        *answered += 1;
        Ok((200, h, json!({"jsonrpc": "2.0", "id": 1, "result": {}}).to_string()))
    }
}

#[test]
fn the_shim_checks_the_server_before_every_request_not_only_the_first() {
    let token = format!("{}{}", crate::node_mcp_token::TOKEN_PREFIX, "ab".repeat(32));
    let t = CoreThenSquatter {
        token: token.clone(),
        authorized_answered: std::sync::Mutex::new(0),
        after_switch: std::sync::Mutex::new(Vec::new()),
    };
    let mut out: Vec<u8> = Vec::new();
    let input = format!(
        "{}\n{}\n",
        init_body(),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    );
    run_stdio_shim(std::io::Cursor::new(input), &mut out, &t, &token);
    assert_eq!(*t.authorized_answered.lock().unwrap_or_else(|e| e.into_inner()), 1);
    let seen = t.after_switch.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(!seen.is_empty(), "the second request reached the new listener");
    for hs in &seen {
        for (k, v) in hs {
            assert!(!k.eq_ignore_ascii_case("authorization"), "token sent: {v}");
            assert!(!v.contains(&token));
        }
    }
    let text = String::from_utf8(out).unwrap_or_default();
    assert!(text.contains("was not sent"), "{text}");
}

#[test]
fn the_identity_proof_is_bound_to_the_token_and_the_challenge() {
    let store = crate::node_mcp_token::TokenStore::in_memory();
    let issued = store.issue("client", 0).expect("issue");
    let n1 = [7u8; 32];
    let n2 = [8u8; 32];
    let mine = crate::node_mcp_token::identity_proof_for(&issued.connect_token, &n1);
    assert!(store.identity_proofs(&n1).contains(&mine));
    assert!(!store.identity_proofs(&n2).contains(&mine), "a proof is for one challenge");
    let other = format!("{}{}", crate::node_mcp_token::TOKEN_PREFIX, "cd".repeat(32));
    assert!(!store
        .identity_proofs(&n1)
        .contains(&crate::node_mcp_token::identity_proof_for(&other, &n1)));
}

// ---------------------------------------------------------------------------
// sessions are bounded per client; reads never unlock the wallet
// ---------------------------------------------------------------------------

#[test]
fn one_client_cannot_push_out_another_clients_sessions() {
    use crate::node_mcp_http::{pick_eviction, MAX_SESSIONS_PER_TOKEN};
    let mk = |tok: &str, n: usize, from: u64| -> Vec<(String, String, u64)> {
        (0..n)
            .map(|i| (format!("{tok}-{i}"), tok.to_string(), from + i as u64))
            .collect()
    };
    // Under both caps: nobody goes.
    let few = mk("a", 3, 0);
    assert_eq!(pick_eviction(&few, "a"), None);
    // A client at its own cap loses its own oldest, never another's.
    let mut s = mk("b", 2, 0);
    s.extend(mk("a", MAX_SESSIONS_PER_TOKEN, 10));
    assert_eq!(pick_eviction(&s, "a"), Some("a-0".into()));
    // A full table: the busiest client gives way, not the newcomer's victim of choice.
    let mut full = Vec::new();
    for (i, t) in ["c", "d", "e", "f", "g", "h", "i", "j"].iter().enumerate() {
        full.extend(mk(t, MAX_SESSIONS_PER_TOKEN, (i * 100) as u64));
    }
    let victim = pick_eviction(&full, "k").expect("full");
    assert!(victim.ends_with("-0"));
}

#[test]
fn a_node_mcp_read_never_unlocks_the_wallet() {
    let src = include_str!("node_mcp_live.rs");
    assert!(!src.contains("address_auto_unlocked"), "reads must not auto-unlock the vault");
    assert!(!src.contains("ensure_auto_unlocked"));
}

#[test]
fn a_node_mcp_cluster_read_never_starts_the_group_daemon() {
    let src = include_str!("node_mcp_live.rs");
    for f in ["fn cluster_status(", "fn cluster_peers("] {
        let i = src.find(f).expect(f);
        let body = &src[i..i + 400];
        assert!(body.contains("is_daemon_running()"), "{f} must not start the daemon");
    }
}

// ---------------------------------------------------------------------------
// HUP-S4.1 (US-4.1 AC1): the built-in Hermes entry's connect token.

#[test]
fn an_ephemeral_token_verifies_is_never_written_and_revokes() {
    let dir = std::env::temp_dir().join(format!("n6-node-mcp-eph-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("tokens.json");
    let s = TokenStore::load(path.clone());
    let t = s.issue_ephemeral("Hermes in this app", false, 5).expect("issue");
    assert!(t.connect_token.starts_with(crate::node_mcp_token::TOKEN_PREFIX));
    assert!(s.verify(&t.connect_token, 6).is_some());
    assert!(s.is_live_ephemeral(&t.connect_token));
    assert!(!path.exists(), "an in-memory token never touches disk");
    // A persisted token issued later does not write the in-memory one either.
    let p = s.issue("Cursor", 7).expect("issue");
    let on_disk = std::fs::read_to_string(&path).expect("written");
    let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(
        t.connect_token.as_bytes(),
    ));
    assert!(!on_disk.contains(&digest), "{on_disk}");
    assert!(!s.is_live_ephemeral(&p.connect_token));
    assert_eq!(s.list().len(), 2);
    // The next app launch knows nothing of it.
    assert!(TokenStore::load(path.clone())
        .verify(&t.connect_token, 8)
        .is_none());
    assert!(s.revoke(&t.id).expect("revoke"));
    assert!(s.verify(&t.connect_token, 9).is_none());
    assert!(s.verify(&p.connect_token, 9).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_hermes_token_is_kept_while_live_and_rotated_otherwise() {
    let st = NodeMcpState::new(Arc::new(Fixture::default()), TokenStore::in_memory(), None);
    let t1 = st.hermes_token(None).expect("mint");
    assert_eq!(st.hermes_token(Some(&t1)).expect("keep"), t1);
    let t2 = st.hermes_token(Some("not-a-live-token")).expect("rotate");
    assert_ne!(t1, t2);
    assert!(
        st.shared().tokens.verify(&t1, 1).is_none(),
        "the earlier Hermes token is revoked"
    );
    assert!(st.shared().tokens.verify(&t2, 1).is_some());
    // A member's persisted token is never adopted as Hermes's.
    let member = st.create_token("Claude Code").expect("issue");
    let t3 = st.hermes_token(Some(&member.connect_token)).expect("mint");
    assert_ne!(t3, member.connect_token);
    assert!(st.shared().tokens.verify(&member.connect_token, 2).is_some());
    let views = st.status().tokens;
    assert!(views.iter().any(|v| v.label == HERMES_TOKEN_LABEL));
    st.revoke_hermes_tokens();
    assert!(st.shared().tokens.verify(&t3, 3).is_none());
    assert!(st.shared().tokens.verify(&member.connect_token, 3).is_some());
}

// ---------------------------------------------------------------------------
// Demo (manual), HUP-S4.1: Hermes's built-in `node` entry against the real server
// ---------------------------------------------------------------------------

/// Chain reads from the live public 40204 RPC; a proposed transaction is recorded as a harness
/// ceremony (nothing is signed: in the app the SignatureCeremony opens instead).
struct HermesDemoBackend {
    proposals: std::sync::Mutex<Vec<(String, String, u128)>>,
}

impl NodeBackend for HermesDemoBackend {
    fn node_status(&self) -> Result<Value, String> {
        PublicChainOnly.node_status()
    }
    fn rpc_read(&self, method: &str, params: Value) -> Result<RpcRead, String> {
        PublicChainOnly.rpc_read(method, params)
    }
    fn wallet_address(&self) -> Result<String, String> {
        PublicChainOnly.wallet_address()
    }
    fn memory_search(&self, t: &str, q: &str, n: usize) -> Result<Value, String> {
        PublicChainOnly.memory_search(t, q, n)
    }
    fn groups(&self) -> Result<Value, String> {
        PublicChainOnly.groups()
    }
    fn cluster_status(&self, g: &str) -> Result<Value, String> {
        PublicChainOnly.cluster_status(g)
    }
    fn cluster_peers(&self, g: &str) -> Result<Value, String> {
        PublicChainOnly.cluster_peers(g)
    }
    fn invites(&self, g: &str) -> Result<Value, String> {
        PublicChainOnly.invites(g)
    }
    fn propose_transaction(
        &self,
        origin: &str,
        to: &str,
        value_wei: u128,
        _data: &str,
    ) -> Result<ProposedSignature, String> {
        let mut p = self.proposals.lock().unwrap_or_else(|e| e.into_inner());
        p.push((origin.into(), to.into(), value_wei));
        let id = format!("harness-ceremony-{}", p.len());
        Ok(ProposedSignature {
            ceremony_id: id.clone(),
            ceremony: json!({"id": id, "origin": origin, "decoded": {"action": "Transfer", "destination": to, "valueWei": value_wei.to_string()}, "requiresRawAck": false, "note": "demo harness: nothing is signed"}),
        })
    }
    fn close_ceremony(&self, _: &str) {}
    fn devices(&self) -> Result<Value, String> {
        Err("not in this demo".into())
    }
    fn pins(&self) -> Result<Value, String> {
        Err("not in this demo".into())
    }
    fn propose_deploy(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u128,
        _: Option<u64>,
    ) -> Result<ProposedSignature, String> {
        Err("not in this demo".into())
    }
    fn anchor_ready(&self) -> Result<(), String> {
        Err("not in this demo".into())
    }
    fn contract_abi(&self, _: &str) -> Result<Value, String> {
        Err("not in this demo".into())
    }
}

/// Run with `cargo test --lib hermes_node_entry_demo -- --ignored --nocapture` and
/// `HERMES_NODE_DEMO_SHIM=<a citrate-core binary>`, `HERMES_NODE_DEMO_ALLOWLIST=<file>`. Starts the
/// real node MCP server, mints Hermes's in-memory token, writes the allowlist core would give
/// Hermes (the built-in `node` entry), serves for `HERMES_NODE_DEMO_HOLD_SECS` (default 60) while
/// an external agent-mcp-host connects through the stdio shim, then prints the approval inbox.
#[test]
#[ignore = "manual demo: needs network access to rpc.citrate.ai and a citrate-core binary"]
fn hermes_node_entry_demo() {
    let backend = Arc::new(HermesDemoBackend {
        proposals: std::sync::Mutex::new(Vec::new()),
    });
    let state = NodeMcpState::new(backend.clone(), TokenStore::in_memory(), None);
    let port = state.start_on(47298).expect("bind");
    let token = state.hermes_token(None).expect("mint");
    let exe = std::path::PathBuf::from(std::env::var("HERMES_NODE_DEMO_SHIM").expect("shim exe"));
    let settings = crate::hermes_mcp::McpSettings {
        mem: false,
        scan: false,
        node: true,
    };
    let target = crate::hermes_mcp::NodeTarget { exe, port, token };
    let cfg = crate::hermes_mcp::render_config(&settings, None, Some(&target)).expect("node entry");
    let out = std::env::var("HERMES_NODE_DEMO_ALLOWLIST").expect("allowlist path");
    crate::node_mcp_token::write_private(
        std::path::Path::new(&out),
        serde_json::to_string_pretty(&cfg).expect("json").as_bytes(),
    )
    .expect("write");
    let redacted = cfg.to_string().replace(&target.token, "cnmcp_<minted for Hermes, elided>");
    println!("allowlist core wrote for Hermes: {redacted}");
    println!("serving {} ; tokens: {:?}", crate::node_mcp_http::endpoint_url(port), state.status().tokens.iter().map(|t| t.label.clone()).collect::<Vec<_>>());
    let secs: u64 = std::env::var("HERMES_NODE_DEMO_HOLD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < until {
        if !state.requests().is_empty() && std::env::var("HERMES_NODE_DEMO_STOP_ON_REQUEST").is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(500));
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    for r in state.requests() {
        println!("approval inbox: {}", serde_json::to_string(&r).expect("json"));
    }
    println!("recent calls: {}", serde_json::to_string(&state.status().recent_calls).expect("json"));
    println!("harness ceremonies opened: {:?}", backend.proposals.lock().map(|p| p.clone()).unwrap_or_default());
    state.revoke_hermes_tokens();
    state.stop();
}

/// Stack merge (node MCP lane + MCP host lane): the stdio shim checks the server's identity before
/// it sends a token, so the server must prove it holds Hermes's in-memory token as well as the
/// member's saved ones; otherwise Hermes's shim would never send its token.
#[test]
fn the_server_proves_it_holds_hermes_in_memory_token_to_the_shim() {
    let s = TokenStore::in_memory();
    let member = s.issue("Claude Code", 1).expect("issue");
    let hermes = s
        .issue_ephemeral(crate::hermes_mcp::HERMES_NODE_TOKEN_LABEL, true, 2)
        .expect("issue");
    let nonce = [7u8; 32];
    let proofs = s.identity_proofs(&nonce);
    for t in [&member.connect_token, &hermes.connect_token] {
        let p = crate::node_mcp_token::identity_proof_for(t, &nonce);
        assert!(proofs.contains(&p), "a proof for every live token");
    }
    assert!(s.verify(&hermes.connect_token, 3).is_some_and(|a| a.read_only));
}
