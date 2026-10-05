// HUP-S1.1 (US-1.1 AC3) — core-hosted tool calls of a session the view is not watching are answered
// on the Tauri side: a read-only tool core serves runs here; anything else is declined at once with
// the reason; a call the view was shown stays the view's; a claimed call is hidden from the view.

use super::*;
use crate::hermes::{ControlResp, HermesControl};
use std::sync::Mutex as StdMutex;

/// A recorded node status (the shape `NodeState::status` serializes to), served by the test host.
const NODE_STATUS: &str = r#"{"state":"running","height":91234,"peers":5,"sync_pct":100.0}"#;

struct ReadHost;
impl CoreToolHost for ReadHost {
    fn run(&self, name: &str, _args: &Value) -> Option<Result<String, String>> {
        match name {
            "node_status" => Some(Ok(NODE_STATUS.to_string())),
            "groups_list" => Some(Err("the comms daemon is not running".to_string())),
            _ => None,
        }
    }
}

fn call_event(seq: u64, id: &str, name: &str, args: &str, hic: bool) -> Value {
    let mut ev = json!({
        "type": "tool_call",
        "call": {"id": id, "name": name, "arguments": args},
        "host": "core",
    });
    if hic {
        ev["hic"] = json!("required");
        ev["hic_reason"] = json!("read untrusted content");
    }
    json!({"seq": seq, "event": ev})
}

fn page(events: Vec<Value>, pending: &[&str]) -> Value {
    let last = events
        .iter()
        .filter_map(|e| e["seq"].as_u64())
        .max()
        .unwrap_or(0);
    json!({"events": events, "lastSeq": last, "busy": true, "pendingCoreCalls": pending})
}

#[test]
fn an_unwatched_session_has_its_pending_core_calls_claimed_once() {
    let leases = ViewLeases::new(Duration::from_secs(30));
    let now = Instant::now();
    let p = page(
        vec![
            json!({"seq": 1, "event": {"type": "step", "step": 1}}),
            call_event(2, "call_0", "node_status", "{}", false),
        ],
        &["call_0"],
    );
    let claimed = leases.claim("s-1", &p, now);
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].call_id, "call_0");
    assert_eq!(claimed[0].name, "node_status");
    assert_eq!(claimed[0].seq, 2);
    assert_eq!(claimed[0].arguments, json!({}));
    assert!(
        leases.claim("s-1", &p, now).is_empty(),
        "a claimed call is never run twice"
    );
}

#[test]
fn a_watched_session_keeps_its_calls_with_the_view() {
    let leases = ViewLeases::new(Duration::from_secs(30));
    let t0 = Instant::now();
    let p = page(
        vec![call_event(4, "call_1", "node_status", "{}", false)],
        &["call_1"],
    );
    // The view read the session's events just now: the lease is fresh.
    let _ = leases.view_page("s-2", page(vec![], &[]), t0);
    assert!(leases.watched("s-2", t0 + Duration::from_secs(29)));
    assert!(leases
        .claim("s-2", &p, t0 + Duration::from_secs(29))
        .is_empty());
    // Once the lease runs out, the call is claimed here.
    assert!(!leases.watched("s-2", t0 + Duration::from_secs(31)));
    assert_eq!(
        leases.claim("s-2", &p, t0 + Duration::from_secs(31)).len(),
        1
    );
}

