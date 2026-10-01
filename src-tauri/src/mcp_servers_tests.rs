// HUP-S4.4 — user-added MCP servers: validation, the registry, the review gate, and the allowlist
// file the sidecar reads. Included into `mcp_servers::tests`.

use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-mcp-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).expect("mk tmpdir");
    p
}

fn stdio_input(name: &str) -> ServerInput {
    ServerInput {
        name: name.into(),
        transport: "stdio".into(),
        command: Some("/opt/notes/bin/notes-mcp".into()),
        args: vec!["--root".into(), "/Users/me/notes".into()],
        cwd: None,
        url: None,
        env: vec![EnvInput {
            key: "NOTES_TOKEN".into(),
            value: Some("tok-abc-123".into()),
        }],
        allow_write_tools: false,
        previous_name: None,
    }
}

fn http_input(name: &str) -> ServerInput {
    ServerInput {
        name: name.into(),
        transport: "http".into(),
        command: None,
        args: vec![],
        cwd: None,
        url: Some("https://scan.example/api/mcp".into()),
        env: vec![],
        allow_write_tools: false,
        previous_name: None,
    }
}

fn fields(errs: &[FieldError]) -> Vec<String> {
    errs.iter().map(|e| e.field.clone()).collect()
}

fn ok_probe(name: &str) -> ProbeReport {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "transport": "stdio",
        "ok": true,
        "error": null,
        "protocolVersion": "2025-06-18",
        "serverName": "notes",
        "serverVersion": "1.0.0",
        "capabilities": {"tools": {}},
        "tools": [
            {"name": "search", "exposedName": "mcp__notes__search", "description": "Search notes.",
             "annotations": {"readOnlyHint": true, "destructiveHint": null, "idempotentHint": null, "openWorldHint": false, "title": null},
             "effective": {"readOnly": true, "destructive": false, "idempotent": false, "openWorld": false},
             "trust": "untrusted", "offered": true, "skipReason": null},
            {"name": "delete_note", "exposedName": "mcp__notes__delete_note", "description": "Delete a note.",
             "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": null, "openWorldHint": null, "title": null},
             "effective": {"readOnly": false, "destructive": true, "idempotent": false, "openWorld": true},
             "trust": "untrusted", "offered": false, "skipReason": "not annotated read-only, and this server does not allow write tools"}
        ],
        "toolsTruncated": false,
        "allowWriteTools": false
    }))
    .expect("probe report")
}

// ---------------------------------------------------------------- validation

#[test]
fn a_valid_stdio_and_http_input_pass() {
    assert!(validate_input(&stdio_input("notes")).is_ok());
    assert!(validate_input(&http_input("scan2")).is_ok());
    let mut loopback = http_input("local");
    loopback.url = Some("http://127.0.0.1:3000/mcp".into());
    assert!(validate_input(&loopback).is_ok());
}

#[test]
fn every_problem_is_reported_against_its_field() {
    let mut bad = stdio_input("Bad Name");
    bad.command = Some("notes-mcp".into());
    bad.cwd = Some("relative".into());
    bad.env = vec![
        EnvInput {
            key: "1BAD".into(),
            value: Some("x".into()),
        },
        EnvInput {
            key: "LD_PRELOAD".into(),
            value: Some("/tmp/x.so".into()),
        },
        EnvInput {
            key: "REF".into(),
            value: Some("${HF_TOKEN}".into()),
        },
    ];
    let errs = validate_input(&bad).expect_err("invalid");
    let f = fields(&errs);
    for want in [
        "name",
        "command",
        "cwd",
        "env.1BAD",
        "env.LD_PRELOAD",
        "env.REF",
    ] {
        assert!(f.iter().any(|x| x == want), "{want} in {f:?}");
    }
}

#[test]
fn reserved_names_and_bad_urls_are_refused() {
    for n in ["mem", "citrate-node", "scan", "hermes"] {
        let errs = validate_input(&http_input(n)).expect_err(n);
        assert_eq!(fields(&errs), vec!["name"]);
    }
    for u in [
        "http://10.0.0.1/mcp",
        "https://user:pw@example.com/mcp",
        "ftp://example.com",
        "http://127.0.0.1.example.com/mcp",
    ] {
        let mut i = http_input("a");
        i.url = Some(u.into());
        let errs = validate_input(&i).expect_err(u);
        assert_eq!(fields(&errs), vec!["url"], "{u}");
    }
}

