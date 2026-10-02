// HUP-S4.3 — tests for the Hermes MCP allowlist writer. Included from hermes_mcp.rs.
use super::*;
use serde_json::Value;

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let d = std::env::temp_dir().join(format!("hmcp-{tag}-{nanos:x}"));
    std::fs::create_dir_all(&d).expect("tmp dir");
    d
}

fn mem_target() -> MemBridgeTarget {
    MemBridgeTarget {
        exe: PathBuf::from("/Applications/Citrate.app/Contents/MacOS/citrate-core"),
        socket: PathBuf::from(
            "/Users/m/Library/Application Support/ai.citrate.core/memory/memdag.sock",
        ),
    }
}

/// The keys agent-mcp-host's config parser accepts (it refuses unknown keys).
const HOST_KEYS: &[&str] = &[
    "name",
    "transport",
    "command",
    "args",
    "env",
    "cwd",
    "url",
    "timeout_ms",
    "init_timeout_ms",
    "max_response_bytes",
    "max_output_chars",
    "allow_write_tools",
];

/// Mirror of agent-mcp-host's acceptance rules, so a file core writes is one the host loads.
fn assert_host_accepts(cfg: &Value) {
    let obj = cfg.as_object().expect("top-level object");
    assert_eq!(
        obj.keys().collect::<Vec<_>>(),
        vec!["servers"],
        "only `servers` at top level"
    );
    let servers = cfg["servers"].as_array().expect("servers array");
    assert!(servers.len() <= 16);
    let mut names = std::collections::HashSet::new();
    for s in servers {
        for k in s.as_object().expect("server object").keys() {
            assert!(HOST_KEYS.contains(&k.as_str()), "unknown key {k}");
        }
        let name = s["name"].as_str().expect("name");
        assert!(!name.is_empty() && name.len() <= 24 && !name.contains("__"));
        assert!(name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'));
        assert!(names.insert(name.to_string()), "duplicate {name}");
        assert_eq!(
            s["allow_write_tools"],
            Value::Bool(false),
            "{name} must not allow writes"
        );
        match s["transport"].as_str() {
            Some("stdio") => {
                let cmd = s["command"].as_str().expect("command");
                assert!(Path::new(cmd).is_absolute(), "command must be absolute");
                assert!(s.get("url").is_none());
            }
            Some("http") => {
                let url = s["url"].as_str().expect("url");
                assert!(url.starts_with("https://"), "{url}");
                assert!(s.get("command").is_none());
            }
            other => panic!("bad transport {other:?}"),
        }
    }
}

#[test]
fn defaults_are_off_so_nothing_changes_for_members() {
    let s = McpSettings::default();
    assert!(!s.mem && !s.scan);
    assert!(render_config(&s, Some(&mem_target())).is_none());
}

#[test]
fn scan_renders_the_public_explorer_mcp_endpoint_read_only() {
    let s = McpSettings {
        mem: false,
        scan: true,
        node: false,
    };
    let cfg = render_config(&s, None).expect("one server");
    assert_host_accepts(&cfg);
    let servers = cfg["servers"].as_array().expect("servers");
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0]["name"], "scan");
    assert_eq!(servers[0]["transport"], "http");
    assert_eq!(servers[0]["url"], SCAN_MCP_URL);
    assert_eq!(
        SCAN_MCP_URL,
        format!("{}/api/mcp", crate::activity::EXPLORER_BASE)
    );
}

#[test]
fn mem_renders_core_itself_as_the_stdio_bridge() {
    let s = McpSettings {
        mem: true,
        scan: false,
        node: false,
    };
    let cfg = render_config(&s, Some(&mem_target())).expect("one server");
    assert_host_accepts(&cfg);
    let m = &cfg["servers"][0];
    assert_eq!(m["name"], "mem");
    assert_eq!(m["transport"], "stdio");
    assert_eq!(
        m["command"],
        "/Applications/Citrate.app/Contents/MacOS/citrate-core"
    );
    assert_eq!(
        m["args"],
        serde_json::json!([
            crate::mem_mcp_bridge::BRIDGE_FLAG,
            "/Users/m/Library/Application Support/ai.citrate.core/memory/memdag.sock"
        ])
    );
    // No env: the bridge needs nothing from the environment, so nothing is passed.
    assert!(m.get("env").is_none());
}

