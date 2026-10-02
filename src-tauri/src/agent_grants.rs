//! HUP-S2.1 — the member's folder grants for Hermes: store, commands, and delivery to the agent.
//!
//! The member grants Hermes a folder (read and write are separate grants) or turns on the
//! read-only full-access window (24 h, after a one-shot HIC-1 confirmation), and can revoke any
//! grant. Core is the only writer of the grant document. It lives in core's app data
//! (`<app data>/agent/agent-grants.json`, owner-only), which the agent's default-deny list never
//! lets an agent read or write, so the agent cannot change its own grants.
//!
//! The document is the `citrate-agent-grants` `GrantState` (version 1); the cross-repo contract
//! fixture is `tests/fixtures/agent-grants/core-grant-state-v1.json`. The sidecar is the
//! enforcement point: every agent file operation is checked there, at the moment of use, against
//! the document core last sent (with its default-deny list always winning). Core sends the
//! document when it opens an agent session and again after every change ([`push_after_change`]).
//!
//! Fail closed: a document that does not parse or breaks a grant rule is treated as **no grants**.
//! The Grants panel says so, nothing is written over it, and the member can set it aside and start
//! empty ([`GrantStore::reset_corrupted`]). The agent then receives an empty document.
//!
//! HUP-S2.6: every change (a folder granted or revoked, the full-access window confirmed, a reset)
//! is recorded in core's HIC outbox ([`crate::hic_records`]) and from there in the decision records
//! the nightly anchor covers. A change whose record cannot be written is put back and refused.
//!
//! Keyless: nothing here signs or holds a key (Rule 3).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// The grant document's file name inside the store directory.
pub const GRANTS_FILE: &str = "agent-grants.json";
/// The document format version (`citrate-agent-grants` `STATE_VERSION`).
pub const GRANTS_VERSION: u32 = 1;
// The two durations below and GRANTED_BY are conservative defaults, pending owner sign-off.
/// Full access lasts exactly this long once confirmed (D-15 amended: at most 24 h).
pub const FULL_ACCESS_SECS: u64 = 24 * 60 * 60;
/// How long a prepared full-access confirmation may be confirmed.
pub const CONFIRM_WINDOW_SECS: u64 = 120;

/// Who granted it. Local and member-facing; the grant never leaves this device except to the
/// local sidecar.
const GRANTED_BY: &str = "member";
const FOLDER_REASON: &str = "Granted in Settings";
const FULL_ACCESS_REASON: &str = "Read-only full access (24 h), confirmed in Settings";

/// Read or write. Separate on purpose: neither implies the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
}

/// A folder grant, or the read-only full-access window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantKind {
    Folder,
    FullAccess,
}

/// One stored grant. Field names and meaning match `citrate_agent_grants::Grant`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub id: String,
    pub kind: GrantKind,
    /// Absolute, symlink-resolved folder.
    pub root: String,
    pub access: Access,
    /// `subtree` (the folder and everything below it) or `shallow`. Core grants `subtree`.
    pub scope: String,
    /// Unix seconds.
    pub granted_at: u64,
    /// Unix seconds; nothing is allowed at or after it. `None` = until revoked (folders only).
    pub expires_at: Option<u64>,
    pub granted_by: String,
    pub reason: String,
    /// Unix seconds; set once.
    pub revoked_at: Option<u64>,
}

impl Grant {
    fn status(&self, now: u64) -> &'static str {
        if self.revoked_at.is_some() {
            "revoked"
        } else if now < self.granted_at {
            "not_yet_active"
        } else if self.expires_at.is_some_and(|t| now >= t) {
            "expired"
        } else {
            "active"
        }
    }
}

/// The grant document (`citrate_agent_grants::GrantState`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantState {
    pub version: u32,
    pub next_id: u64,
    pub grants: Vec<Grant>,
}

impl Default for GrantState {
    fn default() -> Self {
        GrantState {
            version: GRANTS_VERSION,
            next_id: 1,
            grants: Vec::new(),
        }
    }
}

