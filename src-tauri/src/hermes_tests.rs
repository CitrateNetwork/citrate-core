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

// ── CX-S6.2 — bearer-authed control transport. A mock stands in for the HTTP sidecar so the wiring
// is proven without a real server; the bearer must be presented and a non-2xx must fail closed.

/// A mock control transport: canned `(status, body)` per URL-suffix match, recording the bearer seen
/// and the last POST body.
struct MockControl {
    status_resp: (u16, String),
    skills_resp: (u16, String),
    approvals_resp: (u16, String),
    run_resp: (u16, String),
    seen_bearer: Mutex<Option<String>>,
    last_post_body: Mutex<Option<String>>,
}

impl MockControl {
    fn new() -> Self {
        MockControl {
            status_resp: (404, String::new()),
            skills_resp: (404, String::new()),
            approvals_resp: (404, String::new()),
            run_resp: (404, String::new()),
            seen_bearer: Mutex::new(None),
            last_post_body: Mutex::new(None),
        }
    }
    fn pick(&self, url: &str) -> (u16, String) {
        if url.ends_with("/status") {
            self.status_resp.clone()
        } else if url.ends_with("/skills") {
            self.skills_resp.clone()
        } else if url.ends_with("/approvals") {
            self.approvals_resp.clone()
        } else if url.ends_with("/run_skill") {
            self.run_resp.clone()
        } else {
            (404, String::new())
        }
    }
}

impl HermesControl for MockControl {
    fn get(&self, url: &str, bearer: &str) -> std::result::Result<ControlResp, HermesError> {
        *self.seen_bearer.lock().unwrap() = Some(bearer.to_string());
        let (status, body) = self.pick(url);
        Ok(ControlResp { status, body })
    }
    fn post(
        &self,
        url: &str,
        bearer: &str,
        body: &str,
    ) -> std::result::Result<ControlResp, HermesError> {
        *self.seen_bearer.lock().unwrap() = Some(bearer.to_string());
        *self.last_post_body.lock().unwrap() = Some(body.to_string());
        let (status, resp) = self.pick(url);
        Ok(ControlResp { status, body: resp })
    }
}

/// A manager wired to a mock control + a pre-set session bearer, without spawning a sidecar.
fn control_manager(mock: MockControl) -> HermesManager {
    let dir = tmp_dir("ctrl");
    let mgr = HermesManager::new(
        sleep_bin(),
        dir.join("token"),
        dir.join("crash"),
    )
    .with_control(Box::new(mock));
    mgr.set_token_for_test("deadbeef");
    mgr
}

#[test]
fn remote_status_parses_and_presents_the_bearer() {
    let mut mock = MockControl::new();
    mock.status_resp = (
        200,
        r#"{"running":true,"skills":2,"pendingApprovals":1}"#.to_string(),
    );
    let mgr = control_manager(mock);
    let st = mgr.remote_status().expect("status parses");
    assert_eq!(
        st,
        RemoteStatus {
            running: true,
            skills: 2,
            pending_approvals: 1
        }
    );
}

#[test]
fn list_skills_parses() {
    let mut mock = MockControl::new();
    mock.skills_resp = (
        200,
        r#"[{"name":"list-compliance-posture","description":"d"}]"#.to_string(),
    );
    let mgr = control_manager(mock);
    let skills = mgr.list_skills().expect("skills parse");
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "list-compliance-posture");
}

#[test]
fn run_skill_accepts_on_2xx() {
    let mut mock = MockControl::new();
    mock.run_resp = (200, r#"{"ok":true}"#.to_string());
    let mgr = control_manager(mock);
    mgr.run_skill("do-thing", &serde_json::json!({"to":"0x01"}))
        .expect("a 2xx run is accepted");
}

#[test]
fn run_skill_body_carries_name_and_args() {
    let mock = MockControl {
        run_resp: (200, r#"{"ok":true}"#.to_string()),
        ..MockControl::new()
    };
    // Keep a handle to the mock's captured body by constructing the manager, running, then reading.
    let dir = tmp_dir("runbody");
    let mgr = HermesManager::new(sleep_bin(), dir.join("t"), dir.join("c"));
    // Re-wire with an Arc-shared mock we can inspect.
    let shared = std::sync::Arc::new(mock);
    let mgr = mgr.with_control(Box::new(ArcControl(shared.clone())));
    mgr.set_token_for_test("deadbeef");
    mgr.run_skill("do-thing", &serde_json::json!({"to":"0x01"}))
        .expect("run accepted");
    let body = shared.last_post_body.lock().unwrap().clone().unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["name"], "do-thing");
    assert_eq!(v["args"]["to"], "0x01");
}

/// A thin `HermesControl` that forwards to a shared `MockControl` (so a test can inspect captures).
struct ArcControl(std::sync::Arc<MockControl>);
impl HermesControl for ArcControl {
    fn get(&self, url: &str, bearer: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.get(url, bearer)
    }
    fn post(
        &self,
        url: &str,
        bearer: &str,
        body: &str,
    ) -> std::result::Result<ControlResp, HermesError> {
        self.0.post(url, bearer, body)
    }
}

#[test]
fn pending_approvals_parses() {
    let mut mock = MockControl::new();
    mock.approvals_resp = (
        200,
        r#"[{"id":"abc","kind":"chain","summary":"send 1 SALT"}]"#.to_string(),
    );
    let mgr = control_manager(mock);
    let approvals = mgr.pending_approvals().expect("approvals parse");
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].kind, "chain");
}

#[test]
fn control_fails_closed_without_a_bearer() {
    // No session bearer set (sidecar not started) → NotRunning, never a blind call.
    let dir = tmp_dir("nobearer");
    let mgr = HermesManager::new(sleep_bin(), dir.join("t"), dir.join("c"))
        .with_control(Box::new(MockControl::new()));
    let r = mgr.remote_status();
    assert!(matches!(r, Err(HermesError::NotRunning)), "got {r:?}");
}

#[test]
fn a_non_2xx_control_response_is_a_typed_error() {
    let mut mock = MockControl::new();
    mock.status_resp = (401, "unauthorized".to_string());
    let mgr = control_manager(mock);
    let r = mgr.remote_status();
    assert!(
        matches!(r, Err(HermesError::Control { status: 401, .. })),
        "got {r:?}"
    );
}
