//! K5a: Store v1 in the app. The signed catalog, staged installs that arrive paused, and
//! one-call removal.
//!
//! Data sources (Rule 7):
//! - The catalog: a JSON document at [`STORE_CATALOG_URL`] with a detached minisign signature at
//!   [`STORE_CATALOG_SIG_URL`], verified against the pinned catalog key
//!   (`citrate_components::catalog`). The key comes from the genesis key ceremony; until then
//!   the slot is empty and every command that needs the catalog refuses with "Store catalog not
//!   available yet: signing key not issued", before any network or disk access.
//! - The member's tier: the claim-derived `AuthStatus` (the same `/userinfo` entitlement the rest
//!   of the app gates on). Gating is fail-closed: an unknown tier or an unparseable expiry
//!   reaches nothing gated.
//! - Install targets: the daemon book (`daemons.rs`, `daemons.json`) and the models folder (the
//!   same streamed download and SHA-256 verify the Models screen uses, `model.rs`).
//!
//! Install path: verify the catalog signature, then stage (a daemon template is downloaded and
//! its SHA-256 and size are checked now; a model's hash is checked as it downloads, after the
//! member approves, so a multi-GB file is not pulled before they say yes), then return the
//! declared permissions and budget to the install sheet. The member approves there (HIC: a
//! member click bound to one single-use stage id that expires), and the listing installs PAUSED:
//! a daemon is saved paused, a model is downloaded and verified but not selected. Removal is one
//! call and needs no key.
//!
//! Only kinds with a real target in core today install: daemons and models. Apps, widgets,
//! skills and MCP servers are listed and say "not installable yet".
//!
//! Third-party listings are refused by the catalog verifier in this version.
//!
//! Rule 3: nothing here signs or holds a key; the catalog key is a public key.
use std::path::{Path, PathBuf};

use citrate_components::catalog::{
    catalog_trust_root, is_listing_id, verify_catalog, CatalogError, DeclaredBudget, Listing,
    ListingKind, SeenCatalog, VerifiedCatalog, MAX_CATALOG_BYTES,
};
use citrate_components::fetch::{Fetcher, HttpsFetcher};
use citrate_components::key::TrustRoot;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// Where the signed Store catalog will be published (CDN, beside the component manifests). Not
/// published yet: the first catalog is signed with the ceremony key.
pub const STORE_CATALOG_URL: &str =
    "https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/store/v1/catalog.json";
pub const STORE_CATALOG_SIG_URL: &str =
    "https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/store/v1/catalog.json.minisig";
const MAX_SIG_BYTES: u64 = 4096;
/// The largest daemon template the Store downloads.
pub(crate) const MAX_DAEMON_TEMPLATE_BYTES: u64 = 64 * 1024;
/// How long a reviewed install stays approvable.
pub(crate) const STAGE_TTL_SECS: u64 = 15 * 60;
const STATE_FILE: &str = "state.json";
const CATALOG_FILE: &str = "catalog.json";
const CATALOG_SIG_FILE: &str = "catalog.json.minisig";
const STAGING_DIR: &str = "staging";
const STATE_VERSION: u32 = 1;

/// The commands this module registers (kept in step with lib.rs and main-window.toml by a test).
#[cfg(test)]
pub(crate) const COMMANDS: [&str; 5] = [
    "store_catalog",
    "store_stage",
    "store_install",
    "store_installed",
    "store_remove",
];

// ---------------------------------------------------------------------------
// Refusals: one code per honest state the UI draws
// ---------------------------------------------------------------------------

/// Why a Store call did not go ahead. `code` is one of: `no_key`, `fetch_failed`,
/// `signature_failed`, `invalid`, `expired`, `rollback`, `hash_failed`, `tier_locked`,
/// `not_installable`, `not_listed`, `stage_expired`, `changed`, `already_installed`,
/// `not_installed`, `in_use`, `install_running`, `daemon_running`, `install_failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

fn refuse(code: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code: code.to_string(),
        message: message.into(),
    }
}

fn catalog_refusal(e: &CatalogError) -> Refusal {
    let code = match e {
        CatalogError::KeyNotIssued => "no_key",
        CatalogError::BadKey(_)
        | CatalogError::SignatureInvalid(_)
        | CatalogError::WrongPurpose => "signature_failed",
        CatalogError::Malformed(_) => "invalid",
        CatalogError::Expired | CatalogError::NotYetValid => "expired",
        CatalogError::Rollback { .. } => "rollback",
    };
    refuse(code, e.to_string())
}

fn io_refusal(e: impl std::fmt::Display) -> Refusal {
    refuse("install_failed", format!("Store file error: {e}"))
}

/// A result the webview can draw without parsing error text: exactly one of `ok` or `refused`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<Refusal>,
}

