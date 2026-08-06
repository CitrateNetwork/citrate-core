// CORE-C1.2 — node-agent sidecar + signature-request bridge suite (@rule8 evidence).
//
// Red-first. Splits into:
//   * WP0 bundling — asserted by the (out-of-source) tauri overlay + gitignore;
//     here we prove the binary resolution + spawn under the supervisor via a
//     STUB node-agent (a small shell helper that serves the grounded supervision
//     API on a loopback port and blocks until killed).
//   * WP1 @rule8 bearer — mint (OsRng, 64 hex, distinct), persist 0600/0700,
//     round-trip against a REAL loopback stub server, and the 401 NEGATIVE
//     CONTROL (wrong/absent bearer → 401). Plus the bearer-never-in-logs/Debug
//     tripwire.
//   * WP2 @rule8 bridge — decode the grounded wire shape, build a
//     `SignatureIntent{origin:"agent:node-agent"}`, route request→approve→sign→
//     broadcast through the ceremony (mocked RPC, reusing B1.4), report observed,
//     and the ADV-7 no-direct-sign proof (bridging returns a ceremony, NEVER a
//     signature; the gated signer stays reachable only via ceremony approval).
//
// CI-safe + deterministic: the bridge/ceremony path uses a mock RpcTransport (no
// port races); the supervision round-trip uses a single-connection loopback stub
// on an ephemeral port. The REAL node-agent spawn is a heavy `#[ignore]` live
// proof at the bottom (build minutes) with the exact commands.

use super::*;
use crate::custody::{CustodyError, CustodyVault, Keyring};
use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

// ---------------------------------------------------------------------------
// Vault + wallet fixture (mirrors ceremony_tests: canonical BIP44 vector).
// ---------------------------------------------------------------------------

const CANONICAL_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const CANONICAL_ADDRESS_LOWER: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
const PASS: &[u8] = b"correct horse battery staple";

#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<std::collections::HashMap<String, Vec<u8>>>,
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

/// A fresh vault holding the canonical wallet, so the ceremony can actually sign.
fn vault_with_wallet() -> (CustodyVault, PathBuf) {
    let mut p = std::env::temp_dir();
    let uniq = format!("citrate-core-agent-test-{}-{}.enc", std::process::id(), {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    });
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    crate::wallet::import(&v, CANONICAL_MNEMONIC).expect("import canonical wallet");
    (v, p)
}

/// A unique temp dir for a test's token file / crash records.
fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!(
        "citrate-core-c1_2-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    p
}

// ---------------------------------------------------------------------------
// A scripted mock RPC transport (mirrors ceremony_tests) for the bridge path.
// ---------------------------------------------------------------------------

