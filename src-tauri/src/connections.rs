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

use crate::secret::SecretString;
use base64::Engine as _;
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

// 0.6.0 L6-WP05: the OAuth client v2 (provider registry, relay mode, refresh, revoke). Declared
// here rather than in lib.rs (L0's file) so the lane stays inside its own files; its path is the
// lane's `src-tauri/src/oauth_client/`. L0 may hoist it to the crate root without changing it.
#[path = "oauth_client/mod.rs"]
pub(crate) mod oauth_client;

// 0.6.0 L6-WP06: MCP remote-server authorization, declared here for the same reason (its path is
// the lane's `src-tauri/src/mcp_auth/`). It reuses this file's PKCE, state and loopback pieces.
#[path = "mcp_auth/mod.rs"]
pub(crate) mod mcp_auth;

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
    /// Hugging Face — model downloads (public need no token; gated/private need `read-repos`).
    /// Added for Commons CX-S1 (model catalog). Standard OAuth2 + PKCE; loopback redirect OK.
    HuggingFace,
    /// HUP-S10.2: Google Sheets (read ranges, append rows). Same Google OAuth client as Drive.
    GoogleSheets,
    /// HUP-S10.2: Google Calendar (list events, create an event). Same Google OAuth client.
    GoogleCalendar,
}

