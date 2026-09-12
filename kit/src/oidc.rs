//! citrate-core — OIDC loopback sign-in (CORE-A3). @rule8 · T1 auth surface.
//!
//! Turns the honest `Unavailable` `auth` seam into a real RFC 8252 loopback
//! Authorization-Code + PKCE (S256) flow against auth.citrate.ai (the
//! `citrate-core` client). A3 is AUTH ONLY — per CLAUDE.md rule 3 there are **no
//! signing / wallet / UserOp code paths here** (that is B1). This module never
//! holds or touches a wallet key.
//!
//! ## Flow (A3.1)
//! 1. `auth_login` fetches the authority discovery document
//!    (`${issuer}/.well-known/openid-configuration`, HTTPS + cert-verified),
//!    verifies its `issuer` equals the hardcoded trust anchor
//!    (`https://auth.citrate.ai` — no attacker-authority swap), and derives the
//!    `authorization`/`token`/`userinfo`/`jwks` endpoints from it (the authority
//!    serves `/auth`, `/token`, `/me`, `/jwks` — NOT the OIDC-default paths). It
//!    then binds a single-use loopback listener on `127.0.0.1:<random>`
//!    (RFC 8252 — never `0.0.0.0`), mints a PKCE verifier + `state` + `nonce`,
//!    builds the authorization URL (`code_challenge_method=S256`, scope
//!    `openid profile wallet kyc offline_access`,
//!    `redirect_uri=http://127.0.0.1:<port>/auth/callback`), and opens the system
//!    browser (tauri-plugin-opener).
//! 2. The listener accepts ONE callback, parses `code`+`state` from the request
//!    line, validates `state` (CSRF — ADV-1/ADV-6), then closes. A bind-timeout
//!    fails closed and releases the port (ADV-10).
//! 3. `code` is exchanged at `/token` with the `code_verifier` (PKCE — ADV-2).
//!    The returned `id_token` is validated (nonce/iss/aud/exp/signature via the
//!    authority JWKS — ADV-7). The authority signs **RS256** (JWKS RSA key); the
//!    accepted algs are driven by discovery's
//!    `id_token_signing_alg_values_supported` ∩ what we can verify, and
//!    `header.alg` is gated to the matched JWKS key's alg (OIDC-1 alg-pin held).
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
/// Per-request deadline for the OIDC HTTP client (discovery / token / userinfo). The chain RPC got a
/// timeout in #206 but this client did not, so a stale/rotated refresh token or a slow authority made
/// `auth_refresh`/`auth_userinfo` hang with no bound. Bound them so they fail fast instead — critical
/// now that these run at launch (a returning user's silent refresh). Generous enough that a real,
/// slightly-slow authority still completes.
const OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Read cap on the single callback request so a malicious/oversized request
/// cannot exhaust memory. A real OIDC callback line is a few hundred bytes.
const MAX_CALLBACK_BYTES: usize = 8 * 1024;

// ---------------------------------------------------------------------------
// Authority endpoint config
// ---------------------------------------------------------------------------

/// The OIDC authority TRUST ANCHOR + the `citrate-core` client id. This holds the
/// hardcoded prod `issuer` (never derived from the wire) and the discovery URL;
/// the actual protocol endpoints (authorization/token/userinfo/jwks) are NOT
/// guessed — they are fetched from the discovery document at login/refresh and
/// gated so the discovery `issuer` equals this `issuer` (no attacker-authority
/// swap). Tests inject the mock authority's base URL + discovery URL.
#[derive(Debug, Clone)]
pub struct AuthorityConfig {
    /// The discovery document URL (`${issuer}/.well-known/openid-configuration`).
    /// Fetched over HTTPS (cert-verified) at login/refresh.
    pub discovery: String,
    /// The `/kyc/start` seam URL. Not part of the OIDC discovery document, so it
    /// is derived from the issuer directly (S2 browser-open only, not a protocol
    /// endpoint).
    pub kyc_start: String,
    /// `POST /kyc/handoff` — the AUTHENTICATED hand-off mint (citrate-identity #85).
    /// The app proves its subject with the access token it already holds and gets
    /// back a single-use, subject-bound URL. Not in discovery; derived from the
    /// issuer like `kyc_start`.
    pub kyc_handoff: String,
    /// The hardcoded production issuer — the TRUST ANCHOR. The discovery `issuer`
    /// MUST equal this or the whole flow fails closed (no endpoint is trusted from
    /// an authority whose issuer we did not pin).
    pub issuer: String,
    pub client_id: String,
}

impl AuthorityConfig {
    /// Production authority (auth.citrate.ai). The endpoints are NOT hardcoded
    /// here anymore — they come from discovery (the authority serves `/auth`,
    /// `/token`, `/me`, `/jwks`, NOT the OIDC-default paths). We hardcode only the
    /// discovery URL, the issuer trust anchor, the client id, and the kyc seam.
    /// `allow(dead_code)`: only exercised by a Tauri build; the mock drives CI.
    #[allow(dead_code)]
    pub fn production() -> Self {
        let base = "https://auth.citrate.ai";
        AuthorityConfig {
            discovery: format!("{base}/.well-known/openid-configuration"),
            kyc_start: format!("{base}/kyc/start"),
            kyc_handoff: format!("{base}/kyc/handoff"),
            issuer: base.to_string(),
            client_id: "citrate-core".to_string(),
        }
    }
}

/// The OIDC protocol endpoints resolved FROM the discovery document. Only built
/// after the discovery `issuer` has been verified against the trust anchor, so
/// these URLs are authority-attested, not guessed.
#[derive(Debug, Clone)]
struct Endpoints {
    authorization: String,
    token: String,
    userinfo: String,
    jwks: String,
    /// The revocation endpoint (`revocation_endpoint`). Optional in discovery; a
    /// logout best-effort-revokes only when present.
    revocation: Option<String>,
    /// The id_token signing algs the authority advertises
    /// (`id_token_signing_alg_values_supported`). Intersected with what we can
    /// verify (RS256/ES256) to pin the accepted alg set.
    id_token_signing_algs: Vec<String>,
}