struct MockRpc {
    requests: RefCell<Vec<JsonValue>>,
    responses: RefCell<VecDeque<JsonValue>>,
}
impl MockRpc {
    fn new(responses: Vec<JsonValue>) -> Self {
        MockRpc {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }
}
impl RpcTransport for MockRpc {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.requests.borrow_mut().push(body.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}
fn ok(result: JsonValue) -> JsonValue {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}
fn bcfg(attempts: u32) -> BroadcastConfig {
    BroadcastConfig {
        chain_id: 40204,
        poll_attempts: attempts,
        poll_interval: std::time::Duration::from_millis(1),
    }
}

// ---------------------------------------------------------------------------
// A mock SupervisionTransport for the bridge-logic tests (no port).
// ---------------------------------------------------------------------------

/// A scripted supervision transport: records (method, path, bearer) and replies
/// with a queued (status, body). Lets the bridge tests assert the bearer was
/// presented AND drive the fetch → observe flow without a real socket. Backed by
/// a `Mutex` so it satisfies the `SupervisionTransport: Send + Sync` bound.
struct MockSupervisionSync(StdMutex<MockSupervisionInner>);
struct MockSupervisionInner {
    calls: Vec<(String, String, String)>,
    responses: VecDeque<SupervisionResponse>,
}
impl MockSupervisionSync {
    fn new(responses: Vec<SupervisionResponse>) -> Self {
        MockSupervisionSync(StdMutex::new(MockSupervisionInner {
            calls: Vec::new(),
            responses: responses.into_iter().collect(),
        }))
    }
    fn resp(status: u16, body: &str) -> SupervisionResponse {
        SupervisionResponse {
            status,
            body: body.to_string(),
        }
    }
    fn calls(&self) -> Vec<(String, String, String)> {
        self.0.lock().unwrap().calls.clone()
    }
}
impl SupervisionTransport for MockSupervisionSync {
    fn get(&self, _base: &str, path: &str, bearer: &str) -> Result<SupervisionResponse, AgentError> {
        let mut g = self.0.lock().unwrap();
        g.calls
            .push(("GET".into(), path.to_string(), bearer.to_string()));
        g.responses
            .pop_front()
            .ok_or_else(|| AgentError::Transport("mock: no scripted response".into()))
    }
    fn post_json(
        &self,
        _base: &str,
        path: &str,
        bearer: &str,
        _body: &str,
    ) -> Result<SupervisionResponse, AgentError> {
        let mut g = self.0.lock().unwrap();
        g.calls
            .push(("POST".into(), path.to_string(), bearer.to_string()));
        g.responses
            .pop_front()
            .ok_or_else(|| AgentError::Transport("mock: no scripted response".into()))
    }
}

/// A shareable adapter so a test can hold the mock (to read its call log) while
/// the manager also holds it (to drive requests). Both point at one `Arc`.
struct SharedMockSupervision(std::sync::Arc<MockSupervisionSync>);
impl SupervisionTransport for SharedMockSupervision {
    fn get(&self, base: &str, path: &str, bearer: &str) -> Result<SupervisionResponse, AgentError> {
        self.0.get(base, path, bearer)
    }
    fn post_json(
        &self,
        base: &str,
        path: &str,
        bearer: &str,
        body: &str,
    ) -> Result<SupervisionResponse, AgentError> {
        self.0.post_json(base, path, bearer, body)
    }
}

const TEST_BEARER: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

/// Build a manager wired to a mock supervision transport + a fake session bearer
/// (so the bearer-gated supervision calls have a token WITHOUT spawning a child).
fn mock_manager(transport: Box<dyn SupervisionTransport>) -> AgentManager {
    let dir = tmp_dir("mock");
    let mgr = AgentManager::new(
        PathBuf::from("/nonexistent/node-agent-xyz"),
        dir.join("compute.json"),
        dir.join("supervision.token"),
        dir.join("crash.jsonl"),
        "http://127.0.0.1:19600",
        "127.0.0.1:19600",
        transport,
    );
    // Inject a session bearer directly (as if `start` minted one) so the
    // supervision calls are exercised without a live child.
    mgr.test_set_bearer(TEST_BEARER);
    mgr
}

/// Build a manager + return the shared mock so the test can read its call log.
fn mock_manager_shared(
    responses: Vec<SupervisionResponse>,
) -> (AgentManager, std::sync::Arc<MockSupervisionSync>) {
    let shared = std::sync::Arc::new(MockSupervisionSync::new(responses));
    let mgr = mock_manager(Box::new(SharedMockSupervision(shared.clone())));
    (mgr, shared)
}

/// A canonical claimRewards request as the node-agent would serve it (grounded
/// state.rs PendingSignatureRequest / earnings.rs ClaimRewards).
fn claim_rewards_request() -> AgentSignatureRequest {
    AgentSignatureRequest {
        id: 3,
        intent: "claimRewards".into(),
        to: "0x1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a".into(),
        // 4-byte selector only (claimRewards() takes no args).
        calldata: "0x372500ab".into(),
        value_wei: "0".into(),
        chain_id: 40204,
        context: "claimRewards (5000000000000000000 wei claimable)".into(),
        expires_block: "0".into(),
        status: "pending".into(),
        tx_hash: None,
    }
}

// ===========================================================================
// WP1 @rule8 — bearer minting + persistence + zeroize/perms
// ===========================================================================

/// The session bearer is 64 lowercase hex chars (256-bit) — matches node-agent
/// `SupervisionAuth::generate`, so the child accepts it verbatim.
#[test]
fn bearer_is_64_lowercase_hex() {
    let tok = super::mint_bearer();
    assert_eq!(tok.len(), 64, "256-bit token → 64 hex chars");
    assert!(tok.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

/// Two minted tokens differ (real CSPRNG, not a fixed/derived value).
#[test]
fn minted_bearers_differ() {
    assert_ne!(*super::mint_bearer(), *super::mint_bearer());
}

/// The token file is written 0600 and its parent dir 0700 (the grounded IPC
/// channel hardening the node-agent's auth.rs also enforces). @rule8: the token's
/// only protection is filesystem perms.
#[cfg(unix)]
#[test]
fn token_file_is_0600_parent_0700() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("perms");
    let path = dir.join("supervision.token");
    super::test_persist_bearer(&path, "abc123").expect("persist");
    let fmode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let dmode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(fmode, 0o600, "token file must be 0600");
    assert_eq!(dmode, 0o700, "token dir must be 0700");
    // And re-persisting repairs loosened perms.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    super::test_persist_bearer(&path, "abc123").expect("re-persist");
    let fmode2 = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(fmode2, 0o600, "loosened token file re-hardened to 0600");
    let _ = std::fs::remove_dir_all(&dir);
}

/// BEARER-NEVER-IN-LOGS TRIPWIRE (@rule8): no error variant, no Display, and no
/// status/Debug surface carries the token. We scan the module SOURCE to prove no
/// error/log formats the bearer, and assert the transport errors are token-free.
#[test]
fn bearer_never_in_errors_or_debug() {
    // The AgentError Display for a 401 carries only the PUBLIC status code.
    let e = AgentError::Status(401);
    assert_eq!(e.to_string(), "agent supervision status 401");
    assert!(!e.to_string().contains("Bearer") && !e.to_string().contains("token value"));
    // The transport error never echoes a token.
    let t = AgentError::Transport("connection refused".into());
    assert!(!t.to_string().to_lowercase().contains("bearer"));
    // AgentStatus (the bridge surface) has no token field at all.
    let s = AgentStatus {
        state: "running".into(),
        authed: true,
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("bearer") && !json.contains("token"), "status json: {json}");

    // SOURCE TRIPWIRE: the module must NOT LOG the bearer. The ONLY sanctioned use
    // of the token value is building the `Authorization: Bearer <token>` HTTP
    // header (which sends it to the loopback server — that is the whole point).
    // Logging it (println/eprintln) or panicking with it is forbidden. We scan for
    // a log macro that interpolates the bearer/token binding.
    let src = include_str!("agent.rs");
    let non_test = strip_agent_test_module(src);
    for line in non_test.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let is_log = t.contains("println!")
            || t.contains("eprintln!")
            || t.contains("panic!")
            || t.contains("dbg!");
        let mentions_secret =
            t.contains("{bearer}") || t.contains("{tok}") || t.contains("{token}");
        assert!(
            !(is_log && mentions_secret),
            "bearer must never be logged: `{}`",
            t.trim()
        );
    }
    // And the ONLY CODE interpolation of the bearer is the Authorization header
    // (a defensive upper bound: exactly two header-build sites — GET + POST;
    // doc-comment mentions of the header form are excluded).
    let header_uses = non_test
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("///") && t.contains("Bearer {bearer}")
        })
        .count();
    assert_eq!(
        header_uses, 2,
        "the bearer value may ONLY be used to build the Authorization header (GET + POST)"
    );
}

/// Truncate the agent.rs source at its test module so a source scan sees only
/// non-test code (mirrors ceremony_tests::strip_test_module).
fn strip_agent_test_module(src: &str) -> String {
    match src.find("#[cfg(test)]") {
        Some(i) => src[..i].to_string(),
        None => src.to_string(),
    }
}

// ===========================================================================
// WP1 @rule8 — supervision round-trip + 401 NEGATIVE CONTROL (real loopback stub)
// ===========================================================================