impl Service {
    pub fn id(self) -> &'static str {
        match self {
            Service::GitHub => "github",
            Service::GoogleDrive => "gdrive",
            Service::Notion => "notion",
            Service::HuggingFace => "hf",
            Service::GoogleSheets => "gsheets",
            Service::GoogleCalendar => "gcal",
        }
    }

    pub fn from_id(id: &str) -> Option<Service> {
        match id {
            "github" => Some(Service::GitHub),
            "gdrive" => Some(Service::GoogleDrive),
            "notion" => Some(Service::Notion),
            "hf" => Some(Service::HuggingFace),
            "gsheets" => Some(Service::GoogleSheets),
            "gcal" => Some(Service::GoogleCalendar),
            _ => None,
        }
    }

    /// The provider's authorization endpoint (where the browser is sent).
    pub fn authorize_endpoint(self) -> &'static str {
        match self {
            Service::GitHub => "https://github.com/login/oauth/authorize",
            Service::GoogleDrive | Service::GoogleSheets | Service::GoogleCalendar => {
                "https://accounts.google.com/o/oauth2/v2/auth"
            }
            Service::Notion => "https://api.notion.com/v1/oauth/authorize",
            Service::HuggingFace => "https://huggingface.co/oauth/authorize",
        }
    }

    /// The provider's token endpoint (where the code is exchanged).
    pub fn token_endpoint(self) -> &'static str {
        match self {
            Service::GitHub => "https://github.com/login/oauth/access_token",
            Service::GoogleDrive | Service::GoogleSheets | Service::GoogleCalendar => {
                "https://oauth2.googleapis.com/token"
            }
            Service::Notion => "https://api.notion.com/v1/oauth/token",
            Service::HuggingFace => "https://huggingface.co/oauth/token",
        }
    }

    /// The OAuth `redirect_uri` PARAMETER for this service — what the provider has
    /// registered and validates on both the authorize request and the token
    /// exchange. GitHub + Google accept the http loopback directly; Notion rejects a
    /// plaintext-http redirect, so it is registered against (and must be sent) the
    /// hosted https bounce, which returns the browser to the same loopback listener.
    pub fn redirect_uri(self) -> &'static str {
        match self {
            Service::GitHub
            | Service::GoogleDrive
            | Service::GoogleSheets
            | Service::GoogleCalendar
            | Service::HuggingFace => OAUTH_REDIRECT_URI,
            Service::Notion => HOSTED_REDIRECT_URI,
        }
    }

    /// The narrowest scope set we request (ADR-3 — least privilege). Notion carries
    /// no OAuth scope param (capabilities are set on the integration), so it is
    /// empty here.
    pub fn default_scopes(self) -> &'static [&'static str] {
        match self {
            // read repos + open PR drafts (the member approves every write via the
            // ceremony regardless of the granted scope). L6-WP05: `read:org` as the runbook
            // registers it (docs/CONNECTIONS_RUNBOOK.md section 2), so org repos and teams list.
            Service::GitHub => &["repo", "read:org"],
            Service::GoogleDrive => &["https://www.googleapis.com/auth/drive.readonly"],
            Service::Notion => &[],
            Service::HuggingFace => &["read-repos"],
            Service::GoogleSheets => &["https://www.googleapis.com/auth/spreadsheets"],
            Service::GoogleCalendar => &["https://www.googleapis.com/auth/calendar.events"],
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
        Service::GoogleDrive | Service::GoogleSheets | Service::GoogleCalendar => {
            q.push(("access_type".into(), "offline".into()));
            q.push(("prompt".into(), "consent".into()));
        }
        Service::Notion => {
            q.push(("owner".into(), "user".into()));
        }
        Service::GitHub => {}
        Service::HuggingFace => {}
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
#[cfg(debug_assertions)]
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;
use zeroize::Zeroize as _;

/// How long the loopback listener waits for the provider callback before failing
/// closed (the member gets time to consent in the system browser).
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Cap on the callback request bytes we read (first request line only).
const MAX_CALLBACK_BYTES: usize = 8 * 1024;

/// The three MCP services, for status enumeration.
pub(crate) const ALL_SERVICES: [Service; 6] = [
    Service::GitHub,
    Service::GoogleDrive,
    Service::Notion,
    Service::HuggingFace,
    Service::GoogleSheets,
    Service::GoogleCalendar,
];

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
    /// L6-WP05: the member declined on the provider's consent page (`error=access_denied` on
    /// the callback). Nothing was exchanged or stored.
    Declined,
    /// L6-WP05: the relay needs a signed-in member and there is no live Citrate session.
    NotSignedIn,
    /// L6-WP05: the relay answered 429.
    RateLimited,
    /// L6-WP05: the stored token has expired and could not be refreshed; connect again.
    ReauthRequired,
    /// L6-WP05: no token is stored for this service.
    NotConnected,
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
            ConnError::Declined => {
                "the sign-in was declined at the provider; nothing was connected"
            }
            ConnError::NotSignedIn => "Sign in to Citrate to connect",
            ConnError::RateLimited => "the Citrate relay is busy; try again in a minute",
            ConnError::ReauthRequired => "the connection expired; connect it again",
            ConnError::NotConnected => "not connected",
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
pub(crate) fn slot(service: Service) -> String {
    format!("connection-{}", service.id())
}

/// The `oauth.dev.json` env-key prefix for a service (the file uses provider
/// names, not the internal ids: GitHub→GITHUB, GoogleDrive→GOOGLE, Notion→NOTION).
/// Debug builds only (L6-WP05): a release build has no dev file path at all.
#[cfg(debug_assertions)]
fn env_prefix(service: Service) -> &'static str {
    match service {
        Service::GitHub => "GITHUB",
        Service::GoogleDrive => "GOOGLE",
        Service::Notion => "NOTION",
        Service::HuggingFace => "HF",
        // HUP-S10.2: one Google OAuth client (Desktop app type) serves Drive, Sheets and Calendar.
        Service::GoogleSheets | Service::GoogleCalendar => "GOOGLE",
    }
}

// ---------------------------------------------------------------------------
// Dev-credential loader (debug builds only, L6-WP05)
//
// A release build compiles none of this: it holds no client secret and never reads a
// credential file. It connects through the provider registry (`oauth_client`), behind
// `CONNECTIONS_V2`; with the flag off it reports `NotConfigured`, never the dev file.
// ---------------------------------------------------------------------------

/// A provider's OAuth client credentials. The secret is `Zeroizing` so it wipes on
/// drop and never lands in a `{:?}`.
#[cfg(debug_assertions)]
struct ClientCreds {
    client_id: String,
    client_secret: Zeroizing<String>,
}

/// The dev credential file path: `CITRATE_OAUTH_DEV_FILE` if set (tests/CI), else
/// `oauth.dev.json` relative to the working dir (dev). Debug builds only: a release
/// build exchanges through the Citrate relay and never holds a client secret
/// (L6-WP05, `oauth_client`).
#[cfg(debug_assertions)]
fn dev_credentials_path() -> PathBuf {
    if let Ok(p) = std::env::var("CITRATE_OAUTH_DEV_FILE") {
        return PathBuf::from(p);
    }
    PathBuf::from("oauth.dev.json")
}

/// Parse the dotenv-style `KEY=VALUE` credential file (blank lines + `#` comments
/// skipped). Despite the `.json` suffix the file is line-oriented `KEY=VALUE`.
#[cfg(debug_assertions)]
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
        // Fail closed if the OS-assigned port can't be read back (tests bind on 0).
        listener.local_addr().map_err(|_| ConnError::Network)?;
        Ok(ConnectionListener { listener })
    }

    #[cfg(test)]
    fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// Wait for THE sign-in callback: the first request whose `state` matches
    /// `expected_state` (constant-time), or fail closed on timeout. Non-blocking poll
    /// against a wall-clock deadline so a timeout drops `self` (and the socket)
    /// deterministically.
    ///
    /// PBA-L7b-009: the port is fixed and any web page can make the browser hit
    /// `127.0.0.1:8975` (`<img src=…>`). The old listener consumed the FIRST
    /// connection, so such a drive-by request burned the member's sign-in with a
    /// `StateMismatch`. Now a foreign / malformed / wrong-state request gets a 400 and
    /// the listener keeps waiting for the real callback; only a request carrying the
    /// flow's own `state` completes it (still exactly one code is accepted).
    ///
    /// L6-WP05: a provider error callback (`error=access_denied`) carrying the flow's own
    /// `state` ends the flow at once with `Declined` (the member said no; nothing is stored).
    /// An error callback with any other `state` is a drive-by like the rest and changes nothing.
    fn wait_for_callback(
        self,
        timeout: Duration,
        expected_state: &str,
    ) -> std::result::Result<CallbackParams, ConnError> {
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
                    match Self::serve_callback(stream, expected_state) {
                        Served::Code(cb) => return Ok(cb),
                        Served::Declined => return Err(ConnError::Declined),
                        Served::Foreign => {}
                    }
                    // Not our callback — keep listening until the deadline.
                    if Instant::now() >= deadline {
                        return Err(ConnError::Timeout);
                    }
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

    /// Parse the request; if it is THE callback (`code` + the expected `state`) write a
    /// minimal close-the-tab page and return it; if it is the provider's error callback for
    /// this flow, say nothing was connected and return `Declined`; otherwise answer 400.
    fn serve_callback(mut stream: TcpStream, expected_state: &str) -> Served {
        let served = match Self::read_request(&mut stream) {
            Ok(target) => classify_callback(&target, expected_state),
            Err(_) => Served::Foreign,
        };
        let (status, body) = match &served {
            Served::Code(_) => (
                "200 OK",
                "<!doctype html><meta charset=utf-8><title>Citrate Core</title>\
                 <body style=\"font-family:system-ui;padding:3rem;text-align:center\">\
                 <p>Connection authorized. You can close this tab and return to \
                 Citrate Core.</p>",
            ),
            Served::Declined => (
                "200 OK",
                "<!doctype html><meta charset=utf-8><title>Citrate Core</title>\
                 <body style=\"font-family:system-ui;padding:3rem;text-align:center\">\
                 <p>Connection not authorized. Nothing was connected. You can close this \
                 tab and return to Citrate Core.</p>",
            ),
            Served::Foreign => ("400 Bad Request", "not the sign-in callback"),
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        served
    }

    /// Read the first request line and return its target (path and query).
    fn read_request(stream: &mut TcpStream) -> std::result::Result<String, ConnError> {
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|_| ConnError::Network)?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|_| ConnError::Network)?)
            .take(MAX_CALLBACK_BYTES as u64);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|_| ConnError::Network)?;
        let target = line
            .split_whitespace()
            .nth(1)
            .ok_or(ConnError::StateMismatch)?;
        Ok(target.to_string())
    }
}

