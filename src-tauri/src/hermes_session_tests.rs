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

// HUP-S2.4/A8: every tool carries effect/trust annotations (top-level beside an OpenAI `function`
// wrapper, or on a flat spec).
const TOOLS: &str = r#"[{"type":"function","function":{"name":"node_status","description":"Read node vitals","parameters":{"type":"object","properties":{}}},"annotations":{"effect":"none","trust":"trusted"}},
 {"name":"group_invite","description":"Mint an invite","parameters":{"type":"object"},"host":"sidecar","annotations":{"effect":"write","trust":"trusted"}}]"#;

#[test]
fn the_session_body_takes_the_endpoint_from_rust_and_stamps_every_tool_core() {
    let body = build_session_body("You are Hermes.", TOOLS, "http://127.0.0.1:18080/v1", "sk-local", "gemma.gguf", 16_384).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["llm"]["baseUrl"], "http://127.0.0.1:18080/v1");
    assert_eq!(v["llm"]["bearer"], "sk-local");
    assert_eq!(v["model"], "gemma.gguf");
    let tools = v["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "node_status", "OpenAI function wrappers are unwrapped");
    assert!(tools.iter().all(|t| t["host"] == "core"), "the webview cannot route a tool to the sidecar");
    assert_eq!(v["contextTokens"], 16_384, "the running llama-server's planned context window");
    assert_eq!(v["maxTokens"], crate::ai::AI_MAX_TOKENS, "the same per-turn cap as direct chat");
    assert_eq!(v["maxToolsPerRequest"], 8);
}