/// A one-shot loopback HTTP stub answering the node-agent supervision contract:
/// bearer-gates every path (401 on missing/wrong), and on the right bearer serves
/// `/status` → "idle". Serves exactly `n` connections then exits, so a test can
/// drive the authed + unauthed legs deterministically. Returns (base_url, token).
fn spawn_supervision_stub(n: usize) -> (String, String, std::thread::JoinHandle<()>) {
    let token = "cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe".to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let tok = token.clone();
    let handle = std::thread::spawn(move || {
        for _ in 0..n {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 2048];
            let read = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..read]);
            // The request line + headers; find the Authorization header.
            let authed = req
                .lines()
                .find_map(|l| {
                    let l = l.trim();
                    l.strip_prefix("Authorization: Bearer ")
                        .or_else(|| l.strip_prefix("authorization: Bearer "))
                })
                .map(|presented| presented.trim() == tok)
                .unwrap_or(false);
            let (status_line, body) = if authed {
                ("HTTP/1.1 200 OK", "\"idle\"".to_string())
            } else {
                ("HTTP/1.1 401 Unauthorized", "\"missing or invalid supervision token\"".to_string())
            };
            let resp = format!(
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    (base, token, handle)
}

/// WP1 acceptance: the REAL ureq transport presents the bearer and gets a 200 +
/// the status string back (the supervision handshake round-trip).
#[test]
fn ureq_transport_bearer_round_trip_200() {
    let (base, token, handle) = spawn_supervision_stub(1);
    let t = UreqSupervisionTransport;
    let resp = t.get(&base, "/status", &token).expect("transport get");
    assert_eq!(resp.status, 200, "correct bearer → 200");
    assert_eq!(resp.body.trim(), "\"idle\"");
    handle.join().ok();
}

/// WP1 @rule8 NEGATIVE CONTROL: a WRONG bearer is rejected 401 (not a transport
/// error, not a silent pass). This is the core of the bearer gate — proven
/// against the real loopback surface, not just a mock.
#[test]
fn ureq_transport_wrong_bearer_is_401() {
    let (base, _token, handle) = spawn_supervision_stub(1);
    let t = UreqSupervisionTransport;
    let resp = t
        .get(&base, "/status", "not-the-right-token-000000000000000000000000000000")
        .expect("transport get (status observed, not error)");
    assert_eq!(resp.status, 401, "wrong bearer → 401");
    handle.join().ok();
}

/// WP1 @rule8 NEGATIVE CONTROL: an ABSENT bearer is rejected 401. We hit the stub
/// with a bare ureq GET (no Authorization header) and assert 401.
#[test]
fn absent_bearer_is_401() {
    let (base, _token, handle) = spawn_supervision_stub(1);
    // A bare request with NO Authorization header.
    let resp = ureq::get(&format!("{base}/status"))
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .expect("call");
    assert_eq!(resp.status().as_u16(), 401, "absent bearer → 401");
    handle.join().ok();
}

/// The manager's `supervision_status` presents the SESSION bearer and decodes the
/// status string (end-to-end through the manager + a mock transport).
#[test]
fn manager_supervision_status_presents_bearer() {
    let mock = Box::new(MockSupervisionSync::new(vec![MockSupervisionSync::resp(200, "\"idle\"")]));
    let mgr = mock_manager(mock);
    let status = mgr.supervision_status().expect("status");
    assert_eq!(status, "idle");
}

/// Without a session bearer (agent not started), every supervision call fails
/// closed — a caller can never reach the surface unauthenticated.
#[test]
fn no_session_bearer_fails_closed() {
    let dir = tmp_dir("nosession");
    let mgr = AgentManager::new(
        PathBuf::from("/nonexistent/node-agent"),
        dir.join("compute.json"),
        dir.join("supervision.token"),
        dir.join("crash.jsonl"),
        "http://127.0.0.1:19600",
        "127.0.0.1:19600",
        Box::new(MockSupervisionSync::new(vec![])),
    );
    // No bearer injected/minted → the supervision call is refused before any HTTP.
    let r = mgr.supervision_status();
    assert!(matches!(r, Err(AgentError::Transport(_))), "got: {r:?}");
}

// ===========================================================================
// WP2 @rule8 — wire-shape decode + intent build
// ===========================================================================

/// The grounded PendingSignatureRequest JSON decodes field-for-field (decimal
/// strings for value_wei/expires_block; optional tx_hash).
#[test]
fn signature_request_wire_shape_decodes() {
    let wire = r#"[{"id":3,"intent":"claimRewards","to":"0x1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a","calldata":"0x372500ab","value_wei":"0","chain_id":40204,"context":"claimRewards (5000000000000000000 wei claimable)","expires_block":"0","status":"pending"}]"#;
    let reqs: Vec<AgentSignatureRequest> = serde_json::from_str(wire).expect("decode wire shape");
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.id, 3);
    assert_eq!(r.intent, "claimRewards");
    assert_eq!(r.value_wei, "0");
    assert_eq!(r.chain_id, 40204);
    assert!(r.is_pending());
    assert_eq!(r.tx_hash, None);
}

/// `intent_from_request` stamps origin "agent:node-agent", a Transaction intent
/// bound to the request chain, and encodes {from,to,value,data,gas} for the B1.4
/// decoder. The origin is the ADV-5 verbatim-display label.
#[test]
fn intent_carries_agent_origin_and_tx_fields() {
    let req = claim_rewards_request();
    let intent = intent_from_request(&req, CANONICAL_ADDRESS_LOWER, Some(0x8000));
    assert_eq!(intent.origin, "agent:node-agent");
    assert_eq!(intent.origin, AGENT_ORIGIN);
    assert_eq!(intent.kind, IntentKind::Transaction);
    assert_eq!(intent.chain_id, 40204);
    // The raw payload is a JSON tx object the B1.4 decoder consumes.
    let v: JsonValue = serde_json::from_str(&intent.raw).unwrap();
    assert_eq!(v["from"], CANONICAL_ADDRESS_LOWER);
    assert_eq!(v["to"], req.to);
    assert_eq!(v["data"], req.calldata);
    assert_eq!(v["value"], "0x0");
    assert_eq!(v["gas"], "0x8000");
    // And txdecode actually parses it to a legible contract call (not raw-gated).
    let (_parsed, display) = crate::txdecode::decode_transaction(&intent.raw)
        .expect("bridged intent decodes to a legible tx");
    assert!(display.action.contains("Call"), "action: {}", display.action);
}

// ===========================================================================
// WP2 @rule8 — the full bridge: fetch → ceremony request → approve → broadcast
// ===========================================================================

