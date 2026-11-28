// 0.6.0 L6-WP05: OAuth client v2 tests (lanes/L6 WP05 RED list, plus the lead's additions:
// long tokens, OAuth state, declined consent, refresh, revoke, and no credential in an IPC
// payload or a log line). The provider and the relay are test seams (Rule 1: a TEST seam, never
// a live socket presented as real); the vault runs headless over an in-memory keyring; the
// listener tests use a real loopback socket on an OS-assigned port.
//
// L6-WP05 stack, part 2: the registry, the relay exchange, the debug-only dev file, OAuth state
// and declined consent. Refresh and revoke tests arrive with that code (part 3).

use super::relay::{self, RelayError, RelayOp, RelayTransport};
use super::*;
use crate::connections::*;
use crate::custody::{CustodyError, CustodyVault, Keyring};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::Zeroizing;

const PASS: &[u8] = b"correct horse battery staple";

/// Every fixture credential below contains this, so one substring check covers them all.
const MARK: &str = "FIXTURE-WP05";

#[derive(Default)]
struct FakeKeyring {
    store: Mutex<HashMap<String, Vec<u8>>>,
}
impl Keyring for FakeKeyring {
    fn get(&self, a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(a).cloned())
    }
    fn set(&self, a: &str, s: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().insert(a.to_string(), s.to_vec());
        Ok(())
    }
    fn delete(&self, a: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(a);
        Ok(())
    }
}

fn tmp(tag: &str) -> PathBuf {
    let n: u64 = rand::random();
    std::env::temp_dir().join(format!(
        "citrate-core-wp05-{tag}-{}-{n:016x}",
        std::process::id()
    ))
}

fn fresh_vault() -> CustodyVault {
    let p = tmp("vault");
    let vault = CustodyVault::new(Box::new(FakeKeyring::default()), p, 30);
    vault.init(&mut PASS.to_vec()).unwrap();
    vault.unlock(&mut PASS.to_vec()).unwrap();
    vault
}

/// One recorded form POST: the URL and its fields.
type FormPost = (String, Vec<(String, String)>);

/// The provider side: records every form POST and answers from a script.
#[derive(Default)]
struct Provider {
    posts: Mutex<Vec<FormPost>>,
    answers: Mutex<VecDeque<std::result::Result<String, crate::oidc::AuthError>>>,
}
struct ProviderHttp(Arc<Provider>);
impl crate::oidc::HttpClient for ProviderHttp {
    fn get(
        &self,
        url: &str,
        _bearer: Option<&str>,
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        panic!("no GET expected, got {url}")
    }
    fn post_form(
        &self,
        url: &str,
        form: &[(&str, &str)],
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        self.0.posts.lock().unwrap().push((
            url.to_string(),
            form.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ));
        self.0
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(crate::oidc::AuthError::Network))
    }
}

/// An HTTP seam that fails the test if any request is made.
struct NoNetwork;
impl crate::oidc::HttpClient for NoNetwork {
    fn get(
        &self,
        url: &str,
        _bearer: Option<&str>,
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        panic!("no request may be made here, got GET {url}")
    }
    fn post_form(
        &self,
        url: &str,
        _form: &[(&str, &str)],
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        panic!("no request may be made here, got POST {url}")
    }
}

/// The relay side: records (provider, op, body) and answers from a script.
#[derive(Default)]
struct FakeRelay {
    calls: Mutex<Vec<(String, RelayOp, String)>>,
    answers: Mutex<VecDeque<std::result::Result<String, RelayError>>>,
}
impl FakeRelay {
    fn answering(answers: Vec<std::result::Result<String, RelayError>>) -> Self {
        FakeRelay {
            calls: Mutex::new(Vec::new()),
            answers: Mutex::new(answers.into()),
        }
    }
    fn calls(&self) -> Vec<(String, RelayOp, String)> {
        self.calls.lock().unwrap().clone()
    }
}
impl RelayTransport for FakeRelay {
    fn post(
        &self,
        provider: &str,
        op: RelayOp,
        body: &str,
    ) -> std::result::Result<Zeroizing<String>, RelayError> {
        self.calls
            .lock()
            .unwrap()
            .push((provider.to_string(), op, body.to_string()));
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(RelayError::Network))
            .map(Zeroizing::new)
    }
}

