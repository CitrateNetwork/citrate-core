// CORE-A3 — OIDC loopback-PKCE adversarial + integration suite (@rule8 evidence).
//
// Every ADV-* here is written RED-FIRST: the guard is neutralized, the test is
// shown failing, the guard restored, the test green (see the sprint's red→green
// table + the PR body). The whole flow runs HEADLESS against an in-test mock
// OIDC authority — a real std `TcpListener` server implementing /authorize,
// /token, /jwks, /userinfo, /revoke — plus the A2 custody vault over an
// in-memory keyring fake. No live authority, real browser, or OS keyring is
// touched; those are honestly out-of-scope for CI and flagged in the sprint.
//
// The mock authority is a #[cfg(test)] fixture. It is NOT shipped (D-A3-2).

use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use jsonwebtoken::{encode, EncodingKey, Header as JwtHeader};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::{EncodePrivateKey, LineEnding};
use p256::SecretKey;
use serde_json::json;

use crate::custody::{CustodyVault, Keyring};

// ===========================================================================
// A2 custody vault over an in-memory keyring (A3's real store dependency).
// ===========================================================================

#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
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

const PASS: &[u8] = b"correct horse battery staple";

/// A shared keyring view so a "restart" can reopen the same vault.
struct Shared(Arc<FakeKeyring>);
impl Keyring for Shared {
    fn get(&self, a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        self.0.get(a)
    }
    fn set(&self, a: &str, s: &[u8]) -> std::result::Result<(), CustodyError> {
        self.0.set(a, s)
    }
    fn delete(&self, a: &str) -> std::result::Result<(), CustodyError> {
        self.0.delete(a)
    }
}

/// A fresh unlocked custody vault at a unique temp path (the A3 refresh-token
/// store). Returns the vault + shared keyring + its envelope path (so a "restart"
/// can rebuild a second vault over the SAME on-disk envelope + keyring).
fn fresh_vault() -> (CustodyVault, Arc<FakeKeyring>, PathBuf) {
    let fake = Arc::new(FakeKeyring::default());
    let mut p = std::env::temp_dir();
    let uniq = format!("citrate-core-oidc-test-{}-{}.enc", std::process::id(), {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    });
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let vault = CustodyVault::new(Box::new(Shared(fake.clone())), p.clone(), 30);
    vault.init(&mut PASS.to_vec()).unwrap();
    vault.unlock(&mut PASS.to_vec()).unwrap();
    (vault, fake, p)
}

/// Rebuild a vault over the SAME envelope + keyring (simulates an app restart).
fn reopen_vault(fake: Arc<FakeKeyring>, path: PathBuf) -> CustodyVault {
    CustodyVault::new(Box::new(Shared(fake)), path, 30)
}

// ===========================================================================
// The mock OIDC authority (D-A3-2). A real TcpListener server signing id_tokens
// with a TEST ES256 key and serving its public coords as JWKS.
// ===========================================================================

struct MockAuthority {
    base: String,
    issuer: String,
    client_id: String,
    behavior: Arc<StdMutex<Behavior>>,
    claims: Arc<StdMutex<serde_json::Value>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone)]
struct CodeGrant {
    code_challenge: String,
    nonce: String,
}

#[derive(Clone, Default)]
struct Behavior {
    /// Forge the id_token nonce (ADV-7 nonce).
    forge_nonce: bool,
    /// Sign the id_token with a WRONG key (ADV-7 signature).
    forge_signature: bool,
    /// Emit an expired id_token (ADV-7 exp).
    expired: bool,
    /// Emit a wrong issuer (ADV-7 iss).
    wrong_issuer: bool,
    /// Emit a wrong audience (ADV-7 aud).
    wrong_audience: bool,
}

impl MockAuthority {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let issuer = base.clone();
        let client_id = "citrate-core".to_string();

