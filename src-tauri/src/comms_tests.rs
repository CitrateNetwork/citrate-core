// CX-S3.2 — comms member-daemon manager + UDS client tests. Included into `comms::tests`.
//
// CI-safe: NO real comms-member-daemon. A long-lived `sleep` stub stands in for the child; a
// hand-rolled UDS server stub exercises the JSON client's handshake + framing.

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

/// A configured manager over the sleep stub (a dummy seed set, so it can start).
fn stub_manager(tag: &str) -> (CommsMemberManager, PathBuf) {
    let dir = tmp_dir(tag);
    let mgr = CommsMemberManager::new(
        sleep_bin(),
        dir.join("member.sock"),
        dir.join("member.bearer"),
        dir.clone(),
        "deadbeef".repeat(8), // dummy seed
        COMMS_DOMAIN,
        dir.join("crash.jsonl"),
    );
    (mgr, dir)
}

#[test]
fn start_refuses_when_identity_not_provisioned() {
    let dir = tmp_dir("noid");
    // Empty seed → NotConfigured, WITHOUT touching the binary (the identity gate).
    let mgr = CommsMemberManager::new(
        sleep_bin(),
        dir.join("s"),
        dir.join("b"),
        dir.clone(),
        "",
        COMMS_DOMAIN,
        dir.join("c"),
    );
    assert!(!mgr.is_configured());
    assert!(matches!(mgr.start(), Err(CommsError::NotConfigured)));
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn start_refuses_when_binary_missing() {
    let dir = tmp_dir("nobin");
    let mgr = CommsMemberManager::new(
        dir.join("no-daemon"),
        dir.join("s"),
        dir.join("b"),
        dir.clone(),
        "aa".repeat(32),
        COMMS_DOMAIN,
        dir.join("c"),
    );
    assert!(matches!(mgr.start(), Err(CommsError::BinaryNotFound(_))));
}

#[test]
fn spec_env_carries_socket_bearerpath_seedpath_domain_never_inline() {
    let (mgr, dir) = stub_manager("env");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(ENV_SOCKET).map(String::as_str), Some(dir.join("member.sock").to_string_lossy().as_ref()));
    // The bearer crosses as a FILE PATH, never the token value.
    assert_eq!(env.get(ENV_BEARER_FILE).map(String::as_str), Some(dir.join("member.bearer").to_string_lossy().as_ref()));
    // The seed ALSO crosses as a 0600 FILE PATH — never inline (env/argv leak to `ps`).
    assert_eq!(env.get(ENV_SEED_FILE).map(String::as_str), Some(dir.join("member.seed").to_string_lossy().as_ref()));
    assert!(!env.contains_key("CITRATE_MEMBER_SEED"), "the raw seed value must never cross inline");
    assert!(!env.values().any(|v| v.contains(&*mgr_seed_hex())), "no env value carries the seed bytes");
    assert_eq!(env.get(ENV_DOMAIN).map(String::as_str), Some(COMMS_DOMAIN));
    // No positional argv (config is all ENV).
    assert!(mgr.spec_env_for_test().iter().all(|(k, _)| k.starts_with("CITRATE_MEMBER_")));
}

#[test]
fn relay_url_selects_ws_transport_in_daemon_env() {
    // GROW-S2: with a networked relay configured, the daemon env carries CITRATE_MEMBER_RELAY_URL —
    // a public URL, no secret — so the daemon selects the WsRelay transport (cluster rendezvous).
    let (mgr, _dir) = stub_manager("relay");
    let mgr = mgr.with_relay_url(CLUSTER_RELAY_URL);
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(ENV_RELAY_URL).map(String::as_str), Some(CLUSTER_RELAY_URL));
    // Still all CITRATE_MEMBER_* keys; still no inline secret.
    assert!(env.keys().all(|k| k.starts_with("CITRATE_MEMBER_")));
    assert!(!env.values().any(|v| v.contains(&*mgr_seed_hex())), "no env value carries the seed bytes");
}

#[test]
fn empty_relay_url_stays_in_process() {
    // An empty/whitespace URL is ignored — no CITRATE_MEMBER_RELAY_URL, so the daemon runs its
    // in-process relay (local single-machine); the in-process SIWE domain is unchanged.
    let (mgr, _dir) = stub_manager("norelay");
    let mgr = mgr.with_relay_url("   ");
    let env: std::collections::BTreeMap<String, String> =
        mgr.spec_env_for_test().into_iter().collect();
    assert!(!env.contains_key(ENV_RELAY_URL), "an empty relay url must not set the transport env");
    assert_eq!(env.get(ENV_DOMAIN).map(String::as_str), Some(COMMS_DOMAIN));
}

// ---- resolve_relay_transport (pure) — GROW-S2 DEFAULT-ON transport + SIWE-domain rule ----

