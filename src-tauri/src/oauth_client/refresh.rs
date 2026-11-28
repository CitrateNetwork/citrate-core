//! Refresh an expired connection token before use (L6-WP05b).
//!
//! [`ConnectionManager::fresh_access_token`] returns the sealed access token while it is valid.
//! When the provider's stated expiry is near or past and a refresh token is sealed, it refreshes
//! once (through the relay in relay mode), seals the new record in one vault put, and returns the
//! new token. A rotated refresh token replaces the old one in that same put, so the old one is
//! never sent again. Any failure means "connect again" (`ReauthRequired`); it never loops.

use super::relay::{self, RelayOp, RelayTransport};
use crate::connections::{
    now_unix, slot, ClientPlan, ConnError, ConnectionManager, SealedRecord, Service,
};
use crate::custody::CustodyVault;
use zeroize::Zeroizing;

/// A token this close to its stated expiry is refreshed before use.
pub(crate) const REFRESH_SKEW_SECS: u64 = 60;

impl ConnectionManager {
    /// The service's access token for an in-process request, refreshed once if it has expired.
    /// I-2: crate-private and never reachable from a `#[tauri::command]` return value.
    pub(crate) fn fresh_access_token(
        &self,
        service: Service,
        vault: &CustodyVault,
        relay: Option<&dyn RelayTransport>,
    ) -> std::result::Result<Zeroizing<String>, ConnError> {
        // One refresh at a time: a second caller waits, then reads the record the first wrote.
        let _guard = self
            .refresh_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rec = read_record(service, vault)?;
        let now = now_unix();
        let present = !rec.access_token.expose().is_empty();
        let still_valid = present && rec.expires_at.is_none_or(|exp| exp > now);
        let due = rec
            .expires_at
            .is_some_and(|exp| exp <= now.saturating_add(REFRESH_SKEW_SECS));
        let current = || Zeroizing::new(rec.access_token.expose().to_string());
        if still_valid && !due {
            return Ok(current());
        }
        let Some(refresh_token) = rec.refresh_token.as_ref().filter(|t| !t.is_blank()) else {
            if still_valid {
                // Inside the skew with nothing to refresh with: usable until it expires.
                return Ok(current());
            }
            self.log(&format!(
                "connections: {} token expired and no refresh token is sealed; connect again",
                service.id()
            ));
            return Err(ConnError::ReauthRequired);
        };
        let plan = match self.plan(service) {
            Ok(plan) => plan,
            Err(_) if still_valid => return Ok(current()),
            Err(_) => return Err(ConnError::ReauthRequired),
        };
        let body = match &plan {
            ClientPlan::Relay { provider, .. } => match relay {
                Some(relay) => relay::refresh_body(refresh_token.expose()).and_then(|req| {
                    relay
                        .post(provider, RelayOp::Refresh, &req)
                        .map_err(|e| e.into_conn(ConnError::ReauthRequired))
                }),
                None => Err(ConnError::NotSignedIn),
            },
            // The dev file (debug builds) or a public client: straight to the provider.
            _ => self.post_provider_form(
                service.token_endpoint(),
                &plan.direct_refresh_params(refresh_token.expose()),
            ),
        };
        let sealed =
            body.and_then(|body| self.seal_token_response(service, &body, vault, Some(&rec)));
        if let Err(e) = sealed {
            self.log(&format!(
                "connections: {} refresh via {} failed: {e}",
                service.id(),
                plan.mode()
            ));
            if still_valid {
                return Ok(current());
            }
            return Err(match e {
                ConnError::NotSignedIn | ConnError::RateLimited | ConnError::Vault => e,
                _ => ConnError::ReauthRequired,
            });
        }
        self.log(&format!(
            "connections: {} token refreshed via {}",
            service.id(),
            plan.mode()
        ));
        let fresh = read_record(service, vault)?;
        if fresh.access_token.expose().is_empty()
            || fresh.expires_at.is_some_and(|exp| exp <= now_unix())
        {
            // Refreshed once and still not usable: never loop.
            return Err(ConnError::ReauthRequired);
        }
        Ok(Zeroizing::new(fresh.access_token.expose().to_string()))
    }
}

/// The whole sealed record for `service`. `NotConnected` when the slot is absent; `Vault` when
/// the vault is locked (a read never forces an unlock).
pub(in crate::connections) fn read_record(
    service: Service,
    vault: &CustodyVault,
) -> std::result::Result<SealedRecord, ConnError> {
    if !vault.is_unlocked() {
        return Err(ConnError::Vault);
    }
    let bytes = vault
        .custody_get(&slot(service))
        .map_err(|_| ConnError::NotConnected)?;
    serde_json::from_slice(&bytes).map_err(|_| ConnError::NotConnected)
}
