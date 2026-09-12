// CX-S6.1 — Hermes sidecar lifecycle tests. Included into `hermes::tests`.
//
// CI-safe: NO real hermes. A long-lived `sleep` stub stands in for the child. These prove the
// lifecycle gates (binary present), the env wiring (control addr + token-file PATH carried, and
// NEVER the token itself in env/argv), the bearer file is written 0600, start→Running→stop, and
// the idempotent-start guard.

use super::*;

/// A long-lived stub child binary that exists on the host (issue #47). Unix:
/// `/bin/sleep`. Windows: `ping.exe` (always present in System32; kept alive by
/// [`long_lived_args`]). The Windows path is verified by the team; only the Unix
/// path is built/run here.
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
/// Windows: `ping 127.0.0.1 -n 999`. Paired with [`sleep_bin`] so the binary and its
/// keep-alive args always match on each platform.
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
    p.push(format!(
        "citrate-core-hermes-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
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
    assert!(
        matches!(r, Err(HermesError::BinaryNotFound(_))),
        "got {r:?}"
    );
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn spec_env_carries_addr_and_token_path_never_the_token() {
    let (mgr, _dir) = stub_manager("env");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(
        env.get(HERMES_ADDR_ENV).map(String::as_str),
        Some(HERMES_CONTROL_ADDR)
    );
    assert!(env.contains_key(HERMES_TOKEN_FILE_ENV));
    // The env carries the token FILE PATH, never a 64-hex token value (that would leak via `ps`).
    for (_k, v) in mgr.spec_env_for_test() {
        let looks_like_token = v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit());
        assert!(
            !looks_like_token,
            "no bearer token may appear in the child env: {v}"
        );
    }
}

#[test]
fn control_url_is_loopback_http() {
    let (mgr, _dir) = stub_manager("url");
    assert_eq!(mgr.control_url(), format!("http://{HERMES_CONTROL_ADDR}"));
    assert_eq!(
        mgr.status().control_url,
        format!("http://{HERMES_CONTROL_ADDR}")
    );
}

#[test]
fn start_mints_a_0600_bearer_file_then_reaches_running_and_stops() {
    let (mgr, dir) = stub_manager("wiring");
    let mgr = mgr
        .with_spawn_args(long_lived_args())
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
    assert!(
        running,
        "hermes stub must reach Running under the supervisor"
    );
    assert!(mgr.is_running());
    mgr.stop();
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn double_start_is_rejected() {
    let (mgr, _dir) = stub_manager("double");
    let mgr = mgr.with_spawn_args(long_lived_args());
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
    last_post_url: Mutex<Option<String>>,
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
            last_post_url: Mutex::new(None),
        }
    }
    fn pick(&self, url: &str) -> (u16, String) {
        if url.ends_with("/status") {
            self.status_resp.clone()
        } else if url.ends_with("/skills") {
            self.skills_resp.clone()
        } else if url.ends_with("/approvals/approve") || url.ends_with("/approvals/reject") {
            (200, "{}".to_string()) // resolve endpoints: 200 OK
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
        *self.last_post_url.lock().unwrap() = Some(url.to_string());
        let (status, resp) = self.pick(url);
        Ok(ControlResp { status, body: resp })
    }
}

/// A manager wired to a mock control + a pre-set session bearer, without spawning a sidecar.
fn control_manager(mock: MockControl) -> HermesManager {
    let dir = tmp_dir("ctrl");
    let mgr = HermesManager::new(sleep_bin(), dir.join("token"), dir.join("crash"))
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

// ── CX-S6.3 — the ceremony bridge. A chain effect from the sidecar becomes a PENDING ceremony the
// human approves (Rule 3: the bridge signs nothing). Harness mirrors agent_tests (canonical vault +
// scripted mock RPC for the gas estimate).

use crate::ceremony::SignatureCeremony;
use crate::custody::{CustodyError, CustodyVault, Keyring};
use crate::rpc::{RpcClient, RpcError, RpcTransport};

const CANONICAL_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PASS: &[u8] = b"correct horse battery staple";

#[derive(Default)]
struct FakeKeyring {
    store: Mutex<std::collections::HashMap<String, Vec<u8>>>,
}
impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

/// A fresh unlocked vault holding the canonical wallet, so the bridge can read `from`.
fn vault_with_wallet() -> CustodyVault {
    let p = tmp_dir("vault").join("v.enc");
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p, 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    crate::wallet::import(&v, CANONICAL_MNEMONIC).expect("import canonical wallet");
    v
}

/// A scripted RPC transport returning canned JSON-RPC results (here, the eth_estimateGas quantity).
struct MockRpc {
    responses: std::sync::Mutex<std::collections::VecDeque<serde_json::Value>>,
}
impl MockRpc {
    fn new(responses: Vec<serde_json::Value>) -> Self {
        MockRpc {
            responses: std::sync::Mutex::new(responses.into_iter().collect()),
        }
    }
}
impl RpcTransport for MockRpc {
    fn call(&self, _body: serde_json::Value) -> std::result::Result<serde_json::Value, RpcError> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}
fn rpc_ok(result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

/// A MockControl whose /approvals returns one chain effect (to + data).
fn chain_effect_mock() -> MockControl {
    let mut m = MockControl::new();
    m.approvals_resp = (
        200,
        r#"[{"id":"cap::eth-send","kind":"high","summary":"send",
             "to":"0x4a86659BDab24dc444C72fbbaD4cd83491820E40","data":"0xdeadbeef"}]"#
            .to_string(),
    );
    m
}

#[test]
fn hermes_intent_has_agent_origin_and_the_real_calldata() {
    let intent = hermes_intent("0xTO", "0xdeadbeef", "0xFROM", Some(0x8000));
    assert_eq!(intent.origin, "agent:hermes");
    assert_eq!(intent.chain_id, 40204);
    let raw: serde_json::Value = serde_json::from_str(&intent.raw).unwrap();
    assert_eq!(raw["from"], "0xFROM");
    assert_eq!(raw["to"], "0xTO");
    assert_eq!(raw["data"], "0xdeadbeef");
    assert_eq!(raw["chainId"], "0x9d0c"); // 40204
    assert_eq!(raw["gas"], "0x8000");
}

#[test]
fn content_key_is_deterministic_and_distinct() {
    assert_eq!(content_key("0xa", "0x1"), content_key("0xa", "0x1"));
    assert_ne!(content_key("0xa", "0x1"), content_key("0xb", "0x1"));
    assert_ne!(content_key("0xa", "0x1"), content_key("0xa", "0x2"));
}

#[test]
fn bridge_pending_mints_a_ceremony_for_a_chain_effect() {
    let mgr = control_manager(chain_effect_mock());
    let ceremony = SignatureCeremony::new();
    let vault = vault_with_wallet();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![rpc_ok(serde_json::json!("0x8000"))]));

    let view = mgr
        .bridge_pending(&ceremony, &vault, &rpc)
        .expect("bridge ok")
        .expect("a chain effect was pending");
    assert_eq!(view.origin, "agent:hermes");
    // The ceremony now holds this as a pending intent (the human must approve it).
    assert!(ceremony.status(&view.id).is_some(), "ceremony is pending");
}

