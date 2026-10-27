//! AMC-H1 (0.5.3, P-11): the write path into the conversation history, from the in-app chat and
//! from sidecar agent sessions, behind a feature gate that is **off** by default.
//!
//! ## The gate
//!
//! History is recorded only when `CITRATE_AMC_HISTORY=1` is set for the app process. Unset (every
//! 0.5.2 build and install), [`HistoryState`] is off: no store is opened, the OS keyring is not
//! touched, every hook below returns at once, and the `history_*` commands report "off". So
//! 0.5.2 behaviour is unchanged. The 0.5.3 agent management center replaces the variable with a
//! member setting.
//!
//! ## What is recorded, and from where
//!
//! - **Chat** (`ai_chat`, `ai_chat_local`): a conversation is started by `history_start_chat`
//!   (core resolves the provider's model from its sealed config; the webview names only the
//!   provider id). When the chat call carries that conversation id, core records the last member
//!   message of the request and the reply it returned. A call without one records nothing.
//! - **Agent sessions** (`hermes_session_open` / `_send` / `_events`): opening a session starts an
//!   agent conversation on the local model; each member message is recorded as it is sent; the
//!   event pages core relays are recorded once each, by sequence number: `final` → assistant turn,
//!   `tool_call`, `tool_result` (untrusted), `tainted` → a taint notice, after which assistant
//!   turns are marked tainted. `assistant_delta` (partial text) and `usage` (cost data, which the
//!   usage log owns) are never recorded.
//!
//! The recorder never derives a provider, model or endpoint from recorded content: the route is
//! what core resolved when the conversation started.
//!
//! Recording is best effort and content-free in its failures: a history error is logged without
//! any content and never fails the chat or the session.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::history::{
    clean_title, HistoryError, HistoryStore, NewConversation, Record, Role, Source, HISTORY_DIR,
};

/// The process environment variable that turns history recording on (`1`). Off otherwise.
pub const ENABLE_ENV: &str = "CITRATE_AMC_HISTORY";
/// What the history commands say while the gate is off.
pub const OFF_MESSAGE: &str = "conversation history is not turned on in this build";
/// The provider id agent sessions run on (the local model through the sidecar).
pub const LOCAL_PROVIDER: &str = "local";
/// How much of the first member message becomes a conversation's title.
const TITLE_CHARS: usize = 80;
/// The title a conversation has until its first member message names it.
const UNTITLED: &str = "New conversation";
/// How often retention is enforced while the app runs.
pub const RETENTION_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Whether the gate value turns history on.
pub fn enabled_value(v: Option<&str>) -> bool {
    v == Some("1")
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One sidecar session being recorded.
struct Track {
    conversation: String,
    last_seq: Option<u64>,
    tainted: bool,
    titled: bool,
}

/// The recorder over an opened store. An open failure is kept so status can report it.
pub struct Recorder {
    store: std::result::Result<HistoryStore, HistoryError>,
    sessions: Mutex<HashMap<String, Track>>,
    /// Chat conversations still waiting for their first member message to give them a title.
    untitled: Mutex<std::collections::HashSet<String>>,
}

/// Managed state: `None` is the gate off (0.5.2 behaviour).
pub struct HistoryState(Option<Arc<Recorder>>);

impl HistoryState {
    pub fn off() -> Self {
        HistoryState(None)
    }

    /// On only when [`ENABLE_ENV`] is `1`; then the store under `<app_data>/history` is opened
    /// with the OS keyring.
    pub fn from_env(app_data: &Path) -> Self {
        if !enabled_value(std::env::var(ENABLE_ENV).ok().as_deref()) {
            return Self::off();
        }
        Self::with_store(HistoryStore::open(
            app_data.join(HISTORY_DIR),
            Arc::new(crate::ai::OsAiKeyring),
        ))
    }

    pub fn with_store(store: std::result::Result<HistoryStore, HistoryError>) -> Self {
        HistoryState(Some(Arc::new(Recorder {
            store,
            sessions: Mutex::new(HashMap::new()),
            untitled: Mutex::new(Default::default()),
        })))
    }

    pub fn enabled(&self) -> bool {
        self.0.is_some()
    }

    pub fn recorder(&self) -> Option<&Arc<Recorder>> {
        self.0.as_ref()
    }

    /// The open store, or why there is none (off, or it failed to open).
    pub fn store(&self) -> std::result::Result<&HistoryStore, String> {
        match &self.0 {
            None => Err(OFF_MESSAGE.into()),
            Some(r) => r.store.as_ref().map_err(|e| e.to_string()),
        }
    }
}

fn first_title(text: &str) -> Option<String> {
    let cut: String = text.chars().take(TITLE_CHARS).collect();
    clean_title(&cut).ok()
}

/// The text of the last member message in an OpenAI-style `messages` array.
pub fn last_user_text(messages_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(messages_json).ok()?;
    v.as_array()?
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))?
        .get("content")?
        .as_str()
        .map(str::to_string)
}