/// The result of reading the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    Ok(GrantState),
    /// The file exists but is not a valid document. It grants nothing.
    Corrupted(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantsError {
    /// The stored document is corrupted; nothing is written over it until the member resets it.
    Corrupted(String),
    /// The request is not allowed (bad folder, unknown id, expired confirmation, ...).
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for GrantsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantsError::Corrupted(e) => write!(
                f,
                "the saved folder grants could not be read ({e}), so Hermes has no folder access; reset them in Settings to start again"
            ),
            GrantsError::Invalid(e) => f.write_str(e),
            GrantsError::Io(e) => write!(f, "could not save the folder grants: {e}"),
        }
    }
}

/// One row of the Grants panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRow {
    pub id: String,
    /// `folder` | `full_access`
    pub kind: String,
    pub root: String,
    /// `read` | `write`
    pub access: String,
    /// `active` | `expired` | `revoked` | `not_yet_active`
    pub status: String,
    pub granted_at: u64,
    pub expires_at: Option<u64>,
    /// Seconds until expiry (0 once expired); `None` = until revoked. The full-access countdown.
    pub remaining_secs: Option<u64>,
    pub reason: String,
}

/// What the Grants panel shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantsView {
    /// `ok` | `corrupted`
    pub status: &'static str,
    pub error: Option<String>,
    pub grants: Vec<GrantRow>,
    /// Remaining seconds of the live full-access window, if one is on.
    pub full_access_remaining_secs: Option<u64>,
    /// The folder full access would cover (the member's home).
    pub full_access_root: String,
    pub now: u64,
}

/// A prepared full-access confirmation (the HIC-1 step). Confirm it with its `id` before
/// `confirm_by`; it works once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullAccessConfirmation {
    pub id: String,
    pub root: String,
    /// What the member is agreeing to, shown verbatim.
    pub statement: String,
    pub prepared_at: u64,
    pub confirm_by: u64,
    /// When the window would end if confirmed now.
    pub grant_expires_at: u64,
}

#[derive(Debug, Clone)]
struct Pending {
    id: String,
    confirm_by: u64,
}

/// Prepared confirmations, one per store (keyed by the document path).
fn pending() -> &'static Mutex<HashMap<PathBuf, Pending>> {
    static P: OnceLock<Mutex<HashMap<PathBuf, Pending>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Folders a grant may never be rooted in (or under), relative to home. The sidecar's default-deny
/// list is the authority and is checked on every agent file operation; this early check only gives
/// the member a clear refusal for the obvious cases.
const DENIED_UNDER_HOME: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
    ".docker",
    ".config/gcloud",
    "Library/Keychains",
];

/// The member's grant store.
#[derive(Debug, Clone)]
pub struct GrantStore {
    dir: PathBuf,
    home: PathBuf,
    /// Core's own data folder: never grantable.
    protected: PathBuf,
}

impl GrantStore {
    /// A store in `dir` for the member whose home is `home`.
    pub fn new(dir: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        GrantStore {
            protected: dir.clone(),
            dir,
            home: home.into(),
        }
    }

    /// Also refuse grants on or under `protected` (core's whole app-data folder).
    pub fn protecting(mut self, protected: impl Into<PathBuf>) -> Self {
        self.protected = protected.into();
        self
    }

    /// The production store: `<app data>/agent`, home from the OS.
    pub fn for_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Self, String> {
        use tauri::Manager;
        let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let home = app.path().home_dir().map_err(|e| e.to_string())?;
        Ok(GrantStore::new(data.join("agent"), home).protecting(data))
    }

    fn file(&self) -> PathBuf {
        self.dir.join(GRANTS_FILE)
    }

