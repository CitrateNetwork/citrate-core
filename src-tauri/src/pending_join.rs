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
/// shorten it ([`PendingJoinStore::resolve_held`]).
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
    /// Every read and write of the hold runs under this lock, so a set, a clear and the result of
    /// a resolve never interleave half-way. The resolve's network call runs outside it.
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Fallback only while the keyring is unreachable.
    memory: Option<Stored>,
    /// Bumped by every set and every wipe. A resolve applies its answer only if the generation it
    /// started on is still current, so it never acts on an invite that replaced the one it checked.
    generation: u64,
    /// The member cancelled but the keyring slot could not be deleted (keyring unreachable). Reads
    /// ignore the slot and retry the delete until it succeeds, so the invite does not come back.
    unwiped_slot: bool,
}

fn view(s: &Stored, persisted: bool) -> PendingJoinView {
    PendingJoinView {
        has_key: s.link.contains('#'),
        stored_at: s.stored_at,
        expires_at: s.expires_at,
        persisted,
    }
}

/// The error a resolve returns when the invite changed (replaced or cancelled) while it was out.
const CHANGED: &str =
    "join: the waiting invite changed while it was being checked, review it again";

impl PendingJoinStore {
    pub fn new(keyring: Box<dyn Keyring>, now: Box<dyn Fn() -> u64 + Send + Sync>) -> Self {
        PendingJoinStore {
            keyring,
            now,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self, g: &mut Inner, s: Stored) -> bool {
        let bytes = match serde_json::to_vec(&s) {
            Ok(b) => Zeroizing::new(b),
            Err(_) => {
                g.memory = Some(s);
                return false;
            }
        };
        match self.keyring.set(PENDING_JOIN_ACCOUNT, &bytes) {
            Ok(()) => {
                g.memory = None;
                g.unwiped_slot = false;
                true
            }
            Err(_) => {
                g.memory = Some(s);
                false
            }
        }
    }

    /// Wipe memory and the keyring slot. If the slot can't be deleted now, remember that so reads
    /// never trust it again and the delete is retried.
    fn wipe(&self, g: &mut Inner) -> Result<(), String> {
        g.memory = None;
        g.generation = g.generation.wrapping_add(1);
        match self.keyring.delete(PENDING_JOIN_ACCOUNT) {
            Ok(()) => {
                g.unwiped_slot = false;
                Ok(())
            }
            Err(e) => {
                g.unwiped_slot = true;
                Err(e.to_string())
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
        let mut g = self.lock();
        g.generation = g.generation.wrapping_add(1);
        let persisted = self.write(&mut g, s.clone());
        Ok(view(&s, persisted))
    }

    /// The held invite, if any and not expired. An expired or unreadable one is wiped.
    fn load(&self, g: &mut Inner) -> Option<(Stored, bool)> {
        let (s, persisted) = match g.memory.clone() {
            Some(s) => (s, false),
            None if g.unwiped_slot => {
                if self.keyring.delete(PENDING_JOIN_ACCOUNT).is_ok() {
                    g.unwiped_slot = false;
                }
                return None;
            }
            None => match self.keyring.get(PENDING_JOIN_ACCOUNT) {
                Ok(Some(bytes)) => {
                    let bytes = Zeroizing::new(bytes);
                    match serde_json::from_slice::<Stored>(&bytes) {
                        Ok(s) if join::parse_link(&s.link).is_ok() => (s, true),
                        _ => {
                            let _ = self.wipe(g);
                            return None;
                        }
                    }
                }
                Ok(None) | Err(_) => return None,
            },
        };
        if (self.now)() >= s.expires_at {
            let _ = self.wipe(g);
            return None;
        }
        Some((s, persisted))
    }

    /// The webview's view of the held invite.
    pub fn get(&self) -> Option<PendingJoinView> {
        let mut g = self.lock();
        self.load(&mut g).map(|(s, p)| view(&s, p))
    }

    /// The held `<code>[#K]`. Stays in Rust.
    #[cfg(test)]
    pub(crate) fn link(&self) -> Option<Zeroizing<String>> {
        let mut g = self.lock();
        self.load(&mut g)
            .map(|(s, _)| Zeroizing::new(s.link.clone()))
    }

    /// Wipe the held invite (memory and keyring slot). An unreachable keyring is reported, and the
    /// slot is still never read back.
    pub fn clear(&self) -> Result<(), String> {
        let mut g = self.lock();
        self.wipe(&mut g)
    }

    /// Wipe the held invite only if it is for `code` (after that code was redeemed or found gone).
    pub fn forget_code(&self, code: &str) {
        let mut g = self.lock();
        if let Some((s, _)) = self.load(&mut g) {
            if s.code() == code {
                let _ = self.wipe(&mut g);
            }
        }
    }

    /// Resolve the held invite with `resolve` (the join service, injected so tests can drive it),
    /// then apply the answer to the hold: a signed expiry caps it (never lengthens it), and a code
    /// the service reports gone is wiped. The network call runs without the lock. If the invite was
    /// replaced or cancelled meanwhile, the answer is about an invite no longer held: it is not
    /// applied and not returned.
    pub(crate) fn resolve_held(
        &self,
        resolve: impl FnOnce(&str) -> Result<ResolvedView, JoinError>,
    ) -> Result<ResolvedView, String> {
        let (link, started) = {
            let mut g = self.lock();
            let (s, _) = self
                .load(&mut g)
                .ok_or_else(|| "join: no invite is waiting".to_string())?;
            (Zeroizing::new(s.link.clone()), g.generation)
        };
        let out = resolve(&link);
        let mut g = self.lock();
        if g.generation != started {
            return Err(CHANGED.to_string());
        }
        match out {
            Ok(v) => {
                if let Some(exp) = join::parse_iso_utc_secs(&v.exp) {
                    if let Some((mut s, _)) = self.load(&mut g) {
                        if exp < s.expires_at {
                            s.expires_at = exp;
                            self.write(&mut g, s);
                        }
                    }
                }
                Ok(v)
            }
            Err(e) => {
                if e == JoinError::NotFound {
                    let _ = self.wipe(&mut g);
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
