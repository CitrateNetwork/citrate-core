//! L1-WP03 session manager tests. The peer is `acp-test-agent` (tests/support/test_agent.rs): a
//! real ACP agent built from the SDK's `Agent` role, run as a real child process, so every test
//! crosses real stdio and a real JSON-RPC connection.

use citrate_acp_host::permission::DenyByDefault;
use citrate_acp_host::process::{Direction, WireTap};
use citrate_acp_host::registry::{sha256_file, AgentSpec};
use citrate_acp_host::{AcpHost, HostConfig, Session, SessionStart};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const AGENT: &str = env!("CARGO_BIN_EXE_acp-test-agent");
/// The marker every test scanner treats as a credential (the host takes the scanner from Core).
const FAKE_KEY: &str = "sk-citrate-test-0123456789abcdef";

fn scan(s: &str) -> bool {
    s.contains("sk-citrate-test-")
}

fn spec() -> AgentSpec {
    AgentSpec {
        id: "test".into(),
        command: PathBuf::from(AGENT),
        args: vec![],
        pinned_file: PathBuf::from(AGENT),
        sha256: sha256_file(Path::new(AGENT)).expect("hash test agent"),
    }
}

type Frames = Arc<Mutex<Vec<(Direction, String)>>>;

fn host_with(cancel_grace: Duration) -> (AcpHost, Frames) {
    let frames: Frames = Arc::default();
    let f = frames.clone();
    let tap: WireTap =
        Arc::new(move |d, line: &str| f.lock().expect("frames").push((d, line.to_string())));
    let host = AcpHost::new(HostConfig {
        client_name: "citrate-core-test".into(),
        client_version: "0.0.0".into(),
        scan: Arc::new(scan),
        policy: Arc::new(DenyByDefault),
        tap: Some(tap),
        init_timeout: Duration::from_secs(20),
        cancel_grace,
    })
    .expect("host");
    (host, frames)
}

fn host() -> (AcpHost, Frames) {
    host_with(Duration::from_secs(3))
}

fn start_in(host: &AcpHost, cwd: &Path, env: Vec<(String, String)>, mcp: Vec<Value>) -> Session {
    host.start(SessionStart {
        spec: spec(),
        env,
        cwd: cwd.to_path_buf(),
        mcp_servers: mcp,
    })
    .unwrap_or_else(|e| panic!("start: {}", e.reason))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("acp-host-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp");
    d
}

fn text(t: &str) -> Vec<Value> {
    vec![json!({"type": "text", "text": t})]
}