impl Recorder {
    fn store(&self) -> std::result::Result<&HistoryStore, HistoryError> {
        self.store.as_ref().map_err(Clone::clone)
    }

    fn lock_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, Track>> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Start a chat conversation on `provider`/`model` (resolved by core).
    pub fn start_chat(
        &self,
        provider: &str,
        model: &str,
        title: Option<&str>,
        now: u64,
    ) -> std::result::Result<String, HistoryError> {
        let named = title.and_then(|t| clean_title(t).ok());
        let id = self.store()?.create(
            NewConversation {
                title: named.as_deref().unwrap_or(UNTITLED),
                source: Source::Chat,
                provider,
                model,
            },
            now,
        )?;
        if named.is_none() {
            self.untitled
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id.clone());
        }
        Ok(id)
    }

    /// Record one chat exchange: the request's last member message and the reply. `route` is the
    /// provider and model core resolved for this call; when it differs from the conversation's,
    /// the change is recorded first.
    pub fn record_chat_exchange(
        &self,
        conversation: &str,
        route: Option<(&str, &str)>,
        messages_json: &str,
        reply: &str,
        usage_call: Option<&str>,
        now: u64,
    ) -> std::result::Result<(), HistoryError> {
        let store = self.store()?;
        if let Some((provider, model)) = route {
            let s = store.summary(conversation)?;
            if s.provider != provider || s.model != model {
                store.set_route(conversation, provider, model, now)?;
            }
        }
        if let Some(text) = last_user_text(messages_json) {
            let untitled = self
                .untitled
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(conversation);
            if untitled {
                if let Some(t) = first_title(&text) {
                    store.rename(conversation, &t)?;
                }
            }
            store.append(
                conversation,
                Record::Turn {
                    role: Role::User,
                    text,
                    usage_call: None,
                    tainted: false,
                },
                now,
            )?;
        }
        store.append(
            conversation,
            Record::Turn {
                role: Role::Assistant,
                text: reply.to_string(),
                usage_call: usage_call.map(str::to_string),
                tainted: false,
            },
            now,
        )?;
        Ok(())
    }

    /// Start recording a sidecar session on the local model.
    pub fn open_agent_session(
        &self,
        session: &str,
        model: &str,
        now: u64,
    ) -> std::result::Result<String, HistoryError> {
        let id = self.store()?.create(
            NewConversation {
                title: UNTITLED,
                source: Source::Agent,
                provider: LOCAL_PROVIDER,
                model,
            },
            now,
        )?;
        self.lock_sessions().insert(
            session.to_string(),
            Track {
                conversation: id.clone(),
                last_seq: None,
                tainted: false,
                titled: false,
            },
        );
        Ok(id)
    }

    /// Record a member message sent to a recorded session. An unrecorded session is ignored.
    pub fn agent_user_turn(
        &self,
        session: &str,
        text: &str,
        now: u64,
    ) -> std::result::Result<(), HistoryError> {
        let store = self.store()?;
        let mut sessions = self.lock_sessions();
        let Some(t) = sessions.get_mut(session) else {
            return Ok(());
        };
        if !t.titled {
            if let Some(title) = first_title(text) {
                store.rename(&t.conversation, &title)?;
            }
            t.titled = true;
        }
        store.append(
            &t.conversation,
            Record::Turn {
                role: Role::User,
                text: text.to_string(),
                usage_call: None,
                tainted: false,
            },
            now,
        )?;
        Ok(())
    }

    /// Record the events of one relayed page, each sequence number once.
    pub fn agent_events(
        &self,
        session: &str,
        page: &Value,
        now: u64,
    ) -> std::result::Result<usize, HistoryError> {
        let store = self.store()?;
        let mut sessions = self.lock_sessions();
        let Some(t) = sessions.get_mut(session) else {
            return Ok(0);
        };
        let mut written = 0;
        let events = page.get("events").and_then(Value::as_array);
        for env in events.into_iter().flatten() {
            let (Some(seq), Some(ev)) = (env.get("seq").and_then(Value::as_u64), env.get("event"))
            else {
                continue;
            };
            if t.last_seq.is_some_and(|l| seq <= l) {
                continue;
            }
            let s = |k: &str| ev.get(k).and_then(Value::as_str).map(str::to_string);
            let record = match ev.get("type").and_then(Value::as_str) {
                Some("final") => s("content").map(|text| Record::Turn {
                    role: Role::Assistant,
                    text,
                    usage_call: None,
                    tainted: t.tainted,
                }),
                Some("tool_call") => ev.get("call").and_then(|c| {
                    Some(Record::ToolCall {
                        call_id: c.get("id")?.as_str()?.to_string(),
                        name: c.get("name")?.as_str()?.to_string(),
                        arguments: c.get("arguments")?.as_str()?.to_string(),
                    })
                }),
                Some("tool_result") => match (s("call_id"), s("status"), s("content")) {
                    (Some(call_id), Some(status), Some(content)) => Some(Record::ToolResult {
                        call_id,
                        status,
                        content,
                    }),
                    _ => None,
                },
                Some("tainted") => s("source").map(|source| {
                    t.tainted = true;
                    Record::Taint { source }
                }),
                _ => None,
            };
            if let Some(r) = record {
                store.append(&t.conversation, r, now)?;
                written += 1;
            }
            t.last_seq = Some(seq);
        }
        Ok(written)
    }

    /// Stop tracking a closed session (its conversation stays in the history).
    pub fn close_agent_session(&self, session: &str) {
        self.lock_sessions().remove(session);
    }
}