#[test]
fn fields_of_the_other_transport_are_refused() {
    let mut i = http_input("a");
    i.command = Some("/bin/a".into());
    assert!(fields(&validate_input(&i).expect_err("cmd on http")).contains(&"command".to_string()));
    let mut i = stdio_input("a");
    i.url = Some("https://x".into());
    assert!(fields(&validate_input(&i).expect_err("url on stdio")).contains(&"url".to_string()));
    let mut i = http_input("a");
    i.transport = "sse".into();
    assert!(fields(&validate_input(&i).expect_err("sse")).contains(&"transport".to_string()));
}

#[test]
fn env_references_are_recognised_and_plain_values_pass() {
    for v in ["$HF_TOKEN", "${X}", "%APPDATA%", "a${B}c"] {
        assert!(is_env_reference(v), "{v}");
    }
    for v in ["pa$$word", "100%", "$1", "plain"] {
        assert!(!is_env_reference(v), "{v}");
    }
}

#[test]
fn validation_messages_never_contain_env_values() {
    let mut i = stdio_input("a");
    i.env = vec![EnvInput {
        key: "T".into(),
        value: Some("${sekrit-value}".into()),
    }];
    let errs = validate_input(&i).expect_err("ref");
    assert!(!format!("{errs:?}").contains("sekrit"));
}

// ---------------------------------------------------------------- registry

#[test]
fn save_adds_a_disabled_entry_that_needs_review() {
    let mut reg = Registry::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let s = &reg.servers[0];
    assert!(!s.enabled);
    assert!(s.reviewed.is_none());
    let views = views(&reg);
    assert!(views[0].needs_review);
    assert!(!views[0].enabled);
}

#[test]
fn views_mask_env_values() {
    let mut reg = Registry::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let v = views(&reg);
    let json = serde_json::to_string(&v).expect("json");
    assert!(!json.contains("tok-abc-123"), "{json}");
    assert_eq!(v[0].env[0].key, "NOTES_TOKEN");
    assert!(
        v[0].env[0].masked.contains("11 characters"),
        "{}",
        v[0].env[0].masked
    );
}

#[test]
fn an_omitted_env_value_keeps_the_stored_one_and_a_new_key_needs_a_value() {
    let mut reg = Registry::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let mut edit = stdio_input("notes");
    edit.previous_name = Some("notes".into());
    edit.env = vec![EnvInput {
        key: "NOTES_TOKEN".into(),
        value: None,
    }];
    apply_save(&mut reg, edit).expect("keep");
    assert_eq!(
        reg.servers[0].env.get("NOTES_TOKEN").map(String::as_str),
        Some("tok-abc-123")
    );
    let mut edit = stdio_input("notes");
    edit.previous_name = Some("notes".into());
    edit.env = vec![EnvInput {
        key: "OTHER".into(),
        value: None,
    }];
    let errs = apply_save(&mut reg, edit).expect_err("no value");
    assert_eq!(fields(&errs), vec!["env.OTHER"]);
}

#[test]
fn duplicate_names_and_too_many_servers_are_refused() {
    let mut reg = Registry::default();
    apply_save(&mut reg, http_input("a")).expect("a");
    let errs = apply_save(&mut reg, http_input("a")).expect_err("dup");
    assert_eq!(fields(&errs), vec!["name"]);
    for i in 1..MAX_SERVERS {
        apply_save(&mut reg, http_input(&format!("s{i}"))).expect("fill");
    }
    let errs = apply_save(&mut reg, http_input("one-too-many")).expect_err("full");
    assert_eq!(fields(&errs), vec!["entry"]);
}

#[test]
fn rename_moves_the_entry_and_requires_a_fresh_review() {
    let mut reg = Registry::default();
    apply_save(&mut reg, http_input("a")).expect("a");
    let fp = fingerprint(&reg.servers[0]);
    reg.servers[0].enabled = true;
    reg.servers[0].reviewed = Some(fp);
    let mut r = http_input("b");
    r.previous_name = Some("a".into());
    apply_save(&mut reg, r).expect("rename");
    assert_eq!(reg.servers.len(), 1);
    assert_eq!(reg.servers[0].name, "b");
    assert!(!reg.servers[0].enabled);
    assert!(reg.servers[0].reviewed.is_none());
}