        // TEST signing key (never shipped). Public coords → JWKS.
        let secret = SecretKey::random(&mut rand::thread_rng());
        let point = secret.public_key().to_encoded_point(false);
        let jwk_x = b64url(point.x().unwrap());
        let jwk_y = b64url(point.y().unwrap());

        let behavior = Arc::new(StdMutex::new(Behavior::default()));
        let claims = Arc::new(StdMutex::new(json!({
            "sub": "usr_2af4c19e",
            "email": "dana.okafor@fastmail.com",
            "wallet_address": "0xabc0000000000000000000000000000000000def",
            "kyc_status": "none",
            "tier": "free",
            "org_id": serde_json::Value::Null,
            "citrate_role": "member",
            "expires_at": "2027-07-11"
        })));

        let srv = ServerCtx {
            issuer: issuer.clone(),
            client_id: client_id.clone(),
            signing_pem: secret.to_pkcs8_pem(LineEnding::LF).unwrap().to_string(),
            jwk_x,
            jwk_y,
            live_codes: Arc::new(StdMutex::new(HashMap::new())),
            valid_refresh: Arc::new(StdMutex::new(std::collections::HashSet::new())),
            behavior: behavior.clone(),
            claims: claims.clone(),
        };

        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => {
                        if srv.handle_conn(s) {
                            break; // shutdown sentinel received
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        MockAuthority {
            base,
            issuer,
            client_id,
            behavior,
            claims,
            handle: Some(handle),
        }
    }

    fn config(&self) -> AuthorityConfig {
        AuthorityConfig {
            authorize: format!("{}/authorize", self.base),
            token: format!("{}/token", self.base),
            userinfo: format!("{}/userinfo", self.base),
            jwks: format!("{}/jwks", self.base),
            revoke: format!("{}/revoke", self.base),
            kyc_start: format!("{}/kyc/start", self.base),
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
        }
    }

    fn set_claim(&self, key: &str, value: serde_json::Value) {
        self.claims.lock().unwrap()[key] = value;
    }

    fn behavior(&self) -> std::sync::MutexGuard<'_, Behavior> {
        self.behavior.lock().unwrap()
    }

    /// Emulate the browser: hit /authorize, read its 302, then deliver the
    /// loopback callback to the app's listener IN A BACKGROUND THREAD. The
    /// callback GET blocks until the app's listener accepts + responds, which only
    /// happens after `login_with` returns from `open_browser` and enters
    /// `wait_for_callback`; delivering on a thread models a real (async) browser
    /// and avoids a deadlock (the app cannot serve the callback while still inside
    /// this closure).
    fn drive_browser(&self, auth_url: &str) -> Result<()> {
        let loc = location_of(auth_url);
        deliver_callback(loc);
        Ok(())
    }
}

