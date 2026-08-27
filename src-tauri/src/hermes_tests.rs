// CX-S6.1 — Hermes sidecar lifecycle tests. Included into `hermes::tests`.
//
// CI-safe: NO real hermes. A long-lived `sleep` stub stands in for the child. These prove the
// lifecycle gates (binary present), the env wiring (control addr + token-file PATH carried, and
// NEVER the token itself in env/argv), the bearer file is written 0600, start→Running→stop, and
// the idempotent-start guard.

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
    p.push(format!("citrate-core-hermes-{tag}-{nanos}-{:?}", std::thread::current().id()));
    std::fs::create_dir_all(&p).expect("mk tmpdir");
    p
}

fn stub_manager(tag: &str) -> (HermesManager, PathBuf) {
    let dir = tmp_dir(tag);
    let mgr = HermesManager::new(
        sleep_bin(),
        dir.join("hermes").join("token"),
        dir.join("hermes-crash.jsonl"),
    );
    (mgr, dir)
}

#[test]
fn start_refuses_when_binary_missing() {
    let dir = tmp_dir("nobin");
    let mgr = HermesManager::new(
        dir.join("no-such-hermes"),
        dir.join("token"),
        dir.join("crash"),
    );
    let r = mgr.start();
    assert!(matches!(r, Err(HermesError::BinaryNotFound(_))), "got {r:?}");
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn spec_env_carries_addr_and_token_path_never_the_token() {
    let (mgr, _dir) = stub_manager("env");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(HERMES_ADDR_ENV).map(String::as_str), Some(HERMES_CONTROL_ADDR));
    assert!(env.contains_key(HERMES_TOKEN_FILE_ENV));
    // The env carries the token FILE PATH, never a 64-hex token value (that would leak via `ps`).
    for (_k, v) in mgr.spec_env_for_test() {
        let looks_like_token = v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit());
        assert!(!looks_like_token, "no bearer token may appear in the child env: {v}");
    }
}

#[test]
fn control_url_is_loopback_http() {
    let (mgr, _dir) = stub_manager("url");
    assert_eq!(mgr.control_url(), format!("http://{HERMES_CONTROL_ADDR}"));
    assert_eq!(mgr.status().control_url, format!("http://{HERMES_CONTROL_ADDR}"));
}

#[test]
fn start_mints_a_0600_bearer_file_then_reaches_running_and_stops() {
    let (mgr, dir) = stub_manager("wiring");
    let mgr = mgr
        .with_spawn_args(vec!["3600".to_string()])
        .with_health_interval(std::time::Duration::from_secs(3600));
    mgr.start().expect("stub hermes starts");

    // The bearer file exists, is 0600, and holds a 64-hex token.
    let token_path = dir.join("hermes").join("token");
    assert!(token_path.exists(), "bearer file must be written on start");
    let tok = std::fs::read_to_string(&token_path).unwrap();
    assert_eq!(tok.len(), 64);
    assert!(tok.bytes().all(|b| b.is_ascii_hexdigit()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "bearer file must be 0600");
    }

    let mut running = false;
    for _ in 0..100 {
        if mgr.status().state == "running" {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "hermes stub must reach Running under the supervisor");
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
    assert!(matches!(r, Err(HermesError::AlreadyRunning)), "got {r:?}");
    mgr.stop();
}