impl<T> From<Result<T, Refusal>> for Outcome<T> {
    fn from(r: Result<T, Refusal>) -> Self {
        match r {
            Ok(v) => Outcome {
                ok: Some(v),
                refused: None,
            },
            Err(e) => Outcome {
                ok: None,
                refused: Some(e),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tier gating (fail-closed)
// ---------------------------------------------------------------------------

/// Days since 1970-01-01 for a civil date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// `n` ASCII digits of `b` from `at`, as a number; `None` unless all are digits.
fn digits(b: &[u8], at: usize, n: usize) -> Option<i64> {
    let part = b.get(at..at + n)?;
    if !part.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(
        part.iter()
            .fold(0i64, |acc, d| acc * 10 + i64::from(d - b'0')),
    )
}

/// An entitlement expiry in unix seconds, parsed STRICTLY. Accepted: all digits (unix seconds,
/// or milliseconds when >= 1e12); an RFC 3339 full-date `YYYY-MM-DD` (midnight UTC); or an RFC
/// 3339 date-time `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)` (`T`/`t`/space, `Z`/`z`), with a real
/// calendar date, in-range time fields and the offset applied. Anything else (trailing text, a
/// missing offset, an impossible date) is `None`, which the caller treats as lapsed.
pub(crate) fn expiry_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    let b = s.as_bytes();
    if !b.is_empty() && b.iter().all(u8::is_ascii_digit) {
        if b.len() > 16 {
            return None;
        }
        let n: u64 = s.parse().ok()?;
        return Some(if n >= 1_000_000_000_000 { n / 1000 } else { n });
    }
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d) = (digits(b, 0, 4)?, digits(b, 5, 2)?, digits(b, 8, 2)?);
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    let day_secs = days_from_civil(y, m, d) * 86_400;
    if b.len() == 10 {
        return u64::try_from(day_secs).ok();
    }
    // Date-time: the separator, HH:MM:SS, optional fraction, then the offset, and nothing after.
    if !matches!(b[10], b'T' | b't' | b' ') || b.len() < 20 || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (h, mi, sec) = (digits(b, 11, 2)?, digits(b, 14, 2)?, digits(b, 17, 2)?);
    // 60 is a leap second (RFC 3339 §5.7); it counts as the last second of the minute.
    if h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        let frac = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if frac == 0 {
            return None;
        }
        i += 1 + frac;
    }
    let offset = match b.get(i..)? {
        [b'Z'] | [b'z'] => 0,
        [sign @ (b'+' | b'-'), _, _, b':', _, _] => {
            let (oh, om) = (digits(b, i + 1, 2)?, digits(b, i + 4, 2)?);
            if oh > 23 || om > 59 {
                return None;
            }
            let o = oh * 3600 + om * 60;
            if *sign == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    let local = day_secs + h * 3600 + mi * 60 + sec.min(59);
    u64::try_from(local - offset).ok()
}

/// The member's rank on the listing ladder (public 0, member 1, enterprise 2), FAIL-CLOSED.
/// The tier names are a hard-coded allowlist on purpose: a tier added in identity later locks
/// its members out of every listing (public included) until core ships it here
/// (docs/STORE_CATALOG.md, "Tier gating"). The rules:
/// signed out, no tier claim or a lapsed entitlement is 0 (public listings only); a tier this
/// app does not recognise is -1 (nothing, not even public). An expiry that does not parse counts
/// as lapsed.
pub(crate) fn member_rank(
    signed_in: bool,
    tier: Option<&str>,
    expires_at: Option<&str>,
    now: u64,
) -> i32 {
    if !signed_in {
        return 0;
    }
    if let Some(e) = expires_at.filter(|e| !e.trim().is_empty()) {
        match expiry_secs(e) {
            Some(t) if t > now => {}
            _ => return 0,
        }
    }
    match tier {
        None => 0,
        Some("free" | "public") => 0,
        Some("pilot" | "member" | "commercial" | "commercial.kyc" | "academic") => 1,
        Some("enterprise" | "confidential") => 2,
        Some(_) => -1,
    }
}

fn lock_reason(listing: &Listing, rank: i32) -> Option<String> {
    if rank >= listing.tier.rank() {
        return None;
    }
    Some(if rank < 0 {
        "Locked: your membership tier is not recognised by this version of the app".to_string()
    } else {
        match listing.tier.as_str() {
            "enterprise" => "Needs an Enterprise seat".to_string(),
            _ => "Needs an active membership".to_string(),
        }
    })
}

// ---------------------------------------------------------------------------
// Installers: only kinds with a real target in core today
// ---------------------------------------------------------------------------

/// K5a: the agent tools each declared permission grants a Store daemon. DENY BY DEFAULT: a
/// daemon installed from the Store is offered and may call only the tools its approved
/// permissions map to here; a permission not in this table (or one that names no core tool,
/// like `local_model` or `files_read`) grants nothing. `app_navigate` is never granted.
pub(crate) const PERMISSION_TOOLS: &[(&str, &[&str])] = &[
    ("local_model", &[]),
    ("memory_read", &["memory_recall", "memory_search"]),
    ("memory_write", &["memory_assert"]),
    ("journal_read", &["journal_read"]),
    ("journal_write", &["journal_append"]),
    (
        "chain_read",
        &[
            "node_status",
            "staking_status",
            "contract_view",
            "get_verified_source",
        ],
    ),
    ("groups_read", &["groups_list", "group_roster"]),
    ("models_read", &["models_list"]),
    ("skills_read", &["skills_list"]),
    ("calendar_read", &["calendar_list"]),
    ("sheets_read", &["gsheets_read"]),
    ("schedule_read", &["schedule_list"]),
];

/// The tools a set of approved permissions grants (sorted, without duplicates): core tools from
/// [`PERMISSION_TOOLS`], plus a `host:<set>` marker for each sidecar-hosted tool set a
/// permission covers (`daemons::HOST_TOOL_SETS`), which lets the daemon start while that set is on.
pub(crate) fn tools_for_permissions(permissions: &[String]) -> Vec<String> {
    let mut out: Vec<String> = permissions
        .iter()
        .filter_map(|p| PERMISSION_TOOLS.iter().find(|(id, _)| id == p))
        .flat_map(|(_, tools)| tools.iter())
        .filter(|t| **t != "app_navigate")
        .map(|t| t.to_string())
        .collect();
    out.extend(
        crate::daemons::HOST_TOOL_SETS
            .iter()
            .filter(|set| permissions.iter().any(|p| p == set.permission))
            .map(|set| crate::daemons::host_set_marker(set.id)),
    );
    out.sort();
    out.dedup();
    out
}

/// `None` when the kind installs; otherwise the honest reason it does not yet.
pub(crate) fn not_installable_reason(kind: ListingKind) -> Option<&'static str> {
    match kind {
        ListingKind::Daemon | ListingKind::Model => None,
        ListingKind::App => {
            Some("Not installable yet: the app has no installer for federation apps and SDKs")
        }
        ListingKind::Widget => Some("Not installable yet: widget installs from the Store are not built"),
        ListingKind::Skill => Some("Not installable yet: skill installs from the Store are not built"),
        ListingKind::McpServer => Some(
            "Not installable yet: MCP server installs from the Store are not built (Settings > MCP servers adds one by hand)",
        ),
    }
}

/// A daemon listing's artifact: the task and schedule. The budget is the listing's declared one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DaemonTemplate {
    pub name: String,
    pub prompt: String,
    pub schedule: String,
}

fn parse_daemon_template(bytes: &[u8]) -> Result<DaemonTemplate, Refusal> {
    let t: DaemonTemplate = serde_json::from_slice(bytes)
        .map_err(|e| refuse("invalid", format!("the daemon template is not valid: {e}")))?;
    crate::daemons::Schedule::parse(t.schedule.trim()).map_err(|e| {
        refuse(
            "invalid",
            format!("the daemon template schedule is not valid: {e}"),
        )
    })?;
    if t.prompt.trim().is_empty() || t.prompt.chars().count() > crate::daemons::PROMPT_MAX {
        return Err(refuse(
            "invalid",
            "the daemon template task is empty or too long",
        ));
    }
    if t.name.trim().is_empty() || t.name.chars().count() > crate::daemons::NAME_MAX {
        return Err(refuse(
            "invalid",
            "the daemon template name is empty or too long",
        ));
    }
    Ok(t)
}

/// The model file name a listing installs as: the URL's last path segment, which must be a plain
/// `.gguf` name.
pub(crate) fn model_file_for(listing: &Listing) -> Result<String, Refusal> {
    let path = listing
        .artifact
        .url
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let file = path.rsplit('/').next().unwrap_or_default();
    crate::model_delete::file_from_id(file).map_err(|e| {
        refuse(
            "invalid",
            format!("the model listing's file name is not usable: {e}"),
        )
    })
}

// ---------------------------------------------------------------------------
// On-disk state: <app data>/store/
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InstallRecord {
    pub listing_id: String,
    pub kind: ListingKind,
    pub name: String,
    pub version: String,
    /// The daemon id, or the model file name. Known before the target is created: the record
    /// is saved first, so an install the Store made is always tracked.
    pub target: String,
    pub installed_at: u64,
    /// Saved before the target was created and not yet confirmed. A pending record whose target
    /// exists is a full install (only the confirming write was lost); one whose target is absent
    /// shows `installing` while an install runs, else `missing` (removable).
    #[serde(default)]
    pub pending: bool,
    /// For a model: the pinned SHA-256, size and URL from the signed listing, so an install the
    /// app left between the rename and the verify can be checked again on the next start.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub size: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoreFile {
    version: u32,
    last_seen: Option<SeenCatalog>,
    installs: Vec<InstallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StagedFile {
    stage_id: String,
    catalog_digest: String,
    listing: Listing,
    created_at: u64,
    /// The verified daemon template bytes (daemon listings only).
    payload: Option<String>,
}

static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// One install at a time per Store folder (a model install can take minutes); a second is
/// refused, not queued. The value is the listing being installed, once known: Remove refuses
/// that listing until the install ends (K5-F).
static INSTALLS_RUNNING: std::sync::Mutex<std::collections::BTreeMap<PathBuf, Option<String>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

struct InstallSlot(PathBuf);

impl InstallSlot {
    fn take(root: &Path) -> Result<Self, Refusal> {
        let mut running = INSTALLS_RUNNING.lock().unwrap_or_else(|p| p.into_inner());
        if running.contains_key(root) {
            return Err(refuse(
                "install_failed",
                "Another Store install is running; try again when it finishes",
            ));
        }
        running.insert(root.to_path_buf(), None);
        Ok(InstallSlot(root.to_path_buf()))
    }

    /// Name the listing this install is for (before its record is saved).
    fn set_listing(&self, listing_id: &str) {
        let mut running = INSTALLS_RUNNING.lock().unwrap_or_else(|p| p.into_inner());
        running.insert(self.0.clone(), Some(listing_id.to_string()));
    }
}

impl Drop for InstallSlot {
    fn drop(&mut self) {
        let mut running = INSTALLS_RUNNING.lock().unwrap_or_else(|p| p.into_inner());
        running.remove(&self.0);
    }
}

/// The listing an install in this Store folder is working on right now, if any.
fn installing_listing(root: &Path) -> Option<String> {
    INSTALLS_RUNNING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(root)
        .cloned()
        .flatten()
}

fn read_store_file(root: &Path) -> Result<StoreFile, Refusal> {
    match std::fs::read_to_string(root.join(STATE_FILE)) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| io_refusal(format!("the Store state file is damaged: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StoreFile {
            version: STATE_VERSION,
            ..StoreFile::default()
        }),
        Err(e) => Err(io_refusal(e)),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    let dir = path
        .parent()
        .ok_or_else(|| io_refusal("no parent folder"))?;
    std::fs::create_dir_all(dir).map_err(io_refusal)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(io_refusal)?;
    std::fs::rename(&tmp, path).map_err(io_refusal)
}

fn write_store_file(root: &Path, st: &StoreFile) -> Result<(), Refusal> {
    let text = serde_json::to_vec_pretty(st).map_err(io_refusal)?;
    write_atomic(&root.join(STATE_FILE), &text)
}

fn fetch_vec(f: &dyn Fetcher, url: &str, max: u64) -> Result<Vec<u8>, Refusal> {
    let mut buf = Vec::new();
    f.fetch(url, max, &mut buf).map_err(|e| {
        refuse(
            "fetch_failed",
            format!("Could not fetch the Store catalog: {e}"),
        )
    })?;
    Ok(buf)
}

/// Fetch, verify and record the catalog (the last verified copy is kept for staging).
fn fetch_catalog(
    root: &Path,
    key: &TrustRoot,
    fetcher: &dyn Fetcher,
    now: u64,
) -> Result<VerifiedCatalog, Refusal> {
    let bytes = fetch_vec(fetcher, STORE_CATALOG_URL, MAX_CATALOG_BYTES as u64)?;
    let sig_bytes = fetch_vec(fetcher, STORE_CATALOG_SIG_URL, MAX_SIG_BYTES)?;
    let sig = String::from_utf8(sig_bytes).map_err(|_| {
        refuse(
            "signature_failed",
            "the Store catalog signature is not text",
        )
    })?;
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut st = read_store_file(root)?;
    let vc = verify_catalog(&bytes, &sig, key, st.last_seen.as_ref(), now)
        .map_err(|e| catalog_refusal(&e))?;
    write_atomic(&root.join(CATALOG_FILE), &bytes)?;
    write_atomic(&root.join(CATALOG_SIG_FILE), sig.as_bytes())?;
    st.version = STATE_VERSION;
    st.last_seen = Some(vc.seen());
    write_store_file(root, &st)?;
    Ok(vc)
}

/// The last verified catalog, verified again from disk (signature, expiry, sequence).
fn cached_catalog(root: &Path, key: &TrustRoot, now: u64) -> Result<VerifiedCatalog, Refusal> {
    let missing = || {
        refuse(
            "fetch_failed",
            "Open the Store catalog again: no verified copy is on this machine",
        )
    };
    let bytes = std::fs::read(root.join(CATALOG_FILE)).map_err(|_| missing())?;
    let sig = std::fs::read_to_string(root.join(CATALOG_SIG_FILE)).map_err(|_| missing())?;
    let st = read_store_file(root)?;
    verify_catalog(&bytes, &sig, key, st.last_seen.as_ref(), now).map_err(|e| catalog_refusal(&e))
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionView {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    pub max_runs_per_day: u32,
    pub max_tokens_per_day: u32,
    pub max_tokens_per_run: u32,
    pub max_spend_salt: String,
}

fn budget_view(b: &DeclaredBudget) -> BudgetView {
    BudgetView {
        max_runs_per_day: b.max_runs_per_day,
        max_tokens_per_day: b.max_tokens_per_day,
        max_tokens_per_run: b.max_tokens_per_run,
        max_spend_salt: b.max_spend_salt.clone(),
    }
}

fn permission_views(l: &Listing) -> Vec<PermissionView> {
    l.permissions
        .iter()
        .map(|p| PermissionView {
            id: p.id.clone(),
            reason: p.reason.clone(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub summary: String,
    pub version: String,
    pub publisher: String,
    pub first_party: bool,
    pub tier: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub permissions: Vec<PermissionView>,
    pub budget: Option<BudgetView>,
    pub locked: bool,
    pub lock_reason: Option<String>,
    pub installable: bool,
    pub install_note: Option<String>,
    pub installed: bool,
}

fn listing_view(l: &Listing, rank: i32, installs: &[InstallRecord]) -> ListingView {
    let lock = lock_reason(l, rank);
    let note = not_installable_reason(l.kind);
    ListingView {
        id: l.id.clone(),
        kind: l.kind.as_str().to_string(),
        name: l.name.clone(),
        summary: l.summary.clone(),
        version: l.version.clone(),
        publisher: l.publisher.clone(),
        first_party: l.first_party,
        tier: l.tier.as_str().to_string(),
        size_bytes: l.artifact.size,
        sha256: l.artifact.sha256.clone(),
        permissions: permission_views(l),
        budget: l.budget.as_ref().map(budget_view),
        locked: lock.is_some(),
        lock_reason: lock,
        installable: note.is_none(),
        install_note: note.map(str::to_string),
        installed: installs.iter().any(|i| i.listing_id == l.id),
    }
}

/// The catalog as the Store screen draws it. `state` is `ready` or a [`Refusal`] code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub state: String,
    pub message: Option<String>,
    pub catalog_url: String,
    pub key_fingerprint: Option<String>,
    pub sequence: Option<u64>,
    pub expires_at: Option<u64>,
    pub listings: Vec<ListingView>,
}

fn unavailable_view(r: Refusal, fingerprint: Option<String>) -> CatalogView {
    CatalogView {
        state: r.code,
        message: Some(r.message),
        catalog_url: STORE_CATALOG_URL.to_string(),
        key_fingerprint: fingerprint,
        sequence: None,
        expires_at: None,
        listings: Vec::new(),
    }
}

/// The catalog view. With the key slot empty nothing is fetched or written.
pub(crate) fn catalog_view_sync(
    root: &Path,
    key: &Result<TrustRoot, CatalogError>,
    fetcher: &dyn Fetcher,
    rank: i32,
    now: u64,
) -> CatalogView {
    let key = match key {
        Ok(k) => k,
        Err(e) => return unavailable_view(catalog_refusal(e), None),
    };
    let fp = Some(key.fingerprint().to_string());
    let vc = match fetch_catalog(root, key, fetcher, now) {
        Ok(v) => v,
        Err(r) => return unavailable_view(r, fp),
    };
    let installs = read_store_file(root)
        .map(|s| s.installs)
        .unwrap_or_default();
    let c = vc.catalog();
    CatalogView {
        state: "ready".to_string(),
        message: None,
        catalog_url: STORE_CATALOG_URL.to_string(),
        key_fingerprint: fp,
        sequence: Some(c.sequence),
        expires_at: Some(c.expires_at),
        listings: c
            .listings
            .iter()
            .map(|l| listing_view(l, rank, &installs))
            .collect(),
    }
}

/// What the install sheet shows before the member approves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedView {
    pub stage_id: String,
    pub listing_id: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub permissions: Vec<PermissionView>,
    pub budget: Option<BudgetView>,
    /// `verified`: the artifact was downloaded and its size and SHA-256 matched the signed
    /// catalog. `on_download`: the hash is checked as the file downloads after approval.
    pub hash_check: String,
    /// Unix seconds after which the review must be done again.
    pub expires_at: u64,
}

fn new_stage_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

fn is_stage_id(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn require_installable(l: &Listing, rank: i32, st: &StoreFile) -> Result<(), Refusal> {
    if let Some(why) = lock_reason(l, rank) {
        return Err(refuse("tier_locked", why));
    }
    if let Some(why) = not_installable_reason(l.kind) {
        return Err(refuse("not_installable", why));
    }
    if st.installs.iter().any(|i| i.listing_id == l.id) {
        return Err(refuse(
            "already_installed",
            format!("{} is already installed", l.name),
        ));
    }
    Ok(())
}

/// Stage one listing: verify, check the tier, check the artifact, keep a single-use stage.
pub(crate) fn stage_sync(
    root: &Path,
    key: &Result<TrustRoot, CatalogError>,
    fetcher: &dyn Fetcher,
    models_dir: &Path,
    rank: i32,
    listing_id: &str,
    now: u64,
) -> Result<StagedView, Refusal> {
    let key = key.as_ref().map_err(catalog_refusal)?;
    if !is_listing_id(listing_id) {
        return Err(refuse("not_listed", "that is not a Store listing id"));
    }
    let vc = cached_catalog(root, key, now)?;
    let listing = vc.catalog().listing(listing_id).cloned().ok_or_else(|| {
        refuse(
            "not_listed",
            format!("{listing_id} is not in the Store catalog"),
        )
    })?;
    let st = read_store_file(root)?;
    require_installable(&listing, rank, &st)?;
    let (payload, hash_check) = match listing.kind {
        ListingKind::Daemon => {
            if listing.artifact.size > MAX_DAEMON_TEMPLATE_BYTES {
                return Err(refuse(
                    "invalid",
                    "the daemon template is larger than the Store allows",
                ));
            }
            let mut buf = Vec::new();
            fetcher
                .fetch(&listing.artifact.url, listing.artifact.size, &mut buf)
                .map_err(|e| {
                    refuse(
                        "fetch_failed",
                        format!("Could not download {}: {e}", listing.name),
                    )
                })?;
            if buf.len() as u64 != listing.artifact.size {
                return Err(refuse(
                    "hash_failed",
                    format!(
                        "The download is {} bytes; the signed catalog says {}",
                        buf.len(),
                        listing.artifact.size
                    ),
                ));
            }
            if hex::encode(sha2::Sha256::digest(&buf)) != listing.artifact.sha256 {
                return Err(refuse(
                    "hash_failed",
                    "The download does not match the SHA-256 in the signed catalog",
                ));
            }
            parse_daemon_template(&buf)?;
            let text = String::from_utf8(buf)
                .map_err(|_| refuse("invalid", "the daemon template is not text"))?;
            (Some(text), "verified")
        }
        ListingKind::Model => {
            let file = model_file_for(&listing)?;
            if models_dir.join(&file).exists() {
                return Err(refuse(
                    "already_installed",
                    format!("{file} is already on this machine; manage it in Models"),
                ));
            }
            (None, "on_download")
        }
        _ => return Err(refuse("not_installable", "this kind does not install yet")),
    };
    let stage_id = new_stage_id();
    let staged = StagedFile {
        stage_id: stage_id.clone(),
        catalog_digest: vc.digest_hex().to_string(),
        listing: listing.clone(),
        created_at: now,
        payload,
    };
    let text = serde_json::to_vec(&staged).map_err(io_refusal)?;
    write_atomic(
        &root.join(STAGING_DIR).join(format!("{stage_id}.json")),
        &text,
    )?;
    Ok(StagedView {
        stage_id,
        listing_id: listing.id.clone(),
        kind: listing.kind.as_str().to_string(),
        name: listing.name.clone(),
        version: listing.version.clone(),
        publisher: listing.publisher.clone(),
        size_bytes: listing.artifact.size,
        sha256: listing.artifact.sha256.clone(),
        permissions: permission_views(&listing),
        budget: listing.budget.as_ref().map(budget_view),
        hash_check: hash_check.to_string(),
        expires_at: now.saturating_add(STAGE_TTL_SECS),
    })
}

/// Where installs land, and how a model is fetched and checked for use.
pub(crate) struct Targets<'a> {
    pub daemons_path: PathBuf,
    pub models_dir: PathBuf,
    pub bundled_models: Option<PathBuf>,
    pub usage: &'a dyn crate::model_delete::ModelUsage,
    /// The chat server's model selection, reset when the Store removes the selected model.
    pub selection: &'a dyn crate::model_delete::ModelSelection,
    /// Builds the model transport for an artifact URL.
    pub model_transport: &'a dyn Fn(&str) -> Box<dyn crate::model::ModelTransport>,
}

/// One installed listing and its live state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledView {
    pub listing_id: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub target: String,
    pub installed_at: u64,
    /// `paused` (a daemon that does not run until started), `active` (a daemon the member
    /// started), `not_selected` (a model on disk, not in use until selected in Models),
    /// `installing` (recorded, target still being created), or `missing` (removed outside the
    /// Store, or an install that did not finish).
    pub state: String,
}

fn installed_view(
    r: &InstallRecord,
    t: &Targets<'_>,
    now_ms: u64,
    installing: Option<&str>,
) -> InstalledView {
    let state = match r.kind {
        ListingKind::Daemon => crate::daemons::with_book_file(&t.daemons_path, false, |b| {
            Ok(b.view(&r.target, now_ms, 0))
        })
        .ok()
        .flatten()
        .map(|v| if v.paused { "paused" } else { "active" })
        .unwrap_or("missing"),
        ListingKind::Model if store_owns_model(&t.models_dir, r) => "not_selected",
        _ if r.pending && installing == Some(r.listing_id.as_str()) => "installing",
        _ => "missing",
    };
    InstalledView {
        listing_id: r.listing_id.clone(),
        kind: r.kind.as_str().to_string(),
        name: r.name.clone(),
        version: r.version.clone(),
        target: r.target.clone(),
        installed_at: r.installed_at,
        state: state.to_string(),
    }
}

/// Whether the model file a Store record names still holds the bytes that record installed: the
/// file is verified-Ready, its verify matched the record's pinned SHA-256, and its size is the
/// record's. A file of the same name with other bytes (a replacement made in Models after the
/// Store's copy was deleted there) or with no recorded hash is never the Store's: it is not
/// reported as installed and Remove never deletes it.
fn store_owns_model(models_dir: &Path, r: &InstallRecord) -> bool {
    if r.sha256.is_empty() || r.size == 0 {
        return false;
    }
    let hash_ok = crate::model::verified_sha256(models_dir, &r.target)
        .is_some_and(|h| h.eq_ignore_ascii_case(&r.sha256));
    hash_ok && std::fs::metadata(models_dir.join(&r.target)).is_ok_and(|m| m.len() == r.size)
}

/// Every install the Store made, with its live state. Needs no key.
pub(crate) fn installed_sync(
    root: &Path,
    t: &Targets<'_>,
    now_ms: u64,
) -> Result<Vec<InstalledView>, Refusal> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let installing = installing_listing(root);
    reconcile_models(root, t, installing.as_deref())?;
    let st = read_store_file(root)?;
    Ok(st
        .installs
        .iter()
        .map(|r| installed_view(r, t, now_ms, installing.as_deref()))
        .collect())
}

/// Remove the Store's own unfinished model file `file` (the GGUF, if it is NOT verified, and its
/// side files) under the download lock. A verified model is never removed here.
fn remove_unverified_model(models_dir: &Path, file: &str) -> Result<(), Refusal> {
    let file = crate::model_delete::file_from_id(file)
        .map_err(|e| refuse("install_failed", e.to_string()))?;
    let _lock = crate::model::DownloadGuard::acquire(&models_dir.join(format!("{file}.part")))
        .map_err(|_| refuse("in_use", "A download of this model is running"))?;
    if crate::model::is_file_ready(models_dir, &file) {
        return Ok(());
    }
    let mut paths = vec![models_dir.join(&file)];
    paths.extend(
        crate::model_delete::SIDE_SUFFIXES
            .iter()
            .map(|s| models_dir.join(format!("{file}{s}"))),
    );
    for p in paths {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_refusal(e)),
        }
    }
    Ok(())
}

