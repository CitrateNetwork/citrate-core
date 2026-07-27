//! W4 — MCP connections OAuth core (ADR-3).
//!
//! The provider-agnostic pieces of the authorization-code + PKCE(S256) flow for the
//! three MCP services (GitHub, Google Drive, Notion): the service catalog
//! (endpoints + scopes), PKCE/`state` minting, the authorize-URL builder, and the
//! token-exchange request builder — all PURE + unit-tested here. The loopback
//! listener (RFC 8252, fixed port), the live HTTP exchange, and the keyring token
//! custody build on top of this (subsequent WPs), mirroring `oidc.rs`.
//!
//! Redirect is a FIXED loopback (not oidc.rs's random `:0`) so the single-use
//! listener always binds the same port. GitHub + Google accept that `http://`
//! loopback `redirect_uri` directly; NOTION rejects a plaintext-http redirect and
//! is registered against the hosted `https://auth.citrate.ai/oauth/callback`
//! bounce, which 302s the browser back to the same loopback (see the runbook +
//! ADR-3). So `redirect_uri` is PER-SERVICE ({@link Service::redirect_uri}) while
//! the loopback the listener binds is common. NO `#[tauri::command]` here returns a
//! token or the client secret (I-2 barrier); those live sealed in the OS keyring.

use base64::Engine as _;
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

/// The loopback callback the single-use listener binds — and the `redirect_uri`
/// registered for the providers that accept a plaintext-http loopback (GitHub,
/// Google's Desktop-app client type). See {@link Service::redirect_uri}.
pub const OAUTH_REDIRECT_URI: &str = "http://127.0.0.1:8975/oauth/callback";

/// The hosted https `redirect_uri` for providers that reject an http loopback
/// (Notion). Registered against the stateless bounce on the Citrate OIDC authority
/// (citrate-identity `src/oauth-bounce.ts`), which 302s the browser straight back
/// to {@link OAUTH_REDIRECT_URI}. This URL is the OAuth `redirect_uri` PARAMETER
/// (sent in both the authorize request and the token exchange, and matched by the
/// provider); the browser still ultimately lands on the loopback the listener owns.
pub const HOSTED_REDIRECT_URI: &str = "https://auth.citrate.ai/oauth/callback";

/// The fixed loopback port the single-use callback listener binds (common to all
/// services — Notion reaches it via the hosted bounce, the others land directly).
pub const OAUTH_LOOPBACK_PORT: u16 = 8975;

/// PKCE code-challenge method — S256 ONLY (never `plain`).
pub const CODE_CHALLENGE_METHOD: &str = "S256";

/// An MCP-connectable service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    GitHub,
    GoogleDrive,
    Notion,
}

impl Service {
    pub fn id(self) -> &'static str {
        match self {
            Service::GitHub => "github",
            Service::GoogleDrive => "gdrive",
            Service::Notion => "notion",
        }
    }

    pub fn from_id(id: &str) -> Option<Service> {
        match id {
            "github" => Some(Service::GitHub),
            "gdrive" => Some(Service::GoogleDrive),
            "notion" => Some(Service::Notion),
            _ => None,
        }
    }

    /// The provider's authorization endpoint (where the browser is sent).
    pub fn authorize_endpoint(self) -> &'static str {
        match self {
            Service::GitHub => "https://github.com/login/oauth/authorize",
            Service::GoogleDrive => "https://accounts.google.com/o/oauth2/v2/auth",
            Service::Notion => "https://api.notion.com/v1/oauth/authorize",
        }
    }

    /// The provider's token endpoint (where the code is exchanged).
    pub fn token_endpoint(self) -> &'static str {
        match self {
            Service::GitHub => "https://github.com/login/oauth/access_token",
            Service::GoogleDrive => "https://oauth2.googleapis.com/token",
            Service::Notion => "https://api.notion.com/v1/oauth/token",
        }
    }

    /// The OAuth `redirect_uri` PARAMETER for this service — what the provider has
    /// registered and validates on both the authorize request and the token
    /// exchange. GitHub + Google accept the http loopback directly; Notion rejects a
    /// plaintext-http redirect, so it is registered against (and must be sent) the
    /// hosted https bounce, which returns the browser to the same loopback listener.
    pub fn redirect_uri(self) -> &'static str {
        match self {
            Service::GitHub | Service::GoogleDrive => OAUTH_REDIRECT_URI,
            Service::Notion => HOSTED_REDIRECT_URI,
        }
    }

    /// The narrowest scope set we request (ADR-3 — least privilege). Notion carries
    /// no OAuth scope param (capabilities are set on the integration), so it is
    /// empty here.
    pub fn default_scopes(self) -> &'static [&'static str] {
        match self {
            // read repos + open PR drafts (the member approves every write via the
            // ceremony regardless of the granted scope).
            Service::GitHub => &["repo"],
            Service::GoogleDrive => &["https://www.googleapis.com/auth/drive.readonly"],
            Service::Notion => &[],
        }
    }
}

/// A PKCE pair: the secret `verifier` (zeroized) + its public S256 `challenge`.
pub struct Pkce {
    pub verifier: Zeroizing<String>,
    pub challenge: String,
}

