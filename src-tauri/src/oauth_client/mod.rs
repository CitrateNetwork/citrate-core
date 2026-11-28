//! 0.6.0 L6-WP05: Core OAuth client v2 (planset 2026-11-07-v060, lanes/L6 section 3.2, WP05).
//!
//! A desktop build never holds a provider client secret. Each MCP service has a row in a
//! compiled provider registry ([`providers`]) naming how its OAuth client works:
//!
//! - [`ClientMode::Relay`]: the provider's token endpoint needs a client secret, so the device
//!   holds only the public client id and exchanges, refreshes and revokes through the Citrate
//!   relay on auth.citrate.ai ([`relay`]), which adds the secret server-side. The relay accepts
//!   only a signed-in member's auth.citrate.ai access token as its bearer.
//! - [`ClientMode::PublicPkce`]: a public client; the device exchanges directly, verifier only.
//! - [`ClientMode::Cimd`] / [`ClientMode::Dcr`]: frozen for WP06 (MCP authorization).
//!
//! The registry applies only while `CONNECTIONS_V2` is on. Off, a release build reports every
//! service "not configured" and never falls back to a device-held secret. Debug builds keep the
//! developer's `oauth.dev.json` path (`connections::ConnectionManager::creds`); a release build
//! does not compile it.
//!
//! Data source (Rule 7): the compiled table here (public client ids only, safe to embed like
//! `social.rs`), the relay endpoints of lanes/L6 section 3.1 as built in citrate-identity
//! (`src/oauth-relay/routes.ts`, `docs/OAUTH_RELAY.md`), and the custody vault slot
//! `connection-<id>`.
//!
//! Refresh ([`refresh`]) and revoke ([`revoke`]) are methods on the connection manager.

mod providers;
pub(crate) mod refresh;
pub(crate) mod relay;
pub(crate) mod revoke;

#[cfg(test)]
mod tests;

use super::Service;

pub use providers::provider_setup;
pub(crate) use providers::setup_for;
// L6-WP06: the GitHub MCP server uses the same OAuth App through the relay.
pub(crate) use providers::GITHUB_CLIENT_ID;

/// How a provider's OAuth client is set up (lanes/L6 section 3.2, frozen).
#[derive(Debug, Clone, PartialEq, Eq)]
// Cimd and Dcr are part of the frozen interface; WP06 (MCP authorization) builds them.
#[allow(dead_code)]
pub enum ClientMode {
    /// A public client: authorization code + PKCE, no secret anywhere.
    PublicPkce,
    /// A client identified by its metadata document URL (CIMD, WP03).
    Cimd,
    /// A dynamically registered client (RFC 7591), per authorization server.
    Dcr { registered_client_id: String },
    /// The provider needs a client secret; the Citrate relay holds it.
    Relay,
}

/// One provider's registry row (lanes/L6 section 3.2, frozen). `configured` says the compiled
/// row is complete (a public client id is registered for release builds); whether this run may
/// use it also depends on `CONNECTIONS_V2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSetup {
    /// The provider name the relay knows (`/oauth/relay/v1/{provider}/..`).
    pub provider: &'static str,
    pub mode: ClientMode,
    /// The public client id compiled into the binary, if one is registered.
    pub client_id: Option<&'static str>,
    pub configured: bool,
    /// Why the row is not configured, naming the missing registration.
    pub reason: Option<&'static str>,
}

// @flag CONNECTIONS_V2
/// The connections v2 flag (lanes/L6 WP05): the provider registry and relay mode. Off until a
/// release build has a recorded relay connect (runtime-proven); off means "not configured",
/// never the dev file. Row for `release/flags.json` when L0-WP09 lands it: lane L6, default
/// false, sign-off none.
pub(crate) const CONNECTIONS_V2: bool = false;

/// Whether connections v2 is on for this run. A debug build can turn it on with
/// `CITRATE_CONNECTIONS_V2=1` to exercise the relay path against auth.citrate.ai; a release
/// build reads only the compiled flag.
pub(crate) fn connections_v2_enabled() -> bool {
    #[cfg(debug_assertions)]
    if std::env::var("CITRATE_CONNECTIONS_V2").is_ok_and(|v| v == "1") {
        return true;
    }
    CONNECTIONS_V2
}

/// Whether the provider's relay row has a revocation endpoint (citrate-identity
/// `src/oauth-relay/providers.ts`: GitHub yes, Notion no). A service without one is
/// disconnected locally and the grant stays listed at the provider.
pub(crate) fn relay_revokes(service: Service) -> bool {
    matches!(service, Service::GitHub)
}