fn log_failure(what: &str, e: &HistoryError) {
    // The error kind only; never content.
    let kind = match e {
        HistoryError::Keyring => "keyring",
        HistoryError::Unreadable(_) => "unreadable",
        HistoryError::Tampered(_) => "tampered",
        HistoryError::NotFound => "not found",
        HistoryError::Invalid(_) => "invalid",
        HistoryError::Io(_) => "io",
    };
    eprintln!("citrate-core: conversation history did not record {what} ({kind})");
}

fn recorder_of(app: &tauri::AppHandle) -> Option<Arc<Recorder>> {
    tauri::Manager::try_state::<HistoryState>(app).and_then(|s| s.recorder().cloned())
}

// ---------------------------------------------------------------------------
// Hooks (each returns at once while the gate is off)
// ---------------------------------------------------------------------------

/// The model core routes `provider_id` to: the served local model for [`LOCAL_PROVIDER`], else
/// the model in that provider's sealed config. `None` when it cannot be resolved.
pub fn resolve_model(app: &tauri::AppHandle, provider_id: &str) -> Option<String> {
    if provider_id == LOCAL_PROVIDER {
        let serve = tauri::Manager::try_state::<crate::serve::ServeState>(app)?;
        return Some(serve.0.current_model_file());
    }
    let ai = tauri::Manager::try_state::<crate::ai::AiState>(app)?;
    crate::ai::ai_provider_status_sync(ai)
        .ok()?
        .into_iter()
        .find(|p| p.id == provider_id && p.configured)
        .map(|p| p.model)
}

/// After a chat reply on `provider_id`: record the exchange when the call named a conversation.
pub fn on_chat_reply(
    app: &tauri::AppHandle,
    conversation: Option<&str>,
    provider_id: &str,
    messages_json: &str,
    reply: &str,
) {
    let (Some(r), Some(c)) = (recorder_of(app), conversation) else {
        return;
    };
    let model = resolve_model(app, provider_id);
    let route = model.as_deref().map(|m| (provider_id, m));
    if let Err(e) = r.record_chat_exchange(c, route, messages_json, reply, None, now_ms()) {
        log_failure("a chat exchange", &e);
    }
}

/// After a sidecar session opened on the local model.
pub fn on_session_open(app: &tauri::AppHandle, session: &str, model: &str) {
    let Some(r) = recorder_of(app) else {
        return;
    };
    if let Err(e) = r.open_agent_session(session, model, now_ms()) {
        log_failure("an agent session", &e);
    }
}

/// After a member message was sent to a sidecar session.
pub fn on_session_send(app: &tauri::AppHandle, session: &str, text: &str) {
    let Some(r) = recorder_of(app) else {
        return;
    };
    if let Err(e) = r.agent_user_turn(session, text, now_ms()) {
        log_failure("a member message", &e);
    }
}

/// After an event page was read from a sidecar session.
pub fn on_session_events(app: &tauri::AppHandle, session: &str, page: &Value) {
    let Some(r) = recorder_of(app) else {
        return;
    };
    if let Err(e) = r.agent_events(session, page, now_ms()) {
        log_failure("agent events", &e);
    }
}

/// After a sidecar session closed.
pub fn on_session_close(app: &tauri::AppHandle, session: &str) {
    if let Some(r) = recorder_of(app) {
        r.close_agent_session(session);
    }
}

/// Enforce retention now and then every [`RETENTION_INTERVAL`] on a background thread. Not
/// started while the gate is off.
pub fn spawn_retention(app: tauri::AppHandle) {
    let Some(r) = recorder_of(&app) else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("history-retention".into())
        .spawn(move || loop {
            if let Ok(store) = r.store() {
                if let Err(e) = store.enforce_retention(now_ms()) {
                    log_failure("a retention purge", &e);
                }
            }
            std::thread::sleep(RETENTION_INTERVAL);
        });
    if spawned.is_err() {
        eprintln!("citrate-core: the conversation history retention thread did not start");
    }
}

#[cfg(test)]
mod tests {
    include!("history_recorder_tests.rs");
}