/// What one request to the loopback turned out to be (L6-WP05).
enum Served {
    /// The flow's own callback with a code.
    Code(CallbackParams),
    /// The provider's error callback for this flow (the member declined consent).
    Declined,
    /// Anything else: a drive-by, a wrong `state`, a malformed request.
    Foreign,
}

/// Classify a callback target against the flow's `state` (constant-time compare on both the
/// code and the error path, so a page that does not know `state` cannot end the flow).
fn classify_callback(target: &str, expected_state: &str) -> Served {
    if let Ok(cb) = parse_callback_target(target) {
        if ct_eq(cb.state.as_bytes(), expected_state.as_bytes()) {
            return Served::Code(cb);
        }
        return Served::Foreign;
    }
    match parse_callback_denial(target) {
        Some(state) if ct_eq(state.as_bytes(), expected_state.as_bytes()) => Served::Declined,
        _ => Served::Foreign,
    }
}

/// The `state` of a provider error callback (`?error=access_denied&state=..`, RFC 6749
/// 4.1.2.1), when the target is one: an `error`, a `state`, and no `code`.
fn parse_callback_denial(target: &str) -> Option<String> {
    let parsed = url::Url::parse("http://127.0.0.1/")
        .and_then(|base| base.join(target))
        .ok()?;
    let mut error = false;
    let mut code = false;
    let mut state = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "error" => error = !v.is_empty(),
            "code" => code = true,
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    match state {
        Some(st) if error && !code && !st.is_empty() => Some(st),
        _ => None,
    }
}

/// The `(code, state)` parsed off the loopback callback. `code` is a bearer secret
/// until exchanged, so `Debug` redacts both.
#[derive(Clone, PartialEq, Eq)]
struct CallbackParams {
    code: String,
    state: String,
    /// L6-WP06: the RFC 9207 `iss` response parameter, when the server sent one.
    iss: Option<String>,
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
    let mut iss = None;
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "iss" => iss = Some(v.into_owned()),
            _ => {}
        }
    }
    match (code, state) {
        (Some(code), Some(state)) if !code.is_empty() && !state.is_empty() => {
            Ok(CallbackParams { code, state, iss })
        }
        _ => Err(ConnError::StateMismatch),
    }
}

// ---------------------------------------------------------------------------
// Reusable PUBLIC-client loopback-PKCE capture (social identity — ADR-2026-08-30)
// ---------------------------------------------------------------------------

/// Run the loopback + PKCE dance for a PUBLIC client (no client secret, none embedded): bind the
/// fixed callback port, mint `state` + a fresh S256 PKCE pair, build + open the provider's authorize
/// URL in the system browser, await exactly one callback, verify `state` constant-time, and return
/// the authorization `code` + the PKCE `verifier`. The caller exchanges the code sending the
/// verifier (a public client sends the verifier, never a secret). Blocking — run off the main
/// thread. Reuses the same single-use listener the MCP flow uses (so the two never bind at once).
pub(crate) fn capture_public_pkce(
    build_authorize_url: impl FnOnce(&str, &str) -> String,
    open: impl Fn(&str) -> std::result::Result<(), ConnError>,
) -> std::result::Result<(String, Zeroizing<String>), ConnError> {
    let listener = ConnectionListener::bind()?;
    let state = random_state();
    let pkce = Pkce::new();
    let auth_url = build_authorize_url(&state, &pkce.challenge);
    open(&auth_url)?;
    let cb = listener.wait_for_callback(CALLBACK_TIMEOUT, &state)?;
    // Defence in depth: the listener only returns a state-matching callback.
    if !ct_eq(cb.state.as_bytes(), state.as_bytes()) {
        return Err(ConnError::StateMismatch);
    }
    Ok((cb.code, pkce.verifier))
}

// ---------------------------------------------------------------------------
// Token exchange response + stored record + public status
// ---------------------------------------------------------------------------

