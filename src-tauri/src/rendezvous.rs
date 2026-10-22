//! GROW-S2 (Join 9) — the relay rendezvous join, app side.
//!
//! The member daemon does the rendezvous: the inviter's daemon opens it when an invite is minted,
//! the invitee's daemon asks to join, and the inviter's daemon admits the request the next time it
//! runs. This module is the app's half: the daemon IPC views (`JoinView`, `InviteView`), the
//! derivations the app needs from the relay token, and the tick that keeps both sides moving while
//! the app is open.
//!
//! Contract: citrate-federation `.agentile/planset/2026-10-19-betanet/JOIN_GROW_S2_RELAY.md`
//! (rev 8). §3 derivations, §9 IPC, §11 user-visible states, §12 fallback, §13 vectors.
//!
//! Data sources (Rule 7): every view comes from the member daemon over its loopback socket
//! (`rendezvousTick`, `rendezvousJoin`, `rendezvousCancel`); the daemon reads the comms relay. The
//! app never invents a state: when the daemon can't answer, nothing is emitted and the UI keeps the
//! last view it had.
//!
//! What crosses `invoke` and the `join://state` event: the two view types only. They carry group
//! ids, a display name, the rendezvous `lookup` (the relay's own index, not a secret), state words,
//! a shortened-on-screen joiner address and times. Never the relay token `t`, `auth`, `mac_key` or
//! any key material.

use serde::{Deserialize, Serialize};

/// The webview event carrying the latest rendezvous views (`TickViews`).
pub const JOIN_STATE_EVENT: &str = "join://state";

/// How often the app ticks while it is open (§6, §15 M4: the comms poll cadence, ≤ 30 s).
pub const TICK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// Delay before the first tick after app start, so the tick doesn't race the rest of setup.
const FIRST_TICK_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

// ---------------------------------------------------------------------------
// §3 derivations (the subset the app needs). Input `T` is the UTF-8 bytes of the token string as it
// appears in the link's `t=` parameter; never base64-decoded first.
// ---------------------------------------------------------------------------

/// BLAKE3 `derive_key` context for `auth` (§3). Fixed by the contract.
const AUTH_CONTEXT: &str = "citrate-comms 2026-10-19 rendezvous v1 auth";

/// The IPC `token` field: lowercase hex of the token's UTF-8 bytes (§9, vector `token_utf8_hex`).
pub(crate) fn token_ipc_hex(token: &str) -> String {
    hex::encode(token.as_bytes())
}

/// `auth = BLAKE3.derive_key(AUTH_CONTEXT, T)`. Kept in this process only, to compute `lookup`.
fn auth(token: &str) -> [u8; 32] {
    blake3::derive_key(AUTH_CONTEXT, token.as_bytes())
}

/// `lookup = BLAKE3(auth)`, lowercase hex. The relay's index for the rendezvous; the inviter's
/// daemon returns it from `rendezvousOpen`, and the app checks it matches before recording it.
pub(crate) fn lookup_hex(token: &str) -> String {
    hex::encode(blake3::hash(&auth(token)).as_bytes())
}

// ---------------------------------------------------------------------------
// §9 views. Deserialized from the daemon (snake_case fields), serialized to the webview (camelCase,
// like every other DTO in this app).
// ---------------------------------------------------------------------------

/// Invitee join state (§9 `JoinView.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinState {
    Requesting,
    Waiting,
    Joining,
    Joined,
    Expired,
    Revoked,
    Taken,
    Dropped,
    Withdrawn,
    Unknown,
    WelcomeExpired,
    ReplyUnverified,
    RelayUnreachable,
    RelayUnsupported,
    Failed,
}

/// The invitee's view of one join (§9 `JoinView`). `reason` is the daemon's stable snake_case code
/// (e.g. `no_rendezvous`, `purged`, `already_delivered`, `bad_binding`); the set grows with the
/// contract, so it stays a string and the UI maps the codes it knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct JoinView {
    pub group: String,
    pub name: String,
    pub state: JoinState,
    #[serde(default)]
    pub reason: Option<String>,
    /// The relay's request deadline; `null` until the relay answered.
    #[serde(default)]
    pub expires_at: Option<u64>,
    /// The last moment the inviter can still admit; `null` until answered.
    #[serde(default)]
    pub grace_until: Option<u64>,
    /// The last time the daemon got a definite answer from the relay. Nullable here because a
    /// `requesting` join may not have had one yet.
    #[serde(default)]
    pub updated_at: Option<u64>,
}