impl Pkce {
    /// Mint a fresh S256 pair (32 random bytes → base64url verifier; challenge =
    /// base64url(sha256(verifier))).
    pub fn new() -> Self {
        let mut raw = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut raw);
        let verifier = b64url(&raw);
        raw.zeroize_bytes();
        let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
        Pkce {
            verifier: Zeroizing::new(verifier),
            challenge,
        }
    }
}

/// 32 bytes of URL-safe randomness for the `state` CSRF token.
pub fn random_state() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let out = b64url(&raw);
    raw.zeroize_bytes();
    out
}

/// URL-safe, unpadded base64 (RFC 7636 / 7515).
fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Small helper so we can zeroize a fixed array without pulling extra traits.
trait ZeroizeBytes {
    fn zeroize_bytes(&mut self);
}
impl ZeroizeBytes for [u8; 32] {
    fn zeroize_bytes(&mut self) {
        use zeroize::Zeroize as _;
        self.zeroize();
    }
}

/// Percent-encode a query-parameter value (RFC 3986 unreserved kept; everything
/// else `%XX`). Small + dependency-free so the URL builder stays self-contained.
fn pct(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build the provider's authorization URL. Common params (client_id, redirect_uri,
/// response_type=code, state, PKCE S256) plus per-provider extras:
///  - Google: `access_type=offline` + `prompt=consent` (to receive a refresh
///    token) + space-joined scope;
///  - GitHub: space-joined scope;
///  - Notion: `owner=user` + no scope param (capabilities live on the integration).
pub fn authorize_url(service: Service, client_id: &str, state: &str, challenge: &str) -> String {
    let mut q: Vec<(String, String)> = vec![
        ("client_id".into(), client_id.into()),
        ("redirect_uri".into(), service.redirect_uri().into()),
        ("response_type".into(), "code".into()),
        ("state".into(), state.into()),
        ("code_challenge".into(), challenge.into()),
        ("code_challenge_method".into(), CODE_CHALLENGE_METHOD.into()),
    ];
    let scopes = service.default_scopes();
    if !scopes.is_empty() {
        q.push(("scope".into(), scopes.join(" ")));
    }
    match service {
        Service::GoogleDrive => {
            q.push(("access_type".into(), "offline".into()));
            q.push(("prompt".into(), "consent".into()));
        }
        Service::Notion => {
            q.push(("owner".into(), "user".into()));
        }
        Service::GitHub => {}
    }
    let query = q
        .iter()
        .map(|(k, v)| format!("{}={}", pct(k), pct(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}?{}", service.authorize_endpoint(), query)
}

/// The `application/x-www-form-urlencoded` token-exchange body params (as pairs).
/// All three send client_id + client_secret + code + redirect_uri + code_verifier;
/// Google additionally requires `grant_type=authorization_code`. (Notion also
/// accepts HTTP Basic for the client creds; sending them in the body is accepted
/// and keeps one code path — the transport layer chooses headers.)
pub fn token_exchange_params(
    service: Service,
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", service.redirect_uri().to_string()),
        ("client_id", client_id.to_string()),
        ("client_secret", client_secret.to_string()),
        ("code_verifier", verifier.to_string()),
    ];
    // GitHub's token endpoint ignores grant_type; harmless to send. Keep the body
    // uniform across providers.
    params.retain(|(_, v)| !v.is_empty());
    params
}

// ===========================================================================
// W4.2 — runtime flow: dev-credential loader, fixed-port loopback listener,
// token exchange, vault custody, and the Tauri command surface.
//
// The HTTP seam (`oidc::HttpClient` + `UreqClient`, ureq/rustls) and the custody
// vault (`CustodyVault::put`/`custody_get`/`clear_slot`) are REUSED, not
// re-implemented — this flow mirrors `oidc.rs` (Rule 9 in spirit). NO command
// returns a token or the client secret across the invoke boundary (I-2 barrier):
// `connection_start` returns a claim-free `ConnectionStatus`; the token is sealed
// in the vault and read back only by in-process code.
// ===========================================================================

use crate::custody::CustodyVault;
use crate::oidc::HttpClient;
use serde::{Deserialize, Serialize};
use tauri::State;
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroize as _;

/// How long the loopback listener waits for the provider callback before failing
/// closed (the member gets time to consent in the system browser).
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Cap on the callback request bytes we read (first request line only).
const MAX_CALLBACK_BYTES: usize = 8 * 1024;

/// The three MCP services, for status enumeration.
const ALL_SERVICES: [Service; 3] = [Service::GitHub, Service::GoogleDrive, Service::Notion];

/// Errors surfaced by the connection flow. Mapped to a `String` at the command
/// boundary; never carries a token, code, or secret.
#[derive(Debug, PartialEq, Eq)]
pub enum ConnError {
    /// No client id/secret configured for this service (dev file missing/absent).
    NotConfigured,
    /// `127.0.0.1:8975` is already bound (another sign-in in flight, or a stale one).
    PortInUse,
    /// A socket / browser-open failure.
    Network,
    /// No callback arrived within `CALLBACK_TIMEOUT`.
    Timeout,
    /// The callback `state` did not match the one we minted (CSRF — fail closed).
    StateMismatch,
    /// The provider rejected the code exchange, or the token response was unparsable.
    TokenExchange,
    /// The custody vault is locked/unavailable, or a put/get failed.
    Vault,
    /// The service id from the UI is not one of github/gdrive/notion.
    UnknownService,
}

impl std::fmt::Display for ConnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConnError::NotConfigured => "no OAuth credentials configured for this service",
            ConnError::PortInUse => "the sign-in callback port (127.0.0.1:8975) is already in use",
            ConnError::Network => "network or browser-open failure",
            ConnError::Timeout => "timed out waiting for the browser callback",
            ConnError::StateMismatch => "callback state mismatch (sign-in aborted)",
            ConnError::TokenExchange => "the provider rejected the authorization code",
            ConnError::Vault => "the secure vault is locked or unavailable",
            ConnError::UnknownService => "unknown service",
        };
        f.write_str(s)
    }
}