#[test]
fn mem_without_a_resolvable_bridge_is_left_out_not_guessed() {
    let s = McpSettings {
        mem: true,
        scan: false,
        node: false,
    };
    assert!(render_config(&s, None).is_none());
    let both = McpSettings {
        mem: true,
        scan: true,
        node: false,
    };
    let cfg = render_config(&both, None).expect("scan only");
    assert_eq!(cfg["servers"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(cfg["servers"][0]["name"], "scan");
}

#[test]
fn a_relative_bridge_executable_is_refused() {
    let t = MemBridgeTarget {
        exe: PathBuf::from("citrate-core"),
        socket: PathBuf::from("/s/memdag.sock"),
    };
    let s = McpSettings {
        mem: true,
        scan: false,
        node: false,
    };
    assert!(render_config(&s, Some(&t)).is_none());
}

#[test]
fn sync_writes_the_file_when_enabled_and_removes_it_when_disabled() {
    let dir = tmp_dir("sync");
    let on = McpSettings {
        mem: true,
        scan: true,
        node: false,
    };
    let path = sync_config_file(&dir, &on, Some(&mem_target()))
        .expect("io")
        .expect("written");
    assert_eq!(path, config_path(&dir));
    let text = std::fs::read_to_string(&path).expect("read");
    let v: Value = serde_json::from_str(&text).expect("json");
    assert_host_accepts(&v);
    assert_eq!(v["servers"].as_array().map(|a| a.len()), Some(2));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let off = McpSettings::default();
    assert!(sync_config_file(&dir, &off, Some(&mem_target()))
        .expect("io")
        .is_none());
    assert!(
        !path.exists(),
        "a disabled config leaves no file, so the sidecar runs no MCP"
    );
}

#[test]
fn settings_round_trip_and_a_missing_or_corrupt_file_is_the_default() {
    let dir = tmp_dir("settings");
    assert_eq!(load_settings(&dir), McpSettings::default());
    let s = McpSettings {
        mem: false,
        scan: true,
        node: false,
    };
    save_settings(&dir, &s).expect("save");
    assert_eq!(load_settings(&dir), s);
    std::fs::write(settings_path(&dir), "{not json").expect("write");
    assert_eq!(load_settings(&dir), McpSettings::default());
}

#[test]
fn the_view_lists_both_servers_and_says_why_one_is_unavailable() {
    let s = McpSettings {
        mem: true,
        scan: false,
        node: false,
    };
    let v = build_view(&s, None, false, false);
    assert_eq!(v.servers.len(), 3);
    let mem = v.servers.iter().find(|x| x.name == "mem").expect("mem row");
    assert!(mem.enabled && !mem.available);
    assert!(mem.detail.contains("not available"), "{}", mem.detail);
    let scan = v
        .servers
        .iter()
        .find(|x| x.name == "scan")
        .expect("scan row");
    assert!(!scan.enabled && scan.available);
    assert!(!v.restart_required);
    let running = build_view(&s, Some(&mem_target()), true, true);
    assert!(
        running.restart_required,
        "a running sidecar reads the file only at start"
    );
}

// ---- HUP-S4.2 / S8.5: the node MCP server as a built-in server for Hermes ----

fn node_target() -> NodeShimTarget {
    NodeShimTarget {
        exe: PathBuf::from("/Applications/Citrate.app/Contents/MacOS/citrate-core"),
        port: 47204,
        // Built at runtime so no credential-looking literal sits in the source.
        token: format!("cnmcp_{}", "ab".repeat(32)),
    }
}

#[test]
fn node_is_off_by_default_and_left_out_without_a_running_server() {
    assert!(!McpSettings::default().node);
    let s = McpSettings {
        mem: false,
        scan: false,
        node: true,
    };
    assert!(render_config_with_node(&s, None, None).is_none());
    let off = McpSettings::default();
    assert!(render_config_with_node(&off, None, Some(&node_target())).is_none());
}

#[test]
fn node_renders_the_stdio_shim_with_its_own_token_read_only() {
    let s = McpSettings {
        mem: false,
        scan: true,
        node: true,
    };
    let t = node_target();
    let cfg = render_config_with_node(&s, None, Some(&t)).expect("two servers");
    assert_host_accepts(&cfg);
    let n = cfg["servers"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == "node"))
        .cloned()
        .expect("node entry");
    assert_eq!(n["transport"], "stdio");
    assert_eq!(n["command"], "/Applications/Citrate.app/Contents/MacOS/citrate-core");
    assert_eq!(n["args"], serde_json::json!(["--mcp-stdio"]));
    assert_eq!(n["env"]["CITRATE_NODE_MCP_TOKEN"], serde_json::json!(t.token));
    assert_eq!(n["env"]["CITRATE_NODE_MCP_PORT"], "47204");
    assert_eq!(n["allow_write_tools"], serde_json::json!(false));
    // Debug output never carries the token.
    assert!(!format!("{t:?}").contains(&t.token));
    // A relative executable or an empty token is refused, not guessed.
    let mut rel = node_target();
    rel.exe = PathBuf::from("citrate-core");
    let only_node = McpSettings {
        mem: false,
        scan: false,
        node: true,
    };
    assert!(render_config_with_node(&only_node, None, Some(&rel)).is_none());
    let mut empty = node_target();
    empty.token.clear();
    assert!(render_config_with_node(&only_node, None, Some(&empty)).is_none());
}

#[test]
fn the_node_row_is_available_only_while_the_node_server_runs() {
    let s = McpSettings {
        mem: false,
        scan: false,
        node: true,
    };
    let off = build_view_with_node(&s, None, false, false, false);
    let row = off.servers.iter().find(|x| x.name == "node").expect("node row");
    assert!(row.enabled && !row.available);
    assert!(row.detail.contains("Node MCP server"), "{}", row.detail);
    let on = build_view_with_node(&s, None, true, false, false);
    let row = on.servers.iter().find(|x| x.name == "node").expect("node row");
    assert!(row.available);
}

#[test]
fn node_settings_round_trip_and_old_files_read_as_off() {
    let dir = tmp_dir("node-settings");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(settings_path(&dir), r#"{"mem":true,"scan":false}"#).expect("write");
    let old = load_settings(&dir);
    assert!(old.mem && !old.node, "a file from before the node switch reads as node off");
    let s = McpSettings {
        mem: false,
        scan: false,
        node: true,
    };
    save_settings(&dir, &s).expect("save");
    assert_eq!(load_settings(&dir), s);
}
