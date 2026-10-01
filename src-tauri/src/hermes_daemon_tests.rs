// HUP-S10.3 — daemon runs open UNATTENDED sidecar sessions and close them when the run ends, so a
// day of scheduled runs never fills the sidecar's session table.

use super::*;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Recorder {
    posts: StdMutex<Vec<(String, String)>>,
    deletes: StdMutex<Vec<String>>,
}
struct RecControl(std::sync::Arc<Recorder>);
impl HermesControl for RecControl {
    fn get(&self, _url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp {
            status: 200,
            body: "{}".into(),
        })
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
            .map_err(|_| HermesError::Transport("poisoned".into()))?
            .push((url.to_string(), body.to_string()));
        Ok(ControlResp {
            status: 201,
            body: r#"{"id":"s7-ab"}"#.into(),
        })
    }
    fn delete(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0
            .deletes
            .lock()
            .map_err(|_| HermesError::Transport("poisoned".into()))?
            .push(url.to_string());
        Ok(ControlResp {
            status: 204,
            body: String::new(),
        })
    }
}

/// A transport written before `delete` existed (every test fake in this crate): the default refuses.
struct OldControl;
impl HermesControl for OldControl {
    fn get(&self, _url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp {
            status: 200,
            body: "{}".into(),
        })
    }
    fn post(
        &self,
        _url: &str,
        _b: &str,
        _body: &str,
    ) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp {
            status: 200,
            body: "{}".into(),
        })
    }
}

fn mgr(control: Box<dyn HermesControl>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!(
        "hdaemon-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(control);
    m.set_token_for_test("deadbeef");
    m
}

const TOOLS: &str = r#"[{"name":"node_status","description":"Read node vitals","parameters":{"type":"object"},"annotations":{"effect":"none","trust":"trusted"}}]"#;

#[test]
fn a_daemon_session_body_is_the_chat_body_marked_unattended() -> std::result::Result<(), String> {
    let chat: serde_json::Value = serde_json::from_str(&build_session_body(
        "p",
        TOOLS,
        "http://127.0.0.1:18080/v1",
        "k",
        "m.gguf",
        8192,
    )?)
    .map_err(|e| e.to_string())?;
    let daemon: serde_json::Value = serde_json::from_str(&build_daemon_session_body(
        "p",
        TOOLS,
        "http://127.0.0.1:18080/v1",
        "k",
        "m.gguf",
        8192,
    )?)
    .map_err(|e| e.to_string())?;
    assert!(
        chat.get("unattended").is_none(),
        "chat sessions are unchanged"
    );
    assert_eq!(daemon["unattended"], true);
    assert_eq!(
        daemon["hicAware"], true,
        "core still promises to ask a person"
    );
    let mut stripped = daemon.clone();
    if let Some(o) = stripped.as_object_mut() {
        o.remove("unattended");
    }
    assert_eq!(stripped, chat, "nothing else differs");
    // Malformed tools are refused the same way.
    assert!(build_daemon_session_body("p", "{}", "http://127.0.0.1:1/v1", "", "m", 8192).is_err());
    Ok(())
}

#[test]
fn closing_a_session_deletes_it_and_validates_the_id_first() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(Box::new(RecControl(rec.clone())));
    assert!(m.session_close("../x").is_err());
    assert!(
        rec.deletes.lock().map(|d| d.is_empty()).unwrap_or(false),
        "a bad id never reaches the sidecar"
    );
    assert!(m.session_close("s7-ab").is_ok());
    let deletes = rec.deletes.lock().map(|d| d.clone()).unwrap_or_default();
    assert_eq!(deletes.len(), 1);
    assert!(deletes[0].ends_with("/sessions/s7-ab"), "{deletes:?}");
}

#[test]
fn a_transport_without_delete_fails_honestly() {
    let m = mgr(Box::new(OldControl));
    assert!(m.session_close("s7-ab").is_err());
}

#[test]
fn both_daemon_session_commands_are_async_registered_and_in_the_main_window_acl() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    let src = include_str!("hermes.rs");
    for cmd in ["hermes_session_open_unattended", "hermes_session_close"] {
        assert!(lib.contains(&format!("hermes::{cmd},")), "{cmd} registered");
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
        assert!(
            src.contains(&format!("pub async fn {cmd}(")),
            "{cmd} is async"
        );
    }
}
