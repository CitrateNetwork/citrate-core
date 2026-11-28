//! 0.6.0 L6-WP06: MCP remote-server authorization (planset 2026-11-07-v060, lanes/L6 WP06 and
//! section 3.2; the MCP authorization specification 2025-11-25).
//!
//! A brand remote MCP server (Linear, Notion, GitHub, ...) answers an unauthenticated request with
//! `401` and a `WWW-Authenticate: Bearer resource_metadata=".."` challenge. This module:
//!
//! - [`discovery`]: reads the challenge, fetches the protected resource metadata (RFC 9728) and
//!   the authorization server metadata (RFC 8414 or OpenID discovery, path-inserted forms
//!   included), through a hardened fetch ([`http`]: https only, no redirects, public addresses
//!   only, 64 KiB, 10 s);
//! - [`registration`]: picks the client: the Citrate client metadata document (CIMD) when the
//!   server advertises it, else dynamic registration (RFC 7591, sealed once per issuer and
//!   reused), else a fixed pre-registered client whose secret lives in the Citrate relay;
//! - [`flow`]: runs authorization code + PKCE S256 with `state`, the exact loopback redirect, the
//!   RFC 8707 `resource` parameter and the RFC 9207 `iss` check, and seals a token bound to one
//!   resource ([`store`]);
//! - [`token_source`]: [`token_source::ConnectorTokenSource`], the only way L1's MCP gateway gets
//!   a bearer: bound to the resource, refreshed single-flight with rotation, revoked on request;
//! - [`policy`]: write tools are off by default for every remote server, whatever the token's
//!   scope; a member turns writes on per server;
//! - [`migration`]: the GitHub part of L6-WP13 (the 0.5.x `connection-github` token stays until
//!   the GitHub MCP server connection succeeds).
//!
//! Data sources (Rule 7): the remote server's `WWW-Authenticate` header and
//! `/.well-known/oauth-protected-resource`; the authorization server's metadata; the custody vault
//! slots `connection-mcp-<connectorId>` and `connection-dcr-<issuer hash>`; the relay of lanes/L6
//! section 3.1 for fixed clients. Provider quirks are data (the L6-WP07 provider matrix,
//! `evidence/L6-WP07_MCP_PROVIDER_MATRIX_2026-11-28.md`), never code branches per provider.
//!
//! Nothing here logs or returns a token, code, verifier, client secret or response body.
//!
//! Claim status: implemented and unit-tested over an injected HTTP seam. Not runtime-proven: a
//! real-provider connect needs QA accounts and L1's gateway calling `bearer_for` (lanes/L6 WP06
//! acceptance). Until then nothing in the app calls into this module, hence the dead-code allow.
#![allow(dead_code)]

pub(crate) mod discovery;
pub(crate) mod http;

#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod http_tests;

// @flag MCP_REMOTE_AUTH
/// The MCP remote authorization flag (lanes/L6 WP06). Off until a release build has a recorded
/// connect through L1's gateway (runtime-proven). Off: `connect` refuses with `Disabled` before
/// any network call. Row for `release/flags.json` when L0-WP09 lands it: lane L6, default false,
/// sign-off none.
pub(crate) const MCP_REMOTE_AUTH: bool = false;

/// Whether MCP remote authorization is on for this run. A debug build can turn it on with
/// `CITRATE_MCP_REMOTE_AUTH=1`; a release build reads only the compiled flag.
pub(crate) fn mcp_remote_auth_enabled() -> bool {
    #[cfg(debug_assertions)]
    if std::env::var("CITRATE_MCP_REMOTE_AUTH").is_ok_and(|v| v == "1") {
        return true;
    }
    MCP_REMOTE_AUTH
}

/// The Citrate Core client id for CIMD: the URL of its client metadata document (lanes/L6
/// section 3.1, frozen; the document is published by L6-WP03 in citrate-landing).
pub(crate) const CIMD_CLIENT_ID: &str = "https://citrate.ai/oauth/client/citrate-core.json";

/// The redirect a CIMD or DCR client uses: exactly the loopback in the CIMD document.
pub(crate) const LOOPBACK_REDIRECT: &str = super::OAUTH_REDIRECT_URI;