/// The provider token response. Only `access_token` is required; the rest vary by
/// provider (Google → refresh_token + expires_in; GitHub → scope; Notion →
/// neither, plus workspace fields we ignore). Unknown fields are dropped by serde.
#[derive(Deserialize, Debug)]
struct TokenResponse {
    access_token: SecretString,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<SecretString>,
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

/// HUP-S0.3b: the token fields of the SAME sealed record, for in-process request auth only.
#[derive(Deserialize)]
struct SealedToken {
    access_token: SecretString,
    #[serde(default)]
    expires_at: Option<u64>,
}

/// HUP-S0.3b: a connected service's access token, read from the vault for an in-process request
/// (the Hugging Face download path attaches it to Hugging Face origins only; see `hf_auth.rs`).
/// `None` when the service is not connected, the vault is locked (a read never forces an unlock),
/// the record is unreadable, or the provider's stated expiry has passed (an expired token is never
/// sent). I-2: crate-private and NEVER reachable from a `#[tauri::command]` return value.
pub(crate) fn sealed_access_token(
    service: Service,
    vault: &CustodyVault,
) -> Option<Zeroizing<String>> {
    let bytes = vault.custody_get(&slot(service)).ok()?;
    let rec: SealedToken = serde_json::from_slice(&bytes).ok()?;
    if rec.expires_at.is_some_and(|exp| exp <= now_unix()) || rec.access_token.expose().is_empty() {
        return None;
    }
    Some(rec.access_token.into_zeroizing())
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

/// The whole sealed record, read back in-process for refresh and revoke (L6-WP05). The same
/// JSON `StoredConnection` writes, so a record written before WP05 reads unchanged.
#[derive(Deserialize)]
struct SealedRecord {
    access_token: SecretString,
    #[serde(default)]
    refresh_token: Option<SecretString>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
    #[serde(default)]
    connected_at: Option<u64>,
}

/// How this run reaches a service's OAuth client (L6-WP05). Never holds a secret outside the
/// debug-only `DevFile` arm.
enum ClientPlan {
    /// Debug builds only: the developer's `oauth.dev.json` client id and secret, sent from the
    /// device to the provider (the pre-WP05 path, kept for development).
    #[cfg(debug_assertions)]
    DevFile(ClientCreds),
    /// A public client: the device exchanges with the provider directly, sending the PKCE
    /// verifier and no secret.
    PublicPkce { client_id: &'static str },
    /// A provider that needs a client secret: the device holds only the public client id and
    /// exchanges through the Citrate relay, which adds the secret server-side.
    Relay {
        client_id: &'static str,
        provider: &'static str,
    },
}

impl ClientPlan {
    fn client_id(&self) -> &str {
        match self {
            #[cfg(debug_assertions)]
            ClientPlan::DevFile(c) => &c.client_id,
            ClientPlan::PublicPkce { client_id } | ClientPlan::Relay { client_id, .. } => client_id,
        }
    }

    /// The form a direct refresh posts to the provider's token endpoint (`DevFile` and
    /// `PublicPkce`; a relay refresh sends only the refresh token, see `oauth_client::relay`).
    fn direct_refresh_params(&self, refresh_token: &str) -> Vec<(&'static str, String)> {
        #[cfg_attr(not(debug_assertions), allow(unused_mut))] // only the dev file adds a field
        let mut params = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.to_string()),
            ("client_id", self.client_id().to_string()),
        ];
        #[cfg(debug_assertions)]
        if let ClientPlan::DevFile(c) = self {
            params.push(("client_secret", c.client_secret.to_string()));
        }
        params
    }

    /// The mode name for a log line (never a credential).
    fn mode(&self) -> &'static str {
        match self {
            #[cfg(debug_assertions)]
            ClientPlan::DevFile(_) => "dev-file",
            ClientPlan::PublicPkce { .. } => "public-pkce",
            ClientPlan::Relay { .. } => "relay",
        }
    }
}

/// A log sink for the flow's lifecycle lines. Lines name the service, the mode and the outcome
/// only; the production sink also runs them through `secret_shapes::redact`.
type LogSink = Box<dyn Fn(&str) + Send + Sync>;

fn stderr_log() -> LogSink {
    Box::new(|line: &str| eprintln!("{}", crate::secret_shapes::redact(line)))
}

/// Process-wide MCP-connection manager: the HTTP client (ureq in prod, a test seam
/// in tests), the credential-file path (debug builds only), and whether the provider
/// registry is on (`CONNECTIONS_V2`).
pub struct ConnectionManager {
    http: Box<dyn HttpClient>,
    #[cfg(debug_assertions)]
    creds_path: PathBuf,
    /// L6-WP05: `CONNECTIONS_V2`. Off: only the dev file (debug) can configure a service, and
    /// a release build reports `NotConfigured`. On: the compiled provider registry applies.
    v2: bool,
    log: LogSink,
    /// Serializes refreshes so a rotated refresh token is never sent twice.
    refresh_lock: std::sync::Mutex<()>,
}

impl ConnectionManager {
    /// The development manager: reads client credentials from the dev file at `creds_path`.
    /// `CONNECTIONS_V2` starts off; see [`Self::with_v2`].
    #[cfg(debug_assertions)]
    pub fn new(http: Box<dyn HttpClient>, creds_path: PathBuf) -> Self {
        ConnectionManager {
            http,
            creds_path,
            v2: false,
            log: stderr_log(),
            refresh_lock: std::sync::Mutex::new(()),
        }
    }

    /// The release manager: no credential file, no client secret.
    #[cfg(not(debug_assertions))]
    fn release(http: Box<dyn HttpClient>) -> Self {
        ConnectionManager {
            http,
            v2: false,
            log: stderr_log(),
            refresh_lock: std::sync::Mutex::new(()),
        }
    }

    /// Turn the provider registry (`CONNECTIONS_V2`) on or off.
    pub(crate) fn with_v2(mut self, on: bool) -> Self {
        self.v2 = on;
        self
    }

    /// Replace the log sink (tests capture the lines).
    #[cfg(test)]
    fn with_log(mut self, log: LogSink) -> Self {
        self.log = log;
        self
    }

    fn log(&self, line: &str) {
        (self.log)(line);
    }

    /// Load a service's client credentials from the dev file. Missing file or key →
    /// `NotConfigured` (honest: the flow cannot start without real creds). Debug builds only.
    #[cfg(debug_assertions)]
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

    /// How this run reaches `service`'s OAuth client. Debug builds prefer the developer's dev
    /// file; otherwise (and always in a release build) the compiled registry decides, and only
    /// while `CONNECTIONS_V2` is on. `NotConfigured` otherwise.
    fn plan(&self, service: Service) -> std::result::Result<ClientPlan, ConnError> {
        #[cfg(debug_assertions)]
        if let Ok(creds) = self.creds(service) {
            return Ok(ClientPlan::DevFile(creds));
        }
        if !self.v2 {
            return Err(ConnError::NotConfigured);
        }
        let setup = oauth_client::setup_for(service);
        match (&setup.mode, setup.client_id, setup.configured) {
            (oauth_client::ClientMode::Relay, Some(client_id), true) => Ok(ClientPlan::Relay {
                client_id,
                provider: setup.provider,
            }),
            (oauth_client::ClientMode::PublicPkce, Some(client_id), true) => {
                Ok(ClientPlan::PublicPkce { client_id })
            }
            _ => Err(ConnError::NotConfigured),
        }
    }

    /// HUP-S10.2: whether this service's OAuth client is configured (without it the flow cannot
    /// start, and the UI keeps Connect disabled with that reason). L6-WP05: a relay-mode service
    /// counts as configured when its public client id is compiled in and `CONNECTIONS_V2` is on.
    pub(crate) fn is_configured(&self, service: Service) -> bool {
        self.plan(service).is_ok()
    }

