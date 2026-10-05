// Hermes terminal commands, end to end with the REAL sidecar (env-gated; test-only).
//
// `CITRATE_E2E_SHELL_SIDECAR_BIN` names a built `citrate-agent-sidecar` (citrate-agent-runtime,
// `cargo build -p agent-sidecar`). Without it this test says so and passes without running, like
// the other live harnesses. With it, the production spawn path runs: a [`HermesManager`] whose
// env source is this module's [`file_env_source`] (the stored switch), so whether the sidecar
// offers `shell_run` is decided by exactly what core passes at spawn.
//
// The model is a scripted OpenAI-compatible server (a test double that only decides which tool to
// call and records what it was sent). Everything else is real: the sidecar's tool offer, the
// held command, core's `hermes_shell` pending/decide calls (what the approval card uses), the OS
// sandbox (Seatbelt on macOS, bubblewrap on Linux) and the tool result the model reads back.
//
// Proved:
// 1. Switch on + a conversation with a shared folder: the model is offered `shell_run`.
// 2. The call is held (HIC required) until the member decides; core reads it pending with the
//    exact argv and folder.
// 3. Allow: the command runs in the sandbox, writes in the shared folder, and its output is the
//    tool result the model reads.
// 4. Decline (a fresh conversation): nothing runs, and the model reads a refusal.
// 5. Switch off + restart: a conversation with the same shared folder is not offered `shell_run`.
use super::*;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BIN_ENV: &str = "CITRATE_E2E_SHELL_SIDECAR_BIN";
const MARK: &str = "shell-e2e-ok";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

/// Every request the scripted model received.
type Seen = Arc<Mutex<Vec<serde_json::Value>>>;

fn tool_names(req: &serde_json::Value) -> Vec<String> {
    req["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|t| {
            t["function"]["name"]
                .as_str()
                .or_else(|| t["name"].as_str())
                .map(str::to_string)
        })
        .collect()
}

/// The scripted model: offered `shell_run` and asked for a command, it proposes the one named in
/// the user's text (after `ARGV:` as JSON) in `cwd`; given a tool result, it answers with it.
fn reply(req: &serde_json::Value, cwd: &str) -> serde_json::Value {
    let msgs = req["messages"].as_array().cloned().unwrap_or_default();
    let last = msgs.last().cloned().unwrap_or_default();
    let message = if last["role"] == "tool" {
        let content = last["content"].as_str().unwrap_or_default();
        serde_json::json!({ "role": "assistant", "content": format!("The command result: {}", content.chars().take(400).collect::<String>()) })
    } else if tool_names(req).iter().any(|n| n == "shell_run") {
        let text = last["content"].as_str().unwrap_or_default();
        let argv: serde_json::Value = text
            .split_once("ARGV:")
            .and_then(|(_, a)| serde_json::from_str(a.trim()).ok())
            .unwrap_or_else(|| serde_json::json!(["true"]));
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": format!("call_shell_{}", msgs.len()),
                "type": "function",
                "function": {
                    "name": "shell_run",
                    "arguments": serde_json::json!({ "argv": argv, "cwd": cwd, "timeout_secs": 30 }).to_string()
                }
            }]
        })
    } else {
        serde_json::json!({ "role": "assistant", "content": "I cannot run commands in this conversation." })
    };
    serde_json::json!({ "choices": [{ "index": 0, "message": message, "finish_reason": "stop" }] })
}

fn serve_one(mut s: TcpStream, cwd: &str, seen: &Seen) -> Result<(), String> {
    let mut r = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    r.read_line(&mut request_line).map_err(|e| e.to_string())?;
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        r.read_line(&mut line).map_err(|e| e.to_string())?;
        let l = line.trim();
        if l.is_empty() {
            break;
        }
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().map_err(|_| "bad content-length")?;
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    // Only chat completions are modelled; anything else the sidecar probes (a llama-server
    // tokenizer, say) is answered 404, as a plain OpenAI-compatible server would.
    if !request_line.contains("/chat/completions") {
        return write!(
            s,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .map_err(|e| e.to_string());
    }
    let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let out = reply(&req, cwd).to_string();
    seen.lock().unwrap_or_else(|e| e.into_inner()).push(req);
    write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    )
    .map_err(|e| e.to_string())
}