/// END-TO-END (the C1.2 acceptance): an unsigned node-agent request is fetched
/// (bearer-authed), routed into the ceremony, approved by the HUMAN, signed +
/// broadcast via the B1.4 path (mocked RPC), and the tx hash reported back —
/// proving a node-agent write reaches the chain ONLY through a human ceremony
/// approval. No fabricated tx: the RPC is a scripted mock (Rule 1: a TEST
/// transport).
#[test]
fn bridge_request_to_ceremony_to_broadcast_end_to_end() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();

    // The node-agent serves one pending claimRewards; then accepts the observe.
    let (mgr, shared) = mock_manager_shared(vec![
        MockSupervisionSync::resp(
            200,
            &serde_json::to_string(&vec![claim_rewards_request()]).unwrap(),
        ),
        MockSupervisionSync::resp(200, "\"observed\""), // POST /observed
    ]);

    // Script the RPC: estimateGas (bridge) → nonce → gasPrice → sendRaw → receipt.
    let tx_hash = "0xabc0000000000000000000000000000000000000000000000000000000000abc";
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x8000".into())),      // eth_estimateGas
        ok(JsonValue::String("0x7".into())),         // eth_getTransactionCount
        ok(JsonValue::String("0x77359400".into())),  // eth_gasPrice (2e9)
        ok(JsonValue::String(tx_hash.into())),       // eth_sendRawTransaction
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]));

    // Step 1-4: bridge the pending request into a PENDING ceremony (NO signature).
    let (id, req) = mgr
        .bridge_one_pending(&ceremony, &v, &rpc)
        .expect("bridge one pending request");
    assert_eq!(req.intent, "claimRewards");
    // The ceremony is PENDING and displays the agent origin verbatim — no key
    // was touched, no signature produced by bridging.
    let view = ceremony.status(&id).expect("ceremony pending");
    assert_eq!(view.origin, "agent:node-agent");
    assert!(!view.requires_raw_ack, "a legible contract call is not raw-gated");

    // Step 5: the HUMAN approves → sign + broadcast (B1.4) → report observed.
    let result = mgr
        .approve_bridged_and_report(&ceremony, &v, &rpc, &id, false, &req, bcfg(2))
        .expect("human approves → signed broadcast");
    assert_eq!(result.tx_hash, tx_hash, "the node-accepted hash is returned");
    assert_eq!(result.block_number, Some(100), "0x64 → block 100 inclusion");

    // The observe callback presented the SESSION bearer (POST /observed authed).
    let calls = shared.0.lock().unwrap().calls.clone();
    assert!(
        calls.iter().any(|(m, p, b)| m == "POST"
            && p == "/signature-requests/3/observed"
            && b == TEST_BEARER),
        "observe must POST with the session bearer: {calls:?}"
    );
}

// ===========================================================================
// WP2 @rule8 — C1.2-F-1 dedup: one request → one ceremony → one broadcast
// ===========================================================================

/// C1.2-F-1 (the RED test / negative control): a still-`pending` node-agent
/// request bridged TWICE must map to the SAME ceremony — never two. Without the
/// dedup, two bridges mint two ceremony ids, which could each be approved into a
/// SECOND broadcast (a double-claim). The invariant: one request id → one
/// ceremony id.
#[test]
fn duplicate_bridge_of_same_request_reuses_one_ceremony() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    // The node-agent serves the SAME pending claimRewards on both polls (it keeps
    // re-emitting a pending request until it is observed).
    let one = serde_json::to_string(&vec![claim_rewards_request()]).unwrap();
    let mgr = mock_manager(Box::new(MockSupervisionSync::new(vec![
        MockSupervisionSync::resp(200, &one),
        MockSupervisionSync::resp(200, &one),
    ])));
    // estimateGas is called once per NEW ceremony; script two just in case, so a
    // buggy second-ceremony path would still run (and then be caught by the id
    // assertion) rather than erroring on a missing mock.
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x8000".into())),
        ok(JsonValue::String("0x8000".into())),
    ]));

    let (id1, _r1) = mgr.bridge_one_pending(&ceremony, &v, &rpc).expect("bridge 1");
    let (id2, _r2) = mgr.bridge_one_pending(&ceremony, &v, &rpc).expect("bridge 2");
    assert_eq!(id1, id2, "same pending request must map to ONE ceremony (C1.2-F-1)");
    // And the second bridge did NOT consume a second gas estimate (it reused the
    // existing ceremony), so exactly one estimateGas hit the RPC.
    let estimate_calls = rpc
        .transport()
        .requests
        .borrow()
        .iter()
        .filter(|r| r["method"] == "eth_estimateGas")
        .count();
    assert_eq!(estimate_calls, 1, "the reused bridge does not re-estimate gas");
}

/// C1.2-F-1 end-to-end: two bridges of one still-pending request, then approve —
/// exactly ONE broadcast reaches the chain, and the second ceremony id (== the
/// first) is already consumed so it cannot be approved again (`UnknownCeremony`).
#[test]
fn dedup_yields_exactly_one_broadcast_for_one_request() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let one = serde_json::to_string(&vec![claim_rewards_request()]).unwrap();
    let (mgr, _shared) = mock_manager_shared(vec![
        MockSupervisionSync::resp(200, &one),
        MockSupervisionSync::resp(200, &one),
        MockSupervisionSync::resp(200, "\"observed\""), // POST /observed after the ONE broadcast
    ]);
    let tx_hash = "0xabc0000000000000000000000000000000000000000000000000000000000abc";
    // Only ONE broadcast's worth of RPC is scripted: estimateGas → nonce →
    // gasPrice → sendRaw → receipt. A second broadcast would run out of scripted
    // responses and error — the test proves there IS no second broadcast.
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x8000".into())),      // eth_estimateGas (bridge 1)
        ok(JsonValue::String("0x7".into())),         // eth_getTransactionCount
        ok(JsonValue::String("0x77359400".into())),  // eth_gasPrice
        ok(JsonValue::String(tx_hash.into())),       // eth_sendRawTransaction
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]));

    let (id1, req) = mgr.bridge_one_pending(&ceremony, &v, &rpc).expect("bridge 1");
    let (id2, _req2) = mgr.bridge_one_pending(&ceremony, &v, &rpc).expect("bridge 2 (dup)");
    assert_eq!(id1, id2, "dedup: one ceremony");

    // Approve once → one broadcast.
    let result = mgr
        .approve_bridged_and_report(&ceremony, &v, &rpc, &id1, false, &req, bcfg(2))
        .expect("one approval → one broadcast");
    assert_eq!(result.tx_hash, tx_hash);

    // The (identical) second id is already consumed — a second approval finds
    // nothing → UnknownCeremony (no double-broadcast, single-use holds).
    let second = mgr.approve_bridged_and_report(&ceremony, &v, &rpc, &id2, false, &req, bcfg(2));
    assert!(
        matches!(second, Err(AgentError::Ceremony(_))),
        "the consumed ceremony cannot broadcast again: {second:?}"
    );
}