fn texts(s: &Session) -> Vec<String> {
    s.events(0, Duration::ZERO)
        .0
        .into_iter()
        .filter(|e| e.kind == "message_chunk")
        .filter_map(|e| {
            e.body
                .pointer("/content/text")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .collect()
}

fn report(s: &Session) -> Value {
    assert_eq!(s.prompt(text("report")).expect("report"), "end_turn");
    let last = texts(s).pop().expect("a report chunk");
    serde_json::from_str(&last).expect("report json")
}

fn alive(pid: u32) -> bool {
    // kill(pid, 0) succeeds for a live (or zombie) process; reaped processes are gone.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 && !is_zombie(pid)
}

fn is_zombie(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| {
            s.rsplit(')')
                .next()
                .is_some_and(|r| r.trim_start().starts_with('Z'))
        })
        .unwrap_or(false)
}

fn wait_dead(pid: u32, within: Duration) -> bool {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !alive(pid)
}

#[test]
fn initialize_advertises_exactly_the_served_client_capabilities() {
    // fs/* and terminal/* are served by L1-WP05 (sandboxed); until then Core advertises false, the
    // lane file's WP05 rollback ("advertise fs: false and terminal: false"), so an agent uses its
    // own tools instead of calling methods Core would refuse.
    let (h, _) = host();
    let s = start_in(&h, &tmp("caps"), vec![], vec![]);
    let caps = &report(&s)["clientCapabilities"];
    assert_eq!(caps["fs"]["readTextFile"], false);
    assert_eq!(caps["fs"]["writeTextFile"], false);
    assert_eq!(caps["terminal"], false);
    assert_eq!(s.acp_session_id(), "test-s1");
}

#[test]
fn session_new_cwd_is_the_given_worktree() {
    let (h, _) = host();
    let wt = tmp("cwd");
    let s = start_in(&h, &wt, vec![], vec![]);
    assert_eq!(report(&s)["cwd"], wt.display().to_string());
}

#[test]
fn session_new_injects_only_the_given_mcp_servers() {
    let (h, _) = host();
    let gateway = json!({"type": "http", "name": "citrate", "url": "http://127.0.0.1:47300/mcp", "headers": []});
    let s = start_in(&h, &tmp("mcp"), vec![], vec![gateway.clone()]);
    let got = report(&s)["mcpServers"].clone();
    assert_eq!(got.as_array().map(Vec::len), Some(1));
    assert_eq!(got[0]["name"], "citrate");
    let s2 = start_in(&h, &tmp("mcp0"), vec![], vec![]);
    assert_eq!(report(&s2)["mcpServers"], json!([]));
}

#[test]
fn updates_are_sequenced_and_lossless() {
    let (h, _) = host();
    let s = start_in(&h, &tmp("seq"), vec![], vec![]);
    for n in [1usize, 17, 300] {
        let (_, before) = s.events(0, Duration::ZERO);
        assert_eq!(
            s.prompt(text(&format!("updates {n}"))).expect("prompt"),
            "end_turn"
        );
        let (evs, last) = s.events(before, Duration::ZERO);
        let seqs: Vec<u64> = evs.iter().map(|e| e.seq).collect();
        assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "contiguous seq");
        assert_eq!(*seqs.last().expect("events"), last);
        let chunks: Vec<String> = evs
            .iter()
            .filter(|e| e.kind == "message_chunk")
            .map(|e| {
                e.body["content"]["text"]
                    .as_str()
                    .expect("text")
                    .to_string()
            })
            .collect();
        assert_eq!(chunks, (0..n).map(|i| i.to_string()).collect::<Vec<_>>());
        let stop = evs.last().expect("stop");
        assert_eq!(
            (stop.kind.as_str(), &stop.body["stopReason"]),
            ("stopped", &json!("end_turn"))
        );
    }
}

#[test]
fn events_long_poll_waits_for_the_next_event() {
    let (h, _) = host();
    let s = Arc::new(start_in(&h, &tmp("poll"), vec![], vec![]));
    let (_, last) = s.events(0, Duration::ZERO);
    let s2 = s.clone();
    let t = std::thread::spawn(move || s2.prompt(text("updates 1")));
    let (evs, _) = s.events(last, Duration::from_secs(10));
    assert!(!evs.is_empty() && evs[0].seq == last + 1);
    t.join().expect("join").expect("prompt");
}

#[test]
fn pin_mismatch_refuses_spawn() {
    let (h, _) = host();
    let mut sp = spec();
    sp.sha256[0] ^= 0xff;
    let marker = tmp("pin").join("spawned");
    let err = h
        .start(SessionStart {
            spec: sp,
            env: vec![],
            cwd: tmp("pin2"),
            mcp_servers: vec![],
        })
        .err()
        .expect("refused");
    assert!(err.reason.contains("sha256"), "{}", err.reason);
    assert!(err.reason.contains("does not match"), "{}", err.reason);
    assert!(!marker.exists());
}

#[test]
fn write_tool_permission_is_denied_by_default() {
    let (h, _) = host();
    let s = start_in(&h, &tmp("perm"), vec![], vec![]);
    for kind in ["edit", "delete", "execute", "read"] {
        assert_eq!(
            s.prompt(text(&format!("permission {kind}")))
                .expect("prompt"),
            "end_turn"
        );
        let outcome = texts(&s).pop().expect("outcome chunk");
        assert!(outcome.contains("\"reject\""), "{kind}: {outcome}");
        assert!(
            !outcome.contains("allow"),
            "{kind}: never an allow option: {outcome}"
        );
    }
    let perms: Vec<_> = s
        .events(0, Duration::ZERO)
        .0
        .into_iter()
        .filter(|e| e.kind == "permission")
        .collect();
    assert_eq!(perms.len(), 4);
    assert_eq!(perms[0].body["verdict"], "deny");
    assert_eq!(perms[0].body["toolKind"], "edit");
}

