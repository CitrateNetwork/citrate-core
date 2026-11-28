//! L1-WP03a: the host's parts without a session: the lock and pin check, the agent process
//! (cleared environment, process group, bounded stderr, scanned and tapped stdio), the event log and
//! the permission answers.

use citrate_acp_host::events::{EventLog, EVENT_CAP};
use citrate_acp_host::permission::{
    outcome, DenyByDefault, PermissionAsk, PermissionPolicy, Verdict,
};
use citrate_acp_host::process::{
    self, Direction, ScannedWriter, SecretScan, StderrRing, TappedReader, WireTap, STDERR_CAP,
};
use citrate_acp_host::registry::{locked_launch, parse_sha256, sha256_file, verify_pin, AgentSpec};
use futures::io::{AsyncReadExt as _, AsyncWriteExt as _};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("acp-parts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp");
    d
}

fn scan() -> SecretScan {
    Arc::new(|s: &str| s.contains("sk-citrate-test-"))
}

const LOCK: &str = r#"{"version":1,"agents":[
 {"id":"codex","version":"2.12.1","launch":{"cmd":"codex-acp","args":[],"env":{}}},
 {"id":"muse","version":"1.4.24","launch":{"cmd":"muse","args":["acp"],"env":{"MUSE_EXPERIMENTAL_ACP":"on"}}},
 {"id":"broken","version":"1","launch":{"args":[]}}]}"#;

#[test]
fn lock_row_gives_the_launch_command_args_and_env() {
    let m = locked_launch(LOCK, "muse").expect("muse");
    assert_eq!((m.cmd.as_str(), m.version.as_str()), ("muse", "1.4.24"));
    assert_eq!(m.args, vec!["acp"]);
    assert_eq!(
        m.env,
        vec![("MUSE_EXPERIMENTAL_ACP".to_string(), "on".to_string())]
    );
    assert!(locked_launch(LOCK, "codex").expect("codex").args.is_empty());
}

#[test]
fn unknown_or_incomplete_lock_rows_are_refused() {
    assert!(locked_launch(LOCK, "nope")
        .unwrap_err()
        .contains("not in agents.lock.json"));
    assert!(locked_launch(LOCK, "broken")
        .unwrap_err()
        .contains("launch.cmd"));
    assert!(locked_launch("{", "codex").is_err());
}

#[test]
fn pin_check_passes_only_on_the_exact_hash() {
    let d = tmp("pin");
    let f = d.join("agent");
    std::fs::write(&f, b"#!/bin/sh\n").expect("write");
    let good = sha256_file(&f).expect("hash");
    let mut spec = AgentSpec {
        id: "t".into(),
        command: f.clone(),
        args: vec![],
        pinned_file: f.clone(),
        sha256: good,
    };
    verify_pin(&spec).expect("matches");
    std::fs::write(&f, b"#!/bin/sh\necho changed\n").expect("write");
    let e = verify_pin(&spec).unwrap_err();
    assert!(
        e.contains("does not match") && e.contains(&hex::encode(good)),
        "{e}"
    );
    spec.pinned_file = d.join("missing");
    assert!(verify_pin(&spec).unwrap_err().contains("cannot read"));
    assert_eq!(parse_sha256(&hex::encode(good)).expect("hex"), good);
    assert!(parse_sha256("abcd").is_err() && parse_sha256(&"zz".repeat(32)).is_err());
}

#[test]
fn launch_with_a_credential_in_env_or_argv_is_refused() {
    let s = scan();
    assert!(process::check_launch(&[("HOME".into(), "/h".into())], &[], &s).is_ok());
    let e =
        process::check_launch(&[("X".into(), "sk-citrate-test-1".into())], &[], &s).unwrap_err();
    assert!(
        e.contains("X") && !e.contains("sk-citrate"),
        "names the variable, never the value: {e}"
    );
    assert!(process::check_launch(&[], &["--k=sk-citrate-test-1".into()], &s).is_err());
}