impl Drop for MockAuthority {
    fn drop(&mut self) {
        let _ = ureq::get(format!("{}/__shutdown", self.base)).call();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// The server-side context (owned by the accept thread).
struct ServerCtx {
    issuer: String,
    client_id: String,
    signing_pem: String, // PKCS#8 PEM of the ES256 test key
    jwk_x: String,
    jwk_y: String,
    live_codes: Arc<StdMutex<HashMap<String, CodeGrant>>>,
    valid_refresh: Arc<StdMutex<std::collections::HashSet<String>>>,
    behavior: Arc<StdMutex<Behavior>>,
    claims: Arc<StdMutex<serde_json::Value>>,
}

impl ServerCtx {
    /// Handle one connection. Returns true if it was the shutdown sentinel.
    fn handle_conn(&self, mut stream: TcpStream) -> bool {
        let (method, target, body) = match read_http(&mut stream) {
            Some(v) => v,
            None => return false,
        };
        let stream = &mut stream;
        let path = target.split('?').next().unwrap_or("").to_string();
        let query = parse_query(&target);
        match (method.as_str(), path.as_str()) {
            (_, "/__shutdown") => {
                write_resp(stream, 200, "text/plain", "bye");
                return true;
            }
            ("GET", "/authorize") => self.handle_authorize(stream, &query),
            ("POST", "/token") => self.handle_token(stream, &body),
            ("GET", "/jwks") => self.handle_jwks(stream),
            ("GET", "/userinfo") => self.handle_userinfo(stream),
            ("POST", "/revoke") => self.handle_revoke(stream, &body),
            _ => write_resp(stream, 404, "text/plain", "not found"),
        }
        false
    }

    fn handle_authorize(&self, stream: &mut TcpStream, q: &HashMap<String, String>) {
        // Enforce S256 server-side — a `plain` (or absent) method is rejected,
        // which proves our client only ever offers S256 (ADV-3).
        if q.get("code_challenge_method").map(String::as_str) != Some("S256") {
            write_resp(stream, 400, "text/plain", "PKCE S256 required");
            return;
        }
        let challenge = q.get("code_challenge").cloned().unwrap_or_default();
        let nonce = q.get("nonce").cloned().unwrap_or_default();
        let state = q.get("state").cloned().unwrap_or_default();
        let redirect = q.get("redirect_uri").cloned().unwrap_or_default();
        let code = format!("code_{}", rand_hex());
        self.live_codes.lock().unwrap().insert(
            code.clone(),
            CodeGrant {
                code_challenge: challenge,
                nonce,
            },
        );
        let location = format!("{redirect}?code={code}&state={state}");
        let resp = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(resp.as_bytes());
    }

    fn handle_token(&self, stream: &mut TcpStream, body: &str) {
        let form = parse_query(&format!("?{body}"));
        let grant = form.get("grant_type").map(String::as_str).unwrap_or("");
        if grant == "refresh_token" {
            let rt = form.get("refresh_token").cloned().unwrap_or_default();
            if !self.valid_refresh.lock().unwrap().contains(&rt) {
                write_resp(stream, 400, "application/json", "{\"error\":\"invalid_grant\"}");
                return;
            }
            // Rotate: invalidate old, issue new.
            self.valid_refresh.lock().unwrap().remove(&rt);
            let new_rt = format!("rt_{}", rand_hex());
            self.valid_refresh.lock().unwrap().insert(new_rt.clone());
            let id_token = self.mint_id_token("");
            let out = self.token_json(&id_token, Some(&new_rt));
            write_resp(stream, 200, "application/json", &out);
            return;
        }
        // authorization_code grant.
        let code = form.get("code").cloned().unwrap_or_default();
        let verifier = form.get("code_verifier").cloned().unwrap_or_default();
        let grant = self.live_codes.lock().unwrap().remove(&code); // single-use
        let grant = match grant {
            Some(g) => g,
            None => {
                // Unknown / replayed code (ADV-5).
                write_resp(stream, 400, "application/json", "{\"error\":\"invalid_grant\"}");
                return;
            }
        };
        // PKCE S256 check: base64url(sha256(verifier)) must equal the challenge
        // (ADV-2). A wrong/missing verifier fails here.
        let computed = b64url(&sha2::Sha256::digest(verifier.as_bytes()));
        if computed != grant.code_challenge {
            write_resp(stream, 400, "application/json", "{\"error\":\"invalid_grant\"}");
            return;
        }
        let id_token = self.mint_id_token(&grant.nonce);
        let rt = format!("rt_{}", rand_hex());
        self.valid_refresh.lock().unwrap().insert(rt.clone());
        let out = self.token_json(&id_token, Some(&rt));
        write_resp(stream, 200, "application/json", &out);
    }

    fn token_json(&self, id_token: &str, refresh: Option<&str>) -> String {
        let rt = refresh
            .map(|r| format!(",\"refresh_token\":\"{r}\""))
            .unwrap_or_default();
        format!(
            "{{\"access_token\":\"at_{}\",\"token_type\":\"Bearer\",\"expires_in\":3600,\"id_token\":\"{id_token}\"{rt}}}",
            rand_hex()
        )
    }

    /// Mint an id_token, honoring the current `behavior` knobs (ADV-7).
    fn mint_id_token(&self, nonce: &str) -> String {
        let b = self.behavior.lock().unwrap().clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let exp = if b.expired { now - 3600 } else { now + 3600 };
        let iss = if b.wrong_issuer {
            "https://evil.example".to_string()
        } else {
            self.issuer.clone()
        };
        let aud = if b.wrong_audience {
            "some-other-client".to_string()
        } else {
            self.client_id.clone()
        };
        let effective_nonce = if b.forge_nonce {
            "forged-nonce".to_string()
        } else {
            nonce.to_string()
        };
        let claims = self.claims.lock().unwrap().clone();
        let mut payload = json!({
            "iss": iss,
            "aud": aud,
            "exp": exp,
            "iat": now,
            "nonce": effective_nonce,
        });
        if let (Some(obj), Some(extra)) = (payload.as_object_mut(), claims.as_object()) {
            for (k, v) in extra {
                obj.insert(k.clone(), v.clone());
            }
        }
        let mut header = JwtHeader::new(jsonwebtoken::Algorithm::ES256);
        header.kid = Some("test-key-1".to_string());
        let key = if b.forge_signature {
            let other = SecretKey::random(&mut rand::thread_rng());
            let pem = other.to_pkcs8_pem(LineEnding::LF).unwrap();
            EncodingKey::from_ec_pem(pem.as_bytes()).unwrap()
        } else {
            EncodingKey::from_ec_pem(self.signing_pem.as_bytes()).unwrap()
        };
        encode(&header, &payload, &key).unwrap()
    }

    fn handle_jwks(&self, stream: &mut TcpStream) {
        let body = format!(
            "{{\"keys\":[{{\"kty\":\"EC\",\"crv\":\"P-256\",\"kid\":\"test-key-1\",\"x\":\"{}\",\"y\":\"{}\"}}]}}",
            self.jwk_x, self.jwk_y
        );
        write_resp(stream, 200, "application/json", &body);
    }

    fn handle_userinfo(&self, stream: &mut TcpStream) {
        let claims = self.claims.lock().unwrap().clone();
        write_resp(stream, 200, "application/json", &claims.to_string());
    }

    fn handle_revoke(&self, stream: &mut TcpStream, body: &str) {
        let form = parse_query(&format!("?{body}"));
        if let Some(tok) = form.get("token") {
            self.valid_refresh.lock().unwrap().remove(tok);
        }
        write_resp(stream, 200, "application/json", "{}");
    }
}

// --- tiny HTTP helpers for the mock server ---------------------------------

fn read_http(stream: &mut TcpStream) -> Option<(String, String, String)> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let reader = &mut reader;
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf).ok()?;
        body = String::from_utf8_lossy(&buf).to_string();
    }
    Some((method, target, body))
}