/// After a bridged request is approved+broadcast, its dedup entry is cleared, so
/// a node-agent that RE-EMITS the same request id later (a legitimate new
/// accrual) can bridge a FRESH ceremony (dedup does not permanently pin an id).
#[test]
fn dedup_entry_clears_after_broadcast_so_reaccrual_bridges_fresh() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let one = serde_json::to_string(&vec![claim_rewards_request()]).unwrap();
    let (mgr, _shared) = mock_manager_shared(vec![
        MockSupervisionSync::resp(200, &one), // poll 1
        MockSupervisionSync::resp(200, "\"observed\""), // observe after broadcast 1
        MockSupervisionSync::resp(200, &one), // poll 2 (re-emitted, same id)
    ]);
    let tx_hash = "0xabc0000000000000000000000000000000000000000000000000000000000abc";
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x8000".into())),      // estimateGas (bridge 1)
        ok(JsonValue::String("0x7".into())),         // nonce
        ok(JsonValue::String("0x77359400".into())),  // gasPrice
        ok(JsonValue::String(tx_hash.into())),       // sendRaw
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
        ok(JsonValue::String("0x8000".into())),      // estimateGas (bridge 2, fresh)
    ]));

    let (id1, req) = mgr.bridge_one_pending(&ceremony, &v, &rpc).expect("bridge 1");
    mgr.approve_bridged_and_report(&ceremony, &v, &rpc, &id1, false, &req, bcfg(2))
        .expect("broadcast 1");
    // The re-emitted request bridges a NEW ceremony (entry was cleared).
    let (id2, _req2) = mgr.bridge_one_pending(&ceremony, &v, &rpc).expect("bridge 2 fresh");
    assert_ne!(id1, id2, "a re-accrual after broadcast bridges a fresh ceremony");
}

// ---------------------------------------------------------------------------
// C2-F-2 — a THREAD-SAFE RPC transport for the concurrency test. The module's
// primary `MockRpc` is RefCell-backed (single-thread); this one is Mutex-backed
// and carries a Barrier so two threads can be forced to reach the mint window
// simultaneously (each thread still holds its OWN RpcClient — RpcClient is not
// Sync because of its Cell<u64> request counter, so it can never be shared).
// ---------------------------------------------------------------------------

struct SyncMockRpc {
    responses: StdMutex<VecDeque<JsonValue>>,
    /// Tripped inside `call` (which runs AFTER the dedup fast-path check) so both
    /// bridging threads are guaranteed past the fast-path check before EITHER
    /// mints — this is what makes the non-atomic (pre-fix) race observable.
    gate: std::sync::Arc<std::sync::Barrier>,
}
impl SyncMockRpc {
    fn new(responses: Vec<JsonValue>, gate: std::sync::Arc<std::sync::Barrier>) -> Self {
        SyncMockRpc {
            responses: StdMutex::new(responses.into_iter().collect()),
            gate,
        }
    }
}
impl RpcTransport for SyncMockRpc {
    fn call(&self, _body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        // Rendezvous: both threads must arrive here (post fast-path check) before
        // either returns a gas estimate + proceeds to the mint. Wait ONCE.
        self.gate.wait();
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("sync mock: no scripted response".into()))
    }
}

/// C2-F-2 (the RED test / concurrency negative control): two threads bridge the
/// SAME still-pending node-agent request CONCURRENTLY. The check-and-mint must be
/// ATOMIC — exactly ONE ceremony may exist for the one request id. Before the fix
/// the check (lock released) and the insert (lock re-acquired) straddled the mint,
/// so both threads saw an empty slot, both minted a DISTINCT ceremony, and BOTH
/// were independently approvable → a double-claim of one request. This test forces
/// that interleaving with a Barrier tripped inside the RPC (after the fast-path
/// check) and asserts a single ceremony. It goes RED against the non-atomic code
/// (two distinct approvable ceremonies) and GREEN after (one shared ceremony).
#[test]
fn concurrent_bridge_of_same_request_mints_exactly_one_ceremony() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    // The node-agent serves the SAME pending claimRewards on every poll (both
    // threads fetch it). One response per thread's fetch.
    let one = serde_json::to_string(&vec![claim_rewards_request()]).unwrap();
    let mgr = mock_manager(Box::new(MockSupervisionSync::new(vec![
        MockSupervisionSync::resp(200, &one),
        MockSupervisionSync::resp(200, &one),
    ])));

    // A 2-thread barrier tripped in each thread's estimate_gas call, so both are
    // past the fast-path dedup check before either mints (maximally adversarial to
    // the pre-fix non-atomic window).
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));

    let (id_a, id_b) = std::thread::scope(|scope| {
        let mk_client = || {
            RpcClient::with_transport(SyncMockRpc::new(
                vec![ok(JsonValue::String("0x8000".into()))], // one eth_estimateGas per thread
                gate.clone(),
            ))
        };
        let mgr_ref = &mgr;
        let cer_ref = &ceremony;
        let v_ref = &v;
        let ca = mk_client();
        let cb = mk_client();
        let ha = scope.spawn(move || {
            mgr_ref
                .bridge_one_pending(cer_ref, v_ref, &ca)
                .expect("bridge A")
                .0
        });
        let hb = scope.spawn(move || {
            mgr_ref
                .bridge_one_pending(cer_ref, v_ref, &cb)
                .expect("bridge B")
                .0
        });
        (ha.join().expect("join A"), hb.join().expect("join B"))
    });

    // The atomicity invariant: both concurrent bridges resolve to the SAME
    // ceremony id (one request → one ceremony). Pre-fix, the two threads mint
    // DISTINCT ids and this fails.
    assert_eq!(
        id_a, id_b,
        "concurrent bridges of one request must yield ONE ceremony (C2-F-2)"
    );

    // And there is exactly ONE approvable ceremony. Ceremony ids are minted
    // sequentially from 1; if the race had minted two, "1" AND "2" would both be
    // pending (two independently approvable double-claim ceremonies). Assert the
    // shared id is pending and that no SECOND distinct ceremony exists.
    assert!(
        ceremony.status(&id_a).is_some(),
        "the one ceremony is pending"
    );
    let other = if id_a == "1" { "2" } else { "1" };
    assert!(
        ceremony.status(other).is_none(),
        "no second ceremony was minted for the same request (C2-F-2): {other} is present"
    );
}

