// HUP-S4.3 — tests for the read-only mem-mcp stdio bridge. Included from mem_mcp_bridge.rs.
use super::*;
use interprocess::local_socket::{prelude::*, ListenerOptions};
use interprocess::TryClone as _;
use serde_json::json;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::sync::{Arc, Mutex};

fn reply_error_code(g: &Gate) -> Option<i64> {
    match g {
        Gate::Reply(v) => v["error"]["code"].as_i64(),
        _ => None,
    }
}

#[test]
fn read_tools_are_forwarded_and_write_tools_are_refused_locally() {
    for t in READ_ONLY_MEMORY_TOOLS {
        let line = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":t,"arguments":{}}})
            .to_string();
        assert!(
            matches!(gate_request(&line), Gate::Forward { .. }),
            "{t} should be forwarded"
        );
    }
    for t in [
        "memory.assert",
        "memory.propose_edge",
        "memory.confirm_edge",
        "memory.merge_diff",
        "memory.something_new",
    ] {
        let line = json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":t,"arguments":{}}})
            .to_string();
        let g = gate_request(&line);
        assert_eq!(
            reply_error_code(&g),
            Some(-32602),
            "{t} must not reach the daemon"
        );
        if let Gate::Reply(v) = g {
            assert_eq!(v["id"], json!(7), "the refusal answers the caller's id");
        }
    }
}

#[test]
fn only_the_base_methods_pass_and_notifications_pass_silently() {
    for m in ["initialize", "ping", "tools/list"] {
        let line = json!({"jsonrpc":"2.0","id":"a","method":m,"params":{}}).to_string();
        assert!(matches!(gate_request(&line), Gate::Forward { .. }), "{m}");
    }
    let line = json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string();
    assert!(matches!(
        gate_request(&line),
        Gate::Forward { has_id: false, .. }
    ));
    // Unknown request method: answered locally, never forwarded.
    let line = json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{}}).to_string();
    assert_eq!(reply_error_code(&gate_request(&line)), Some(-32601));
    // Unknown notification: dropped.
    let line = json!({"jsonrpc":"2.0","method":"custom/thing"}).to_string();
    assert_eq!(gate_request(&line), Gate::Drop);
    // A tools/call with no name is refused.
    let line = json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{}}).to_string();
    assert_eq!(reply_error_code(&gate_request(&line)), Some(-32602));
}

#[test]
fn malformed_input_is_answered_not_forwarded() {
    assert_eq!(reply_error_code(&gate_request("{not json")), Some(-32700));
    assert_eq!(
        reply_error_code(&gate_request(
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#
        )),
        Some(-32600)
    );
    assert_eq!(gate_request("   "), Gate::Drop);
}

#[test]
fn tools_list_ids_are_tracked_so_only_that_response_is_rewritten() {
    let g = gate_request(&json!({"jsonrpc":"2.0","id":9,"method":"tools/list"}).to_string());
    assert_eq!(
        g,
        Gate::Forward {
            list_id: Some("9".into()),
            has_id: true
        }
    );
    let g = gate_request(&json!({"jsonrpc":"2.0","id":10,"method":"ping"}).to_string());
    assert_eq!(
        g,
        Gate::Forward {
            list_id: None,
            has_id: true
        }
    );
}

fn daemon_tools_list() -> Value {
    json!({"tools": [
        {"name":"memory.recall","description":"r","inputSchema":{"type":"object"}},
        {"name":"memory.assert","description":"w","inputSchema":{"type":"object"}},
        {"name":"memory.search","description":"s","inputSchema":{"type":"object"},
         "annotations":{"readOnlyHint":false,"destructiveHint":true}},
        {"name":"memory.merge_diff","description":"w","inputSchema":{"type":"object"}}
    ]})
}

#[test]
fn tools_list_keeps_only_read_tools_and_marks_them_read_only() {
    let mut result = daemon_tools_list();
    filter_tools_list(&mut result);
    let tools = result["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, vec!["memory.recall", "memory.search"]);
    for t in tools {
        assert_eq!(t["annotations"]["readOnlyHint"], json!(true));
        assert_eq!(t["annotations"]["destructiveHint"], json!(false));
        assert_eq!(t["annotations"]["openWorldHint"], json!(false));
    }
}

#[test]
fn rewrite_response_touches_only_tracked_tools_list_responses() {
    let mut ids = std::collections::HashSet::new();
    ids.insert("5".to_string());
    let list = json!({"jsonrpc":"2.0","id":5,"result":daemon_tools_list()}).to_string();
    let (out, id) = rewrite_response(&list, &mut ids);
    assert_eq!(id.as_deref(), Some("5"));
    let v: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["result"]["tools"].as_array().map(|a| a.len()), Some(2));
    assert!(ids.is_empty(), "the id is consumed");

    // Same body under an id that was not a tools/list request: untouched.
    let other = json!({"jsonrpc":"2.0","id":6,"result":daemon_tools_list()}).to_string();
    let (out, _) = rewrite_response(&other, &mut ids);
    assert_eq!(out, other);
}

