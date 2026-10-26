// HUP-S4.4 — user-added MCP servers: validation, the registry, the review gate, and the allowlist
// the sidecar reads. RC52 P-9a: env values in the keychain, the migration from plain text, and the
// spawn-time and probe-time files. Included into `mcp_servers::tests`.

use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A keychain double shared by the tests that do not inspect it (every item has a unique account).
fn mem() -> &'static MemSecrets {
    static M: OnceLock<MemSecrets> = OnceLock::new();
    M.get_or_init(MemSecrets::default)
}

fn entry_text(s: &StoredServer) -> String {
    runtime_entry_json(s, mem()).expect("values in the keychain").to_string()
}

fn allowlist_text(reg: &Registry) -> Option<String> {
    runtime_allowlist_json(reg, mem(), &[])
        .expect("values in the keychain")
        .map(|t| t.to_string())
}

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
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
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
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    let v = views(&reg);
    let json = serde_json::to_string(&v).expect("json");
    assert!(!json.contains("tok-abc-123"), "{json}");
    assert_eq!(v[0].env[0].key, "NOTES_TOKEN");
    assert_eq!(v[0].env[0].masked, MASK);
}

#[test]
fn an_omitted_env_value_keeps_the_stored_one_and_a_new_key_needs_a_value() {
    let mut reg = Registry::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    let mut edit = stdio_input("notes");
    edit.previous_name = Some("notes".into());
    edit.env = vec![EnvInput {
        key: "NOTES_TOKEN".into(),
        value: None,
    }];
    apply_save(&mut reg, edit, mem()).expect("keep");
    assert!(entry_text(&reg.servers[0]).contains("\"NOTES_TOKEN\":\"tok-abc-123\""));
    let mut edit = stdio_input("notes");
    edit.previous_name = Some("notes".into());
    edit.env = vec![EnvInput {
        key: "OTHER".into(),
        value: None,
    }];
    let errs = apply_save(&mut reg, edit, mem()).expect_err("no value");
    assert_eq!(fields(&errs), vec!["env.OTHER"]);
}

#[test]
fn duplicate_names_and_too_many_servers_are_refused() {
    let mut reg = Registry::default();
    apply_save(&mut reg, http_input("a"), mem()).expect("a");
    let errs = apply_save(&mut reg, http_input("a"), mem()).expect_err("dup");
    assert_eq!(fields(&errs), vec!["name"]);
    for i in 1..MAX_SERVERS {
        apply_save(&mut reg, http_input(&format!("s{i}")), mem()).expect("fill");
    }
    let errs = apply_save(&mut reg, http_input("one-too-many"), mem()).expect_err("full");
    assert_eq!(fields(&errs), vec!["entry"]);
}

#[test]
fn rename_moves_the_entry_and_requires_a_fresh_review() {
    let mut reg = Registry::default();
    apply_save(&mut reg, http_input("a"), mem()).expect("a");
    let fp = fingerprint(&reg.servers[0]);
    reg.servers[0].enabled = true;
    reg.servers[0].reviewed = Some(fp);
    let mut r = http_input("b");
    r.previous_name = Some("a".into());
    apply_save(&mut reg, r, mem()).expect("rename");
    assert_eq!(reg.servers.len(), 1);
    assert_eq!(reg.servers[0].name, "b");
    assert!(!reg.servers[0].enabled);
    assert!(reg.servers[0].reviewed.is_none());
}

type Change = Box<dyn Fn(&mut ServerInput)>;