    /// Run the full connect flow with no relay transport (a relay-mode service then reports
    /// `NotSignedIn`). See [`Self::connect_with`]; the command passes the member's session.
    #[cfg(test)]
    pub fn connect(
        &self,
        service: Service,
        vault: &CustodyVault,
        open: impl Fn(&str) -> std::result::Result<(), ConnError>,
    ) -> std::result::Result<ConnectionStatus, ConnError> {
        self.connect_with(service, vault, None, open)
    }

    /// Run the full connect flow: bind the fixed loopback, mint PKCE + state, open
    /// the authorize URL in the system browser (via `open`), await one callback,
    /// verify `state` (constant-time), then exchange + seal the token. Blocking
    /// (the caller runs it off the main thread). A relay-mode service exchanges through
    /// `relay` (the member's Citrate session); with none it fails before any browser opens.
    pub(crate) fn connect_with(
        &self,
        service: Service,
        vault: &CustodyVault,
        relay: Option<&dyn oauth_client::relay::RelayTransport>,
        open: impl Fn(&str) -> std::result::Result<(), ConnError>,
    ) -> std::result::Result<ConnectionStatus, ConnError> {
        // Fail fast before opening a browser / binding a socket if unconfigured.
        let plan = match self.plan(service) {
            Ok(plan) => plan,
            Err(e) => {
                let reason = if self.v2 {
                    oauth_client::provider_setup(service.id())
                        .and_then(|setup| setup.reason)
                        .unwrap_or("no client registered")
                } else {
                    "CONNECTIONS_V2 is off in this build"
                };
                self.log(&format!(
                    "connections: {} not configured: {reason}",
                    service.id()
                ));
                return Err(e);
            }
        };
        if matches!(plan, ClientPlan::Relay { .. }) && relay.is_none() {
            return Err(ConnError::NotSignedIn);
        }
        let listener = ConnectionListener::bind()?;
        let state = random_state();
        let pkce = Pkce::new();
        let auth_url = authorize_url(service, plan.client_id(), &state, &pkce.challenge);
        open(&auth_url)?;
        let cb = match listener.wait_for_callback(CALLBACK_TIMEOUT, &state) {
            Ok(cb) => cb,
            Err(ConnError::Declined) => {
                self.log(&format!(
                    "connections: {} sign-in declined at the provider; nothing stored",
                    service.id()
                ));
                return Err(ConnError::Declined);
            }
            Err(e) => return Err(e),
        };
        // Defence in depth: the listener only returns a state-matching callback.
        if !ct_eq(cb.state.as_bytes(), state.as_bytes()) {
            return Err(ConnError::StateMismatch);
        }
        self.exchange_with_plan(service, &plan, vault, &cb.code, &pkce.verifier, relay)
    }

    /// Exchange the code the way `plan` says and seal the token.
    fn exchange_with_plan(
        &self,
        service: Service,
        plan: &ClientPlan,
        vault: &CustodyVault,
        code: &str,
        verifier: &str,
        relay: Option<&dyn oauth_client::relay::RelayTransport>,
    ) -> std::result::Result<ConnectionStatus, ConnError> {
        let out = match plan {
            #[cfg(debug_assertions)]
            ClientPlan::DevFile(creds) => {
                self.exchange_and_store(service, creds, vault, code, verifier)
            }
            ClientPlan::PublicPkce { client_id } => {
                // No secret: `token_exchange_params` drops the empty value.
                let params = token_exchange_params(service, client_id, "", code, verifier);
                self.post_provider_form(service.token_endpoint(), &params)
                    .and_then(|body| self.seal_token_response(service, &body, vault, None))
            }
            ClientPlan::Relay { provider, .. } => {
                let relay = relay.ok_or(ConnError::NotSignedIn)?;
                let req = oauth_client::relay::token_body(code, verifier, service.redirect_uri())?;
                relay
                    .post(provider, oauth_client::relay::RelayOp::Token, &req)
                    .map_err(|e| e.into_conn(ConnError::TokenExchange))
                    .and_then(|body| self.seal_token_response(service, &body, vault, None))
            }
        };
        match &out {
            Ok(_) => self.log(&format!(
                "connections: {} connected via {}; token sealed in {}",
                service.id(),
                plan.mode(),
                slot(service)
            )),
            Err(e) => self.log(&format!(
                "connections: {} exchange via {} failed: {e}",
                service.id(),
                plan.mode()
            )),
        }
        out
    }