/// The discovery document fields A3 consumes (RFC 8414 / OIDC Discovery).
#[derive(Debug, Deserialize)]
struct DiscoveryDoc {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    /// OIDC serves userinfo at `userinfo_endpoint` (the authority uses `/me`).
    userinfo_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    revocation_endpoint: Option<String>,
    #[serde(default)]
    id_token_signing_alg_values_supported: Vec<String>,
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
    /// The user closed the sign-in popup (or otherwise aborted the flow) before
    /// the callback arrived (CORE-D3.0). An HONEST, distinct outcome (Rule 1):
    /// the in-flight flow is cancelled cleanly, the loopback listener is torn
    /// down, and NO partial session is created. Never a fabricated success.
    SignInCancelled,
    /// No session (not signed in) — a refresh/userinfo was attempted signed-out.
    NotSignedIn,
    /// The custody vault is locked or unavailable (cannot store/read the token).
    Custody,
    /// A network/transport error talking to the authority. RESERVED for genuine
    /// transport failures (DNS, TLS, connection refused, timeout) — never a
    /// reply the authority actually sent. See [`Self::Unauthorized`] /
    /// [`Self::Rejected`].
    Network,
    /// The authority answered **401**: the access token is missing, expired, or
    /// rejected. Distinct from [`Self::Network`] because the remedy is entirely
    /// different — refresh the session / sign in again, not "check your
    /// connection". Collapsing this into `Network` told members the service was
    /// unreachable when it was answering fine (the 2026-08-04 wallet-link
    /// dead end); a signed-out session read as an outage.
    Unauthorized,
    /// The authority answered some other non-2xx. Carries ONLY the status code —
    /// never the body, which can echo attacker-influenced input and is not
    /// needed to choose a remedy (ADV-9: no token/secret material in any
    /// variant). `409` on a wallet link means "already linked to another
    /// identity"; `400` a replayed nonce or bad signature.
    Rejected(u16),
    /// The authority is unreachable / not yet deployed (honest, per Rule 1). The
    /// production `login`/`refresh` path surfaces this when auth.citrate.ai has
    /// not been redeployed yet (the sprint's hard dependency); the mock-driven
    /// tests never hit it, so it is `dead_code` in the test-cfg build only.
    #[allow(dead_code)]
    Unavailable,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Rejected carries a code, so it cannot share the &'static str table.
        if let AuthError::Rejected(status) = self {
            return write!(f, "auth: the authority rejected the request (HTTP {status})");
        }
        let s = match self {
            AuthError::StateMismatch => "auth: callback state mismatch (rejected)",
            AuthError::TokenExchange => "auth: token exchange rejected",
            AuthError::IdTokenInvalid => "auth: id_token validation failed",
            AuthError::Timeout => "auth: sign-in timed out waiting for the browser",
            AuthError::SignInCancelled => "auth: sign-in cancelled",
            AuthError::NotSignedIn => "auth: not signed in",
            AuthError::Custody => "auth: custody vault locked or unavailable",
            AuthError::Network => "auth: could not reach the authority",
            AuthError::Unauthorized => "auth: session expired or not authorized — sign in again",
            AuthError::Unavailable => "auth: authority unavailable",
            // Handled above (needs the status code); unreachable here.
            AuthError::Rejected(_) => "auth: the authority rejected the request",
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
// Cancellation (CORE-D3.0) — an honest user-abort signal
// ---------------------------------------------------------------------------

/// A cheap, clonable cancellation flag shared between the blocking sign-in flow
/// and whatever surface can abort it (the in-app popup's window-close/destroyed
/// event — CORE-D3.0). Setting it makes the in-flight `wait_for_callback` loop
/// return `SignInCancelled` promptly and drop the loopback listener (no orphaned
/// listener, no partial session). Presentation-only: it does NOT touch any token,
/// PKCE, state/nonce, or validation path — those A3 invariants are unchanged.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh, un-cancelled token.
    pub fn new() -> Self {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }

    /// Signal cancellation. Idempotent; safe to call from a window-event thread.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
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
    ///
    /// This is the A3 entry point (used by the existing suite): it waits with NO
    /// cancellation. `wait_for_callback_cancellable` adds the CORE-D3.0 user-abort
    /// signal on top of the SAME accept loop.
    /// `allow(dead_code)`: the production path uses the cancellable form; this
    /// stays as the A3 suite's byte-for-byte entry point.
    #[cfg_attr(not(test), allow(dead_code))]
    fn wait_for_callback(self, timeout: Duration) -> Result<CallbackParams> {
        // A never-cancelled token: byte-for-byte the original ADV-10 behavior.
        self.wait_for_callback_cancellable(timeout, &CancelToken::new())
    }

    /// Same single-use accept loop as `wait_for_callback`, but ALSO polls a
    /// `CancelToken` each tick (CORE-D3.0). If the user aborts the flow (closes the
    /// popup), the loop returns `SignInCancelled` promptly and drops `self` — so
    /// the loopback listener is torn down and no partial session is created. The
    /// PKCE/state/nonce/exchange/validation path is untouched: this only decides
    /// WHETHER we keep waiting for the callback, never how a received one is
    /// handled. A timeout still fails closed exactly as before (ADV-10).
    fn wait_for_callback_cancellable(
        self,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> Result<CallbackParams> {
        self.listener
            .set_nonblocking(true)
            .map_err(|_| AuthError::Network)?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            // Honest user-abort (Rule 1): checked BEFORE and between accepts so a
            // popup close aborts even while idly waiting. `self` drops here → the
            // loopback listener is closed and the port released.
            if cancel.is_cancelled() {
                return Err(AuthError::SignInCancelled);
            }
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
#[derive(Clone, PartialEq, Eq)]
struct CallbackParams {
    code: String,
    state: String,
}

// A3-06: redact the authorization `code` (a bearer secret until exchanged) from
// any `{:?}`/dbg!/tracing output. No raw code ever reaches a log.
impl std::fmt::Debug for CallbackParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackParams")
            .field("code", &"<redacted>")
            .field("state", &"<redacted>")
            .finish()
    }
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

/// Build the authorization-endpoint URL (from the discovered
/// `authorization_endpoint`). `code_challenge_method` is hard-wired to `S256`
/// (ADV-3 — no caller can request `plain`).
fn build_authorize_url(
    authorization_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    nonce: &str,
) -> Result<String> {
    let mut url = url::Url::parse(authorization_endpoint).map_err(|_| AuthError::Network)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
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
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    id_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

// A3-06: redact all token material from any `{:?}`/dbg!/tracing output. Even the
// id_token (which carries claims) is not printed. No token ever reaches a log.
impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("id_token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .finish()
    }
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
    /// The authority NESTS the entitlement under this claim key (matching
    /// citrate-identity's `ENTITLEMENT_CLAIM = https://citrate.ai/entitlement`).
    /// On the REAL /userinfo the tier/org/role/expiry live HERE, not as top-level
    /// claims (the A3 mock used top-level — this is the mock-vs-live fix that made
    /// the desktop app never see the granted membership). The accessors below
    /// prefer this nested value and fall back to any top-level field.
    #[serde(rename = "https://citrate.ai/entitlement", default)]
    pub entitlement: Option<EntitlementClaim>,
}

/// The nested entitlement object the authority returns under `ENTITLEMENT_CLAIM`.
/// `expiresAt` is epoch-ms (a JSON number) or absent; kept as a raw `Value` so the
/// accessor can normalize it to the `Option<String>` the frontend's A3-03 reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntitlementClaim {
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default, rename = "orgId")]
    pub org_id: Option<String>,
    #[serde(default, rename = "citrateRole")]
    pub citrate_role: Option<String>,
    #[serde(default, rename = "expiresAt")]
    pub expires_at: Option<serde_json::Value>,
}

/// Map the identity authority's ENTITLEMENT vocabulary onto the app's tier
/// vocabulary (`RANK` = free/pilot/enterprise in `state.ts`). The authority grants
/// the paid membership as `commercial.kyc` (citrate-identity `MEMBER_TIER`); the
/// desktop app's gating only knows `free`/`pilot`/`enterprise`, so an unmapped
/// `commercial.kyc` reads as an unknown tier and `isPaidEntitlementActive` fails
/// closed — the app never advances past checkout even with a real, granted
/// membership. This is the boundary that translates the two vocabularies. Unknown
/// tiers pass through verbatim (the frontend then fails them closed — honest).
fn normalize_tier(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "free" | "public" | "anonymous" => "free".to_string(),
        // The $48/yr Pilot membership. The authority issues it as `commercial.kyc`
        // (KYC-gated commercial member); the app models it as the `pilot` tier.
        "commercial.kyc" | "commercial" | "member" | "pilot" => "pilot".to_string(),
        "enterprise" | "enterprise.kyc" => "enterprise".to_string(),
        other => other.to_string(),
    }
}

impl Claims {
    /// Effective tier: the NESTED entitlement claim wins; fall back to a top-level
    /// `tier` (mock/back-compat). None ⇒ the frontend treats it as unpaid. The raw
    /// authority vocabulary is normalized onto the app's tier vocabulary so the
    /// granted `commercial.kyc` membership actually reads as a paid tier.
    fn eff_tier(&self) -> Option<String> {
        self.entitlement
            .as_ref()
            .and_then(|e| e.tier.clone())
            .or_else(|| self.tier.clone())
            .map(|t| normalize_tier(&t))
    }
    fn eff_org(&self) -> Option<String> {
        self.entitlement
            .as_ref()
            .and_then(|e| e.org_id.clone())
            .or_else(|| self.org_id.clone())
    }
    fn eff_role(&self) -> Option<String> {
        self.entitlement
            .as_ref()
            .and_then(|e| e.citrate_role.clone())
            .or_else(|| self.citrate_role.clone())
    }
    /// Effective expiry as a string (frontend `isExpiredClaim` accepts a digit
    /// string or ISO). Nested `expiresAt` (JSON number/string) wins; else top-level.
    fn eff_expires_at(&self) -> Option<String> {
        let nested = self.entitlement.as_ref().and_then(|e| match &e.expires_at {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        });
        nested.or_else(|| self.expires_at.clone())
    }
}

