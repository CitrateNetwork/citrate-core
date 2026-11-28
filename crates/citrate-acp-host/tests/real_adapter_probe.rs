//! L1-WP03 runtime evidence: drive a real, pinned ACP adapter through the host. Ignored by default
//! (it needs an installed adapter); run on the Linux build host only, never on a member's machine:
//!
//! ```text
//! ACP_PROBE_ID=codex ACP_PROBE_CMD=/usr/bin/node ACP_PROBE_ARGS='["/root/acp-probe/.../index.js"]' \
//! ACP_PROBE_PINNED=/root/acp-probe/.../index.js ACP_PROBE_SHA256=<hex> ACP_PROBE_HOME=<dir> \
//! ACP_PROBE_CWD=<dir> ACP_PROBE_OUT=<dir> ACP_PROBE_ENV='{"CODEX_HOME":"<dir>/.codex"}' \
//! cargo test -p citrate-acp-host --test real_adapter_probe -- --ignored --nocapture
//! ```
//!
//! No credential exists in the run: the environment is built here from scratch (PATH, LANG, the
//! scratch HOME and XDG folders, the adapter's own config variables), so a prompt is expected to end
//! in an auth or model error. That error is the evidence that the protocol path works. Every frame
//! both ways is written to `transcript.jsonl` and scanned for credential shapes.

use citrate_acp_host::permission::DenyByDefault;
use citrate_acp_host::process::{Direction, WireTap};
use citrate_acp_host::registry::{parse_sha256, AgentSpec};
use citrate_acp_host::{AcpHost, HostConfig, SessionStart};
use serde_json::{json, Value};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Credential shapes looked for in every frame and in stderr (vendor keys and Citrate route tokens).
fn credential_shape(s: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "sk-",
        "sk_",
        "AIza",
        "xai-",
        "ghp_",
        "gho_",
        "github_pat_",
        "AKIA",
        "cmp_",
    ];
    PREFIXES.iter().any(|p| {
        s.match_indices(p).any(|(i, _)| {
            let tail = &s[i + p.len()..];
            tail.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .count()
                >= 20
        })
    })
}

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is required"))
}

fn alive_in_group(pgid: u32) -> Vec<u32> {
    let mut out = vec![];
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let rest: Vec<&str> = stat
            .rsplit(')')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        // fields after the name: state ppid pgrp ...
        if rest.get(2).and_then(|g| g.parse::<u32>().ok()) == Some(pgid)
            && rest.first() != Some(&"Z")
        {
            out.push(pid);
        }
    }
    out
}

