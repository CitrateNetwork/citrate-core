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
        mdns: false,
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
    // HUP-S8.4: LAN discovery stays off unless asked for.
    assert!(!env.contains_key(ENV_MDNS), "mDNS is off by default");
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
    let name = format!("ctzcl-{tag}-{n}.sock");
    if cfg!(windows) {
        std::env::temp_dir().join(name)
    } else {
        PathBuf::from("/tmp").join(name)
    }
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

// HUP-S8.1 follow-on: the manager slot builds once, and can be emptied so the next call rebuilds
// under a new mesh identity (no app restart).
#[test]
fn slot_builds_once_and_rebuilds_only_after_take_if() {
    let slot: Slot<String> = Slot::new();
    let mut builds = 0;
    let a = slot
        .get_or_try_insert(|| {
            builds += 1;
            Ok("comms-identity".to_string())
        })
        .expect("init");
    let b = slot
        .get_or_try_insert(|| {
            builds += 1;
            Ok("never built".to_string())
        })
        .expect("existing");
    assert_eq!(builds, 1);
    assert!(Arc::ptr_eq(&a, &b));

    // Same identity: kept.
    assert!(slot.take_if(|m| m != "comms-identity").is_none());
    assert!(slot.get().is_some());
    // Identity changed: taken out, and the next call builds the new one.
    let old = slot.take_if(|m| m != "device-identity").expect("taken");
    assert_eq!(*old, "comms-identity");
    assert!(slot.get().is_none());
    let c = slot
        .get_or_try_insert(|| Ok("device-identity".to_string()))
        .expect("rebuilt");
    assert_eq!(*c, "device-identity");
}

#[test]
fn a_failed_build_leaves_the_slot_empty() {
    let slot: Slot<String> = Slot::new();
    assert!(slot
        .get_or_try_insert(|| Err("binary not bundled".to_string()))
        .is_err());
    assert!(slot.get().is_none());
    assert!(slot.get_or_try_insert(|| Ok("ok".to_string())).is_ok());
}

// Review fix (fan-out 6): restarting a stopped manager happens under the slot's lock and only for the
// value still in the slot. Before, `ensure_started` read the manager, released the lock, and then
// restarted it if it was not running; a mesh-identity reload in between took that manager out and
// stopped it, and the restart brought the old daemon back while the next call built a second one
// (two daemons, the bearer file overwritten, "unauthorized"). The fake below records a start of a
// value that was already taken out.
#[test]
fn a_manager_taken_out_of_the_slot_is_never_restarted() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Fake {
        running: AtomicBool,
        taken: AtomicBool,
    }
    let slot: Arc<Slot<Fake>> = Arc::new(Slot::new());
    let revived = Arc::new(AtomicUsize::new(0));
    let fresh = || {
        Ok(Fake {
            running: AtomicBool::new(true),
            taken: AtomicBool::new(false),
        })
    };
    let mut users = Vec::new();
    for _ in 0..4 {
        let (slot, revived) = (slot.clone(), revived.clone());
        users.push(std::thread::spawn(move || {
            for _ in 0..500 {
                let _ = slot.get_or_try_insert_ensured(fresh, |m| {
                    // `is_running` asks the OS about the child process: it takes time.
                    std::thread::sleep(std::time::Duration::from_micros(50));
                    if !m.running.load(Ordering::SeqCst) {
                        if m.taken.load(Ordering::SeqCst) {
                            revived.fetch_add(1, Ordering::SeqCst);
                        }
                        std::thread::yield_now();
                        m.running.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                });
            }
        }));
    }
    let done = Arc::new(AtomicBool::new(false));
    let reloader = {
        let (slot, done) = (slot.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                if let Some(old) = slot.take_if(|_| true) {
                    old.taken.store(true, Ordering::SeqCst);
                    old.running.store(false, Ordering::SeqCst);
                }
                // A daemon that died on its own (not taken) is restarted in place.
                if let Some(cur) = slot.get() {
                    cur.running.store(false, Ordering::SeqCst);
                }
                std::thread::yield_now();
            }
        })
    };
    for u in users {
        u.join().expect("user thread");
    }
    done.store(true, Ordering::SeqCst);
    reloader.join().expect("reloader thread");
    assert_eq!(revived.load(Ordering::SeqCst), 0, "a taken-out manager was restarted");
}

