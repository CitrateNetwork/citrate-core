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

// K5a + citrate-agent-runtime#87: a Store daemon's session names exactly the host tool sets its
// approved permissions cover, asks the sidecar to add nothing to the prompt, and carries only the
// core tools it was approved for.
const STORE_TOOLS: &str = r#"[{"name":"node_status","description":"Read node vitals","parameters":{"type":"object"},"annotations":{"effect":"none","trust":"trusted"}},{"name":"memory_recall","description":"Recall","parameters":{"type":"object"},"annotations":{"effect":"none","trust":"trusted"}}]"#;

fn store_body(allowed: &[&str]) -> serde_json::Value {
    let allowed: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    let body = build_store_daemon_session_body("p", STORE_TOOLS, "http://127.0.0.1:18080/v1", "k", "m.gguf", 8192, &allowed)
        .expect("body");
    serde_json::from_str(&body).expect("json")
}

#[test]
fn a_no_permission_store_daemon_session_has_no_tool_sets_no_snapshot_and_no_tools() {
    let b = store_body(&[]);
    assert_eq!(b["toolSets"], serde_json::json!([]));
    assert_eq!(b["promptSnapshot"], false);
    assert_eq!(b["unattended"], true);
    assert_eq!(b["tools"], serde_json::json!([]), "only approved core tools are sent");
}

#[test]
fn a_web_search_only_store_daemon_session_names_exactly_web_search() {
    let b = store_body(&["host:search", "memory_recall"]);
    assert_eq!(b["toolSets"], serde_json::json!(["web_search"]));
    assert_eq!(b["promptSnapshot"], false);
    let names: Vec<&str> = b["tools"].as_array().expect("tools").iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, vec!["memory_recall"]);
}

