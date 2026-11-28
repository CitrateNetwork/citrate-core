//! Revoke at the provider, then forget the token (L6-WP05b).
//!
//! [`ConnectionManager::disconnect_with`] reads the sealed record, attempts the provider revoke
//! where one exists (through the relay in relay mode), records the outcome in the log, and only
//! then clears the vault slot. The slot is cleared on the member's request whatever the revoke
//! outcome; a failed revoke is reported honestly ([`RevokeOutcome::member_message`]).

use super::relay::{self, RelayOp, RelayTransport};
use crate::connections::{slot, ClientPlan, ConnError, ConnectionManager, Service};
use crate::custody::CustodyVault;

/// Google's RFC 7009 revocation endpoint (a public client may call it with the token alone).
const GOOGLE_REVOKE: &str = "https://oauth2.googleapis.com/revoke";

/// What happened at the provider when a connection was disconnected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RevokeOutcome {
    /// No token was stored: nothing to revoke.
    NothingStored,
    /// The provider confirmed the revoke.
    Revoked,
    /// This provider or this build has no revoke path; the grant stays listed at the provider.
    NotSupported,
    /// The revoke was attempted and not confirmed; carries a short reason (never a credential).
    Failed(String),
}

impl RevokeOutcome {
    fn log_word(&self) -> &'static str {
        match self {
            RevokeOutcome::NothingStored => "nothing stored",
            RevokeOutcome::Revoked => "revoked at the provider",
            RevokeOutcome::NotSupported => "no revoke path; grant stays at the provider",
            RevokeOutcome::Failed(_) => "revoke failed",
        }
    }

    /// The message the member sees when the revoke failed (the slot is cleared all the same).
    pub(crate) fn member_message(&self, service: Service) -> Option<String> {
        match self {
            RevokeOutcome::Failed(reason) => Some(format!(
                "Disconnected on this device: the token is removed from this computer. {} did \
                 not confirm it revoked access ({reason}); you can remove Citrate Core in your \
                 {} account settings.",
                provider_name(service),
                provider_name(service)
            )),
            _ => None,
        }
    }
}

fn provider_name(service: Service) -> &'static str {
    match service {
        Service::GitHub => "GitHub",
        Service::Notion => "Notion",
        Service::GoogleDrive | Service::GoogleSheets | Service::GoogleCalendar => "Google",
        Service::HuggingFace => "Hugging Face",
    }
}

impl ConnectionManager {
    /// Revoke where supported, record the outcome, then clear the slot. `Vault` when the vault
    /// is locked (nothing can be read or cleared; the slot is left as it was).
    pub(crate) fn disconnect_with(
        &self,
        service: Service,
        vault: &CustodyVault,
        relay: Option<&dyn RelayTransport>,
    ) -> std::result::Result<RevokeOutcome, ConnError> {
        let outcome = match super::refresh::read_record(service, vault) {
            Ok(rec) => self.revoke_at_provider(
                service,
                rec.access_token.expose(),
                rec.refresh_token.as_ref().map(|t| t.expose()),
                relay,
            ),
            Err(ConnError::Vault) => return Err(ConnError::Vault),
            Err(_) => RevokeOutcome::NothingStored,
        };
        // Recorded before the slot is cleared.
        let line = match &outcome {
            RevokeOutcome::Failed(reason) => format!(
                "connections: {} disconnect: {} ({reason}); clearing {}",
                service.id(),
                outcome.log_word(),
                slot(service)
            ),
            _ => format!(
                "connections: {} disconnect: {}; clearing {}",
                service.id(),
                outcome.log_word(),
                slot(service)
            ),
        };
        self.log(&line);
        vault
            .clear_slot(&slot(service))
            .map_err(|_| ConnError::Vault)?;
        Ok(outcome)
    }

    fn revoke_at_provider(
        &self,
        service: Service,
        access_token: &str,
        refresh_token: Option<&str>,
        relay: Option<&dyn RelayTransport>,
    ) -> RevokeOutcome {
        let Ok(plan) = self.plan(service) else {
            return RevokeOutcome::NotSupported;
        };
        match plan {
            ClientPlan::Relay { provider, .. } => {
                if !super::relay_revokes(service) {
                    return RevokeOutcome::NotSupported;
                }
                let Some(relay) = relay else {
                    return RevokeOutcome::Failed(relay::RelayError::NotSignedIn.reason());
                };
                let body = match relay::revoke_body(access_token) {
                    Ok(body) => body,
                    Err(_) => {
                        return RevokeOutcome::Failed("the request could not be built".into())
                    }
                };
                match relay.post(provider, RelayOp::Revoke, &body) {
                    Ok(answer) => revoke_answer(&answer),
                    Err(e) => RevokeOutcome::Failed(e.reason()),
                }
            }
            #[cfg(debug_assertions)]
            ClientPlan::DevFile(_) => self.revoke_direct(service, access_token, refresh_token),
            ClientPlan::PublicPkce { .. } => {
                self.revoke_direct(service, access_token, refresh_token)
            }
        }
    }

    /// A provider whose RFC 7009 endpoint takes the token alone (Google). Revoking the refresh
    /// token ends the whole grant, so it goes first when one is sealed.
    fn revoke_direct(
        &self,
        service: Service,
        access_token: &str,
        refresh_token: Option<&str>,
    ) -> RevokeOutcome {
        match service {
            Service::GoogleDrive | Service::GoogleSheets | Service::GoogleCalendar => {
                let token = refresh_token
                    .filter(|t| !t.is_empty())
                    .unwrap_or(access_token);
                match self.http.post_form(GOOGLE_REVOKE, &[("token", token)]) {
                    Ok(_) => RevokeOutcome::Revoked,
                    Err(_) => RevokeOutcome::Failed("Google did not accept the revoke".into()),
                }
            }
            // GitHub's revoke needs the client secret (HTTP Basic); only the relay holds it.
            // Notion and Hugging Face publish no revocation endpoint.
            Service::GitHub | Service::Notion | Service::HuggingFace => RevokeOutcome::NotSupported,
        }
    }
}

/// The relay's revoke answer: `{"revoked":true,"upstream_status":N}` on success.
fn revoke_answer(body: &str) -> RevokeOutcome {
    #[derive(serde::Deserialize)]
    struct Answer {
        #[serde(default)]
        revoked: Option<bool>,
        #[serde(default)]
        upstream_status: Option<u16>,
    }
    match serde_json::from_str::<Answer>(body) {
        Ok(Answer {
            revoked: Some(false),
            upstream_status,
        }) => RevokeOutcome::Failed(match upstream_status {
            Some(s) => format!("the provider answered HTTP {s}"),
            None => "the provider did not confirm".to_string(),
        }),
        // A 2xx from the relay is a confirmed revoke.
        _ => RevokeOutcome::Revoked,
    }
}
