//! W4 — MCP connections OAuth core (ADR-3).
//!
//! The provider-agnostic pieces of the authorization-code + PKCE(S256) flow for the
//! three MCP services (GitHub, Google Drive, Notion): the service catalog
//! (endpoints + scopes), PKCE/`state` minting, the authorize-URL builder, and the
//! token-exchange request builder — all PURE + unit-tested here. The loopback
//! listener (RFC 8252, fixed port), the live HTTP exchange, and the keyring token
//! custody build on top of this (subsequent WPs), mirroring `oidc.rs`.
//!
//! Redirect is a FIXED loopback (not oidc.rs's random `:0`) because GitHub + Notion
//! match `redirect_uri` exactly — one registered URL across all three (see the
//! runbook + ADR-3). NO `#[tauri::command]` here returns a token or the client
//! secret (I-2 barrier); those live sealed in the OS keyring.

use base64::Engine as _;
use rand::RngCore as _;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

/// The one callback URL registered for every service (ADR-3 / runbook).
pub const OAUTH_REDIRECT_URI: &str = "http://127.0.0.1:8975/oauth/callback";

/// The fixed loopback port the single-use callback listener binds.
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
        ("redirect_uri".into(), OAUTH_REDIRECT_URI.into()),
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
        ("redirect_uri", OAUTH_REDIRECT_URI.to_string()),
        ("client_id", client_id.to_string()),
        ("client_secret", client_secret.to_string()),
        ("code_verifier", verifier.to_string()),
    ];
    // GitHub's token endpoint ignores grant_type; harmless to send. Keep the body
    // uniform across providers.
    let _ = service;
    params.retain(|(_, v)| !v.is_empty());
    params
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
}