    /// Read the document. Missing = no grants; unreadable or invalid = [`Loaded::Corrupted`].
    pub fn load(&self) -> Loaded {
        let text = match std::fs::read_to_string(self.file()) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Loaded::Ok(GrantState::default())
            }
            Err(e) => return Loaded::Corrupted(format!("unreadable: {}", e.kind())),
        };
        let st: GrantState = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => return Loaded::Corrupted(format!("not a grant document: {e}")),
        };
        match validate(&st) {
            Ok(()) => Loaded::Ok(st),
            Err(e) => Loaded::Corrupted(e),
        }
    }

    fn load_for_change(&self) -> Result<GrantState, GrantsError> {
        match self.load() {
            Loaded::Ok(s) => Ok(s),
            Loaded::Corrupted(e) => Err(GrantsError::Corrupted(e)),
        }
    }

    /// Write the document owner-only, through a temporary file and a rename.
    pub fn save(&self, st: &GrantState) -> Result<(), GrantsError> {
        validate(st).map_err(GrantsError::Invalid)?;
        let text = serde_json::to_string_pretty(st).map_err(|e| GrantsError::Io(e.to_string()))?;
        let tmp = self.dir.join(format!("{GRANTS_FILE}.tmp"));
        citrate_core_kit::fsutil::write_secret_file(&tmp, text.as_bytes())
            .map_err(|e| GrantsError::Io(e.kind().to_string()))?;
        std::fs::rename(&tmp, self.file()).map_err(|e| GrantsError::Io(e.kind().to_string()))
    }

    /// The stored file as it is (`None` when there is none), so a change can be put back.
    pub(crate) fn raw(&self) -> Result<Option<Vec<u8>>, GrantsError> {
        match std::fs::read(self.file()) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(GrantsError::Io(e.kind().to_string())),
        }
    }

    /// Put back what [`Self::raw`] returned.
    pub(crate) fn restore_raw(&self, raw: Option<Vec<u8>>) -> Result<(), GrantsError> {
        match raw {
            Some(bytes) => {
                let tmp = self.dir.join(format!("{GRANTS_FILE}.tmp"));
                citrate_core_kit::fsutil::write_secret_file(&tmp, &bytes)
                    .map_err(|e| GrantsError::Io(e.kind().to_string()))?;
                std::fs::rename(&tmp, self.file())
                    .map_err(|e| GrantsError::Io(e.kind().to_string()))
            }
            None => match std::fs::remove_file(self.file()) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(GrantsError::Io(e.kind().to_string())),
            },
        }
    }

    /// The document the agent receives: the stored one, or an empty one (no grants) when the
    /// stored one is corrupted.
    pub fn document_for_agent(&self) -> GrantState {
        match self.load() {
            Loaded::Ok(s) => s,
            Loaded::Corrupted(_) => GrantState::default(),
        }
    }

    /// The Grants panel view at `now`.
    pub fn view(&self, now: u64) -> GrantsView {
        let full_access_root = self.home.display().to_string();
        let st = match self.load() {
            Loaded::Ok(s) => s,
            Loaded::Corrupted(e) => {
                return GrantsView {
                    status: "corrupted",
                    error: Some(format!(
                        "The saved folder grants could not be read ({e}). Hermes is treated as having no folder access, so it grants nothing until you reset them."
                    )),
                    grants: Vec::new(),
                    full_access_remaining_secs: None,
                    full_access_root,
                    now,
                }
            }
        };
        let grants: Vec<GrantRow> = st
            .grants
            .iter()
            .map(|g| GrantRow {
                id: g.id.clone(),
                kind: match g.kind {
                    GrantKind::Folder => "folder",
                    GrantKind::FullAccess => "full_access",
                }
                .into(),
                root: g.root.clone(),
                access: match g.access {
                    Access::Read => "read",
                    Access::Write => "write",
                }
                .into(),
                status: g.status(now).into(),
                granted_at: g.granted_at,
                expires_at: g.expires_at,
                remaining_secs: g.expires_at.map(|t| t.saturating_sub(now)),
                reason: g.reason.clone(),
            })
            .collect();
        let full_access_remaining_secs = st
            .grants
            .iter()
            .filter(|g| g.kind == GrantKind::FullAccess && g.status(now) == "active")
            .filter_map(|g| g.expires_at.map(|t| t.saturating_sub(now)))
            .max();
        GrantsView {
            status: "ok",
            error: None,
            grants,
            full_access_remaining_secs,
            full_access_root,
            now,
        }
    }

    /// Resolve and check a folder the member picked.
    fn canonical_folder(&self, root: &Path) -> Result<PathBuf, GrantsError> {
        if !root.is_absolute() {
            return Err(GrantsError::Invalid(
                "pick a folder by its full path".into(),
            ));
        }
        let canon = std::fs::canonicalize(root).map_err(|_| {
            GrantsError::Invalid(format!("{} is not an existing folder", root.display()))
        })?;
        if !canon.is_dir() {
            return Err(GrantsError::Invalid(format!(
                "{} is not a folder",
                canon.display()
            )));
        }
        let home = std::fs::canonicalize(&self.home).unwrap_or_else(|_| self.home.clone());
        for d in DENIED_UNDER_HOME {
            if canon.starts_with(home.join(d)) {
                return Err(GrantsError::Invalid(format!(
                    "{} holds credentials and cannot be granted",
                    canon.display()
                )));
            }
        }
        let own = std::fs::canonicalize(&self.protected).unwrap_or_else(|_| self.protected.clone());
        if canon.starts_with(&own) {
            return Err(GrantsError::Invalid(
                "Citrate Core's own data folder cannot be granted".into(),
            ));
        }
        Ok(canon)
    }

    /// Grant `root` for reading and/or writing (one grant per access). Returns the new ids.
    pub fn add_folder(
        &self,
        root: &Path,
        read: bool,
        write: bool,
        now: u64,
    ) -> Result<Vec<String>, GrantsError> {
        if !read && !write {
            return Err(GrantsError::Invalid("choose read, write, or both".into()));
        }
        let mut st = self.load_for_change()?;
        let canon = self.canonical_folder(root)?;
        if write && canon.parent().is_none() {
            return Err(GrantsError::Invalid(
                "write access to the whole disk cannot be granted".into(),
            ));
        }
        let mut ids = Vec::new();
        for (on, access) in [(read, Access::Read), (write, Access::Write)] {
            if !on {
                continue;
            }
            let id = format!("g-{}", st.next_id);
            st.next_id = st
                .next_id
                .checked_add(1)
                .ok_or_else(|| GrantsError::Invalid("grant ids exhausted".into()))?;
            st.grants.push(Grant {
                id: id.clone(),
                kind: GrantKind::Folder,
                root: canon.display().to_string(),
                access,
                scope: "subtree".into(),
                granted_at: now,
                expires_at: None,
                granted_by: GRANTED_BY.into(),
                reason: FOLDER_REASON.into(),
                revoked_at: None,
            });
            ids.push(id);
        }
        self.save(&st)?;
        Ok(ids)
    }

    /// Revoke a grant now. Final; the row stays in the list as revoked.
    pub fn revoke(&self, id: &str, now: u64) -> Result<(), GrantsError> {
        let mut st = self.load_for_change()?;
        let g = st
            .grants
            .iter_mut()
            .find(|g| g.id == id)
            .ok_or_else(|| GrantsError::Invalid(format!("no grant {id}")))?;
        if g.revoked_at.is_some() {
            return Err(GrantsError::Invalid(format!("{id} is already revoked")));
        }
        g.revoked_at = Some(now.max(g.granted_at));
        self.save(&st)
    }

    /// Step 1 of the HIC-1 confirmation for full access: what the member is agreeing to, and a
    /// one-shot id valid for [`CONFIRM_WINDOW_SECS`]. A newer prepare replaces an older one.
    pub fn full_access_prepare(&self, now: u64) -> Result<FullAccessConfirmation, GrantsError> {
        let st = self.load_for_change()?;
        if st
            .grants
            .iter()
            .any(|g| g.kind == GrantKind::FullAccess && g.status(now) == "active")
        {
            return Err(GrantsError::Invalid(
                "full access is already on; turn it off first to start a new window".into(),
            ));
        }
        let root = std::fs::canonicalize(&self.home).unwrap_or_else(|_| self.home.clone());
        let id = confirmation_id();
        let c = FullAccessConfirmation {
            id: id.clone(),
            root: root.display().to_string(),
            statement: format!(
                "For the next 24 hours Hermes may read any file under {}. It cannot change, create or delete anything through this window; writing still needs a folder grant. Credential folders (.ssh, .aws, .gnupg, .kube), .env files, keychains and wallet data stay blocked. You can turn it off at any time.",
                root.display()
            ),
            prepared_at: now,
            confirm_by: now + CONFIRM_WINDOW_SECS,
            grant_expires_at: now + FULL_ACCESS_SECS,
        };
        pending().lock().unwrap_or_else(|e| e.into_inner()).insert(
            self.file(),
            Pending {
                id,
                confirm_by: c.confirm_by,
            },
        );
        Ok(c)
    }

    /// Step 2: the member confirmed `id`. Creates the read-only window, expiring 24 h from `now`.
    pub fn full_access_confirm(&self, id: &str, now: u64) -> Result<String, GrantsError> {
        let p = {
            let mut map = pending().lock().unwrap_or_else(|e| e.into_inner());
            match map.get(&self.file()) {
                Some(p) if p.id == id => map.remove(&self.file()),
                _ => None,
            }
        };
        let p = p.ok_or_else(|| {
            GrantsError::Invalid("this confirmation is not the current one; start again".into())
        })?;
        if now >= p.confirm_by {
            return Err(GrantsError::Invalid(
                "the confirmation timed out; start again".into(),
            ));
        }
        let mut st = self.load_for_change()?;
        if st
            .grants
            .iter()
            .any(|g| g.kind == GrantKind::FullAccess && g.status(now) == "active")
        {
            return Err(GrantsError::Invalid("full access is already on".into()));
        }
        let root = std::fs::canonicalize(&self.home).unwrap_or_else(|_| self.home.clone());
        let gid = format!("g-{}", st.next_id);
        st.next_id = st
            .next_id
            .checked_add(1)
            .ok_or_else(|| GrantsError::Invalid("grant ids exhausted".into()))?;
        st.grants.push(Grant {
            id: gid.clone(),
            kind: GrantKind::FullAccess,
            root: root.display().to_string(),
            access: Access::Read,
            scope: "subtree".into(),
            granted_at: now,
            expires_at: Some(now + FULL_ACCESS_SECS),
            granted_by: GRANTED_BY.into(),
            reason: FULL_ACCESS_REASON.into(),
            revoked_at: None,
        });
        self.save(&st)?;
        Ok(gid)
    }

    /// Set a corrupted document aside (`agent-grants.json.corrupt-<now>`) and start with no
    /// grants. Returns where the old file was kept. Refused when the document is valid.
    pub fn reset_corrupted(&self, now: u64) -> Result<PathBuf, GrantsError> {
        if let Loaded::Ok(_) = self.load() {
            return Err(GrantsError::Invalid(
                "the saved grants are readable; revoke them one by one instead".into(),
            ));
        }
        let kept = self.dir.join(format!("{GRANTS_FILE}.corrupt-{now}"));
        std::fs::rename(self.file(), &kept).map_err(|e| GrantsError::Io(e.kind().to_string()))?;
        self.save(&GrantState::default())?;
        Ok(kept)
    }
}