/// An id_token `aud` claim: OIDC allows a single string OR an array of strings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    /// Whether `client_id` is (one of) the audience(s).
    fn contains(&self, client_id: &str) -> bool {
        match self {
            Audience::One(a) => a == client_id,
            Audience::Many(v) => v.iter().any(|a| a == client_id),
        }
    }

    /// Whether `aud` names MORE than one audience (OIDC Core §3.1.3.7 — a
    /// multi-valued `aud` triggers the `azp` requirement).
    fn is_multi(&self) -> bool {
        matches!(self, Audience::Many(v) if v.len() > 1)
    }
}

/// Registered id_token claims we validate against (iss/aud/azp/exp/nonce).
#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    /// OIDC-1: `aud` is a REQUIRED, non-defaulted field. A token that omits `aud`
    /// fails to deserialize here (and is separately marked required in the
    /// `Validation`), closing the audience-confusion bypass where an absent `aud`
    /// would otherwise pass `set_audience`.
    aud: Audience,
    /// NEW-1: `azp` (authorized party). OIDC Core §3.1.3.7 rules 4–5: when `aud`
    /// is multi-valued the RP MUST require `azp` and, if `azp` is present, it MUST
    /// equal our `client_id`. Optional in the wire (single-aud tokens may omit
    /// it); the multi-aud requirement is enforced in `decode_and_validate_id_token`.
    #[serde(default)]
    azp: Option<String>,
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

/// One JWK. The live authority signs id_tokens with **RS256** (`kty=RSA`, the
/// modulus `n` + exponent `e`); we also keep EC P-256 support (`kty=EC`,
/// `crv=P-256`, `x`/`y`) so a future per-client ES256 key verifies without a
/// code change. `kid` selects the key by the token header; `alg`, when the JWKS
/// publishes it, is the pin the token `header.alg` is gated against.
#[derive(Debug, Deserialize)]
struct Jwk {
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    kty: Option<String>,
    /// The key's algorithm, e.g. `RS256`/`ES256`. When present it is authoritative
    /// for the alg-pin (`header.alg` must equal it). The live JWKS publishes it.
    #[serde(default)]
    alg: Option<String>,
    // RSA (RS256) components.
    #[serde(default)]
    n: Option<String>,
    #[serde(default)]
    e: Option<String>,
    // EC (ES256) components.
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
    /// `POST url` with a JSON body and an optional bearer → body string.
    ///
    /// The identity ↔ wallet registry (`/identity/:sub/wallets`) is the first
    /// endpoint that needs BOTH a JSON body and an Authorization header, which
    /// neither `get` nor `post_form` can express. Default-implemented as
    /// unsupported so existing test fakes keep compiling and fail LOUDLY rather
    /// than silently succeeding if they are ever driven down this path.
    fn post_json(&self, _url: &str, _bearer: Option<&str>, _body: &str) -> Result<String> {
        Err(AuthError::Network)
    }
    /// `DELETE url` with a JSON body and an optional bearer → body string. Used by the #61 directory
    /// revoke (a signed tombstone). Default-implemented as unsupported so existing fakes keep
    /// compiling and fail LOUDLY rather than silently succeeding if driven down this path.
    fn delete_json(&self, _url: &str, _bearer: Option<&str>, _body: &str) -> Result<String> {
        Err(AuthError::Network)
    }
}

/// Classify a `ureq` failure, PRESERVING what the authority actually said.
///
/// ureq is configured (its default) to treat a non-2xx as `Err`, so a reply the
/// authority genuinely sent arrives here as an error. Mapping the whole enum to
/// [`AuthError::Network`] — as every call site used to — threw the status away
/// and told the member "could not reach the authority" when the authority had
/// answered promptly with `401`. That single lost byte is what made an expired
/// session indistinguishable from an outage.
///
/// Only genuine transport failures stay [`AuthError::Network`]. The status code
/// is the ONLY thing carried out of the response (ADV-9: never the body).
pub(crate) fn classify_ureq(err: &ureq::Error) -> AuthError {
    match err {
        ureq::Error::StatusCode(401) => AuthError::Unauthorized,
        ureq::Error::StatusCode(status) => AuthError::Rejected(*status),
        _ => AuthError::Network,
    }
}

/// Production HTTP client: blocking `ureq`.
pub struct UreqClient;

impl HttpClient for UreqClient {
    fn get(&self, url: &str, bearer: Option<&str>) -> Result<String> {
        let mut req = ureq::get(url)
            .config()
            .timeout_global(Some(OIDC_HTTP_TIMEOUT))
            .build();
        if let Some(tok) = bearer {
            req = req.header("Authorization", &format!("Bearer {tok}"));
        }
        let mut resp = req.call().map_err(|e| classify_ureq(&e))?;
        // A body-read failure IS a transport fault — Network is correct here.
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AuthError::Network)
    }

    fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<String> {
        let body = encode_form(form);
        // `Accept: application/json` is REQUIRED for GitHub's token endpoint: without it GitHub
        // returns `application/x-www-form-urlencoded`, which the token-response `serde_json` parse
        // then rejects (Commons CX-S1.2). Every OAuth2 token endpoint we call returns JSON when
        // asked, so this is universally safe (OIDC/Google/Notion/HF already do; GitHub needs it).
        let mut resp = ureq::post(url)
            .config()
            .timeout_global(Some(OIDC_HTTP_TIMEOUT))
            .build()
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .send(&body)
            .map_err(|_| AuthError::TokenExchange)?;
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AuthError::TokenExchange)
    }

    fn post_json(&self, url: &str, bearer: Option<&str>, body: &str) -> Result<String> {
        let mut req = ureq::post(url)
            .config()
            .timeout_global(Some(OIDC_HTTP_TIMEOUT))
            .build()
            .header("Content-Type", "application/json");
        if let Some(tok) = bearer {
            req = req.header("Authorization", &format!("Bearer {tok}"));
        }
        // A non-2xx from ureq is an Err — never mistake it for a successful link
        // with an odd body. `classify_ureq` keeps WHICH rejection it was: 401
        // (expired session) and 409 (already linked to another identity) need
        // opposite remedies, and both used to read as "could not reach".
        let mut resp = req.send(body).map_err(|e| classify_ureq(&e))?;
        // A body-read failure IS a transport fault — Network is correct here.
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AuthError::Network)
    }

    fn delete_json(&self, url: &str, bearer: Option<&str>, body: &str) -> Result<String> {
        // ureq's `delete()` builder is body-less (`WithoutBody`); the directory revoke needs a JSON
        // body, so build an `http::Request` and run it through a timeout-configured agent.
        let mut builder = ureq::http::Request::builder()
            .method("DELETE")
            .uri(url)
            .header("Content-Type", "application/json");
        if let Some(tok) = bearer {
            builder = builder.header("Authorization", format!("Bearer {tok}"));
        }
        let req = builder.body(body.to_string()).map_err(|_| AuthError::Network)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(OIDC_HTTP_TIMEOUT))
            .build()
            .into();
        let mut resp = agent.run(req).map_err(|e| classify_ureq(&e))?;
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AuthError::Network)
    }
}

/// A one-time wallet-link challenge from `/identity/:sub/wallets/challenge`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletLinkChallenge {
    /// The one-time nonce; must be echoed back on submit.
    pub nonce: String,
    /// The exact message to sign, with the literal `0x<wallet>` placeholder where
    /// the address goes. Substitute — never re-derive the format (see
    /// `AuthManager::wallet_link_challenge`).
    #[serde(rename = "message_template")]
    pub message_template: String,
}

/// The placeholder the authority puts where the wallet address belongs.
pub const WALLET_LINK_PLACEHOLDER: &str = "0x<wallet>";

/// Substitute the real address into a link-challenge template.
///
/// Returns `None` when the placeholder is absent, which means the authority's
/// message format changed under us. Failing closed is deliberate: signing a
/// message we did not fully understand is exactly what the ceremony exists to
/// prevent, and a proof over the wrong string would fail verification anyway —
/// better to refuse before asking a human to approve it.
pub fn wallet_link_message(template: &str, address: &str) -> Option<String> {
    if !template.contains(WALLET_LINK_PLACEHOLDER) {
        return None;
    }
    // The authority lowercases the address when it builds the message.
    Some(template.replace(WALLET_LINK_PLACEHOLDER, &address.to_lowercase()))
}