    /// Exchange the authorization `code` (with the PKCE verifier) and seal the
    /// resulting token in the vault, with the dev-file client secret (debug builds only).
    /// Factored out so it is unit-tested over an injected HTTP seam + a headless vault,
    /// without a live provider or socket.
    #[cfg(debug_assertions)]
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
        let body = self.post_provider_form(service.token_endpoint(), &params)?;
        self.seal_token_response(service, &body, vault, None)
    }

    /// POST a form to a provider endpoint; the body (it holds tokens) comes back in a wiping
    /// buffer.
    fn post_provider_form(
        &self,
        url: &str,
        params: &[(&'static str, String)],
    ) -> std::result::Result<Zeroizing<String>, ConnError> {
        let form: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.http
            .post_form(url, &form)
            .map(Zeroizing::new)
            .map_err(|_| ConnError::TokenExchange)
    }

    /// Parse a provider token response and seal it as the service's record, in one vault put.
    /// `prior` (a refresh) keeps the first `connected_at`, and the old refresh token and scope
    /// when the provider did not send new ones. No length or shape is assumed of any token.
    fn seal_token_response(
        &self,
        service: Service,
        body: &str,
        vault: &CustodyVault,
        prior: Option<&SealedRecord>,
    ) -> std::result::Result<ConnectionStatus, ConnError> {
        let tok: TokenResponse =
            serde_json::from_str(body).map_err(|_| ConnError::TokenExchange)?;
        if tok.access_token.expose().is_empty() {
            return Err(ConnError::TokenExchange);
        }
        let now = now_unix();
        let connected_at = prior.and_then(|p| p.connected_at).unwrap_or(now);
        let expires_at = tok.expires_in.map(|e| now.saturating_add(e));
        let scope = tok
            .scope
            .clone()
            .or_else(|| prior.and_then(|p| p.scope.clone()));
        let record = StoredConnection {
            access_token: tok.access_token.expose(),
            refresh_token: tok
                .refresh_token
                .as_ref()
                .or_else(|| prior.and_then(|p| p.refresh_token.as_ref()))
                .map(SecretString::expose),
            token_type: tok
                .token_type
                .as_deref()
                .or_else(|| prior.and_then(|p| p.token_type.as_deref())),
            scope: scope.as_deref(),
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
            scope,
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

    /// Forget a service's token with no relay transport (a relay-mode revoke is then recorded
    /// as failed). See [`Self::disconnect_with`]; the command passes the member's session.
    #[cfg(test)]
    pub fn disconnect(
        &self,
        service: Service,
        vault: &CustodyVault,
    ) -> std::result::Result<(), ConnError> {
        self.disconnect_with(service, vault, None).map(|_| ())
    }
}

/// Managed Tauri state: the process-wide connection manager.
pub struct ConnectionState(pub ConnectionManager);

/// Build the connection state for a Tauri build: production ureq client, the dev
/// credential path in a debug build (none in a release build), and `CONNECTIONS_V2`.
pub fn build_connection_state() -> ConnectionState {
    #[cfg(debug_assertions)]
    let mgr = ConnectionManager::new(Box::new(crate::oidc::UreqClient), dev_credentials_path());
    #[cfg(not(debug_assertions))]
    let mgr = ConnectionManager::release(Box::new(crate::oidc::UreqClient));
    ConnectionState(mgr.with_v2(oauth_client::connections_v2_enabled()))
}

// ---------------------------------------------------------------------------
// Tauri command surface (I-2: none returns a token or secret)
// ---------------------------------------------------------------------------

/// `connection_start` — run the loopback-PKCE flow for one MCP service, opening the
/// provider's authorize page in the SYSTEM browser (third-party OAuth forbids
/// embedded webviews). ASYNC so Tauri runs it off the main thread while the
/// loopback blocks. Returns a claim-free `ConnectionStatus`; the token is sealed in
/// the vault, never returned. L6-WP05: a relay-mode service exchanges through the
/// Citrate relay with the member's Citrate session (`oauth_client::relay`).
#[tauri::command]
pub async fn connection_start(
    app: tauri::AppHandle,
    state: State<'_, ConnectionState>,
    custody: State<'_, crate::custody::CustodyState>,
    auth: State<'_, crate::oidc::AuthState>,
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
    let relay = oauth_client::relay::AuthorityRelay::new(&auth.0, &custody.0);
    state
        .0
        .connect_with(svc, &custody.0, Some(&relay), open)
        .map_err(|e| e.to_string())
}

/// `connection_status` — the connect/disconnect state of all three services. No
/// token crosses the boundary.
#[tauri::command]
pub async fn connection_status(
    app_h: tauri::AppHandle,
) -> std::result::Result<Vec<ConnectionStatus>, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<ConnectionState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st1 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        connection_status_sync(st0, st1)
    })
    .await
}

/// Blocking body of [`connection_status`]; reached only through [`crate::blocking::off_main`].
pub fn connection_status_sync(
    state: State<'_, ConnectionState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<Vec<ConnectionStatus>, String> {
    Ok(state.0.status(&custody.0))
}

/// `connection_disconnect` — revoke at the provider where that is supported, then forget the
/// sealed token. L6-WP05: the slot is cleared on the member's request even when the revoke
/// fails; the command then returns an error that says so.
#[tauri::command]
pub async fn connection_disconnect(
    app_h: tauri::AppHandle,
    service: String,
) -> std::result::Result<(), String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<ConnectionState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st1 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st2 = tauri::Manager::try_state::<crate::oidc::AuthState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        connection_disconnect_sync(st0, st1, st2, service)
    })
    .await
}

/// Blocking body of [`connection_disconnect`]; reached only through [`crate::blocking::off_main`].
pub fn connection_disconnect_sync(
    state: State<'_, ConnectionState>,
    custody: State<'_, crate::custody::CustodyState>,
    auth: State<'_, crate::oidc::AuthState>,
    service: String,
) -> std::result::Result<(), String> {
    let relay = oauth_client::relay::AuthorityRelay::new(&auth.0, &custody.0);
    connection_disconnect_with(&state.0, &custody.0, Some(&relay), service)
}