fn confirmation_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The rules `citrate_agent_grants::FolderGrants::from_state` enforces on a stored document, minus
/// its default-deny check (the sidecar runs that on load and on every operation).
fn validate(st: &GrantState) -> Result<(), String> {
    if st.version != GRANTS_VERSION {
        return Err(format!("unknown version {}", st.version));
    }
    let mut seen = std::collections::HashSet::new();
    for g in &st.grants {
        let n =
            g.id.strip_prefix("g-")
                .and_then(|n| n.parse::<u64>().ok())
                .ok_or_else(|| format!("bad id {:?}", g.id))?;
        if n >= st.next_id {
            return Err(format!("id {} is not below next_id {}", g.id, st.next_id));
        }
        if !seen.insert(n) {
            return Err(format!("duplicate id {}", g.id));
        }
        let root = Path::new(&g.root);
        let plain = root.components().all(|c| {
            matches!(
                c,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        });
        if !root.is_absolute() || !plain {
            return Err(format!("{}: root is not absolute and normalized", g.id));
        }
        if g.scope != "subtree" && g.scope != "shallow" {
            return Err(format!("{}: unknown scope {:?}", g.id, g.scope));
        }
        if g.access == Access::Write && root.parent().is_none() {
            return Err(format!("{}: write on the filesystem root", g.id));
        }
        if g.granted_by.trim().is_empty() || g.reason.trim().is_empty() {
            return Err(format!("{}: missing granted_by or reason", g.id));
        }
        if let Some(t) = g.expires_at {
            if t <= g.granted_at {
                return Err(format!("{}: expires before it was granted", g.id));
            }
        }
        if g.kind == GrantKind::FullAccess {
            if g.access != Access::Read {
                return Err(format!("{}: full access is read-only", g.id));
            }
            match g.expires_at {
                Some(t) if t - g.granted_at <= FULL_ACCESS_SECS => {}
                _ => return Err(format!("{}: full access must expire within 24 h", g.id)),
            }
        }
    }
    Ok(())
}

/// Add the grant document to a `POST /sessions` body (as `grants`).
pub fn attach_grants(body: &str, doc: &GrantState) -> Result<String, String> {
    let mut v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "internal: session body is not JSON".to_string())?;
    let obj = v
        .as_object_mut()
        .ok_or("internal: session body is not an object")?;
    obj.insert(
        "grants".into(),
        serde_json::to_value(doc).map_err(|e| e.to_string())?,
    );
    Ok(v.to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a change returns to the panel: the new view, and how delivery to open agent sessions went.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantsChange {
    pub view: GrantsView,
    pub sync: crate::hermes::GrantsPushOutcome,
}

/// One grant change (or one grant-carrying session open) at a time. Every change is a read, a
/// modify and a write of the whole document, so two at once could otherwise lose one (a revocation
/// undone by a concurrent grant). The lock also covers sending the result to the open sessions, so
/// the sidecar receives documents in the order they were saved, and it covers a session open, so a
/// change saved while a session is opening still reaches that session.
fn store_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Apply `f` to the store, record the member's decision with `record` (HUP-S2.6), then send the
/// new document with `push`, under [`store_lock`]. Fail closed (pending owner sign-off): when the
/// decision cannot be recorded, the saved change is undone (the previous file is put back) and
/// the change is refused, so no grant change goes unrecorded.
pub(crate) fn apply_change(
    store: &GrantStore,
    f: impl FnOnce(&GrantStore, u64) -> Result<crate::hic_records::HicEvent, GrantsError>,
    record: impl FnOnce(crate::hic_records::HicEvent) -> Result<(), String>,
    push: impl FnOnce(&GrantState) -> crate::hermes::GrantsPushOutcome,
) -> Result<GrantsChange, String> {
    let _one_at_a_time = store_lock();
    let before = store.raw().map_err(|e| e.to_string())?;
    let ev = f(store, now_secs()).map_err(|e| e.to_string())?;
    if let Err(e) = record(ev) {
        return match store.restore_raw(before) {
            Ok(()) => Err(format!(
                "the change was not kept because its decision record could not be written: {e}"
            )),
            Err(r) => Err(format!(
                "the decision record could not be written ({e}) and the change could not be undone ({r})"
            )),
        };
    }
    let sync = push(&store.document_for_agent());
    Ok(GrantsChange {
        view: store.view(now_secs()),
        sync,
    })
}

/// Open an agent session with the current document attached (`open` does the `POST /sessions`
/// and records the session), under [`store_lock`].
pub(crate) fn open_with_grants(
    store: &GrantStore,
    body: &str,
    open: impl FnOnce(&str) -> Result<String, String>,
) -> Result<String, String> {
    let _one_at_a_time = store_lock();
    let body = attach_grants(body, &store.document_for_agent())?;
    open(&body)
}

fn change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    f: impl FnOnce(&GrantStore, u64) -> Result<crate::hic_records::HicEvent, GrantsError>,
) -> Result<GrantsChange, String> {
    let store = GrantStore::for_app(app)?;
    apply_change(
        &store,
        f,
        |ev| crate::hic_records::record_for_app(app, ev).map(|_| ()),
        crate::hermes::push_grants_to_sessions,
    )
}

/// HUP-S2.6: the decision record for folder grants just added.
pub(crate) fn added_event(store: &GrantStore, ids: &[String]) -> crate::hic_records::HicEvent {
    let root = match store.load() {
        Loaded::Ok(st) => st
            .grants
            .iter()
            .find(|g| ids.first() == Some(&g.id))
            .map(|g| g.root.clone())
            .unwrap_or_default(),
        Loaded::Corrupted(_) => String::new(),
    };
    crate::hic_records::grant_added(&root, ids)
}

/// HUP-S2.6: the decision record for a confirmed full-access window.
pub(crate) fn full_access_event(store: &GrantStore, gid: &str) -> crate::hic_records::HicEvent {
    let (root, expires) = match store.load() {
        Loaded::Ok(st) => st
            .grants
            .iter()
            .find(|g| g.id == gid)
            .map(|g| (g.root.clone(), g.expires_at.unwrap_or(0)))
            .unwrap_or_default(),
        Loaded::Corrupted(_) => (String::new(), 0),
    };
    crate::hic_records::full_access_confirmed(gid, &root, expires)
}

/// **agent_grants_view** — the Grants panel.
#[tauri::command]
pub async fn agent_grants_view(app: tauri::AppHandle) -> Result<GrantsView, String> {
    crate::blocking::off_main(move || Ok(GrantStore::for_app(&app)?.view(now_secs()))).await
}

/// **agent_grants_add_folder** — grant a folder the member picked, for reading and/or writing.
#[tauri::command]
pub async fn agent_grants_add_folder(
    app: tauri::AppHandle,
    path: String,
    read: bool,
    write: bool,
) -> Result<GrantsChange, String> {
    crate::blocking::off_main(move || {
        change(&app, |s, now| {
            let ids = s.add_folder(Path::new(&path), read, write, now)?;
            Ok(added_event(s, &ids))
        })
    })
    .await
}

/// **agent_grants_revoke** — revoke a grant now.
#[tauri::command]
pub async fn agent_grants_revoke(
    app: tauri::AppHandle,
    id: String,
) -> Result<GrantsChange, String> {
    crate::blocking::off_main(move || {
        change(&app, |s, now| {
            s.revoke(&id, now)?;
            Ok(crate::hic_records::grant_revoked(&id))
        })
    })
    .await
}

/// **agent_grants_full_access_prepare** — step 1 of the HIC-1 confirmation.
#[tauri::command]
pub async fn agent_grants_full_access_prepare(
    app: tauri::AppHandle,
) -> Result<FullAccessConfirmation, String> {
    crate::blocking::off_main(move || {
        GrantStore::for_app(&app)?
            .full_access_prepare(now_secs())
            .map_err(|e| e.to_string())
    })
    .await
}

/// **agent_grants_full_access_confirm** — step 2: the member confirmed this id.
#[tauri::command]
pub async fn agent_grants_full_access_confirm(
    app: tauri::AppHandle,
    id: String,
) -> Result<GrantsChange, String> {
    crate::blocking::off_main(move || {
        change(&app, |s, now| {
            let gid = s.full_access_confirm(&id, now)?;
            Ok(full_access_event(s, &gid))
        })
    })
    .await
}

/// **agent_grants_reset** — set a corrupted document aside and start with no grants.
#[tauri::command]
pub async fn agent_grants_reset(app: tauri::AppHandle) -> Result<GrantsChange, String> {
    crate::blocking::off_main(move || {
        change(&app, |s, now| {
            s.reset_corrupted(now)?;
            Ok(crate::hic_records::grant_reset())
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("agent_grants_tests.rs");
}
