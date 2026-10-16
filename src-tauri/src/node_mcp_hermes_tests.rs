// HUP-S1.1 (g1-loop) — Hermes sessions over the citrate-node MCP server: the catalog and its
// annotations, the read tools (bounded, redacted), the write tools (approval inbox only), and the
// app's Hermes client underneath.

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
use crate::node_mcp_approvals::{ApprovalInbox, RequestKind, RequestState};
use crate::node_mcp_protocol::{CallerCtx, McpCore, NodeBackend, ProposedSignature, RpcRead};
use std::sync::{Arc, Mutex};

/// A node backend with only Hermes sessions behind it (every other source is absent).
#[derive(Default)]
struct HermesFixture {
    sessions: Option<Value>,
    page: Option<Value>,
    events_calls: Mutex<Vec<(String, u64, u64)>>,
}

impl NodeBackend for HermesFixture {
    fn node_status(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn rpc_read(&self, _m: &str, _p: Value) -> Result<RpcRead, String> {
        Err("fixture: absent".into())
    }
    fn wallet_address(&self) -> Result<String, String> {
        Err("fixture: absent".into())
    }
    fn memory_search(&self, _t: &str, _q: &str, _l: usize) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn groups(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn cluster_status(&self, _g: &str) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn cluster_peers(&self, _g: &str) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn invites(&self, _g: &str) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn propose_transaction(&self, _o: &str, _t: &str, _v: u128, _d: &str) -> Result<ProposedSignature, String> {
        Err("fixture: absent".into())
    }
    fn close_ceremony(&self, _id: &str) {}
    fn devices(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn pins(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn propose_deploy(&self, _o: &str, _b: &str, _c: &str, _v: u128, _g: Option<u64>) -> Result<ProposedSignature, String> {
        Err("fixture: absent".into())
    }
    fn anchor_ready(&self) -> Result<(), String> {
        Err("fixture: absent".into())
    }
    fn contract_abi(&self, _a: &str) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn hermes_sessions(&self) -> Result<Value, String> {
        self.sessions.clone().ok_or_else(|| HERMES_UNAVAILABLE.to_string())
    }
    fn hermes_events(&self, session: &str, after: u64, wait_ms: u64) -> Result<Value, String> {
        self.events_calls.lock().unwrap_or_else(|e| e.into_inner()).push((session.into(), after, wait_ms));
        self.page.clone().ok_or_else(|| HERMES_UNAVAILABLE.to_string())
    }
}

/// A backend that implements none of the Hermes methods (the trait defaults apply).
struct NoHermes;
impl NodeBackend for NoHermes {
    fn node_status(&self) -> Result<Value, String> {
        Err("x".into())
    }
    fn rpc_read(&self, _m: &str, _p: Value) -> Result<RpcRead, String> {
        Err("x".into())
    }
    fn wallet_address(&self) -> Result<String, String> {
        Err("x".into())
    }
    fn memory_search(&self, _t: &str, _q: &str, _l: usize) -> Result<Value, String> {
        Err("x".into())
    }
    fn groups(&self) -> Result<Value, String> {
        Err("x".into())
    }
    fn cluster_status(&self, _g: &str) -> Result<Value, String> {
        Err("x".into())
    }
    fn cluster_peers(&self, _g: &str) -> Result<Value, String> {
        Err("x".into())
    }
    fn invites(&self, _g: &str) -> Result<Value, String> {
        Err("x".into())
    }
    fn propose_transaction(&self, _o: &str, _t: &str, _v: u128, _d: &str) -> Result<ProposedSignature, String> {
        Err("x".into())
    }
    fn close_ceremony(&self, _id: &str) {}
    fn devices(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn pins(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn propose_deploy(&self, _o: &str, _b: &str, _c: &str, _v: u128, _g: Option<u64>) -> Result<ProposedSignature, String> {
        Err("fixture: absent".into())
    }
    fn anchor_ready(&self) -> Result<(), String> {
        Err("fixture: absent".into())
    }
    fn contract_abi(&self, _a: &str) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
}

fn ctx() -> CallerCtx {
    CallerCtx { token_id: "tok1".into(), token_label: "Claude Code".into(), client_name: Some("claude-code".into()), read_only: false }
}

fn call(core: &McpCore, name: &str, args: Value) -> Value {
    core.dispatch(&ctx(), &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": name, "arguments": args}}))
        .expect("a reply")
}

/// The tool's JSON answer (the text content parsed), or the error text.
fn answer(reply: &Value) -> Result<Value, String> {
    let text = reply["result"]["content"][0]["text"].as_str().unwrap_or("").to_string();
    if reply["result"]["isError"] == json!(true) {
        Err(text)
    } else {
        serde_json::from_str(&text).map_err(|e| format!("{e}: {text}"))
    }
}

fn page() -> Value {
    json!({
        "events": [
            {"seq": 3, "event": {"type": "step_start", "step": 1}},
            {"seq": 4, "event": {"type": "tool_call", "step": 1, "host": "core", "call": {"id": "c1", "name": "memory_recall", "arguments": "{\"q\":\"my doctor\"}"}}},
            {"seq": 5, "event": {"type": "tool_result", "step": 1, "call_id": "c1", "status": "ok", "content": "personal: Dr. Example, Tuesdays"}},
            {"seq": 6, "event": {"type": "assistant_delta", "step": 2, "text": "Done "}},
            {"seq": 7, "event": {"type": "final", "content": "Done for today."}},
            {"seq": 8, "event": {"type": "done", "outcome": "answered"}}
        ],
        "lastSeq": 8,
        "busy": false,
        "pendingCoreCalls": []
    })
}

// --- catalog ------------------------------------------------------------------------------

#[test]
fn the_four_session_tools_are_listed_with_honest_annotations() {
    let core = McpCore::new(Arc::new(HermesFixture::default()), Arc::new(ApprovalInbox::new()));
    let r = core.dispatch(&ctx(), &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).expect("reply");
    let tools = r["result"]["tools"].as_array().expect("array").clone();
    let find = |n: &str| tools.iter().find(|t| t["name"] == n).cloned().unwrap_or_else(|| panic!("missing {n}"));
    for read in ["hermes_session_list", "hermes_session_events"] {
        let t = find(read);
        assert_eq!(t["annotations"]["readOnlyHint"], json!(true), "{read}");
        assert!(t["annotations"].get("destructiveHint").is_none(), "{read}");
    }
    for write in ["hermes_session_send", "hermes_session_stop"] {
        let t = find(write);
        assert_eq!(t["annotations"]["readOnlyHint"], json!(false), "{write} is not read-only");
        assert_eq!(t["annotations"]["destructiveHint"], json!(true), "{write}");
        assert!(t["description"].as_str().unwrap_or("").contains("approves it in Citrate Core"), "{write}");
    }
    for t in &tools {
        assert_eq!(t["inputSchema"]["additionalProperties"], json!(false), "{}", t["name"]);
    }
    // The node catalog is still all there, in front.
    assert_eq!(tools[0]["name"], "node_status");
    assert_eq!(tools.len(), crate::node_mcp_tools::TOOLS.len() + HERMES_TOOLS.len());
}

// --- read tools ---------------------------------------------------------------------------

#[test]
fn the_session_list_comes_from_the_sidecar() {
    let f = HermesFixture {
        sessions: Some(json!({"sessions": [{"id": "s1-ab", "model": "gemma", "busy": true, "lastSeq": 8, "persona": null, "pendingCoreCalls": []}]})),
        ..Default::default()
    };
    let core = McpCore::new(Arc::new(f), Arc::new(ApprovalInbox::new()));
    let v = answer(&call(&core, "hermes_session_list", json!({}))).expect("ok");
    assert_eq!(v["sessions"][0]["id"], "s1-ab");
    assert_eq!(v["sessions"][0]["busy"], true);
}

#[test]
fn events_withhold_tool_arguments_and_results_and_keep_the_conversation() {
    let f = Arc::new(HermesFixture { page: Some(page()), ..Default::default() });
    let core = McpCore::new(f.clone(), Arc::new(ApprovalInbox::new()));
    let v = answer(&call(&core, "hermes_session_events", json!({"session": "s1-ab", "since_seq": 2, "wait_ms": 500}))).expect("ok");
    let evs = v["events"].as_array().unwrap();
    assert_eq!(evs.len(), 6);
    assert_eq!(evs[1]["event"]["call"]["name"], "memory_recall", "the tool name is shown");
    assert_eq!(evs[1]["event"]["call"]["arguments"], WITHHELD);
    assert_eq!(evs[2]["event"]["status"], "ok");
    assert_eq!(evs[2]["event"]["content"], WITHHELD);
    let text = v.to_string();
    assert!(!text.contains("Dr. Example") && !text.contains("my doctor"), "{text}");
    assert_eq!(evs[3]["event"]["text"], "Done ");
    assert_eq!(evs[4]["event"]["content"], "Done for today.");
    assert_eq!(v["nextAfter"], 8);
    assert_eq!(v["more"], false);
    assert_eq!(*f.events_calls.lock().unwrap(), vec![("s1-ab".to_string(), 2, 500)]);
}

#[test]
fn events_after_since_seq_only_and_a_long_page_continues_from_next_after() {
    let events: Vec<Value> = (1..=250).map(|i| json!({"seq": i, "event": {"type": "assistant_delta", "step": 1, "text": "x"}})).collect();
    let f = HermesFixture { page: Some(json!({"events": events, "lastSeq": 250, "busy": true, "pendingCoreCalls": ["c9"]})), ..Default::default() };
    let core = McpCore::new(Arc::new(f), Arc::new(ApprovalInbox::new()));
    let v = answer(&call(&core, "hermes_session_events", json!({"session": "s1-ab", "since_seq": 10}))).expect("ok");
    let evs = v["events"].as_array().unwrap();
    assert_eq!(evs.len(), MAX_EVENTS);
    assert_eq!(evs[0]["seq"], 11, "events at or below since_seq are never repeated");
    assert_eq!(v["nextAfter"], 10 + MAX_EVENTS as u64);
    assert_eq!(v["more"], true);
    assert_eq!(v["lastSeq"], 250);
    assert_eq!(v["busy"], true);
    assert_eq!(v["waitingOnAppTools"], 1);
}

#[test]
fn an_empty_poll_keeps_the_client_where_it_was() {
    let f = HermesFixture { page: Some(json!({"events": [], "lastSeq": 8, "busy": false})), ..Default::default() };
    let core = McpCore::new(Arc::new(f), Arc::new(ApprovalInbox::new()));
    let v = answer(&call(&core, "hermes_session_events", json!({"session": "s1-ab", "since_seq": 8}))).expect("ok");
    assert_eq!(v["events"], json!([]));
    assert_eq!(v["nextAfter"], 8);
}

#[test]
fn very_long_text_is_cut_over_mcp() {
    let long = "a".repeat(MAX_EVENT_TEXT_CHARS + 50);
    let ev = redact_event(&json!({"type": "final", "content": long}));
    let shown = ev["content"].as_str().unwrap();
    assert!(shown.ends_with("(cut over MCP)"));
    assert!(shown.chars().count() < MAX_EVENT_TEXT_CHARS + 30);
}

#[test]
fn event_arguments_are_checked_before_the_sidecar_is_asked() {
    let f = Arc::new(HermesFixture { page: Some(page()), ..Default::default() });
    let core = McpCore::new(f.clone(), Arc::new(ApprovalInbox::new()));
    for args in [
        json!({"session": "../stop"}),
        json!({"session": ""}),
        json!({"session": "s1-ab", "wait_ms": MAX_WAIT_MS + 1}),
        json!({"session": "s1-ab", "since_seq": -1}),
        json!({"session": "s1-ab", "since_seq": "3"}),
        json!({"session": "s1-ab", "extra": 1}),
        json!({}),
    ] {
        assert!(answer(&call(&core, "hermes_session_events", args.clone())).is_err(), "{args}");
    }
    assert!(f.events_calls.lock().unwrap().is_empty(), "nothing reached the sidecar");
}

#[test]
fn a_backend_without_hermes_says_so() {
    let core = McpCore::new(Arc::new(NoHermes), Arc::new(ApprovalInbox::new()));
    let e = answer(&call(&core, "hermes_session_list", json!({}))).unwrap_err();
    assert!(e.contains("not running"), "{e}");
    let e = answer(&call(&core, "hermes_session_events", json!({"session": "s1-ab"}))).unwrap_err();
    assert!(e.contains("not running"), "{e}");
}

// --- write tools: approval inbox only -------------------------------------------------------

#[test]
fn send_and_stop_wait_for_the_member_and_touch_nothing() {
    let f = Arc::new(HermesFixture { page: Some(page()), ..Default::default() });
    let inbox = Arc::new(ApprovalInbox::new());
    let core = McpCore::new(f.clone(), inbox.clone());
    let v = answer(&call(&core, "hermes_session_send", json!({"session": "s1-ab", "text": "What is my node height?"}))).expect("queued");
    assert_eq!(v["state"], "pending");
    assert!(v["requestId"].is_string());
    let v2 = answer(&call(&core, "hermes_session_stop", json!({"session": "s1-ab"}))).expect("queued");
    assert_eq!(v2["state"], "pending");
    let (reqs, _) = inbox.list(core.now());
    assert_eq!(reqs.len(), 2);
    let send = reqs
        .iter()
        .find(|r| matches!(&r.kind, RequestKind::Action { action: McpAction::HermesSessionSend { .. } }))
        .expect("the send request");
    match &send.kind {
        RequestKind::Action { action: McpAction::HermesSessionSend { session, text } } => {
            assert_eq!(session, "s1-ab");
            assert_eq!(text, "What is my node height?");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(send.summary.contains("s1-ab") && send.summary.contains("What is my node height?"), "{}", send.summary);
    assert!(reqs.iter().any(|r| matches!(&r.kind, RequestKind::Action { action: McpAction::HermesSessionStop { .. } })));
    assert!(reqs.iter().all(|r| r.state == RequestState::Pending));
    assert!(f.events_calls.lock().unwrap().is_empty(), "queuing reads nothing from the sidecar either");
    // The request is the caller's own and pollable.
    let s = answer(&call(&core, "request_status", json!({"id": v["requestId"]}))).expect("status");
    assert_eq!(s["state"], "pending");
}

#[test]
fn a_message_is_validated_before_it_is_queued() {
    let inbox = Arc::new(ApprovalInbox::new());
    let core = McpCore::new(Arc::new(HermesFixture::default()), inbox.clone());
    for args in [
        json!({"session": "s1-ab", "text": "   "}),
        json!({"session": "s1-ab", "text": "a".repeat(MAX_TEXT_CHARS + 1)}),
        json!({"session": "s1-ab", "text": "hi\u{202E}reversed"}),
        json!({"session": "s1-ab", "text": "bell\u{7}"}),
        json!({"session": "bad id", "text": "hi"}),
        json!({"session": "s1-ab"}),
    ] {
        assert!(answer(&call(&core, "hermes_session_send", args.clone())).is_err(), "{args}");
    }
    assert!(inbox.list(core.now()).0.is_empty());
    // Newlines and tabs are fine; the card shows one line.
    assert!(answer(&call(&core, "hermes_session_send", json!({"session": "s1-ab", "text": "line one\n\tline two"}))).is_ok());
    let (reqs, _) = inbox.list(core.now());
    assert!(reqs[0].summary.contains("line one  line two"), "{}", reqs[0].summary);
}

#[test]
fn the_approval_card_text_is_one_clipped_line_without_bidi_controls() {
    let s = summary_text(&format!("{}\u{2066}x", "w".repeat(400)));
    assert!(s.ends_with('…'));
    assert!(!s.contains('\u{2066}'));
    assert_eq!(summary_text("a\nb"), "a b");
}

// --- the app's Hermes client underneath ------------------------------------------------------

#[derive(Default)]
struct Rec {
    gets: Mutex<Vec<String>>,
    posts: Mutex<Vec<(String, String)>>,
    reply: Mutex<Vec<(u16, String)>>,
}
struct RecControl(Arc<Rec>);
impl HermesControl for RecControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.gets.lock().unwrap_or_else(|e| e.into_inner()).push(url.to_string());
        let (status, body) = self.0.reply.lock().unwrap_or_else(|e| e.into_inner()).pop().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
    fn post(&self, url: &str, _b: &str, body: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.posts.lock().unwrap_or_else(|e| e.into_inner()).push((url.to_string(), body.to_string()));
        let (status, body) = self.0.reply.lock().unwrap_or_else(|e| e.into_inner()).pop().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
}

fn mgr(rec: Arc<Rec>, started: bool) -> HermesManager {
    let dir = std::env::temp_dir().join(format!("n6mcp-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec)));
    if started {
        m.set_token_for_test("n6-test-bearer");
    }
    m
}

#[test]
fn an_approved_send_reaches_the_session_messages_route() {
    let rec = Arc::new(Rec::default());
    let m = mgr(rec.clone(), true);
    let v = run_action(&m, &McpAction::HermesSessionSend { session: "s1-ab".into(), text: "hello".into() }).expect("sent");
    assert_eq!(v["sent"], true);
    let posts = rec.posts.lock().unwrap().clone();
    assert!(posts[0].0.ends_with("/sessions/s1-ab/messages"), "{posts:?}");
    assert_eq!(serde_json::from_str::<Value>(&posts[0].1).unwrap()["text"], "hello");
    run_action(&m, &McpAction::HermesSessionStop { session: "s1-ab".into() }).expect("stopped");
    assert!(rec.posts.lock().unwrap()[1].0.ends_with("/sessions/s1-ab/stop"));
}

#[test]
fn a_busy_or_missing_session_and_a_stopped_sidecar_are_reported_plainly() {
    let rec = Arc::new(Rec::default());
    rec.reply.lock().unwrap().push((409, "busy".into()));
    let m = mgr(rec.clone(), true);
    let e = run_action(&m, &McpAction::HermesSessionSend { session: "s1-ab".into(), text: "x".into() }).unwrap_err();
    assert!(e.contains("already running a turn"), "{e}");
    rec.reply.lock().unwrap().push((404, "".into()));
    let e = ManagerSessions(&m).events("s1-ab", 0, 0).unwrap_err();
    assert!(e.contains("no Hermes session s1-ab"), "{e}");
    let stopped = mgr(Arc::new(Rec::default()), false);
    assert_eq!(run_action(&stopped, &McpAction::HermesSessionStop { session: "s1-ab".into() }).unwrap_err(), HERMES_UNAVAILABLE);
    assert_eq!(ManagerSessions(&stopped).list().unwrap_err(), HERMES_UNAVAILABLE);
    assert!(run_action(&m, &McpAction::ClusterJoin { group: "g".into() }).is_err());
}

#[test]
fn the_list_and_events_use_the_sidecar_routes_with_a_capped_wait() {
    let rec = Arc::new(Rec::default());
    let m = mgr(rec.clone(), true);
    rec.reply.lock().unwrap().push((200, json!({"sessions": []}).to_string()));
    assert_eq!(ManagerSessions(&m).list().unwrap(), json!({"sessions": []}));
    rec.reply.lock().unwrap().push((200, json!({"events": [], "lastSeq": 0, "busy": false}).to_string()));
    ManagerSessions(&m).events("s1-ab", 4, 60_000).unwrap();
    let gets = rec.gets.lock().unwrap().clone();
    assert!(gets[0].ends_with("/sessions"), "{gets:?}");
    assert!(gets[1].contains("/sessions/s1-ab/events?after=4&wait_ms=8000"), "{gets:?}");
}

// --- live proof (manual) --------------------------------------------------------------------

/// The app's Hermes client against a running sidecar, behind the real node MCP HTTP server.
struct LiveHermes(HermesManager);
impl NodeBackend for LiveHermes {
    fn node_status(&self) -> Result<Value, String> {
        Err("not part of this proof".into())
    }
    fn rpc_read(&self, _m: &str, _p: Value) -> Result<RpcRead, String> {
        Err("not part of this proof".into())
    }
    fn wallet_address(&self) -> Result<String, String> {
        Err("not part of this proof".into())
    }
    fn memory_search(&self, _t: &str, _q: &str, _l: usize) -> Result<Value, String> {
        Err("not part of this proof".into())
    }
    fn groups(&self) -> Result<Value, String> {
        Err("not part of this proof".into())
    }
    fn cluster_status(&self, _g: &str) -> Result<Value, String> {
        Err("not part of this proof".into())
    }
    fn cluster_peers(&self, _g: &str) -> Result<Value, String> {
        Err("not part of this proof".into())
    }
    fn invites(&self, _g: &str) -> Result<Value, String> {
        Err("not part of this proof".into())
    }
    fn propose_transaction(&self, _o: &str, _t: &str, _v: u128, _d: &str) -> Result<ProposedSignature, String> {
        Err("not part of this proof".into())
    }
    fn close_ceremony(&self, _id: &str) {}
    fn devices(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn pins(&self) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn propose_deploy(&self, _o: &str, _b: &str, _c: &str, _v: u128, _g: Option<u64>) -> Result<ProposedSignature, String> {
        Err("fixture: absent".into())
    }
    fn anchor_ready(&self) -> Result<(), String> {
        Err("fixture: absent".into())
    }
    fn contract_abi(&self, _a: &str) -> Result<Value, String> {
        Err("fixture: absent".into())
    }
    fn hermes_sessions(&self) -> Result<Value, String> {
        ManagerSessions(&self.0).list()
    }
    fn hermes_events(&self, session: &str, after: u64, wait_ms: u64) -> Result<Value, String> {
        ManagerSessions(&self.0).events(session, after, wait_ms)
    }
}

/// One MCP request over real loopback HTTP; returns (status, headers, body).
fn mcp_http(port: u16, headers: &[(&str, String)], body: &str) -> (u16, Vec<(String, String)>, String) {
    use std::io::{Read as _, Write as _};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
    let mut req = format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", body.len());
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
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let hs = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    (status, hs, body.to_string())
}

/// HUP-S1.1 proof: an MCP client reads the session the UI provider and the CLI drove, through
/// the real node MCP server (HTTP, connect token, MCP session) and the app's Hermes client.
/// Needs HERMES_LIVE_ADDR, HERMES_LIVE_TOKEN_FILE and HERMES_LIVE_SESSION.
#[test]
#[ignore = "manual proof: needs a running Hermes sidecar and a session id"]
fn live_mcp_client_reads_the_shared_session() {
    let addr = std::env::var("HERMES_LIVE_ADDR").expect("HERMES_LIVE_ADDR");
    let token = std::fs::read_to_string(std::env::var("HERMES_LIVE_TOKEN_FILE").expect("HERMES_LIVE_TOKEN_FILE")).expect("token");
    let session = std::env::var("HERMES_LIVE_SESSION").expect("HERMES_LIVE_SESSION");
    let dir = std::env::temp_dir().join(format!("n6-live-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control_addr(&addr);
    m.set_token_for_test(token.trim());
    let state = crate::node_mcp::NodeMcpState::new(Arc::new(LiveHermes(m)), crate::node_mcp_token::TokenStore::in_memory(), None);
    let connect = state.create_token("proof client").expect("token").connect_token;
    let port = state.start_on(0).expect("bind");
    let auth = ("Authorization", format!("Bearer {connect}"));
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "n6-proof", "version": "1"}}});
    let (st, hs, _) = mcp_http(port, std::slice::from_ref(&auth), &init.to_string());
    assert_eq!(st, 200);
    let sid = hs.iter().find(|(k, _)| k == "mcp-session-id").map(|(_, v)| v.clone()).expect("mcp session");
    let h = [auth, ("Mcp-Session-Id", sid)];
    let (_, _, list) = mcp_http(port, &h, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "hermes_session_list", "arguments": {}}}).to_string());
    let (_, _, events) = mcp_http(
        port,
        &h,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "hermes_session_events", "arguments": {"session": session, "since_seq": 0, "wait_ms": 1000}}}).to_string(),
    );
    let (_, _, send) = mcp_http(
        port,
        &h,
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "hermes_session_send", "arguments": {"session": session, "text": "Hello from an MCP client."}}}).to_string(),
    );
    println!("MCP_LIST {list}");
    println!("MCP_EVENTS {events}");
    println!("MCP_SEND {send}");
    let ev: Value = serde_json::from_str(&events).expect("json");
    assert_eq!(ev["result"]["isError"], json!(false), "{events}");
    assert!(ev["result"]["structuredContent"]["events"].as_array().is_some_and(|a| !a.is_empty()));
    let sv: Value = serde_json::from_str(&send).expect("json");
    assert_eq!(sv["result"]["structuredContent"]["state"], "pending", "a send waits for the member");
    assert_eq!(state.requests().len(), 1);
    state.stop();
}