#[cfg(unix)]
#[test]
fn spawned_child_gets_only_the_given_env_and_leads_its_own_group() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let d = tmp("spawn");
    let out = d.join("out");
    std::env::set_var("CITRATE_PARENT_ONLY", "1");
    let script = format!("env > {0}; ps -o pgid= -p $$ >> {0}", out.display());
    let env = vec![
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        ("LANG".into(), "C".into()),
    ];
    let mut child = rt.block_on(async {
        process::spawn(
            std::path::Path::new("/bin/sh"),
            &["-c".into(), script],
            &env,
            &d,
        )
        .expect("spawn")
    });
    let pid = child.id().expect("pid");
    rt.block_on(child.wait()).expect("wait");
    let text = std::fs::read_to_string(&out).expect("out");
    let mut lines: Vec<&str> = text.lines().collect();
    let pgid: u32 = lines.pop().expect("pgid").trim().parse().expect("num");
    assert_eq!(pgid, pid, "leader of its own process group");
    let names: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.split('=').next())
        .filter(|n| *n != "PWD" && *n != "SHLVL" && *n != "_")
        .collect();
    assert_eq!(names, vec!["PATH", "LANG"]);
}

#[cfg(unix)]
#[test]
fn group_kill_reaches_grandchildren() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let d = tmp("group");
    let pidf = d.join("gc");
    let script = format!("sleep 300 & echo $! > {}; wait", pidf.display());
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let mut child = rt.block_on(async {
        process::spawn(
            std::path::Path::new("/bin/sh"),
            &["-c".into(), script],
            &env,
            &d,
        )
        .expect("spawn")
    });
    let end = Instant::now() + Duration::from_secs(5);
    while std::fs::read_to_string(&pidf)
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
        && Instant::now() < end
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let gc: i32 = std::fs::read_to_string(&pidf)
        .expect("pid")
        .trim()
        .parse()
        .expect("pid");
    process::signal_group(child.id().expect("pid"), true);
    rt.block_on(child.wait()).expect("wait");
    let end = Instant::now() + Duration::from_secs(5);
    let gone = |p: i32| unsafe { libc::kill(p, 0) } != 0 || std::fs::read_to_string(format!("/proc/{p}/stat")).map(|s| s.contains(") Z")).unwrap_or(true);
    while !gone(gc) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(gone(gc), "grandchild {gc} killed with the group");
}

#[test]
fn stderr_ring_keeps_only_the_newest_bytes() {
    let mut r = StderrRing::default();
    r.push(&vec![b'a'; STDERR_CAP]);
    r.push(b"tail");
    let t = r.text();
    assert_eq!(t.len(), STDERR_CAP);
    assert!(t.ends_with("aaatail"));
    r.push(&vec![b'z'; STDERR_CAP * 3]);
    assert_eq!(r.text(), "z".repeat(STDERR_CAP));
}

#[test]
fn scanned_writer_forwards_clean_lines_and_refuses_a_flagged_one() {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s2 = seen.clone();
    let tap: WireTap = Arc::new(move |d, l: &str| {
        assert_eq!(d, Direction::ToAgent);
        s2.lock().expect("tap").push(l.to_string())
    });
    let mut w = ScannedWriter::new(Vec::<u8>::new(), scan(), Some(tap));
    futures::executor::block_on(async {
        w.write_all(b"{\"a\":1}\n{\"b\":").await.expect("clean");
        w.write_all(b"2}\n").await.expect("split line");
        let e = w
            .write_all(b"{\"k\":\"sk-citrate-test-9\"}\n")
            .await
            .expect_err("refused");
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        w.flush().await.expect("flush");
    });
    assert_eq!(*seen.lock().expect("tap"), vec!["{\"a\":1}", "{\"b\":2}"]);
}