#[test]
fn an_unchanged_save_keeps_the_review_and_any_change_drops_it() {
    let mut reg = Registry::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    let fp = fingerprint(&reg.servers[0]);
    reg.servers[0].enabled = true;
    reg.servers[0].reviewed = Some(fp);
    let mut same = stdio_input("notes");
    same.previous_name = Some("notes".into());
    apply_save(&mut reg, same, mem()).expect("same");
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
        apply_save(&mut reg, stdio_input("notes"), mem()).expect("base");
        let fp = fingerprint(&reg.servers[0]);
        reg.servers[0].enabled = true;
        reg.servers[0].reviewed = Some(fp);
        let mut edit = stdio_input("notes");
        edit.previous_name = Some("notes".into());
        change(&mut edit);
        apply_save(&mut reg, edit, mem()).expect("edit");
        assert!(!reg.servers[0].enabled);
        assert!(reg.servers[0].reviewed.is_none());
    }
}

#[test]
fn remove_drops_the_entry() {
    let mut reg = Registry::default();
    apply_save(&mut reg, http_input("a"), mem()).expect("a");
    assert!(apply_remove(&mut reg, "a").is_some());
    assert!(reg.servers.is_empty());
    assert!(apply_remove(&mut reg, "a").is_none());
}

// ---------------------------------------------------------------- review gate

#[test]
fn enable_requires_a_successful_probe_of_the_entry_as_it_stands() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
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
fn turning_a_server_off_needs_a_fresh_check_to_turn_it_on_again() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let token = review_token(&gate, &reg.servers[0]);
    apply_enable(&mut reg, &gate, "notes", &token).expect("enable");
    assert!(apply_disable(&mut reg, &mut gate, "notes"));
    assert!(!reg.servers[0].enabled);
    assert!(reg.servers[0].reviewed.is_none());
    assert!(allowlist_text(&reg).is_none());
    // The earlier check's token no longer enables it.
    assert!(apply_enable(&mut reg, &gate, "notes", &token).is_err());
    assert!(!apply_disable(&mut reg, &mut gate, "missing"));
    // A new check does.
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let token = review_token(&gate, &reg.servers[0]);
    apply_enable(&mut reg, &gate, "notes", &token).expect("re-enable");
}

#[test]
fn a_review_of_an_older_version_cannot_enable_the_edited_entry() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let old_token = review_token(&gate, &reg.servers[0]);
    let mut edit = stdio_input("notes");
    edit.previous_name = Some("notes".into());
    edit.command = Some("/opt/evil".into());
    apply_save(&mut reg, edit, mem()).expect("edit");
    assert!(apply_enable(&mut reg, &gate, "notes", &old_token).is_err());
}

#[test]
fn the_review_token_is_not_the_stored_fingerprint() {
    let mut reg = Registry::default();
    let gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    let token = review_token(&gate, &reg.servers[0]);
    assert_ne!(token, fingerprint(&reg.servers[0]));
    assert_eq!(token.len(), 64);
}

#[test]
fn the_review_view_shows_the_exact_command_and_masks_env() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
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
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    gate.record(&reg.servers[0], &p);
    let token = review_token(&gate, &reg.servers[0]);
    assert!(apply_enable(&mut reg, &gate, "notes", &token).is_err());
}

// ---------------------------------------------------------------- allowlist file

#[test]
fn the_allowlist_has_only_enabled_reviewed_entries_in_the_runtime_shape() {
    let mut reg = Registry::default();
    let mut gate = ReviewGate::default();
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    apply_save(&mut reg, http_input("scan2"), mem()).expect("save");
    assert!(allowlist_text(&reg).is_none(), "nothing enabled: no file");
    gate.record(&reg.servers[0], &ok_probe("notes"));
    let token = review_token(&gate, &reg.servers[0]);
    apply_enable(&mut reg, &gate, "notes", &token).expect("enable");
    let text = allowlist_text(&reg).expect("one enabled");
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
    apply_save(&mut reg, http_input("a"), mem()).expect("a");
    reg.servers[0].enabled = true;
    reg.servers[0].reviewed = Some("0".repeat(64));
    assert!(allowlist_text(&reg).is_none());
}

fn store_with(tag: &str) -> (McpStore, Arc<MemSecrets>, PathBuf) {
    let dir = tmp_dir(tag);
    let secrets = Arc::new(MemSecrets::default());
    (McpStore::new(dir.clone(), secrets.clone()), secrets, dir)
}