type Logs = Arc<Mutex<Vec<String>>>;

fn capture() -> (Logs, LogSink) {
    let logs: Logs = Arc::default();
    let sink = logs.clone();
    (
        logs,
        Box::new(move |line: &str| sink.lock().unwrap().push(line.to_string())),
    )
}

/// A v2 (relay) manager with no dev file and a provider seam that must not be called.
fn relay_mgr() -> (ConnectionManager, Logs) {
    let (logs, sink) = capture();
    let mgr = ConnectionManager::new(Box::new(NoNetwork), tmp("nodev").join("oauth.dev.json"))
        .with_v2(true)
        .with_log(sink);
    (mgr, logs)
}

fn github_plan() -> ClientPlan {
    ClientPlan::Relay {
        client_id: GITHUB_CLIENT_ID_FOR_TESTS,
        provider: "github",
    }
}

const GITHUB_CLIENT_ID_FOR_TESTS: &str = super::providers::GITHUB_CLIENT_ID;

fn record_json(slot_service: Service, vault: &CustodyVault, rec: serde_json::Value) {
    let mut bytes = serde_json::to_vec(&rec).unwrap();
    vault.put(&slot(slot_service), &mut bytes).unwrap();
}

fn sealed(service: Service, vault: &CustodyVault) -> serde_json::Value {
    let bytes = vault.custody_get(&slot(service)).expect("slot present");
    serde_json::from_slice(&bytes).expect("record parses")
}

fn body_keys(body: &str) -> BTreeSet<String> {
    let v: serde_json::Value = serde_json::from_str(body).expect("relay body is JSON");
    v.as_object()
        .expect("relay body is an object")
        .keys()
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// 05a: registry, dev file, relay exchange
// ---------------------------------------------------------------------------

#[test]
fn release_build_ignores_dev_file() {
    // Every dev-file item in connections.rs is compiled only into a debug build.
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/connections.rs"));
    let prod = src
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or(src);
    for item in [
        "struct ClientCreds",
        "fn dev_credentials_path",
        "fn parse_dev_credentials",
        "fn env_prefix",
        "fn creds(",
        "fn exchange_and_store(",
        "DevFile(ClientCreds)",
    ] {
        let at = prod
            .find(item)
            .unwrap_or_else(|| panic!("{item} not found"));
        let before: Vec<&str> = prod[..at].lines().rev().take(3).collect();
        assert!(
            before
                .iter()
                .any(|l| l.trim() == "#[cfg(debug_assertions)]"),
            "{item} must be #[cfg(debug_assertions)]: {before:?}"
        );
    }
    // The registry and the relay client never read a file or the dev-file variable.
    for (name, file) in [
        ("providers.rs", include_str!("providers.rs")),
        ("relay.rs", include_str!("relay.rs")),
    ] {
        assert!(!file.contains("std::fs"), "{name} reads a file");
        assert!(
            !file.contains("CITRATE_OAUTH_DEV_FILE"),
            "{name} names the dev file"
        );
        assert!(
            !file.contains("client_secret"),
            "{name} names a client secret"
        );
    }
    // provider_setup answers from the compiled table only.
    let gh = provider_setup("github").expect("github row");
    assert_eq!(gh.mode, ClientMode::Relay);
    assert_eq!(gh.client_id, Some("Ov23liUgI4U92ZzDXwnt"));
    assert!(gh.configured);
}

#[test]
fn debug_build_still_reads_dev_file() {
    let dev = tmp("dev");
    std::fs::write(
        &dev,
        "GITHUB_CLIENT_ID=dev-gh-id\nGITHUB_CLIENT_SECRET=FIXTURE-WP05-DEV-SECRET\n",
    )
    .unwrap();
    let provider = Arc::new(Provider::default());
    provider.answers.lock().unwrap().push_back(Ok(
        r#"{"access_token":"FIXTURE-WP05-AT","scope":"repo,read:org","token_type":"bearer"}"#
            .to_string(),
    ));
    // The dev file wins even with v2 on (development keeps its own app).
    let mgr =
        ConnectionManager::new(Box::new(ProviderHttp(provider.clone())), dev.clone()).with_v2(true);
    let plan = mgr
        .plan(Service::GitHub)
        .expect("configured from the dev file");
    assert_eq!(plan.mode(), "dev-file");
    assert_eq!(plan.client_id(), "dev-gh-id");
    let vault = fresh_vault();
    let st = mgr
        .exchange_with_plan(Service::GitHub, &plan, &vault, "code", "verifier", None)
        .expect("exchanged");
    assert!(st.connected);
    let posts = provider.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].0, "https://github.com/login/oauth/access_token");
    assert!(posts[0]
        .1
        .iter()
        .any(|(k, v)| k == "client_secret" && v == "FIXTURE-WP05-DEV-SECRET"));
    let _ = std::fs::remove_file(&dev);
}