// ---- HUP-S8.4: group seeds (link/QR) and opt-in LAN discovery ----

#[test]
fn mdns_reaches_the_daemon_only_when_the_operator_asks() {
    let (mgr, _dir) = stub_manager("mdns");
    let mgr = mgr.with_libp2p(Libp2pOpts {
        listen: "/ip4/0.0.0.0/tcp/0".into(),
        bootstrap: None,
        seed_hex: Zeroizing::new("22".repeat(32)),
        mdns: true,
    });
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(ENV_MDNS).map(String::as_str), Some("1"));
    // No libp2p, no mDNS: in-process mode never forwards it.
    let (plain, _d) = stub_manager("mdns-off");
    assert!(!plain
        .spec_env_for_test()
        .iter()
        .any(|(k, _)| k == ENV_MDNS));
}

#[test]
fn the_operator_mdns_flag_is_on_only_for_explicit_yes() {
    for on in ["1", "true", " ON "] {
        assert!(mdns_requested(Some(on)), "{on}");
    }
    for off in ["", "0", "false", "off", "yes", "2"] {
        assert!(!mdns_requested(Some(off)), "{off}");
    }
    assert!(!mdns_requested(None));
}

#[test]
fn seed_text_is_trimmed_and_bounded_before_it_reaches_the_daemon() {
    assert_eq!(
        seed_text("  citrate-cluster://seed?v=1&g=g&a=/ip4/1.2.3.4/tcp/1/p2p/x \n"),
        Ok("citrate-cluster://seed?v=1&g=g&a=/ip4/1.2.3.4/tcp/1/p2p/x".to_string())
    );
    assert!(seed_text("   ").unwrap_err().contains("Paste"));
    assert!(seed_text(&"a".repeat(MAX_SEED_TEXT + 1))
        .unwrap_err()
        .contains("too long"));
}

#[test]
fn seed_requests_and_responses_match_the_daemon_wire() {
    assert_eq!(
        serde_json::to_string(&Request::Seed { group: "g".into() }).expect("json"),
        r#"{"op":"seed","group":"g"}"#
    );
    assert_eq!(
        serde_json::to_string(&Request::AddSeed {
            group: "g".into(),
            seed: "s".into()
        })
        .expect("json"),
        r#"{"op":"addSeed","group":"g","seed":"s"}"#
    );
    let r: Response = serde_json::from_str(
        r#"{"type":"seed","seed":"citrate-cluster://seed?v=1","addrs":["/ip4/1.2.3.4/tcp/1/p2p/x"]}"#,
    )
    .expect("parse");
    assert!(matches!(r, Response::Seed { ref addrs, .. } if addrs.len() == 1));
    let r: Response = serde_json::from_str(r#"{"type":"seeded","dialing":2}"#).expect("parse");
    assert!(matches!(r, Response::Seeded { dialing: 2 }));
    let dto = ClusterGroupSeedDto {
        group_id: "g".into(),
        link: "s".into(),
        addrs: vec!["a".into()],
    };
    let v = serde_json::to_value(dto).expect("json");
    assert_eq!(v["groupId"], "g", "the UI sees camelCase");
    assert_eq!(v["link"], "s", "the link text, named so it never reads as key material");
    assert!(v.get("seed").is_none());
}

#[test]
fn the_seed_commands_are_registered_and_allowed_for_the_main_window_only() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in ["cluster_group_seed", "cluster_add_seed"] {
        assert!(lib.contains(&format!("cluster::{cmd},")), "{cmd} in lib.rs");
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} in main-window.toml");
    }
}