/// Called with the Store lock held (from [`installed_sync`], so on the first read after a start).
/// A model install the app left between the download's rename and its verify (a pending record,
/// the file on disk, no verified flag, no install running) is checked again by its pinned hash:
/// it is confirmed when it matches, and when it does not (or cannot be checked) the file and its
/// record are removed together, so the listing can be installed again.
fn reconcile_models(root: &Path, t: &Targets<'_>, installing: Option<&str>) -> Result<(), Refusal> {
    let mut st = read_store_file(root)?;
    let mut changed = false;
    let mut keep = Vec::with_capacity(st.installs.len());
    for mut r in std::mem::take(&mut st.installs) {
        let interrupted = r.kind == ListingKind::Model
            && r.pending
            && installing != Some(r.listing_id.as_str())
            && t.models_dir.join(&r.target).is_file()
            && !crate::model::is_file_ready(&t.models_dir, &r.target);
        if !interrupted {
            keep.push(r);
            continue;
        }
        changed = true;
        let verified = !r.sha256.is_empty()
            && r.size > 0
            && crate::model::ModelManager::new(
                t.models_dir.clone(),
                (t.model_transport)(&r.url),
                r.sha256.clone(),
                r.size,
            )
            .with_file(r.target.clone())
            .verify()
            .is_ok();
        if verified {
            r.pending = false;
            keep.push(r);
        } else {
            remove_unverified_model(&t.models_dir, &r.target)?;
        }
    }
    st.installs = keep;
    if changed {
        st.version = STATE_VERSION;
        write_store_file(root, &st)?;
    }
    Ok(())
}