#[test]
fn a_call_the_view_was_shown_stays_the_views_however_long_the_member_takes() {
    let leases = ViewLeases::new(Duration::from_secs(30));
    let t0 = Instant::now();
    let p = page(
        vec![call_event(7, "call_2", "group_create", r#"{"name":"x"}"#, false)],
        &["call_2"],
    );
    // The view read the page that announced the call, then sat on an approval card for an hour.
    let _ = leases.view_page("s-3", p.clone(), t0);
    let later = t0 + Duration::from_secs(3600);
    assert!(!leases.watched("s-3", later));
    assert!(
        leases.claim("s-3", &p, later).is_empty(),
        "the member's open decision is never taken from the view"
    );
    // The view answered it (and renewed the lease); the next call it unblocks stays the view's.
    leases.view_answered("s-3", "call_2", later);
    let next = page(
        vec![call_event(9, "call_3", "node_status", "{}", false)],
        &["call_3"],
    );
    assert!(leases
        .claim("s-3", &next, later + Duration::from_millis(1))
        .is_empty());
}

#[test]
fn a_claimed_call_is_hidden_from_the_view() {
    let leases = ViewLeases::new(Duration::from_secs(30));
    let now = Instant::now();
    let p = page(
        vec![call_event(3, "call_0", "node_status", "{}", false)],
        &["call_0"],
    );
    assert_eq!(leases.claim("s-4", &p, now).len(), 1);
    // The view comes back and reads the same page: the call is not a core call for it any more.
    let seen = leases.view_page("s-4", p, now);
    assert_eq!(seen["events"][0]["event"]["host"], HEADLESS_HOST);
    assert_eq!(
        seen["events"][0]["event"]["call"]["id"], "call_0",
        "the rest of the event is unchanged"
    );
}

#[test]
fn reused_call_ids_on_later_steps_are_new_calls() {
    let leases = ViewLeases::new(Duration::from_secs(30));
    let now = Instant::now();
    let first = page(
        vec![call_event(2, "call_0", "node_status", "{}", false)],
        &["call_0"],
    );
    assert_eq!(leases.claim("s-5", &first, now).len(), 1);
    // The sidecar recorded that result, then a later step reused the id.
    let second = page(
        vec![
            call_event(2, "call_0", "node_status", "{}", false),
            json!({"seq": 3, "event": {"type": "tool_result", "call_id": "call_0", "status": "ok"}}),
            call_event(5, "call_0", "groups_list", "{}", false),
        ],
        &["call_0"],
    );
    let c = leases.claim("s-5", &second, now);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].seq, c[0].name.as_str()), (5, "groups_list"));
}

#[test]
fn an_unclaimed_call_can_be_claimed_again() {
    let leases = ViewLeases::new(Duration::from_secs(30));
    let now = Instant::now();
    let p = page(
        vec![call_event(2, "call_0", "node_status", "{}", false)],
        &["call_0"],
    );
    let c = leases.claim("s-6", &p, now);
    leases.unclaim("s-6", c[0].seq);
    assert_eq!(leases.claim("s-6", &p, now).len(), 1);
}

#[test]
fn answers_run_only_reads_and_decline_the_rest_without_running_anything() {
    let mk = |name: &str, args: Value, hic: bool| Claimed {
        session: "s".into(),
        seq: 1,
        call_id: "c".into(),
        name: name.into(),
        arguments: args,
        hic_required: hic,
    };
    assert_eq!(
        answer(&ReadHost, &mk("node_status", json!({}), false)),
        ("ok", NODE_STATUS.to_string())
    );
    let (st, text) = answer(&ReadHost, &mk("groups_list", json!({}), false));
    assert_eq!(st, "error");
    assert!(text.contains("comms daemon"), "{text}");
    let (st, text) = answer(&ReadHost, &mk("group_create", json!({"name": "x"}), false));
    assert_eq!(st, "denied");
    assert!(text.contains("Nothing was done"), "{text}");
    // HIC: an explicit-approval call is never run here, even a read.
    let (st, text) = answer(&ReadHost, &mk("node_status", json!({}), true));
    assert_eq!(st, "denied");
    assert!(text.contains("explicit approval"), "{text}");
    let (st, _) = answer(&ReadHost, &mk("node_status", json!("[1]"), false));
    assert_eq!(st, "error");
}

