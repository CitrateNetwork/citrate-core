//! V60 SVC-1: the service process. The process tests re-run this test binary as the service
//! (and as a short-lived "app") with a stub node, a stub kubo and an in-memory keyring, in temp
//! dirs. Nothing here binds a node port, dials a host, or runs a unit manager.

use super::*;
use citrate_core_kit::custody::{CustodyError, Keyring};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::svc::{attach, Link, SvcOp};

/// The env var that turns a re-run of this binary into the service child.
const CHILD_ENV: &str = "CITRATE_SVC_TEST_CHILD_ROOT";
/// The env var that turns a re-run into the short-lived app child.
const APP_ENV: &str = "CITRATE_SVC_TEST_APP_ROOT";

/// An in-memory keyring for the child service (the real one is the OS keyring). Test-only.
#[derive(Default)]
struct MemKeyring(std::sync::Mutex<HashMap<String, Vec<u8>>>);
impl Keyring for MemKeyring {
    fn get(&self, a: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.0.lock().unwrap().get(a).cloned())
    }
    fn set(&self, a: &str, s: &[u8]) -> Result<(), CustodyError> {
        self.0.lock().unwrap().insert(a.to_string(), s.to_vec());
        Ok(())
    }
    fn delete(&self, a: &str) -> Result<(), CustodyError> {
        self.0.lock().unwrap().remove(a);
        Ok(())
    }
}

/// A keyring that is not reachable (Linux before login). Test-only.
struct LockedKeyring;
impl Keyring for LockedKeyring {
    fn get(&self, _a: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> Result<(), CustodyError> {
        Ok(())
    }
}

/// The checked-in native node fixture, compiled once into OUT_DIR (as `node_tests` does).
fn stub_node() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stub_node.rs");
        let out_dir = PathBuf::from(env!("OUT_DIR")).join("test-fixtures");
        std::fs::create_dir_all(&out_dir).unwrap();
        let out = out_dir.join(format!("svc_stub_node-{}", std::process::id()));
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let ok = Command::new(rustc)
            .arg("--edition=2021")
            .arg(&src)
            .arg("-o")
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok, "stub node fixture must compile");
        out
    })
    .clone()
}

