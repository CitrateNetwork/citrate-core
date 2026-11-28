//! `ConnectorTokenSource` (lanes/L6 section 3.2, frozen): the only way L1's MCP gateway gets a
//! bearer for a remote server (L6-WP06 step 4).
//!
//! - A bearer is returned only for the resource the token is bound to (or a URL below it on the
//!   same origin); anything else is `ResourceMismatch`, and nothing is sent.
//! - An expired token is refreshed once, single-flight per connector: concurrent callers wait
//!   on one refresh and read the record it wrote. A rotated refresh token replaces the old one in
//!   the same vault put, so the old one is never sent again.
//! - A `401 invalid_token` marks the record; the next use refreshes once. When that is not
//!   possible the member is asked to reconnect, once (`l6:connector-auth-required`, deduped until
//!   the connector works again).
//! - Revoke calls the server's revocation endpoint (or the relay) and clears the slot whatever
//!   the answer.

use super::discovery::parse_www_authenticate;
use super::flow::{client_fields, post_token_form, relay_err, Grant};
use super::http::{self, McpHttp, Request};
use super::registration::{self, Client};
use super::store::{self, TokenRecord};
use super::McpAuthError;
use crate::connections::now_unix;
use crate::connections::oauth_client::relay::{RelayOp, RelayTransport};
use crate::custody::CustodyVault;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use zeroize::Zeroizing;

/// A token this close to its stated expiry is refreshed before use.
pub(crate) const REFRESH_SKEW_SECS: u64 = 60;

/// The token source L1's gateway calls (frozen interface).
pub trait ConnectorTokenSource: Send + Sync {
    /// A bearer for exactly this connector and this resource (RFC 8707 canonical URI).
    /// Refreshes once if expired. Never returns a token minted for another resource.
    fn bearer_for(
        &self,
        connector_id: &str,
        resource: &url::Url,
    ) -> Result<Zeroizing<String>, ConnectorAuthError>;
    /// Called by the gateway on an upstream 401 with its WWW-Authenticate header.
    fn on_unauthorized(&self, connector_id: &str, www_authenticate: Option<&str>);
}

/// Why no bearer (frozen interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorAuthError {
    NotConnected,
    VaultLocked,
    Expired,
    ReauthRequired,
    ResourceMismatch,
    Network,
}

/// Where an auth-required notice goes: `(connectorId, reason)`. Production emits the Tauri event
/// `l6:connector-auth-required`; tests record it.
pub(crate) type AuthRequiredSink = Box<dyn Fn(&str, &str) + Send + Sync>;

/// The process-wide MCP token manager.
pub(crate) struct McpTokens {
    net: Box<dyn McpHttp>,
    flights: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    notified: Mutex<HashSet<String>>,
    sink: AuthRequiredSink,
}

/// What a revoke did at the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Revoked {
    NothingStored,
    /// The server (or the relay) confirmed the revoke.
    Confirmed,
    /// No revocation endpoint: the grant stays listed at the provider.
    NotSupported,
    /// Attempted, not confirmed (HTTP status, or 0 for a transport failure).
    Failed(u16),
}

impl McpTokens {
    pub(crate) fn new(net: Box<dyn McpHttp>, sink: AuthRequiredSink) -> Self {
        McpTokens {
            net,
            flights: Mutex::new(HashMap::new()),
            notified: Mutex::new(HashSet::new()),
            sink,
        }
    }

    fn flight(&self, connector_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.flights.lock().unwrap_or_else(PoisonError::into_inner);
        map.entry(connector_id.to_string()).or_default().clone()
    }

    /// Ask the member to reconnect, once until the connector works again.
    fn notify(&self, connector_id: &str, reason: &str) {
        let first = self
            .notified
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(connector_id.to_string());
        if first {
            (self.sink)(connector_id, reason);
        }
    }

    /// The connector works again (a fresh token): a later failure may notify again.
    pub(crate) fn clear_notice(&self, connector_id: &str) {
        self.notified
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(connector_id);
    }