type Change = Box<dyn Fn(&mut ServerInput)>;

#[test]
fn an_unchanged_save_keeps_the_review_and_any_change_drops_it() {
    let mut reg = Registry::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let fp = fingerprint(&reg.servers[0]);
    reg.servers[0].enabled = true;
    reg.servers[0].reviewed = Some(fp);
    let mut same = stdio_input("notes");
    same.previous_name = Some("notes".into());
    apply_save(&mut reg, same).expect("same");
    assert!(reg.servers[0].enabled, "an identical save keeps it enabled");
    // Each material change needs a new review: command, args, env value, write tools.
    let changes: Vec<Change> = vec![
        Box::new(|i| i.command = Some("/opt/other".into())),
        Box::new(|i| i.args.push("--x".into())),
        Box::new(|i| i.env[0].value = Some("another".into())),
        Box::new(|i| i.allow_write_tools = true),
    ];
    for change in changes {
        // Each change is applied in isolation to a freshly reviewed base entry.
        let mut reg = Registry::default();
        apply_save(&mut reg, stdio_input("notes")).expect("base");
        let fp = fingerprint(&reg.servers[0]);
        reg.servers[0].enabled = true;
        reg.servers[0].reviewed = Some(fp);
        let mut edit = stdio_input("notes");
        edit.previous_name = Some("notes".into());
        change(&mut edit);
        apply_save(&mut reg, edit).expect("edit");
        assert!(!reg.servers[0].enabled);
        assert!(reg.servers[0].reviewed.is_none());
    }
}

#[test]
fn remove_drops_the_entry() {
    let mut reg = Registry::default();
    apply_save(&mut reg, http_input("a")).expect("a");
    assert!(apply_remove(&mut reg, "a"));
    assert!(reg.servers.is_empty());
    assert!(!apply_remove(&mut reg, "a"));
}

// ---------------------------------------------------------------- review gate

#[test]
fn enable_requires_a_successful_probe_of_the_entry_as_it_stands() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let token = review_token(&gate, &reg.servers[0]);
    // No probe yet.
    assert!(apply_enable(&mut reg, &gate, "notes", &token).is_err());
    // A failed probe does not count.
    let mut failed = ok_probe("notes");
    failed.ok = false;
    gate.record(&reg.servers[0], &failed);
    assert!(apply_enable(&mut reg, &gate, "notes", &token).is_err());
    // A successful probe does.
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let token = review_token(&gate, &reg.servers[0]);
    assert!(apply_enable(&mut reg, &gate, "notes", "not-the-token").is_err());
    apply_enable(&mut reg, &gate, "notes", &token).expect("enable");
    assert!(reg.servers[0].enabled);
    assert_eq!(
        reg.servers[0].reviewed.as_deref(),
        Some(fingerprint(&reg.servers[0]).as_str())
    );
}

#[test]
fn a_review_of_an_older_version_cannot_enable_the_edited_entry() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let old_token = review_token(&gate, &reg.servers[0]);
    let mut edit = stdio_input("notes");
    edit.previous_name = Some("notes".into());
    edit.command = Some("/opt/evil".into());
    apply_save(&mut reg, edit).expect("edit");
    assert!(apply_enable(&mut reg, &gate, "notes", &old_token).is_err());
}

#[test]
fn the_review_token_is_not_the_stored_fingerprint() {
    let mut reg = Registry::default();
    let gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let token = review_token(&gate, &reg.servers[0]);
    assert_ne!(token, fingerprint(&reg.servers[0]));
    assert_eq!(token.len(), 64);
}