/// A stub kubo: `init` and `config` succeed, `daemon` sleeps until signalled.
fn stub_ipfs(dir: &std::path::Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("stub-ipfs");
    std::fs::write(
        &p,
        "#!/bin/sh\ncase \"$1\" in daemon) exec sleep 3600;; *) exit 0;; esac\n",
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn tmp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!("csr-{tag}-{}-{nanos}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn opened_barrier() -> std::sync::Arc<crate::startup_barrier::StartupBarrier> {
    let b = std::sync::Arc::new(crate::startup_barrier::StartupBarrier::new());
    b.run_cleanup(|| {});
    b
}

/// A host over the stub node and kubo, with its own spawn gate (never the process-wide one, so
/// tests in this binary do not hold each other's gate).
fn stub_host(root: &std::path::Path, keyring: Box<dyn Keyring>) -> NodeHost {
    let paths = SvcPaths::under(root);
    let node = NodeManager::new(
        keyring,
        stub_node(),
        root.join("node"),
        root.join("node/crash-records.jsonl"),
        // Nothing listens here: status reads 0/0 honestly.
        "http://127.0.0.1:59998",
    )
    .with_startup_barrier(opened_barrier(), Duration::ZERO)
    .with_node_ports(Vec::new());
    let ipfs = IpfsManager::new(
        stub_ipfs(root),
        root.join("ipfs"),
        root.join("ipfs/crash.jsonl"),
    );
    NodeHost::new(node, ipfs, paths, SpawnGate::global())
}

#[test]
fn coinbases_are_checked() {
    assert!(valid_coinbase("0x00000000000000000000000000000000000000aB"));
    assert!(!valid_coinbase(
        "00000000000000000000000000000000000000ab00"
    ));
    assert!(!valid_coinbase("0x123"));
    assert!(!valid_coinbase(
        "0xzz000000000000000000000000000000000000ab"
    ));
}

#[test]
fn a_drain_restarts_for_a_new_executable_and_releases_after_its_limit() {
    let max = Duration::from_secs(900);
    let a = Some((1, 10, 100));
    let b = Some((1, 11, 200));
    assert_eq!(drain_step(None, a, b, max), DrainStep::Keep);
    assert_eq!(
        drain_step(Some(Duration::from_secs(1)), a, a, max),
        DrainStep::Keep
    );
    assert_eq!(
        drain_step(Some(Duration::from_secs(1)), a, b, max),
        DrainStep::Restart
    );
    // Mid-replacement (no file yet): wait for the new one.
    assert_eq!(
        drain_step(Some(Duration::from_secs(1)), a, None, max),
        DrainStep::Keep
    );
    assert_eq!(drain_step(Some(max), a, a, max), DrainStep::Release);
}

#[test]
fn file_identity_changes_when_a_file_is_replaced() {
    let root = tmp_root("ident");
    let f = root.join("exe");
    std::fs::write(&f, "a").unwrap();
    let before = file_identity(&f);
    let tmp = root.join("exe.new");
    std::fs::write(&tmp, "b").unwrap();
    std::fs::rename(&tmp, &f).unwrap();
    assert!(before.is_some());
    assert_ne!(before, file_identity(&f));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_log_dir_is_the_apps() {
    let d = headless_log_dir(std::path::Path::new("/data/ai.citrate.core"));
    #[cfg(target_os = "macos")]
    assert!(
        d.ends_with("Library/Logs/ai.citrate.core"),
        "{}",
        d.display()
    );
    #[cfg(not(target_os = "macos"))]
    assert_eq!(d, PathBuf::from("/data/ai.citrate.core/logs"));
}

/// Spike finding 1: a keyring that is not reachable holds the node start with the reason and is
/// retried by the tick; the service does not fail and there is no plaintext fallback.
#[test]
fn a_locked_keyring_holds_the_node_and_is_retried() {
    let root = tmp_root("locked");
    let host = stub_host(&root, Box::new(LockedKeyring));
    host.tick();
    let st = host.status();
    assert_eq!(st.node.state, "stopped");
    let hold = st.node_hold.expect("held with a reason");
    assert!(hold.to_lowercase().contains("keyring"), "{hold}");
    // No key file was written anywhere under the data root.
    assert!(!root.join("node").join("storage.key").exists());
    host.tick();
    assert!(host.status().node_hold.is_some());
    host.ipfs_stop();
    let _ = std::fs::remove_dir_all(&root);
}

/// In-process: start, a member Stop kept across ticks, the coinbase. The drain is exercised in
/// the process tests only, because it holds the process-wide spawn gate (other tests in this
/// binary start supervisors under it).
#[test]
fn the_host_starts_both_and_keeps_a_member_stop() {
    let root = tmp_root("host");
    let host = stub_host(&root, Box::new(MemKeyring::default()));
    host.tick();
    let st = host.status();
    assert_ne!(st.node.state, "stopped", "{st:?}");
    assert!(st.node_hold.is_none());
    // A member Stop is kept across ticks.
    host.node_stop();
    host.tick();
    assert_eq!(host.status().node.state, "stopped");
    host.node_start().unwrap();
    assert_ne!(host.status().node.state, "stopped");
    // The coinbase is checked, kept on disk and handed to the node. No tick runs after this:
    // with a coinbase the mining gate reads the public RPC, which no test may reach.
    assert!(host.set_coinbase("nope".into()).is_err());
    let cb = "0x00000000000000000000000000000000000000ab".to_string();
    host.set_coinbase(cb.clone()).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("svc/coinbase")).unwrap(),
        cb
    );
    assert_eq!(host.status().coinbase.as_deref(), Some(cb.as_str()));
    host.node_stop();
    host.ipfs_stop();
    // A host built again over the same root picks the coinbase up from disk.
    let again = stub_host(&root, Box::new(MemKeyring::default()));
    assert_eq!(again.status().coinbase.as_deref(), Some(cb.as_str()));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// Process tests
// ---------------------------------------------------------------------------

/// Re-run this test binary for one ignored entry point with `env=root`.
fn spawn_entry(entry: &str, env: &str, root: &std::path::Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            entry,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(env, root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

fn wait_attached(paths: &SvcPaths, within: Duration) -> crate::svc::SvcClient {
    let t0 = Instant::now();
    loop {
        if let Link::Attached(c) = attach(paths) {
            return c;
        }
        assert!(t0.elapsed() < within, "the service did not come up");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_exit(child: &mut Child, within: Duration) -> Option<std::process::ExitStatus> {
    let t0 = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return Some(s);
        }
        if t0.elapsed() >= within {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The service child: runs [`run_service`] over the stub host and exits with its code.
#[test]
#[ignore = "entry point for svc_survives_app_exit; does nothing unless CITRATE_SVC_TEST_CHILD_ROOT is set"]
fn svc_child_entry() {
    let Some(root) = std::env::var_os(CHILD_ENV).map(PathBuf::from) else {
        return;
    };
    let host = std::sync::Arc::new(stub_host(&root, Box::new(MemKeyring::default())));
    let code = run_service(
        &SvcPaths::under(&root),
        host,
        Duration::from_millis(200),
        DRAIN_MAX,
    );
    std::process::exit(code);
}

/// The "app" child: attaches, reads the node's status the way `node_status` does, prints it, and
/// exits (the app window closing and the app quitting).
#[test]
#[ignore = "entry point for svc_survives_app_exit; does nothing unless CITRATE_SVC_TEST_APP_ROOT is set"]
fn svc_app_entry() {
    let Some(root) = std::env::var_os(APP_ENV).map(PathBuf::from) else {
        return;
    };
    let Link::Attached(c) = attach(&SvcPaths::under(&root)) else {
        std::process::exit(3);
    };
    let st = c.status().unwrap();
    println!("APP-SAW node={} ipfs={}", st.node.state, st.ipfs_running);
    std::process::exit(0);
}

/// SVC-1 (planset 05) / F-B1: the node runs under the service, not under the app: the app
/// attaches, reads it, and exits, and the service and its node keep running. A Shutdown then
/// drains the node and the service exits 0 and removes its socket.
#[test]
fn svc_survives_app_exit() {
    let root = tmp_root("survive");
    let paths = SvcPaths::under(&root);
    let mut svc = spawn_entry("svc_run::tests::svc_child_entry", CHILD_ENV, &root);
    let client = wait_attached(&paths, Duration::from_secs(20));
    assert_ne!(
        client.hello.pid,
        std::process::id(),
        "the service is its own process"
    );
    // Wait for the node to be up under the service.
    let t0 = Instant::now();
    while client.status().unwrap().node.state == "stopped" {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the node did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // The app comes and goes.
    let app = spawn_entry("svc_run::tests::svc_app_entry", APP_ENV, &root);
    let out = app.wait_with_output().unwrap();
    assert!(out.status.success(), "app child failed: {:?}", out.status);
    let seen = String::from_utf8_lossy(&out.stdout);
    assert!(seen.contains("APP-SAW node="), "{seen}");
    assert!(!seen.contains("node=stopped"), "{seen}");
    // After the app exited: the service is alive and its node still runs.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        svc.try_wait().unwrap().is_none(),
        "the service exited with the app"
    );
    let again = wait_attached(&paths, Duration::from_secs(5));
    assert_eq!(
        again.hello.pid, client.hello.pid,
        "same service, not a restart"
    );
    let st = again.status().unwrap();
    assert_ne!(st.node.state, "stopped", "{st:?}");
    // A drain (CORE-D6) stops both and refuses starts until resume.
    let d = again.drain().unwrap();
    assert!(d.still_exiting.is_empty(), "{d:?}");
    let st = again.status().unwrap();
    assert!(st.drained);
    assert_eq!(st.node.state, "stopped");
    assert!(!st.ipfs_running);
    assert_eq!(again.call(SvcOp::NodeStart).unwrap_err(), DRAIN_HOLD);
    again.call(SvcOp::Resume).unwrap();
    assert!(!again.status().unwrap().drained);
    assert_ne!(again.status().unwrap().node.state, "stopped");
    // Turning the service off: drain, exit 0, socket and bearer gone.
    let report = again.call(SvcOp::Shutdown).unwrap().unwrap();
    let report: crate::svc::SvcDrain = serde_json::from_value(report).unwrap();
    assert!(report.still_exiting.is_empty(), "{report:?}");
    let status = wait_exit(&mut svc, Duration::from_secs(40)).expect("the service exits");
    assert_eq!(status.code(), Some(0));
    assert!(!paths.socket.exists());
    assert!(!paths.bearer.exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// SIGTERM (the unit manager's stop) drains and exits 0; a `restart` exits [`EXIT_RESTART`].
#[test]
fn svc_stops_on_sigterm_and_restarts_on_request() {
    let root = tmp_root("sigterm");
    let paths = SvcPaths::under(&root);
    let mut svc = spawn_entry("svc_run::tests::svc_child_entry", CHILD_ENV, &root);
    let _ = wait_attached(&paths, Duration::from_secs(20));
    let pid = svc.id() as libc::pid_t;
    // SAFETY: kill(2) on our own child's pid with a valid signal number.
    #[allow(unsafe_code)]
    let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    assert_eq!(rc, 0);
    let status = wait_exit(&mut svc, Duration::from_secs(40)).expect("exits on SIGTERM");
    assert_eq!(status.code(), Some(0));
    assert!(!paths.socket.exists());

    let mut svc = spawn_entry("svc_run::tests::svc_child_entry", CHILD_ENV, &root);
    let c = wait_attached(&paths, Duration::from_secs(20));
    c.call(SvcOp::Restart).unwrap();
    let status = wait_exit(&mut svc, Duration::from_secs(40)).expect("exits on restart");
    assert_eq!(status.code(), Some(EXIT_RESTART));
    let _ = std::fs::remove_dir_all(&root);
}