fn take_stage(root: &Path, stage_id: &str, now: u64) -> Result<StagedFile, Refusal> {
    let gone = || {
        refuse(
            "stage_expired",
            "This review is no longer open. Review the listing again.",
        )
    };
    if !is_stage_id(stage_id) {
        return Err(gone());
    }
    let path = root.join(STAGING_DIR).join(format!("{stage_id}.json"));
    let text = std::fs::read(&path).map_err(|_| gone())?;
    // Single use: the stage is gone from disk before anything installs.
    std::fs::remove_file(&path).map_err(io_refusal)?;
    let staged: StagedFile = serde_json::from_slice(&text).map_err(|_| gone())?;
    if staged.stage_id != stage_id || now >= staged.created_at.saturating_add(STAGE_TTL_SECS) {
        return Err(gone());
    }
    Ok(staged)
}

/// The member approved the install sheet for `stage_id`: install the listing PAUSED.
#[allow(clippy::too_many_arguments)]
pub(crate) fn install_sync(
    root: &Path,
    key: &Result<TrustRoot, CatalogError>,
    t: &Targets<'_>,
    rank: i32,
    stage_id: &str,
    now: u64,
    now_ms: u64,
    offset_min: i32,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<InstalledView, Refusal> {
    let key = key.as_ref().map_err(catalog_refusal)?;
    let slot = InstallSlot::take(root)?;
    let staged = take_stage(root, stage_id, now)?;
    slot.set_listing(&staged.listing.id);
    // The listing must still be in the verified catalog exactly as the member reviewed it.
    let vc = cached_catalog(root, key, now)?;
    let listing = vc.catalog().listing(&staged.listing.id).ok_or_else(|| {
        refuse(
            "not_listed",
            format!("{} left the Store catalog", staged.listing.name),
        )
    })?;
    if *listing != staged.listing {
        return Err(refuse(
            "changed",
            "The listing changed since you reviewed it. Review it again.",
        ));
    }
    {
        let st = read_store_file(root)?;
        require_installable(listing, rank, &st)?;
    }
    // Everything that can be checked before anything is written, and the target's name.
    let plan = match listing.kind {
        ListingKind::Daemon => Plan::Daemon(
            crate::daemons::DaemonBook::fresh_id(),
            daemon_input(listing, staged.payload.as_deref())?,
            // The permissions the member approved on the install sheet bind the daemon's tools.
            tools_for_permissions(
                &listing
                    .permissions
                    .iter()
                    .map(|p| p.id.clone())
                    .collect::<Vec<_>>(),
            ),
        ),
        ListingKind::Model => Plan::Model(model_target(listing, t)?),
        _ => return Err(refuse("not_installable", "this kind does not install yet")),
    };
    let target = match &plan {
        Plan::Daemon(id, _, _) => id.clone(),
        Plan::Model(file) => file.clone(),
    };
    let mut record = InstallRecord {
        listing_id: listing.id.clone(),
        kind: listing.kind,
        name: listing.name.clone(),
        version: listing.version.clone(),
        target,
        installed_at: now,
        pending: true,
        sha256: listing.artifact.sha256.clone(),
        size: listing.artifact.size,
        url: listing.artifact.url.clone(),
    };
    // 1. The record first: if it cannot be saved, nothing is created, so the Store never
    //    leaves an install it does not track.
    update_installs(root, |installs| {
        installs.push(record.clone());
    })?;
    // 2. The target. On failure the record is taken back out (if even that fails, the record
    //    shows `missing` and Remove clears it).
    let created = match plan {
        Plan::Daemon(id, input, tools) => create_daemon(id, input, tools, t, now_ms, offset_min),
        Plan::Model(_) => install_model(listing, t, on_progress),
    };
    if let Err(e) = created {
        // A model download that failed partway leaves its `.part` (it can be GBs). It is removed
        // under the download lock before the record goes, so no bytes are left untracked. When
        // the lock is held, a running download owns those bytes (the Models screen shows it);
        // when removal fails otherwise, the record stays pending so Store > Remove can retry.
        let keep_record = listing.kind == ListingKind::Model
            && matches!(
                remove_unverified_model(&t.models_dir, &record.target),
                Err(ref r) if r.code != "in_use"
            );
        if !keep_record {
            let _ = update_installs(root, |installs| {
                installs.retain(|i| !(i.listing_id == record.listing_id && i.pending));
            });
        }
        return Err(e);
    }
    // 3. Confirm. A failed confirming write keeps the pending record, which still tracks the
    //    install (its target exists), so the install is reported as done.
    record.pending = false;
    let _ = update_installs(root, |installs| {
        for i in installs.iter_mut() {
            if i.listing_id == record.listing_id {
                i.pending = false;
            }
        }
    });
    Ok(installed_view(&record, t, now_ms, None))
}

enum Plan {
    Daemon(String, crate::daemons::DaemonInput, Vec<String>),
    Model(String),
}

/// Load, change and store the install list under the Store lock.
fn update_installs(root: &Path, f: impl FnOnce(&mut Vec<InstallRecord>)) -> Result<(), Refusal> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut st = read_store_file(root)?;
    st.version = STATE_VERSION;
    f(&mut st.installs);
    write_store_file(root, &st)
}

