// HUP-S4.1 (core half): the member's decision on an MCP card the sidecar holds. Core re-reads what
// is waiting, binds the decision to the subject shown, and only opens a URL the sidecar holds.

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Recorder {
    posts: StdMutex<Vec<(String, String)>>,
    gets: StdMutex<Vec<String>>,
    /// What GET /mcp/pending answers.
    pending: StdMutex<String>,
    post_reply: StdMutex<Option<(u16, String)>>,
}
struct RecControl(std::sync::Arc<Recorder>);
impl HermesControl for RecControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.gets.lock().unwrap().push(url.to_string());
        let body = self.0.pending.lock().unwrap().clone();
        Ok(ControlResp { status: 200, body })
    }
    fn post(
        &self,
        url: &str,
        _b: &str,
        body: &str,
    ) -> std::result::Result<ControlResp, HermesError> {
        self.0
            .posts
            .lock()
            .unwrap()
            .push((url.to_string(), body.to_string()));
        let (status, body) = self
            .0
            .post_reply
            .lock()
            .unwrap()
            .clone()
            .unwrap_or((200, r#"{"ok":true}"#.into()));
        Ok(ControlResp { status, body })
    }
}

fn mgr(rec: std::sync::Arc<Recorder>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!(
        "hmcpcards-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

fn waiting(cards: serde_json::Value) -> String {
    serde_json::json!({ "pending": cards }).to_string()
}

const ARGS: &str = r#"{"text":"from the card"}"#;
const URL: &str = "https://auth.example.com/connect?state=abc";

fn tool_card() -> serde_json::Value {
    serde_json::json!({"id": "mcp-3", "kind": "tool_call", "subject": ARGS, "arguments": ARGS})
}
fn url_card(url: &str) -> serde_json::Value {
    serde_json::json!({"id": "mcp-4", "kind": "open_url", "subject": url, "url": url})
}

#[test]
fn pending_reads_the_sessions_cards() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([tool_card()]));
    let m = mgr(rec.clone());
    let list = mcp_pending(&m, "s1-abc").expect("pending");
    assert_eq!(list[0]["id"], "mcp-3");
    assert!(rec.gets.lock().unwrap()[0].ends_with("/sessions/s1-abc/mcp/pending"));
    assert!(mcp_pending(&m, "../x").is_err());
}

#[test]
fn a_tool_call_decision_is_bound_to_the_arguments_shown() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([tool_card()]));
    let m = mgr(rec.clone());
    let refused = mcp_decide(&m, "s1-abc", "mcp-3", true, r#"{"text":"else"}"#);
    assert!(refused.is_err());
    assert!(rec.posts.lock().unwrap().is_empty(), "nothing was sent");
    let ok = mcp_decide(&m, "s1-abc", "mcp-3", true, ARGS).expect("decide");
    assert_eq!(ok, None, "a tool call opens nothing");
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/sessions/s1-abc/mcp/decide"));
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).expect("json");
    assert_eq!(body["subject"], ARGS);
    assert_eq!(body["allow"], true);
}

#[test]
fn an_allowed_url_card_returns_the_sidecars_url_to_open_and_a_decline_opens_nothing() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([url_card(URL)]));
    let m = mgr(rec.clone());
    assert_eq!(
        mcp_decide(&m, "s1-abc", "mcp-4", true, URL).expect("decide"),
        Some(URL.to_string())
    );
    assert_eq!(
        mcp_decide(&m, "s1-abc", "mcp-4", false, URL).expect("decide"),
        None
    );
}

#[test]
fn an_unsafe_url_is_never_opened_and_the_sidecar_is_not_told_allow() {
    for bad in ["javascript:alert(1)", "file:///etc/passwd", "https://u:p@example.com/"] {
        let rec = std::sync::Arc::new(Recorder::default());
        *rec.pending.lock().unwrap() = waiting(serde_json::json!([url_card(bad)]));
        let m = mgr(rec.clone());
        assert!(mcp_decide(&m, "s1-abc", "mcp-4", true, bad).is_err(), "{bad}");
        assert!(rec.posts.lock().unwrap().is_empty(), "{bad}");
    }
}

#[test]
fn a_card_that_is_no_longer_waiting_or_a_sidecar_refusal_is_reported() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([]));
    let m = mgr(rec.clone());
    let e = mcp_decide(&m, "s1-abc", "mcp-3", true, ARGS).expect_err("gone");
    assert!(e.starts_with("MCP_DECISION_REFUSED"), "{e}");
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([tool_card()]));
    *rec.post_reply.lock().unwrap() = Some((409, r#"{"error":"no longer waiting"}"#.into()));
    let e = mcp_decide(&m, "s1-abc", "mcp-3", true, ARGS).expect_err("refused");
    assert!(e.contains("no longer waiting"), "{e}");
}

#[test]
fn inputs_are_bounded_before_they_leave_the_app() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    for id in ["sh-1", "mcp-", "mcp-x", "mcp-1/../2"] {
        assert!(mcp_decide(&m, "s1-abc", id, false, "{}").is_err(), "{id}");
    }
    let huge = "x".repeat(MAX_SUBJECT_BYTES + 1);
    assert!(mcp_decide(&m, "s1-abc", "mcp-1", false, &huge).is_err());
    assert!(mcp_decide(&m, "s1-abc", "mcp-1", false, "a\0b").is_err());
    assert!(rec.gets.lock().unwrap().is_empty());
    assert!(rec.posts.lock().unwrap().is_empty());
}

#[test]
fn only_http_addresses_are_openable() {
    assert!(openable(URL));
    assert!(openable("http://127.0.0.1:8080/x"));
    assert!(!openable("ftp://example.com/"));
    assert!(!openable("not a url"));
}

#[test]
fn addresses_with_credentials_or_without_a_host_are_never_openable() {
    assert!(!openable("https://someone@example.com/"));
    assert!(!openable("https://someone:secret@example.com/"));
    assert!(!openable("javascript:alert(1)"));
    assert!(!openable("file:///etc/hosts"));
    assert!(!openable("data:text/html,hi"));
}

#[test]
fn the_new_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    assert!(acl.contains("\"hermes_mcp_pending\""));
    assert!(acl.contains("\"hermes_mcp_decide\""));
}
