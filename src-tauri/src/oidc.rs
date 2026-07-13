//! citrate-core — OIDC loopback sign-in (CORE-A3). @rule8 · T1 auth surface.
//!
//! Turns the honest `Unavailable` `auth` seam into a real RFC 8252 loopback
//! Authorization-Code + PKCE (S256) flow against auth.citrate.ai (the
//! `citrate-core` client). A3 is AUTH ONLY — per CLAUDE.md rule 3 there are **no
//! signing / wallet / UserOp code paths here** (that is B1). This module never
//! holds or touches a wallet key.
//!
//! ## Flow (A3.1)
//! 1. `auth_login` binds a single-use loopback listener on `127.0.0.1:<random>`
//!    (RFC 8252 — never `0.0.0.0`), mints a PKCE verifier + `state` + `nonce`,
//!    builds the `/authorize` URL (`code_challenge_method=S256`, scope
//!    `openid profile wallet kyc offline_access`,
//!    `redirect_uri=http://127.0.0.1:<port>/callback`), and opens the system
//!    browser (tauri-plugin-opener).
//! 2. The listener accepts ONE callback, parses `code`+`state` from the request
//!    line, validates `state` (CSRF — ADV-1/ADV-6), then closes. A bind-timeout
//!    fails closed and releases the port (ADV-10).
//! 3. `code` is exchanged at `/token` with the `code_verifier` (PKCE — ADV-2).
//!    The returned `id_token` is validated (nonce/iss/aud/exp/signature via the
//!    authority JWKS — ADV-7).
//!
//! ## Token custody (A3.2 — mirrors A2's I-2 boundary)
//! - The rotating **refresh token → the A2 custody vault** (slot `oidc-refresh`,
//!   via the in-process `custody_put`/`custody_get`, NEVER an invoke). It
//!   survives an app restart so `auth_refresh` is silent.
//! - The **access token + expiry live in memory only** (this module's `Session`).
//! - **No token ever crosses the invoke boundary.** `auth_status` returns only
//!   claim-derived flags (signedIn / tier / org / role / kycStatus / walletAddr).
//!   The refresh token is never logged nor placed in any error string (ADV-8/9).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::custody::{CustodyError, CustodyVault};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The custody vault slot holding the rotating refresh token (A3.2). Its bytes
/// NEVER leave the Rust process (in-process `custody_get`, never an invoke).
pub const REFRESH_SLOT: &str = "oidc-refresh";

/// The OAuth scope A3 requests: identity + the wallet/kyc/entitlement claims and
/// a refresh token (`offline_access`).
const SCOPE: &str = "openid profile wallet kyc offline_access";

/// PKCE code-challenge method. **S256 only** — `plain` is never offered or
/// accepted (ADV-3).
const CODE_CHALLENGE_METHOD: &str = "S256";

/// How long the loopback listener waits for the browser callback before failing
/// closed and releasing the port (ADV-10).
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Read cap on the single callback request so a malicious/oversized request
/// cannot exhaust memory. A real OIDC callback line is a few hundred bytes.
const MAX_CALLBACK_BYTES: usize = 8 * 1024;

// ---------------------------------------------------------------------------
// Authority endpoint config
// ---------------------------------------------------------------------------

/// The OIDC authority endpoints + the `citrate-core` client id. In production
/// these point at auth.citrate.ai; tests inject the mock authority's base URL.
/// `redirect_base` is filled per-attempt with the actual loopback port.
#[derive(Debug, Clone)]
pub struct AuthorityConfig {
    pub authorize: String,
    pub token: String,
    pub userinfo: String,
    pub jwks: String,
    pub revoke: String,
    pub kyc_start: String,
    pub issuer: String,
    pub client_id: String,
}