#[test]
fn every_headless_tool_is_a_read_only_chat_tool() {
    // The tripwire in src/shell/agentToolGates.test.ts holds READ_ONLY_AGENT_TOOLS to tools that
    // reach no approval gate; answering anything else here would skip the member.
    let harness = include_str!("../../src/agent/harness.ts");
    let start = harness
        .find("export const READ_ONLY_AGENT_TOOLS")
        .expect("the read-only list");
    let block = &harness[start..start + harness[start..].find("]);").expect("end of the list")];
    for t in HEADLESS_TOOLS {
        assert!(
            block.contains(&format!("\"{t}\"")),
            "{t} is not in READ_ONLY_AGENT_TOOLS"
        );
    }
}

// ---- tick over the control API (recorded sidecar answers) --------------------------------------

#[derive(Default)]
struct Sidecar {
    gets: StdMutex<Vec<String>>,
    posts: StdMutex<Vec<(String, String)>>,
    sessions: StdMutex<String>,
    events: StdMutex<String>,
}
struct SidecarControl(Arc<Sidecar>);
impl HermesControl for SidecarControl {
    fn get(&self, url: &str, _b: &str) -> Result<ControlResp, HermesError> {
        self.0.gets.lock().unwrap().push(url.to_string());
        let body = if url.ends_with("/sessions") {
            self.0.sessions.lock().unwrap().clone()
        } else {
            self.0.events.lock().unwrap().clone()
        };
        Ok(ControlResp { status: 200, body })
    }
    fn post(&self, url: &str, _b: &str, body: &str) -> Result<ControlResp, HermesError> {
        self.0
            .posts
            .lock()
            .unwrap()
            .push((url.to_string(), body.to_string()));
        Ok(ControlResp {
            status: 200,
            body: "{}".into(),
        })
    }
}

fn recorded_mgr(sc: Arc<Sidecar>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!(
        "hheadless-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_control(Box::new(SidecarControl(sc)));
    m.set_token_for_test("deadbeef");
    m
}

#[test]
fn tick_answers_an_unwatched_sessions_call_and_leaves_a_watched_one() {
    let sc = Arc::new(Sidecar::default());
    *sc.sessions.lock().unwrap() = json!({"sessions": [
        {"id": "s-a", "model": "m", "busy": true, "lastSeq": 2, "pendingCoreCalls": ["call_0"]},
        {"id": "s-b", "model": "m", "busy": true, "lastSeq": 2, "pendingCoreCalls": ["call_0"]},
        {"id": "s-c", "model": "m", "busy": false, "lastSeq": 9, "pendingCoreCalls": []},
    ]})
    .to_string();
    *sc.events.lock().unwrap() = page(
        vec![call_event(2, "call_0", "node_status", "{}", false)],
        &["call_0"],
    )
    .to_string();
    let m = recorded_mgr(sc.clone());
    let leases = ViewLeases::new(Duration::from_secs(30));
    let now = Instant::now();
    // The view is driving s-b.
    let _ = leases.view_page("s-b", page(vec![], &[]), now);
    assert_eq!(tick(&leases, &m, &ReadHost, now).unwrap(), 1);
    let posts = sc.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 1);
    assert!(
        posts[0].0.ends_with("/sessions/s-a/tool_results"),
        "{posts:?}"
    );
    let body: Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(body["callId"], "call_0");
    assert_eq!(body["status"], "ok");
    assert_eq!(body["content"], NODE_STATUS);
    let gets = sc.gets.lock().unwrap().clone();
    assert!(
        !gets.iter().any(|g| g.contains("/sessions/s-b/")),
        "a watched session's events are not even read: {gets:?}"
    );
    assert!(
        !gets.iter().any(|g| g.contains("/sessions/s-c/")),
        "a session with nothing pending is not read: {gets:?}"
    );
    // A second pass does not answer the same call again.
    assert_eq!(tick(&leases, &m, &ReadHost, now).unwrap(), 0);
}

#[test]
fn tick_is_quiet_while_the_sidecar_is_not_running() {
    let dir = std::env::temp_dir().join(format!("hheadless-off-{}", std::process::id()));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    let leases = ViewLeases::new(Duration::from_secs(30));
    assert_eq!(tick(&leases, &m, &ReadHost, Instant::now()).unwrap(), 0);
}