/// Inviter state for one rendezvous it opened (§9 `InviteView.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteState {
    Open,
    Adding,
    Added,
    PickedUp,
    WelcomeExpired,
    Expired,
    Revoked,
    Failed,
    RelayUnreachable,
    Unknown,
}

/// The inviter's view of one rendezvous (§9 `InviteView`). `joiner` is the full `0x` + 40 hex
/// address; the UI shortens it and never resolves it to a handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct InviteView {
    pub lookup: String,
    pub group: String,
    pub state: InviteState,
    #[serde(default)]
    pub joiner: Option<String>,
    pub expires_at: u64,
    /// Nullable for an open the relay hasn't confirmed yet.
    #[serde(default)]
    pub updated_at: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
    /// Requests the inviter's device rejected (diagnostics only, never shown as requests).
    #[serde(default)]
    pub dropped: u32,
}

/// The answer to `rendezvousTick`, and the `join://state` event payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TickViews {
    pub joins: Vec<JoinView>,
    pub invites: Vec<InviteView>,
}

// ---------------------------------------------------------------------------
// Daemon answers.
// ---------------------------------------------------------------------------

/// Why a rendezvous IPC call didn't produce a view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RvError {
    /// The daemon doesn't know the op (a build from before the rendezvous ops; the sidecar pin bump
    /// is task M6). Callers treat this as "rendezvous not available here", never as a join state.
    DaemonUnsupported,
    /// The daemon answered `{"type":"error"}` with this message.
    Daemon(String),
    /// The loopback IPC itself failed (daemon not started, socket, timeout).
    Ipc(String),
}

impl std::fmt::Display for RvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RvError::DaemonUnsupported => {
                f.write_str("this app's comms daemon doesn't support invite rendezvous yet")
            }
            RvError::Daemon(m) => f.write_str(m),
            RvError::Ipc(m) => f.write_str(m),
        }
    }
}

/// Classify a daemon `{"type":"error","message":…}`. The daemon parses every request line as its
/// `Request` enum and answers a parse failure with `bad request: <serde error>`; for an op it
/// doesn't have, serde's error names the unknown variant. That (and only that) is
/// [`RvError::DaemonUnsupported`].
pub(crate) fn classify_daemon_error(message: &str) -> RvError {
    let m = message.trim();
    if m.starts_with("bad request:") && m.contains("unknown variant `rendezvous") {
        RvError::DaemonUnsupported
    } else {
        RvError::Daemon(m.to_string())
    }
}

/// True when a daemon error says the RELAY doesn't speak rendezvous (§12: the daemon maps a
/// `malformed frame` answer on a Rendezvous op to `relay_unsupported`). §9 gives only the success
/// shape for `rendezvousOpen`, so the stable code is accepted as the error message.
pub(crate) fn is_relay_unsupported(err: &RvError) -> bool {
    matches!(err, RvError::Daemon(m) if m.trim() == "relay_unsupported")
}

// ---------------------------------------------------------------------------
// M4 — the tick.
// ---------------------------------------------------------------------------

/// Whether a scheduled tick should run. A tick starts the comms daemon if it isn't running, so the
/// scheduled tick only does that when this device has rendezvous work: invites it minted, or joins
/// it asked for. Once the daemon is running (any comms use this session), every tick runs.
pub(crate) fn should_tick(daemon_running: bool, local_work: bool) -> bool {
    daemon_running || local_work
}

/// Non-terminal invitee states: the daemon still has something to do for these joins (§5.1).
pub(crate) fn join_needs_ticks(state: JoinState) -> bool {
    matches!(
        state,
        JoinState::Requesting
            | JoinState::Waiting
            | JoinState::Joining
            | JoinState::ReplyUnverified
            | JoinState::RelayUnreachable
    )
}

/// Run one tick: ask the daemon (`rendezvousTick`), then emit the views to the webview. Returns the
/// views. On any error nothing is emitted, so the UI keeps the last views it had (§11: a relay or
/// daemon hiccup never changes a shown state).
pub(crate) fn tick_and_emit(app: &tauri::AppHandle) -> Result<TickViews, RvError> {
    use tauri::Emitter;
    let views = crate::comms::rendezvous_tick(app)?;
    record_active_joins(app, &views);
    let _ = app.emit(JOIN_STATE_EVENT, &views);
    Ok(views)
}

