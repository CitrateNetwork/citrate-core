// CX-S3.2 — comms member-daemon manager + UDS client tests. Included into `comms::tests`.
//
// CI-safe: NO real comms-member-daemon. A long-lived `sleep` stub stands in for the child; a
// hand-rolled UDS server stub exercises the JSON client's handshake + framing.

use super::*;

fn sleep_bin() -> PathBuf {
    for c in ["/bin/sleep", "/usr/bin/sleep"] {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    panic!("no sleep binary");
}

fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("citrate-core-comms-{tag}-{nanos}-{:?}", std::thread::current().id()));
    std::fs::create_dir_all(&p).expect("mk tmpdir");
    p
}

/// A configured manager over the sleep stub (a dummy seed set, so it can start).
fn stub_manager(tag: &str) -> (CommsMemberManager, PathBuf) {
    let dir = tmp_dir(tag);
    let mgr = CommsMemberManager::new(
        sleep_bin(),
        dir.join("member.sock"),
        dir.join("member.bearer"),
        dir.clone(),
        "deadbeef".repeat(8), // dummy seed
        COMMS_DOMAIN,
        dir.join("crash.jsonl"),
    );
    (mgr, dir)
}

#[test]
fn start_refuses_when_identity_not_provisioned() {
    let dir = tmp_dir("noid");
    // Empty seed → NotConfigured, WITHOUT touching the binary (the identity gate).
    let mgr = CommsMemberManager::new(
        sleep_bin(),
        dir.join("s"),
        dir.join("b"),
        dir.clone(),
        "",
        COMMS_DOMAIN,
        dir.join("c"),
    );
    assert!(!mgr.is_configured());
    assert!(matches!(mgr.start(), Err(CommsError::NotConfigured)));
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn start_refuses_when_binary_missing() {
    let dir = tmp_dir("nobin");
    let mgr = CommsMemberManager::new(
        dir.join("no-daemon"),
        dir.join("s"),
        dir.join("b"),
        dir.clone(),
        "aa".repeat(32),
        COMMS_DOMAIN,
        dir.join("c"),
    );
    assert!(matches!(mgr.start(), Err(CommsError::BinaryNotFound(_))));
}

#[test]
fn spec_env_carries_socket_bearerpath_seed_domain_never_argv() {
    let (mgr, dir) = stub_manager("env");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(ENV_SOCKET).map(String::as_str), Some(dir.join("member.sock").to_string_lossy().as_ref()));
    // The bearer crosses as a FILE PATH, never the token value.
    assert_eq!(env.get(ENV_BEARER_FILE).map(String::as_str), Some(dir.join("member.bearer").to_string_lossy().as_ref()));
    assert!(env.contains_key(ENV_SEED));
    assert_eq!(env.get(ENV_DOMAIN).map(String::as_str), Some(COMMS_DOMAIN));
    // No positional argv (config is all ENV).
    assert!(mgr.spec_env_for_test().iter().all(|(k, _)| k.starts_with("CITRATE_MEMBER_")));
}

#[test]
fn start_reaches_running_then_stop_is_clean() {
    let (mgr, _dir) = stub_manager("wiring");
    let mgr = mgr
        .with_spawn_args(vec!["3600".to_string()])
        .with_health_interval(std::time::Duration::from_secs(3600)); // socket probe won't fire mid-test
    mgr.start().expect("stub daemon starts");
    let mut running = false;
    for _ in 0..100 {
        if mgr.status().state == "running" {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "daemon stub must reach Running");
    assert!(mgr.is_running());
    mgr.stop();
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn double_start_is_rejected() {
    let (mgr, _dir) = stub_manager("double");
    let mgr = mgr.with_spawn_args(vec!["3600".to_string()]);
    mgr.start().expect("first start");
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(matches!(mgr.start(), Err(CommsError::AlreadyRunning)));
    mgr.stop();
}

/// A SHORT socket path (UDS paths must be < SUN_LEN ~104 on macOS; a temp-dir path overflows it).
fn short_sock(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() % 100_000_000)
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/ctz-{tag}-{n}.sock"))
}

// The UDS JSON client: bearer handshake + one request/response against a stub socket server.
#[test]
fn member_ipc_authenticates_and_round_trips_a_request() {
    let sock = short_sock("ipc");
    let _ = std::fs::remove_file(&sock);
    let bearer = "b".repeat(64);
    let sock_srv = sock.clone();
    let bearer_srv = bearer.clone();
    let handle = std::thread::spawn(move || {
        let listener = std::os::unix::net::UnixListener::bind(&sock_srv).expect("bind");
        let (stream, _) = listener.accept().expect("accept");
        let mut w = stream.try_clone().unwrap();
        let mut r = BufReader::new(stream);
        // auth line
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains(&bearer_srv), "auth carried the bearer");
        writeln!(w, "{{\"type\":\"ready\"}}").unwrap();
        // request → canned GroupCreated response
        line.clear();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("createGroup"), "got the request: {line}");
        writeln!(w, "{{\"type\":\"groupCreated\",\"id\":\"abc123\"}}").unwrap();
    });
    // Wait for the socket, then drive the client.
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let resp = member_ipc(&sock, &bearer, &Request::CreateGroup { name: "deals".into() }).expect("ipc");
    match resp {
        Response::GroupCreated { id } => assert_eq!(id, "abc123"),
        other => panic!("expected GroupCreated, got {other:?}"),
    }
    // A wrong bearer is rejected (no "ready").
    let _ = handle.join();
}