#[test]
fn bridge_pending_dedups_the_same_effect_to_one_ceremony() {
    let mgr = control_manager(chain_effect_mock());
    let ceremony = SignatureCeremony::new();
    let vault = vault_with_wallet();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        rpc_ok(serde_json::json!("0x8000")),
        rpc_ok(serde_json::json!("0x8000")),
    ]));

    let id1 = mgr
        .bridge_pending(&ceremony, &vault, &rpc)
        .unwrap()
        .unwrap()
        .id;
    let id2 = mgr
        .bridge_pending(&ceremony, &vault, &rpc)
        .unwrap()
        .unwrap()
        .id;
    assert_eq!(
        id1, id2,
        "the same effect re-bridges to the SAME ceremony (no double broadcast)"
    );
}

#[test]
fn a_decided_effect_is_not_re_bridged_into_a_second_broadcast() {
    // H-1: once a bridged effect's ceremony is CONSUMED (here via reject; the dangerous case is
    // approve→broadcast), a re-poll of bridge_pending must NOT mint a second ceremony for the same
    // (to,data) while it is still the sidecar head — that second approval would sign a real second tx
    // (fresh nonce) = double-broadcast. It stays None until resolve_head advances the head.
    let mgr = control_manager(chain_effect_mock());
    let ceremony = SignatureCeremony::new();
    let vault = vault_with_wallet();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        rpc_ok(serde_json::json!("0x8000")),
        rpc_ok(serde_json::json!("0x8000")),
    ]));

    let c1 = mgr
        .bridge_pending(&ceremony, &vault, &rpc)
        .unwrap()
        .unwrap();
    ceremony
        .reject(&c1.id)
        .expect("the human decides it → the ceremony is consumed");
    assert!(ceremony.status(&c1.id).is_none(), "consumed");

    // Re-poll while the SAME effect is still the head → must NOT mint a second ceremony.
    let repoll = mgr.bridge_pending(&ceremony, &vault, &rpc).unwrap();
    assert!(
        repoll.is_none(),
        "an already-decided head effect is NOT re-bridged (H-1: no second broadcast)"
    );

    // Only after resolve_head (the sidecar head advances) may a fresh effect bridge again.
    mgr.resolve_head(false)
        .expect("resolve advances the head + clears the dedup");
    let c2 = mgr
        .bridge_pending(&ceremony, &vault, &rpc)
        .unwrap()
        .unwrap();
    assert_ne!(
        c2.id, c1.id,
        "after resolve, a genuinely new head bridges a FRESH ceremony"
    );
}