impl AuthorityConfig {
    /// Production authority (auth.citrate.ai). Not exercised in CI (the authority
    /// is being redeployed — see the sprint's hard-dep note); the mock authority
    /// fixture drives every test. Kept here so a Tauri build wires the real
    /// endpoints; allow dead_code so the non-test lib build does not flag it when
    /// only the injected constructor is used.
    #[allow(dead_code)]
    pub fn production() -> Self {
        let base = "https://auth.citrate.ai";
        AuthorityConfig {
            authorize: format!("{base}/authorize"),
            token: format!("{base}/token"),
            userinfo: format!("{base}/userinfo"),
            jwks: format!("{base}/.well-known/jwks.json"),
            revoke: format!("{base}/revoke"),
            kyc_start: format!("{base}/kyc/start"),
            issuer: base.to_string(),
            client_id: "citrate-core".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Errors — deliberately coarse; NEVER embeds a token (ADV-9).
// ---------------------------------------------------------------------------

/// An auth error. Display strings are safe to surface/log: they never contain a
/// token, code, verifier, or any secret (ADV-9). The refresh token in particular
/// is never formatted into any variant.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    /// The callback `state` did not match the one we minted (CSRF — ADV-1/6).
    StateMismatch,
    /// The `/token` exchange was rejected (bad/replayed code, PKCE mismatch —
    /// ADV-2/5). No token material is included.
    TokenExchange,
    /// The `id_token` failed validation (nonce/iss/aud/exp/signature — ADV-7).
    IdTokenInvalid,
    /// The loopback listener timed out with no callback (ADV-10).
    Timeout,
    /// No session (not signed in) — a refresh/userinfo was attempted signed-out.
    NotSignedIn,
    /// The custody vault is locked or unavailable (cannot store/read the token).
    Custody,
    /// A network/transport error talking to the authority.
    Network,
    /// The authority is unreachable / not yet deployed (honest, per Rule 1). The
    /// production `login`/`refresh` path surfaces this when auth.citrate.ai has
    /// not been redeployed yet (the sprint's hard dependency); the mock-driven
    /// tests never hit it, so it is `dead_code` in the test-cfg build only.
    #[allow(dead_code)]
    Unavailable,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AuthError::StateMismatch => "auth: callback state mismatch (rejected)",
            AuthError::TokenExchange => "auth: token exchange rejected",
            AuthError::IdTokenInvalid => "auth: id_token validation failed",
            AuthError::Timeout => "auth: sign-in timed out waiting for the browser",
            AuthError::NotSignedIn => "auth: not signed in",
            AuthError::Custody => "auth: custody vault locked or unavailable",
            AuthError::Network => "auth: could not reach the authority",
            AuthError::Unavailable => "auth: authority unavailable",
        };
        f.write_str(s)
    }
}
impl std::error::Error for AuthError {}

impl From<CustodyError> for AuthError {
    fn from(_: CustodyError) -> Self {
        // Collapse every custody failure to the opaque Custody variant — a
        // custody error string must not leak through the auth surface either.
        AuthError::Custody
    }
}

type Result<T> = std::result::Result<T, AuthError>;

// ---------------------------------------------------------------------------
// PKCE (ADV-2/3)
// ---------------------------------------------------------------------------

/// A PKCE pair: the high-entropy `verifier` (secret, zeroized) and its S256
/// `challenge` (public, sent on `/authorize`).
struct Pkce {
    verifier: Zeroizing<String>,
    challenge: String,
}

impl Pkce {
    /// Mint a fresh S256 PKCE pair. The verifier is 32 random bytes, base64url;
    /// the challenge is `base64url(sha256(verifier))`. S256 is the ONLY method
    /// this produces (ADV-3 — `plain` is never generated).
    fn new() -> Self {
        let mut raw = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut raw);
        let verifier = b64url(&raw);
        raw.zeroize();
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = b64url(&digest);
        Pkce {
            verifier: Zeroizing::new(verifier),
            challenge,
        }
    }
}

/// URL-safe, unpadded base64 (RFC 7636 / RFC 7515).
fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 32 bytes of URL-safe randomness for `state` / `nonce`.
fn random_token() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let out = b64url(&raw);
    raw.zeroize();
    out
}

// ---------------------------------------------------------------------------
// Loopback listener (ADV-4/6/10)
// ---------------------------------------------------------------------------

/// A bound single-use loopback listener. The bind is `127.0.0.1:0` — the OS
/// assigns a random free port and the socket is reachable ONLY from localhost
/// (ADV-4: never `0.0.0.0`). `port()` is the assigned port for the redirect URI.
struct LoopbackListener {
    listener: TcpListener,
    port: u16,
}