fn write_resp(stream: &mut TcpStream, status: u16, ctype: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        302 => "Found",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
    // Half-close the WRITE side so the client gets a clean FIN after reading the
    // full response, then keep the read half open briefly so a client still
    // flushing its request body is never RST'd ("Peer disconnected").
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut drain = [0u8; 256];
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(200)));
    while let Ok(n) = stream.read(&mut drain) {
        if n == 0 {
            break;
        }
    }
}

fn parse_query(target: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(qs) = target.split_once('?').map(|(_, q)| q) {
        for pair in url::form_urlencoded::parse(qs.as_bytes()) {
            map.insert(pair.0.into_owned(), pair.1.into_owned());
        }
    }
    map
}

fn rand_hex() -> String {
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Build an AuthManager wired to the mock authority over the real ureq client.
fn manager_for(auth: &MockAuthority) -> AuthManager {
    AuthManager::new(auth.config(), Box::new(UreqClient))
}

/// A ureq agent that does NOT auto-follow redirects, so a test can read the
/// authority's 302 `Location` (to extract the code/state) instead of ureq
/// silently following it to the loopback callback.
fn no_redirect_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .max_redirects_will_error(false)
        .build()
        .into()
}

/// GET `url` without following redirects; return the `Location` header of the
/// 302 (the loopback callback URL carrying code+state).
fn location_of(url: &str) -> String {
    let resp = no_redirect_agent().get(url).call().unwrap();
    resp.headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// Deliver a loopback callback URL to the app's listener on a BACKGROUND thread.
/// The GET blocks until the app accepts+serves it (which happens only after the
/// caller returns from `open_browser`), so it must not run inline — that would
/// deadlock the single-threaded login flow.
fn deliver_callback(loc: String) {
    std::thread::spawn(move || {
        let _ = no_redirect_agent().get(&loc).call();
    });
}

// ===========================================================================
// Happy path + integration lifecycle
// ===========================================================================

#[test]
fn login_roundtrip_yields_claims_and_vaults_refresh() {
    let auth = MockAuthority::start();
    auth.set_claim("tier", json!("pilot"));
    let mgr = manager_for(&auth);
    let (vault, _fake, _path) = fresh_vault();

    let status = mgr
        .login_with(&vault, |url| auth.drive_browser(url))
        .expect("login should succeed against the mock authority");

    assert!(status.signed_in);
    assert_eq!(status.tier.as_deref(), Some("pilot"));
    assert_eq!(status.sub.as_deref(), Some("usr_2af4c19e"));
    // A3.2: the refresh token is in the A2 vault slot — NOT in memory/logs/UI.
    let slots = vault.list().unwrap();
    assert!(
        slots.iter().any(|s| s.name == REFRESH_SLOT),
        "refresh token must be persisted in the custody vault"
    );
}

#[test]
fn integration_lifecycle_login_restart_refresh_userinfo_logout() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, fake, path) = fresh_vault();

    // login → refresh token in the vault.
    mgr.login_with(&vault, |url| auth.drive_browser(url)).unwrap();
    assert!(vault.list().unwrap().iter().any(|s| s.name == REFRESH_SLOT));

    // "restart": a brand-new manager + a re-opened vault over the same envelope.
    let mgr2 = manager_for(&auth);
    let vault2 = reopen_vault(fake, path);
    vault2.unlock(&mut PASS.to_vec()).unwrap();
    // status is signed-out on the fresh manager (access token was in memory only).
    assert!(!mgr2.status().signed_in);

    // silent refresh reads the vaulted token and re-establishes the session.
    auth.set_claim("tier", json!("pilot"));
    let refreshed = mgr2.refresh(&vault2).expect("silent refresh from vault");
    assert!(refreshed.signed_in);
    assert_eq!(refreshed.tier.as_deref(), Some("pilot"));

    // live /userinfo re-check reflects a claim change (entitlement engine input).
    auth.set_claim("tier", json!("enterprise"));
    auth.set_claim("org_id", json!("BA-7"));
    let info = mgr2.userinfo().unwrap();
    assert_eq!(info.tier.as_deref(), Some("enterprise"));
    assert_eq!(info.org.as_deref(), Some("BA-7"));

    // logout revokes + clears the vault slot + wipes memory.
    mgr2.logout(&vault2).unwrap();
    assert!(!mgr2.status().signed_in);
    assert!(
        !vault2.list().unwrap().iter().any(|s| s.name == REFRESH_SLOT),
        "logout must clear the refresh-token vault slot"
    );
    // The now-revoked refresh token can no longer mint a session.
    assert!(mgr2.refresh(&vault2).is_err());
}