#[test]
fn the_review_view_shows_the_exact_command_and_masks_env() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let probe = ok_probe("notes");
    gate.record(&reg.servers[0], &probe);
    let r = review_view(&gate, &reg.servers[0], probe);
    assert_eq!(
        r.server.command.as_deref(),
        Some("/opt/notes/bin/notes-mcp")
    );
    assert_eq!(r.server.args, vec!["--root", "/Users/me/notes"]);
    assert_eq!(
        r.command_line,
        "/opt/notes/bin/notes-mcp --root /Users/me/notes"
    );
    let json = serde_json::to_string(&r).expect("json");
    assert!(!json.contains("tok-abc-123"), "{json}");
    assert!(json.contains("\"readOnlyHint\":true"), "{json}");
    assert!(json.contains("\"trust\":\"untrusted\""), "{json}");
    assert!(r.can_enable);
}

#[test]
fn command_lines_quote_arguments_with_spaces() {
    assert_eq!(
        command_line(
            "/bin/x",
            &["a b".to_string(), "c".to_string(), "".to_string()]
        ),
        "/bin/x \"a b\" c \"\""
    );
}

#[test]
fn a_probe_report_claiming_a_trusted_tool_is_refused() {
    let mut p = ok_probe("notes");
    p.tools[0].trust = "trusted".into();
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    gate.record(&reg.servers[0], &p);
    let token = review_token(&gate, &reg.servers[0]);
    assert!(apply_enable(&mut reg, &gate, "notes", &token).is_err());
}

// ---------------------------------------------------------------- allowlist file

#[test]
fn the_allowlist_has_only_enabled_reviewed_entries_in_the_runtime_shape() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    apply_save(&mut reg, http_input("scan2")).expect("save");
    assert!(allowlist_json(&reg).is_none(), "nothing enabled: no file");
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let token = review_token(&gate, &reg.servers[0]);
    apply_enable(&mut reg, &gate, "notes", &token).expect("enable");
    let text = allowlist_json(&reg).expect("one enabled");
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    let servers = v["servers"].as_array().expect("servers");
    assert_eq!(servers.len(), 1);
    let s = &servers[0];
    assert_eq!(s["name"], "notes");
    assert_eq!(s["transport"], "stdio");
    assert_eq!(s["command"], "/opt/notes/bin/notes-mcp");
    assert_eq!(s["env"]["NOTES_TOKEN"], "tok-abc-123");
    assert_eq!(s["allow_write_tools"], false);
    // Only the runtime's keys: no core bookkeeping leaks into the file the sidecar parses
    // with deny_unknown_fields.
    let keys: Vec<&str> = s
        .as_object()
        .expect("obj")
        .keys()
        .map(String::as_str)
        .collect();
    for k in &keys {
        assert!(
            [
                "name",
                "transport",
                "command",
                "args",
                "env",
                "cwd",
                "url",
                "allow_write_tools"
            ]
            .contains(k),
            "{k}"
        );
    }
}

#[test]
fn an_enabled_entry_whose_review_no_longer_matches_is_left_out() {
    let mut reg = Registry::default();
    apply_save(&mut reg, http_input("a")).expect("a");
    reg.servers[0].enabled = true;
    reg.servers[0].reviewed = Some("0".repeat(64));
    assert!(allowlist_json(&reg).is_none());
}