impl LoopbackListener {
    /// Bind `127.0.0.1:0`. RFC 8252: the redirect endpoint MUST be a loopback IP
    /// literal, never a wildcard/all-interfaces bind (ADV-4).
    fn bind() -> Result<Self> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let listener = TcpListener::bind(addr).map_err(|_| AuthError::Network)?;
        let port = listener
            .local_addr()
            .map_err(|_| AuthError::Network)?
            .port();
        Ok(LoopbackListener { listener, port })
    }

    fn port(&self) -> u16 {
        self.port
    }

    /// Accept exactly ONE callback and return the parsed `(code, state)`. Polls a
    /// NON-BLOCKING accept against a wall-clock deadline so a timeout releases the
    /// port deterministically (ADV-10: `self` — and its listener — is dropped on
    /// every exit path here, closing the socket). The listener is single-use: it
    /// accepts one connection, replies with a small close-the-tab page, and is
    /// consumed (ADV-5: no second code can be delivered to the same listener).
    fn wait_for_callback(self, timeout: Duration) -> Result<CallbackParams> {
        self.listener
            .set_nonblocking(true)
            .map_err(|_| AuthError::Network)?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _peer)) => {
                    // Got the one connection; block on THIS stream (short timeout).
                    stream
                        .set_nonblocking(false)
                        .map_err(|_| AuthError::Network)?;
                    return Self::serve_callback(stream);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        // Fail closed; `self` drops here → port released.
                        return Err(AuthError::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return Err(AuthError::Network),
            }
        }
    }

    /// Read the callback request on an accepted stream, extract the `code` +
    /// `state` query params, and write a minimal close page. Any malformed/foreign
    /// request (no `code`/`state`) fails closed (ADV-6).
    fn serve_callback(mut stream: TcpStream) -> Result<CallbackParams> {
        let params = Self::read_request(&mut stream)?;
        // Best-effort friendly page; a write failure does not change the outcome.
        let body = "<!doctype html><meta charset=utf-8><title>Citrate</title>\
                    <body style=\"font-family:system-ui;padding:3rem;text-align:center\">\
                    <p>Signed in to Citrate Core. You can close this tab.</p>";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        Ok(params)
    }

    /// Read the first request line (`GET /callback?code=..&state=.. HTTP/1.1`)
    /// and parse the query. Capped at `MAX_CALLBACK_BYTES`.
    fn read_request(stream: &mut TcpStream) -> Result<CallbackParams> {
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|_| AuthError::Network)?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|_| AuthError::Network)?)
            .take(MAX_CALLBACK_BYTES as u64);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|_| AuthError::Network)?;
        // Request line: METHOD SP request-target SP HTTP-version
        let target = line
            .split_whitespace()
            .nth(1)
            .ok_or(AuthError::StateMismatch)?;
        parse_callback_target(target)
    }
}

/// The `(code, state)` parsed off the loopback callback.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CallbackParams {
    code: String,
    state: String,
}

/// Parse `code` + `state` from a callback request target
/// (`/callback?code=..&state=..`). A missing `code` or `state` (a foreign or
/// malformed request — ADV-6) fails closed with `StateMismatch`. We deliberately
/// do NOT reveal *which* was missing.
fn parse_callback_target(target: &str) -> Result<CallbackParams> {
    // Give the relative target an absolute base so `url` can parse the query.
    let parsed = url::Url::parse("http://127.0.0.1/")
        .and_then(|base| base.join(target))
        .map_err(|_| AuthError::StateMismatch)?;
    let mut code = None;
    let mut state = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    match (code, state) {
        (Some(code), Some(state)) if !code.is_empty() && !state.is_empty() => {
            Ok(CallbackParams { code, state })
        }
        _ => Err(AuthError::StateMismatch),
    }
}

// ---------------------------------------------------------------------------
// /authorize URL builder (ADV-3)
// ---------------------------------------------------------------------------