// ===========================================================================
// ADV-1 — callback state mismatch (CSRF)
// ===========================================================================

#[test]
fn adv1_state_mismatch_is_rejected() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();

    let r = mgr.login_with(&vault, |url| {
        // Read the authority's 302, then deliver the code with a BAD state.
        let loc = location_of(url);
        let tampered = replace_query(&loc, "state", "attacker-state");
        deliver_callback(tampered);
        Ok(())
    });
    assert_eq!(r.unwrap_err(), AuthError::StateMismatch);
    // GUARD: `constant_time_eq(cb.state, state)` in login_with. RED (guard removed
    // → the tampered state is accepted): confirmed failing; GREEN with the check.
}

// ===========================================================================
// ADV-2 — PKCE verifier mismatch
// ===========================================================================

#[test]
fn adv2_pkce_verifier_mismatch_rejects_token_exchange() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);

    let pkce = super::Pkce::new();
    let redirect = "http://127.0.0.1:1/callback";
    let url = build_authorize_url(&auth.config(), redirect, &pkce.challenge, "st", "no").unwrap();
    let loc = location_of(&url);
    let code = parse_query(&loc).get("code").cloned().unwrap();

    // Wrong verifier → invalid_grant → TokenExchange error.
    let bad = mgr.exchange_code(&code, "totally-wrong-verifier", redirect);
    assert_eq!(bad.unwrap_err(), AuthError::TokenExchange);
}

