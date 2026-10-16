// HUP-S2.2 (core half): the member's decision on a `shell_run` command the sidecar is holding.
// Core bounds every input before it leaves the app; the sidecar owns the binding check (the
// decision must carry exactly the argv and folder the member was shown).

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
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
        let (status, body) = self
            .0
            .reply
            .lock()
            .unwrap()
            .pop()
            .unwrap_or((200, r#"{"pending":[]}"#.into()));
        Ok(ControlResp { status, body })
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
            .reply
            .lock()
            .unwrap()
            .pop()
            .unwrap_or((200, r#"{"ok":true}"#.into()));
        Ok(ControlResp { status, body })
    }
}

fn mgr(rec: std::sync::Arc<Recorder>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!(
        "hshell-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

const PENDING: &str = r#"{"pending":[{"id":"sh-7","callId":"call_0","tool":"shell_run",
 "hic":"required","argv":["forge","build"],"resolvedProgram":"/opt/homebrew/bin/forge",
 "cwd":"/Users/m/proj","timeoutSecs":120,"expiresInSecs":300,
 "sandbox":{"backend":"seatbelt","enforced":true,"network":"denied",
  "writable":["/Users/m/proj","scratch HOME (deleted after the run)"],"readable_extra":[],
  "summary":"macOS Seatbelt: no network; writes only in /Users/m/proj and a scratch HOME"}}]}"#;

#[test]
fn approval_ids_are_sh_and_digits_only() {
    assert!(valid_approval_id("sh-1").is_ok());
    assert!(valid_approval_id("sh-123456789012345678").is_ok());
    for bad in [
        "",
        "sh-",
        "sh-1a",
        "sh1",
        "b12",
        "sh-1/../x",
        "sh-1234567890123456789",
    ] {
        assert!(valid_approval_id(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn pending_is_read_from_the_session_route() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(200, PENDING.into())];
    let m = mgr(rec.clone());
    let v = shell_pending(&m, "s1-abc").unwrap();
    assert_eq!(v[0]["id"], "sh-7");
    assert_eq!(v[0]["argv"][1], "build");
    assert!(rec.gets.lock().unwrap()[0].ends_with("/sessions/s1-abc/shell/pending"));
}

#[test]
fn pending_refuses_a_bad_session_id_before_any_call() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    assert!(shell_pending(&m, "s1/../x").is_err());
    assert!(rec.gets.lock().unwrap().is_empty());
}

#[test]
fn pending_surfaces_the_sidecar_reason_on_an_error() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(404, r#"{"error":"no such session"}"#.into())];
    let m = mgr(rec);
    let e = shell_pending(&m, "s1-abc").unwrap_err();
    assert!(e.contains("no such session"), "{e}");
}

#[test]
fn pending_refuses_a_body_without_a_pending_list() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(200, r#"{"other":1}"#.into())];
    let m = mgr(rec);
    assert!(shell_pending(&m, "s1-abc").is_err());
}

#[test]
fn a_decision_carries_exactly_the_argv_and_folder_shown() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    shell_decide(
        &m,
        "s1-abc",
        "sh-7",
        true,
        &argv(&["forge", "build", "--sizes"]),
        "/Users/m/proj",
    )
    .unwrap();
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/sessions/s1-abc/shell/decide"));
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"id":"sh-7","allow":true,"argv":["forge","build","--sizes"],"cwd":"/Users/m/proj"})
    );
}

#[test]
fn a_refused_binding_surfaces_the_sidecar_reason() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(
        409,
        r#"{"error":"the command differs from the one waiting"}"#.into(),
    )];
    let m = mgr(rec);
    let e = shell_decide(&m, "s1-abc", "sh-7", true, &argv(&["ls"]), "/w").unwrap_err();
    assert!(e.starts_with("SHELL_DECISION_REFUSED: "), "{e}");
    assert!(e.contains("differs"), "{e}");
}

#[test]
fn a_decision_is_bounded_before_it_leaves_the_app() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    let long_arg = "a".repeat(MAX_ARG_CHARS + 1);
    let many: Vec<String> = (0..=MAX_ARGS).map(|i| i.to_string()).collect();
    let cases: Vec<(&str, &str, Vec<String>, &str)> = vec![
        ("s1/x", "sh-7", argv(&["ls"]), "/w"),
        ("s1-abc", "b7", argv(&["ls"]), "/w"),
        ("s1-abc", "sh-7", vec![], "/w"),
        ("s1-abc", "sh-7", argv(&["", "x"]), "/w"),
        ("s1-abc", "sh-7", argv(&["ls", "a\0b"]), "/w"),
        ("s1-abc", "sh-7", vec![long_arg], "/w"),
        ("s1-abc", "sh-7", many, "/w"),
        ("s1-abc", "sh-7", argv(&["ls"]), "relative/dir"),
        ("s1-abc", "sh-7", argv(&["ls"]), ""),
        ("s1-abc", "sh-7", argv(&["ls"]), "/w\0x"),
    ];
    for (sid, id, a, cwd) in cases {
        assert!(
            shell_decide(&m, sid, id, false, &a, cwd).is_err(),
            "{sid} {id} {a:?} {cwd:?} must be refused"
        );
    }
    let long_cwd = format!("/{}", "d".repeat(MAX_CWD_CHARS));
    assert!(shell_decide(&m, "s1-abc", "sh-7", false, &argv(&["ls"]), &long_cwd).is_err());
    assert!(rec.posts.lock().unwrap().is_empty());
}

#[test]
fn the_new_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    assert!(acl.contains("\"hermes_shell_pending\""));
    assert!(acl.contains("\"hermes_shell_decide\""));
}