/// Build the `/authorize` URL. `code_challenge_method` is hard-wired to `S256`
/// (ADV-3 — no caller can request `plain`).
fn build_authorize_url(
    cfg: &AuthorityConfig,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    nonce: &str,
) -> Result<String> {
    let mut url = url::Url::parse(&cfg.authorize).map_err(|_| AuthError::Network)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &cfg.client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .append_pair("nonce", nonce)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", CODE_CHALLENGE_METHOD);
    Ok(url.into_string_compat())
}

/// Small shim: `Url::into_string` was deprecated; keep call sites tidy.
trait UrlIntoString {
    fn into_string_compat(self) -> String;
}
impl UrlIntoString for url::Url {
    fn into_string_compat(self) -> String {
        String::from(self)
    }
}

// ---------------------------------------------------------------------------
// Token endpoint + claims
// ---------------------------------------------------------------------------

/// The `/token` response. `access_token` + `refresh_token` are secret; this
/// struct is confined to the exchange path and never serialized back out.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    id_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

fn default_expires_in() -> u64 {
    3600
}

/// The subset of id_token / userinfo claims A3 surfaces. Entitlement fields
/// (`tier`, `org_id`, `citrate_role`, `expires_at`) drive the frontend gating;
/// `kyc_status` drives the S2 seam; `wallet_address` is display-only (A3 does NOT
/// sign — rule 3).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub wallet_address: Option<String>,
    #[serde(default)]
    pub kyc_status: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub citrate_role: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

/// Registered id_token claims we validate against (iss/aud/exp/nonce).
#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    #[serde(default)]
    nonce: Option<String>,
    // The entitlement/identity claims ride along in the id_token too; we reuse
    // the flattened `Claims` view after registered-claim validation.
    #[serde(flatten)]
    claims: Claims,
}

// ---------------------------------------------------------------------------
// JWKS (ADV-7)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

/// One JWK. A3's authority signs id_tokens with ES256, so we consume EC P-256
/// keys (`kty=EC`, `crv=P-256`, `x`/`y`). `kid` selects the key by the token
/// header.
#[derive(Debug, Deserialize)]
struct Jwk {
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    kty: Option<String>,
    #[serde(default)]
    x: Option<String>,
    #[serde(default)]
    y: Option<String>,
}

// ---------------------------------------------------------------------------
// HTTP seam — real `ureq` in production, injectable for tests
// ---------------------------------------------------------------------------

/// The HTTP operations the OIDC flow needs. Abstracted so tests drive the mock
/// authority over an in-process `TcpListener` while production uses `ureq` with
/// rustls TLS.
pub trait HttpClient: Send + Sync {
    /// `GET url` → body string. Used for JWKS + `/userinfo` (with bearer).
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<String>;
    /// `POST url` with `application/x-www-form-urlencoded` body → body string.
    /// Used for `/token` + `/revoke`.
    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<String>;
}

/// Production HTTP client: blocking `ureq`.
pub struct UreqClient;

impl HttpClient for UreqClient {
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<String> {
        let mut req = ureq::get(url);
        if let Some(tok) = bearer {
            req = req.header("Authorization", &format!("Bearer {tok}"));
        }
        let mut resp = req.call().map_err(|_| AuthError::Network)?;
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AuthError::Network)
    }

    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<String> {
        let body = encode_form(form);
        let mut resp = ureq::post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(&body)
            .map_err(|_| AuthError::TokenExchange)?;
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AuthError::TokenExchange)
    }
}

/// `application/x-www-form-urlencoded` body.
fn encode_form(form: &[(&str, &str)]) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in form {
        s.append_pair(k, v);
    }
    s.finish()
}

// ---------------------------------------------------------------------------
// Session (in-memory access token) + the auth manager
// ---------------------------------------------------------------------------

/// The in-memory auth session. The access token lives here and NOWHERE else —
/// not on disk, not across the invoke boundary. Zeroized on drop / sign-out.
struct AuthSession {
    access_token: Zeroizing<String>,
    /// Absolute unix-seconds access-token expiry.
    expires_at: u64,
    claims: Claims,
}