/// An enabled entry with a matching review (what a member gets after check + turn on).
fn enable_reviewed(reg: &mut Registry, name: &str) {
    let mut gate = ReviewGate::default();
    let i = reg.servers.iter().position(|s| s.name == name).expect("entry");
    gate.record(&reg.servers[i], &ok_probe(name));
    let token = review_token(&gate, &reg.servers[i]);
    apply_enable(reg, &gate, name, &token).expect("enable");
}

#[test]
fn the_store_round_trips_and_writes_private_files() {
    let (store, secrets, dir) = store_with("store");
    assert!(store.load().expect("empty").servers.is_empty());
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    enable_reviewed(&mut reg, "notes");
    store.commit(&reg, &fx).expect("commit");
    let back = store.load().expect("load");
    assert_eq!(back.servers.len(), 1);
    assert!(back.servers[0].enabled);
    assert!(views(&back)[0].enabled, "the review survives the round trip");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(store.registry_path())
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    // No allowlist file is kept between starts any more.
    assert!(!store.legacy_allowlist_path().exists());
    // Removing the entry removes its keychain item once the file is saved.
    let mut reg = back;
    let fx = apply_remove(&mut reg, "notes").expect("existed");
    store.commit(&reg, &fx).expect("commit");
    assert!(secrets.items().is_empty(), "{:?}", secrets.items().keys());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_registry_is_an_error_not_an_empty_list() {
    let (store, _secrets, dir) = store_with("corrupt");
    std::fs::write(store.registry_path(), "{not json").expect("write");
    assert!(store.load().is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- sidecar wiring

#[test]
fn hermes_spec_carries_an_allowlist_only_when_a_member_server_is_on() {
    let (store, _secrets, dir) = store_with("spec");
    let mgr = crate::hermes::HermesManager::new(
        dir.join("bin"),
        dir.join("token"),
        dir.join("crash.jsonl"),
    );
    let env: BTreeMap<String, String> = mgr.spec_env_for_test().into_iter().collect();
    assert!(
        !env.contains_key(crate::hermes_mcp::MCP_CONFIG_ENV),
        "unset by default"
    );
    let mgr = mgr.with_mcp_servers(store.clone());
    let env: BTreeMap<String, String> = mgr.spec_env_for_test().into_iter().collect();
    assert!(
        !env.contains_key(crate::hermes_mcp::MCP_CONFIG_ENV),
        "no server on: still unset"
    );
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    enable_reviewed(&mut reg, "notes");
    store.commit(&reg, &fx).expect("commit");
    let env: BTreeMap<String, String> = mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(
        env.get(crate::hermes_mcp::MCP_CONFIG_ENV).map(String::as_str),
        Some(
            dir.join(crate::hermes_mcp::EFFECTIVE_FILE)
                .to_string_lossy()
                .as_ref()
        )
    );
    mgr.stop();
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
    apply_save(&mut reg, stdio_input("notes"), mem()).expect("save");
    let body = serde_json::to_string(&ok_probe("notes")).expect("json");
    let mgr = probe_manager(200, body);
    let out = mgr.mcp_probe(&entry_text(&reg.servers[0])).expect("probe");
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
        .mcp_probe("{\"name\": \"a\"}")
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
    assert!(mgr.mcp_probe("{}").is_err());
}

#[test]
fn mask_never_shows_any_part_of_the_value() {
    // Not even its length: the view is built from the registry, which holds no value.
    let mut reg = Registry::default();
    let mut long = stdio_input("long");
    long.env[0].value = Some("x".repeat(500));
    apply_save(&mut reg, long, mem()).expect("save");
    apply_save(&mut reg, stdio_input("short"), mem()).expect("save");
    let v = views(&reg);
    assert_eq!(v[0].env[0].masked, v[1].env[0].masked);
    assert!(!MASK.contains("500") && !MASK.contains('x'));
}

// ---------------------------------------------------------------- RC52 P-9a: values in the keychain

/// A registry file as versions before RC52 wrote it: env values in plain text, an enabled entry
/// whose review matches the old fingerprint, and the old allowlist next to it.
fn write_legacy(store: &McpStore, values: &[(&str, &str)]) -> String {
    let env: serde_json::Map<String, serde_json::Value> = values
        .iter()
        .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
        .collect();
    let mut entry = serde_json::json!({
        "name": "notes",
        "transport": "stdio",
        "command": "/opt/notes/bin/notes-mcp",
        "args": ["--root", "/Users/me/notes"],
        "cwd": null,
        "url": null,
        "env": env,
        "allow_write_tools": false,
        "enabled": true,
        "reviewed": null,
    });
    let parsed: StoredServer = serde_json::from_value(entry.clone()).expect("legacy entry");
    entry["reviewed"] = serde_json::Value::String(legacy_fingerprint(&parsed));
    let text = serde_json::to_string_pretty(&serde_json::json!({ "servers": [entry] }))
        .expect("json");
    std::fs::create_dir_all(store.dir()).expect("dir");
    std::fs::write(store.registry_path(), &text).expect("write registry");
    std::fs::write(
        store.legacy_allowlist_path(),
        serde_json::json!({ "servers": [{"name": "notes", "env": env}] }).to_string(),
    )
    .expect("write allowlist");
    text
}

/// Every file under `dir`, read as text.
fn all_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).expect("dir").flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(all_files(&p));
        } else {
            out.push((p.clone(), String::from_utf8_lossy(&std::fs::read(&p).expect("read")).to_string()));
        }
    }
    out
}

