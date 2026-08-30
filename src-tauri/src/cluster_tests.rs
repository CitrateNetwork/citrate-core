// CX-S4 / CL-S2 — cluster-daemon manager + UDS client tests. Included into `cluster::tests`.
//
// CI-safe: NO real cluster-daemon. A long-lived `sleep` stub stands in for the child; a hand-rolled
// UDS server stub exercises the JSON client's handshake + framing. The admission LOGIC that used to
// be tested here moved to its canonical home, the cluster-core crate (citrate-cluster) — see the
// ADR; net federation coverage rose (cluster-core has 16 admission tests + the daemon's own).

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
    p.push(format!("citrate-core-cluster-{tag}-{nanos}-{:?}", std::thread::current().id()));
    std::fs::create_dir_all(&p).expect("mk tmpdir");
    p
}

const SELF_ADDR: &str = "00000000000000000000000000000000000000aa";

fn stub_manager(tag: &str) -> (ClusterDaemonManager, PathBuf) {
    let dir = tmp_dir(tag);
    let mgr = ClusterDaemonManager::new(
        sleep_bin(),
        dir.join("cluster.sock"),
        dir.join("cluster.bearer"),
        dir.clone(),
        SELF_ADDR,
        dir.join("crash.jsonl"),
    );
    (mgr, dir)
}

#[test]
fn start_refuses_when_no_member_identity() {
    let dir = tmp_dir("noid");
    let mgr = ClusterDaemonManager::new(
        sleep_bin(),
        dir.join("s"),
        dir.join("b"),
        dir.clone(),
        "",
        dir.join("c"),
    );
    assert!(!mgr.is_configured());
    assert!(matches!(mgr.start(), Err(ClusterError::NotConfigured)));
}

#[test]
fn start_refuses_when_binary_missing() {
    let dir = tmp_dir("nobin");
    let mgr = ClusterDaemonManager::new(
        dir.join("no-daemon"),
        dir.join("s"),
        dir.join("b"),
        dir.clone(),
        SELF_ADDR,
        dir.join("c"),
    );
    assert!(matches!(mgr.start(), Err(ClusterError::BinaryNotFound(_))));
}

#[test]
fn spec_env_carries_socket_bearerpath_selfaddr_never_inline_token() {
    let (mgr, dir) = stub_manager("env");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(ENV_SOCKET).map(String::as_str), Some(dir.join("cluster.sock").to_string_lossy().as_ref()));
    // The bearer crosses as a FILE PATH, never the token value.
    assert_eq!(env.get(ENV_BEARER_FILE).map(String::as_str), Some(dir.join("cluster.bearer").to_string_lossy().as_ref()));
    assert_eq!(env.get(ENV_SELF_ADDR).map(String::as_str), Some(SELF_ADDR));
    assert!(mgr.spec_env_for_test().iter().all(|(k, _)| k.starts_with("CITRATE_CLUSTER_")));
}

#[test]
fn start_reaches_running_then_stop_is_clean() {
    let (mgr, _dir) = stub_manager("wiring");
    let mgr = mgr
        .with_spawn_args(vec!["3600".to_string()])
        .with_health_interval(std::time::Duration::from_secs(3600));
    mgr.start().expect("stub daemon starts");
    let mut running = false;
    for _ in 0..100 {
        if mgr.is_running() {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "daemon stub must reach Running");
    mgr.stop();
    assert!(!mgr.is_running());
}

#[test]
fn double_start_is_rejected() {
    let (mgr, _dir) = stub_manager("double");
    let mgr = mgr.with_spawn_args(vec!["3600".to_string()]);
    mgr.start().expect("first start");
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(matches!(mgr.start(), Err(ClusterError::AlreadyRunning)));
    mgr.stop();
}

#[test]
fn libp2p_env_absent_by_default() {
    // Default = in-process transport: none of the libp2p knobs are set.
    let (mgr, _dir) = stub_manager("nolibp2p");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert!(!env.contains_key(ENV_LISTEN));
    assert!(!env.contains_key(ENV_SEED_FILE));
    assert!(!env.contains_key(ENV_BOOTSTRAP));
}

#[test]
fn with_libp2p_adds_listen_seedpath_bootstrap_never_inline_seed() {
    let (mgr, dir) = stub_manager("libp2p");
    let seed = "11".repeat(32); // 32-byte hex secp256k1 secret (test value)
    let mgr = mgr.with_libp2p(Libp2pOpts {
        listen: "/ip4/0.0.0.0/tcp/0".into(),
        bootstrap: Some("/ip4/10.0.0.2/tcp/4001/p2p/12D3KooWxyz".into()),
        seed_hex: Zeroizing::new(seed.clone()),
    });
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(ENV_LISTEN).map(String::as_str), Some("/ip4/0.0.0.0/tcp/0"));
    // The seed crosses as a FILE PATH, never the secret value.
    assert_eq!(
        env.get(ENV_SEED_FILE).map(String::as_str),
        Some(dir.join("cluster.seed").to_string_lossy().as_ref())
    );
    assert_eq!(env.get(ENV_BOOTSTRAP).map(String::as_str), Some("/ip4/10.0.0.2/tcp/4001/p2p/12D3KooWxyz"));
    // The raw seed hex must NEVER appear in any env value (Rule 4 / CLAUDE.md: secrets via file path).
    assert!(
        mgr.spec_env_for_test().iter().all(|(_, v)| !v.contains(&seed)),
        "seed value must never cross env"
    );
    assert!(mgr.spec_env_for_test().iter().all(|(k, _)| k.starts_with("CITRATE_CLUSTER_")));
}

/// A SHORT socket path (UDS paths must be < SUN_LEN ~104 on macOS).
fn short_sock(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() % 100_000_000)
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/ctzcl-{tag}-{n}.sock"))
}

#[test]
fn cluster_ipc_authenticates_and_parses_status_defaulting_sharedfiles() {
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
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains(&bearer_srv), "auth carried the bearer");
        writeln!(w, "{{\"type\":\"ready\"}}").unwrap();
        line.clear();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("status"), "got the request: {line}");
        // A daemon that predates the co-pin field: no sharedFiles → client defaults to [].
        writeln!(w, "{{\"type\":\"status\",\"online\":2,\"total\":3}}").unwrap();
    });
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let resp = cluster_ipc(&sock, &bearer, &Request::Status { group: "g1".into() }).expect("ipc");
    match resp {
        Response::Status { online, total, shared_files } => {
            assert_eq!((online, total), (2, 3));
            assert!(shared_files.is_empty(), "sharedFiles defaults to [] when the daemon omits it");
        }
        other => panic!("expected Status, got {other:?}"),
    }
    let _ = handle.join();
}