/// The daemon a listing installs, from the staged template (re-hashed) and the declared budget.
fn daemon_input(
    listing: &Listing,
    payload: Option<&str>,
) -> Result<crate::daemons::DaemonInput, Refusal> {
    let text = payload.ok_or_else(|| {
        refuse(
            "stage_expired",
            "The reviewed template is missing. Review the listing again.",
        )
    })?;
    // The staged bytes were hash-checked at stage time; check them again before use.
    if hex::encode(sha2::Sha256::digest(text.as_bytes())) != listing.artifact.sha256 {
        return Err(refuse(
            "hash_failed",
            "The staged template does not match the signed catalog",
        ));
    }
    let tpl = parse_daemon_template(text.as_bytes())?;
    let b = listing
        .budget
        .as_ref()
        .ok_or_else(|| refuse("invalid", "a daemon listing must declare a budget"))?;
    Ok(crate::daemons::DaemonInput {
        id: None,
        name: tpl.name,
        prompt: tpl.prompt,
        schedule: tpl.schedule,
        budget: Some(crate::daemons::BudgetInput {
            max_runs_per_day: b.max_runs_per_day,
            max_tokens_per_day: b.max_tokens_per_day,
            max_tokens_per_run: b.max_tokens_per_run,
            max_spend_salt: b.max_spend_salt.clone(),
        }),
    })
}

