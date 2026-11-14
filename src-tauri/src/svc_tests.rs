//! V60 SVC-1: the link (paths, bearer file, attach or fall back) and the socket server's gate.

use super::*;
use std::path::PathBuf;

fn tmp_root(tag: &str) -> PathBuf {
    // Short: a Unix socket path must fit in sun_path (104 bytes on macOS).
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("csv-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("temp root");
    p
}

#[test]
fn headless_paths_follow_the_app_data_dir() {
    let p = SvcPaths::headless_from(None, Some(PathBuf::from("/home/m/.local/share"))).unwrap();
    assert_eq!(
        p.data_root,
        PathBuf::from("/home/m/.local/share/ai.citrate.core")
    );
    assert_eq!(
        p.socket,
        PathBuf::from("/home/m/.local/share/ai.citrate.core/svc/svc.sock")
    );
    assert_eq!(
        p.bearer,
        PathBuf::from("/home/m/.local/share/ai.citrate.core/svc/svc.bearer")
    );
    let o =
        SvcPaths::headless_from(Some(PathBuf::from("/tmp/x")), Some(PathBuf::from("/y"))).unwrap();
    assert_eq!(o.data_root, PathBuf::from("/tmp/x"));
    assert!(SvcPaths::headless_from(Some(PathBuf::from("rel")), None).is_err());
    assert!(SvcPaths::headless_from(None, None).is_err());
    // An empty override is no override.
    let e = SvcPaths::headless_from(Some(PathBuf::new()), Some(PathBuf::from("/d"))).unwrap();
    assert_eq!(e.data_root, PathBuf::from("/d/ai.citrate.core"));
}

#[test]
fn ct_eq_is_length_and_content_sensitive() {
    assert!(ct_eq(b"abc", b"abc"));
    assert!(!ct_eq(b"abc", b"abd"));
    assert!(!ct_eq(b"abc", b"abcd"));
}

#[test]
fn minted_bearers_are_fresh_256_bit_hex() {
    let a = mint_bearer();
    let b = mint_bearer();
    assert_eq!(a.len(), BEARER_LEN * 2);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(*a, *b);
}

#[test]
fn the_wire_format_is_tagged_by_op() {
    let req = SvcRequest {
        v: PROTOCOL_VERSION,
        bearer: String::from("b").into(),
        op: SvcOp::SetCoinbase {
            coinbase: "0x00".into(),
        },
    };
    let j = serde_json::to_value(&req).unwrap();
    assert_eq!(j["op"], "set_coinbase");
    assert_eq!(j["coinbase"], "0x00");
    assert_eq!(j["v"], 1);
    let back: SvcRequest =
        serde_json::from_str(r#"{"v":1,"bearer":"x","op":"node_start"}"#).unwrap();
    assert_eq!(back.op, SvcOp::NodeStart);
}

#[test]
fn exe_to_watch_prefers_an_absolute_appimage() {
    let cur = Some(PathBuf::from("/tmp/.mount_x/usr/bin/citrate-core"));
    assert_eq!(
        exe_to_watch(Some(PathBuf::from("/home/m/Citrate.AppImage")), cur.clone()),
        Some(PathBuf::from("/home/m/Citrate.AppImage"))
    );
    assert_eq!(
        exe_to_watch(Some(PathBuf::from("rel.AppImage")), cur.clone()),
        cur
    );
    assert_eq!(exe_to_watch(None, cur.clone()), cur);
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::svc_server::{answer, BindError, Control, SvcHost, SvcServer};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// Records what was dispatched; answers fixed values. Test-only.
    #[derive(Default)]
    pub(crate) struct Recording {
        pub calls: Mutex<Vec<String>>,
    }
    impl Recording {
        fn rec(&self, s: &str) {
            self.calls.lock().unwrap().push(s.to_string());
        }
    }
    impl SvcHost for Recording {
        fn hello(&self) -> SvcHello {
            self.rec("hello");
            SvcHello {
                protocol: PROTOCOL_VERSION,
                app_version: env!("CARGO_PKG_VERSION").into(),
                pid: std::process::id(),
                started_unix_ms: 1,
            }
        }
        fn status(&self) -> SvcStatus {
            self.rec("status");
            SvcStatus {
                node: crate::node::NodeStatus {
                    state: "running".into(),
                    peers: 0,
                    height: 0,
                    sync_pct: 0.0,
                    notice: None,
                    blocked: None,
                    mining_armed: false,
                    mining_hold: None,
                    sync_state: None,
                    peer_tips: None,
                },
                ipfs_running: true,
                node_hold: None,
                drained: false,
                coinbase: None,
            }
        }
        fn node_start(&self) -> Result<(), String> {
            self.rec("node_start");
            Ok(())
        }
        fn node_stop(&self) {
            self.rec("node_stop");
        }
        fn ipfs_start(&self) -> Result<(), String> {
            self.rec("ipfs_start");
            Ok(())
        }
        fn ipfs_stop(&self) {
            self.rec("ipfs_stop");
        }
        fn set_coinbase(&self, c: String) -> Result<(), String> {
            self.rec(&format!("set_coinbase {c}"));
            Ok(())
        }
        fn arm_mining(&self) -> bool {
            self.rec("arm_mining");
            false
        }
        fn drain(&self) -> SvcDrain {
            self.rec("drain");
            SvcDrain {
                still_exiting: vec![],
                took_ms: 0,
            }
        }
        fn resume(&self) {
            self.rec("resume");
        }
    }

    /// A server on its own thread; returns the stop flag, the control and the host.
    fn serve(
        paths: &SvcPaths,
    ) -> (
        Arc<AtomicBool>,
        Arc<Control>,
        Arc<Recording>,
        std::thread::JoinHandle<()>,
    ) {
        let server = SvcServer::bind(paths).expect("bind");
        let host = Arc::new(Recording::default());
        let control = Control::new();
        let stop = Arc::new(AtomicBool::new(false));
        let (h, c, s) = (host.clone(), control.clone(), stop.clone());
        let join = std::thread::spawn(move || {
            let h: Arc<dyn SvcHost> = h;
            server.serve(h, c, s)
        });
        (stop, control, host, join)
    }

    fn mode(p: &std::path::Path) -> u32 {
        std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
    }

    /// SVC-1 (planset 05): with no service, the app runs its supervisors in-process.
    #[test]
    fn app_falls_back_in_process_when_svc_absent() {
        let root = tmp_root("absent");
        let paths = SvcPaths::under(&root);
        match attach(&paths) {
            Link::InProcess(why) => assert!(why.contains("not running"), "{why}"),
            Link::Attached(_) => panic!("attached to nothing"),
        }
        // The link state the app manages says the same, and without a data dir too.
        assert!(SvcLinkState::new(Some(paths.clone())).client().is_none());
        assert!(SvcLinkState::new(None).client().is_none());
        // A socket left by a service that died: nobody answers, so in-process.
        std::fs::create_dir_all(&paths.dir).unwrap();
        drop(std::os::unix::net::UnixListener::bind(&paths.socket).unwrap());
        citrate_core_kit::fsutil::write_secret_file(&paths.bearer, mint_bearer().as_bytes())
            .unwrap();
        match attach(&paths) {
            Link::InProcess(why) => assert!(why.contains("not answering"), "{why}"),
            Link::Attached(_) => panic!("attached to a dead socket"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn attach_needs_the_bearer_the_service_wrote() {
        let root = tmp_root("bearer");
        let paths = SvcPaths::under(&root);
        let (stop, _c, host, join) = serve(&paths);
        // The live service: attached, and hello was the only call.
        match attach(&paths) {
            Link::Attached(c) => assert_eq!(c.hello.protocol, PROTOCOL_VERSION),
            Link::InProcess(why) => panic!("not attached: {why}"),
        }
        // Another token in the file (a planted or stale one): refused before dispatch.
        citrate_core_kit::fsutil::write_secret_file(&paths.bearer, mint_bearer().as_bytes())
            .unwrap();
        match attach(&paths) {
            Link::InProcess(why) => assert!(why.contains("unauthorized"), "{why}"),
            Link::Attached(_) => panic!("attached with a wrong bearer"),
        }
        assert_eq!(host.calls.lock().unwrap().as_slice(), ["hello"]);
        stop.store(true, Ordering::SeqCst);
        join.join().unwrap();
        // The server removes its socket and bearer on the way out.
        assert!(!paths.socket.exists());
        assert!(!paths.bearer.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bearer_file_others_can_read_or_a_link_is_refused() {
        let root = tmp_root("loose");
        let b = root.join("svc.bearer");
        citrate_core_kit::fsutil::write_secret_file(&b, mint_bearer().as_bytes()).unwrap();
        assert!(read_bearer(&b).is_ok());
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            read_bearer(&b).unwrap_err(),
            BearerRefusal::ReadableByOthers
        );
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o600)).unwrap();
        let l = root.join("link.bearer");
        std::os::unix::fs::symlink(&b, &l).unwrap();
        assert_eq!(read_bearer(&l).unwrap_err(), BearerRefusal::NotARegularFile);
        std::fs::write(&b, "not-hex").unwrap();
        assert_eq!(read_bearer(&b).unwrap_err(), BearerRefusal::Malformed);
        assert_eq!(
            read_bearer(&root.join("none")).unwrap_err(),
            BearerRefusal::Missing
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_socket_and_bearer_are_owner_only_and_one_service_per_user() {
        let root = tmp_root("modes");
        let paths = SvcPaths::under(&root);
        let (stop, _c, _h, join) = serve(&paths);
        assert_eq!(mode(&paths.dir), 0o700);
        assert_eq!(mode(&paths.bearer), 0o600);
        assert_eq!(mode(&paths.socket), 0o600);
        // A second service for the same user refuses to bind beside the live one.
        assert_eq!(
            SvcServer::bind(&paths).unwrap_err(),
            BindError::AlreadyRunning
        );
        stop.store(true, Ordering::SeqCst);
        join.join().unwrap();
        // With the first gone, a stale socket file does not block a new service.
        drop(std::os::unix::net::UnixListener::bind(&paths.socket).unwrap());
        let again = SvcServer::bind(&paths).expect("rebind over a stale socket");
        drop(again);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_op_is_dispatched_only_behind_the_bearer() {
        let host = Recording::default();
        let control = Control::default();
        let good = "a".repeat(64);
        let line = |op: &str| format!(r#"{{"v":1,"bearer":"{good}","op":"{op}"}}"#);
        // Wrong bearer, malformed line, other protocol: nothing dispatched.
        let bad = format!(
            r#"{{"v":1,"bearer":"{}","op":"node_stop"}}"#,
            "b".repeat(64)
        );
        assert_eq!(
            answer(&bad, &host, &control, &good).error.as_deref(),
            Some("unauthorized")
        );
        assert_eq!(
            answer("{", &host, &control, &good).error.as_deref(),
            Some("malformed request")
        );
        let v2 = format!(r#"{{"v":2,"bearer":"{good}","op":"node_stop"}}"#);
        assert!(answer(&v2, &host, &control, &good)
            .error
            .unwrap()
            .contains("protocol 2"));
        assert!(host.calls.lock().unwrap().is_empty());
        for op in [
            "node_start",
            "node_stop",
            "ipfs_start",
            "ipfs_stop",
            "arm_mining",
            "drain",
            "resume",
        ] {
            assert!(answer(&line(op), &host, &control, &good).ok, "{op}");
        }
        let cb = format!(r#"{{"v":1,"bearer":"{good}","op":"set_coinbase","coinbase":"0xab"}}"#);
        assert!(answer(&cb, &host, &control, &good).ok);
        assert_eq!(
            host.calls.lock().unwrap().as_slice(),
            [
                "node_start",
                "node_stop",
                "ipfs_start",
                "ipfs_stop",
                "arm_mining",
                "drain",
                "resume",
                "set_coinbase 0xab"
            ]
        );
        assert_eq!(control.exit_requested(), None);
        // Restart asks for the restart code; Shutdown drains first and asks for 0 (first wins,
        // so a fresh control for each).
        assert!(answer(&line("restart"), &host, &control, &good).ok);
        assert_eq!(control.exit_requested(), Some(EXIT_RESTART));
        let c2 = Control::default();
        assert!(answer(&line("shutdown"), &host, &c2, &good).ok);
        assert_eq!(c2.exit_requested(), Some(0));
        assert_eq!(
            host.calls.lock().unwrap().last().map(String::as_str),
            Some("drain")
        );
    }

    #[test]
    fn a_client_reads_status_and_drain_over_the_socket() {
        let root = tmp_root("client");
        let paths = SvcPaths::under(&root);
        let (stop, _c, host, join) = serve(&paths);
        let Link::Attached(client) = attach(&paths) else {
            panic!("not attached")
        };
        let st = client.status().unwrap();
        assert_eq!(st.node.state, "running");
        assert!(st.ipfs_running);
        assert!(client.drain().unwrap().still_exiting.is_empty());
        // The bearer never appears in the client's Debug output.
        let bearer = read_bearer(&paths.bearer).unwrap();
        assert!(!format!("{client:?}").contains(bearer.as_str()));
        assert_eq!(
            host.calls.lock().unwrap().as_slice(),
            ["hello", "status", "drain"]
        );
        stop.store(true, Ordering::SeqCst);
        join.join().unwrap();
        // The link state notices the service went away once its cache is dropped.
        let link = SvcLinkState::new(Some(paths.clone()));
        link.invalidate();
        assert!(link.client().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stale_service_is_told_to_restart_and_a_current_one_is_not() {
        let root = tmp_root("stale");
        let paths = SvcPaths::under(&root);
        let (stop, control, _h, join) = serve(&paths);
        let Link::Attached(client) = attach(&paths) else {
            panic!("not attached")
        };
        assert!(!restart_if_stale(&client, env!("CARGO_PKG_VERSION")));
        assert_eq!(control.exit_requested(), None);
        assert!(restart_if_stale(&client, "0.0.0-other"));
        assert_eq!(control.exit_requested(), Some(EXIT_RESTART));
        stop.store(true, Ordering::SeqCst);
        join.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The single-executable choice (see the module doc): the app's startup orphan sweep leaves
    /// the service's node alone because its parent runs the app's main executable. A separate
    /// service binary would get its node signalled.
    #[test]
    fn svc_children_are_not_orphans_to_the_app() {
        use crate::owned_cleanup::{select_orphans, ProcEntry};
        let main = PathBuf::from("/opt/citrate/citrate-core");
        let node = PathBuf::from("/opt/citrate/citrate");
        let app_pid = 500;
        let svc = ProcEntry {
            pid: 100,
            ppid: 1,
            exe: main.clone(),
        };
        let svc_node = ProcEntry {
            pid: 101,
            ppid: 100,
            exe: node.clone(),
        };
        let procs = vec![svc, svc_node];
        assert!(
            select_orphans(&procs, std::slice::from_ref(&node), Some(&main), app_pid).is_empty()
        );
        let other = ProcEntry {
            pid: 100,
            ppid: 1,
            exe: PathBuf::from("/opt/citrate/citrate-svc"),
        };
        let procs = vec![
            other,
            ProcEntry {
                pid: 101,
                ppid: 100,
                exe: node.clone(),
            },
        ];
        assert_eq!(
            select_orphans(&procs, &[node], Some(&main), app_pid),
            vec![101]
        );
    }

    #[test]
    fn the_update_drain_waits_for_the_service_too() {
        use crate::update_drain::{with_service_drain, DrainReport};
        let base = || DrainReport {
            all_stopped: true,
            still_exiting: vec![],
            services: vec![],
            took_ms: 1,
        };
        assert!(with_service_drain(base(), None).all_stopped);
        let ok = with_service_drain(
            base(),
            Some(Ok(SvcDrain {
                still_exiting: vec![],
                took_ms: 2,
            })),
        );
        assert!(ok.all_stopped);
        let left = with_service_drain(
            base(),
            Some(Ok(SvcDrain {
                still_exiting: vec!["node".into()],
                took_ms: 2,
            })),
        );
        assert!(!left.all_stopped);
        assert_eq!(
            left.still_exiting,
            vec!["background service: node".to_string()]
        );
        let err = with_service_drain(base(), Some(Err("connect: refused".into())));
        assert!(!err.all_stopped);
        assert!(err.still_exiting[0].contains("not drained"));
    }
}