// ---- The real sidecar process, no view ----------------------------------------------------------

/// A loopback OpenAI-compatible stand-in for the model: the first request asks for `node_status`,
/// the request that carries its result gets a final answer quoting it. Answers streamed requests
/// as server-sent events. `/tokenize` and `/v1/embeddings` answer 404, so the session estimates
/// tokens and ranks lexically (and says so).
fn model_stand_in() -> (String, Arc<StdMutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let seen = seen2.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    return;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = reader.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body).to_string();
                let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
                seen.lock().unwrap().push(path.clone());
                if !path.ends_with("/chat/completions") {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    );
                    return;
                }
                let req: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let tool_result = req["messages"]
                    .as_array()
                    .and_then(|m| m.iter().find(|x| x["role"] == "tool"))
                    .and_then(|m| m["content"].as_str())
                    .map(str::to_string);
                let stream_on = req["stream"] == json!(true);
                let (delta, message, finish) = match &tool_result {
                    None => {
                        let tc = json!([{"index": 0, "id": "call_0", "type": "function",
                            "function": {"name": "node_status", "arguments": "{}"}}]);
                        (
                            json!({"role": "assistant", "tool_calls": tc}),
                            json!({"role": "assistant", "content": null, "tool_calls": tc}),
                            "tool_calls",
                        )
                    }
                    Some(r) => {
                        let text = format!("The node reports: {r}");
                        (
                            json!({"role": "assistant", "content": text}),
                            json!({"role": "assistant", "content": text}),
                            "stop",
                        )
                    }
                };
                let (ctype, out) = if stream_on {
                    let c1 = json!({"choices": [{"index": 0, "delta": delta, "finish_reason": null}]});
                    let c2 = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 5}});
                    (
                        "text/event-stream",
                        format!("data: {c1}\n\ndata: {c2}\n\ndata: [DONE]\n\n"),
                    )
                } else {
                    (
                        "application/json",
                        json!({"choices": [{"index": 0, "message": message, "finish_reason": finish}],
                            "usage": {"prompt_tokens": 10, "completion_tokens": 5}})
                        .to_string(),
                    )
                };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}",
                        out.len()
                    )
                    .as_bytes(),
                );
            });
        }
    });
    (format!("http://{addr}/v1"), seen)
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// US-1.1 AC3, end to end against the real `citrate-agent-sidecar` process: a turn started the way
/// the node MCP server starts one (`hermes_session_send` after the member's approval runs
/// `HermesManager::session_send`) needs the core-hosted `node_status`. No view exists. The
/// dispatcher answers it and the turn finishes in seconds, not at the sidecar's 300 s deadline.
///
/// Run: `CITRATE_E2E_HERMES_BIN=<path to citrate-agent-sidecar> cargo test --lib
/// hermes_headless -- --nocapture`. Skipped (passes) when the variable is unset.
#[test]
fn a_turn_started_without_the_view_gets_its_core_tool_answered_by_the_real_sidecar() {
    let Ok(bin) = std::env::var("CITRATE_E2E_HERMES_BIN") else {
        eprintln!("skipped: set CITRATE_E2E_HERMES_BIN to the built citrate-agent-sidecar");
        return;
    };
    let dir = std::env::temp_dir().join(format!("hheadless-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let m = HermesManager::new(
        std::path::PathBuf::from(bin),
        dir.join("bearer.token"),
        dir.join("crashes.log"),
    )
    .with_control_addr(&format!("127.0.0.1:{port}"))
    .with_health_interval(Duration::from_secs(60));
    m.start().unwrap();
    let up = Instant::now();
    while m.control_get_path("/sessions").map(|r| r.status).ok() != Some(200) {
        assert!(up.elapsed() < Duration::from_secs(20), "the sidecar did not come up");
        std::thread::sleep(Duration::from_millis(100));
    }
    let (base, seen) = model_stand_in();
    let tools = r#"[{"type":"function","function":{"name":"node_status","description":"Read this node's status: state, height, peers and sync.","parameters":{"type":"object","properties":{}}},"annotations":{"effect":"none","trust":"trusted"}}]"#;
    let body = crate::hermes::build_session_body(
        "You are Hermes.",
        tools,
        &base,
        "stand-in-key",
        "stand-in.gguf",
        8192,
    )
    .unwrap();
    let id = m.session_open(&body).unwrap();
    let leases = ViewLeases::new(VIEW_LEASE);
    let started = Instant::now();
    m.session_send(&id, "What is my node's status?").unwrap();
    let mut answered = 0;
    let mut evs: Vec<Value> = Vec::new();
    let mut after = 0;
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the turn did not finish without the view (answered here: {answered})"
        );
        answered += tick(&leases, &m, &ReadHost, Instant::now()).unwrap();
        // The test's own read (not the view's): it renews no lease.
        let p = m.session_events(&id, after, 500).unwrap();
        for e in p["events"].as_array().unwrap() {
            after = after.max(e["seq"].as_u64().unwrap());
            evs.push(e["event"].clone());
        }
        if evs.iter().any(|e| e["type"] == "done") {
            break;
        }
        std::thread::sleep(TICK);
    }
    let finished = evs;
    m.stop();
    assert_eq!(answered, 1, "the one core call was answered here");
    let result = finished
        .iter()
        .find(|e| e["type"] == "tool_result")
        .expect("a tool_result event");
    assert_eq!(result["status"], "ok", "{result}");
    let fin = finished
        .iter()
        .find(|e| e["type"] == "final")
        .expect("a final answer");
    assert!(
        fin["content"].as_str().unwrap().contains("91234"),
        "the answer quotes the real tool result: {fin}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "no wait for the 300 s deadline"
    );
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.ends_with("/chat/completions")));
}