/// Why an MCP authorization step failed. Never carries a token, code, secret or body; a token
/// endpoint rejection carries only the OAuth `error` code, filtered to `[a-z0-9_]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum McpAuthError {
    /// `MCP_REMOTE_AUTH` is off in this build.
    Disabled,
    /// Not an https URL, or it carries user info or a fragment.
    InsecureUrl,
    /// The host is, or resolves to, a loopback, private, link-local or other non-public address.
    PrivateAddress,
    /// A metadata document was over the size cap.
    TooLarge,
    Timeout,
    Network,
    /// No protected resource metadata could be found for the server.
    NoResourceMetadata,
    /// A metadata document was not the JSON it should be; names the field at fault.
    BadMetadata(&'static str),
    /// The metadata's `resource` does not cover the server URL, or a token was asked for another
    /// resource.
    ResourceMismatch,
    /// The authorization server metadata names another issuer.
    IssuerMismatch,
    /// The authorization server does not advertise PKCE S256: the spec says refuse.
    PkceS256Unsupported,
    /// No CIMD, no registration endpoint and no fixed client for this issuer.
    NoRegistrationPath,
    /// A fixed client row exists but its client id is not compiled in; names the registration.
    NotConfigured(&'static str),
    /// Dynamic registration was refused (HTTP status).
    RegistrationRejected(u16),
    /// The callback `state` did not match.
    StateMismatch,
    /// The callback `iss` is missing where required, or names another issuer (RFC 9207).
    IssMismatch,
    /// The member declined consent.
    Declined,
    /// The authorization or token endpoint answered with this OAuth error code.
    TokenRejected(String),
    NotSignedIn,
    RateLimited,
    PortInUse,
    Vault,
    NotConnected,
    ReauthRequired,
}

impl std::fmt::Display for McpAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpAuthError::Disabled => f.write_str("remote MCP sign-in is off in this build"),
            McpAuthError::InsecureUrl => f.write_str("the server URL is not a plain https URL"),
            McpAuthError::PrivateAddress => {
                f.write_str("the server is on a local or private address; it is never contacted")
            }
            McpAuthError::TooLarge => f.write_str("a metadata document was too large"),
            McpAuthError::Timeout => f.write_str("the server did not answer in time"),
            McpAuthError::Network => f.write_str("the server could not be reached"),
            McpAuthError::NoResourceMetadata => {
                f.write_str("the server publishes no OAuth protected resource metadata")
            }
            McpAuthError::BadMetadata(field) => {
                write!(f, "the server's metadata is invalid ({field})")
            }
            McpAuthError::ResourceMismatch => {
                f.write_str("the token is for another server, or the metadata names another server")
            }
            McpAuthError::IssuerMismatch => {
                f.write_str("the authorization server metadata names another issuer")
            }
            McpAuthError::PkceS256Unsupported => {
                f.write_str("the authorization server does not support PKCE S256; refused")
            }
            McpAuthError::NoRegistrationPath => {
                f.write_str("Citrate Core has no way to register with this server yet")
            }
            McpAuthError::NotConfigured(why) => write!(f, "not configured: {why}"),
            McpAuthError::RegistrationRejected(s) => {
                write!(f, "the server refused client registration (HTTP {s})")
            }
            McpAuthError::StateMismatch => f.write_str("callback state mismatch (sign-in aborted)"),
            McpAuthError::IssMismatch => {
                f.write_str("the callback came from another authorization server (sign-in aborted)")
            }
            McpAuthError::Declined => {
                f.write_str("the sign-in was declined; nothing was connected")
            }
            McpAuthError::TokenRejected(code) => {
                write!(f, "the server refused the sign-in ({code})")
            }
            McpAuthError::NotSignedIn => f.write_str("Sign in to Citrate to connect"),
            McpAuthError::RateLimited => {
                f.write_str("the Citrate relay is busy; try again in a minute")
            }
            McpAuthError::PortInUse => {
                f.write_str("the sign-in callback port (127.0.0.1:8975) is already in use")
            }
            McpAuthError::Vault => f.write_str("the secure vault is locked or unavailable"),
            McpAuthError::NotConnected => f.write_str("not connected"),
            McpAuthError::ReauthRequired => f.write_str("the connection expired; connect it again"),
        }
    }
}

/// An OAuth `error` code from a server, kept only if it is a plain code (RFC 6749 section 5.2
/// codes are `[a-z_]`); anything else becomes `invalid_response`, so no echoed input survives.
pub(crate) fn oauth_error_code(body: &str) -> String {
    #[derive(serde::Deserialize)]
    struct E {
        error: Option<String>,
    }
    let code = serde_json::from_str::<E>(body)
        .ok()
        .and_then(|e| e.error)
        .unwrap_or_default();
    sanitize_code(&code)
}

/// Keep `code` only when it is 1..=64 chars of `[a-z0-9_]`.
pub(crate) fn sanitize_code(code: &str) -> String {
    let ok = !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        code.to_string()
    } else {
        "invalid_response".to_string()
    }
}
