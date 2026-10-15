// HGW-W1 (core half): the member's decision on an action card the sidecar holds. Core re-reads
// what is waiting and binds the decision to the subject shown.

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Recorder {
    posts: StdMutex<Vec<(String, String)>>,
    gets: StdMutex<Vec<String>>,
    /// What GET /approvals/pending answers.
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
        "hactcards-{}-{:?}",
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

const ARGS: &str = r#"{"content":"hi","path":"/w/notes.md"}"#;

fn card() -> serde_json::Value {
    serde_json::json!({
        "id": "act-3", "callId": "c1", "tool": "fs_write", "hic": "required", "subject": ARGS,
        "summary": "fs_write /w/notes.md", "hicReason": "this session read untrusted content",
        "taintSources": ["shell_run"], "expiresInSecs": 300
    })
}

#[test]
fn pending_reads_the_sessions_action_cards() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([card()]));
    let m = mgr(rec.clone());
    let list = action_pending(&m, "s1-abc").expect("pending");
    assert_eq!(list[0]["id"], "act-3");
    assert_eq!(list[0]["taintSources"][0], "shell_run");
    assert!(rec.gets.lock().unwrap()[0].ends_with("/sessions/s1-abc/approvals/pending"));
    assert!(action_pending(&m, "../x").is_err());
}

#[test]
fn an_unreadable_pending_answer_is_an_error_not_an_empty_list() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = r#"{"cards":[]}"#.into();
    let m = mgr(rec.clone());
    assert!(action_pending(&m, "s1-abc").is_err());
}

#[test]
fn a_decision_is_bound_to_the_arguments_shown() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([card()]));
    let m = mgr(rec.clone());
    let refused = action_decide(&m, "s1-abc", "act-3", true, r#"{"path":"/w/other.md"}"#);
    assert!(refused.expect_err("mismatch").starts_with("ACTION_DECISION_REFUSED"));
    assert!(rec.posts.lock().unwrap().is_empty(), "nothing was sent");
    action_decide(&m, "s1-abc", "act-3", true, ARGS).expect("decide");
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/sessions/s1-abc/approvals/act-3/decide"));
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).expect("json");
    assert_eq!(body["subject"], ARGS);
    assert_eq!(body["allow"], true);
}

#[test]
fn a_decline_is_sent_bound_to_the_same_subject() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([card()]));
    let m = mgr(rec.clone());
    action_decide(&m, "s1-abc", "act-3", false, ARGS).expect("decline");
    let posts = rec.posts.lock().unwrap();
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).expect("json");
    assert_eq!(body["allow"], false);
    assert_eq!(body["subject"], ARGS);
}

#[test]
fn a_card_that_is_no_longer_waiting_or_a_sidecar_refusal_is_reported() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([]));
    let m = mgr(rec.clone());
    let e = action_decide(&m, "s1-abc", "act-3", true, ARGS).expect_err("gone");
    assert!(e.starts_with("ACTION_DECISION_REFUSED"), "{e}");
    *rec.pending.lock().unwrap() = waiting(serde_json::json!([card()]));
    *rec.post_reply.lock().unwrap() = Some((409, r#"{"error":"no longer waiting"}"#.into()));
    let e = action_decide(&m, "s1-abc", "act-3", true, ARGS).expect_err("refused");
    assert!(e.contains("no longer waiting"), "{e}");
}

#[test]
fn inputs_are_bounded_before_they_leave_the_app() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    for id in ["sh-1", "mcp-1", "act-", "act-x", "act-1/../2"] {
        assert!(action_decide(&m, "s1-abc", id, false, "{}").is_err(), "{id}");
    }
    let huge = "x".repeat(MAX_SUBJECT_BYTES + 1);
    assert!(action_decide(&m, "s1-abc", "act-1", false, &huge).is_err());
    assert!(action_decide(&m, "s1-abc", "act-1", false, "a\0b").is_err());
    assert!(rec.gets.lock().unwrap().is_empty());
    assert!(rec.posts.lock().unwrap().is_empty());
}

#[test]
fn the_new_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    assert!(acl.contains("\"hermes_action_pending\""));
    assert!(acl.contains("\"hermes_action_decide\""));
}