/// Constant-time byte compare for the `state` CSRF check (no early-return timing
/// leak on length-equal inputs).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Absolute unix seconds, saturating to 0 before the epoch.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The custody vault slot a service's token record lives under. Not `\0`-prefixed
/// and outside the backend-reserved `oidc-`/`wallet-` prefixes, so it is a valid
/// user slot.
fn slot(service: Service) -> String {
    format!("connection-{}", service.id())
}

/// The `oauth.dev.json` env-key prefix for a service (the file uses provider
/// names, not the internal ids: GitHub→GITHUB, GoogleDrive→GOOGLE, Notion→NOTION).
fn env_prefix(service: Service) -> &'static str {
    match service {
        Service::GitHub => "GITHUB",
        Service::GoogleDrive => "GOOGLE",
        Service::Notion => "NOTION",
    }
}

// ---------------------------------------------------------------------------
// Dev-credential loader
// ---------------------------------------------------------------------------

/// A provider's OAuth client credentials. The secret is `Zeroizing` so it wipes on
/// drop and never lands in a `{:?}`.
struct ClientCreds {
    client_id: String,
    client_secret: Zeroizing<String>,
}

/// The dev credential file path: `CITRATE_OAUTH_DEV_FILE` if set (tests/CI), else
/// `oauth.dev.json` relative to the working dir (dev). NOT bundled in a release —
/// the ship path seals these in the OS keyring (Settings → Connections), a
/// subsequent WP; until then, a release build has no file here and reports
/// `NotConfigured` honestly (Rule 1).
fn dev_credentials_path() -> PathBuf {
    if let Ok(p) = std::env::var("CITRATE_OAUTH_DEV_FILE") {
        return PathBuf::from(p);
    }
    PathBuf::from("oauth.dev.json")
}

/// Parse the dotenv-style `KEY=VALUE` credential file (blank lines + `#` comments
/// skipped). Despite the `.json` suffix the file is line-oriented `KEY=VALUE`.
fn parse_dev_credentials(contents: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Fixed-port loopback listener (mirrors oidc::LoopbackListener, port 8975)
// ---------------------------------------------------------------------------

/// A single-use loopback listener on the FIXED port (`OAUTH_LOOPBACK_PORT`) — the
/// providers registered `redirect_uri` targets this exact port (directly for
/// GitHub/Google; via the hosted bounce for Notion). Bound to `127.0.0.1` only
/// (never a wildcard). One accepted connection, then consumed.
struct ConnectionListener {
    listener: TcpListener,
    port: u16,
}

impl ConnectionListener {
    /// Bind the production fixed port. `PortInUse` if a prior sign-in never released
    /// it (fail closed rather than silently pick another port the provider hasn't
    /// registered).
    fn bind() -> std::result::Result<Self, ConnError> {
        Self::bind_on(OAUTH_LOOPBACK_PORT)
    }

    /// Bind `127.0.0.1:<port>`. `port = 0` (tests) lets the OS pick a free port.
    fn bind_on(port: u16) -> std::result::Result<Self, ConnError> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let listener = TcpListener::bind(addr).map_err(|_| ConnError::PortInUse)?;
        let port = listener.local_addr().map_err(|_| ConnError::Network)?.port();
        Ok(ConnectionListener { listener, port })
    }

    fn port(&self) -> u16 {
        self.port
    }

    /// Accept exactly one callback and return the parsed `(code, state)`, or fail
    /// closed on timeout. Non-blocking poll against a wall-clock deadline so a
    /// timeout drops `self` (and the socket) deterministically. Single-use.
    fn wait_for_callback(self, timeout: Duration) -> std::result::Result<CallbackParams, ConnError> {
        self.listener
            .set_nonblocking(true)
            .map_err(|_| ConnError::Network)?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _peer)) => {
                    stream
                        .set_nonblocking(false)
                        .map_err(|_| ConnError::Network)?;
                    return Self::serve_callback(stream);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(ConnError::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return Err(ConnError::Network),
            }
        }
    }

    /// Parse the request, then write a minimal close-the-tab page. A foreign or
    /// malformed request (no `code`/`state`) fails closed.
    fn serve_callback(mut stream: TcpStream) -> std::result::Result<CallbackParams, ConnError> {
        let params = Self::read_request(&mut stream);
        let body = "<!doctype html><meta charset=utf-8><title>Citrate Core</title>\
                    <body style=\"font-family:system-ui;padding:3rem;text-align:center\">\
                    <p>Connection authorized. You can close this tab and return to \
                    Citrate Core.</p>";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        params
    }

    /// Read the first request line and extract `code` + `state` from its target.
    fn read_request(stream: &mut TcpStream) -> std::result::Result<CallbackParams, ConnError> {
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|_| ConnError::Network)?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|_| ConnError::Network)?)
            .take(MAX_CALLBACK_BYTES as u64);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|_| ConnError::Network)?;
        let target = line
            .split_whitespace()
            .nth(1)
            .ok_or(ConnError::StateMismatch)?;
        parse_callback_target(target)
    }
}

