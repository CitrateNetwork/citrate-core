//! The Citrate token relay client (L6-WP05; relay built by L6-WP04 in citrate-identity,
//! `src/oauth-relay/routes.ts`, `docs/OAUTH_RELAY.md`).
//!
//! `POST https://auth.citrate.ai/oauth/relay/v1/{provider}/{token|refresh|revoke}`, JSON body,
//! `Authorization: Bearer <the member's auth.citrate.ai access token>`. The relay adds the
//! client secret server-side and returns the provider's token answer unchanged (`token`,
//! `refresh`), or `{"revoked":true,"upstream_status":N}` (`revoke`). The bearer is the member's
//! Citrate session, sent by `AuthManager::post_json_to_authority` to the pinned issuer only: this
//! module never sees it.

use crate::connections::ConnError;
use crate::custody::CustodyVault;
use crate::oidc::{AuthError, AuthManager};
use serde::Serialize;
use zeroize::Zeroizing;

/// The relay's path prefix on the authority (lanes/L6 section 3.1, frozen).
pub(crate) const RELAY_PATH_PREFIX: &str = "/oauth/relay/v1";

/// One relay operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelayOp {
    Token,
    Refresh,
    Revoke,
}

impl RelayOp {
    fn segment(self) -> &'static str {
        match self {
            RelayOp::Token => "token",
            RelayOp::Refresh => "refresh",
            RelayOp::Revoke => "revoke",
        }
    }
}

/// The path of one relay operation for one provider, on the authority.
pub(crate) fn relay_path(provider: &str, op: RelayOp) -> String {
    format!("{RELAY_PATH_PREFIX}/{provider}/{}", op.segment())
}

/// Why a relay call failed. Carries a status code at most, never a body (a relay error body can
/// echo request fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelayError {
    /// No Citrate session, or the relay answered 401 `not_signed_in` after one refresh.
    NotSignedIn,
    /// 429 `rate_limited`.
    RateLimited,
    /// 503 `not_configured` / `relay_disabled`: the relay holds no secret for this provider.
    NotConfigured,
    /// Any other non-2xx (400 `bad_request`, 403 `client_not_permitted`, 404, 502
    /// `provider_rejected`, ...).
    Rejected(u16),
    /// A transport failure.
    Network,
}

impl RelayError {
    /// The connection error for a failed relay call; `rejected` names what a rejection means for
    /// the operation (a code exchange that failed is `TokenExchange`).
    pub(crate) fn into_conn(self, rejected: ConnError) -> ConnError {
        match self {
            RelayError::NotSignedIn => ConnError::NotSignedIn,
            RelayError::RateLimited => ConnError::RateLimited,
            RelayError::NotConfigured => ConnError::NotConfigured,
            RelayError::Rejected(_) => rejected,
            RelayError::Network => ConnError::Network,
        }
    }

    /// A short reason for a log line or a member message (no body, no credential).
    pub(crate) fn reason(self) -> String {
        match self {
            RelayError::NotSignedIn => "not signed in to Citrate".to_string(),
            RelayError::RateLimited => "the Citrate relay is busy".to_string(),
            RelayError::NotConfigured => "the Citrate relay has no client for it".to_string(),
            RelayError::Rejected(status) => format!("the relay answered HTTP {status}"),
            RelayError::Network => "the Citrate relay could not be reached".to_string(),
        }
    }

    fn from_auth(e: AuthError) -> RelayError {
        match e {
            AuthError::Unauthorized | AuthError::NotSignedIn => RelayError::NotSignedIn,
            AuthError::Rejected(429) => RelayError::RateLimited,
            AuthError::Rejected(503) => RelayError::NotConfigured,
            AuthError::Rejected(status) => RelayError::Rejected(status),
            _ => RelayError::Network,
        }
    }
}

/// The relay as the connection manager uses it: one JSON POST per operation. The production
/// transport is [`AuthorityRelay`]; tests script it.
pub(crate) trait RelayTransport {
    /// POST `body` to `/oauth/relay/v1/{provider}/{op}` as the signed-in member; the 2xx body.
    fn post(
        &self,
        provider: &str,
        op: RelayOp,
        body: &str,
    ) -> std::result::Result<Zeroizing<String>, RelayError>;
}

/// The production transport: the member's live session on auth.citrate.ai.
pub(crate) struct AuthorityRelay<'a> {
    auth: &'a AuthManager,
    vault: &'a CustodyVault,
}

impl<'a> AuthorityRelay<'a> {
    pub(crate) fn new(auth: &'a AuthManager, vault: &'a CustodyVault) -> Self {
        AuthorityRelay { auth, vault }
    }
}

impl RelayTransport for AuthorityRelay<'_> {
    fn post(
        &self,
        provider: &str,
        op: RelayOp,
        body: &str,
    ) -> std::result::Result<Zeroizing<String>, RelayError> {
        let path = relay_path(provider, op);
        match self.auth.post_json_to_authority(&path, body) {
            Ok(out) => Ok(Zeroizing::new(out)),
            // An expired or missing in-memory session: refresh it once from the vaulted refresh
            // token (the relay accepts only a live citrate-core access token), then retry once.
            Err(AuthError::Unauthorized | AuthError::NotSignedIn) => {
                self.auth
                    .refresh(self.vault)
                    .map_err(|_| RelayError::NotSignedIn)?;
                self.auth
                    .post_json_to_authority(&path, body)
                    .map(Zeroizing::new)
                    .map_err(RelayError::from_auth)
            }
            Err(e) => Err(RelayError::from_auth(e)),
        }
    }
}

#[derive(Serialize)]
struct TokenRequest<'a> {
    code: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
}

#[derive(Serialize)]
struct RefreshRequest<'a> {
    refresh_token: &'a str,
}

#[derive(Serialize)]
struct RevokeRequest<'a> {
    token: &'a str,
}

fn to_body(value: &impl Serialize) -> std::result::Result<Zeroizing<String>, ConnError> {
    serde_json::to_string(value)
        .map(Zeroizing::new)
        .map_err(|_| ConnError::TokenExchange)
}

/// `{ code, code_verifier, redirect_uri }`. No client id or secret: the relay adds both.
pub(crate) fn token_body(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> std::result::Result<Zeroizing<String>, ConnError> {
    to_body(&TokenRequest {
        code,
        code_verifier: verifier,
        redirect_uri,
    })
}

/// `{ refresh_token }`.
pub(crate) fn refresh_body(
    refresh_token: &str,
) -> std::result::Result<Zeroizing<String>, ConnError> {
    to_body(&RefreshRequest { refresh_token })
}

/// `{ token }` (RFC 7009 field name; the relay maps it to the provider's own field).
pub(crate) fn revoke_body(token: &str) -> std::result::Result<Zeroizing<String>, ConnError> {
    to_body(&RevokeRequest { token })
}