#[test]
fn resolve_transport_defaults_ON_to_the_shared_relay() {
    // DEFAULT-ON (alpha): unset OR whitespace-only ⇒ the shared rendezvous relay under its domain, so a
    // partner opening the DMG connects out of the box (no env/config).
    assert_eq!(
        resolve_relay_transport(None, None),
        (Some(CLUSTER_RELAY_URL.to_string()), CLUSTER_RELAY_DOMAIN.to_string())
    );
    assert_eq!(
        resolve_relay_transport(Some("   ".into()), None),
        (Some(CLUSTER_RELAY_URL.to_string()), CLUSTER_RELAY_DOMAIN.to_string())
    );
}

#[test]
fn resolve_transport_explicit_off_switch_falls_back_to_in_process() {
    // The escape hatch: an explicit off-value (case-insensitive) drops to the local in-process relay.
    for off in ["off", "OFF", "disabled", "none", "local", "in-process", "0", "false"] {
        assert_eq!(
            resolve_relay_transport(Some(off.into()), Some("ignored".into())),
            (None, COMMS_DOMAIN.to_string()),
            "'{off}' must select in-process"
        );
    }
}

#[test]
fn resolve_transport_cluster_url_derives_matching_domain() {
    // Setting JUST the cluster URL derives the correct SIWE domain from the host — the relay would
    // reject a mismatch, so this must equal CLUSTER_RELAY_DOMAIN (guards that consistency too).
    assert_eq!(
        resolve_relay_transport(Some(CLUSTER_RELAY_URL.into()), None),
        (Some(CLUSTER_RELAY_URL.to_string()), CLUSTER_RELAY_DOMAIN.to_string())
    );
    assert_eq!(host_of(CLUSTER_RELAY_URL).as_deref(), Some(CLUSTER_RELAY_DOMAIN));
}

#[test]
fn resolve_transport_custom_url_derives_host_domain() {
    // A custom relay with no explicit domain derives the host (port/path/userinfo stripped) — not the
    // cluster domain, so it won't silently mismatch (F2 footgun fixed).
    assert_eq!(
        resolve_relay_transport(Some("wss://other.example:443/ws".into()), None),
        (Some("wss://other.example:443/ws".to_string()), "other.example".to_string())
    );
}

#[test]
fn resolve_transport_explicit_domain_overrides_the_host() {
    // For the rare host != relay-domain case, CITRATE_MEMBER_DOMAIN wins.
    assert_eq!(
        resolve_relay_transport(Some("wss://h.example".into()), Some("d.example".into())),
        (Some("wss://h.example".to_string()), "d.example".to_string())
    );
}

/// The dummy seed value stub_manager configures — used to assert it never appears inline in env.
fn mgr_seed_hex() -> String {
    "deadbeef".repeat(8)
}

// ---- Option A: device-sealed comms key (keyring mint/load) ----

use crate::custody::Keyring as _; // bring `.set()`/`.get()` into scope for the direct test call