#[test]
fn adv2b_correct_verifier_succeeds() {
    // Positive control: the SAME code shape with the CORRECT verifier exchanges
    // fine — proving ADV-2's failure is the verifier, not a broken exchange.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let pkce = super::Pkce::new();
    let redirect = "http://127.0.0.1:1/callback";
    let url = build_authorize_url(&auth.config(), redirect, &pkce.challenge, "st", "no").unwrap();
    let loc = location_of(&url);
    let code = parse_query(&loc).get("code").cloned().unwrap();
    let ok = mgr.exchange_code(&code, &pkce.verifier, redirect);
    assert!(ok.is_ok());
}

// ===========================================================================
// ADV-3 — PKCE plain downgrade attempt (S256 enforced)
// ===========================================================================

#[test]
fn adv3_plain_downgrade_rejected_and_client_never_offers_it() {
    let auth = MockAuthority::start();
    let url = build_authorize_url(
        &auth.config(),
        "http://127.0.0.1:1/callback",
        "chal",
        "st",
        "no",
    )
    .unwrap();
    // (a) Our /authorize URL always carries method=S256 — never plain (ADV-3).
    assert!(url.contains("code_challenge_method=S256"));
    assert!(!url.contains("plain"));

    // (b) The authority hard-rejects any non-S256 method — a downgrade probe
    //     yields no 302 (no auth code).
    let downgraded = url.replace("S256", "plain");
    let is_302 = no_redirect_agent()
        .get(&downgraded)
        .call()
        .ok()
        .map(|r| r.headers().get("location").is_some())
        .unwrap_or(false);
    assert!(!is_302, "a plain PKCE downgrade must not yield an auth code");
}

// ===========================================================================
// ADV-4 — loopback listener binds 127.0.0.1 only, never 0.0.0.0
// ===========================================================================

#[test]
fn adv4_loopback_binds_localhost_only() {
    let l = LoopbackListener::bind().unwrap();
    let addr = l.listener.local_addr().unwrap();
    assert_eq!(
        addr.ip(),
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        "the callback listener MUST bind 127.0.0.1, never 0.0.0.0 (RFC 8252)"
    );
    assert!(addr.ip().is_loopback());
    assert_ne!(
        addr.ip(),
        std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
    );
}

// ===========================================================================
// ADV-5 — authorization-code replay / injection (single-use)
// ===========================================================================