#[test]
fn cancel_ends_turn_cancelled() {
    let (h, _) = host();
    let s = Arc::new(start_in(&h, &tmp("cancel"), vec![], vec![]));
    let s2 = s.clone();
    let t = std::thread::spawn(move || s2.prompt(text("wait")));
    std::thread::sleep(Duration::from_millis(300));
    s.cancel().expect("cancel");
    assert_eq!(t.join().expect("join").expect("prompt"), "cancelled");
    assert!(
        alive(s.pid()),
        "a cooperative agent keeps running after cancel"
    );
    assert_eq!(s.prompt(text("updates 2")).expect("next turn"), "end_turn");
}

#[cfg(target_os = "linux")]
#[test]
fn cancel_kills_an_unresponsive_agent_and_its_process_group() {
    let (h, _) = host_with(Duration::from_millis(500));
    let dir = tmp("stubborn");
    let pidfile = dir.join("grandchild.pid");
    let s = Arc::new(start_in(
        &h,
        &dir,
        vec![("PATH".into(), "/usr/bin:/bin".into())],
        vec![],
    ));
    let s2 = s.clone();
    let pf = pidfile.display().to_string();
    let t = std::thread::spawn(move || s2.prompt(text(&format!("stubborn {pf}"))));
    let end = Instant::now() + Duration::from_secs(10);
    while !pidfile.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let grandchild: u32 = std::fs::read_to_string(&pidfile)
        .expect("pid")
        .trim()
        .parse()
        .expect("pid");
    assert!(alive(grandchild));
    s.cancel().expect("cancel");
    let started = Instant::now();
    assert_eq!(t.join().expect("join").expect("prompt"), "cancelled");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(wait_dead(s.pid(), Duration::from_secs(5)), "agent killed");
    assert!(
        wait_dead(grandchild, Duration::from_secs(5)),
        "grandchild in its group killed"
    );
    let evs = s.events(0, Duration::ZERO).0;
    assert!(evs
        .iter()
        .any(|e| e.kind == "stopped" && e.body["killed"] == true));
}

#[test]
fn agent_exit_mid_turn_reports_failed_not_answered() {
    let (h, _) = host();
    let s = start_in(&h, &tmp("exit"), vec![], vec![]);
    let err = s.prompt(text("exit")).expect_err("not answered");
    assert!(err.contains("exited"), "{err}");
    let evs = s.events(0, Duration::ZERO).0;
    assert!(evs.iter().any(|e| e.kind == "exited"));
    assert!(
        !evs.iter().any(|e| e.kind == "stopped"),
        "no stop reason for a turn that never ended"
    );
}

#[test]
fn stderr_is_bounded_and_never_in_events() {
    let (h, _) = host();
    let s = start_in(&h, &tmp("stderr"), vec![], vec![]);
    assert_eq!(s.prompt(text("stderr 200000")).expect("prompt"), "end_turn");
    std::thread::sleep(Duration::from_millis(200));
    let tail = s.stderr_tail();
    assert!(tail.len() <= 64 * 1024 + 64, "bounded: {}", tail.len());
    assert!(tail.ends_with("eeee"));
    let evs = serde_json::to_string(&s.events(0, Duration::ZERO).0).expect("json");
    assert!(!evs.contains("eeeeeeee"));
}

#[test]
fn spawned_env_is_exactly_the_given_env() {
    let (h, _) = host();
    let home = tmp("home");
    std::env::set_var("CITRATE_TEST_PARENT_SECRETLESS", "leak-check");
    let env = vec![
        ("HOME".to_string(), home.display().to_string()),
        ("LANG".into(), "C".into()),
    ];
    let s = start_in(&h, &tmp("env"), env, vec![]);
    let got = report(&s)["env"].clone();
    let names: Vec<&String> = got.as_object().expect("env").keys().collect();
    assert_eq!(names, vec!["HOME", "LANG"], "inherited environment cleared");
    assert_eq!(got["HOME"], home.display().to_string());
}