// ===========================================================================
// C2-F-1 — the USER Claim path bridges a REAL ceremony (never a sim mutation)
// ===========================================================================

/// C2-F-1: `bridge_user_claim` turns the user's REAL claimable into a PENDING
/// ceremony carrying the legible `claimRewards()` call — signing NOTHING. This is
/// the real path the Claim button drives: a ceremony the human must approve via
/// sign_and_broadcast (B1.4), NOT a local balance mutation with a fabricated hash.
#[test]
fn user_claim_bridges_a_real_pending_ceremony() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    // No supervision fetch on the user path — the request is built locally.
    let mgr = mock_manager(Box::new(MockSupervisionSync::new(vec![])));
    let rpc = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String("0x8000".into()))]));

    let claimable = 5 * 10u128.pow(18); // 5 SALT
    let (id, req) = mgr
        .bridge_user_claim(&ceremony, &v, &rpc, claimable, crate::earnings::CONTRIBUTION_ACCOUNTING)
        .expect("user claim bridges");
    // The bridged request is the user-claim id space (C2-F-3) + real claimRewards().
    assert_eq!(req.id, crate::earnings::USER_CLAIM_ID);
    assert_eq!(req.intent, "claimRewards");
    // A PENDING ceremony exists — nothing was signed by bridging.
    let view = ceremony.status(&id).expect("ceremony pending");
    assert_eq!(view.origin, crate::agent::AGENT_ORIGIN);
    assert!(id.parse::<u64>().is_ok(), "a ceremony id, not a signature");
}

/// C2-F-1 (honest zero): a caller with NOTHING to claim gets an honest
/// `NoPending` — no ceremony, no tx, and (crucially) no fabricated "claimed"
/// settlement. Rule 1: 0 claimable is a real state, surfaced honestly.
#[test]
fn user_claim_with_zero_claimable_is_honest_nothing_to_claim() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let mgr = mock_manager(Box::new(MockSupervisionSync::new(vec![])));
    // No RPC should be consumed — the zero check short-circuits before gas estimate.
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));

    let r = mgr.bridge_user_claim(&ceremony, &v, &rpc, 0, crate::earnings::CONTRIBUTION_ACCOUNTING);
    assert!(
        matches!(r, Err(AgentError::NoPending)),
        "zero claimable → honest NoPending, never a faked claim: {r:?}"
    );
    // And no ceremony was minted (the ids start at 1; none is pending).
    assert!(ceremony.status("1").is_none(), "no ceremony minted for a 0 claim");
}

// ===========================================================================
// C2-F-3 — user-claim id and a node-agent req id do NOT alias in the dedup map
// ===========================================================================

/// C2-F-3: a USER claim (disjoint `USER_CLAIM_ID`) and a node-agent request with
/// `id == 0` must NOT alias in the shared dedup map — they bridge to DISTINCT,
/// independently-pending ceremonies. Before the fix the user claim hardcoded
/// `id: 0`, colliding with a node-agent `id: 0`: one intent's ceremony would be
/// suppressed (or reused) by the other. This proves the two id spaces are disjoint.
#[test]
fn user_claim_and_node_agent_id_zero_do_not_alias() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();

    // A node-agent request that happens to carry id 0 (the value the user claim
    // used to hardcode). Served on the supervision surface for bridge_one_pending.
    let mut na_req = claim_rewards_request();
    na_req.id = 0;
    let one = serde_json::to_string(&vec![na_req]).unwrap();
    let mgr = mock_manager(Box::new(MockSupervisionSync::new(vec![
        MockSupervisionSync::resp(200, &one),
    ])));

    // One gas estimate per DISTINCT bridged request (two here → two estimates).
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x8000".into())), // user claim estimate
        ok(JsonValue::String("0x8000".into())), // node-agent (id 0) estimate
    ]));

    // Bridge the USER claim (USER_CLAIM_ID) and the node-agent request (id 0).
    let (user_id, user_req) = mgr
        .bridge_user_claim(&ceremony, &v, &rpc, 5 * 10u128.pow(18), crate::earnings::CONTRIBUTION_ACCOUNTING)
        .expect("user claim bridges");
    let (na_id, na_req) = mgr
        .bridge_one_pending(&ceremony, &v, &rpc)
        .expect("node-agent id 0 bridges");

    // The two requests occupy DISJOINT id spaces...
    assert_eq!(user_req.id, crate::earnings::USER_CLAIM_ID);
    assert_eq!(na_req.id, 0);
    assert_ne!(user_req.id, na_req.id, "user-claim id must not equal node-agent id 0");
    // ...and therefore bridge to TWO DISTINCT, independently-pending ceremonies —
    // no aliasing / suppression in the shared dedup map (C2-F-3).
    assert_ne!(
        user_id, na_id,
        "distinct id spaces must mint distinct ceremonies (C2-F-3)"
    );
    assert!(ceremony.status(&user_id).is_some(), "user claim ceremony pending");
    assert!(ceremony.status(&na_id).is_some(), "node-agent ceremony pending");
}

/// ADV-7 no-direct-sign (behavioural): bridging a request returns a CEREMONY id,
/// NEVER a signature. The node-agent origin has no signing shortcut — a signature
/// exists only after an explicit ceremony approval.
#[test]
fn bridge_never_returns_a_signature() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let sup = Box::new(MockSupervisionSync::new(vec![MockSupervisionSync::resp(
        200,
        &serde_json::to_string(&vec![claim_rewards_request()]).unwrap(),
    )]));
    let mgr = mock_manager(sup);
    let rpc = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String("0x8000".into()))]));

    let (id, _req) = mgr.bridge_one_pending(&ceremony, &v, &rpc).expect("bridge");
    // The return is a ceremony id (a decimal string), not a signature. The
    // ceremony is still PENDING (one unconsumed) — nothing signed.
    assert!(id.parse::<u64>().is_ok(), "bridge returns a ceremony id, not a signature");
    assert!(ceremony.status(&id).is_some(), "ceremony is pending, unsigned");
}

