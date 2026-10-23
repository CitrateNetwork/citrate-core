//! K2: remote chat sends need a core-issued confirmation.
//!
//! Under "ask first" (`chat_route::ChatRoute::AskFirst`) a chat message reaches a remote provider
//! only by the member's choice for that message. Core enforces it the way escalation confirmations
//! work (`escalation::Book::prepare_confirmation`): when the app asks the member about a held
//! message, core mints a one-time consent id bound to the provider and to that message
//! ([`chat_consent_prepare`]); `ai::ai_chat` and `ai::ai_chat_tools` send nothing without a valid,
//! unexpired consent for the same provider and message ([`ConsentBook::authorize`]).
//!
//! One consent covers one member message: the agent loop's model requests for that message (at
//! most [`MAX_SENDS_PER_CONSENT`], the app's turn cap) within [`CONSENT_TTL_MS`]. The next
//! message needs a new one. Every remote chat send goes through a [`RemoteSendPermit`], which
//! only [`ConsentBook::authorize`] can create, and the `ai` remote send methods take one.
//!
//! Data sources (Rule 7): the consent book is in memory only (nothing is written to disk); the
//! message binding is the last `user` message of the request the webview sends.

use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// How long a minted consent stays usable.
pub const CONSENT_TTL_MS: u64 = 10 * 60 * 1000;

/// Model requests one consent covers: the app's agent loop cap for one message
/// (`AGENT_MAX_TURNS` in `src/agent/harness.ts`).
pub const MAX_SENDS_PER_CONSENT: u32 = 8;

/// Consents kept open at once (the oldest is dropped first).
pub const MAX_OPEN: usize = 16;

/// Proof that a remote chat send was authorized for one provider. Only
/// [`ConsentBook::authorize`] creates one.
#[derive(Debug)]
pub struct RemoteSendPermit {
    provider_id: String,
}

impl RemoteSendPermit {
    /// The provider this permit is for.
    pub(crate) fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Tests only: a permit for `provider_id` without a consent.
    #[cfg(test)]
    pub(crate) fn for_tests(provider_id: &str) -> Self {
        RemoteSendPermit {
            provider_id: provider_id.to_string(),
        }
    }
}

/// What the app gets back when it opens the question for a held message.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConsentView {
    pub consent_id: String,
    pub provider_id: String,
    pub expires_at_ms: u64,
}

struct Open {
    consent_id: String,
    provider_id: String,
    message_hash: [u8; 32],
    expires_at_ms: u64,
    sends: u32,
}

/// The open consents (in memory).
#[derive(Default)]
pub struct ConsentBook {
    open: Vec<Open>,
}

fn message_hash(text: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"citrate/chat-consent/v1\0");
    h.update(text.as_bytes());
    h.finalize().into()
}

/// The last `user` message's text in an OpenAI-style messages array, if any.
pub fn last_user_text(messages_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(messages_json).ok()?;
    v.as_array()?
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))?
        .get("content")?
        .as_str()
        .map(str::to_string)
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

const NOT_CONFIRMED: &str =
    "this message was not confirmed for the gateway, so nothing was sent. Choose \"Send this message to the gateway this time\" to send it";

impl ConsentBook {
    /// Mint a consent for `message` to `provider_id`. `consent_id` is a fresh random id.
    pub fn prepare(
        &mut self,
        provider_id: &str,
        message: &str,
        now_ms: u64,
        consent_id: String,
    ) -> ConsentView {
        self.open.retain(|o| o.expires_at_ms > now_ms);
        while self.open.len() >= MAX_OPEN {
            self.open.remove(0);
        }
        let expires_at_ms = now_ms.saturating_add(CONSENT_TTL_MS);
        self.open.push(Open {
            consent_id: consent_id.clone(),
            provider_id: provider_id.to_string(),
            message_hash: message_hash(message),
            expires_at_ms,
            sends: 0,
        });
        ConsentView {
            consent_id,
            provider_id: provider_id.to_string(),
            expires_at_ms,
        }
    }

    /// Authorize one remote send: `consent_id` must be open, unexpired, for `provider_id`, and
    /// minted for the last user message in `messages_json`. A consent that fails a check is
    /// spent; one that reaches [`MAX_SENDS_PER_CONSENT`] is spent after that send.
    pub fn authorize(
        &mut self,
        consent_id: &str,
        provider_id: &str,
        messages_json: &str,
        now_ms: u64,
    ) -> Result<RemoteSendPermit, String> {
        self.open.retain(|o| o.expires_at_ms > now_ms);
        let Some(i) = self
            .open
            .iter()
            .position(|o| ct_eq(&o.consent_id, consent_id))
        else {
            return Err(NOT_CONFIRMED.into());
        };
        let bound = last_user_text(messages_json)
            .map(|t| message_hash(&t) == self.open[i].message_hash)
            .unwrap_or(false);
        if self.open[i].provider_id != provider_id || !bound {
            self.open.remove(i);
            return Err(NOT_CONFIRMED.into());
        }
        self.open[i].sends += 1;
        if self.open[i].sends >= MAX_SENDS_PER_CONSENT {
            self.open.remove(i);
        }
        Ok(RemoteSendPermit {
            provider_id: provider_id.to_string(),
        })
    }
}

/// The managed consent book.
#[derive(Default)]
pub struct ChatConsentState(pub Mutex<ConsentBook>);

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_consent_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("cc_{}", hex::encode(b))
}

/// Authorize a remote chat send against the managed book.
pub(crate) fn authorize(
    state: &ChatConsentState,
    consent_id: &str,
    provider_id: &str,
    messages_json: &str,
    now_ms: u64,
) -> Result<RemoteSendPermit, String> {
    state.0.lock().unwrap_or_else(|e| e.into_inner()).authorize(
        consent_id,
        provider_id,
        messages_json,
        now_ms,
    )
}

/// **chat_consent_prepare**: the app is asking the member whether to send a held message to the
/// gateway. Core mints the one-time consent `ai_chat` / `ai_chat_tools` need for that message.
/// Refused under Local model only (nothing remote is offered there).
#[tauri::command]
pub async fn chat_consent_prepare(
    app: tauri::AppHandle,
    provider_id: String,
    message: String,
) -> Result<ConsentView, String> {
    let route_dir = crate::chat_route::route_dir(&app)?;
    crate::blocking::off_main(move || {
        crate::chat_route::remote_chat_allowed(&route_dir)?;
        let st = tauri::Manager::try_state::<ChatConsentState>(&app)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let view = st.0.lock().unwrap_or_else(|e| e.into_inner()).prepare(
            &provider_id,
            &message,
            now_ms(),
            new_consent_id(),
        );
        Ok(view)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("chat_consent_tests.rs");
}