#[test]
fn tapped_reader_passes_bytes_through_and_shows_lines() {
    let seen: Arc<Mutex<Vec<(Direction, String)>>> = Arc::default();
    let s2 = seen.clone();
    let tap: WireTap = Arc::new(move |d, l: &str| s2.lock().expect("tap").push((d, l.to_string())));
    let src = futures::io::Cursor::new(b"one\ntwo\nthr".to_vec());
    let mut r = TappedReader::new(src, Some(tap));
    let mut all = String::new();
    futures::executor::block_on(r.read_to_string(&mut all)).expect("read");
    assert_eq!(all, "one\ntwo\nthr");
    let got = seen.lock().expect("tap").clone();
    assert_eq!(
        got,
        vec![
            (Direction::FromAgent, "one".into()),
            (Direction::FromAgent, "two".into())
        ]
    );
}

#[test]
fn event_log_is_sequenced_bounded_and_maps_updates() {
    let log = EventLog::default();
    assert_eq!(log.push_update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi"}})), 1);
    log.push_update(
        json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "q"}}),
    );
    log.push_update(json!({"sessionUpdate": "tool_call", "toolCallId": "t"}));
    log.push_update(json!({"sessionUpdate": "available_commands_update", "availableCommands": []}));
    log.push_update(json!({"sessionUpdate": "something_new"}));
    let (evs, last) = log.after(0, Duration::ZERO);
    let kinds: Vec<&str> = evs.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "message_chunk",
            "message_chunk",
            "tool_call",
            "commands",
            "update"
        ]
    );
    assert_eq!(
        (evs[0].body["role"].as_str(), evs[1].body["role"].as_str()),
        (Some("agent"), Some("user"))
    );
    assert_eq!(last, 5);
    for _ in 0..EVENT_CAP + 10 {
        log.push("x", json!(null));
    }
    let (evs, last) = log.after(0, Duration::ZERO);
    assert_eq!(evs.len(), EVENT_CAP);
    assert_eq!(
        evs[0].seq,
        last - EVENT_CAP as u64 + 1,
        "the oldest fall off; seq never repeats"
    );
}

#[test]
fn event_long_poll_times_out_empty() {
    let log = EventLog::default();
    let t = Instant::now();
    let (evs, last) = log.after(0, Duration::from_millis(150));
    assert!(evs.is_empty() && last == 0 && t.elapsed() >= Duration::from_millis(140));
}

fn ask(kind: &str, opts: &[(&str, &str)]) -> PermissionAsk {
    PermissionAsk::from_request(&json!({
        "sessionId": "s", "toolCall": {"toolCallId": "t1", "kind": kind, "title": "x"},
        "options": opts.iter().map(|(id, k)| json!({"optionId": id, "name": id, "kind": k})).collect::<Vec<_>>()
    }))
}

#[test]
fn default_policy_denies_every_kind() {
    for kind in [
        "edit", "delete", "move", "execute", "fetch", "read", "search", "think", "other", "unknown",
    ] {
        assert!(
            matches!(DenyByDefault.decide(&ask(kind, &[])), Verdict::Deny(_)),
            "{kind}"
        );
    }
}

#[test]
fn outcome_never_selects_allow_always_and_prefers_reject_once() {
    let all = ask(
        "edit",
        &[
            ("aa", "allow_always"),
            ("ao", "allow_once"),
            ("rj", "reject_once"),
            ("ra", "reject_always"),
        ],
    );
    assert_eq!(
        outcome(&Verdict::Deny("d".into()), &all.options),
        json!({"outcome": "selected", "optionId": "rj"})
    );
    assert_eq!(
        outcome(&Verdict::Allow("a".into()), &all.options),
        json!({"outcome": "selected", "optionId": "ao"})
    );
    let only_always = ask("edit", &[("aa", "allow_always"), ("ra", "reject_always")]);
    assert_eq!(
        outcome(&Verdict::Allow("a".into()), &only_always.options),
        json!({"outcome": "cancelled"})
    );
    assert_eq!(
        outcome(&Verdict::Deny("d".into()), &only_always.options),
        json!({"outcome": "selected", "optionId": "ra"})
    );
    assert_eq!(
        outcome(&Verdict::Deny("d".into()), &[]),
        json!({"outcome": "cancelled"})
    );
    assert_eq!(all.tool_kind.as_deref(), Some("edit"));
    assert_eq!(all.tool_call_id, "t1");
}