/// Serve one connection on a real local socket: answer each request line in order, then close.
fn fake_daemon(sock: &std::path::Path) -> std::thread::JoinHandle<Vec<String>> {
    let name = crate::ipc_name::endpoint_name(&sock.to_string_lossy()).expect("endpoint name");
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .expect("bind fake daemon");
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        if let Ok(stream) = listener.accept() {
            let mut writer = stream.try_clone().expect("clone");
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                seen.push(v["method"].as_str().unwrap_or("").to_string());
                let Some(id) = v.get("id").cloned() else {
                    continue;
                };
                let result = match v["method"].as_str() {
                    Some("initialize") => {
                        json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},
                        "serverInfo":{"name":"citrate-memories","version":"t"}})
                    }
                    Some("tools/list") => daemon_tools_list(),
                    Some("tools/call") => {
                        json!({"content":[{"type":"text","text":"recalled"}],"isError":false})
                    }
                    _ => json!({}),
                };
                let _ = writeln!(
                    writer,
                    "{}",
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                );
                let _ = writer.flush();
            }
        }
        seen
    })
}

fn short_sock(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("mb{tag}{:x}", nanos % 0xffff_ffff));
    std::fs::create_dir_all(&dir).expect("dir");
    dir.join("m.sock")
}

#[derive(Clone, Default)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);
impl Write for SharedBuf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_bridge_relays_a_real_socket_session_and_never_forwards_a_write() {
    let sock = short_sock("a");
    let daemon = fake_daemon(&sock);
    let input = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory.assert","arguments":{"content":"x"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory.recall","arguments":{"repo":"personal"}}}),
    ]
    .iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let out = SharedBuf::default();
    run_bridge(
        &sock,
        Cursor::new(input.into_bytes()),
        Box::new(out.clone()),
        std::time::Duration::from_secs(2),
    )
    .expect("bridge runs");

    let text =
        String::from_utf8(out.0.lock().unwrap_or_else(|e| e.into_inner()).clone()).expect("utf8");
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).expect("each output line is JSON"))
        .collect();
    let by_id = |id: i64| {
        lines
            .iter()
            .find(|v| v["id"] == json!(id))
            .cloned()
            .unwrap_or(Value::Null)
    };
    assert_eq!(
        by_id(1)["result"]["serverInfo"]["name"],
        json!("citrate-memories")
    );
    let tools = by_id(2)["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(tools.len(), 2, "only read tools are listed: {tools:?}");
    assert_eq!(
        by_id(3)["error"]["code"],
        json!(-32602),
        "the write is refused"
    );
    assert_eq!(by_id(4)["result"]["content"][0]["text"], json!("recalled"));
    assert_eq!(
        lines.len(),
        4,
        "one answer per request, none for the notification"
    );

    let seen = daemon.join().expect("daemon thread");
    // The daemon saw initialize, the notification, tools/list, and exactly ONE tools/call (the read).
    assert_eq!(
        seen,
        vec![
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/call"
        ]
    );
}

#[test]
fn an_unreachable_daemon_is_an_honest_error() {
    let err = run_bridge(
        std::path::Path::new("/nonexistent/citrate-core-mb/nope.sock"),
        Cursor::new(Vec::new()),
        Box::new(SharedBuf::default()),
        std::time::Duration::from_millis(50),
    )
    .expect_err("no daemon");
    assert!(err.contains("memory daemon"), "{err}");
}

#[test]
fn the_flag_is_distinctive_and_not_a_tauri_or_webview_argument() {
    assert!(BRIDGE_FLAG.starts_with("--citrate-"));
    assert_eq!(
        parse_bridge_args(&["--citrate-mem-mcp-stdio".into(), "/s".into()]),
        Some(Ok("/s".into()))
    );
    assert!(matches!(
        parse_bridge_args(&["--citrate-mem-mcp-stdio".into()]),
        Some(Err(_))
    ));
    assert_eq!(parse_bridge_args(&["--other".into()]), None);
    assert_eq!(parse_bridge_args(&[]), None);
}