fn start_model(cwd: String, seen: Seen) -> String {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind model");
    let port = l.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let _ = serve_one(s, &cwd, &seen);
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

/// The member's folder grants on `root` (read + write, whole subtree), as core sends them.
fn grants_for(root: &Path) -> crate::agent_grants::GrantState {
    use crate::agent_grants::{Access, Grant, GrantKind, GrantState};
    let grant = |id: &str, access| Grant {
        id: id.into(),
        kind: GrantKind::Folder,
        root: root.display().to_string(),
        access,
        scope: "subtree".into(),
        granted_at: 1,
        expires_at: None,
        granted_by: "member".into(),
        reason: "Granted in Settings".into(),
        revoked_at: None,
    };
    GrantState {
        version: crate::agent_grants::GRANTS_VERSION,
        next_id: 3,
        grants: vec![grant("g-1", Access::Read), grant("g-2", Access::Write)],
    }
}

fn wait_until<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Open a conversation with the shared folder, send `text`, and return its id.
fn converse(m: &crate::hermes::HermesManager, model_url: &str, root: &Path, text: &str) -> String {
    let body = crate::hermes::build_session_body(
        "You are Hermes. Use shell_run for commands in the member's shared folder.",
        "[]",
        model_url,
        "",
        "scripted",
        8192,
    )
    .expect("body");
    let body = crate::agent_grants::attach_grants(&body, &grants_for(root)).expect("grants");
    let id = m.session_open(&body).expect("session open");
    m.session_send(&id, text).expect("send");
    id
}

/// The first held command in `session` (what the approval card shows).
fn held(m: &crate::hermes::HermesManager, session: &str) -> serde_json::Value {
    wait_until("the held shell_run command", Duration::from_secs(60), || {
        crate::hermes::shell::shell_pending(m, session)
            .ok()
            .and_then(|v| v.as_array().and_then(|a| a.first().cloned()))
    })
}

/// The tool result the model read for `call_marker` (the first request whose last message is a
/// tool message), after `from` requests.
fn tool_result_seen(seen: &Seen, from: usize) -> String {
    wait_until("the tool result at the model", Duration::from_secs(90), || {
        let g = seen.lock().unwrap_or_else(|e| e.into_inner());
        g.iter().skip(from).find_map(|r| {
            let last = r["messages"].as_array()?.last()?.clone();
            (last["role"] == "tool").then(|| last["content"].as_str().unwrap_or_default().to_string())
        })
    })
}

fn argv_of(v: &serde_json::Value) -> Vec<String> {
    v["argv"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|a| a.as_str().map(str::to_string))
        .collect()
}

#[test]
fn shell_run_live_with_the_real_sidecar() {
    let Ok(bin) = std::env::var(BIN_ENV) else {
        eprintln!("skipped: set {BIN_ENV} to a built citrate-agent-sidecar to run the live shell_run e2e");
        return;
    };
    let base = std::env::temp_dir().join(format!("hermes-term-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let hermes_dir = base.join("hermes");
    let project = base.join("project");
    std::fs::create_dir_all(&hermes_dir).expect("hermes dir");
    std::fs::create_dir_all(&project).expect("project");
    let project = std::fs::canonicalize(&project).expect("canon");
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let model_url = start_model(project.display().to_string(), seen.clone());

    let m = crate::hermes::HermesManager::new(
        PathBuf::from(&bin),
        hermes_dir.join("bearer.token"),
        hermes_dir.join("crashes.log"),
    )
    .with_control_addr(&format!("127.0.0.1:{}", free_port()))
    .with_health_interval(Duration::from_millis(200))
    .with_env_source(file_env_source(hermes_dir.clone()));
    let start = |m: &crate::hermes::HermesManager| {
        m.start().expect("start the sidecar");
        let health = format!("{}/health", m.control_url());
        wait_until("the sidecar to answer /health", Duration::from_secs(30), || {
            crate::serve::http_health_ok(&health).then_some(())
        });
    };

    // 1-3: default switch (on), a shared folder, allow.
    assert!(enabled(&hermes_dir), "the default is on");
    start(&m);
    let before = seen.lock().unwrap_or_else(|e| e.into_inner()).len();
    let s1 = converse(
        &m,
        &model_url,
        &project,
        r#"Run this terminal command with shell_run in my shared folder. ARGV: ["sh", "-c", "echo shell-e2e-ok; echo made > made-by-hermes.txt"]"#,
    );
    let first = wait_until("the model's first request", Duration::from_secs(30), || {
        seen.lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(before)
            .cloned()
    });
    assert!(
        tool_names(&first).iter().any(|n| n == "shell_run"),
        "a conversation with a shared folder is offered shell_run: {:?} in {}",
        tool_names(&first),
        first.to_string().chars().take(3000).collect::<String>()
    );
    let p = held(&m, &s1);
    assert_eq!(p["hic"], "required", "{p}");
    assert_eq!(
        argv_of(&p),
        ["sh", "-c", "echo shell-e2e-ok; echo made > made-by-hermes.txt"]
    );
    let cwd = p["cwd"].as_str().expect("cwd").to_string();
    assert_eq!(Path::new(&cwd), project.as_path());
    assert!(
        !project.join("made-by-hermes.txt").exists(),
        "nothing runs before the member decides"
    );
    let id = p["id"].as_str().expect("id").to_string();
    // A decision for a different command is refused (bound to what the card showed).
    let e = crate::hermes::shell::shell_decide(&m, &s1, &id, true, &["ls".to_string()], &cwd)
        .expect_err("a different argv is refused");
    assert!(e.starts_with("SHELL_DECISION_REFUSED"), "{e}");
    crate::hermes::shell::shell_decide(&m, &s1, &id, true, &argv_of(&p), &cwd).expect("allow");
    let out = tool_result_seen(&seen, before);
    assert!(out.contains(MARK), "the command's output reached the model: {out}");
    let sandbox = p["sandbox"].to_string();
    assert!(
        !sandbox.is_empty() && sandbox != "null",
        "the card names the sandbox: {p}"
    );
    assert_eq!(
        std::fs::read_to_string(project.join("made-by-hermes.txt"))
            .expect("the command wrote in the shared folder")
            .trim(),
        "made"
    );
    eprintln!("allow: sandbox {sandbox}; model read: {}", out.chars().take(300).collect::<String>());
    let _ = m.session_close(&s1);

    // 4: decline, in a fresh conversation (the first one read command output, so it is tainted).
    let before = seen.lock().unwrap_or_else(|e| e.into_inner()).len();
    let s2 = converse(
        &m,
        &model_url,
        &project,
        r#"Run this terminal command with shell_run in my shared folder. ARGV: ["sh", "-c", "echo should-not-run > declined.txt"]"#,
    );
    let p = held(&m, &s2);
    let id = p["id"].as_str().expect("id").to_string();
    crate::hermes::shell::shell_decide(&m, &s2, &id, false, &argv_of(&p), &cwd).expect("decline");
    let out = tool_result_seen(&seen, before);
    assert!(
        out.to_lowercase().contains("declined"),
        "the model reads a refusal: {out}"
    );
    assert!(!out.contains("should-not-run"));
    assert!(!project.join("declined.txt").exists(), "a declined command never runs");
    eprintln!("decline: model read: {}", out.chars().take(300).collect::<String>());
    let _ = m.session_close(&s2);

    // 5: switch off; core restarts the sidecar; the same shared folder offers no shell_run.
    let st = apply(&hermes_dir, false, 1, || {
        m.stop();
        start(&m);
        Ok(true)
    })
    .expect("off");
    assert!(st.restarted && !st.enabled);
    let before = seen.lock().unwrap_or_else(|e| e.into_inner()).len();
    let s3 = converse(
        &m,
        &model_url,
        &project,
        r#"Run this terminal command with shell_run in my shared folder. ARGV: ["sh", "-c", "echo off > off.txt"]"#,
    );
    let first = wait_until("the model's first request (off)", Duration::from_secs(30), || {
        seen.lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(before)
            .cloned()
    });
    assert!(
        !tool_names(&first).iter().any(|n| n == "shell_run"),
        "off: shell_run is not offered: {:?}",
        tool_names(&first)
    );
    let _ = m.session_close(&s3);
    m.stop();
    assert!(!project.join("off.txt").exists());
    let _ = std::fs::remove_dir_all(&base);
}