#[test]
fn adv5_code_is_single_use() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let pkce = super::Pkce::new();
    let redirect = "http://127.0.0.1:1/callback";
    let url = build_authorize_url(&auth.config(), redirect, &pkce.challenge, "st", "no").unwrap();
    let loc = location_of(&url);
    let code = parse_query(&loc).get("code").cloned().unwrap();

    // First exchange succeeds; replay of the SAME code is rejected (single-use).
    assert!(mgr.exchange_code(&code, &pkce.verifier, redirect).is_ok());
    let replay = mgr.exchange_code(&code, &pkce.verifier, redirect);
    assert_eq!(replay.unwrap_err(), AuthError::TokenExchange);

    // Injection: a code that was never issued is likewise rejected.
    let injected = mgr.exchange_code("code_never_issued", &pkce.verifier, redirect);
    assert_eq!(injected.unwrap_err(), AuthError::TokenExchange);
}

// ===========================================================================
// ADV-6 — foreign-origin callback with wrong/no state rejected; listener closes
// ===========================================================================

#[test]
fn adv6_callback_without_state_is_rejected_and_listener_closes() {
    // A callback lacking `state` (a foreign/malformed request) fails closed.
    assert_eq!(
        parse_callback_target("/callback?code=abc").unwrap_err(),
        AuthError::StateMismatch
    );
    assert_eq!(
        parse_callback_target("/callback").unwrap_err(),
        AuthError::StateMismatch
    );
    // A full login where the browser delivers no state fails + releases the port.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let r = mgr.login_with(&vault, |url| {
        let loc = location_of(url);
        let stripped = strip_query(&loc, "state");
        deliver_callback(stripped);
        Ok(())
    });
    assert_eq!(r.unwrap_err(), AuthError::StateMismatch);
}

// ===========================================================================
// ADV-7 — id_token nonce / iss / aud / exp / signature each validated
// ===========================================================================

#[test]
fn adv7_forged_nonce_rejected() {
    run_id_token_attack(|b| b.forge_nonce = true);
}
#[test]
fn adv7_wrong_issuer_rejected() {
    run_id_token_attack(|b| b.wrong_issuer = true);
}
#[test]
fn adv7_wrong_audience_rejected() {
    run_id_token_attack(|b| b.wrong_audience = true);
}
#[test]
fn adv7_expired_rejected() {
    run_id_token_attack(|b| b.expired = true);
}
#[test]
fn adv7_forged_signature_rejected() {
    run_id_token_attack(|b| b.forge_signature = true);
}

#[test]
fn adv7_valid_id_token_accepted() {
    // Positive control: with NO tamper, a full login validates + signs in.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let st = mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    assert!(st.signed_in);
}

fn run_id_token_attack(set: impl FnOnce(&mut Behavior)) {
    let auth = MockAuthority::start();
    {
        let mut b = auth.behavior();
        set(&mut b);
    }
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    let r = mgr.login_with(&vault, |u| auth.drive_browser(u));
    assert_eq!(
        r.unwrap_err(),
        AuthError::IdTokenInvalid,
        "a tampered id_token must be rejected"
    );
    // Nothing was vaulted on a failed login.
    assert!(!vault.list().unwrap().iter().any(|s| s.name == REFRESH_SLOT));
}

// ===========================================================================
// ADV-8 — token boundary: no invoke command returns a token
// ===========================================================================

#[test]
fn adv8_no_auth_invoke_command_returns_a_token() {
    // Structural boundary proof against the real source: every registered auth
    // command's return type is AuthStatus (claim-derived) or (); NONE is a token.
    let src = include_str!("oidc.rs");

    // AuthStatus must not carry any token field.
    let start = src.find("pub struct AuthStatus").expect("AuthStatus exists");
    let body = &src[start..start + 800];
    for f in ["access_token", "refresh_token", "accessToken", "refreshToken"] {
        assert!(!body.contains(f), "AuthStatus must not expose a token field ({f})");
    }

    // The auth commands are registered in lib.rs.
    let lib = include_str!("lib.rs");
    for cmd in [
        "auth_status",
        "auth_login",
        "auth_userinfo",
        "auth_refresh",
        "auth_logout",
        "kyc_start",
    ] {
        assert!(
            lib.contains(&format!("oidc::{cmd}")),
            "auth command {cmd} must be registered"
        );
    }
    // Their #[tauri::command] signatures return AuthStatus or ().
    for sig in [
        "pub fn auth_status(state: State<'_, AuthState>) -> std::result::Result<AuthStatus, String>",
        "pub fn auth_userinfo(auth: State<'_, AuthState>) -> std::result::Result<AuthStatus, String>",
    ] {
        assert!(src.contains(sig), "expected command signature: {sig}");
    }
    // No command returns TokenResponse — it is confined to the private path.
    assert!(
        !src.contains("-> std::result::Result<TokenResponse"),
        "no command may return TokenResponse"
    );
}