    /// See [`ConnectorTokenSource::bearer_for`].
    pub(crate) fn bearer(
        &self,
        vault: &CustodyVault,
        relay: Option<&dyn RelayTransport>,
        connector_id: &str,
        resource: &url::Url,
    ) -> Result<Zeroizing<String>, ConnectorAuthError> {
        let rec = read(vault, connector_id)?;
        if !rec.serves(resource) {
            return Err(ConnectorAuthError::ResourceMismatch);
        }
        if usable(&rec, now_unix()) {
            return Ok(token_of(&rec));
        }
        let flight = self.flight(connector_id);
        let _guard = flight.lock().unwrap_or_else(PoisonError::into_inner);
        // Another caller may have refreshed while this one waited.
        let rec = read(vault, connector_id)?;
        if usable(&rec, now_unix()) {
            return Ok(token_of(&rec));
        }
        let Some(refresh) = rec.refresh_token.as_ref().filter(|t| !t.is_blank()) else {
            self.notify(connector_id, "expired");
            return Err(if rec.needs_refresh {
                ConnectorAuthError::ReauthRequired
            } else {
                ConnectorAuthError::Expired
            });
        };
        let refresh = Zeroizing::new(refresh.expose().to_string());
        match self.refresh(vault, relay, &rec, &refresh) {
            Ok(grant) => {
                let fresh = merge(rec, grant);
                store::write(vault, &fresh).map_err(|_| ConnectorAuthError::VaultLocked)?;
                self.clear_notice(connector_id);
                if !usable(&fresh, now_unix()) {
                    return Err(ConnectorAuthError::ReauthRequired);
                }
                Ok(token_of(&fresh))
            }
            Err(McpAuthError::Network | McpAuthError::Timeout) => Err(ConnectorAuthError::Network),
            Err(_) => {
                self.notify(connector_id, "refresh_rejected");
                Err(ConnectorAuthError::ReauthRequired)
            }
        }
    }

    /// One refresh request (direct, or through the relay), carrying `resource`.
    fn refresh(
        &self,
        vault: &CustodyVault,
        relay: Option<&dyn RelayTransport>,
        rec: &TokenRecord,
        refresh_token: &str,
    ) -> Result<Grant, McpAuthError> {
        let client = client_of(vault, rec)?;
        if let Client::Relay { provider, .. } = &client {
            let relay = relay.ok_or(McpAuthError::NotSignedIn)?;
            let body = Zeroizing::new(
                serde_json::json!({ "refresh_token": refresh_token, "resource": rec.resource })
                    .to_string(),
            );
            let out = relay
                .post(provider, RelayOp::Refresh, &body)
                .map_err(relay_err)?;
            return super::flow::parse_grant(200, &out);
        }
        let endpoint = http::parse_checked(&rec.token_endpoint)?;
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("resource", rec.resource.as_str()),
        ];
        client_fields(&client, &mut form);
        post_token_form(self.net.as_ref(), &endpoint, &client, &form)
    }

    /// See [`ConnectorTokenSource::on_unauthorized`].
    pub(crate) fn unauthorized(&self, vault: &CustodyVault, connector_id: &str, www: Option<&str>) {
        let challenge = www.and_then(parse_www_authenticate);
        if challenge.as_ref().and_then(|c| c.error.as_deref()) == Some("insufficient_scope") {
            // More scope needs the member's consent (step-up); a refresh cannot add scope.
            self.notify(connector_id, "insufficient_scope");
            return;
        }
        let flight = self.flight(connector_id);
        let _guard = flight.lock().unwrap_or_else(PoisonError::into_inner);
        let Ok(mut rec) = store::read(vault, connector_id) else {
            return;
        };
        let has_refresh = rec.refresh_token.as_ref().is_some_and(|t| !t.is_blank());
        rec.needs_refresh = true;
        let _ = store::write(vault, &rec);
        if !has_refresh {
            self.notify(connector_id, "invalid_token");
        }
    }

    /// Revoke at the server where possible, then clear the slot whatever the answer.
    pub(crate) fn revoke(
        &self,
        vault: &CustodyVault,
        relay: Option<&dyn RelayTransport>,
        connector_id: &str,
    ) -> Result<Revoked, McpAuthError> {
        let rec = match store::read(vault, connector_id) {
            Ok(r) => r,
            Err(McpAuthError::Vault) => return Err(McpAuthError::Vault),
            Err(_) => return Ok(Revoked::NothingStored),
        };
        let token = rec
            .refresh_token
            .as_ref()
            .filter(|t| !t.is_blank())
            .unwrap_or(&rec.access_token);
        let hint = if rec.refresh_token.is_some() {
            "refresh_token"
        } else {
            "access_token"
        };
        let outcome = match client_of(vault, &rec) {
            Ok(Client::Relay { provider, .. }) => match relay {
                Some(relay) => {
                    let body =
                        Zeroizing::new(serde_json::json!({ "token": token.expose() }).to_string());
                    match relay.post(provider, RelayOp::Revoke, &body) {
                        Ok(_) => Revoked::Confirmed,
                        Err(_) => Revoked::Failed(0),
                    }
                }
                None => Revoked::Failed(0),
            },
            Ok(client) => match rec.revocation_endpoint.as_deref().map(http::parse_checked) {
                Some(Ok(endpoint)) => {
                    let mut form: Vec<(&str, &str)> =
                        vec![("token", token.expose()), ("token_type_hint", hint)];
                    client_fields(&client, &mut form);
                    let basic = match &client {
                        Client::Confidential {
                            client_id,
                            secret,
                            basic: true,
                        } => Some((client_id.as_str(), secret.expose())),
                        _ => None,
                    };
                    match http::send(
                        self.net.as_ref(),
                        &endpoint,
                        Request::PostForm { form: &form, basic },
                    ) {
                        Ok(r) if (200..300).contains(&r.status) => Revoked::Confirmed,
                        Ok(r) => Revoked::Failed(r.status),
                        Err(_) => Revoked::Failed(0),
                    }
                }
                _ => Revoked::NotSupported,
            },
            Err(_) => Revoked::Failed(0),
        };
        store::clear(vault, connector_id)?;
        self.clear_notice(connector_id);
        Ok(outcome)
    }
}

