//! The pending v2 invite (GROW-S1 task 12): a `citrate.ai/join/<code>#K` link that reached this app
//! (deep link or paste) and is waiting for the member to act on it.
//!
//! The link key `K` is a bearer secret, so the pending invite is held ONLY here, in Rust: in the OS
//! keyring (the same device-sealed store the comms key uses, its own account), or in this process's
//! memory when the keyring is unreachable (reported as `persisted: false`, so the UI can say it will
//! not survive a restart). It is never written to the webview's localStorage or state file, and the
//! view that crosses `invoke` carries neither the code nor the key.
//!
//! Lifecycle: `pending_join_set` on arrival (no network call: nothing is resolved until the member
//! acts) → `pending_join_resolve` when the member clicks Review → `join_redeem` on their explicit
//! Join, which wipes it. It is also wiped on `pending_join_clear` (cancel), on expiry (checked on
//! every read) and when the service reports the code gone.
//!
//! Data sources (Rule 7): the OS keyring (account [`PENDING_JOIN_ACCOUNT`]); resolve goes to the
//! citrate.ai join service through [`crate::join::JoinState`].
//!
//! This module never logs: no code, key, link or fragment is ever printed.

use citrate_core_kit::custody::Keyring;
use citrate_core_kit::join::{self, JoinError, ResolvedView};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Manager;
use zeroize::{Zeroize, Zeroizing};

/// Keyring account for the one pending invite.
pub(crate) const PENDING_JOIN_ACCOUNT: &str = "join-pending-v1";
/// Longest hold: a direct invite's relay TTL (14 days). The signed expiry, once resolved, can only
/// shorten it ([`PendingJoinStore::cap_expiry`]).
pub(crate) const PENDING_JOIN_TTL_SECS: u64 = 14 * 24 * 3600;
/// Upper bound on an accepted link, before parsing.
const MAX_LINK_LEN: usize = 2048;

/// What is sealed in the keyring slot: the canonical `<code>[#K]` and the hold window.
#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Stored {
    link: String,
    stored_at: u64,
    expires_at: u64,
}

impl Drop for Stored {
    fn drop(&mut self) {
        self.link.zeroize();
    }
}

impl std::fmt::Debug for Stored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stored")
            .field("link", &"<redacted>")
            .field("stored_at", &self.stored_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl Stored {
    fn code(&self) -> &str {
        self.link.split('#').next().unwrap_or("")
    }
}

/// The webview's view of the pending invite: whether a key came with it and the hold window. No
/// code, no key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJoinView {
    pub has_key: bool,
    pub stored_at: u64,
    pub expires_at: u64,
    /// false = the keyring was unreachable and the invite is held in memory only.
    pub persisted: bool,
}

/// The pending-invite store over a keyring seam and a clock (both injected for tests).
pub struct PendingJoinStore {
    keyring: Box<dyn Keyring>,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    /// Fallback only while the keyring is unreachable.
    memory: Mutex<Option<Stored>>,
}

fn view(s: &Stored, persisted: bool) -> PendingJoinView {
    PendingJoinView {
        has_key: s.link.contains('#'),
        stored_at: s.stored_at,
        expires_at: s.expires_at,
        persisted,
    }
}