/// The `(code, state)` parsed off the loopback callback. `code` is a bearer secret
/// until exchanged, so `Debug` redacts both.
#[derive(Clone, PartialEq, Eq)]
struct CallbackParams {
    code: String,
    state: String,
}

impl std::fmt::Debug for CallbackParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackParams")
            .field("code", &"<redacted>")
            .field("state", &"<redacted>")
            .finish()
    }
}

/// Parse `code` + `state` from a callback target (`/oauth/callback?code=..&state=..`).
/// A missing/empty `code` or `state` (foreign or error callback) fails closed; we
/// do not reveal which was missing.
fn parse_callback_target(target: &str) -> std::result::Result<CallbackParams, ConnError> {
    let parsed = url::Url::parse("http://127.0.0.1/")
        .and_then(|base| base.join(target))
        .map_err(|_| ConnError::StateMismatch)?;
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
        _ => Err(ConnError::StateMismatch),
    }
}

// ---------------------------------------------------------------------------
// Token exchange response + stored record + public status
// ---------------------------------------------------------------------------

/// The provider token response. Only `access_token` is required; the rest vary by
/// provider (Google → refresh_token + expires_in; GitHub → scope; Notion →
/// neither, plus workspace fields we ignore). Unknown fields are dropped by serde.
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// The record sealed in the vault. Serialized to JSON, encrypted by custody; the
/// plaintext buffer is zeroized immediately after the put.
#[derive(Serialize)]
struct StoredConnection<'a> {
    access_token: &'a str,
    refresh_token: Option<&'a str>,
    token_type: Option<&'a str>,
    scope: Option<&'a str>,
    /// Absolute unix-seconds expiry, if the provider gave one.
    expires_at: Option<u64>,
    connected_at: u64,
}

/// The non-secret metadata read back for status — deserializes the SAME record but
/// ignores the token fields (they never need to leave the vault for a status read).
#[derive(Deserialize)]
struct ConnMeta {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    connected_at: Option<u64>,
}

/// The claim-free connection status crossing the invoke boundary — NO token.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    /// Internal service id: `github` / `gdrive` / `notion`.
    pub service: String,
    pub connected: bool,
    pub scope: Option<String>,
    pub connected_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// The connection manager + its Tauri state
// ---------------------------------------------------------------------------

/// Process-wide MCP-connection manager: the HTTP client (ureq in prod, a test seam
/// in tests) + the credential-file path.
pub struct ConnectionManager {
    http: Box<dyn HttpClient>,
    creds_path: PathBuf,
}

impl ConnectionManager {
    pub fn new(http: Box<dyn HttpClient>, creds_path: PathBuf) -> Self {
        ConnectionManager { http, creds_path }
    }

    /// Load a service's client credentials from the dev file. Missing file or key →
    /// `NotConfigured` (honest: the flow cannot start without real creds).
    fn creds(&self, service: Service) -> std::result::Result<ClientCreds, ConnError> {
        let contents =
            std::fs::read_to_string(&self.creds_path).map_err(|_| ConnError::NotConfigured)?;
        let map = parse_dev_credentials(&contents);
        let prefix = env_prefix(service);
        let id = map
            .get(&format!("{prefix}_CLIENT_ID"))
            .filter(|s| !s.is_empty())
            .cloned()
            .ok_or(ConnError::NotConfigured)?;
        let secret = map
            .get(&format!("{prefix}_CLIENT_SECRET"))
            .filter(|s| !s.is_empty())
            .cloned()
            .ok_or(ConnError::NotConfigured)?;
        Ok(ClientCreds {
            client_id: id,
            client_secret: Zeroizing::new(secret),
        })
    }

    /// Run the full connect flow: bind the fixed loopback, mint PKCE + state, open
    /// the authorize URL in the system browser (via `open`), await one callback,
    /// verify `state` (constant-time), then exchange + seal the token. Blocking
    /// (the caller runs it off the main thread).
    pub fn connect(
        &self,
        service: Service,
        vault: &CustodyVault,
        open: impl Fn(&str) -> std::result::Result<(), ConnError>,
    ) -> std::result::Result<ConnectionStatus, ConnError> {
        // Fail fast before opening a browser / binding a socket if unconfigured.
        let creds = self.creds(service)?;
        let listener = ConnectionListener::bind()?;
        let state = random_state();
        let pkce = Pkce::new();
        let auth_url = authorize_url(service, &creds.client_id, &state, &pkce.challenge);
        open(&auth_url)?;
        let cb = listener.wait_for_callback(CALLBACK_TIMEOUT)?;
        if !ct_eq(cb.state.as_bytes(), state.as_bytes()) {
            return Err(ConnError::StateMismatch);
        }
        self.exchange_and_store(service, &creds, vault, &cb.code, &pkce.verifier)
    }