/// An in-memory fake OS keyring for the seed-provisioning tests (no real keychain in CI).
struct FakeKeyring(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);
impl FakeKeyring {
    fn new() -> Self {
        FakeKeyring(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}
impl crate::custody::Keyring for FakeKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, crate::custody::CustodyError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), crate::custody::CustodyError> {
        self.0.lock().unwrap().insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), crate::custody::CustodyError> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

// A fixed VALID secp256k1 scalar standing in for wallet::derive_scoped_secret (which is deterministic
// in the wallet). Distinct from B so we can prove the cache wins over a second derive.
const DERIVED_A: [u8; COMMS_SEED_LEN] = [7u8; COMMS_SEED_LEN];
const DERIVED_B: [u8; COMMS_SEED_LEN] = [9u8; COMMS_SEED_LEN];

#[test]
fn comms_seed_is_derived_sealed_and_stable_across_restarts() {
    let kr = FakeKeyring::new();
    // First start under v2: derive from the (fake) wallet + seal it.
    let first =
        load_or_derive_comms_seed(&kr, || Ok(zeroize::Zeroizing::new(DERIVED_A))).expect("derive");
    // 32 bytes hex, a VALID secp256k1 scalar — the daemon's EthWallet::from_secret_key accepts it.
    assert_eq!(first.len(), COMMS_SEED_LEN * 2);
    let bytes = hex::decode(&*first).expect("hex");
    assert!(k256::ecdsa::SigningKey::from_slice(&bytes).is_ok(), "derived key is a valid secp256k1 scalar");
    assert_eq!(bytes, DERIVED_A, "the sealed key is exactly what derive produced");
    // Sealed under the CONNECT-S5 v2 account (not the legacy random one).
    assert!(kr.0.lock().unwrap().contains_key(KEYRING_COMMS_ACCOUNT_V2));
    assert!(!kr.0.lock().unwrap().contains_key(KEYRING_COMMS_ACCOUNT), "legacy account untouched");
    // Stable across restarts: a second load returns the CACHED key and does NOT re-derive — proven by
    // handing it a derive that would return a DIFFERENT key; the cached one must still win.
    let second =
        load_or_derive_comms_seed(&kr, || Ok(zeroize::Zeroizing::new(DERIVED_B))).expect("load");
    assert_eq!(&*first, &*second, "cached key wins; derive is not re-invoked once sealed");
}

#[test]
fn comms_seed_rejects_a_corrupt_stored_key() {
    let kr = FakeKeyring::new();
    // Wrong length in the v2 slot → hard fault (never hand a bad key to the daemon). derive unused.
    kr.set(KEYRING_COMMS_ACCOUNT_V2, &[1, 2, 3]).unwrap();
    assert!(matches!(
        load_or_derive_comms_seed(&kr, || Ok(zeroize::Zeroizing::new(DERIVED_A))),
        Err(CommsError::Keyring(_))
    ));
}

#[test]
fn comms_seed_fails_closed_when_wallet_not_ready() {
    let kr = FakeKeyring::new();
    // No cached key and the wallet can't derive (locked / not created) → WalletNotReady, NEVER a
    // random throwaway identity (Rule 1). Nothing is sealed.
    let r = load_or_derive_comms_seed(&kr, || Err("custody vault unavailable: locked".to_string()));
    assert!(matches!(r, Err(CommsError::WalletNotReady(_))));
    assert!(kr.0.lock().unwrap().is_empty(), "no identity sealed on a fail-closed derive");
}

#[test]
fn start_reaches_running_then_stop_is_clean() {
    let (mgr, _dir) = stub_manager("wiring");
    let mgr = mgr
        .with_spawn_args(vec!["3600".to_string()])
        .with_health_interval(std::time::Duration::from_secs(3600)); // socket probe won't fire mid-test
    mgr.start().expect("stub daemon starts");
    let mut running = false;
    for _ in 0..100 {
        if mgr.status().state == "running" {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "daemon stub must reach Running");
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
    assert!(matches!(mgr.start(), Err(CommsError::AlreadyRunning)));
    mgr.stop();
}

/// A SHORT socket path (UDS paths must be < SUN_LEN ~104 on macOS; a temp-dir path overflows it).
fn short_sock(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() % 100_000_000)
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/ctz-{tag}-{n}.sock"))
}

// The UDS JSON client: bearer handshake + one request/response against a stub socket server.
#[test]
fn member_ipc_authenticates_and_round_trips_a_request() {
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
        // auth line
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains(&bearer_srv), "auth carried the bearer");
        writeln!(w, "{{\"type\":\"ready\"}}").unwrap();
        // request → canned GroupCreated response
        line.clear();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("createGroup"), "got the request: {line}");
        writeln!(w, "{{\"type\":\"groupCreated\",\"id\":\"abc123\"}}").unwrap();
    });
    // Wait for the socket, then drive the client.
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let resp = member_ipc(&sock, &bearer, &Request::CreateGroup { name: "deals".into() }).expect("ipc");
    match resp {
        Response::GroupCreated { id } => assert_eq!(id, "abc123"),
        other => panic!("expected GroupCreated, got {other:?}"),
    }
    // A wrong bearer is rejected (no "ready").
    let _ = handle.join();
}

#[test]
fn add_member_request_serializes_and_added_response_parses() {
    // The post-S0 owner-invite path: Request::AddMember -> op "addMember"; the daemon's Added
    // response (member/welcome/ratchet_tree) parses (extra fields ignored — the owner-client only
    // needs to know the add succeeded).
    let req = serde_json::to_string(&Request::AddMember {
        group: "aa".repeat(32),
        member: "0x00000000000000000000000000000000000000c0".into(),
    })
    .expect("serialize");
    assert!(req.contains("\"op\":\"addMember\""), "op tag: {req}");
    assert!(req.contains("\"member\":\"0x0000"), "member field: {req}");

    let resp: Response = serde_json::from_str(
        "{\"type\":\"added\",\"member\":\"0xabc\",\"welcome\":\"dead\",\"ratchetTree\":\"beef\"}",
    )
    .expect("parse Added");
    assert!(matches!(resp, Response::Added { .. }));
}

#[test]
fn address_from_secret_hex_matches_known_evm_vector() {
    // Standard EVM derivation vector: secp256k1 private key = 1 → address
    // 0x7e5f4552091a69125d5dfcb7b8c2659029395bdf. Proves the comms/cluster identity address this
    // computes equals the daemon's `derive_address_from_secp256k1`, so it matches the roster.
    let sk1 = format!("{:0>64}", "1");
    let addr = address_from_secret_hex(&sk1).expect("derive");
    assert_eq!(addr, "7e5f4552091a69125d5dfcb7b8c2659029395bdf");
    // No 0x prefix, lowercase, 40 hex chars.
    assert_eq!(addr.len(), 40);
    assert!(addr.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn address_from_secret_hex_rejects_non_hex_and_bad_scalar() {
    assert!(address_from_secret_hex("nothex").is_err());
    // 32 zero bytes is not a valid secp256k1 scalar.
    assert!(address_from_secret_hex(&"00".repeat(32)).is_err());
}

#[test]
fn shutdown_is_a_safe_noop_when_never_started() {
    // The process-wide MANAGER singleton is only set via ensure_started (needs an app handle), so in
    // the unit-test binary it stays uninitialized and shutdown() must be a clean no-op (never panics).
    super::shutdown();
}