#[test]
#[ignore = "needs an installed adapter; run on the build host (see the module docs)"]
fn real_adapter_initialize_session_prompt_cancel_cleanup() {
    let id = var("ACP_PROBE_ID");
    let out = PathBuf::from(var("ACP_PROBE_OUT"));
    std::fs::create_dir_all(&out).expect("out dir");
    let home = var("ACP_PROBE_HOME");
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("HOME".into(), home.clone()),
        ("XDG_CONFIG_HOME".into(), format!("{home}/.config")),
        ("XDG_DATA_HOME".into(), format!("{home}/.local/share")),
        ("XDG_STATE_HOME".into(), format!("{home}/.local/state")),
        ("XDG_CACHE_HOME".into(), format!("{home}/.cache")),
    ];
    let extra: Value = serde_json::from_str(&std::env::var("ACP_PROBE_ENV").unwrap_or("{}".into()))
        .expect("env json");
    for (k, v) in extra.as_object().into_iter().flatten() {
        env.push((k.clone(), v.as_str().expect("string env").to_string()));
    }
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("ACP_PROBE_ARGS").unwrap_or("[]".into()))
            .expect("args");
    let spec = AgentSpec {
        id: id.clone(),
        command: PathBuf::from(var("ACP_PROBE_CMD")),
        args,
        pinned_file: PathBuf::from(var("ACP_PROBE_PINNED")),
        sha256: parse_sha256(&var("ACP_PROBE_SHA256")).expect("sha256"),
    };
    let frames: Arc<Mutex<Vec<Value>>> = Arc::default();
    let f = frames.clone();
    let t0 = Instant::now();
    let tap: WireTap = Arc::new(move |d, line: &str| {
        let dir = if d == Direction::ToAgent {
            "core->agent"
        } else {
            "agent->core"
        };
        let msg: Value = serde_json::from_str(line).unwrap_or_else(|_| json!({"unparsed": line}));
        f.lock()
            .expect("frames")
            .push(json!({"ms": t0.elapsed().as_millis() as u64, "dir": dir, "msg": msg}));
    });
    let host = AcpHost::new(HostConfig {
        client_name: "citrate-core-acp-probe".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        scan: Arc::new(credential_shape),
        policy: Arc::new(DenyByDefault),
        tap: Some(tap),
        init_timeout: Duration::from_secs(90),
        cancel_grace: Duration::from_secs(5),
    })
    .expect("host");
    let mut summary =
        json!({"id": id, "env_names": env.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()});
    let started = host.start(SessionStart {
        spec,
        env,
        cwd: PathBuf::from(var("ACP_PROBE_CWD")),
        mcp_servers: vec![],
    });
    match started {
        Err(e) => {
            let init = e.initialize.clone().unwrap_or(Value::Null);
            summary["start"] = json!({"ok": false, "reason": e.reason, "auth_required": e.auth_required,
                "agentInfo": init["agentInfo"], "authMethods": init["authMethods"], "protocolVersion": init["protocolVersion"],
                "stderr_tail_last_2k": e.stderr_tail.chars().rev().take(2000).collect::<String>().chars().rev().collect::<String>(),
                "stderr_credential_shape": credential_shape(&e.stderr_tail)});
        }
        Ok(session) => {
            let init = session.initialize_response().clone();
            summary["start"] = json!({"ok": true, "acp_session_id": session.acp_session_id(), "pid": session.pid(),
                "agentInfo": init["agentInfo"], "authMethods": init["authMethods"], "agentCapabilities": init["agentCapabilities"],
                "protocolVersion": init["protocolVersion"]});
            let s = Arc::new(session);
            // Turn 1: a plain prompt with no credential anywhere: expect an auth or model error.
            let t = Instant::now();
            let s1 = s.clone();
            let h = std::thread::spawn(move || {
                s1.prompt(vec![
                    json!({"type": "text", "text": "Reply with the single word: hello"}),
                ])
            });
            let mut waited = Duration::ZERO;
            while !h.is_finished() && waited < Duration::from_secs(60) {
                std::thread::sleep(Duration::from_millis(200));
                waited += Duration::from_millis(200);
            }
            let turn1 = if h.is_finished() {
                match h.join().expect("join") {
                    Ok(r) => json!({"stopReason": r}),
                    Err(e) => json!({"error": e}),
                }
            } else {
                s.cancel().expect("cancel");
                json!({"timed_out_then_cancelled": h.join().expect("join")})
            };
            summary["turn1"] = json!({"result": turn1, "ms": t.elapsed().as_millis() as u64});
            // Turn 2: cancel after one second, whatever the agent is doing.
            let s2 = s.clone();
            let t = Instant::now();
            let h = std::thread::spawn(move || {
                s2.prompt(vec![json!({"type": "text", "text": "Count slowly from 1 to 1000, one number per line."})])
            });
            std::thread::sleep(Duration::from_secs(1));
            let cancel = s.cancel();
            let turn2 = h.join().expect("join");
            summary["turn2_cancel"] = json!({"cancel": format!("{cancel:?}"), "result": format!("{turn2:?}"), "ms": t.elapsed().as_millis() as u64});
            let events = s.events(0, Duration::ZERO).0;
            let mut kinds = std::collections::BTreeMap::<String, usize>::new();
            for e in &events {
                *kinds.entry(e.kind.clone()).or_default() += 1;
            }
            summary["event_kinds"] = json!(kinds);
            summary["events_tail"] = json!(events.iter().rev().take(8).collect::<Vec<_>>());
            let pid = s.pid();
            let tail = s.stderr_tail();
            summary["stderr_tail_chars"] = json!(tail.len());
            summary["stderr_tail_last_2k"] = json!(tail
                .chars()
                .rev()
                .take(2000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>());
            summary["stderr_credential_shape"] = json!(credential_shape(&tail));
            let s = Arc::try_unwrap(s).ok().expect("one owner");
            s.close();
            std::thread::sleep(Duration::from_millis(500));
            summary["after_close_group_members"] = json!(alive_in_group(pid));
        }
    }
    let frames = frames.lock().expect("frames").clone();
    let flagged = frames
        .iter()
        .filter(|f| credential_shape(&f.to_string()))
        .count();
    summary["frames"] = json!(frames.len());
    summary["frames_with_credential_shape"] = json!(flagged);
    let mut tf = std::fs::File::create(out.join("transcript.jsonl")).expect("transcript");
    for fr in &frames {
        writeln!(tf, "{fr}").expect("write");
    }
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary).expect("json"),
    )
    .expect("summary");
    println!("{}", serde_json::to_string_pretty(&summary).expect("json"));
    assert_eq!(flagged, 0, "no frame carries a credential shape");
}