#[test]
fn the_store_round_trips_and_writes_private_files() {
    let dir = tmp_dir("store");
    let store = McpStore::new(dir.clone());
    assert!(store.load().expect("empty").servers.is_empty());
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let token = review_token(&gate, &reg.servers[0]);
    apply_enable(&mut reg, &gate, "notes", &token).expect("enable");
    store.save(&reg).expect("save");
    let back = store.load().expect("load");
    assert_eq!(back.servers.len(), 1);
    assert!(back.servers[0].enabled);
    assert!(store.allowlist_path().exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [store.registry_path(), store.allowlist_path()] {
            let mode = std::fs::metadata(&p).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", p.display());
        }
    }
    // Removing the last enabled server removes the allowlist file (the sidecar then runs no MCP).
    let mut reg = back;
    apply_remove(&mut reg, "notes");
    store.save(&reg).expect("save");
    assert!(!store.allowlist_path().exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_registry_is_an_error_not_an_empty_list() {
    let dir = tmp_dir("corrupt");
    let store = McpStore::new(dir.clone());
    std::fs::write(store.registry_path(), "{not json").expect("write");
    assert!(store.load().is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- sidecar wiring

#[test]
fn hermes_spec_carries_the_allowlist_path_only_when_the_file_exists() {
    let dir = tmp_dir("spec");
    let allow = dir.join("hermes").join(ALLOWLIST_FILE);
    let mgr = crate::hermes::HermesManager::new(
        dir.join("bin"),
        dir.join("hermes").join("token"),
        dir.join("crash.jsonl"),
    );
    let env: BTreeMap<String, String> = mgr.spec_env_for_test().into_iter().collect();
    assert!(
        !env.contains_key(crate::hermes::HERMES_MCP_ENV),
        "unset by default"
    );
    let mgr = mgr.with_mcp_allowlist(allow.clone());
    let env: BTreeMap<String, String> = mgr.spec_env_for_test().into_iter().collect();
    assert!(
        !env.contains_key(crate::hermes::HERMES_MCP_ENV),
        "no file: still unset"
    );
    std::fs::create_dir_all(allow.parent().expect("parent")).expect("mkdir");
    std::fs::write(&allow, "{\"servers\": []}").expect("write");
    let env: BTreeMap<String, String> = mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(
        env.get(crate::hermes::HERMES_MCP_ENV).map(String::as_str),
        Some(allow.to_string_lossy().as_ref())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

struct ProbeMock {
    status: u16,
    body: String,
    seen: std::sync::Mutex<Vec<(String, String)>>,
}

impl crate::hermes::HermesControl for ProbeMock {
    fn get(
        &self,
        url: &str,
        _bearer: &str,
    ) -> std::result::Result<crate::hermes::ControlResp, crate::hermes::HermesError> {
        if let Ok(mut s) = self.seen.lock() {
            s.push((url.to_string(), String::new()));
        }
        Ok(crate::hermes::ControlResp {
            status: self.status,
            body: self.body.clone(),
        })
    }
    fn post(
        &self,
        url: &str,
        _bearer: &str,
        body: &str,
    ) -> std::result::Result<crate::hermes::ControlResp, crate::hermes::HermesError> {
        if let Ok(mut s) = self.seen.lock() {
            s.push((url.to_string(), body.to_string()));
        }
        Ok(crate::hermes::ControlResp {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

fn probe_manager(status: u16, body: String) -> crate::hermes::HermesManager {
    let dir = tmp_dir("probe");
    let mgr = crate::hermes::HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_control(Box::new(ProbeMock {
            status,
            body,
            seen: std::sync::Mutex::new(vec![]),
        }));
    mgr.set_token_for_test("tok");
    mgr
}

#[test]
fn the_probe_posts_the_runtime_entry_and_parses_the_report() {
    let mut reg = Registry::default();
    apply_save(&mut reg, stdio_input("notes")).expect("save");
    let body = serde_json::to_string(&ok_probe("notes")).expect("json");
    let mgr = probe_manager(200, body);
    let out = mgr.mcp_probe(&entry_json(&reg.servers[0])).expect("probe");
    match out {
        ProbeOutcome::Report(r) => {
            assert!(r.ok);
            assert_eq!(r.tools.len(), 2);
        }
        other => panic!("expected a report, got {other:?}"),
    }
}

#[test]
fn a_422_from_the_sidecar_becomes_field_errors() {
    let mgr = probe_manager(
        422,
        serde_json::json!({"error": "invalid server entry", "errors": [{"field": "env.X", "message": "bad"}]})
            .to_string(),
    );
    match mgr
        .mcp_probe(&serde_json::json!({"name": "a"}))
        .expect("probe")
    {
        ProbeOutcome::Invalid(errs) => assert_eq!(fields(&errs), vec!["env.X"]),
        other => panic!("expected field errors, got {other:?}"),
    }
}

#[test]
fn the_probe_fails_closed_without_a_running_sidecar() {
    let dir = tmp_dir("noprobe");
    let mgr = crate::hermes::HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    assert!(mgr.mcp_probe(&serde_json::json!({})).is_err());
}

#[test]
fn mask_never_shows_any_part_of_the_value() {
    let m = mask("abcdefgh-secret");
    assert!(!m.contains("abc") && !m.contains("ret"), "{m}");
    assert!(m.contains("15 characters"));
    assert_eq!(mask(""), "(empty)");
}