#[test]
fn the_sidecar_status_capabilities_are_read_when_present_and_absent_on_an_older_sidecar() {
    let new: RemoteStatus = serde_json::from_str(r#"{"running":true,"skills":2,"pendingApprovals":0,"capabilities":{"sessionToolSets":["web_search","capsules_run"],"sessionPromptSnapshot":true}}"#).expect("new");
    let caps = new.capabilities.expect("caps");
    assert_eq!(caps.session_tool_sets, vec!["web_search".to_string(), "capsules_run".to_string()]);
    assert!(caps.session_prompt_snapshot);
    let old: RemoteStatus = serde_json::from_str(r#"{"running":true,"skills":2,"pendingApprovals":0}"#).expect("old");
    assert!(old.capabilities.is_none());
}

// ---- K5a + runtime#87: the session-create response must echo the scoping back --------------

/// A sidecar that answers 201 and echoes whatever `echo` returns for the posted body.
struct EchoControl {
    rec: std::sync::Arc<Recorder>,
    echo: fn(&serde_json::Value) -> serde_json::Value,
}
impl HermesControl for EchoControl {
    fn get(&self, _url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp { status: 200, body: "{}".into() })
    }
    fn post(&self, url: &str, _b: &str, body: &str) -> std::result::Result<ControlResp, HermesError> {
        self.rec
            .posts
            .lock()
            .map_err(|_| HermesError::Transport("poisoned".into()))?
            .push((url.to_string(), body.to_string()));
        let sent: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        let mut resp = (self.echo)(&sent);
        resp["id"] = serde_json::json!("s7-ab");
        Ok(ControlResp { status: 201, body: resp.to_string() })
    }
    fn delete(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.rec
            .deletes
            .lock()
            .map_err(|_| HermesError::Transport("poisoned".into()))?
            .push(url.to_string());
        Ok(ControlResp { status: 204, body: String::new() })
    }
}

fn echo_mgr(echo: fn(&serde_json::Value) -> serde_json::Value) -> (HermesManager, std::sync::Arc<Recorder>) {
    let rec = std::sync::Arc::new(Recorder::default());
    (mgr(Box::new(EchoControl { rec: rec.clone(), echo })), rec)
}

fn store_body_with(allowed: &[&str]) -> (String, Vec<String>) {
    let allowed: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    let body = build_store_daemon_session_body("p", STORE_TOOLS, "http://127.0.0.1:18080/v1", "k", "m.gguf", 8192, &allowed)
        .expect("body");
    (body, crate::daemons::store_tool_sets(&allowed))
}

#[test]
fn a_sidecar_that_ignores_tool_sets_is_closed_and_the_run_refused() {
    // An older sidecar (or one that dropped the field): a plain 201 with no echo.
    let (m, rec) = echo_mgr(|_| serde_json::json!({}));
    let (body, sets) = store_body_with(&[]);
    let err = m.session_open_store(&body, &sets).unwrap_err().to_string();
    assert!(err.contains("toolSets"), "{err}");
    let deletes = rec.deletes.lock().map(|d| d.clone()).unwrap_or_default();
    assert_eq!(deletes.len(), 1, "the unscoped session is closed");
    assert!(deletes[0].ends_with("/sessions/s7-ab"));
}

#[test]
fn a_sidecar_that_applies_other_sets_or_keeps_the_snapshot_is_refused() {
    let (m, rec) = echo_mgr(|_| serde_json::json!({"toolSets": ["web_search"], "promptSnapshot": false}));
    let (body, sets) = store_body_with(&[]);
    assert!(m.session_open_store(&body, &sets).is_err(), "applied sets differ from the sent ones");
    let (m2, rec2) = echo_mgr(|sent| serde_json::json!({"toolSets": sent["toolSets"].clone()}));
    assert!(m2.session_open_store(&body, &sets).is_err(), "no promptSnapshot:false echo");
    let (m3, rec3) = echo_mgr(|sent| serde_json::json!({"toolSets": sent["toolSets"].clone(), "promptSnapshot": true}));
    assert!(m3.session_open_store(&body, &sets).is_err());
    for r in [rec, rec2, rec3] {
        assert_eq!(r.deletes.lock().map(|d| d.len()).unwrap_or(0), 1, "each refused session is closed");
    }
}

#[test]
fn a_sidecar_that_echoes_the_scoping_opens_the_session() {
    let (m, rec) = echo_mgr(|sent| serde_json::json!({"toolSets": sent["toolSets"].clone(), "promptSnapshot": false}));
    let (body, sets) = store_body_with(&["host:search", "memory_recall"]);
    assert_eq!(sets, vec!["web_search".to_string()]);
    assert_eq!(m.session_open_store(&body, &sets).expect("opened"), "s7-ab");
    assert_eq!(rec.deletes.lock().map(|d| d.len()).unwrap_or(9), 0);
    // The echo may list the same sets in another order.
    let (m2, _) = echo_mgr(|_| serde_json::json!({"toolSets": ["capsules_run", "web_search"], "promptSnapshot": false}));
    let (body2, sets2) = store_body_with(&["host:search", "host:capsules"]);
    assert!(m2.session_open_store(&body2, &sets2).is_ok());
}

#[test]
fn the_store_session_body_is_explicit_and_carries_no_persona_or_grant_tool_sets() {
    // Every host-set marker there is, plus names that must never be sent.
    let mut all: Vec<&str> = crate::daemons::HOST_TOOL_SETS.iter().map(|s| s.id).collect();
    let markers: Vec<String> = all.drain(..).map(crate::daemons::host_set_marker).collect();
    let mut allowed: Vec<&str> = markers.iter().map(String::as_str).collect();
    allowed.extend(["files", "shell_run", "host:files", "host:shell_run"]);
    let b = store_body(&allowed);
    let sets: Vec<&str> = b["toolSets"].as_array().expect("an explicit array").iter().filter_map(|v| v.as_str()).collect();
    assert!(!sets.contains(&"files") && !sets.contains(&"shell_run"), "{sets:?}");
    let grantable = crate::daemons::HOST_TOOL_SETS.iter().filter(|s| s.grantable).count();
    assert_eq!(sets.len(), grantable, "every grantable set, and no other");
    // Never omitted or null, even for none.
    let none = store_body(&[]);
    assert!(none["toolSets"].is_array());
    for key in ["persona", "customPersona", "grants", "capsuleSandbox", "projectRoots"] {
        assert!(none.get(key).is_none(), "{key} must not be sent");
    }
    assert_eq!(none["unattended"], true);
    assert_eq!(none["promptSnapshot"], false);
}

#[test]
fn relaxing_needs_both_capabilities() {
    let status = |caps: &str| -> RemoteStatus {
        serde_json::from_str(&format!(r#"{{"running":true,"skills":0,"pendingApprovals":0{caps}}}"#)).expect("status")
    };
    let all = r#"["web_search","web_browse","mcp","toolchain","skills_read","learn_propose","capsules_run","files","shell_run"]"#;
    // Both: the sets are reported, and they cover every host set.
    let both = scoping_tool_sets(&status(&format!(r#","capabilities":{{"sessionToolSets":{all},"sessionPromptSnapshot":true}}"#)));
    assert!(crate::daemons::sidecar_scopes_host_sets(both.as_deref()));
    // Tool sets without the prompt capability: not relaxed.
    let no_prompt = scoping_tool_sets(&status(&format!(r#","capabilities":{{"sessionToolSets":{all},"sessionPromptSnapshot":false}}"#)));
    assert!(no_prompt.is_none());
    let prompt_missing = scoping_tool_sets(&status(&format!(r#","capabilities":{{"sessionToolSets":{all}}}"#)));
    assert!(prompt_missing.is_none());
    // The prompt capability with only some sets: not relaxed.
    let partial = scoping_tool_sets(&status(r#","capabilities":{"sessionToolSets":["web_search"],"sessionPromptSnapshot":true}"#));
    assert!(!crate::daemons::sidecar_scopes_host_sets(partial.as_deref()));
    // An older sidecar.
    assert!(scoping_tool_sets(&status("")).is_none());
}

// ---- audit parity: adversarial echoes, a failing close, and the fail-open variants -----------

#[test]
fn every_malformed_or_partial_echo_is_refused_and_closed() {
    // Each is a way the scoping could fail to apply while the sidecar still answers 201.
    let echoes: [fn(&serde_json::Value) -> serde_json::Value; 8] = [
        |_| serde_json::json!({"toolSets": null, "promptSnapshot": false}),
        |_| serde_json::json!({"toolSets": "web_search", "promptSnapshot": false}),
        |_| serde_json::json!({"toolSets": ["web_search", 7], "promptSnapshot": false}),
        |_| serde_json::json!({"toolSets": ["web_search", "capsules_run"], "promptSnapshot": false}),
        |_| serde_json::json!({"toolSets": [], "promptSnapshot": false}),
        |_| serde_json::json!({"toolsets": ["web_search"], "promptSnapshot": false}),
        |_| serde_json::json!({"toolSets": ["web_search"], "promptSnapshot": "false"}),
        |_| serde_json::json!({"toolSets": ["web_search"]}),
    ];
    let (body, sets) = store_body_with(&["host:search"]);
    for (i, echo) in echoes.into_iter().enumerate() {
        let (m, rec) = echo_mgr(echo);
        assert!(m.session_open_store(&body, &sets).is_err(), "echo #{i} must be refused");
        assert_eq!(rec.deletes.lock().map(|d| d.len()).unwrap_or(0), 1, "echo #{i}: the session is closed");
    }
}

/// A sidecar that ignores toolSets AND cannot close the session it opened.
struct IgnoreAndKeep;
impl HermesControl for IgnoreAndKeep {
    fn get(&self, _url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp { status: 200, body: "{}".into() })
    }
    fn post(&self, _url: &str, _b: &str, _body: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp { status: 201, body: r#"{"id":"s7-ab"}"#.into() })
    }
    fn delete(&self, _url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        Err(HermesError::Transport("connection reset".into()))
    }
}

#[test]
fn a_session_that_cannot_be_closed_is_still_refused() {
    let m = mgr(Box::new(IgnoreAndKeep));
    let (body, sets) = store_body_with(&[]);
    // The run never gets the session id, so nothing is ever sent to the unscoped session.
    assert!(m.session_open_store(&body, &sets).is_err());
}

// ---- tripwire: every Store daemon session body is explicitly scoped -------------------------

#[test]
fn tripwire_every_store_session_body_has_an_explicit_tool_set_allowlist() {
    // Every combination of host-set markers, with names that must never be sent mixed in.
    let markers: Vec<String> = crate::daemons::HOST_TOOL_SETS.iter().map(|s| crate::daemons::host_set_marker(s.id)).collect();
    for mask in 0u32..(1 << markers.len()) {
        let mut allowed: Vec<String> = markers
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, m)| m.clone())
            .collect();
        allowed.extend(["files", "shell_run", "host:files", "app_navigate"].map(String::from));
        let body = build_store_daemon_session_body("p", STORE_TOOLS, "http://127.0.0.1:18080/v1", "k", "m.gguf", 8192, &allowed)
            .expect("body");
        let b: serde_json::Value = serde_json::from_str(&body).expect("json");
        let sets = b["toolSets"].as_array().unwrap_or_else(|| panic!("mask {mask}: toolSets must be an explicit array"));
        let want = crate::daemons::HOST_TOOL_SETS
            .iter()
            .enumerate()
            .filter(|(i, s)| s.grantable && mask & (1 << i) != 0)
            .count();
        assert_eq!(sets.len(), want, "mask {mask}");
        assert!(sets.iter().all(|s| s.as_str().is_some_and(|s| s != "files" && s != "shell_run")), "mask {mask}");
        assert_eq!(b["promptSnapshot"], false, "mask {mask}");
        assert_eq!(b["unattended"], true, "mask {mask}");
        assert!(b.get("persona").is_none() && b.get("customPersona").is_none(), "mask {mask}");
    }
}

#[test]
fn tripwire_a_store_session_is_opened_only_through_the_checked_paths() {
    // The Store body may be built in exactly one production place, the daemon open command, and
    // there a scoped sidecar's session must be opened through `session_open_store` (which
    // verifies the echo). A new caller of the builder fails this test until it is reviewed.
    // hermes.rs holds no test bodies (they are included from *_tests.rs files).
    let prod = include_str!("hermes.rs");
    let calls = prod.matches("build_store_daemon_session_body(").count();
    assert_eq!(calls, 2, "the definition and the one call in hermes_session_open_unattended");
    let cmd = prod
        .split("pub async fn hermes_session_open_unattended(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("the command");
    assert!(cmd.contains("build_store_daemon_session_body("));
    assert!(cmd.contains("if open.scoped") && cmd.contains("session_open_store("), "scoped sessions verify the echo");
    // Nowhere else in the crate builds a Store body.
    for (name, file) in [("daemons.rs", include_str!("daemons.rs")), ("store_catalog.rs", include_str!("store_catalog.rs"))] {
        assert!(!file.contains("build_store_daemon_session_body("), "{name}");
    }
}