#[test]
fn malformed_tool_specs_are_refused() {
    for bad in ["{}", "not json", r#"[{"description":"no name"}]"#, r#"[{"name":"../x","parameters":{}}]"#, r#"[{"name":"a","parameters":"nope"}]"#] {
        assert!(build_session_body("p", bad, "http://127.0.0.1:1/v1", "", "m", 8192).is_err(), "{bad}");
    }
    let many: Vec<serde_json::Value> = (0..65).map(|i| serde_json::json!({"name": format!("t{i}"), "parameters": {}})).collect();
    assert!(build_session_body("p", &serde_json::to_string(&many).unwrap(), "http://127.0.0.1:1/v1", "", "m", 8192).is_err());
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
    let body = build_session_body("p", "[]", "http://127.0.0.1:1/v1", "", "m", 8192).unwrap();
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

// ---------------------------------------------------------------------------------------------
// HUP-S2.4 / A8 — tool annotations ride the session body to the sidecar's taint downgrade
// ---------------------------------------------------------------------------------------------

#[test]
fn annotations_are_carried_through_with_the_host_still_stamped_core() {
    let body = build_session_body("p", TOOLS, "http://127.0.0.1:1/v1", "", "m", 8192).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let tools = v["tools"].as_array().unwrap();
    assert_eq!(tools[0]["annotations"]["effect"], "none");
    assert_eq!(tools[0]["annotations"]["trust"], "trusted");
    assert_eq!(tools[0]["annotations"]["read_only"], true);
    assert_eq!(tools[1]["annotations"]["effect"], "write");
    assert_eq!(tools[1]["annotations"]["read_only"], false);
    assert!(tools.iter().all(|t| t["host"] == "core"));
}

#[test]
fn an_annotation_cannot_smuggle_a_host_or_extra_hints() {
    let t = r#"[{"name":"a","parameters":{},"annotations":{"effect":"none","trust":"trusted","host":"sidecar","destructive":false}}]"#;
    let v: serde_json::Value = serde_json::from_str(&build_session_body("p", t, "http://127.0.0.1:1/v1", "", "m", 8192).unwrap()).unwrap();
    let a = &v["tools"][0]["annotations"];
    assert!(a.get("host").is_none(), "{a}");
    assert!(a.get("destructive").is_none(), "only core-derived hints are sent: {a}");
    assert_eq!(v["tools"][0]["host"], "core");
}

#[test]
fn a_tool_without_complete_annotations_is_refused() {
    for bad in [
        r#"[{"name":"a","parameters":{}}]"#,
        r#"[{"name":"a","parameters":{},"annotations":{"effect":"write"}}]"#,
        r#"[{"name":"a","parameters":{},"annotations":{"trust":"trusted"}}]"#,
        r#"[{"name":"a","parameters":{},"annotations":"none"}]"#,
        r#"[{"name":"a","parameters":{},"annotations":{"effect":"burn","trust":"trusted"}}]"#,
        r#"[{"name":"a","parameters":{},"annotations":{"effect":"none","trust":"mostly"}}]"#,
        r#"[{"name":"a","parameters":{},"annotations":{"effect":"NONE","trust":"trusted"}}]"#,
    ] {
        let err = build_session_body("p", bad, "http://127.0.0.1:1/v1", "", "m", 8192).unwrap_err();
        assert!(err.contains("annotation"), "{bad}: {err}");
    }
}

#[test]
fn every_effect_and_trust_value_the_runtime_knows_is_accepted() {
    for effect in ["none", "write", "spend", "sign"] {
        for trust in ["trusted", "untrusted"] {
            let t = format!(r#"[{{"name":"a","parameters":{{}},"annotations":{{"effect":"{effect}","trust":"{trust}"}}}}]"#);
            let v: serde_json::Value = serde_json::from_str(&build_session_body("p", &t, "http://127.0.0.1:1/v1", "", "m", 8192).unwrap()).unwrap();
            assert_eq!(v["tools"][0]["annotations"]["effect"], effect);
            assert_eq!(v["tools"][0]["annotations"]["trust"], trust);
        }
    }
}

#[test]
fn the_session_body_claims_hic_awareness() {
    // Owner decision 2026-10-01: core claims `hicAware` by default. Safe because a hic:"required"
    // call resolves only through the member's Approve/Decline click (src/shell/hicApproval.test.ts
    // pins that there is no automatic or budget route); without it the sidecar would silently
    // decline every effectful call after taint.
    let v: serde_json::Value = serde_json::from_str(&build_session_body("p", TOOLS, "http://127.0.0.1:1/v1", "", "m", 8192).unwrap()).unwrap();
    assert_eq!(v["hicAware"], serde_json::Value::Bool(true), "{v}");
}

// HUP-S1.6 (rest): the reply budget never eats more than a quarter of a small window.
#[test]
fn the_session_reply_budget_stays_inside_the_window() {
    let v: serde_json::Value = serde_json::from_str(&build_session_body("p", "[]", "http://127.0.0.1:1/v1", "", "m", 4096).unwrap()).unwrap();
    assert_eq!(v["contextTokens"], 4096);
    assert_eq!(v["maxTokens"], 1024);
    let v: serde_json::Value = serde_json::from_str(&build_session_body("p", "[]", "http://127.0.0.1:1/v1", "", "m", 65_536).unwrap()).unwrap();
    assert_eq!(v["contextTokens"], 65_536);
    assert_eq!(v["maxTokens"], crate::ai::AI_MAX_TOKENS);
}

// HUP-S1.5 — `escalate` posts the core-built body to `/escalations` and hands back the raw status +
// body (the caller settles from the sidecar's `sent` flag), including a non-2xx answer.
#[test]
fn escalate_posts_to_the_escalations_route_and_returns_the_raw_answer() {
    let rec = std::sync::Arc::new(Recorder::default());
    rec.reply.lock().unwrap().push((422, r#"{"error":"bad","sent":false}"#.into()));
    let m = mgr(rec.clone());
    let r = m.escalate(r#"{"escalationId":"esc-1"}"#).unwrap();
    assert_eq!(r.status, 422);
    assert!(r.body.contains("\"sent\":false"));
    let posts = rec.posts.lock().unwrap();
    assert_eq!(posts.len(), 1);
    assert!(posts[0].0.ends_with("/escalations"), "{}", posts[0].0);
    assert_eq!(posts[0].1, r#"{"escalationId":"esc-1"}"#);
}

#[test]
fn escalate_without_a_running_sidecar_fails_closed_before_any_request() {
    let rec = std::sync::Arc::new(Recorder::default());
    let dir = std::env::temp_dir().join(format!("hesc-{}", std::process::id()));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec.clone())));
    assert!(matches!(m.escalate("{}"), Err(HermesError::NotRunning)));
    assert!(rec.posts.lock().unwrap().is_empty());
}

// HUP-S1.9 live parity: the live runner (src/agent/parity/live.test.ts) opens sessions on the
// packaged sidecar with the body pinned in session-body-v1.json. This keeps that pinned body equal
// to what build_session_body really sends, so the live run uses the app's session config.
#[test]
fn build_session_body_matches_the_live_parity_fixture() {
    let fx: serde_json::Value =
        serde_json::from_str(include_str!("../../src/agent/parity/session-body-v1.json")).unwrap();
    let input = &fx["input"];
    let body = build_session_body(
        input["systemPrompt"].as_str().unwrap(),
        &input["tools"].to_string(),
        input["baseUrl"].as_str().unwrap(),
        input["bearer"].as_str().unwrap(),
        input["model"].as_str().unwrap(),
        input["contextTokens"].as_u64().unwrap() as u32,
    )
    .unwrap();
    let got: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(got, fx["body"], "build_session_body drifted from session-body-v1.json");
    // No max_steps override: the session runs with the sidecar's own default (the owner's turn-cap
    // decision, 6 vs 8, is still open).
    assert!(got.get("maxSteps").is_none());
}