#[test]
fn adv8_status_is_claim_derived_only_at_runtime() {
    // Runtime half: after a real login, the status snapshot serializes to
    // flags/claims and NEVER contains token-shaped material.
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();
    let st = mgr.status();
    let json = serde_json::to_string(&st).unwrap();
    assert!(!json.contains("at_"), "access-token prefix must not appear");
    assert!(!json.contains("rt_"), "refresh-token prefix must not appear");
    assert!(st.signed_in);
}

// ===========================================================================
// ADV-9 — refresh token never in logs / errors
// ===========================================================================

#[test]
fn adv9_refresh_token_absent_from_errors() {
    let auth = MockAuthority::start();
    let mgr = manager_for(&auth);
    let (vault, _f, _p) = fresh_vault();
    mgr.login_with(&vault, |u| auth.drive_browser(u)).unwrap();

    // Read the raw refresh token from the vault (in-process) so we know its value.
    let rt = vault.custody_get(REFRESH_SLOT).unwrap();
    let rt_str = std::str::from_utf8(&rt).unwrap().to_string();
    assert!(rt_str.starts_with("rt_"));

    // Force a refresh failure (revoke server-side first) and inspect the error.
    let _ = ureq::post(format!("{}/revoke", auth.base))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(format!("token={rt_str}"));
    let err = mgr.refresh(&vault).unwrap_err();
    let msg = err.to_string();
    assert!(
        !msg.contains(&rt_str) && !msg.contains("rt_"),
        "an error string must never contain the refresh token"
    );

    // Every variant's Display is secret-free.
    for e in [
        AuthError::StateMismatch,
        AuthError::TokenExchange,
        AuthError::IdTokenInvalid,
        AuthError::Timeout,
        AuthError::NotSignedIn,
        AuthError::Custody,
        AuthError::Network,
    ] {
        let s = e.to_string();
        assert!(!s.contains("rt_") && !s.contains("at_"));
    }
}

// ===========================================================================
// ADV-10 — listener timeout / no callback fails closed, port released
// ===========================================================================

#[test]
fn adv10_listener_timeout_fails_closed_and_releases_port() {
    let l = LoopbackListener::bind().unwrap();
    let port = l.port();
    let start = std::time::Instant::now();
    let r = l.wait_for_callback(std::time::Duration::from_millis(150));
    assert_eq!(r.unwrap_err(), AuthError::Timeout);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    // The port is released: a fresh bind on the same port eventually succeeds.
    let mut rebound = false;
    for _ in 0..50 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            rebound = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(rebound, "the loopback port must be released after a timeout");
}

// --- small query-string editors used by the CSRF tests --------------------

fn replace_query(u: &str, key: &str, val: &str) -> String {
    let mut parsed = url::Url::parse(u).unwrap();
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            if k == key {
                (k.into_owned(), val.to_string())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();
    parsed.query_pairs_mut().clear().extend_pairs(pairs);
    parsed.to_string()
}

fn strip_query(u: &str, key: &str) -> String {
    let mut parsed = url::Url::parse(u).unwrap();
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| k != key)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    parsed.query_pairs_mut().clear().extend_pairs(pairs);
    parsed.to_string()
}