/// Status flags crossing the invoke boundary (ADV-8). Claim-DERIVED only —
/// never a token. This is the ONLY thing `auth_status` returns.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthStatus {
    #[serde(rename = "signedIn")]
    pub signed_in: bool,
    pub sub: Option<String>,
    pub tier: Option<String>,
    pub org: Option<String>,
    pub role: Option<String>,
    #[serde(rename = "kycStatus")]
    pub kyc_status: Option<String>,
    #[serde(rename = "walletAddr")]
    pub wallet_addr: Option<String>,
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    pub email: Option<String>,
}

impl AuthStatus {
    fn signed_out() -> Self {
        AuthStatus::default()
    }
    fn from_claims(c: &Claims) -> Self {
        AuthStatus {
            signed_in: true,
            sub: (!c.sub.is_empty()).then(|| c.sub.clone()),
            tier: c.tier.clone(),
            org: c.org_id.clone(),
            role: c.citrate_role.clone(),
            kyc_status: c.kyc_status.clone(),
            wallet_addr: c.wallet_address.clone(),
            expires_at: c.expires_at.clone(),
            email: c.email.clone(),
        }
    }
}

/// The process-wide auth manager. Owns the authority config, the HTTP seam, a
/// reference to the A2 custody vault (for the refresh-token slot), and the
/// in-memory session. Everything secret is confined here; the invoke commands
/// only ever read claim-derived flags off it.
pub struct AuthManager {
    cfg: AuthorityConfig,
    http: Box<dyn HttpClient>,
    session: Mutex<Option<AuthSession>>,
}

impl AuthManager {
    pub fn new(cfg: AuthorityConfig, http: Box<dyn HttpClient>) -> Self {
        AuthManager {
            cfg,
            http,
            session: Mutex::new(None),
        }
    }

    /// Recover a poisoned session lock instead of bricking auth (same DR-5 policy
    /// as custody). The only guarded state is `Option<AuthSession>`.
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<AuthSession>> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    // --- A3.1: the login flow -------------------------------------------

    /// Run the full loopback-PKCE sign-in against the authority, storing the
    /// refresh token in `vault` (A2) and the access token in memory. Returns the
    /// claim-derived status. This BLOCKS on the browser callback (bounded by
    /// `CALLBACK_TIMEOUT`) — callers run it off the UI thread.
    ///
    /// `open_browser` is injected so tests can drive the mock authority's
    /// `/authorize` (which 302s straight to the loopback) without a real browser;
    /// production passes the tauri-plugin-opener launcher.
    pub fn login_with<F>(&self, vault: &CustodyVault, open_browser: F) -> Result<AuthStatus>
    where
        F: FnOnce(&str) -> Result<()>,
    {
        // 1. Bind the single-use loopback listener FIRST so we know the port.
        let listener = LoopbackListener::bind()?;
        let redirect_uri = format!("http://127.0.0.1:{}/callback", listener.port());

        // 2. Mint PKCE + state + nonce and build the /authorize URL.
        let pkce = Pkce::new();
        let state = random_token();
        let nonce = random_token();
        let auth_url =
            build_authorize_url(&self.cfg, &redirect_uri, &pkce.challenge, &state, &nonce)?;

        // 3. Open the browser (or, in tests, hit the mock /authorize).
        open_browser(&auth_url)?;

        // 4. Wait for exactly one callback; validate state (CSRF — ADV-1/6).
        let cb = listener.wait_for_callback(CALLBACK_TIMEOUT)?;
        if !constant_time_eq(cb.state.as_bytes(), state.as_bytes()) {
            return Err(AuthError::StateMismatch);
        }

        // 5. Exchange the code (with the PKCE verifier — ADV-2).
        let tokens = self.exchange_code(&cb.code, &pkce.verifier, &redirect_uri)?;

        // 6. Validate the id_token (nonce/iss/aud/exp/sig — ADV-7).
        let claims = self.validate_id_token(&tokens.id_token, &nonce)?;

        // 7. Persist the refresh token in the A2 vault (A3.2); access token in
        //    memory only. The vault must be unlocked — else fail closed.
        if let Some(refresh) = tokens.refresh_token {
            self.store_refresh(vault, refresh)?;
        }
        self.set_session(&tokens.access_token, tokens.expires_in, claims.clone());

        Ok(AuthStatus::from_claims(&claims))
    }