#[test]
fn relay_mode_never_sends_a_secret_from_device() {
    let (mgr, _logs) = relay_mgr();
    assert!(
        mgr.is_configured(Service::GitHub),
        "GitHub is a relay-mode service in v2"
    );
    let plan = mgr.plan(Service::GitHub).expect("relay plan");
    assert_eq!(plan.mode(), "relay");
    assert_eq!(plan.client_id(), "Ov23liUgI4U92ZzDXwnt");

    let relay = FakeRelay::answering(vec![Ok(
        r#"{"access_token":"FIXTURE-WP05-AT","token_type":"bearer","scope":"repo,read:org"}"#
            .to_string(),
    )]);
    let vault = fresh_vault();
    let st = mgr
        .exchange_with_plan(
            Service::GitHub,
            &plan,
            &vault,
            "the-code",
            "the-verifier",
            Some(&relay),
        )
        .expect("exchanged through the relay");
    assert!(st.connected);
    assert_eq!(st.scope.as_deref(), Some("repo,read:org"));
    let calls = relay.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "github");
    assert_eq!(calls[0].1, RelayOp::Token);
    // Exactly the frozen fields: no client id, no secret.
    assert_eq!(
        body_keys(&calls[0].2),
        BTreeSet::from([
            "code".to_string(),
            "code_verifier".to_string(),
            "redirect_uri".to_string()
        ])
    );
    let body: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
    assert_eq!(body["redirect_uri"], OAUTH_REDIRECT_URI);
    assert_eq!(body["code"], "the-code");
    assert_eq!(body["code_verifier"], "the-verifier");
    assert!(!calls[0].2.contains("secret"));
    assert_eq!(
        sealed(Service::GitHub, &vault)["access_token"],
        "FIXTURE-WP05-AT"
    );

    // Notion would send the hosted bounce, as registered on the relay.
    let notion = relay::token_body("c", "v", Service::Notion.redirect_uri()).unwrap();
    assert!(notion.contains("https://auth.citrate.ai/oauth/callback"));
}

#[test]
fn relay_mode_without_a_session_says_sign_in_and_opens_no_browser() {
    let (mgr, _logs) = relay_mgr();
    let vault = fresh_vault();
    let err = mgr
        .connect_with(Service::GitHub, &vault, None, |_| panic!("no browser"))
        .unwrap_err();
    assert_eq!(err, ConnError::NotSignedIn);
    assert_eq!(err.to_string(), "Sign in to Citrate to connect");
    // A relay that reports no session maps to the same answer, and nothing is stored.
    let relay = FakeRelay::answering(vec![Err(RelayError::NotSignedIn)]);
    let err = mgr
        .exchange_with_plan(
            Service::GitHub,
            &github_plan(),
            &vault,
            "c",
            "v",
            Some(&relay),
        )
        .unwrap_err();
    assert_eq!(err, ConnError::NotSignedIn);
    assert!(mgr.status(&vault).iter().all(|s| !s.connected));
    // 503 not_configured / relay_disabled is "not configured", never a device secret.
    let relay = FakeRelay::answering(vec![Err(RelayError::NotConfigured)]);
    let err = mgr
        .exchange_with_plan(
            Service::GitHub,
            &github_plan(),
            &vault,
            "c",
            "v",
            Some(&relay),
        )
        .unwrap_err();
    assert_eq!(err, ConnError::NotConfigured);
}

