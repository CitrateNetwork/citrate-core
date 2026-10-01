// CX-S4 / CL-S2 — cluster-daemon manager + UDS client tests. Included into `cluster::tests`.
//
// CI-safe: NO real cluster-daemon. A long-lived `sleep` stub stands in for the child; a hand-rolled
// UDS server stub exercises the JSON client's handshake + framing. The admission LOGIC that used to
// be tested here moved to its canonical home, the cluster-core crate (citrate-cluster) — see the
// ADR; net federation coverage rose (cluster-core has 16 admission tests + the daemon's own).

use super::*;

// Issue #46 — the in-process stub daemon listens on the cross-platform interprocess
// local socket (a `UnixListener` on unix, a named pipe on Windows), matching the
// client's transport. `endpoint_name` gives both ends the same name from the path.
use crate::ipc_name::endpoint_name;
use interprocess::local_socket::{prelude::*, ListenerOptions};

/// A long-lived stub child binary that exists on the host (issue #47). Unix:
/// `/bin/sleep`. Windows: `ping.exe` (always present; kept alive by
/// [`long_lived_args`]). The Windows path is verified by the team.
fn sleep_bin() -> PathBuf {
    #[cfg(unix)]
    {
        for c in ["/bin/sleep", "/usr/bin/sleep"] {
            let p = PathBuf::from(c);
            if p.exists() {
                return p;
            }
        }
        panic!("no sleep binary");
    }
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        PathBuf::from(root).join("System32").join("ping.exe")
    }
}

/// The argv that keeps [`sleep_bin`] alive for a lifecycle test. Unix: `sleep 3600`.
/// Windows: `ping 127.0.0.1 -n 999`. Paired with [`sleep_bin`].
fn long_lived_args() -> Vec<String> {
    #[cfg(unix)]
    {
        vec!["3600".to_string()]
    }
    #[cfg(windows)]
    {
        vec!["127.0.0.1".to_string(), "-n".to_string(), "999".to_string()]
    }
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
        .with_spawn_args(long_lived_args())
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
    let mgr = mgr.with_spawn_args(long_lived_args());
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
        let name = endpoint_name(&sock_srv.to_string_lossy()).expect("endpoint name");
        let listener = ListenerOptions::new().name(name).create_sync().expect("bind");
        let stream = listener.accept().expect("accept");
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

#[test]
fn shutdown_is_a_safe_noop_when_never_started() {
    // MANAGER singleton uninitialized in tests → graceful-teardown shutdown() is a clean no-op.
    super::shutdown();
}

// ---- HUP-S8.1: device links on the cluster IPC ----

#[test]
fn set_roster_without_links_serializes_exactly_as_before() {
    // An older daemon must see the request it always did: no `devices`/`revocations` keys.
    let req = Request::SetRoster {
        group: "g".into(),
        roster: vec![("aa".into(), "member".into())],
        devices: vec![],
        revocations: vec![],
    };
    assert_eq!(
        serde_json::to_string(&req).expect("json"),
        r#"{"op":"setRoster","group":"g","roster":[["aa","member"]]}"#
    );
}

#[test]
fn set_roster_carries_links_in_the_daemon_wire_shape() {
    let link = crate::device_link::DeviceLinkWire {
        member: "a1".into(),
        device: "d1".into(),
        wallet: "b1".into(),
        index: 0,
        label: "Studio Mac".into(),
        issued_at: 7,
        member_sig: "0xm".into(),
        device_sig: "0xd".into(),
        wallet_sig: "0xw".into(),
    };
    let rev = crate::device_link::RevocationWire {
        member: "a1".into(),
        device: "d2".into(),
        revoked_at: 9,
        member_sig: "0xr".into(),
    };
    let v: serde_json::Value = serde_json::to_value(Request::SetRoster {
        group: "g".into(),
        roster: vec![],
        devices: vec![link],
        revocations: vec![rev],
    })
    .expect("json");
    assert_eq!(v["devices"][0]["issuedAt"], 7);
    assert_eq!(v["devices"][0]["memberSig"], "0xm");
    assert_eq!(v["devices"][0]["walletSig"], "0xw");
    assert_eq!(v["revocations"][0]["revokedAt"], 9);
    let v: serde_json::Value =
        serde_json::to_value(Request::Devices { group: "g".into() }).expect("json");
    assert_eq!(v["op"], "devices");
}

#[test]
fn responses_parse_with_and_without_the_device_fields() {
    // Older daemon: no `rejected`, no `member`.
    let r: Response = serde_json::from_str(r#"{"type":"reconciled","evicted":[]}"#).expect("parse");
    assert!(matches!(r, Response::Reconciled { ref rejected, .. } if rejected.is_empty()));
    let r: Response =
        serde_json::from_str(r#"{"type":"peers","peers":[{"address":"aa","online":true}]}"#)
            .expect("parse");
    assert!(matches!(r, Response::Peers { ref peers } if peers[0].member.is_none()));
    // HUP-S8.1 daemon.
    let r: Response = serde_json::from_str(
        r#"{"type":"reconciled","evicted":["d2"],"rejected":["d2: revoked"]}"#,
    )
    .expect("parse");
    assert!(matches!(r, Response::Reconciled { ref rejected, .. } if rejected.len() == 1));
    let r: Response = serde_json::from_str(
        r#"{"type":"peers","peers":[{"address":"d1","online":true,"member":"a1"}]}"#,
    )
    .expect("parse");
    assert!(matches!(r, Response::Peers { ref peers } if peers[0].member.as_deref() == Some("a1")));
    let r: Response = serde_json::from_str(
        r#"{"type":"devices","members":[{"member":"a1","role":"member","online":false,
            "devices":[{"device":"d1","index":0,"label":"Studio Mac","issuedAt":7,"online":true}]}]}"#,
    )
    .expect("parse");
    match r {
        Response::Devices { members } => {
            assert_eq!(members[0].devices[0].label, "Studio Mac");
            let out = serde_json::to_value(&members[0]).expect("json");
            assert_eq!(out["devices"][0]["issuedAt"], 7, "the UI sees camelCase");
        }
        other => panic!("expected Devices, got {other:?}"),
    }
}

#[test]
fn peer_dto_omits_member_for_a_member_identity() {
    let dto = ClusterPeerDto {
        address: "aa".into(),
        online: true,
        member: None,
    };
    assert_eq!(
        serde_json::to_string(&dto).expect("json"),
        r#"{"address":"aa","online":true}"#
    );
}