    /// Exchange the authorization `code` (with the PKCE verifier) and seal the
    /// resulting token in the vault. Factored out so it is unit-tested over an
    /// injected HTTP seam + a headless vault, without a live provider or socket.
    fn exchange_and_store(
        &self,
        service: Service,
        creds: &ClientCreds,
        vault: &CustodyVault,
        code: &str,
        verifier: &str,
    ) -> std::result::Result<ConnectionStatus, ConnError> {
        let params = token_exchange_params(
            service,
            &creds.client_id,
            creds.client_secret.as_str(),
            code,
            verifier,
        );
        let form: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let body = self
            .http
            .post_form(service.token_endpoint(), &form)
            .map_err(|_| ConnError::TokenExchange)?;
        let tok: TokenResponse =
            serde_json::from_str(&body).map_err(|_| ConnError::TokenExchange)?;

        let connected_at = now_unix();
        let expires_at = tok.expires_in.map(|e| connected_at.saturating_add(e));
        let record = StoredConnection {
            access_token: &tok.access_token,
            refresh_token: tok.refresh_token.as_deref(),
            token_type: tok.token_type.as_deref(),
            scope: tok.scope.as_deref(),
            expires_at,
            connected_at,
        };
        let mut bytes = serde_json::to_vec(&record).map_err(|_| ConnError::TokenExchange)?;
        let put = vault.put(&slot(service), &mut bytes);
        bytes.zeroize();
        put.map_err(|_| ConnError::Vault)?;

        Ok(ConnectionStatus {
            service: service.id().to_string(),
            connected: true,
            scope: tok.scope,
            connected_at: Some(connected_at),
        })
    }

    /// The connection status of all three services. A present, readable vault slot
    /// is `connected`; an absent slot OR a locked vault reads as not-connected
    /// (a status read never forces an unlock and never surfaces the token).
    pub fn status(&self, vault: &CustodyVault) -> Vec<ConnectionStatus> {
        ALL_SERVICES
            .iter()
            .map(|&svc| match vault.custody_get(&slot(svc)) {
                Ok(bytes) => {
                    let meta: ConnMeta = serde_json::from_slice(&bytes).unwrap_or(ConnMeta {
                        scope: None,
                        connected_at: None,
                    });
                    ConnectionStatus {
                        service: svc.id().to_string(),
                        connected: true,
                        scope: meta.scope,
                        connected_at: meta.connected_at,
                    }
                }
                Err(_) => ConnectionStatus {
                    service: svc.id().to_string(),
                    connected: false,
                    scope: None,
                    connected_at: None,
                },
            })
            .collect()
    }

    /// Forget a service's token (clear the vault slot). Idempotent-ish: a missing
    /// slot is not an error worth surfacing differently.
    pub fn disconnect(
        &self,
        service: Service,
        vault: &CustodyVault,
    ) -> std::result::Result<(), ConnError> {
        vault
            .clear_slot(&slot(service))
            .map_err(|_| ConnError::Vault)
    }
}

/// Managed Tauri state: the process-wide connection manager.
pub struct ConnectionState(pub ConnectionManager);

/// Build the connection state for a Tauri build: production ureq client + the dev
/// credential path.
pub fn build_connection_state() -> ConnectionState {
    ConnectionState(ConnectionManager::new(
        Box::new(crate::oidc::UreqClient),
        dev_credentials_path(),
    ))
}

// ---------------------------------------------------------------------------
// Tauri command surface (I-2: none returns a token or secret)
// ---------------------------------------------------------------------------

/// `connection_start` — run the loopback-PKCE flow for one MCP service, opening the
/// provider's authorize page in the SYSTEM browser (third-party OAuth forbids
/// embedded webviews). ASYNC so Tauri runs it off the main thread while the
/// loopback blocks. Returns a claim-free `ConnectionStatus`; the token is sealed in
/// the vault, never returned.
#[tauri::command]
pub async fn connection_start(
    app: tauri::AppHandle,
    state: State<'_, ConnectionState>,
    custody: State<'_, crate::custody::CustodyState>,
    service: String,
) -> std::result::Result<ConnectionStatus, String> {
    let svc = Service::from_id(&service).ok_or_else(|| ConnError::UnknownService.to_string())?;
    let app_for_open = app.clone();
    let open = move |url: &str| -> std::result::Result<(), ConnError> {
        use tauri_plugin_opener::OpenerExt;
        app_for_open
            .opener()
            .open_url(url.to_string(), None::<&str>)
            .map_err(|_| ConnError::Network)
    };
    state
        .0
        .connect(svc, &custody.0, open)
        .map_err(|e| e.to_string())
}

/// `connection_status` — the connect/disconnect state of all three services. No
/// token crosses the boundary.
#[tauri::command]
pub fn connection_status(
    state: State<'_, ConnectionState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<Vec<ConnectionStatus>, String> {
    Ok(state.0.status(&custody.0))
}