/// The local-work marker: group ids of joins that still need ticks (no secrets; ids only), so the
/// app-start tick can tell there is something to resume after a restart.
fn active_joins_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().ok()?.join("invites");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("rendezvous-active.json"))
}

pub(crate) fn active_join_groups(views: &TickViews) -> Vec<String> {
    let mut groups: Vec<String> = views
        .joins
        .iter()
        .filter(|j| join_needs_ticks(j.state))
        .map(|j| j.group.clone())
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

fn record_active_joins(app: &tauri::AppHandle, views: &TickViews) {
    if let Some(path) = active_joins_path(app) {
        if let Ok(bytes) = serde_json::to_vec(&active_join_groups(views)) {
            let _ = citrate_core_kit::fsutil::write_secret_file(&path, &bytes);
        }
    }
}

/// Note that this device asked to join `group` (M3 calls this once the daemon has the join), so a
/// restart resumes ticking even before the next tick wrote the marker.
#[allow(dead_code)] // called by the invitee join path (M3)
pub(crate) fn note_active_join(app: &tauri::AppHandle, group: &str) {
    let Some(path) = active_joins_path(app) else {
        return;
    };
    let mut groups: Vec<String> = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if !groups.iter().any(|g| g == group) {
        groups.push(group.to_string());
        groups.sort();
    }
    if let Ok(bytes) = serde_json::to_vec(&groups) {
        let _ = citrate_core_kit::fsutil::write_secret_file(&path, &bytes);
    }
}

fn has_local_work(app: &tauri::AppHandle) -> bool {
    let joins: Vec<String> = active_joins_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    local_work(
        &joins,
        &crate::invites::load(app),
        crate::invites::now_unix(),
    )
}

/// Whether this device has rendezvous work: a join still in progress, or an invite it minted.
pub(crate) fn local_work(
    active_joins: &[String],
    invites: &[crate::invites::PendingInvite],
    _now_unix: u64,
) -> bool {
    !active_joins.is_empty() || !invites.is_empty()
}

/// Whether the scheduled tick keeps running after this tick's result.
pub(crate) fn scheduled_tick_continues(_tick: &Result<TickViews, RvError>) -> bool {
    true
}

/// Start the background tick: once shortly after app start, then every [`TICK_INTERVAL`] while the
/// app runs. Errors are quiet (the daemon may not be startable before sign-in); nothing is emitted
/// for a failed tick.
pub fn spawn_ticker(app: tauri::AppHandle) {
    let _ = std::thread::Builder::new()
        .name("rendezvous-tick".into())
        .spawn(move || {
            std::thread::sleep(FIRST_TICK_DELAY);
            loop {
                if should_tick(crate::comms::daemon_running(), has_local_work(&app))
                    && !scheduled_tick_continues(&tick_and_emit(&app))
                {
                    return;
                }
                std::thread::sleep(TICK_INTERVAL);
            }
        });
}

// ---------------------------------------------------------------------------
// Commands.
// ---------------------------------------------------------------------------

/// `rendezvous_tick` — run one tick now (the Retry on "Couldn't check your invite", and the
/// Groups screen on open). Returns the views and emits them on `join://state`.
#[tauri::command]
pub async fn rendezvous_tick(app: tauri::AppHandle) -> Result<TickViews, String> {
    crate::blocking::off_main(move || tick_and_emit(&app).map_err(|e| e.to_string())).await
}

/// `rendezvous_cancel` — the invitee withdraws a `requesting` or `waiting` join (§9). Returns the
/// view after the attempt: `withdrawn`, still `waiting` / `requesting` while unconfirmed, or the
/// state the relay had already moved to.
#[tauri::command]
pub async fn rendezvous_cancel(app: tauri::AppHandle, group: String) -> Result<JoinView, String> {
    crate::blocking::off_main(move || {
        let view = crate::comms::rendezvous_cancel(&app, group).map_err(|e| e.to_string())?;
        let _ = tick_and_emit(&app);
        Ok(view)
    })
    .await
}

/// `rendezvous_forget` — clear a terminal join from the invitee's list (§9).
#[tauri::command]
pub async fn rendezvous_forget(app: tauri::AppHandle, group: String) -> Result<(), String> {
    crate::blocking::off_main(move || {
        crate::comms::rendezvous_forget(&app, group).map_err(|e| e.to_string())?;
        let _ = tick_and_emit(&app);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("rendezvous_tests.rs");
}