#[test]
fn relay_answer_with_an_error_field_is_a_failed_exchange() {
    // GitHub answers a bad code with 200 and an `error` field; the relay passes it through.
    let (mgr, logs) = relay_mgr();
    let vault = fresh_vault();
    let relay = FakeRelay::answering(vec![Ok(
        r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired."}"#
            .to_string(),
    )]);
    let err = mgr
        .exchange_with_plan(
            Service::GitHub,
            &github_plan(),
            &vault,
            "c",
            "v",
            Some(&relay),
        )
        .unwrap_err();
    assert_eq!(err, ConnError::TokenExchange);
    assert!(mgr.status(&vault).iter().all(|s| !s.connected));
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.contains("exchange via relay failed")));
}

#[test]
fn public_pkce_mode_sends_verifier_only() {
    let provider = Arc::new(Provider::default());
    provider
        .answers
        .lock()
        .unwrap()
        .push_back(Ok(r#"{"access_token":"FIXTURE-WP05-AT"}"#.to_string()));
    let mgr = ConnectionManager::new(
        Box::new(ProviderHttp(provider.clone())),
        tmp("nodev").join("none"),
    )
    .with_v2(true);
    let vault = fresh_vault();
    mgr.exchange_with_plan(
        Service::HuggingFace,
        &ClientPlan::PublicPkce {
            client_id: "public-id",
        },
        &vault,
        "c",
        "the-verifier",
        None,
    )
    .expect("public exchange");
    let posts = provider.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 1);
    let keys: BTreeSet<&str> = posts[0].1.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "grant_type",
            "code",
            "redirect_uri",
            "client_id",
            "code_verifier"
        ])
    );
}

#[test]
fn not_configured_reason_names_the_missing_registration() {
    for (id, provider, name) in [
        ("notion", "notion", "Notion"),
        ("gdrive", "google", "Google"),
        ("gsheets", "google", "Google"),
        ("gcal", "google", "Google"),
        ("hf", "huggingface", "Hugging Face"),
    ] {
        let s = provider_setup(id).expect(id);
        assert_eq!(s.provider, provider);
        assert!(!s.configured, "{id}");
        assert_eq!(s.client_id, None, "{id}");
        let reason = s.reason.expect("a reason");
        assert!(reason.contains(name), "{id}: {reason}");
        assert!(reason.contains("CONNECTIONS_RUNBOOK"), "{id}: {reason}");
        assert!(!reason.contains('\u{2014}'), "no em-dash: {reason}");
    }
    assert!(provider_setup("github").and_then(|s| s.reason).is_none());
    assert!(provider_setup("slack").is_none());

    // With v2 on and no dev file, only GitHub is configured; with v2 off, none is.
    let (mgr, logs) = relay_mgr();
    for svc in ALL_SERVICES {
        assert_eq!(mgr.is_configured(svc), svc == Service::GitHub, "{svc:?}");
    }
    let vault = fresh_vault();
    let err = mgr
        .connect_with(Service::Notion, &vault, None, |_| panic!("no browser"))
        .unwrap_err();
    assert_eq!(err, ConnError::NotConfigured);
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.starts_with("connections: notion not configured:") && l.contains("Notion")));
    let off = ConnectionManager::new(Box::new(NoNetwork), tmp("nodev").join("none"));
    assert!(ALL_SERVICES.iter().all(|s| !off.is_configured(*s)));
}

#[test]
fn github_requests_repo_and_read_org() {
    assert_eq!(Service::GitHub.default_scopes(), &["repo", "read:org"]);
    let url = authorize_url(Service::GitHub, GITHUB_CLIENT_ID_FOR_TESTS, "s", "c");
    assert!(url.contains("scope=repo%20read%3Aorg"), "{url}");
    assert!(url.contains("client_id=Ov23liUgI4U92ZzDXwnt"), "{url}");
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8975%2Foauth%2Fcallback"));
}