/// ADV-7 no-direct-sign (STRUCTURAL, extends the B1.2/B1.4 source-scan for the
/// agent origin): the agent module must NEVER invoke the gated signer directly —
/// it only reaches signatures via the ceremony's approve/approve_and_broadcast.
/// NEGATIVE CONTROL (stated): add `wallet::sign_message(` or
/// `wallet::sign_transaction(` to agent.rs and this fails (that is the "sidecar
/// signs directly" attack ADV-7 forbids).
#[test]
fn adv7_agent_module_never_calls_the_gated_signer() {
    let src = include_str!("agent.rs");
    let non_test = strip_agent_test_module(src);
    let forbidden = [
        "sign_".to_string() + "message(",
        "sign_".to_string() + "transaction(",
    ];
    for line in non_test.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        for call in &forbidden {
            assert!(
                !t.contains(call.as_str()),
                "agent.rs must not invoke the gated signer directly: `{}`",
                t.trim()
            );
        }
    }
    // POSITIVE: the agent reaches signatures ONLY through the ceremony's approve
    // path (approve_and_broadcast), never a raw signer.
    assert!(
        non_test.contains("approve_and_broadcast("),
        "the agent bridge must route through the ceremony approve path"
    );
}

/// The bridge is honest when there is nothing to sign: no pending request →
/// `NoPending` (never a fabricated ceremony).
#[test]
fn bridge_with_no_pending_is_honest() {
    let (v, _p) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    // Only a SUBMITTED entry (already broadcast) — nothing pending.
    let submitted = AgentSignatureRequest {
        status: "submitted".into(),
        tx_hash: Some("0xdead".into()),
        ..claim_rewards_request()
    };
    let sup = Box::new(MockSupervisionSync::new(vec![MockSupervisionSync::resp(
        200,
        &serde_json::to_string(&vec![submitted]).unwrap(),
    )]));
    let mgr = mock_manager(sup);
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    let r = mgr.bridge_one_pending(&ceremony, &v, &rpc);
    assert!(matches!(r, Err(AgentError::NoPending)), "got: {r:?}");
    assert_eq!(ceremony_pending_count(&ceremony), 0, "no ceremony created");
}

/// Small helper: how many ceremonies are pending (via a request→reject probe is
/// overkill; we just count by requesting a throwaway and comparing). We instead
/// assert emptiness by status-miss on a never-created id.
fn ceremony_pending_count(c: &SignatureCeremony) -> usize {
    // There is no public counter; a freshly-built ceremony with no successful
    // bridge has nothing at id "1".
    if c.status("1").is_some() {
        1
    } else {
        0
    }
}

/// A 401 from the supervision surface surfaces as `AgentError::Status(401)`
/// through the manager (the gate is observable, not swallowed).
#[test]
fn manager_surfaces_401_from_supervision() {
    let sup = Box::new(MockSupervisionSync::new(vec![MockSupervisionSync::resp(401, "\"nope\"")]));
    let mgr = mock_manager(sup);
    let r = mgr.fetch_signature_requests();
    assert!(matches!(r, Err(AgentError::Status(401))), "got: {r:?}");
}

// ===========================================================================
// WP0 — spawn/supervise under the SidecarSupervisor (stub node-agent binary)
// ===========================================================================

/// Absolute path to the CI stub node-agent shell script (serves the supervision
/// API on the loopback port from CITRATE_NODE_AGENT_ADDR + reads the bearer file,
/// blocks until killed).
fn stub_agent_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_node_agent.sh")
}

/// Build a manager over the stub binary + the production ureq transport, bound to
/// an ephemeral loopback port so parallel tests do not collide on 19600.
fn stub_manager(tag: &str) -> (AgentManager, PathBuf, u16) {
    let dir = tmp_dir(tag);
    // Pick a free port by binding + dropping (racy but fine for a stub spawn).
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let bind = format!("127.0.0.1:{port}");
    let mgr = AgentManager::new(
        stub_agent_bin(),
        dir.join("compute.json"),
        dir.join("supervision.token"),
        dir.join("crash.jsonl"),
        format!("http://{bind}"),
        bind.clone(),
        Box::new(UreqSupervisionTransport),
    );
    (mgr, dir, port)
}

