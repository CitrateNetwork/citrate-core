// HUP-S1.1c — citrate-core's side of the sidecar agent session (ADR loop-in-sidecar).
// The webview supplies only a system prompt + tool specs; the model endpoint and its key come from
// Rust-owned serve state; every tool is stamped host "core" (so it runs through core's approval
// gates); session ids and tool statuses are validated before they reach a URL or the sidecar.

use super::*;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Recorder {
    posts: StdMutex<Vec<(String, String)>>,
    gets: StdMutex<Vec<String>>,
    reply: StdMutex<Vec<(u16, String)>>,
}
struct RecControl(std::sync::Arc<Recorder>);
impl HermesControl for RecControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.gets.lock().unwrap().push(url.to_string());
        let (status, body) = self.0.reply.lock().unwrap().pop().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
    fn post(&self, url: &str, _b: &str, body: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.posts.lock().unwrap().push((url.to_string(), body.to_string()));
        let (status, body) = self.0.reply.lock().unwrap().pop().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
}

fn mgr(rec: std::sync::Arc<Recorder>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!("hsess-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

const TOOLS: &str = r#"[{"type":"function","function":{"name":"node_status","description":"Read node vitals","parameters":{"type":"object","properties":{}}}},
 {"name":"group_invite","description":"Mint an invite","parameters":{"type":"object"},"host":"sidecar"}]"#;

#[test]
fn the_session_body_takes_the_endpoint_from_rust_and_stamps_every_tool_core() {
    let body = build_session_body("You are Hermes.", TOOLS, "http://127.0.0.1:18080/v1", "sk-local", "gemma.gguf").unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["llm"]["baseUrl"], "http://127.0.0.1:18080/v1");
    assert_eq!(v["llm"]["bearer"], "sk-local");
    assert_eq!(v["model"], "gemma.gguf");
    let tools = v["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "node_status", "OpenAI function wrappers are unwrapped");
    assert!(tools.iter().all(|t| t["host"] == "core"), "the webview cannot route a tool to the sidecar");
    assert_eq!(v["contextTokens"], crate::serve::DEFAULT_CTX_SIZE, "the real llama-server context window");
    assert_eq!(v["maxToolsPerRequest"], 8);
}

#[test]
fn malformed_tool_specs_are_refused() {
    for bad in ["{}", "not json", r#"[{"description":"no name"}]"#, r#"[{"name":"../x","parameters":{}}]"#, r#"[{"name":"a","parameters":"nope"}]"#] {
        assert!(build_session_body("p", bad, "http://127.0.0.1:1/v1", "", "m").is_err(), "{bad}");
    }
    let many: Vec<serde_json::Value> = (0..65).map(|i| serde_json::json!({"name": format!("t{i}"), "parameters": {}})).collect();
    assert!(build_session_body("p", &serde_json::to_string(&many).unwrap(), "http://127.0.0.1:1/v1", "", "m").is_err());
}

#[test]
fn session_ids_and_statuses_are_validated_before_any_request() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    assert!(m.session_send("../stop", "hi").is_err());
    assert!(m.session_events("a/b", 0, 10).is_err());
    assert!(m.session_tool_result("s1-ab", "c1", "approved", "x").is_err(), "only ok | denied | error");
    assert!(rec.posts.lock().unwrap().is_empty() && rec.gets.lock().unwrap().is_empty(), "nothing reached the sidecar");
}

#[test]
fn open_send_events_result_and_stop_hit_the_session_routes() {
    let rec = std::sync::Arc::new(Recorder::default());
    // replies are popped from the end
    *rec.reply.lock().unwrap() = vec![
        (200, r#"{"ok":true}"#.into()),                                       // stop
        (200, r#"{"ok":true,"delivered":true}"#.into()),                      // tool_result
        (200, r#"{"events":[{"seq":1,"event":{"type":"step_start","step":1}}],"lastSeq":1,"busy":true}"#.into()), // events
        (202, r#"{"ok":true}"#.into()),                                       // send
        (201, r#"{"id":"s1-abc"}"#.into()),                                   // open
    ];
    let m = mgr(rec.clone());
    let body = build_session_body("p", "[]", "http://127.0.0.1:1/v1", "", "m").unwrap();
    let id = m.session_open(&body).unwrap();
    assert_eq!(id, "s1-abc");
    m.session_send(&id, "hello").unwrap();
    let page = m.session_events(&id, 0, 15_000).unwrap();
    assert_eq!(page["events"][0]["event"]["type"], "step_start");
    m.session_tool_result(&id, "c1", "ok", "{\"height\":1}").unwrap();
    m.session_stop(&id).unwrap();
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/sessions"));
    assert!(posts[1].0.ends_with("/sessions/s1-abc/messages"));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&posts[1].1).unwrap()["text"], "hello");
    assert!(posts[2].0.ends_with("/sessions/s1-abc/tool_results"));
    let tr: serde_json::Value = serde_json::from_str(&posts[2].1).unwrap();
    assert_eq!(tr["callId"], "c1");
    assert_eq!(tr["status"], "ok");
    assert!(posts[3].0.ends_with("/sessions/s1-abc/stop"));
    let gets = rec.gets.lock().unwrap();
    assert!(gets[0].contains("/sessions/s1-abc/events?after=0&wait_ms=15000"));
}

#[test]
fn the_long_poll_wait_is_capped_below_the_control_timeout() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(200, r#"{"events":[],"lastSeq":0,"busy":false}"#.into())];
    let m = mgr(rec.clone());
    m.session_events("s1-abc", 3, 999_999).unwrap();
    let url = rec.gets.lock().unwrap()[0].clone();
    let wait: u64 = url.rsplit("wait_ms=").next().unwrap().parse().unwrap();
    assert!(Duration::from_millis(wait) < HERMES_CONTROL_TIMEOUT);
}

#[test]
fn a_refused_open_surfaces_the_sidecar_status() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(503, r#"{"error":"emergency stop engaged"}"#.into())];
    let m = mgr(rec);
    let err = m.session_open("{}").unwrap_err().to_string();
    assert!(err.contains("503"), "{err}");
}