/// `connection_disconnect` — forget a service's sealed token.
#[tauri::command]
pub fn connection_disconnect(
    state: State<'_, ConnectionState>,
    custody: State<'_, crate::custody::CustodyState>,
    service: String,
) -> std::result::Result<(), String> {
    let svc = Service::from_id(&service).ok_or_else(|| ConnError::UnknownService.to_string())?;
    state
        .0
        .disconnect(svc, &custody.0)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_ids_round_trip() {
        for s in [Service::GitHub, Service::GoogleDrive, Service::Notion] {
            assert_eq!(Service::from_id(s.id()), Some(s));
        }
        assert_eq!(Service::from_id("slack"), None);
    }

    #[test]
    fn pkce_is_s256_and_verifier_differs_from_challenge() {
        let p = Pkce::new();
        assert_ne!(&*p.verifier as &str, p.challenge);
        // challenge = base64url(sha256(verifier))
        let expect = b64url(&Sha256::digest(p.verifier.as_bytes()));
        assert_eq!(p.challenge, expect);
        // base64url no-pad → no '=', '+', '/'
        assert!(!p.challenge.contains('=') && !p.challenge.contains('+') && !p.challenge.contains('/'));
    }

    #[test]
    fn state_is_high_entropy_and_unique() {
        assert_ne!(random_state(), random_state());
        assert!(random_state().len() >= 40);
    }

    #[test]
    fn authorize_url_carries_common_params_and_encodes_scope() {
        let url = authorize_url(Service::GoogleDrive, "client-123", "st4te", "ch4llenge");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=client-123"));
        // redirect_uri percent-encoded (the fixed loopback).
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8975%2Foauth%2Fcallback"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=st4te"));
        assert!(url.contains("code_challenge=ch4llenge"));
        assert!(url.contains("code_challenge_method=S256"));
        // Google specifics: offline + consent (for a refresh token) + encoded scope.
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.readonly"));
    }

    #[test]
    fn github_authorize_has_repo_scope_no_google_params() {
        let url = authorize_url(Service::GitHub, "gh_id", "s", "c");
        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(url.contains("scope=repo"));
        assert!(!url.contains("access_type="));
        assert!(!url.contains("owner="));
    }

    #[test]
    fn notion_authorize_has_owner_user_and_no_scope_param() {
        let url = authorize_url(Service::Notion, "n_id", "s", "c");
        assert!(url.starts_with("https://api.notion.com/v1/oauth/authorize?"));
        assert!(url.contains("owner=user"));
        assert!(!url.contains("scope="), "Notion sets capabilities on the integration, not a scope param");
    }

    #[test]
    fn redirect_uri_is_loopback_for_github_and_google_but_hosted_for_notion() {
        // GitHub + Google accept the plaintext-http loopback redirect directly.
        assert_eq!(Service::GitHub.redirect_uri(), OAUTH_REDIRECT_URI);
        assert_eq!(Service::GoogleDrive.redirect_uri(), OAUTH_REDIRECT_URI);
        assert_eq!(Service::GitHub.redirect_uri(), "http://127.0.0.1:8975/oauth/callback");
        // Notion rejects the http loopback → the hosted https bounce is registered.
        assert_eq!(Service::Notion.redirect_uri(), HOSTED_REDIRECT_URI);
        assert_eq!(Service::Notion.redirect_uri(), "https://auth.citrate.ai/oauth/callback");
    }

    #[test]
    fn notion_authorize_uses_the_hosted_https_redirect_not_the_loopback() {
        let url = authorize_url(Service::Notion, "n_id", "s", "c");
        // The redirect_uri PARAMETER Notion validates must be the hosted https URL,
        // percent-encoded — NOT the loopback (which Notion would reject).
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Fauth.citrate.ai%2Foauth%2Fcallback"),
            "Notion authorize must carry the hosted https redirect_uri: {url}"
        );
        assert!(
            !url.contains("127.0.0.1"),
            "Notion authorize must NOT send the http loopback as redirect_uri: {url}"
        );
    }

    #[test]
    fn token_exchange_redirect_uri_matches_the_authorize_redirect_per_service() {
        // OAuth requires the token-exchange redirect_uri to match the authorize one.
        // Notion → hosted https; GitHub/Google → loopback.
        let notion = token_exchange_params(Service::Notion, "id", "sec", "c", "v");
        let get = |p: &[(&'static str, String)], k: &str| {
            p.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())
        };
        assert_eq!(get(&notion, "redirect_uri").as_deref(), Some(HOSTED_REDIRECT_URI));
        let gh = token_exchange_params(Service::GitHub, "id", "sec", "c", "v");
        assert_eq!(get(&gh, "redirect_uri").as_deref(), Some(OAUTH_REDIRECT_URI));
    }

    #[test]
    fn token_exchange_params_carry_pkce_and_creds_and_drop_empties() {
        let p = token_exchange_params(Service::GitHub, "id", "secret", "the-code", "the-verifier");
        let get = |k: &str| p.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("grant_type"), Some("authorization_code"));
        assert_eq!(get("code"), Some("the-code"));
        assert_eq!(get("code_verifier"), Some("the-verifier"));
        assert_eq!(get("client_id"), Some("id"));
        assert_eq!(get("client_secret"), Some("secret"));
        assert_eq!(get("redirect_uri"), Some(OAUTH_REDIRECT_URI));
        // an empty secret (public client) is dropped rather than sent blank
        let pub_client = token_exchange_params(Service::GoogleDrive, "id", "", "c", "v");
        assert!(pub_client.iter().all(|(k, _)| *k != "client_secret"));
    }

    // =======================================================================
    // W4.2 runtime flow — loader, listener, exchange+custody. The vault runs
    // headless over an in-memory keyring; the token exchange runs over an
    // injected HTTP seam (a trait impl, the same pattern oidc_tests/ai_tests
    // use — Rule 1: a TEST seam, never a live socket presented as real). The
    // listener test uses a REAL loopback socket.
    // =======================================================================

    use crate::custody::{CustodyError, CustodyVault, Keyring};
    use std::collections::HashMap as StdHashMap;
    use std::net::{Ipv4Addr, TcpStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;

    const PASS: &[u8] = b"correct horse battery staple";

    #[derive(Default)]
    struct FakeKeyring {
        store: StdMutex<StdHashMap<String, Vec<u8>>>,
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

    /// A fresh, unlocked custody vault at a unique temp path.
    fn fresh_vault() -> CustodyVault {
        let mut p = std::env::temp_dir();
        let uniq = format!("citrate-core-conn-test-{}-{}.enc", std::process::id(), {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            N.fetch_add(1, Ordering::Relaxed)
        });
        p.push(uniq);
        let _ = std::fs::remove_file(&p);
        let vault = CustodyVault::new(Box::new(FakeKeyring::default()), p, 30);
        vault.init(&mut PASS.to_vec()).unwrap();
        vault.unlock(&mut PASS.to_vec()).unwrap();
        vault
    }

    /// An injected HTTP seam: records the last request and returns a scripted token
    /// body. Shared via `Arc` so the test can inspect what was sent.
    struct FakeHttp {
        last_url: StdMutex<Option<String>>,
        last_form: StdMutex<Vec<(String, String)>>,
        response: String,
    }
    struct SharedHttp(Arc<FakeHttp>);
    impl crate::oidc::HttpClient for SharedHttp {
        fn get(
            &self,
            _url: &str,
            _bearer: Option<&str>,
        ) -> std::result::Result<String, crate::oidc::AuthError> {
            Ok(String::new())
        }
        fn post_form(
            &self,
            url: &str,
            form: &[(&str, &str)],
        ) -> std::result::Result<String, crate::oidc::AuthError> {
            *self.0.last_url.lock().unwrap() = Some(url.to_string());
            *self.0.last_form.lock().unwrap() =
                form.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            Ok(self.0.response.clone())
        }
    }

    #[test]
    fn parse_dev_credentials_skips_comments_blanks_and_trims() {
        let raw = "# a comment\n\nGITHUB_CLIENT_ID = abc \n  \nNOTION_CLIENT_SECRET=sek\n";
        let m = parse_dev_credentials(raw);
        assert_eq!(m.get("GITHUB_CLIENT_ID").map(String::as_str), Some("abc"));
        assert_eq!(m.get("NOTION_CLIENT_SECRET").map(String::as_str), Some("sek"));
        assert!(!m.contains_key("# a comment"));
    }

    #[test]
    fn env_prefix_maps_service_to_file_key() {
        assert_eq!(env_prefix(Service::GitHub), "GITHUB");
        assert_eq!(env_prefix(Service::GoogleDrive), "GOOGLE");
        assert_eq!(env_prefix(Service::Notion), "NOTION");
    }

    #[test]
    fn creds_reads_id_and_secret_from_file_and_missing_is_not_configured() {
        let mut p = std::env::temp_dir();
        p.push(format!("citrate-conn-creds-{}.json", std::process::id()));
        std::fs::write(&p, "NOTION_CLIENT_ID=nid\nNOTION_CLIENT_SECRET=nsec\n").unwrap();
        let mgr = ConnectionManager::new(Box::new(SharedHttp(Arc::new(FakeHttp {
            last_url: StdMutex::new(None),
            last_form: StdMutex::new(Vec::new()),
            response: String::new(),
        }))), p.clone());
        let c = mgr.creds(Service::Notion).unwrap();
        assert_eq!(c.client_id, "nid");
        assert_eq!(c.client_secret.as_str(), "nsec");
        // GitHub keys are absent → NotConfigured (honest). Match rather than
        // unwrap_err so ClientCreds need not derive Debug (it holds a secret).
        assert!(matches!(
            mgr.creds(Service::GitHub),
            Err(ConnError::NotConfigured)
        ));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn ct_eq_is_length_and_content_sensitive() {
        assert!(ct_eq(b"abcdef", b"abcdef"));
        assert!(!ct_eq(b"abcdef", b"abcdeg"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }

    #[test]
    fn slot_is_namespaced_and_not_backend_reserved() {
        assert_eq!(slot(Service::Notion), "connection-notion");
        assert!(!crate::custody::is_backend_reserved_slot(&slot(Service::Notion)));
        assert!(!slot(Service::GitHub).starts_with('\0'));
    }

    #[test]
    fn parse_callback_target_extracts_code_and_state() {
        let p = parse_callback_target("/oauth/callback?code=the-code&state=st8").unwrap();
        assert_eq!(p.code, "the-code");
        assert_eq!(p.state, "st8");
    }

    #[test]
    fn parse_callback_target_missing_param_fails_closed() {
        // A denial (error=access_denied, no code) or foreign hit → StateMismatch.
        assert_eq!(
            parse_callback_target("/oauth/callback?error=access_denied&state=s").unwrap_err(),
            ConnError::StateMismatch
        );
        assert_eq!(
            parse_callback_target("/oauth/callback?code=c").unwrap_err(),
            ConnError::StateMismatch
        );
    }

    #[test]
    fn listener_round_trip_parses_callback_over_a_real_socket() {
        let listener = ConnectionListener::bind_on(0).unwrap();
        let port = listener.port();
        let h = std::thread::spawn(move || {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            s.write_all(
                b"GET /oauth/callback?code=abc123&state=xyz789 HTTP/1.1\r\nHost: x\r\n\r\n",
            )
            .unwrap();
        });
        let cb = listener.wait_for_callback(Duration::from_secs(5)).unwrap();
        h.join().unwrap();
        assert_eq!(cb.code, "abc123");
        assert_eq!(cb.state, "xyz789");
    }

    #[test]
    fn listener_times_out_when_no_callback_arrives() {
        let listener = ConnectionListener::bind_on(0).unwrap();
        assert_eq!(
            listener.wait_for_callback(Duration::from_millis(80)).unwrap_err(),
            ConnError::Timeout
        );
    }

    #[test]
    fn exchange_and_store_seals_token_sends_hosted_redirect_and_hides_it_from_status() {
        let fake = Arc::new(FakeHttp {
            last_url: StdMutex::new(None),
            last_form: StdMutex::new(Vec::new()),
            // A Notion-shaped response: access_token + workspace fields we ignore.
            response: r#"{"access_token":"ntn_tok_secret","token_type":"bearer","bot_id":"b","workspace_id":"w"}"#.to_string(),
        });
        let mgr = ConnectionManager::new(Box::new(SharedHttp(fake.clone())), PathBuf::from("unused"));
        let vault = fresh_vault();
        let creds = ClientCreds {
            client_id: "n_id".to_string(),
            client_secret: Zeroizing::new("n_secret".to_string()),
        };
        let status = mgr
            .exchange_and_store(Service::Notion, &creds, &vault, "the-code", "the-verifier")
            .unwrap();

        // The status carries NO token, only connect facts.
        assert_eq!(status.service, "notion");
        assert!(status.connected);
        assert!(status.connected_at.is_some());

        // The exchange sent the HOSTED https redirect_uri (Notion), the code, the
        // PKCE verifier, and the client creds.
        let form = fake.last_form.lock().unwrap().clone();
        let get = |k: &str| form.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("redirect_uri").as_deref(), Some(HOSTED_REDIRECT_URI));
        assert_eq!(get("code").as_deref(), Some("the-code"));
        assert_eq!(get("code_verifier").as_deref(), Some("the-verifier"));
        assert_eq!(get("client_id").as_deref(), Some("n_id"));
        assert_eq!(get("client_secret").as_deref(), Some("n_secret"));
        assert_eq!(
            fake.last_url.lock().unwrap().as_deref(),
            Some("https://api.notion.com/v1/oauth/token")
        );

        // The token IS sealed in the vault (readable only in-process).
        let sealed = vault.custody_get(&slot(Service::Notion)).unwrap();
        assert!(String::from_utf8_lossy(&sealed).contains("ntn_tok_secret"));
    }

    #[test]
    fn status_reflects_stored_then_disconnected() {
        let fake = Arc::new(FakeHttp {
            last_url: StdMutex::new(None),
            last_form: StdMutex::new(Vec::new()),
            response: r#"{"access_token":"t","scope":"repo"}"#.to_string(),
        });
        let mgr = ConnectionManager::new(Box::new(SharedHttp(fake)), PathBuf::from("unused"));
        let vault = fresh_vault();
        let creds = ClientCreds {
            client_id: "g".to_string(),
            client_secret: Zeroizing::new("s".to_string()),
        };
        // Nothing connected yet.
        let before = mgr.status(&vault);
        assert!(before.iter().all(|s| !s.connected));

        mgr.exchange_and_store(Service::GitHub, &creds, &vault, "c", "v").unwrap();
        let after = mgr.status(&vault);
        let gh = after.iter().find(|s| s.service == "github").unwrap();
        assert!(gh.connected);
        assert_eq!(gh.scope.as_deref(), Some("repo"));
        // The other two remain disconnected.
        assert!(after.iter().filter(|s| s.service != "github").all(|s| !s.connected));

        // Disconnect forgets it.
        mgr.disconnect(Service::GitHub, &vault).unwrap();
        let gone = mgr.status(&vault);
        assert!(gone.iter().find(|s| s.service == "github").unwrap().connected == false);
    }
}