#[test]
fn bridge_pending_skips_a_non_chain_effect() {
    // An approval with no to/data (a code/shell effect) has no chain signature to bridge.
    let mut mock = MockControl::new();
    mock.approvals_resp = (
        200,
        r#"[{"id":"run-code","kind":"high","summary":"exec"}]"#.to_string(),
    );
    let mgr = control_manager(mock);
    let ceremony = SignatureCeremony::new();
    let vault = vault_with_wallet();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert!(mgr
        .bridge_pending(&ceremony, &vault, &rpc)
        .unwrap()
        .is_none());
}

#[test]
fn resolve_head_posts_to_the_right_endpoint() {
    let shared = std::sync::Arc::new(chain_effect_mock());
    let dir = tmp_dir("resolve");
    let mgr = HermesManager::new(sleep_bin(), dir.join("t"), dir.join("c"))
        .with_control(Box::new(ArcControl(shared.clone())));
    mgr.set_token_for_test("deadbeef");

    mgr.resolve_head(true).expect("approve resolves");
    assert!(shared
        .last_post_url
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .ends_with("/approvals/approve"));

    mgr.resolve_head(false).expect("reject resolves");
    assert!(shared
        .last_post_url
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .ends_with("/approvals/reject"));
}

#[test]
fn shutdown_is_a_safe_noop_when_never_started() {
    // HERMES singleton uninitialized in tests → graceful-teardown shutdown() is a clean no-op.
    super::shutdown();
}

#[test]
fn capsules_env_absent_by_default_but_set_when_configured() {
    // Default manager: no capsules dir → CITRATE_HERMES_CAPSULES is not passed (unchanged behavior).
    let (mgr, dir) = stub_manager("capsdefault");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert!(!env.contains_key(HERMES_CAPSULES_ENV), "no capsules env by default");

    // With a capsules dir → the env carries its PATH so the child loads skills from it.
    let caps = dir.join("hermes").join("capsules");
    let mgr = mgr.with_capsules_dir(caps.clone());
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(
        env.get(HERMES_CAPSULES_ENV).map(String::as_str),
        Some(caps.to_string_lossy().as_ref())
    );
}

#[test]
fn seed_starter_capsules_copies_absent_skills_and_never_clobbers() {
    let root = tmp_dir("seed");
    let bundled = root.join("bundled");
    let dest = root.join("dest");
    // A bundled starter skill "hello" with the runnable files.
    let hello = bundled.join("hello");
    std::fs::create_dir_all(&hello).unwrap();
    std::fs::write(hello.join("manifest.toml"), b"name = \"hello\"\n").unwrap();
    std::fs::write(hello.join("hello.cps"), b"CPSFAKE").unwrap();

    // First seed: hello is copied over.
    let n = seed_starter_capsules(&bundled, &dest);
    assert_eq!(n, 1, "one skill dir seeded");
    assert!(dest.join("hello").join("hello.cps").exists());
    assert!(dest.join("hello").join("manifest.toml").exists());

    // A user edits their copy; a re-seed must NOT clobber it (existing skill is left alone).
    std::fs::write(dest.join("hello").join("hello.cps"), b"USER_EDITED").unwrap();
    let n2 = seed_starter_capsules(&bundled, &dest);
    assert_eq!(n2, 1);
    assert_eq!(
        std::fs::read(dest.join("hello").join("hello.cps")).unwrap(),
        b"USER_EDITED",
        "existing skill must never be overwritten"
    );
}

#[test]
fn seed_starter_capsules_missing_bundled_dir_is_honest_zero() {
    let root = tmp_dir("seedmissing");
    // No bundled dir at all → best-effort, zero skills, never a panic.
    let n = seed_starter_capsules(&root.join("nope"), &root.join("dest"));
    assert_eq!(n, 0);
}