fn create_daemon(
    id: String,
    input: crate::daemons::DaemonInput,
    allowed_tools: Vec<String>,
    t: &Targets<'_>,
    now_ms: u64,
    offset_min: i32,
) -> Result<(), Refusal> {
    // Created paused in one write: the daemon never exists unpaused on disk.
    crate::daemons::with_book_file(&t.daemons_path, true, |book| {
        book.create_paused_with_id(id, input, allowed_tools, now_ms, offset_min)
            .map(|_| ())
    })
    .map_err(|e| refuse("install_failed", format!("Could not add the daemon: {e}")))
}

/// The model file a listing installs as, refused when a file of that name is already here.
fn model_target(listing: &Listing, t: &Targets<'_>) -> Result<String, Refusal> {
    let file = model_file_for(listing)?;
    if t.models_dir.join(&file).exists() {
        return Err(refuse(
            "already_installed",
            format!("{file} is already on this machine; manage it in Models"),
        ));
    }
    Ok(file)
}

fn install_model(
    listing: &Listing,
    t: &Targets<'_>,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<(), Refusal> {
    let file = model_target(listing, t)?;
    std::fs::create_dir_all(&t.models_dir).map_err(io_refusal)?;
    let mgr = crate::model::ModelManager::new(
        t.models_dir.clone(),
        (t.model_transport)(&listing.artifact.url),
        listing.artifact.sha256.clone(),
        listing.artifact.size,
    )
    .with_file(file.clone());
    mgr.download_with_progress(on_progress)
        .map_err(|e| match e {
            crate::model::ModelError::SizeMismatch { .. }
            | crate::model::ModelError::ShortStream { .. }
            | crate::model::ModelError::Overshoot { .. }
            | crate::model::ModelError::BadMagic => refuse("hash_failed", e.to_string()),
            other => refuse("install_failed", other.to_string()),
        })?;
    mgr.verify().map_err(|e| match e {
        crate::model::ModelError::ChecksumMismatch
        | crate::model::ModelError::SizeMismatch { .. } => refuse(
            "hash_failed",
            "The model does not match the SHA-256 in the signed catalog; it was not kept",
        ),
        other => refuse("install_failed", other.to_string()),
    })?;
    Ok(())
}

