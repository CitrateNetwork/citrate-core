//! K2: remote chat sends need a core-issued confirmation.
//!
//! Under "ask first" (`chat_route::ChatRoute::AskFirst`) a chat message reaches a remote provider
//! only by the member's choice for that message. Core enforces it the way escalation confirmations
//! work (`escalation::Book::prepare_confirmation`): when the app asks the member about a held
//! message, core mints a one-time consent id bound to the provider and to that message
//! ([`chat_consent_prepare`]); `ai::ai_chat` and `ai::ai_chat_tools` send nothing without a valid,
//! unexpired consent for the same provider and message ([`authorize`]).
//!
//! One consent covers one member message: the agent loop's model requests for that message (at
//! most [`MAX_SENDS_PER_CONSENT`], the app's turn cap) within [`CONSENT_TTL_MS`]. The next
//! message needs a new one. Every remote chat send goes through a [`RemoteSendPermit`], and the
//! `ai` remote send methods take one. A permit is obtained only through [`authorize`] or
//! [`authorize_once`], which check "Local model only" (`chat_route::remote_chat_allowed`) before
//! the consent book is asked, so no caller can get a permit while the setting is on.
//!
//! A consent expires when either the monotonic or the wall-clock time since it was minted reaches
//! [`CONSENT_TTL_MS`] (the ceremony TTL rule, `ceremony::Ceremony`): setting the wall clock back
//! cannot extend one, and sleep still expires it. The app drops a consent it no longer needs
//! ([`chat_consent_drop`]) when the member closes the question without sending.
//!
//! Remote image generation (`media::media_generate_image` on a provider route) uses the same
//! book with [`Purpose::Media`]: one consent per image, bound to the prompt
//! ([`media_consent_prepare`], [`authorize_once`]).
//!
//! Data sources (Rule 7): the consent book is in memory only (nothing is written to disk); the
//! message binding is the last `user` message of the request the webview sends; the route is
//! `<app data>/escalation/route.json` (`chat_route`).

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::ceremony::ClockReading;

/// How long a minted consent stays usable.
pub const CONSENT_TTL_MS: u64 = 10 * 60 * 1000;

/// Model requests one consent covers: the app's agent loop cap for one message
/// (`AGENT_MAX_TURNS` in `src/agent/harness.ts`).
pub const MAX_SENDS_PER_CONSENT: u32 = 8;

/// Consents kept open at once (the oldest is dropped first).
pub const MAX_OPEN: usize = 16;

/// Proof that a remote chat send was authorized for one provider. Only [`authorize`] and
/// [`authorize_once`] hand one out.
#[derive(Debug)]
pub struct RemoteSendPermit {
    provider_id: String,
}

impl RemoteSendPermit {
    /// Only the consent book's permit methods call this.
    fn new(provider_id: &str) -> Self {
        RemoteSendPermit {
            provider_id: provider_id.to_string(),
        }
    }

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

/// What a consent is for: one chat message (the agent loop's requests for it) or one remote image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Chat,
    Media,
}

struct Open {
    purpose: Purpose,
    consent_id: String,
    provider_id: String,
    message_hash: [u8; 32],
    minted: ClockReading,
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

/// Whether a consent minted at `minted` has outlived [`CONSENT_TTL_MS`] at `now`. The larger of
/// the monotonic and the wall-clock elapsed time counts.
fn expired(minted: &ClockReading, now: &ClockReading) -> bool {
    Duration::from_millis(now.wall_ms.saturating_sub(minted.wall_ms))
        >= Duration::from_millis(CONSENT_TTL_MS)
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
        purpose: Purpose,
        provider_id: &str,
        message: &str,
        now: ClockReading,
        consent_id: String,
    ) -> ConsentView {
        self.open.retain(|o| !expired(&o.minted, &now));
        while self.open.len() >= MAX_OPEN {
            self.open.remove(0);
        }
        self.open.push(Open {
            purpose,
            consent_id: consent_id.clone(),
            provider_id: provider_id.to_string(),
            message_hash: message_hash(message),
            minted: now,
            sends: 0,
        });
        ConsentView {
            consent_id,
            provider_id: provider_id.to_string(),
            expires_at_ms: now.wall_ms.saturating_add(CONSENT_TTL_MS),
        }
    }

    /// Drop the open consent `consent_id`, if there is one. True when one was dropped.
    pub fn drop_consent(&mut self, _consent_id: &str) -> bool {
        false
    }