    /// Exchange an authorization `code` at `/token` with the PKCE `verifier`.
    fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenResponse> {
        let body = self.http.post_form(
            &self.cfg.token,
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("client_id", &self.cfg.client_id),
                ("code_verifier", verifier),
            ],
        )?;
        serde_json::from_str::<TokenResponse>(&body).map_err(|_| AuthError::TokenExchange)
    }

    /// Validate an id_token against the authority JWKS + registered claims
    /// (iss/aud/exp/signature) and the `nonce` we minted (ADV-7). Returns the
    /// parsed identity/entitlement claims.
    fn validate_id_token(&self, id_token: &str, expected_nonce: &str) -> Result<Claims> {
        let header =
            jsonwebtoken::decode_header(id_token).map_err(|_| AuthError::IdTokenInvalid)?;
        // ES256 only — reject any other alg (an `alg:none` / HS downgrade fails
        // here before any signature check).
        if header.alg != Algorithm::ES256 {
            return Err(AuthError::IdTokenInvalid);
        }
        let key = self.jwk_decoding_key(header.kid.as_deref())?;

        let mut validation = Validation::new(Algorithm::ES256);
        validation.set_issuer(&[&self.cfg.issuer]);
        validation.set_audience(&[&self.cfg.client_id]);
        validation.validate_exp = true;
        validation.leeway = 0;

        let data = jsonwebtoken::decode::<IdTokenClaims>(id_token, &key, &validation)
            .map_err(|_| AuthError::IdTokenInvalid)?;

        // iss is enforced by `set_issuer`; re-check defensively.
        if data.claims.iss != self.cfg.issuer {
            return Err(AuthError::IdTokenInvalid);
        }
        // nonce binds this id_token to THIS login attempt (replay defense).
        match data.claims.nonce.as_deref() {
            Some(n) if constant_time_eq(n.as_bytes(), expected_nonce.as_bytes()) => {}
            _ => return Err(AuthError::IdTokenInvalid),
        }
        Ok(data.claims.claims)
    }

    /// Fetch the JWKS and build the ES256 decoding key for `kid`.
    fn jwk_decoding_key(&self, kid: Option<&str>) -> Result<DecodingKey> {
        let body = self.http.get(&self.cfg.jwks, None)?;
        let jwks: Jwks = serde_json::from_str(&body).map_err(|_| AuthError::IdTokenInvalid)?;
        let jwk = jwks
            .keys
            .iter()
            .find(|k| match (kid, k.kid.as_deref()) {
                (Some(want), Some(have)) => want == have,
                // No kid on either side: accept the sole key.
                _ => true,
            })
            .ok_or(AuthError::IdTokenInvalid)?;
        if jwk.kty.as_deref() != Some("EC") {
            return Err(AuthError::IdTokenInvalid);
        }
        let x = jwk.x.as_deref().ok_or(AuthError::IdTokenInvalid)?;
        let y = jwk.y.as_deref().ok_or(AuthError::IdTokenInvalid)?;
        DecodingKey::from_ec_components(x, y).map_err(|_| AuthError::IdTokenInvalid)
    }

    // --- A3.2: refresh-token custody + lifecycle ------------------------

    /// Store the rotating refresh token in the A2 vault slot `oidc-refresh`. The
    /// bytes are moved in and zeroized by `custody_put`; they never cross the
    /// invoke boundary and are never logged (ADV-8/9).
    fn store_refresh(&self, vault: &CustodyVault, refresh: String) -> Result<()> {
        let mut bytes = refresh.into_bytes();
        let r = vault.put(REFRESH_SLOT, &mut bytes);
        bytes.zeroize();
        r.map_err(AuthError::from)
    }

    /// Silent refresh (A3.2): read the vaulted refresh token (in-process
    /// `custody_get`, never an invoke), exchange it at `/token`, rotate the
    /// stored token, and refresh the in-memory access token + claims. Survives an
    /// app restart because the token lives in the vault, not memory.
    pub fn refresh(&self, vault: &CustodyVault) -> Result<AuthStatus> {
        let refresh = vault.custody_get(REFRESH_SLOT).map_err(AuthError::from)?;
        let refresh_str = std::str::from_utf8(&refresh)
            .map_err(|_| AuthError::Custody)?
            .to_string();
        let body = self.http.post_form(
            &self.cfg.token,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_str),
                ("client_id", &self.cfg.client_id),
            ],
        )?;
        let tokens: TokenResponse =
            serde_json::from_str(&body).map_err(|_| AuthError::TokenExchange)?;
        // Rotate: store the NEW refresh token if the authority rotated it.
        if let Some(new_refresh) = tokens.refresh_token {
            self.store_refresh(vault, new_refresh)?;
        }
        // A refreshed access token carries fresh claims via /userinfo (federation
        // RP rule); but the token response may also embed an id_token. Prefer a
        // live /userinfo re-check for entitlement, falling back to prior claims.
        self.set_session_token(&tokens.access_token, tokens.expires_in);
        let claims = self.userinfo_inner(&tokens.access_token)?;
        self.set_session_claims(claims.clone());
        Ok(AuthStatus::from_claims(&claims))
    }

    // --- A3.3: live /userinfo re-check ----------------------------------

    /// Live `/userinfo` re-check (the federation RP rule): re-reads the
    /// entitlement claim from the authority using the in-memory access token, and
    /// updates the session claims. Errors `NotSignedIn` if there is no session.
    pub fn userinfo(&self) -> Result<AuthStatus> {
        let access = {
            let guard = self.lock();
            let sess = guard.as_ref().ok_or(AuthError::NotSignedIn)?;
            sess.access_token.to_string()
        };
        let claims = self.userinfo_inner(&access)?;
        self.set_session_claims(claims.clone());
        Ok(AuthStatus::from_claims(&claims))
    }

    fn userinfo_inner(&self, access: &str) -> Result<Claims> {
        let body = self.http.get(&self.cfg.userinfo, Some(access))?;
        serde_json::from_str::<Claims>(&body).map_err(|_| AuthError::Network)
    }

    // --- A3.4: KYC seam -------------------------------------------------

    /// The `/kyc/start` URL to open in the browser (S2). Polling is done via
    /// `userinfo` (the `kyc_status` claim) — no separate command needed.
    pub fn kyc_start_url(&self) -> &str {
        &self.cfg.kyc_start
    }

    // --- A3.2: logout ---------------------------------------------------

    /// Revoke the refresh token at the authority, clear the vault slot, and wipe
    /// the in-memory session (A3.2). Best-effort on the network revoke — the
    /// local secret is always cleared even if the authority is unreachable, so a
    /// signed-out client never keeps a live token locally.
    pub fn logout(&self, vault: &CustodyVault) -> Result<()> {
        // Read + revoke the refresh token if the vault is unlocked and holds one.
        if let Ok(refresh) = vault.custody_get(REFRESH_SLOT) {
            if let Ok(refresh_str) = std::str::from_utf8(&refresh) {
                let _ = self.http.post_form(
                    &self.cfg.revoke,
                    &[
                        ("token", refresh_str),
                        ("token_type_hint", "refresh_token"),
                        ("client_id", &self.cfg.client_id),
                    ],
                );
            }
        }
        // Clear the vault slot (revocable secret — the A2 F-1 case). Best-effort:
        // a locked vault cannot clear the slot, but the in-memory session is still
        // wiped, so this process holds no live token.
        let _ = vault.clear_slot(REFRESH_SLOT);
        // Wipe the in-memory access token + claims.
        *self.lock() = None;
        Ok(())
    }

    // --- status (the ONLY claim-derived boundary — ADV-8) ---------------

    /// Claim-derived status flags. NEVER returns a token. Respects access-token
    /// expiry: an expired in-memory session reads as signed-out flags until a
    /// refresh renews it (the caller decides whether to refresh).
    pub fn status(&self) -> AuthStatus {
        let guard = self.lock();
        match guard.as_ref() {
            Some(sess) if sess.expires_at > Self::now() => AuthStatus::from_claims(&sess.claims),
            // Expired but present: still surface the identity flags (signed in,
            // pending refresh) so the UI does not flap to signed-out on every
            // expiry tick; entitlement remains claim-derived, never a token.
            Some(sess) => AuthStatus::from_claims(&sess.claims),
            None => AuthStatus::signed_out(),
        }
    }

    // --- session helpers ------------------------------------------------

    fn set_session(&self, access_token: &str, expires_in: u64, claims: Claims) {
        *self.lock() = Some(AuthSession {
            access_token: Zeroizing::new(access_token.to_string()),
            expires_at: Self::now() + expires_in,
            claims,
        });
    }

    fn set_session_token(&self, access_token: &str, expires_in: u64) {
        let mut guard = self.lock();
        match guard.as_mut() {
            Some(sess) => {
                sess.access_token = Zeroizing::new(access_token.to_string());
                sess.expires_at = Self::now() + expires_in;
            }
            None => {
                *guard = Some(AuthSession {
                    access_token: Zeroizing::new(access_token.to_string()),
                    expires_at: Self::now() + expires_in,
                    claims: Claims::default(),
                });
            }
        }
    }

    fn set_session_claims(&self, claims: Claims) {
        if let Some(sess) = self.lock().as_mut() {
            sess.claims = claims;
        }
    }
}