impl PendingJoinStore {
    pub fn new(keyring: Box<dyn Keyring>, now: Box<dyn Fn() -> u64 + Send + Sync>) -> Self {
        PendingJoinStore {
            keyring,
            now,
            memory: Mutex::new(None),
        }
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, Option<Stored>> {
        self.memory.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self, s: Stored) -> bool {
        let bytes = match serde_json::to_vec(&s) {
            Ok(b) => Zeroizing::new(b),
            Err(_) => {
                *self.memory() = Some(s);
                return false;
            }
        };
        match self.keyring.set(PENDING_JOIN_ACCOUNT, &bytes) {
            Ok(()) => {
                *self.memory() = None;
                true
            }
            Err(_) => {
                *self.memory() = Some(s);
                false
            }
        }
    }

    /// Validate and hold an invite link. Replaces any earlier one. No network call.
    pub fn set(&self, link: &str) -> Result<PendingJoinView, String> {
        let link = link.trim();
        if link.len() > MAX_LINK_LEN {
            return Err(JoinError::Invalid.to_string());
        }
        let (code, key) = join::parse_link(link).map_err(|e| e.to_string())?;
        let canonical = match &key {
            Some(k) => format!("{code}#{}", k.to_fragment().as_str()),
            None => code,
        };
        let now = (self.now)();
        let s = Stored {
            link: canonical,
            stored_at: now,
            expires_at: now.saturating_add(PENDING_JOIN_TTL_SECS),
        };
        let persisted = self.write(s.clone());
        Ok(view(&s, persisted))
    }

    /// The held invite, if any and not expired. An expired or unreadable one is wiped.
    pub(crate) fn load(&self) -> Option<(Stored, bool)> {
        let from_memory = self.memory().clone();
        let (s, persisted) = match from_memory {
            Some(s) => (s, false),
            None => match self.keyring.get(PENDING_JOIN_ACCOUNT) {
                Ok(Some(bytes)) => {
                    let bytes = Zeroizing::new(bytes);
                    match serde_json::from_slice::<Stored>(&bytes) {
                        Ok(s) if join::parse_link(&s.link).is_ok() => (s, true),
                        _ => {
                            let _ = self.keyring.delete(PENDING_JOIN_ACCOUNT);
                            return None;
                        }
                    }
                }
                Ok(None) | Err(_) => return None,
            },
        };
        if (self.now)() >= s.expires_at {
            let _ = self.clear();
            return None;
        }
        Some((s, persisted))
    }

    /// The webview's view of the held invite.
    pub fn get(&self) -> Option<PendingJoinView> {
        self.load().map(|(s, p)| view(&s, p))
    }

    /// The held `<code>[#K]`, for resolve. Stays in Rust.
    pub(crate) fn link(&self) -> Option<Zeroizing<String>> {
        self.load().map(|(s, _)| Zeroizing::new(s.link.clone()))
    }

    /// Wipe the held invite (memory and keyring slot).
    pub fn clear(&self) -> Result<(), String> {
        *self.memory() = None;
        self.keyring
            .delete(PENDING_JOIN_ACCOUNT)
            .map_err(|e| e.to_string())
    }

    /// Wipe the held invite only if it is for `code` (after that code was redeemed or found gone).
    pub fn forget_code(&self, code: &str) {
        if let Some((s, _)) = self.load() {
            if s.code() == code {
                let _ = self.clear();
            }
        }
    }

    /// Shorten the hold to the invite's signed expiry (never lengthen it).
    pub fn cap_expiry(&self, exp: u64) {
        if let Some((mut s, _)) = self.load() {
            if exp < s.expires_at {
                s.expires_at = exp;
                self.write(s);
            }
        }
    }

    /// Resolve the held invite with `resolve` (the join service, injected so tests can drive it),
    /// then apply the answer to the hold: a signed expiry caps it, and a code the service reports
    /// gone is wiped. The network call runs without holding anything locked.
    pub(crate) fn resolve_held(
        &self,
        resolve: impl FnOnce(&str) -> Result<ResolvedView, JoinError>,
    ) -> Result<ResolvedView, String> {
        let link = self
            .link()
            .ok_or_else(|| "join: no invite is waiting".to_string())?;
        match resolve(&link) {
            Ok(v) => {
                if let Some(exp) = join::parse_iso_utc_secs(&v.exp) {
                    self.cap_expiry(exp);
                }
                Ok(v)
            }
            Err(e) => {
                if e == JoinError::NotFound {
                    let _ = self.clear();
                }
                Err(e.to_string())
            }
        }
    }
}

/// Managed state: the process-wide pending-invite store.
pub struct PendingJoinState(pub PendingJoinStore);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Production store: the OS keyring namespace the comms key lives in, and the wall clock.
pub fn build_pending_join_state() -> PendingJoinState {
    PendingJoinState(PendingJoinStore::new(
        Box::new(crate::custody::OsKeyring::legacy()),
        Box::new(now_secs),
    ))
}

fn with_store<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&PendingJoinStore) -> Result<T, String>,
) -> Result<T, String> {
    let st = app
        .try_state::<PendingJoinState>()
        .ok_or_else(|| "internal: managed state unavailable".to_string())?;
    f(&st.0)
}

/// Wipe the pending invite for `code` (called by `join_redeem` after a successful Join).
pub(crate) fn forget_code(app: &tauri::AppHandle, code: &str) {
    let _ = with_store(app, |s| {
        s.forget_code(code);
        Ok(())
    });
}

/// `pending_join_set` — hold an invite link that arrived by deep link or paste. Not resolved.
#[tauri::command]
pub async fn pending_join_set(
    app: tauri::AppHandle,
    link: String,
) -> Result<PendingJoinView, String> {
    let link = Zeroizing::new(link);
    crate::blocking::off_main(move || with_store(&app, |s| s.set(&link))).await
}

/// `pending_join_get` — whether an invite is waiting (no code, no key).
#[tauri::command]
pub async fn pending_join_get(app: tauri::AppHandle) -> Result<Option<PendingJoinView>, String> {
    crate::blocking::off_main(move || with_store(&app, |s| Ok(s.get()))).await
}

/// `pending_join_clear` — the member cancelled: wipe it.
#[tauri::command]
pub async fn pending_join_clear(app: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || with_store(&app, |s| s.clear())).await
}

/// `pending_join_resolve` — the member chose to look at the waiting invite: resolve and verify it
/// (and, for a direct invite with its key, open it and hold it for `join_redeem`). Counts against
/// the code's resolve limit, which is why it runs only on the member's click. A code the service
/// reports gone is wiped.
#[tauri::command]
pub async fn pending_join_resolve(app: tauri::AppHandle) -> Result<ResolvedView, String> {
    crate::blocking::off_main(move || {
        let join_state = app
            .try_state::<crate::join::JoinState>()
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        with_store(&app, |s| s.resolve_held(|link| join_state.0.resolve(link)))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("pending_join_tests.rs");
}