#[test]
fn credential_in_env_or_argv_refuses_spawn() {
    let (h, _) = host();
    let r = h.start(SessionStart {
        spec: spec(),
        env: vec![("OPENAI_BASE".into(), FAKE_KEY.into())],
        cwd: tmp("envkey"),
        mcp_servers: vec![],
    });
    assert!(r.err().expect("refused").reason.contains("credential"));
    let mut sp = spec();
    sp.args = vec![format!("--key={FAKE_KEY}")];
    let r = h.start(SessionStart {
        spec: sp,
        env: vec![],
        cwd: tmp("argvkey"),
        mcp_servers: vec![],
    });
    assert!(r.err().expect("refused").reason.contains("credential"));
}

#[test]
fn credential_never_crosses_an_acp_frame() {
    let (h, frames) = host();
    let s = start_in(&h, &tmp("frames"), vec![], vec![]);
    let err = s
        .prompt(text(&format!("updates 1 {FAKE_KEY}")))
        .expect_err("refused");
    assert!(err.contains("credential"), "{err}");
    assert!(s.authenticate(FAKE_KEY).is_err());
    assert_eq!(
        s.prompt(text("updates 1")).expect("session still usable"),
        "end_turn"
    );
    let all = frames.lock().expect("frames");
    assert!(all.iter().any(|(d, _)| *d == Direction::ToAgent));
    assert!(all.iter().any(|(d, _)| *d == Direction::FromAgent));
    assert!(
        all.iter().all(|(_, l)| !scan(l)),
        "no frame in either direction carries the marker"
    );
}

#[test]
fn close_kills_the_agent_and_reaps_it() {
    let (h, _) = host();
    let s = start_in(&h, &tmp("close"), vec![], vec![]);
    let pid = s.pid();
    assert!(alive(pid));
    s.close();
    assert!(wait_dead(pid, Duration::from_secs(5)));
}

#[test]
fn dropping_the_session_kills_the_agent() {
    let (h, _) = host();
    let s = start_in(&h, &tmp("drop"), vec![], vec![]);
    let pid = s.pid();
    drop(s);
    assert!(wait_dead(pid, Duration::from_secs(5)));
}

#[cfg(target_os = "linux")]
#[test]
fn cancel_returns_even_when_an_escaped_grandchild_holds_stdout() {
    // A grandchild that leaves the agent's process group survives the group kill (the L2 sandbox is
    // the boundary for that); the turn must still end once the agent itself is gone.
    let (h, _) = host_with(Duration::from_millis(300));
    let dir = tmp("escape");
    let pidfile = dir.join("grandchild.pid");
    let s = Arc::new(start_in(
        &h,
        &dir,
        vec![("PATH".into(), "/usr/bin:/bin".into())],
        vec![],
    ));
    let s2 = s.clone();
    let pf = pidfile.display().to_string();
    let t = std::thread::spawn(move || s2.prompt(text(&format!("escape {pf}"))));
    let end = Instant::now() + Duration::from_secs(10);
    while !pidfile.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let grandchild: i32 = std::fs::read_to_string(&pidfile)
        .expect("pid")
        .trim()
        .parse()
        .expect("pid");
    let started = Instant::now();
    s.cancel().expect("cancel");
    let r = t.join().expect("join");
    unsafe { libc::kill(grandchild, libc::SIGKILL) };
    assert_eq!(r.expect("prompt"), "cancelled");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(wait_dead(s.pid(), Duration::from_secs(5)));
    let stops = s
        .events(0, Duration::ZERO)
        .0
        .into_iter()
        .filter(|e| e.kind == "stopped")
        .count();
    assert_eq!(stops, 1, "one ending per turn");
}

#[test]
fn session_new_auth_required_keeps_the_agents_auth_methods() {
    let (h, _) = host();
    let err = h
        .start(SessionStart {
            spec: spec(),
            env: vec![("ACP_TEST_AGENT_AUTH".into(), "required".into())],
            cwd: tmp("auth"),
            mcp_servers: vec![],
        })
        .err()
        .expect("refused");
    assert!(err.auth_required, "{}", err.reason);
    assert!(
        err.reason.starts_with("session/new failed"),
        "{}",
        err.reason
    );
    let methods = &err.initialize.expect("initialize answered")["authMethods"];
    assert_eq!(methods[0]["id"], "agent-login");
}