fn read(vault: &CustodyVault, connector_id: &str) -> Result<TokenRecord, ConnectorAuthError> {
    store::read(vault, connector_id).map_err(|e| match e {
        McpAuthError::Vault => ConnectorAuthError::VaultLocked,
        _ => ConnectorAuthError::NotConnected,
    })
}

fn usable(rec: &TokenRecord, now: u64) -> bool {
    !rec.needs_refresh
        && !rec.access_token.is_blank()
        && rec
            .expires_at
            .is_none_or(|exp| exp > now.saturating_add(REFRESH_SKEW_SECS))
}

fn token_of(rec: &TokenRecord) -> Zeroizing<String> {
    Zeroizing::new(rec.access_token.expose().to_string())
}

/// The client a sealed record was connected with (a DCR client re-reads its sealed secret).
fn client_of(vault: &CustodyVault, rec: &TokenRecord) -> Result<Client, McpAuthError> {
    match rec.client.kind.as_str() {
        "cimd" => Ok(Client::Public {
            kind: "cimd",
            client_id: rec.client.client_id.clone(),
        }),
        "dcr" => registration::sealed_dcr(vault, &rec.issuer)
            .filter(|d| d.client_id == rec.client.client_id)
            .map(registration::dcr_client)
            .ok_or(McpAuthError::ReauthRequired),
        "relay" => {
            let row = registration::fixed_for(&rec.issuer).ok_or(McpAuthError::ReauthRequired)?;
            let provider = rec.client.provider.as_deref().unwrap_or("");
            match row.client_id {
                Some(id) if row.provider == provider && id == rec.client.client_id => {
                    Ok(Client::Relay {
                        provider: row.provider,
                        client_id: id,
                        redirect_uri: row.redirect_uri,
                    })
                }
                _ => Err(McpAuthError::ReauthRequired),
            }
        }
        _ => Err(McpAuthError::ReauthRequired),
    }
}

/// The record after a refresh: the new access token, the rotated refresh token when the server
/// sent one (else the old one), the new expiry, the first `connected_at`.
fn merge(mut rec: TokenRecord, grant: Grant) -> TokenRecord {
    rec.access_token = grant.access_token;
    if let Some(rt) = grant.refresh_token.filter(|t| !t.is_blank()) {
        rec.refresh_token = Some(rt);
    }
    rec.expires_at = grant.expires_in.map(|e| now_unix().saturating_add(e));
    if grant.scope.is_some() {
        rec.scope = grant.scope;
    }
    rec.needs_refresh = false;
    rec
}

/// The token source bound to the vault and the member's relay session for one call site.
pub(crate) struct BoundTokenSource<'a> {
    pub tokens: &'a McpTokens,
    pub vault: &'a CustodyVault,
    pub relay: Option<&'a (dyn RelayTransport + Sync)>,
}

impl ConnectorTokenSource for BoundTokenSource<'_> {
    fn bearer_for(
        &self,
        connector_id: &str,
        resource: &url::Url,
    ) -> Result<Zeroizing<String>, ConnectorAuthError> {
        let relay = self.relay.map(|r| r as &dyn RelayTransport);
        self.tokens
            .bearer(self.vault, relay, connector_id, resource)
    }

    fn on_unauthorized(&self, connector_id: &str, www_authenticate: Option<&str>) {
        self.tokens
            .unauthorized(self.vault, connector_id, www_authenticate);
    }
}

/// The production sink: the `l6:connector-auth-required { connectorId, reason }` event (L3's
/// inbox renders it; it never carries an approve button, F-13).
pub(crate) fn tauri_sink(app: tauri::AppHandle) -> AuthRequiredSink {
    Box::new(move |connector_id: &str, reason: &str| {
        use tauri::Emitter as _;
        let _ = app.emit(
            "l6:connector-auth-required",
            serde_json::json!({ "connectorId": connector_id, "reason": reason }),
        );
    })
}
