// CX-S3.1 — comms-relay sidecar lifecycle tests. Included into `comms::tests`.
//
// CI-safe: NO real comms-relay. A long-lived `sleep` stub stands in for the child (the same
// supervisor-stub pattern as serve_tests). These prove the lifecycle gates (configured + binary),
// the env wiring (owner/binds/key carried via ENV, never argv), start→Running→stop, and the
// idempotent-start guard.

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

/// A configured manager over the sleep stub (owner + master key set), ready to start.
fn stub_manager(tag: &str) -> (CommsRelayManager, PathBuf) {
    let dir = tmp_dir(tag);
    let crash = dir.join("comms-crash-records.jsonl");
    let mgr = CommsRelayManager::new(
        sleep_bin(),
        dir.join("data"),
        "0x00112233445566778899aabbccddeeff00112233", // dummy owner
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef", // dummy master key
        crash,
    );
    (mgr, dir)
}

#[test]
fn start_refuses_when_not_configured() {
    let dir = tmp_dir("noconf");
    // Empty owner + key → NotConfigured, WITHOUT touching the binary.
    let mgr = CommsRelayManager::new(sleep_bin(), dir.join("data"), "", "", dir.join("crash"));
    assert!(!mgr.is_configured());
    let r = mgr.start();
    assert!(matches!(r, Err(CommsError::NotConfigured)), "got {r:?}");
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn start_refuses_when_binary_missing() {
    let dir = tmp_dir("nobin");
    let mgr = CommsRelayManager::new(
        dir.join("does-not-exist-comms-relay"),
        dir.join("data"),
        "0xabc0000000000000000000000000000000000abc",
        "aa".repeat(32),
        dir.join("crash"),
    );
    let r = mgr.start();
    assert!(matches!(r, Err(CommsError::BinaryNotFound(_))), "got {r:?}");
}

#[test]
fn spec_env_carries_owner_binds_and_key_never_argv() {
    let (mgr, _dir) = stub_manager("env");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(ENV_OWNER).map(String::as_str), Some("0x00112233445566778899aabbccddeeff00112233"));
    assert_eq!(env.get(ENV_WS_BIND).map(String::as_str), Some(DEFAULT_COMMS_WS_BIND));
    assert_eq!(env.get(ENV_ADMIN_BIND).map(String::as_str), Some(DEFAULT_COMMS_ADMIN_BIND));
    assert!(env.contains_key(ENV_MASTER_KEY));
    assert!(env.contains_key(ENV_DATA));
    // The relay is env-configured — no positional argv leaks the config to `ps`.
    assert!(mgr.spec_env_for_test().iter().all(|(k, _)| k.starts_with("CITRATE_COMMS_")));
}

#[test]
fn ws_url_is_loopback_ws() {
    let (mgr, _dir) = stub_manager("wsurl");
    assert_eq!(mgr.ws_url(), format!("ws://{DEFAULT_COMMS_WS_BIND}"));
    assert_eq!(mgr.status().ws_url, format!("ws://{DEFAULT_COMMS_WS_BIND}"));
}

#[test]
fn start_reaches_running_then_stop_is_clean() {
    let (mgr, _dir) = stub_manager("wiring");
    let mgr = mgr
        .with_spawn_args(vec!["3600".to_string()]) // a long-lived stub child
        .with_health_interval(std::time::Duration::from_secs(3600)); // avoid the startup race
    mgr.start().expect("stub comms-relay starts");
    let mut running = false;
    for _ in 0..100 {
        if mgr.status().state == "running" {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "comms-relay stub must reach Running under the supervisor");
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
    let r = mgr.start();
    assert!(matches!(r, Err(CommsError::AlreadyRunning)), "got {r:?}");
    mgr.stop();
}
