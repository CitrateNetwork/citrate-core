//! The sealed MCP token record (L6-WP06) and the module's only vault write.
//!
//! One record per connector in `connection-mcp-<connectorId>`, bound to exactly one resource
//! (the PRM `resource`) and one issuer. A record is read back only in-process (I-2: no command
//! returns it). Each member profile has its own custody vault file, so a record never crosses
//! profiles; a connector id never reads another connector's slot.

use super::discovery::canonical_key;
use super::McpAuthError;
use crate::custody::CustodyVault;
use crate::secret::SecretString;
use serde::{Deserialize, Serialize};
use url::Url;
use zeroize::Zeroize as _;

/// Record format version.
pub(crate) const RECORD_V: u32 = 1;

/// Whether `id` is a valid connector id: 1..=64 of `[a-z0-9-]`, starting with a letter or digit.
pub(crate) fn valid_connector_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `connection-mcp-<connectorId>`.
pub(crate) fn token_slot(connector_id: &str) -> Result<String, McpAuthError> {
    if !valid_connector_id(connector_id) {
        return Err(McpAuthError::NotConnected);
    }
    Ok(format!("connection-mcp-{connector_id}"))
}

/// How the connection's client authenticates on refresh and revoke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ClientRef {
    /// `cimd` | `dcr` | `relay`.
    pub kind: String,
    pub client_id: String,
    /// The relay provider, for `relay`.
    #[serde(default)]
    pub provider: Option<String>,
    pub redirect_uri: String,
}

/// The sealed record.
#[derive(Serialize, Deserialize)]
pub(crate) struct TokenRecord {
    pub v: u32,
    pub connector_id: String,
    /// The PRM `resource`, as published (the RFC 8707 value sent on refresh).
    pub resource: String,
    pub issuer: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    pub client: ClientRef,
    #[serde(serialize_with = "crate::secret::serialize_exposed")]
    pub access_token: SecretString,
    #[serde(default, serialize_with = "crate::secret::serialize_exposed_opt")]
    pub refresh_token: Option<SecretString>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    pub connected_at: u64,
    /// Set by a `401 invalid_token`: the next use refreshes once before sending anything.
    #[serde(default)]
    pub needs_refresh: bool,
    /// Requested scopes the server did not grant (lead decision 2026-11-28: the connector is
    /// "limited", the member sees this list, and tools needing one of these are unavailable).
    #[serde(default)]
    pub missing_scopes: Vec<String>,
}

impl std::fmt::Debug for TokenRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenRecord")
            .field("connector_id", &self.connector_id)
            .field("resource", &self.resource)
            .field("client", &self.client.kind)
            .field("access_token", &"<redacted>")
            .finish()
    }
}

impl TokenRecord {
    /// Connected with fewer scopes than requested.
    pub(crate) fn limited(&self) -> bool {
        !self.missing_scopes.is_empty()
    }

    /// Whether this record's token may be sent to `requested`: the requested URL is the bound
    /// resource or below it on the same origin ([`super::discovery::resource_covers`]).
    pub(crate) fn serves(&self, requested: &Url) -> bool {
        Url::parse(&self.resource)
            .map(|r| super::discovery::resource_covers(&r, requested))
            .unwrap_or(false)
    }

    /// The canonical key of the bound resource.
    pub(crate) fn resource_key(&self) -> String {
        Url::parse(&self.resource)
            .map(|r| canonical_key(&r))
            .unwrap_or_default()
    }
}

/// Seal `bytes` in `slot` (a `connection-` slot) and wipe them. The module's only vault write
/// (`vault_slot_reservation_tests` allowlists it).
pub(crate) fn seal(
    vault: &CustodyVault,
    slot_name: &str,
    mut bytes: Vec<u8>,
) -> Result<(), McpAuthError> {
    if !slot_name.starts_with("connection-mcp-") && !slot_name.starts_with("connection-dcr-") {
        bytes.zeroize();
        return Err(McpAuthError::Vault);
    }
    let put = vault.put(slot_name, &mut bytes);
    bytes.zeroize();
    put.map_err(|_| McpAuthError::Vault)
}

/// Seal a token record.
pub(crate) fn write(vault: &CustodyVault, rec: &TokenRecord) -> Result<(), McpAuthError> {
    let slot = token_slot(&rec.connector_id)?;
    let bytes = serde_json::to_vec(rec).map_err(|_| McpAuthError::Vault)?;
    seal(vault, &slot, bytes)
}

/// Read a token record. `Vault` when locked (a read never forces an unlock), `NotConnected` when
/// absent, unreadable, or written for another connector id.
pub(crate) fn read(vault: &CustodyVault, connector_id: &str) -> Result<TokenRecord, McpAuthError> {
    if !vault.is_unlocked() {
        return Err(McpAuthError::Vault);
    }
    let bytes = vault
        .custody_get(&token_slot(connector_id)?)
        .map_err(|_| McpAuthError::NotConnected)?;
    let rec: TokenRecord =
        serde_json::from_slice(&bytes).map_err(|_| McpAuthError::NotConnected)?;
    if rec.connector_id != connector_id || rec.v != RECORD_V {
        return Err(McpAuthError::NotConnected);
    }
    Ok(rec)
}

/// Forget a connector's token.
pub(crate) fn clear(vault: &CustodyVault, connector_id: &str) -> Result<(), McpAuthError> {
    if !vault.is_unlocked() {
        return Err(McpAuthError::Vault);
    }
    vault
        .clear_slot(&token_slot(connector_id)?)
        .map_err(|_| McpAuthError::Vault)
}
