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
}

impl NodeBackend for Fixture {
    fn node_status(&self) -> Result<Value, String> {
        Ok(json!({"state": "running", "peers": 3, "height": 16, "syncPct": 100.0}))
    }
    fn rpc_read(&self, method: &str, params: Value) -> Result<RpcRead, String> {
        self.rpc_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((method.to_string(), params));
        let value = match method {
            "eth_blockNumber" => json!("0x10"),
            "eth_chainId" => json!("0x9d0c"),
            "eth_getBalance" => json!("0xde0b6b3a7640000"),
            "eth_call" => json!("0x01"),
            "eth_estimateGas" => json!("0x5208"),
            "eth_getLogs" => json!([]),
            "net_peerCount" => json!("0x3"),
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
}

fn ctx(token: &str) -> CallerCtx {
    CallerCtx {
        token_id: token.into(),
        token_label: "Claude Code".into(),
        client_name: Some("claude-code".into()),
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
            "cluster_join",
            "cluster_share",
            "invite_create",
            "invite_revoke",
            "faucet_request",
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
    assert!(v["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("HTTP 401"));
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
    };
    assert_eq!(c.origin(), "mcp:Claude Code via xy");
}