/// The disconnect body: revoke, record, clear, and report a failed revoke honestly.
fn connection_disconnect_with(
    mgr: &ConnectionManager,
    vault: &CustodyVault,
    relay: Option<&dyn oauth_client::relay::RelayTransport>,
    service: String,
) -> std::result::Result<(), String> {
    let svc = Service::from_id(&service).ok_or_else(|| ConnError::UnknownService.to_string())?;
    let outcome = mgr
        .disconnect_with(svc, vault, relay)
        .map_err(|e| e.to_string())?;
    match outcome.member_message(svc) {
        Some(msg) => Err(msg),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_ids_round_trip() {
        for s in [
            Service::GitHub,
            Service::GoogleDrive,
            Service::Notion,
            Service::HuggingFace,
        ] {
            assert_eq!(Service::from_id(s.id()), Some(s));
        }
        assert_eq!(Service::from_id("slack"), None);
    }

    #[test]
    fn huggingface_oauth_shape() {
        // Commons CX-S1: HF is a standard OAuth2+PKCE provider with a loopback redirect.
        assert_eq!(Service::HuggingFace.id(), "hf");
        assert_eq!(
            Service::HuggingFace.authorize_endpoint(),
            "https://huggingface.co/oauth/authorize"
        );
        assert_eq!(
            Service::HuggingFace.token_endpoint(),
            "https://huggingface.co/oauth/token"
        );
        // Accepts the http loopback redirect (native app), like GitHub/Google (not the Notion bounce).
        assert_eq!(Service::HuggingFace.redirect_uri(), OAUTH_REDIRECT_URI);
        assert_eq!(Service::HuggingFace.default_scopes(), &["read-repos"]);
        assert_eq!(env_prefix(Service::HuggingFace), "HF");
        // authorize URL carries the standard params + the read-repos scope, no provider extras.
        let url = authorize_url(Service::HuggingFace, "hf_client", "st", "ch");
        assert!(url.starts_with("https://huggingface.co/oauth/authorize?"));
        assert!(url.contains("client_id=hf_client"));
        assert!(url.contains("scope=read-repos"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8975%2Foauth%2Fcallback"));
    }

    #[test]
    fn pkce_is_s256_and_verifier_differs_from_challenge() {
        let p = Pkce::new();
        assert_ne!(&*p.verifier as &str, p.challenge);
        // challenge = base64url(sha256(verifier))
        let expect = b64url(&Sha256::digest(p.verifier.as_bytes()));
        assert_eq!(p.challenge, expect);
        // base64url no-pad → no '=', '+', '/'
        assert!(
            !p.challenge.contains('=') && !p.challenge.contains('+') && !p.challenge.contains('/')
        );
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
        assert!(
            !url.contains("scope="),
            "Notion sets capabilities on the integration, not a scope param"
        );
    }

    #[test]
    fn redirect_uri_is_loopback_for_github_and_google_but_hosted_for_notion() {
        // GitHub + Google accept the plaintext-http loopback redirect directly.
        assert_eq!(Service::GitHub.redirect_uri(), OAUTH_REDIRECT_URI);
        assert_eq!(Service::GoogleDrive.redirect_uri(), OAUTH_REDIRECT_URI);
        assert_eq!(
            Service::GitHub.redirect_uri(),
            "http://127.0.0.1:8975/oauth/callback"
        );
        // Notion rejects the http loopback → the hosted https bounce is registered.
        assert_eq!(Service::Notion.redirect_uri(), HOSTED_REDIRECT_URI);
        assert_eq!(
            Service::Notion.redirect_uri(),
            "https://auth.citrate.ai/oauth/callback"
        );
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
        assert_eq!(
            get(&notion, "redirect_uri").as_deref(),
            Some(HOSTED_REDIRECT_URI)
        );
        let gh = token_exchange_params(Service::GitHub, "id", "sec", "c", "v");
        assert_eq!(
            get(&gh, "redirect_uri").as_deref(),
            Some(OAUTH_REDIRECT_URI)
        );
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
            *self.0.last_form.lock().unwrap() = form
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            Ok(self.0.response.clone())
        }
    }

    #[test]
    fn parse_dev_credentials_skips_comments_blanks_and_trims() {
        let raw = "# a comment\n\nGITHUB_CLIENT_ID = abc \n  \nNOTION_CLIENT_SECRET=sek\n";
        let m = parse_dev_credentials(raw);
        assert_eq!(m.get("GITHUB_CLIENT_ID").map(String::as_str), Some("abc"));
        assert_eq!(
            m.get("NOTION_CLIENT_SECRET").map(String::as_str),
            Some("sek")
        );
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
        let mgr = ConnectionManager::new(
            Box::new(SharedHttp(Arc::new(FakeHttp {
                last_url: StdMutex::new(None),
                last_form: StdMutex::new(Vec::new()),
                response: String::new(),
            }))),
            p.clone(),
        );
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

    /// The provider's token response never prints a token in `Debug`, and a response the exchange
    /// cannot use yields an error that carries no part of the body.
    #[test]
    fn token_response_debug_and_errors_carry_no_token() {
        let tok: TokenResponse = serde_json::from_str(
            r#"{"access_token":"FIXTURE-ACCESS-VALUE","refresh_token":"FIXTURE-REFRESH-VALUE","scope":"repo"}"#,
        )
        .unwrap();
        let dbg = format!("{tok:?}");
        assert!(
            !dbg.contains("FIXTURE-ACCESS-VALUE"),
            "access token in Debug: {dbg}"
        );
        assert!(
            !dbg.contains("FIXTURE-REFRESH-VALUE"),
            "refresh token in Debug: {dbg}"
        );
        assert!(
            dbg.contains("repo"),
            "non-secret fields stay visible: {dbg}"
        );

        let vault = fresh_vault();
        let fake = Arc::new(FakeHttp {
            last_url: StdMutex::new(None),
            last_form: StdMutex::new(Vec::new()),
            response: r#"{"access_token":42,"leak":"FIXTURE-ACCESS-VALUE"}"#.to_string(),
        });
        let mgr = ConnectionManager::new(Box::new(SharedHttp(fake)), PathBuf::from("unused"));
        let creds = ClientCreds {
            client_id: "id".to_string(),
            client_secret: Zeroizing::new("cs".to_string()),
        };
        let err = mgr
            .exchange_and_store(Service::GitHub, &creds, &vault, "c", "v")
            .expect_err("a non-string token is refused");
        assert!(
            !err.to_string().contains("FIXTURE-ACCESS"),
            "token in error: {err}"
        );
        assert!(
            !format!("{err:?}").contains("FIXTURE-ACCESS"),
            "token in error Debug: {err:?}"
        );
    }

    #[test]
    fn slot_is_namespaced_and_backend_reserved() {
        assert_eq!(slot(Service::Notion), "connection-notion");
        // Only the backend token exchange writes it; the webview's custody_put refuses it.
        assert!(crate::custody::is_backend_reserved_slot(&slot(
            Service::Notion
        )));
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
        let cb = listener
            .wait_for_callback(Duration::from_secs(5), "xyz789")
            .unwrap();
        h.join().unwrap();
        assert_eq!(cb.code, "abc123");
        assert_eq!(cb.state, "xyz789");
    }

    /// PBA-L7b-009: a drive-by request from any web page (`<img src=http://127.0.0.1:8975/x>`)
    /// or a forged callback with the wrong `state` must NOT burn the sign-in; the listener
    /// answers 400 and keeps waiting for the real, state-matching callback.
    #[test]
    fn pba_l7b_009_drive_by_hits_do_not_burn_the_sign_in() {
        let listener = ConnectionListener::bind_on(0).unwrap();
        let port = listener.port();
        let h = std::thread::spawn(move || {
            use std::io::Read;
            let mut statuses = Vec::new();
            for req in [
                &b"GET /x HTTP/1.1\r\nHost: x\r\n\r\n"[..],
                &b"GET /oauth/callback?code=attacker&state=guess HTTP/1.1\r\nHost: x\r\n\r\n"[..],
                &b"GET /oauth/callback?code=real-code&state=the-flow-state HTTP/1.1\r\nHost: x\r\n\r\n"[..],
            ] {
                let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
                s.write_all(req).unwrap();
                let mut resp = String::new();
                let _ = s.read_to_string(&mut resp);
                statuses.push(resp.lines().next().unwrap_or("").to_string());
            }
            statuses
        });
        let cb = listener
            .wait_for_callback(Duration::from_secs(10), "the-flow-state")
            .expect("the real callback still completes the sign-in");
        let statuses = h.join().unwrap();
        assert_eq!(
            cb.code, "real-code",
            "the attacker's code is never accepted"
        );
        assert!(statuses[0].contains("400"), "{statuses:?}");
        assert!(statuses[1].contains("400"), "{statuses:?}");
        assert!(statuses[2].contains("200"), "{statuses:?}");
    }

    #[test]
    fn listener_times_out_when_no_callback_arrives() {
        let listener = ConnectionListener::bind_on(0).unwrap();
        assert_eq!(
            listener
                .wait_for_callback(Duration::from_millis(80), "st")
                .unwrap_err(),
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
        let mgr =
            ConnectionManager::new(Box::new(SharedHttp(fake.clone())), PathBuf::from("unused"));
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

        mgr.exchange_and_store(Service::GitHub, &creds, &vault, "c", "v")
            .unwrap();
        let after = mgr.status(&vault);
        let gh = after.iter().find(|s| s.service == "github").unwrap();
        assert!(gh.connected);
        assert_eq!(gh.scope.as_deref(), Some("repo"));
        // The other two remain disconnected.
        assert!(after
            .iter()
            .filter(|s| s.service != "github")
            .all(|s| !s.connected));

        // Disconnect forgets it.
        mgr.disconnect(Service::GitHub, &vault).unwrap();
        let gone = mgr.status(&vault);
        assert!(
            !gone
                .iter()
                .find(|s| s.service == "github")
                .unwrap()
                .connected
        );
    }

    // HUP-S0.3b: the download path reads the sealed Hugging Face token IN-PROCESS.

    fn hf_mgr(response: &str) -> ConnectionManager {
        let fake = Arc::new(FakeHttp {
            last_url: StdMutex::new(None),
            last_form: StdMutex::new(Vec::new()),
            response: response.to_string(),
        });
        ConnectionManager::new(Box::new(SharedHttp(fake)), PathBuf::from("unused"))
    }

    fn hf_creds() -> ClientCreds {
        ClientCreds {
            client_id: "hf_id".to_string(),
            client_secret: Zeroizing::new("hf_secret".to_string()),
        }
    }

    #[test]
    fn hup_s0_3b_sealed_access_token_reads_the_connected_hf_token() {
        let vault = fresh_vault();
        hf_mgr(r#"{"access_token":"hf_oauth_tok","scope":"read-repos","expires_in":3600}"#)
            .exchange_and_store(Service::HuggingFace, &hf_creds(), &vault, "c", "v")
            .unwrap();
        let tok = sealed_access_token(Service::HuggingFace, &vault).expect("token");
        assert_eq!(tok.as_str(), "hf_oauth_tok");
        // Another service's slot is not read for HF.
        assert!(sealed_access_token(Service::GitHub, &vault).is_none());
    }

    #[test]
    fn hup_s0_3b_sealed_access_token_is_none_when_absent_locked_or_expired() {
        let vault = fresh_vault();
        assert!(sealed_access_token(Service::HuggingFace, &vault).is_none());

        // expires_in 0 -> expires_at == now -> already expired: never sent.
        hf_mgr(r#"{"access_token":"hf_stale","expires_in":0}"#)
            .exchange_and_store(Service::HuggingFace, &hf_creds(), &vault, "c", "v")
            .unwrap();
        assert!(sealed_access_token(Service::HuggingFace, &vault).is_none());

        // A token with no expiry stays usable; a locked vault reads as absent (no forced unlock).
        hf_mgr(r#"{"access_token":"hf_no_expiry"}"#)
            .exchange_and_store(Service::HuggingFace, &hf_creds(), &vault, "c", "v")
            .unwrap();
        assert!(sealed_access_token(Service::HuggingFace, &vault).is_some());
        vault.lock();
        assert!(sealed_access_token(Service::HuggingFace, &vault).is_none());
    }

    #[test]
    fn hup_s0_3b_sealed_access_token_is_none_for_an_empty_token() {
        let vault = fresh_vault();
        let rec = serde_json::json!({ "access_token": "", "expires_at": null });
        let mut bytes = serde_json::to_vec(&rec).unwrap();
        vault.put(&slot(Service::HuggingFace), &mut bytes).unwrap();
        assert!(sealed_access_token(Service::HuggingFace, &vault).is_none());
    }

    // 0.6.0 L6-WP02: the lane's characterization suite (test-only; see the file).
    include!("lanes/l6/characterization/connections_char_tests.rs");
}