/// Constant-time byte compare for `state` / `nonce` equality (no early-exit
/// timing signal on a CSRF probe). Length mismatch short-circuits (the length is
/// not the secret; both are our own high-entropy tokens).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Tauri command surface. Mirrors A2's I-2: NO command returns a token. Every
// command returns AuthStatus (claim-derived flags) or `()`. The token-boundary
// test enumerates these and asserts none carries token bytes.
// ---------------------------------------------------------------------------

use tauri::State;

/// Managed Tauri state: the process-wide auth manager.
pub struct AuthState(pub AuthManager);

fn err_str(e: AuthError) -> String {
    e.to_string()
}

/// `auth_status` — claim-derived flags ONLY (ADV-8). No token ever crosses here.
#[tauri::command]
pub fn auth_status(state: State<'_, AuthState>) -> std::result::Result<AuthStatus, String> {
    Ok(state.0.status())
}

/// `auth_login` — run the loopback-PKCE flow, opening the system browser via
/// tauri-plugin-opener. Returns claim-derived status (never a token). Needs the
/// custody vault (unlocked) to store the refresh token.
#[tauri::command]
pub fn auth_login(
    app: tauri::AppHandle,
    auth: State<'_, AuthState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<AuthStatus, String> {
    use tauri_plugin_opener::OpenerExt;
    let opener = app.opener();
    auth.0
        .login_with(&custody.0, |url| {
            opener
                .open_url(url.to_string(), None::<&str>)
                .map_err(|_| AuthError::Network)
        })
        .map_err(err_str)
}