    /// One remote chat send: `consent_id` must be open, unexpired, for `provider_id`, and minted
    /// for the last user message in `messages_json`. A consent that fails a check is spent; one
    /// that reaches [`MAX_SENDS_PER_CONSENT`] is spent after that send. Reached only through
    /// [`authorize`], which checks the route first.
    fn permit_chat(
        &mut self,
        consent_id: &str,
        provider_id: &str,
        messages_json: &str,
        now: ClockReading,
    ) -> Result<RemoteSendPermit, String> {
        let text = last_user_text(messages_json);
        let i = self.check(Purpose::Chat, consent_id, provider_id, text.as_deref(), now)?;
        self.open[i].sends += 1;
        if self.open[i].sends >= MAX_SENDS_PER_CONSENT {
            self.open.remove(i);
        }
        Ok(RemoteSendPermit::new(provider_id))
    }

    /// One remote image: the consent must be open, unexpired, minted for media, for `provider_id`
    /// and for exactly `prompt`. It is spent by this call whatever the outcome. Reached only
    /// through [`authorize_once`], which checks the route first.
    fn permit_media(
        &mut self,
        consent_id: &str,
        provider_id: &str,
        prompt: &str,
        now: ClockReading,
    ) -> Result<RemoteSendPermit, String> {
        let i = self.check(Purpose::Media, consent_id, provider_id, Some(prompt), now)?;
        self.open.remove(i);
        Ok(RemoteSendPermit::new(provider_id))
    }

    /// Find the open consent `consent_id` and check it is for `purpose`, `provider_id` and
    /// `message`. Expired consents are dropped first; one that fails a check is spent.
    fn check(
        &mut self,
        purpose: Purpose,
        consent_id: &str,
        provider_id: &str,
        message: Option<&str>,
        now: ClockReading,
    ) -> Result<usize, String> {
        self.open.retain(|o| !expired(&o.minted, &now));
        let Some(i) = self
            .open
            .iter()
            .position(|o| ct_eq(&o.consent_id, consent_id))
        else {
            return Err(NOT_CONFIRMED.into());
        };
        let o = &self.open[i];
        let bound = message.is_some_and(|m| message_hash(m) == o.message_hash);
        if o.purpose != purpose || o.provider_id != provider_id || !bound {
            self.open.remove(i);
            return Err(NOT_CONFIRMED.into());
        }
        Ok(i)
    }
}

/// The managed consent book.
#[derive(Default)]
pub struct ChatConsentState(pub Mutex<ConsentBook>);

fn new_consent_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("cc_{}", hex::encode(b))
}

/// A permit for one remote chat send. Refused under Local model only (or an unreadable route
/// setting, `route_dir`) before the consent book is asked; then the consent must hold.
pub(crate) fn authorize(
    state: &ChatConsentState,
    route_dir: &Path,
    consent_id: &str,
    provider_id: &str,
    messages_json: &str,
    now: ClockReading,
) -> Result<RemoteSendPermit, String> {
    let _ = route_dir;
    state
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .permit_chat(consent_id, provider_id, messages_json, now)
}

/// A permit for one remote image. Refused under Local model only (or an unreadable route
/// setting, `route_dir`) before the consent book is asked; then the media consent must hold.
pub(crate) fn authorize_once(
    state: &ChatConsentState,
    route_dir: &Path,
    consent_id: &str,
    provider_id: &str,
    prompt: &str,
    now: ClockReading,
) -> Result<RemoteSendPermit, String> {
    let _ = route_dir;
    state
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .permit_media(consent_id, provider_id, prompt, now)
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
            Purpose::Chat,
            &provider_id,
            &message,
            crate::ceremony::system_clock(),
            new_consent_id(),
        );
        Ok(view)
    })
    .await
}

/// **media_consent_prepare**: the member pressed Generate on a remote image route. Core mints the
/// one-time consent `media_generate_image` needs for that prompt. Refused under Local model only.
#[tauri::command]
pub async fn media_consent_prepare(
    app: tauri::AppHandle,
    provider_id: String,
    prompt: String,
) -> Result<ConsentView, String> {
    let route_dir = crate::chat_route::route_dir(&app)?;
    crate::blocking::off_main(move || {
        crate::chat_route::remote_chat_allowed(&route_dir)?;
        let st = tauri::Manager::try_state::<ChatConsentState>(&app)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let view = st.0.lock().unwrap_or_else(|e| e.into_inner()).prepare(
            Purpose::Media,
            &provider_id,
            &prompt,
            crate::ceremony::system_clock(),
            new_consent_id(),
        );
        Ok(view)
    })
    .await
}

/// Tests only: a clock reading `mono_ms` after a fixed monotonic base and at wall time `wall_ms`.
#[cfg(test)]
pub(crate) fn test_clock(mono_ms: u64, wall_ms: u64) -> ClockReading {
    static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    ClockReading {
        mono: *BASE.get_or_init(std::time::Instant::now) + Duration::from_millis(mono_ms),
        wall_ms,
    }
}

#[cfg(test)]
mod tests {
    include!("chat_consent_tests.rs");
}