#[test]
fn a_call_the_view_was_shown_before_the_loop_listed_it_as_pending_stays_the_views() {
    // The loop emits the tool_call event BEFORE it registers the call as pending, so the view's
    // long-poll can return the call with an empty pendingCoreCalls. The view runs it all the same
    // (host "core"); if the member then takes more than the lease on the approval card, the call
    // must not be declined here behind the member's back.
    let leases = ViewLeases::new(Duration::from_secs(30));
    let t0 = Instant::now();
    let shown = page(
        vec![call_event(5, "call_9", "group_create", r#"{"name":"x"}"#, false)],
        &[],
    );
    let _ = leases.view_page("s-race", shown, t0);
    let later = t0 + Duration::from_secs(120);
    let now_pending = page(
        vec![call_event(5, "call_9", "group_create", r#"{"name":"x"}"#, false)],
        &["call_9"],
    );
    assert!(
        leases.claim("s-race", &now_pending, later).is_empty(),
        "a call the view was shown is the view's, pending flag or not"
    );
}

#[test]
fn a_call_already_answered_in_the_page_the_view_read_is_not_kept_for_the_view() {
    // A page that holds both a call and its result (a reattach read) leaves nothing view-owned.
    let leases = ViewLeases::new(Duration::from_secs(30));
    let t0 = Instant::now();
    let read = page(
        vec![
            call_event(5, "call_4", "node_status", "{}", false),
            json!({"seq": 6, "event": {"type": "tool_result", "call_id": "call_4", "status": "ok", "content": "{}"}}),
        ],
        &[],
    );
    let _ = leases.view_page("s-old", read, t0);
    // A later step reuses the id while the view is away: it is claimed here.
    let later = t0 + Duration::from_secs(120);
    let next = page(
        vec![call_event(9, "call_4", "node_status", "{}", false)],
        &["call_4"],
    );
    assert_eq!(leases.claim("s-old", &next, later).len(), 1);
}