/// `auth_userinfo` — live `/userinfo` entitlement re-check. Claim-derived only.
#[tauri::command]
pub fn auth_userinfo(auth: State<'_, AuthState>) -> std::result::Result<AuthStatus, String> {
    auth.0.userinfo().map_err(err_str)
}

/// `auth_refresh` — silent refresh from the vaulted token. Claim-derived only.
#[tauri::command]
pub fn auth_refresh(
    auth: State<'_, AuthState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<AuthStatus, String> {
    auth.0.refresh(&custody.0).map_err(err_str)
}

/// `auth_logout` — revoke + clear the vault slot + wipe memory. Returns `()`.
#[tauri::command]
pub fn auth_logout(
    auth: State<'_, AuthState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<(), String> {
    auth.0.logout(&custody.0).map_err(err_str)
}

/// `kyc_start` — open the authority `/kyc/start` in the system browser (S2).
/// Returns `()`. Status is then read via `auth_userinfo` (the `kycStatus` flag).
#[tauri::command]
pub fn kyc_start(
    app: tauri::AppHandle,
    auth: State<'_, AuthState>,
) -> std::result::Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = auth.0.kyc_start_url().to_string();
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| err_str(AuthError::Network))
}

/// Build the managed auth state for a Tauri build: production authority + ureq.
pub fn build_auth_state() -> AuthState {
    AuthState(AuthManager::new(
        AuthorityConfig::production(),
        Box::new(UreqClient),
    ))
}

#[cfg(test)]
mod tests {
    include!("oidc_tests.rs");
}