/// Remove an install: one call, no key needed. A target already gone counts as removed.
pub(crate) fn remove_sync(
    root: &Path,
    t: &Targets<'_>,
    listing_id: &str,
    now_ms: u64,
) -> Result<String, Refusal> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // K5-F: never pull a listing out from under its own install. Checked under the Store lock,
    // which the install also holds when it saves the record it is working on.
    if installing_listing(root).as_deref() == Some(listing_id) {
        return Err(refuse(
            "install_running",
            "It is still installing. Remove it when the install has finished.",
        ));
    }
    let mut st = read_store_file(root)?;
    let idx = st
        .installs
        .iter()
        .position(|i| i.listing_id == listing_id)
        .ok_or_else(|| {
            refuse(
                "not_installed",
                "That listing is not installed from the Store",
            )
        })?;
    let rec = st.installs[idx].clone();
    match rec.kind {
        ListingKind::Daemon => {
            // A daemon with a run in flight is not deleted under it: the app stops and waits for
            // the run first (the Store's Remove does), and core refuses until then.
            crate::daemons::with_book_file(&t.daemons_path, true, |b| {
                match b.delete_idle(&rec.target, now_ms) {
                    Ok(()) => Ok(()),
                    Err(e) if e == "no such daemon" => Ok(()),
                    Err(e) => Err(e),
                }
            })
            .map_err(|e| {
                if e == crate::daemons::DAEMON_RUN_IN_FLIGHT {
                    refuse(
                        "daemon_running",
                        "Its run is still going. Stop the run, then remove it.",
                    )
                } else {
                    refuse(
                        "install_failed",
                        format!("Could not remove the daemon: {e}"),
                    )
                }
            })?
        }
        ListingKind::Model if store_owns_model(&t.models_dir, &rec) => {
            match crate::model_delete::delete_and_forget(
                &t.models_dir,
                t.bundled_models.as_deref(),
                &rec.target,
                t.usage,
                t.selection,
            ) {
                // Gone between the check and the delete: nothing left to remove.
                Ok(_) | Err(crate::model_delete::DeleteRefusal::Unknown(_)) => {}
                Err(e @ crate::model_delete::DeleteRefusal::InUse(_))
                | Err(e @ crate::model_delete::DeleteRefusal::Downloading) => {
                    return Err(refuse("in_use", e.to_string()))
                }
                Err(e) => return Err(refuse("install_failed", e.to_string())),
            }
        }
        // Not the Store's bytes on disk. For the Store's own unfinished install (a pending
        // record) its unverified file and side files go with the record; a verified file is
        // never removed here. Otherwise the target is gone or belongs to a replacement, and only
        // the stale record goes (the catch-all arm below).
        ListingKind::Model if rec.pending => {
            remove_unverified_model(&t.models_dir, &rec.target)?;
        }
        _ => {}
    }
    st.installs.remove(idx);
    write_store_file(root, &st)?;
    Ok(format!("{} removed", rec.name))
}