/// WP0 + WP1 live-ish (CI-safe stub): `start` mints a bearer, persists it 0600,
/// spawns the stub under the supervisor; the stub reads OUR token file and serves
/// `/status` gated by it, so `supervision_status()` round-trips the REAL bearer
/// over a REAL loopback socket. Then `stop` is clean (no orphan). This is the
/// closest CI-safe proxy for the real node-agent handshake.
#[test]
fn start_spawns_stub_and_bearer_round_trips_over_loopback() {
    let (mgr, dir, _port) = stub_manager("spawn");
    mgr.start().expect("stub node-agent starts under the supervisor");

    // The token file must exist 0600 (the child adopts it).
    let token_path = dir.join("supervision.token");
    for _ in 0..50 {
        if token_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(token_path.exists(), "session bearer file must be written for the child");

    // Poll the supervision surface until the stub is serving (bearer round-trip).
    let mut ok_status = None;
    for _ in 0..100 {
        if let Ok(s) = mgr.supervision_status() {
            ok_status = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        ok_status.as_deref(),
        Some("idle"),
        "the stub must accept OUR minted bearer and serve /status"
    );

    // status() reflects Running + authed.
    assert_eq!(mgr.status().state, "running");
    assert!(mgr.status().authed, "a live bearer session exists");

    mgr.stop();
    assert_eq!(mgr.status().state, "stopped");
    assert!(!mgr.status().authed, "bearer wiped on stop");
    // The token file is removed on stop (no lingering session credential).
    assert!(!token_path.exists(), "token file removed on stop");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A missing binary fails closed with `BinaryNotFound` — never a silent no-op.
#[test]
fn missing_agent_binary_fails_closed() {
    let dir = tmp_dir("nobin");
    let mgr = AgentManager::new(
        PathBuf::from("/nonexistent/node-agent-zzz"),
        dir.join("compute.json"),
        dir.join("supervision.token"),
        dir.join("crash.jsonl"),
        "http://127.0.0.1:19600",
        "127.0.0.1:19600",
        Box::new(UreqSupervisionTransport),
    );
    let r = mgr.start();
    assert!(matches!(r, Err(AgentError::BinaryNotFound(_))), "got: {r:?}");
    // A failed start leaves NO bearer file (fail closed).
    assert!(!dir.join("supervision.token").exists());
}

/// Double-start is rejected (idempotent guard), not a second spawn.
#[test]
fn double_start_rejected() {
    let (mgr, dir, _port) = stub_manager("double");
    mgr.start().expect("first start");
    std::thread::sleep(std::time::Duration::from_millis(150));
    let r = mgr.start();
    assert!(matches!(r, Err(AgentError::AlreadyRunning)), "got: {r:?}");
    mgr.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// `stop` on a never-started agent is a no-op (idempotent).
#[test]
fn stop_when_never_started_is_noop() {
    let (mgr, dir, _port) = stub_manager("stopnoop");
    mgr.stop();
    assert_eq!(mgr.status().state, "stopped");
    let _ = std::fs::remove_dir_all(&dir);
}

// ===========================================================================
// LIVE (documented, #[ignore]) — the REAL node-agent handshake + real request
// ===========================================================================
//
// Heavy: builds/runs the real node-agent (citrate-node-agent). Run explicitly:
//
//   # 1) build the real node-agent (from citrate-node-agent @ pinned rev):
//   cargo build --release --bin node-agent
//   # 2) point this test at it + a compute.json:
//   CITRATE_NODE_AGENT_BIN=/abs/path/to/node-agent \
//   CITRATE_AGENT_COMPUTE_JSON=/abs/path/to/compute.json \
//     cargo test --locked agent::tests::live_real_node_agent_handshake -- --ignored --nocapture
//
// It spawns the REAL node-agent daemon under the supervisor, hands it OUR minted
// bearer via the token file, and round-trips `/status` over the real loopback
// supervision surface — the genuine WP1 handshake. If CITRATE_RPC_URL +
// CITRATE_PROVIDER_ADDRESS are also set and there is REAL claimable, it emits a
// real claimRewards SignatureRequest which the bridge routes to the ceremony;
// otherwise it honestly reports "no claimable, request well-formed" (Rule 1 — no
// fabricated tx). Broadcasting a real claim is C2 (needs an unlocked funded
// vault + live claimable) and is NOT done here.
#[test]
#[ignore = "heavy: builds/runs the real citrate-node-agent daemon"]
fn live_real_node_agent_handshake() {
    let bin = match std::env::var("CITRATE_NODE_AGENT_BIN") {
        Ok(p) if PathBuf::from(&p).exists() => PathBuf::from(p),
        _ => panic!("set CITRATE_NODE_AGENT_BIN to the built node-agent binary"),
    };
    let dir = tmp_dir("live");
    std::fs::create_dir_all(&dir).unwrap();
    // A minimal compute.json (the node-agent parses ComputeSettings from it).
    let compute = match std::env::var("CITRATE_AGENT_COMPUTE_JSON") {
        Ok(p) => PathBuf::from(p),
        Err(_) => {
            let p = dir.join("compute.json");
            std::fs::write(
                &p,
                r#"{"enabled":true,"allocation_percent":50,"schedule":"always"}"#,
            )
            .unwrap();
            p
        }
    };
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let bind = format!("127.0.0.1:{port}");
    let mgr = AgentManager::new(
        bin,
        compute,
        dir.join("supervision.token"),
        dir.join("crash.jsonl"),
        format!("http://{bind}"),
        bind,
        Box::new(UreqSupervisionTransport),
    );
    mgr.start().expect("real node-agent starts under the supervisor");

    // The genuine WP1 handshake: OUR bearer over the real supervision surface.
    let mut status = None;
    for _ in 0..200 {
        if let Ok(s) = mgr.supervision_status() {
            status = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!("[live] node-agent /status = {status:?}");
    assert!(status.is_some(), "real node-agent must serve /status with OUR bearer");

    // If any signature requests are present (real claimable / market activity),
    // they decode to the grounded wire shape. Otherwise honestly nothing pending.
    match mgr.fetch_signature_requests() {
        Ok(reqs) => eprintln!("[live] {} signature request(s): {reqs:?}", reqs.len()),
        Err(e) => eprintln!("[live] fetch_signature_requests: {e}"),
    }
    mgr.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────────────────
// CLAIM TARGET. A validator's rewards accrue in `ValidatorRegistry` against the
// proposer pubkey, and only the registered staker — the member's MemberBond CLONE
// — may collect them. The member reaches them via `MemberBond.claimRewards()`,
// which is `onlyMember`, so the EOA is the right signer and the calldata is the
// SAME `claimRewards()` 0x372500ab. Only the TARGET differs.
//
// Every claim used to go to ContributionAccounting, which holds none of a
// validator's rewards (2026-08-06).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn user_claim_request_targets_whatever_contract_it_is_given() {
    let bond = "0xd9ff524e1e1959440e4c5f18aa0fe8b54f01752b";
    let to_bond = crate::earnings::user_claim_request_to(1, bond);
    assert_eq!(to_bond.to, bond, "a validator claim must go to the member's bond");

    let to_contrib = crate::earnings::user_claim_request(1);
    assert_eq!(
        to_contrib.to,
        crate::earnings::CONTRIBUTION_ACCOUNTING,
        "a non-validator claim still goes to ContributionAccounting"
    );

    // The 4 bytes are identical either way — MemberBond.claimRewards() and
    // ContributionAccounting.claimRewards() are both `claimRewards()`.
    assert_eq!(to_bond.calldata, to_contrib.calldata);
    assert_eq!(to_bond.calldata, "0x372500ab");
}

#[test]
fn a_validator_claim_never_silently_targets_contribution_accounting() {
    // NEGATIVE CONTROL: the bug was that the bond address was ignored entirely.
    let bond = "0xd9ff524e1e1959440e4c5f18aa0fe8b54f01752b";
    let req = crate::earnings::user_claim_request_to(1, bond);
    assert_ne!(req.to, crate::earnings::CONTRIBUTION_ACCOUNTING);
}