#[test]
fn saved_env_values_live_in_the_keychain_and_never_in_the_file() {
    let (store, secrets, dir) = store_with("kc-save");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    store.commit(&reg, &fx).expect("commit");
    let text = std::fs::read_to_string(store.registry_path()).expect("file");
    assert!(!text.contains("tok-abc-123"), "{text}");
    let items = secrets.items();
    assert_eq!(items.len(), 1, "{:?}", items.keys());
    let (account, value) = items.iter().next().expect("one item");
    assert!(
        account.starts_with("mcp-server-env:notes:NOTES_TOKEN:"),
        "{account}"
    );
    assert_eq!(value, "tok-abc-123");
    assert_eq!(
        keychain_accounts(&text)
            .into_iter()
            .map(|(a, _)| a)
            .collect::<Vec<_>>(),
        vec![account.clone()]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn migration_moves_plain_text_values_to_the_keychain_and_keeps_the_review() {
    let (store, secrets, dir) = store_with("kc-migrate");
    write_legacy(&store, &[("NOTES_TOKEN", "tok-abc-123"), ("OTHER", "zz-9")]);
    let reg = store.load().expect("migrated");
    let text = std::fs::read_to_string(store.registry_path()).expect("file");
    for v in ["tok-abc-123", "zz-9"] {
        assert!(!text.contains(v), "{text}");
    }
    let file: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert!(file["servers"][0].get("env").is_none(), "{text}");
    assert_eq!(secrets.items().len(), 2);
    assert!(!store.legacy_allowlist_path().exists(), "old allowlist removed");
    let v = views(&reg);
    assert!(v[0].enabled && !v[0].needs_review, "a reviewed server stays reviewed");
    let allow = runtime_allowlist_json(&reg, store.secrets(), &[])
        .expect("resolved")
        .expect("one on");
    assert!(allow.contains("\"NOTES_TOKEN\":\"tok-abc-123\""), "values reach the spawn");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn migration_rerun_changes_nothing() {
    let (store, secrets, dir) = store_with("kc-rerun");
    write_legacy(&store, &[("NOTES_TOKEN", "tok-abc-123")]);
    store.load().expect("first");
    let after_first = std::fs::read(store.registry_path()).expect("file");
    let sets = secrets.set_count();
    let items = secrets.items();
    store.load().expect("second");
    assert_eq!(std::fs::read(store.registry_path()).expect("file"), after_first);
    assert_eq!(secrets.set_count(), sets, "no new keychain write");
    assert_eq!(secrets.items(), items);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_keychain_failure_mid_migration_leaves_the_file_as_it_was_and_no_items_behind() {
    let (store, secrets, dir) = store_with("kc-partial");
    let original = write_legacy(&store, &[("A_TOKEN", "first-v"), ("B_TOKEN", "second-v")]);
    *secrets.fail_set_after.lock().expect("lock") = Some(1);
    assert!(store.load().is_err(), "fails closed");
    assert_eq!(
        std::fs::read_to_string(store.registry_path()).expect("file"),
        original
    );
    assert!(secrets.items().is_empty(), "{:?}", secrets.items().keys());
    // The next load (keychain back) finishes the job.
    *secrets.fail_set_after.lock().expect("lock") = None;
    store.load().expect("retry");
    let text = std::fs::read_to_string(store.registry_path()).expect("file");
    assert!(!text.contains("first-v") && !text.contains("second-v"), "{text}");
    assert_eq!(secrets.items().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_file_rewrite_after_the_keychain_writes_removes_the_new_items() {
    let (store, secrets, dir) = store_with("kc-rewrite");
    let original = write_legacy(&store, &[("NOTES_TOKEN", "tok-abc-123")]);
    // The atomic rewrite's temp path is taken by a folder, so the rewrite fails.
    let blocker = store.registry_path().with_extension("json.tmp");
    std::fs::create_dir_all(&blocker).expect("blocker");
    assert!(store.load().is_err());
    assert_eq!(
        std::fs::read_to_string(store.registry_path()).expect("file"),
        original
    );
    assert!(secrets.items().is_empty(), "{:?}", secrets.items().keys());
    std::fs::remove_dir(&blocker).expect("unblock");
    store.load().expect("retry");
    assert_eq!(secrets.items().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Tripwire (bug class: a secret value persisted in a file). Whatever the values look like, after
/// the migration no file the app keeps holds any of them, raw or JSON-escaped, and the registry
/// type never serializes values.
#[test]
fn tripwire_no_env_value_survives_in_any_file_after_migration() {
    let values = [
        ("QUOTE", "a\"b\\c"),
        ("UNICODE", "pässwörd-✓"),
        ("SPACE", "  leading and trailing  "),
        ("LONG", "L0ng-v4lue-that-looks-like-a-token-0123456789abcdef"),
        ("JSONISH", "{\"servers\":[]}"),
    ];
    let (store, _secrets, dir) = store_with("kc-tripwire");
    write_legacy(&store, &values);
    store.load().expect("migrated");
    for (path, text) in all_files(&dir) {
        for (_, v) in values {
            let escaped = serde_json::to_string(v).expect("escape");
            let escaped = escaped.trim_matches('"');
            assert!(
                !text.contains(v) && !text.contains(escaped),
                "{} holds a value",
                path.display()
            );
        }
    }
    let mut s: StoredServer = serde_json::from_value(serde_json::json!({
        "name": "a", "transport": "stdio", "env": {"K": "plain-secret"}
    }))
    .expect("legacy");
    s.env_refs.insert("K".into(), "0".repeat(32));
    let out = serde_json::to_string(&s).expect("json");
    assert!(!out.contains("plain-secret") && !out.contains("\"env\""), "{out}");
    assert!(!format!("{s:?}").contains("plain-secret"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_moves_values_to_items_under_the_new_name_and_remove_deletes_them() {
    let (store, secrets, dir) = store_with("kc-rename");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    store.commit(&reg, &fx).expect("commit");
    let mut r = stdio_input("notes2");
    r.previous_name = Some("notes".into());
    r.env[0].value = None;
    let fx = apply_save(&mut reg, r, store.secrets()).expect("rename");
    store.commit(&reg, &fx).expect("commit");
    let items = secrets.items();
    assert_eq!(items.len(), 1, "{:?}", items.keys());
    assert!(items
        .keys()
        .all(|a| a.starts_with("mcp-server-env:notes2:NOTES_TOKEN:")));
    assert!(entry_text_in(&store, &reg.servers[0]).contains("tok-abc-123"));
    let fx = apply_remove(&mut reg, "notes2").expect("existed");
    store.commit(&reg, &fx).expect("commit");
    assert!(secrets.items().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

fn entry_text_in(store: &McpStore, s: &StoredServer) -> String {
    runtime_entry_json(s, store.secrets())
        .expect("resolved")
        .to_string()
}

#[test]
fn retyping_the_same_value_keeps_the_item_and_a_new_value_replaces_it() {
    let (store, secrets, dir) = store_with("kc-retype");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    store.commit(&reg, &fx).expect("commit");
    let first = secrets.items();
    let mut same = stdio_input("notes");
    same.previous_name = Some("notes".into());
    let fx = apply_save(&mut reg, same, store.secrets()).expect("same");
    assert_eq!(fx, SaveEffects::default(), "nothing written, nothing dropped");
    store.commit(&reg, &fx).expect("commit");
    assert_eq!(secrets.items(), first);
    let mut other = stdio_input("notes");
    other.previous_name = Some("notes".into());
    other.env[0].value = Some("tok-new".into());
    let fx = apply_save(&mut reg, other, store.secrets()).expect("new value");
    store.commit(&reg, &fx).expect("commit");
    let items = secrets.items();
    assert_eq!(items.len(), 1, "the replaced item is removed");
    assert_eq!(items.values().next().map(String::as_str), Some("tok-new"));
    assert!(first.keys().all(|a| !items.contains_key(a)), "a new item, never an overwrite");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_keychain_failure_on_save_stores_nothing() {
    let (store, secrets, dir) = store_with("kc-savefail");
    let mut reg = Registry::default();
    let mut two = stdio_input("notes");
    two.env.push(EnvInput {
        key: "SECOND".into(),
        value: Some("v2".into()),
    });
    *secrets.fail_set_after.lock().expect("lock") = Some(1);
    let errs = apply_save(&mut reg, two, store.secrets()).expect_err("keychain down");
    assert_eq!(fields(&errs), vec!["entry"]);
    assert!(reg.servers.is_empty());
    assert!(secrets.items().is_empty(), "the first item was removed again");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_file_save_removes_the_items_the_save_wrote() {
    let (store, secrets, dir) = store_with("kc-commitfail");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    std::fs::create_dir_all(store.registry_path().with_extension("json.tmp")).expect("blocker");
    assert!(store.commit(&reg, &fx).is_err());
    assert!(secrets.items().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_keychain_item_fails_closed() {
    let (store, secrets, dir) = store_with("kc-missing");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    store.commit(&reg, &fx).expect("commit");
    enable_reviewed(&mut reg, "notes");
    for a in secrets.items().keys() {
        secrets.delete(a).expect("delete");
    }
    assert!(runtime_entry_json(&reg.servers[0], store.secrets()).is_err());
    assert!(runtime_allowlist_json(&reg, store.secrets(), &[]).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_probe_file_holds_the_entry_only_during_the_probe() {
    let (store, _secrets, dir) = store_with("kc-probe");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    store.commit(&reg, &fx).expect("commit");
    let body = runtime_entry_json(&reg.servers[0], store.secrets()).expect("resolved");
    assert!(!store.probe_path().exists());
    let seen = store
        .with_probe_file(&body, || {
            let text = std::fs::read_to_string(store.probe_path()).expect("probe file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(store.probe_path())
                    .expect("meta")
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(mode, 0o600);
            }
            text
        })
        .expect("probe");
    // The sidecar reads the entry from `servers` in the shape it compares with the request.
    let v: serde_json::Value = serde_json::from_str(&seen).expect("json");
    assert_eq!(v["servers"][0]["env"]["NOTES_TOKEN"], "tok-abc-123");
    assert_eq!(v["servers"][0]["name"], "notes");
    assert!(!store.probe_path().exists(), "removed when the probe returns");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn leftover_spawn_and_probe_files_are_swept() {
    let dir = tmp_dir("sweep");
    for f in [crate::hermes_mcp::EFFECTIVE_FILE, PROBE_FILE, REGISTRY_FILE] {
        std::fs::write(dir.join(f), "{}").expect("write");
    }
    sweep_runtime_files(&dir);
    assert!(!dir.join(crate::hermes_mcp::EFFECTIVE_FILE).exists());
    assert!(!dir.join(PROBE_FILE).exists());
    assert!(dir.join(REGISTRY_FILE).exists(), "the server list is kept");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn env_input_debug_never_shows_the_value() {
    let i = stdio_input("notes");
    assert!(!format!("{i:?}").contains("tok-abc-123"));
}

/// A pid with no child processes (a child that already ended and was reaped), so the managed-
/// browser half of the watch finds nothing to record.
fn childless_pid() -> u32 {
    let mut c = std::process::Command::new(if cfg!(windows) { "cmd" } else { "true" });
    if cfg!(windows) {
        c.args(["/C", "exit"]);
    }
    let mut child = c.spawn().expect("spawn");
    let pid = child.id();
    let _ = child.wait();
    pid
}

/// The spawn-time allowlist holds the member's values only until the child is up: it is removed
/// once `/health` answers, written again before a supervisor restart, and removed when Hermes
/// stops.
#[test]
fn the_spawn_allowlist_lives_only_until_the_child_has_loaded_it() {
    let (store, _secrets, dir) = store_with("kc-spawn");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    enable_reviewed(&mut reg, "notes");
    store.commit(&reg, &fx).expect("commit");
    // A stand-in for the child's control port: answers /health with 200.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for mut c in listener.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    let mgr = crate::hermes::HermesManager::new(
        dir.join("bin"),
        dir.join("token"),
        dir.join("crash.jsonl"),
    )
    .with_control_addr(&addr)
    .with_mcp_servers(store.clone());
    let spec = mgr.spec_for_test();
    let path = dir.join(crate::hermes_mcp::EFFECTIVE_FILE);
    assert!(path.exists(), "written at spawn");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("tok-abc-123"));
    let watch = spec.child_watch.expect("a watch for the spawn-time file");
    (watch.observe)(childless_pid());
    assert!(!path.exists(), "removed once the child answers /health");
    (watch.ended)();
    assert!(
        std::fs::read_to_string(&path)
            .expect("written for the restart")
            .contains("tok-abc-123")
    );
    mgr.stop();
    assert!(!path.exists(), "removed when Hermes stops");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unanswered_health_check_keeps_the_spawn_allowlist_for_the_child() {
    let (store, _secrets, dir) = store_with("kc-spawn-wait");
    let mut reg = Registry::default();
    let fx = apply_save(&mut reg, stdio_input("notes"), store.secrets()).expect("save");
    enable_reviewed(&mut reg, "notes");
    store.commit(&reg, &fx).expect("commit");
    // A port nobody listens on: the child has not loaded the file yet.
    let addr = std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .expect("addr")
        .to_string();
    let mgr = crate::hermes::HermesManager::new(
        dir.join("bin"),
        dir.join("token"),
        dir.join("crash.jsonl"),
    )
    .with_control_addr(&addr)
    .with_mcp_servers(store.clone());
    let watch = mgr.spec_for_test().child_watch.expect("watch");
    (watch.observe)(childless_pid());
    assert!(dir.join(crate::hermes_mcp::EFFECTIVE_FILE).exists());
    mgr.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