// ---------------------------------------------------------------------------
// Commands (all async, off the main thread; main window only)
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn store_root<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager as _;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("store"))
}

fn models_dirs<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    use tauri::Manager as _;
    let models = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let bundled = app.path().resource_dir().ok().map(|r| r.join("models"));
    Ok((models, bundled))
}

fn rank_now(app: &tauri::AppHandle, now: u64) -> Result<i32, String> {
    let auth = tauri::Manager::try_state::<crate::oidc::AuthState>(app)
        .ok_or_else(|| "internal: auth state unavailable".to_string())?;
    let s = auth.0.status();
    Ok(member_rank(
        s.signed_in,
        s.tier.as_deref(),
        s.expires_at.as_deref(),
        now,
    ))
}

fn with_targets<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&Targets<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let serve = tauri::Manager::try_state::<crate::serve::ServeState>(app)
        .ok_or_else(|| "internal: serve state unavailable".to_string())?;
    let usage = crate::model_delete::LiveUsage { serve: &serve.0 };
    let (models_dir, bundled_models) = models_dirs(app)?;
    let transport = |url: &str| -> Box<dyn crate::model::ModelTransport> {
        Box::new(crate::model::UreqModelTransport::new(url))
    };
    let t = Targets {
        daemons_path: crate::daemons::book_path(app)?,
        models_dir,
        bundled_models,
        usage: &usage,
        selection: &serve.0,
        model_transport: &transport,
    };
    f(&t)
}

/// **store_catalog**: fetch and verify the signed catalog. Refuses at once with the key slot
/// empty.
#[tauri::command]
pub async fn store_catalog(app: tauri::AppHandle) -> Result<CatalogView, String> {
    crate::blocking::off_main(move || {
        let now = now_secs();
        let root = store_root(&app)?;
        let rank = rank_now(&app, now)?;
        Ok(catalog_view_sync(
            &root,
            &catalog_trust_root(),
            &HttpsFetcher::default(),
            rank,
            now,
        ))
    })
    .await
}

/// **store_stage**: review one listing (the install sheet's data).
#[tauri::command]
pub async fn store_stage(
    app: tauri::AppHandle,
    listing_id: String,
) -> Result<Outcome<StagedView>, String> {
    crate::blocking::off_main(move || {
        let now = now_secs();
        let root = store_root(&app)?;
        let rank = rank_now(&app, now)?;
        let (models_dir, _) = models_dirs(&app)?;
        Ok(stage_sync(
            &root,
            &catalog_trust_root(),
            &HttpsFetcher::default(),
            &models_dir,
            rank,
            &listing_id,
            now,
        )
        .into())
    })
    .await
}

/// **store_install**: the member approved the install sheet; install PAUSED.
#[tauri::command]
pub async fn store_install(
    app: tauri::AppHandle,
    stage_id: String,
    now_ms: u64,
    offset_min: i32,
) -> Result<Outcome<InstalledView>, String> {
    crate::blocking::off_main(move || {
        use tauri::Emitter as _;
        let now = now_secs();
        let root = store_root(&app)?;
        let rank = rank_now(&app, now)?;
        let progress_app = app.clone();
        let sid = stage_id.clone();
        let mut last_pct: i64 = -1;
        let mut on_progress = move |got: u64, total: u64| {
            let pct = if total > 0 {
                ((got as u128 * 100) / total as u128) as i64
            } else {
                0
            };
            if pct != last_pct {
                last_pct = pct;
                let _ = progress_app.emit(
                    "store://install-progress",
                    serde_json::json!({ "stageId": sid, "pct": pct }),
                );
            }
        };
        with_targets(&app, |t| {
            Ok(install_sync(
                &root,
                &catalog_trust_root(),
                t,
                rank,
                &stage_id,
                now,
                now_ms,
                offset_min,
                &mut on_progress,
            )
            .into())
        })
    })
    .await
}

/// **store_installed**: what the Store installed, with live state. Needs no key.
#[tauri::command]
pub async fn store_installed(
    app: tauri::AppHandle,
    now_ms: u64,
) -> Result<Vec<InstalledView>, String> {
    crate::blocking::off_main(move || {
        let root = store_root(&app)?;
        with_targets(&app, |t| {
            installed_sync(&root, t, now_ms).map_err(|r| r.message)
        })
    })
    .await
}

/// **store_remove**: remove one install. Needs no key.
#[tauri::command]
pub async fn store_remove(
    app: tauri::AppHandle,
    listing_id: String,
) -> Result<Outcome<String>, String> {
    crate::blocking::off_main(move || {
        let root = store_root(&app)?;
        let now_ms = now_secs().saturating_mul(1000);
        with_targets(&app, |t| {
            Ok(remove_sync(&root, t, &listing_id, now_ms).into())
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("store_catalog_tests.rs");
}