#[test]
fn relay_paths_are_the_frozen_endpoints_on_the_pinned_issuer() {
    let issuer = crate::oidc::AuthorityConfig::production().issuer;
    for (op, seg) in [
        (RelayOp::Token, "token"),
        (RelayOp::Refresh, "refresh"),
        (RelayOp::Revoke, "revoke"),
    ] {
        assert_eq!(
            format!("{issuer}{}", relay::relay_path("github", op)),
            format!("https://auth.citrate.ai/oauth/relay/v1/github/{seg}")
        );
    }
}

#[test]
fn token_record_shape_unchanged_for_existing_slots() {
    // A record exactly as `exchange_and_store` wrote it at a826e441.
    let vault = fresh_vault();
    record_json(
        Service::GitHub,
        &vault,
        serde_json::json!({
            "access_token": "FIXTURE-WP05-OLD-AT",
            "refresh_token": null,
            "token_type": "bearer",
            "scope": "repo",
            "expires_at": null,
            "connected_at": 1_700_000_000u64,
        }),
    );
    let (mgr, _logs) = relay_mgr();
    let gh = mgr
        .status(&vault)
        .into_iter()
        .find(|s| s.service == "github")
        .unwrap();
    assert!(gh.connected);
    assert_eq!(gh.scope.as_deref(), Some("repo"));
    assert_eq!(gh.connected_at, Some(1_700_000_000));
    assert_eq!(
        sealed_access_token(Service::GitHub, &vault)
            .unwrap()
            .as_str(),
        "FIXTURE-WP05-OLD-AT"
    );

    // A record WP05 writes has the same keys.
    let relay = FakeRelay::answering(vec![
        Ok(r#"{"access_token":"FIXTURE-WP05-AT"}"#.to_string()),
    ]);
    mgr.exchange_with_plan(
        Service::GitHub,
        &github_plan(),
        &vault,
        "c",
        "v",
        Some(&relay),
    )
    .unwrap();
    let keys: BTreeSet<String> = sealed(Service::GitHub, &vault)
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        [
            "access_token",
            "refresh_token",
            "token_type",
            "scope",
            "expires_at",
            "connected_at"
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<BTreeSet<_>>()
    );
}

// ---------------------------------------------------------------------------
// Token handling: no length or shape assumption
// ---------------------------------------------------------------------------

fn long_tokens() -> Vec<String> {
    // An opaque bearer in the full RFC 6750 b64token alphabet, 500 characters.
    let mut opaque = String::from(MARK);
    while opaque.len() < 499 {
        opaque.push_str("aZ09-_.~+/");
    }
    opaque.truncate(499);
    opaque.push('=');
    // A fine-grained PAT shape: `github_pat_` + 22 + `_` + 59 characters (93 in all).
    let pat = format!("github_pat_11{MARK}AAAAAAAA_{}", "b".repeat(59));
    assert_eq!(opaque.len(), 500);
    assert_eq!(pat.len(), 93);
    vec![opaque, pat]
}

#[test]
fn long_and_pat_shaped_tokens_store_seal_unseal_and_redact_unchanged() {
    for tok in long_tokens() {
        let (mgr, logs) = relay_mgr();
        let vault = fresh_vault();
        let refresh = format!("{tok}-refresh");
        let answer = serde_json::json!({
            "access_token": tok,
            "refresh_token": refresh,
            "token_type": "bearer",
            "expires_in": 3600,
        })
        .to_string();
        let relay = FakeRelay::answering(vec![Ok(answer)]);
        let st = mgr
            .exchange_with_plan(
                Service::GitHub,
                &github_plan(),
                &vault,
                "c",
                "v",
                Some(&relay),
            )
            .expect("a long token stores");
        // Sealed and unsealed byte for byte.
        let rec = sealed(Service::GitHub, &vault);
        assert_eq!(rec["access_token"].as_str(), Some(tok.as_str()));
        assert_eq!(rec["refresh_token"].as_str(), Some(refresh.as_str()));
        assert_eq!(
            sealed_access_token(Service::GitHub, &vault)
                .unwrap()
                .as_str(),
            tok
        );
        // Not in the status payload or a log line.
        let payload = serde_json::to_string(&st).unwrap();
        assert!(!payload.contains(MARK), "{payload}");
        assert!(logs.lock().unwrap().iter().all(|l| !l.contains(MARK)));

        // secret_shapes redacts it whole, in every form a log line could carry it.
        for line in [
            format!("Authorization: Bearer {tok}"),
            format!("access_token={tok}"),
            format!("{{\"access_token\":\"{tok}\"}}"),
            format!("refresh_token: {refresh}"),
        ] {
            let out = crate::secret_shapes::redact(&line);
            assert!(!out.contains(MARK), "left a part of the token: {out}");
            assert!(
                !out.contains(&tok[tok.len() - 16..]),
                "left the token's tail: {out}"
            );
            assert!(out.contains(crate::secret_shapes::REDACTED), "{out}");
        }
    }
    // The PAT shape is caught bare, with no key in front of it.
    let pat = &long_tokens()[1];
    let bare = crate::secret_shapes::redact(&format!("cloned with {pat} ok"));
    assert!(!bare.contains("github_pat_"), "{bare}");
    assert_eq!(crate::secret_shapes::first_shape(pat), Some("token"));
}

// ---------------------------------------------------------------------------
// OAuth state and declined consent
// ---------------------------------------------------------------------------

#[test]
fn invalid_or_missing_state_is_rejected() {
    for target in [
        "/oauth/callback?code=c",
        "/oauth/callback?code=c&state=",
        "/oauth/callback?code=c&state=wrong",
        "/oauth/callback?code=c&state=the-flow-statf",
        "/oauth/callback?state=the-flow-state",
        "/oauth/callback?error=access_denied",
        "/oauth/callback?error=access_denied&state=wrong",
        "/oauth/callback?error=&state=the-flow-state",
        "/",
    ] {
        assert!(
            matches!(classify_callback(target, "the-flow-state"), Served::Foreign),
            "{target}"
        );
    }
    assert!(matches!(
        classify_callback(
            "/oauth/callback?code=c&state=the-flow-state",
            "the-flow-state"
        ),
        Served::Code(_)
    ));
    assert!(matches!(
        classify_callback(
            "/oauth/callback?error=access_denied&state=the-flow-state",
            "the-flow-state"
        ),
        Served::Declined
    ));
    // An error callback that also carries a code is not a denial.
    assert!(matches!(
        classify_callback(
            "/oauth/callback?error=x&code=c&state=wrong",
            "the-flow-state"
        ),
        Served::Foreign
    ));
}

fn hit(port: u16, req: &[u8]) -> String {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    s.write_all(req).unwrap();
    let mut resp = String::new();
    let _ = s.read_to_string(&mut resp);
    resp
}

#[test]
fn denied_consent_ends_the_flow_cleanly_and_nothing_is_connected() {
    let listener = ConnectionListener::bind_on(0).unwrap();
    let port = listener.port();
    let h = std::thread::spawn(move || {
        // A drive-by denial with the wrong state does not end the flow.
        let first = hit(
            port,
            b"GET /oauth/callback?error=access_denied&state=guess HTTP/1.1\r\nHost: x\r\n\r\n",
        );
        let second = hit(
            port,
            b"GET /oauth/callback?error=access_denied&error_description=The+user+denied&state=the-flow-state HTTP/1.1\r\nHost: x\r\n\r\n",
        );
        (first, second)
    });
    let err = listener
        .wait_for_callback(Duration::from_secs(10), "the-flow-state")
        .unwrap_err();
    let (first, second) = h.join().unwrap();
    assert_eq!(err, ConnError::Declined);
    assert!(first.starts_with("HTTP/1.1 400"), "{first}");
    assert!(second.starts_with("HTTP/1.1 200"), "{second}");
    assert!(second.contains("Nothing was connected"), "{second}");
    assert_eq!(
        err.to_string(),
        "the sign-in was declined at the provider; nothing was connected"
    );
    // Nothing reached the vault: every service reads not connected.
    let (mgr, _logs) = relay_mgr();
    let vault = fresh_vault();
    assert!(mgr.status(&vault).iter().all(|s| !s.connected));
}

// ---------------------------------------------------------------------------
// No token or secret in an IPC payload or a log line
// ---------------------------------------------------------------------------

#[test]
fn no_token_or_secret_reaches_an_ipc_payload_or_a_log_line() {
    let (logs, sink) = capture();
    let dev = tmp("dev-ipc");
    std::fs::write(
        &dev,
        "GOOGLE_CLIENT_ID=dev-google-id\nGOOGLE_CLIENT_SECRET=FIXTURE-WP05-DEV-SECRET\n",
    )
    .unwrap();
    let provider = Arc::new(Provider::default());
    {
        let mut a = provider.answers.lock().unwrap();
        // Sheets exchange (dev file).
        a.push_back(Ok(r#"{"access_token":"FIXTURE-WP05-G-AT","refresh_token":"FIXTURE-WP05-G-RT","expires_in":3600,"scope":"spreadsheets"}"#.to_string()));
    }
    let mgr = ConnectionManager::new(Box::new(ProviderHttp(provider.clone())), dev.clone())
        .with_v2(true)
        .with_log(sink);
    let vault = fresh_vault();
    let mut payloads: Vec<String> = Vec::new();

    // GitHub through the relay: connect, then a failed exchange.
    let relay = FakeRelay::answering(vec![
        Ok(r#"{"access_token":"FIXTURE-WP05-AT","refresh_token":"FIXTURE-WP05-RT","expires_in":3600}"#.to_string()),
        Err(RelayError::Rejected(400)),
    ]);
    let st = mgr
        .exchange_with_plan(
            Service::GitHub,
            &github_plan(),
            &vault,
            "FIXTURE-WP05-CODE",
            "FIXTURE-WP05-VERIFIER",
            Some(&relay),
        )
        .unwrap();
    payloads.push(serde_json::to_string(&st).unwrap());
    let err = mgr
        .exchange_with_plan(
            Service::Notion,
            &ClientPlan::Relay {
                client_id: "n",
                provider: "notion",
            },
            &vault,
            "FIXTURE-WP05-CODE",
            "FIXTURE-WP05-VERIFIER",
            Some(&relay),
        )
        .unwrap_err();
    payloads.push(err.to_string());
    payloads.push(serde_json::to_string(&mgr.status(&vault)).unwrap());

    // Google Sheets through the dev file.
    let plan = mgr.plan(Service::GoogleSheets).unwrap();
    let st = mgr
        .exchange_with_plan(
            Service::GoogleSheets,
            &plan,
            &vault,
            "FIXTURE-WP05-CODE",
            "FIXTURE-WP05-VERIFIER",
            None,
        )
        .unwrap();
    payloads.push(serde_json::to_string(&st).unwrap());
    payloads.push(serde_json::to_string(&mgr.status(&vault)).unwrap());
    assert_eq!(provider.posts.lock().unwrap().len(), 1);

    let logs = logs.lock().unwrap().clone();
    assert!(logs.len() >= 3, "{logs:?}");
    for text in payloads.iter().chain(logs.iter()) {
        assert!(!text.contains(MARK), "credential leaked: {text}");
        assert!(!text.contains("client_secret"), "{text}");
        assert!(!text.to_ascii_lowercase().contains("bearer "), "{text}");
    }
    let _ = std::fs::remove_file(&dev);
}

#[test]
fn the_command_surface_still_returns_only_claim_free_types() {
    // WP05 adds no command and changes no command return type (lanes/L6 section 3.2).
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/connections.rs"));
    let prod = src
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or(src);
    assert_eq!(prod.matches("\n#[tauri::command]\n").count(), 3);
    for module in [
        include_str!("mod.rs"),
        include_str!("providers.rs"),
        include_str!("relay.rs"),
    ] {
        // An attribute on a line of its own (a doc comment may name it).
        assert!(!module.lines().any(|l| l.trim() == "#[tauri::command]"));
    }
}