/// Percent-encode a `sub` for use as a single path segment.
fn urlencoding_sub(sub: &str) -> String {
    url::form_urlencoded::byte_serialize(sub.as_bytes()).collect::<String>()
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
            // Prefer the NESTED entitlement claim (real authority); fall back to
            // top-level (mock/back-compat). This is what makes the granted
            // membership tier actually reach the frontend gating.
            tier: c.eff_tier(),
            org: c.eff_org(),
            role: c.eff_role(),
            kyc_status: c.kyc_status.clone(),
            wallet_addr: c.wallet_address.clone(),
            expires_at: c.eff_expires_at(),
            email: c.email.clone(),
        }
    }
}

/// #61 — a live directory binding for an exact handle (find-via-X lookup result). `boundAt` is the
/// unix-seconds timestamp the binding was last published at. camelCase for the invoke boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryHit {
    pub address: String,
    pub bound_at: u64,
}

/// #61 — one directory search (typeahead) row. `handle` is the discovery key; the app decides display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySearchHit {
    pub handle: String,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
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

    // --- discovery (consumed at login + refresh) ------------------------

    /// Fetch + validate the authority discovery document, returning the resolved
    /// protocol endpoints. The document is fetched over the configured
    /// `discovery` URL (HTTPS + cert-verified in prod via ureq/rustls), and its
    /// `issuer` MUST equal our hardcoded trust anchor (`self.cfg.issuer`) — a
    /// mismatch fails closed so an attacker cannot swap in a look-alike authority
    /// whose endpoints we would otherwise consume. Endpoints are then taken FROM
    /// the (issuer-verified) document, never guessed.
    fn discover(&self) -> Result<Endpoints> {
        let body = self.http.get(&self.cfg.discovery, None)?;
        let doc: DiscoveryDoc = serde_json::from_str(&body).map_err(|_| AuthError::Network)?;
        // TRUST ANCHOR: the discovery issuer must equal the pinned prod issuer.
        // This is the gate that makes every derived endpoint trustworthy.
        if doc.issuer != self.cfg.issuer {
            return Err(AuthError::IdTokenInvalid);
        }
        Ok(Endpoints {
            authorization: doc.authorization_endpoint,
            token: doc.token_endpoint,
            userinfo: doc.userinfo_endpoint,
            jwks: doc.jwks_uri,
            revocation: doc.revocation_endpoint,
            id_token_signing_algs: doc.id_token_signing_alg_values_supported,
        })
    }

    // --- A3.1: the login flow -------------------------------------------

    /// Run the full loopback-PKCE sign-in against the authority, storing the
    /// refresh token in `vault` (A2) and the access token in memory. Returns the
    /// claim-derived status. This BLOCKS on the browser callback (bounded by
    /// `CALLBACK_TIMEOUT`) — callers run it off the UI thread.
    ///
    /// `open_browser` is injected so tests can drive the mock authority's
    /// `/authorize` (which 302s straight to the loopback) without a real browser;
    /// production passes the surface that navigates the in-app popup (CORE-D3.0).
    ///
    /// This is the A3 entry point (used by the full adversarial suite): it runs
    /// with NO cancellation. The CORE-D3.0 popup path calls `login_with_cancel`,
    /// which is IDENTICAL except it threads a `CancelToken` into the wait.
    /// `allow(dead_code)`: the production command uses `login_with_cancel`; this
    /// non-cancellable form stays as the A3 suite's byte-for-byte entry point.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn login_with<F>(&self, vault: &CustodyVault, open_browser: F) -> Result<AuthStatus>
    where
        F: FnOnce(&str) -> Result<()>,
    {
        // A never-cancelled token: byte-for-byte the original A3 login behavior.
        self.login_with_cancel(vault, &CancelToken::new(), open_browser)
    }

    /// The cancellable form of `login_with` (CORE-D3.0). EVERY security step —
    /// discovery + trust-anchor gate, loopback bind (127.0.0.1), PKCE S256, state,
    /// nonce, code exchange, RS256 id_token validation, refresh-token custody — is
    /// the SAME as `login_with`. The ONLY difference: the callback wait polls
    /// `cancel`, so a user who closes the popup gets an honest `SignInCancelled`
    /// with the listener torn down and no partial session (Rule 1). Cancellation
    /// touches NO token/PKCE/state/nonce/validation logic.
    pub fn login_with_cancel<F>(
        &self,
        vault: &CustodyVault,
        cancel: &CancelToken,
        open_browser: F,
    ) -> Result<AuthStatus>
    where
        F: FnOnce(&str) -> Result<()>,
    {
        // 0. Resolve the real endpoints from the (issuer-verified) discovery doc.
        //    The authority serves `/auth` `/token` `/me` `/jwks` — not the OIDC
        //    defaults — so we consume discovery instead of guessing.
        let endpoints = self.discover()?;

        // 1. Bind the single-use loopback listener FIRST so we know the port.
        let listener = LoopbackListener::bind()?;
        // The authority registered `http://127.0.0.1:<port>/auth/callback` (verified
        // 303). The old `/callback` path 400s — this is the correct redirect path.
        let redirect_uri = format!("http://127.0.0.1:{}/auth/callback", listener.port());

        // 2. Mint PKCE + state + nonce and build the authorization URL (from the
        //    discovered `authorization_endpoint`).
        let pkce = Pkce::new();
        let state = random_token();
        let nonce = random_token();
        let auth_url = build_authorize_url(
            &endpoints.authorization,
            &self.cfg.client_id,
            &redirect_uri,
            &pkce.challenge,
            &state,
            &nonce,
        )?;

        // 3. Present the authorize URL: in production, navigate the in-app popup
        //    (CORE-D3.0); in tests, hit the mock authorization endpoint. The URL
        //    carries ONLY the public PKCE challenge + state + nonce + client_id +
        //    redirect_uri + scope — never a token or the verifier.
        open_browser(&auth_url)?;

        // 4. Wait for exactly one callback; validate state (CSRF — ADV-1/6). The
        //    wait is cancellable: a popup close returns SignInCancelled (Rule 1).
        let cb = listener.wait_for_callback_cancellable(CALLBACK_TIMEOUT, cancel)?;
        if !constant_time_eq(cb.state.as_bytes(), state.as_bytes()) {
            return Err(AuthError::StateMismatch);
        }

        // 5. Exchange the code (with the PKCE verifier — ADV-2).
        let tokens = self.exchange_code(&endpoints, &cb.code, &pkce.verifier, &redirect_uri)?;

        // 6. Validate the id_token (nonce/iss/aud/exp/sig — ADV-7).
        let claims = self.validate_id_token(&endpoints, &tokens.id_token, &nonce)?;

        // 7. Persist the refresh token in the A2 vault (A3.2); access token in
        //    memory only. The vault must be unlocked — else fail closed.
        if let Some(refresh) = tokens.refresh_token {
            self.store_refresh(vault, refresh)?;
        }
        self.set_session(&tokens.access_token, tokens.expires_in, claims.clone());

        Ok(AuthStatus::from_claims(&claims))
    }

    /// Exchange an authorization `code` at the token endpoint with the PKCE
    /// `verifier`.
    fn exchange_code(
        &self,
        endpoints: &Endpoints,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenResponse> {
        let body = self.http.post_form(
            &endpoints.token,
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
    /// (iss/aud/exp/nbf/signature) and the `nonce` we minted (ADV-7). Returns the
    /// parsed identity/entitlement claims.
    fn validate_id_token(
        &self,
        endpoints: &Endpoints,
        id_token: &str,
        expected_nonce: &str,
    ) -> Result<Claims> {
        let data = self.decode_and_validate_id_token(endpoints, id_token)?;
        // nonce binds this id_token to THIS login attempt (replay defense).
        match data.nonce.as_deref() {
            Some(n) if constant_time_eq(n.as_bytes(), expected_nonce.as_bytes()) => {}
            _ => return Err(AuthError::IdTokenInvalid),
        }
        Ok(data.claims)
    }

    /// OIDC-3/A3-04: validate a refresh-response id_token WITHOUT the nonce check
    /// (a refresh grant omits `nonce`). iss/aud/exp/nbf/signature still enforced.
    fn validate_id_token_no_nonce(&self, endpoints: &Endpoints, id_token: &str) -> Result<Claims> {
        Ok(self
            .decode_and_validate_id_token(endpoints, id_token)?
            .claims)
    }

    /// Shared id_token decode + signature + registered-claim validation.
    ///
    /// **Alg pin (OIDC-1 — do NOT regress).** The accepted alg set is driven by
    /// discovery's `id_token_signing_alg_values_supported` INTERSECTED with what
    /// we can actually verify (RS256 + ES256). The token `header.alg` must be in
    /// that set AND must equal the alg the matched JWKS key carries (for the
    /// token's `kid`) — so `alg:none`, HS256, and any alg the authority does not
    /// advertise/key are rejected BEFORE any signature work. The `Validation` is
    /// built with exactly that single, key-attested alg (never a broad allow-list).
    ///
    /// Also enforces `aud` REQUIRED and equal to our client_id (OIDC-1 — absent
    /// `aud` is rejected, not silently accepted), `iss` required + re-checked,
    /// `exp` + `nbf` validated. Does NOT check nonce (callers add that where
    /// applicable).
    fn decode_and_validate_id_token(
        &self,
        endpoints: &Endpoints,
        id_token: &str,
    ) -> Result<IdTokenClaims> {
        let header =
            jsonwebtoken::decode_header(id_token).map_err(|_| AuthError::IdTokenInvalid)?;
        // Resolve the JWKS key for this token's kid AND the alg it must verify
        // under. `jwk_decoding_key` returns (key, key_alg) where key_alg is the
        // alg pinned by the JWKS key (its published `alg`, or the sole alg the
        // key type can produce). `alg:none`/HS never survive: an HS/none header
        // simply won't equal the RSA/EC key's alg, and no key material exists for
        // it in the JWKS.
        let (key, key_alg) = self.jwk_decoding_key(endpoints, header.kid.as_deref())?;

        // Alg-pin gate 1: the authority must ADVERTISE this alg in discovery
        // (∩ what we can verify), and the token header's alg must equal the key's
        // alg. Both must hold — advertised, keyed, and header-matched.
        if !self.authority_accepts_alg(endpoints, key_alg) {
            return Err(AuthError::IdTokenInvalid);
        }
        if header.alg != key_alg {
            return Err(AuthError::IdTokenInvalid);
        }

        let mut validation = Validation::new(key_alg);
        validation.set_issuer(&[&self.cfg.issuer]);
        validation.set_audience(&[&self.cfg.client_id]);
        // OIDC-1: mark iss + aud REQUIRED (jsonwebtoken's default only requires
        // `exp`). Without this, a token that OMITS `aud` bypasses `set_audience`.
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        validation.validate_exp = true;
        // OIDC-4: reject not-yet-valid tokens; small leeway for clock skew.
        validation.validate_nbf = true;
        validation.leeway = 30;

        let token = jsonwebtoken::decode::<IdTokenClaims>(id_token, &key, &validation)
            .map_err(|_| AuthError::IdTokenInvalid)?;
        let data = token.claims;

        // Defensive re-checks mirroring the library validation (belt + braces):
        // iss must equal our authority, and aud MUST contain our client_id — an
        // absent aud already failed deserialization + required-claims, this
        // guarantees a present-but-wrong or array aud is bound too (OIDC-1).
        if data.iss != self.cfg.issuer {
            return Err(AuthError::IdTokenInvalid);
        }
        if !data.aud.contains(&self.cfg.client_id) {
            return Err(AuthError::IdTokenInvalid);
        }
        // NEW-1 (OIDC Core §3.1.3.7 rules 4–5): a MULTI-valued `aud` requires
        // `azp` to be present AND equal to our client_id — otherwise a token
        // issued to a DIFFERENT authorized party that merely lists us in `aud`
        // (confused-deputy / multi-client misissuance) would be accepted. For a
        // single `aud` (the normal case) `azp` is optional; but if `azp` IS
        // present it must still name us (never a different party).
        match (data.aud.is_multi(), data.azp.as_deref()) {
            // Multi-aud with no azp, or an azp that isn't us → reject.
            (true, None) => return Err(AuthError::IdTokenInvalid),
            (true, Some(azp)) if azp != self.cfg.client_id => {
                return Err(AuthError::IdTokenInvalid)
            }
            // Single-aud but a present azp that names someone else → reject.
            (false, Some(azp)) if azp != self.cfg.client_id => {
                return Err(AuthError::IdTokenInvalid)
            }
            _ => {}
        }
        Ok(data)
    }

    /// The set of algs A3 can actually VERIFY (built into this client). The
    /// accepted set is always this ∩ what the authority advertises — never
    /// broader than what we can cryptographically check.
    fn verifiable_algs() -> [Algorithm; 2] {
        // RS256 (the live authority) + ES256 (kept so a future per-client ES256
        // key verifies without a code change). NOT HS*, NOT `none`.
        [Algorithm::RS256, Algorithm::ES256]
    }

    /// Whether `alg` is BOTH verifiable by us AND advertised by the authority in
    /// discovery (`id_token_signing_alg_values_supported`). If discovery does not
    /// list any algs (older/thin authority), fall back to the verifiable set so
    /// the RS256 key still works — but never accept an alg we cannot verify.
    fn authority_accepts_alg(&self, endpoints: &Endpoints, alg: Algorithm) -> bool {
        if !Self::verifiable_algs().contains(&alg) {
            return false;
        }
        let advertised = &endpoints.id_token_signing_algs;
        if advertised.is_empty() {
            return true;
        }
        advertised
            .iter()
            .filter_map(|s| alg_from_str(s))
            .any(|a| a == alg)
    }

    /// Fetch the JWKS and build the decoding key for `kid`, returning the key and
    /// the alg it is pinned to (its published `alg`, or the sole alg its key type
    /// can produce). RS256 (RSA `n`/`e`) is the live case; ES256 (EC `x`/`y`) is
    /// kept for a future per-client key. RSA verification goes through the
    /// `jsonwebtoken` `aws_lc_rs` backend — the `rsa` crate (RUSTSEC-2023-0071) is
    /// never pulled in.
    ///
    /// OIDC-2 key-selection policy (no first-key fallback):
    /// - token HAS a `kid` → require an EXACT `kid` match; else reject. A
    ///   kid-less JWKS key is NOT a match for a kid-bearing token.
    /// - token has NO `kid` → accept ONLY if the JWKS publishes exactly ONE key
    ///   (unambiguous). A kid-less token against a multi-key JWKS is rejected
    ///   (fail closed) rather than silently binding to the first key — which
    ///   otherwise lets an attacker-ordered JWKS or a rotation window decide the
    ///   verifying key by position instead of identity.
    fn jwk_decoding_key(
        &self,
        endpoints: &Endpoints,
        kid: Option<&str>,
    ) -> Result<(DecodingKey, Algorithm)> {
        let body = self.http.get(&endpoints.jwks, None)?;
        let jwks: Jwks = serde_json::from_str(&body).map_err(|_| AuthError::IdTokenInvalid)?;
        let jwk = match kid {
            Some(want) => jwks
                .keys
                .iter()
                .find(|k| k.kid.as_deref() == Some(want))
                .ok_or(AuthError::IdTokenInvalid)?,
            None => {
                // No kid: only a single-key JWKS is unambiguous.
                if jwks.keys.len() != 1 {
                    return Err(AuthError::IdTokenInvalid);
                }
                &jwks.keys[0]
            }
        };
        match jwk.kty.as_deref() {
            Some("RSA") => {
                // The JWKS key's alg pins the accepted header alg. If the key
                // publishes `alg`, honor it (must be RS*); else default to RS256
                // for an RSA key. An EC/HS alg on an RSA key is a mismatch → fail.
                let alg = match jwk.alg.as_deref() {
                    Some(a) => alg_from_str(a).ok_or(AuthError::IdTokenInvalid)?,
                    None => Algorithm::RS256,
                };
                if !matches!(alg, Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512) {
                    return Err(AuthError::IdTokenInvalid);
                }
                let n = jwk.n.as_deref().ok_or(AuthError::IdTokenInvalid)?;
                let e = jwk.e.as_deref().ok_or(AuthError::IdTokenInvalid)?;
                let key = DecodingKey::from_rsa_components(n, e)
                    .map_err(|_| AuthError::IdTokenInvalid)?;
                Ok((key, alg))
            }
            Some("EC") => {
                let alg = match jwk.alg.as_deref() {
                    Some(a) => alg_from_str(a).ok_or(AuthError::IdTokenInvalid)?,
                    None => Algorithm::ES256,
                };
                if !matches!(alg, Algorithm::ES256 | Algorithm::ES384) {
                    return Err(AuthError::IdTokenInvalid);
                }
                let x = jwk.x.as_deref().ok_or(AuthError::IdTokenInvalid)?;
                let y = jwk.y.as_deref().ok_or(AuthError::IdTokenInvalid)?;
                let key =
                    DecodingKey::from_ec_components(x, y).map_err(|_| AuthError::IdTokenInvalid)?;
                Ok((key, alg))
            }
            _ => Err(AuthError::IdTokenInvalid),
        }
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
        // Re-resolve endpoints from discovery (same trust-anchor gate as login).
        let endpoints = self.discover()?;
        let refresh = vault.custody_get(REFRESH_SLOT).map_err(AuthError::from)?;
        let refresh_str = std::str::from_utf8(&refresh)
            .map_err(|_| AuthError::Custody)?
            .to_string();
        let body = self.http.post_form(
            &endpoints.token,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh_str),
                ("client_id", &self.cfg.client_id),
            ],
        )?;
        let tokens: TokenResponse =
            serde_json::from_str(&body).map_err(|_| AuthError::TokenExchange)?;

        // A3-04: if the refresh response carries an id_token, RE-VALIDATE it
        // (iss/aud/exp/ES256 signature via JWKS) — a refresh grant normally omits
        // `nonce`, so nonce is not re-checked, but the signature/issuer/audience/
        // expiry must still hold. Enforce `sub` CONTINUITY against the prior
        // session: a refreshed session must belong to the same subject, so a
        // swapped/injected id_token for a different user fails closed.
        let prior_sub = {
            let guard = self.lock();
            guard.as_ref().map(|s| s.claims.sub.clone())
        };
        if !tokens.id_token.is_empty() {
            let refreshed = self.validate_id_token_no_nonce(&endpoints, &tokens.id_token)?;
            if let Some(prev) = prior_sub.as_deref() {
                if !prev.is_empty() && !refreshed.sub.is_empty() && refreshed.sub != prev {
                    return Err(AuthError::IdTokenInvalid);
                }
            }
        }

        // Rotate: store the NEW refresh token if the authority rotated it.
        if let Some(new_refresh) = tokens.refresh_token {
            self.store_refresh(vault, new_refresh)?;
        }
        // A refreshed access token carries fresh claims via /userinfo (federation
        // RP rule); prefer a live /userinfo re-check for entitlement.
        self.set_session_token(&tokens.access_token, tokens.expires_in);
        let claims = self.userinfo_inner(&endpoints, &tokens.access_token)?;
        // sub continuity also holds against /userinfo.
        if let Some(prev) = prior_sub.as_deref() {
            if !prev.is_empty() && !claims.sub.is_empty() && claims.sub != prev {
                return Err(AuthError::IdTokenInvalid);
            }
        }
        self.set_session_claims(claims.clone());
        Ok(AuthStatus::from_claims(&claims))
    }

    // --- A3.3: live /userinfo re-check ----------------------------------

    /// Live `/userinfo` re-check (the federation RP rule): re-reads the
    /// entitlement claim from the authority using the in-memory access token, and
    /// updates the session claims. Errors `NotSignedIn` if there is no session.
    pub fn userinfo(&self) -> Result<AuthStatus> {
        let endpoints = self.discover()?;
        let access = {
            let guard = self.lock();
            let sess = guard.as_ref().ok_or(AuthError::NotSignedIn)?;
            sess.access_token.to_string()
        };
        let claims = self.userinfo_inner(&endpoints, &access)?;
        let st = AuthStatus::from_claims(&claims);
        self.set_session_claims(claims.clone());
        Ok(st)
    }

    fn userinfo_inner(&self, endpoints: &Endpoints, access: &str) -> Result<Claims> {
        let body = self.http.get(&endpoints.userinfo, Some(access))?;
        serde_json::from_str::<Claims>(&body).map_err(|_| AuthError::Network)
    }

    // --- identity ↔ wallet registry (the wallet_address binding) --------

    /// Ask the authority for a one-time wallet-link challenge.
    ///
    /// WHY THIS LIVES HERE. The registry endpoints need the session bearer, and
    /// the access token never leaves this module (I-2) — the same posture as
    /// `userinfo`. Callers get the nonce + message and never the token.
    ///
    /// The returned `message_template` carries the literal `0x<wallet>` where the
    /// address belongs. Callers MUST substitute rather than re-derive the format:
    /// the authority rebuilds this exact string from (authority, sub, address,
    /// nonce, chainId) and verifies the signature against it, so a client that
    /// formats its own message would produce a proof that silently fails to
    /// recover to the signer.
    pub fn wallet_link_challenge(&self) -> Result<WalletLinkChallenge> {
        let (sub, access) = self.session_sub_and_token()?;
        let url = format!(
            "{}/identity/{}/wallets/challenge",
            self.cfg.issuer.trim_end_matches('/'),
            urlencoding_sub(&sub)
        );
        let body = self.http.post_json(&url, Some(&access), "{}")?;
        serde_json::from_str::<WalletLinkChallenge>(&body).map_err(|_| AuthError::Network)
    }

    /// Submit a proven wallet link. `signature` must be the EIP-191 signature of
    /// the challenge message by `address`'s own key.
    ///
    /// A non-2xx (replayed nonce, a wallet already linked to another identity, a
    /// signature that does not recover) surfaces as an error — never a silent
    /// success, because the caller's next step is to trust that the authority now
    /// serves this address as `wallet_address`.
    pub fn wallet_link_submit(
        &self,
        address: &str,
        signature: &str,
        nonce: &str,
    ) -> Result<()> {
        let (sub, access) = self.session_sub_and_token()?;
        let url = format!(
            "{}/identity/{}/wallets",
            self.cfg.issuer.trim_end_matches('/'),
            urlencoding_sub(&sub)
        );
        let body = serde_json::json!({
            "address": address,
            "signature": signature,
            "nonce": nonce,
        })
        .to_string();
        self.http.post_json(&url, Some(&access), &body)?;
        Ok(())
    }

    /// Make an already-linked wallet the CANONICAL one — the address the
    /// authority serves as the `wallet_address` claim, and therefore the address
    /// the treasury pays.
    ///
    /// Canonical defaults to FIRST-linked at the authority, so that a restart or
    /// a stray second link can never silently move a member's pay-to address.
    /// That default is right, and it is also why this call has to exist: when a
    /// member's device custody vault is replaced, the desktop mints a NEW custody
    /// EOA and links it, but the claim stays pinned to the wallet linked first.
    /// `walletIsLinked` requires `wallet_address == this device's custody
    /// address`, so onboarding blocks — permanently, and silently, because the
    /// link itself succeeded. Observed live 2026-08-04.
    ///
    /// The authority refuses any address that is not already a PROVEN link for
    /// this sub, so this can only re-order wallets whose ownership the EIP-191
    /// challenge already established. It cannot introduce an address.
    ///
    /// A non-2xx surfaces as an error rather than a silent success: the caller's
    /// next step is to trust that the authority now serves this address.
    pub fn wallet_set_canonical(&self, address: &str) -> Result<()> {
        let (sub, access) = self.session_sub_and_token()?;
        let url = format!(
            "{}/identity/{}/wallets/{}/canonical",
            self.cfg.issuer.trim_end_matches('/'),
            urlencoding_sub(&sub),
            urlencoding_sub(address)
        );
        self.http.post_json(&url, Some(&access), "{}")?;
        Ok(())
    }

    // --- #61: self-published bindings directory (find-via-X) --------------
    // The directory is mounted on the authority (`{issuer}/directory/*`) and Bearer-gated, so these
    // reuse the in-memory access token exactly like `wallet_set_canonical` — the token is NEVER
    // returned to a caller, only the parsed result. The two publish PROOFS (the app's verified social
    // IdentityBinding = ownership_proof, and a fresh directory-scoped `sig` from the SignatureCeremony)
    // are assembled in citrate-core and passed IN; this layer only makes the authenticated call.

    /// Publish (opt-in) a self-published `(platform, handle) ↔ address` binding. Returns the outcome
    /// the authority reports (`"stored"` | `"unchanged"`). A 409 (a newer binding won) or 422 (a proof
    /// failed) surfaces as an error (never a false "published", ADV-9 / Rule 1).
    #[allow(clippy::too_many_arguments)]
    pub fn directory_publish(
        &self,
        platform: &str,
        handle: &str,
        address: &str,
        display_name: Option<&str>,
        bound_at: u64,
        ownership_nonce: &str,
        ownership_sig: &str,
        sig: &str,
    ) -> Result<String> {
        let (_sub, access) = self.session_sub_and_token()?;
        let url = format!("{}/directory/bindings", self.cfg.issuer.trim_end_matches('/'));
        let mut body = serde_json::json!({
            "platform": platform,
            "handle": handle,
            "address": address,
            "bound_at": bound_at,
            "ownership_proof": { "nonce": ownership_nonce, "signature": ownership_sig },
            "sig": sig,
        });
        if let Some(dn) = display_name.filter(|d| !d.is_empty()) {
            body["display_name"] = serde_json::Value::String(dn.to_string());
        }
        let resp = self.http.post_json(&url, Some(&access), &body.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&resp).map_err(|_| AuthError::Network)?;
        Ok(v.get("status").and_then(|s| s.as_str()).unwrap_or("stored").to_string())
    }

    /// Revoke (tombstone) this member's directory binding for `(platform, handle)`. `sig` is a fresh
    /// signature over the directory revoke statement by the SAME address (assembled in citrate-core).
    pub fn directory_revoke(&self, platform: &str, handle: &str, address: &str, sig: &str) -> Result<()> {
        let (_sub, access) = self.session_sub_and_token()?;
        let url = format!("{}/directory/bindings", self.cfg.issuer.trim_end_matches('/'));
        let body = serde_json::json!({
            "platform": platform,
            "handle": handle,
            "address": address,
            "sig": sig,
        })
        .to_string();
        self.http.delete_json(&url, Some(&access), &body)?;
        Ok(())
    }

    /// Look up the live binding for an EXACT handle. `None` when nobody published it (never a guess).
    pub fn directory_lookup(&self, platform: &str, handle: &str) -> Result<Option<DirectoryHit>> {
        let (_sub, access) = self.session_sub_and_token()?;
        let url = format!(
            "{}/directory/lookup?platform={}&handle={}",
            self.cfg.issuer.trim_end_matches('/'),
            urlencoding_sub(platform),
            urlencoding_sub(handle),
        );
        let resp = self.http.get(&url, Some(&access))?;
        let trimmed = resp.trim();
        if trimmed.is_empty() || trimmed == "null" {
            return Ok(None);
        }
        let v: serde_json::Value = serde_json::from_str(trimmed).map_err(|_| AuthError::Network)?;
        // A miss also arrives as JSON `null`.
        if v.is_null() {
            return Ok(None);
        }
        let address = v.get("address").and_then(|a| a.as_str());
        let bound_at = v.get("bound_at").and_then(|b| b.as_u64());
        match (address, bound_at) {
            (Some(a), Some(t)) => Ok(Some(DirectoryHit { address: a.to_string(), bound_at: t })),
            _ => Ok(None),
        }
    }

    /// Typeahead search — up to the authority's cap of live bindings whose handle starts with `query`.
    /// Honest-empty on a blank/too-short prefix or no matches (never a fabricated row).
    pub fn directory_search(&self, platform: &str, query: &str) -> Result<Vec<DirectorySearchHit>> {
        let (_sub, access) = self.session_sub_and_token()?;
        let url = format!(
            "{}/directory/search?platform={}&q={}",
            self.cfg.issuer.trim_end_matches('/'),
            urlencoding_sub(platform),
            urlencoding_sub(query),
        );
        let resp = self.http.get(&url, Some(&access))?;
        let arr: Vec<serde_json::Value> = serde_json::from_str(resp.trim()).unwrap_or_default();
        Ok(arr
            .into_iter()
            .filter_map(|v| {
                let handle = v.get("handle").and_then(|h| h.as_str())?.to_string();
                let address = v.get("address").and_then(|a| a.as_str())?.to_string();
                let display_name = v
                    .get("display_name")
                    .and_then(|d| d.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                Some(DirectorySearchHit { handle, address, display_name })
            })
            .collect())
    }

    /// The signed-in sub plus its access token. Both come from the live session;
    /// `NotSignedIn` when there is none.
    fn session_sub_and_token(&self) -> Result<(String, String)> {
        let guard = self.lock();
        let sess = guard.as_ref().ok_or(AuthError::NotSignedIn)?;
        if sess.claims.sub.is_empty() {
            return Err(AuthError::NotSignedIn);
        }
        Ok((sess.claims.sub.clone(), sess.access_token.to_string()))
    }

    // --- A3.4: KYC seam -------------------------------------------------

    /// The `/kyc/start` URL to open in the browser (S2). Polling is done via
    /// `userinfo` (the `kyc_status` claim) — no separate command needed.
    pub fn kyc_start_url(&self) -> &str {
        &self.cfg.kyc_start
    }

    /// Mint a SUBJECT-BOUND hand-off URL for an authority surface.
    ///
    /// `auth.citrate.ai` used to resolve the acting user from the BROWSER cookie,
    /// so opening a bare `/kyc/start` handed the member whichever account their
    /// browser held — a member could verify the WRONG identity (2026-08-06:
    /// signed in as a test member, KYC opened the admin's verified account).
    ///
    /// citrate-identity #85 closed that: we POST the access token we already hold,
    /// the authority mints a single-use nonce bound to OUR `sub`, and the consuming
    /// page resolves from the nonce and IGNORES the cookie.
    ///
    /// `target` is `"kyc"` or `"account"`. Errors are the caller's cue to fall back
    /// to the bare URL — degraded (cookie-resolved) but not broken.
    pub fn handoff_url(&self, target: &str) -> Result<String> {
        let (_sub, access) = self.session_sub_and_token()?;
        let body = serde_json::json!({ "target": target }).to_string();
        let raw = self.http.post_json(&self.cfg.kyc_handoff, Some(&access), &body)?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|_| AuthError::Rejected(200))?;
        v.get("url")
            .and_then(|u| u.as_str())
            .map(|u| u.to_string())
            .ok_or(AuthError::Rejected(200))
    }

    /// The pinned issuer (the trust anchor). Surfaced so a ceremony can display
    /// the true asker verbatim; carries no secret.
    pub fn issuer(&self) -> &str {
        &self.cfg.issuer
    }

    // --- A3.2: logout ---------------------------------------------------

    /// Revoke the refresh token at the authority, clear the vault slot, and wipe
    /// the in-memory session (A3.2). Best-effort on the network revoke — the
    /// local secret is always cleared even if the authority is unreachable, so a
    /// signed-out client never keeps a live token locally.
    pub fn logout(&self, vault: &CustodyVault) -> Result<()> {
        // Read + revoke the refresh token if the vault is unlocked and holds one.
        // The revocation endpoint comes from discovery (`revocation_endpoint`,
        // served at `/token/revocation`). Discovery is best-effort here: if the
        // authority is unreachable or advertises no revocation endpoint, we skip
        // the network revoke — the LOCAL secret is still cleared below, so a
        // signed-out client never keeps a live token locally.
        if let Ok(refresh) = vault.custody_get(REFRESH_SLOT) {
            if let Ok(refresh_str) = std::str::from_utf8(&refresh) {
                if let Ok(endpoints) = self.discover() {
                    if let Some(revocation) = endpoints.revocation.as_deref() {
                        let _ = self.http.post_form(
                            revocation,
                            &[
                                ("token", refresh_str),
                                ("token_type_hint", "refresh_token"),
                                ("client_id", &self.cfg.client_id),
                            ],
                        );
                    }
                }
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

/// Map a JWS `alg` string to the `jsonwebtoken::Algorithm`. Returns `None` for
/// an unknown / unsupported alg — including `none` and the HS* family, which A3
/// never verifies (so a header/JWKS advertising them can never pin an accepted
/// alg). Only the RS* / ES* families we can verify are mapped.
fn alg_from_str(s: &str) -> Option<Algorithm> {
    match s {
        "RS256" => Some(Algorithm::RS256),
        "RS384" => Some(Algorithm::RS384),
        "RS512" => Some(Algorithm::RS512),
        "ES256" => Some(Algorithm::ES256),
        "ES384" => Some(Algorithm::ES384),
        _ => None,
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

use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Managed Tauri state: the process-wide auth manager.
pub struct AuthState(pub AuthManager);

fn err_str(e: AuthError) -> String {
    e.to_string()
}

/// The dedicated in-app sign-in popup window label (CORE-D3.0). A single,
/// well-known label so a stale popup from a prior aborted attempt is reused/closed
/// rather than leaking a second window.
const AUTH_POPUP_LABEL: &str = "auth-popup";

/// Build + show the branded in-app sign-in popup (CORE-D3.0) navigated to the
/// authority authorize `url`, and wire its close/destroy event to `cancel` so a
/// user who dismisses the popup aborts the in-flight flow honestly (Rule 1). The
/// popup is a DEDICATED window (label `auth-popup`), not the main window: ~480×720,
/// centered, focused, non-resizable, titled "Sign in · Citrate".
///
/// The `url` is the public authorize URL ONLY (client_id, redirect_uri, PKCE
/// challenge, state, nonce, scope) — no token or verifier is ever placed in it.
/// If the window cannot be created (headless / CI), returns an honest `Network`
/// error rather than panicking.
fn open_auth_popup(
    app: &tauri::AppHandle,
    url: &str,
    cancel: &CancelToken,
) -> Result<tauri::WebviewWindow> {
    let external = url.parse::<tauri::Url>().map_err(|_| AuthError::Network)?;

    // macOS requires WebviewWindow/NSWindow creation on the MAIN thread. `auth_login`
    // is a sync #[tauri::command] that runs OFF the main thread (so the blocking
    // loopback wait doesn't freeze the UI), so building the window directly here fails
    // silently on macOS and the popup never appears. We MARSHAL the creation onto the
    // main thread via `run_on_main_thread` and hand the handle back over a channel.
    let app_main = app.clone();
    let cancel_on_close = cancel.clone();
    let (tx, rx) =
        std::sync::mpsc::channel::<std::result::Result<tauri::WebviewWindow, tauri::Error>>();
    app.run_on_main_thread(move || {
        // Close any stale popup from a prior aborted attempt (also a main-thread op).
        if let Some(existing) = app_main.get_webview_window(AUTH_POPUP_LABEL) {
            let _ = existing.close();
        }
        let built =
            WebviewWindowBuilder::new(&app_main, AUTH_POPUP_LABEL, WebviewUrl::External(external))
                .title("Sign in · Citrate")
                .inner_size(480.0, 720.0)
                .resizable(false)
                .center()
                .focused(true)
                .build();
        if let Ok(ref window) = built {
            // Wire the user-close abort (registered ON the main thread with the window):
            // a user close before the callback signals cancellation so the awaiting
            // login returns SignInCancelled, the loopback listener is torn down, and no
            // partial session is created. Firing after completion is a harmless no-op.
            let c = cancel_on_close.clone();
            window.on_window_event(move |event| {
                if matches!(
                    event,
                    WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
                ) {
                    c.cancel();
                }
            });
        }
        let _ = tx.send(built);
    })
    .map_err(|_| AuthError::Network)?;

    match rx.recv() {
        Ok(Ok(window)) => Ok(window),
        Ok(Err(e)) => {
            // Surface the real cause (previously swallowed to a blank Network error).
            eprintln!("[auth] popup WebviewWindow build failed on main thread: {e}");
            Err(AuthError::Network)
        }
        Err(_) => Err(AuthError::Network),
    }
}

/// `auth_status` — claim-derived flags ONLY (ADV-8). No token ever crosses here.
#[tauri::command]
pub fn auth_status(state: State<'_, AuthState>) -> std::result::Result<AuthStatus, String> {
    Ok(state.0.status())
}

/// `auth_login` — run the loopback-PKCE flow, presenting the authorize page in an
/// IN-APP popup `WebviewWindow` (CORE-D3.0) instead of kicking the user out to the
/// system browser. Returns claim-derived status (never a token). Needs the custody
/// vault (unlocked) to store the refresh token.
///
/// PRESENTATION SWAP ONLY: the entire A3 security flow — discovery + trust anchor,
/// loopback bind (127.0.0.1), PKCE S256, state, nonce, code exchange, RS256
/// id_token validation, refresh-token custody — is byte-for-byte unchanged. The
/// only change is HOW the user reaches the authorize page: the popup navigates to
/// the SAME authorize URL, and the SAME loopback listener catches the `code`+
/// `state` when the authority 30x-redirects the popup's top-level navigation to
/// `http://127.0.0.1:<port>/auth/callback`.
///
/// Lifecycle: the popup opens when the flow starts and is CLOSED programmatically
/// the moment the flow resolves — success, timeout, or error. If the USER closes
/// the popup first, the popup's close event fires the `CancelToken`, the awaiting
/// flow returns `SignInCancelled`, the loopback listener is torn down, and no
/// partial session is created (Rule 1). No orphaned listener, no hang.
#[tauri::command]
pub async fn auth_login(
    app: tauri::AppHandle,
    auth: State<'_, AuthState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<AuthStatus, String> {
    // ASYNC so Tauri runs this OFF the main thread. The loopback-PKCE flow blocks
    // (waits up to CALLBACK_TIMEOUT for the popup's redirect to hit the 127.0.0.1
    // listener); on the main thread that would freeze the whole UI (the popup could
    // not even render). Off the main thread, the blocking wait occupies a runtime
    // worker while the main thread stays free to render the popup + service the
    // `run_on_main_thread` window creation inside `open_auth_popup`.
    // A per-attempt cancel token shared with the popup's close event.
    let cancel = CancelToken::new();
    // The popup is created lazily INSIDE the open_browser closure — after the
    // loopback listener is bound and the authorize URL is built — so we navigate
    // straight to the real authorize URL (no blank flash). We hold the window
    // handle so we can close it programmatically on every exit path.
    let popup: Mutex<Option<tauri::WebviewWindow>> = Mutex::new(None);

    let result = auth.0.login_with_cancel(&custody.0, &cancel, |url| {
        let window = open_auth_popup(&app, url, &cancel)?;
        *popup.lock().unwrap_or_else(|e| e.into_inner()) = Some(window);
        Ok(())
    });

    // Programmatically close the popup on EVERY outcome (success / timeout / error /
    // cancel), on the MAIN thread (macOS window ops). On a user-close the window is
    // already gone — close() is a harmless no-op. No popup is left after the flow ends.
    if let Some(window) = popup.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = app.run_on_main_thread(move || {
            let _ = window.close();
        });
    }

    result.map_err(err_str)
}

/// `auth_userinfo` — live `/userinfo` entitlement re-check. Claim-derived only.
/// ASYNC so Tauri runs it OFF the main thread (like `auth_login`): the body makes blocking HTTP
/// round-trips to the authority (discovery + userinfo), which on the main thread would freeze the
/// webview. The OIDC client is bounded by `OIDC_HTTP_TIMEOUT`, so a slow authority fails fast.
#[tauri::command]
pub async fn auth_userinfo(auth: State<'_, AuthState>) -> std::result::Result<AuthStatus, String> {
    auth.0.userinfo().map_err(err_str)
}

/// `auth_refresh` — silent refresh from the vaulted token. Claim-derived only.
/// ASYNC so Tauri runs it OFF the main thread. This runs at launch for a returning user (a vaulted
/// refresh token exists) and makes up to three blocking HTTP round-trips (discovery + token +
/// userinfo); on the main thread that was the login pinwheel. Bounded by `OIDC_HTTP_TIMEOUT`.
#[tauri::command]
pub async fn auth_refresh(
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
    // Prefer the authenticated hand-off so the page binds to THIS app's subject.
    // Falling back to the bare URL keeps a signed-out/offline case working, at the
    // old (cookie-resolved) fidelity — degraded, never silently wrong-account,
    // because the UI still names the account it intends.
    let url = auth
        .0
        .handoff_url("kyc")
        .unwrap_or_else(|_| auth.0.kyc_start_url().to_string());
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
