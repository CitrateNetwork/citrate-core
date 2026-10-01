//! HUP-S5.5 + S6.1 — signed first-run components (the `citrate-components` crate) in the app.
//!
//! Three commands, all off the main thread:
//!
//! - `components_status`: the key slot, update freshness (CVE SLA, client side), what is
//!   installed, and the toolchain bundle with an honest per-platform state. Read-only; it does
//!   not create anything on disk.
//! - `components_update`: refuses at once while the production key slot is empty (the @rule8
//!   key ceremony has not happened), before any network or disk access. With a key: fetch the
//!   manifest and its signature, verify, record, then download to staging, verify, unpack,
//!   health-check and swap.
//! - `components_rollback`: makes the previous version current again.
//!
//! Rule 3: no key material here; the component key is a public key pinned in the crate.
use std::path::{Path, PathBuf};

use citrate_components::bundle::{ArtifactStatus, Bundle};
use citrate_components::fetch::{Fetcher, HttpsFetcher};
use citrate_components::install::{EntrypointsPresent, InstallOutcome, Store, StoreState};
use citrate_components::key::TrustRoot;
use citrate_components::manifest::{verify_manifest, MAX_MANIFEST_BYTES};
use citrate_components::platform::Platform;
use citrate_components::policy::{browser_may_open_web, freshness, Freshness, Severity, SlaPolicy};
use serde::Serialize;

/// Where the signed component manifest will be published (CDN, beside the app downloads). Not
/// published yet: the first one is signed at the key ceremony.
pub const COMPONENT_MANIFEST_URL: &str =
    "https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/components/stable/manifest.json";
pub const COMPONENT_MANIFEST_SIG_URL: &str =
    "https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/components/stable/manifest.json.minisig";
const MAX_SIG_BYTES: u64 = 4096;
const BUNDLE_JSON: &str = include_str!("../../components/toolchain-bundle.json");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledView {
    pub name: String,
    pub version: String,
    pub previous: Option<String>,
    pub installed_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleToolView {
    pub name: String,
    pub version: String,
    pub license: String,
    /// Platforms with a measured hash.
    pub measured_platforms: Vec<String>,
    /// This machine's entry: measured, to_be_measured, to_be_built, upstream_unavailable or
    /// none.
    pub this_platform: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlaView {
    pub critical_hours: u64,
    pub high_days: u64,
    pub medium_days: u64,
    pub low_days: u64,
    pub stale_after_days: u64,
    pub pending_owner_signoff: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsStatus {
    pub key_configured: bool,
    pub key_fingerprint: Option<String>,
    pub key_note: String,
    pub platform: Option<String>,
    /// never_checked | fresh | stale | expired
    pub freshness: String,
    pub manifest_age_secs: Option<u64>,
    pub browser_may_open_web: bool,
    pub installed: Vec<InstalledView>,
    pub bundle: Vec<BundleToolView>,
    pub libraries: Vec<LibraryView>,
    pub sla: SlaView,
    pub manifest_url: String,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn components_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager as _;
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(base.join("components"))
}

/// The recorded state without creating the store.
fn read_state(root: &Path) -> Result<StoreState, String> {
    if !root.join("state.json").exists() {
        return Ok(StoreState::default());
    }
    Store::open(root)
        .and_then(|s| s.state())
        .map_err(|e| e.to_string())
}

pub(crate) fn components_status_sync(root: &Path, now: u64) -> Result<ComponentsStatus, String> {
    let (key_configured, key_fingerprint, key_note) = match TrustRoot::production() {
        Ok(k) => (
            true,
            Some(k.fingerprint().to_string()),
            "Component updates are signed with the pinned component key.".to_string(),
        ),
        Err(e) => (false, None, e.to_string()),
    };
    let st = read_state(root)?;
    let f = freshness(st.last_manifest.as_ref(), now);
    let (fresh_label, age) = match &f {
        Freshness::NeverChecked => ("never_checked", None),
        Freshness::Fresh { age_secs } => ("fresh", Some(*age_secs)),
        Freshness::Stale { age_secs } => ("stale", Some(*age_secs)),
        Freshness::Expired => ("expired", None),
    };
    let platform = Platform::current();
    let bundle = Bundle::parse(BUNDLE_JSON).map_err(|e| e.to_string())?;
    let tools = bundle
        .tools
        .iter()
        .map(|t| BundleToolView {
            name: t.name.clone(),
            version: t.version.clone(),
            license: t.license.clone(),
            measured_platforms: t
                .artifacts
                .iter()
                .filter(|(_, a)| a.status == ArtifactStatus::Measured)
                .map(|(p, _)| p.clone())
                .collect(),
            this_platform: platform
                .and_then(|p| t.artifacts.get(p.as_str()))
                .map(|a| {
                    serde_json::to_value(a.status)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default()
                })
                .unwrap_or_else(|| "none".to_string()),
        })
        .collect();
    let libraries = bundle
        .libraries
        .archives
        .iter()
        .map(|(n, a)| LibraryView {
            name: n.clone(),
            sha256: a.sha256.clone(),
        })
        .collect();
    let p = SlaPolicy::default();
    Ok(ComponentsStatus {
        key_configured,
        key_fingerprint,
        key_note,
        platform: platform.map(|p| p.as_str().to_string()),
        freshness: fresh_label.to_string(),
        manifest_age_secs: age,
        browser_may_open_web: browser_may_open_web(&f),
        installed: st
            .components
            .iter()
            .map(|(n, c)| InstalledView {
                name: n.clone(),
                version: c.current.version.clone(),
                previous: c.previous.as_ref().map(|v| v.version.clone()),
                installed_at: c.current.installed_at,
            })
            .collect(),
        bundle: tools,
        libraries,
        sla: SlaView {
            critical_hours: p.deadline_secs(Severity::Critical) / 3_600,
            high_days: p.deadline_secs(Severity::High) / 86_400,
            medium_days: p.deadline_secs(Severity::Medium) / 86_400,
            low_days: p.deadline_secs(Severity::Low) / 86_400,
            stale_after_days: p.stale_after_secs / 86_400,
            pending_owner_signoff: p.pending_owner_signoff,
        },
        manifest_url: COMPONENT_MANIFEST_URL.to_string(),
    })
}

fn valid_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 48
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
    if ok {
        Ok(())
    } else {
        Err("invalid component name".to_string())
    }
}

/// One store writer at a time: an update runs `recover()` (which clears every staging
/// directory) and rewrites `state.json`, so a second update or a rollback running alongside it
/// could delete the first one's staging tree or lose its state write. A second writer is
/// refused rather than queued, so the member sees why.
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn with_store_lock(
    f: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    let _guard = match STORE_LOCK.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => {
            return Err("a component update or rollback is already running".to_string())
        }
    };
    f()
}

fn fetch_vec(f: &dyn Fetcher, url: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    f.fetch(url, max, &mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

pub(crate) fn components_update_sync(root: &Path, name: &str, now: u64) -> Result<String, String> {
    valid_name(name)?;
    // The key check comes first: with the slot empty nothing is fetched or written.
    let key = TrustRoot::production().map_err(|e| e.to_string())?;
    let platform =
        Platform::current().ok_or_else(|| "this platform has no component builds".to_string())?;
    with_store_lock(|| update_locked(root, name, platform, &key, now))
}

fn update_locked(
    root: &Path,
    name: &str,
    platform: Platform,
    key: &TrustRoot,
    now: u64,
) -> Result<String, String> {
    let store = Store::open(root).map_err(|e| e.to_string())?;
    store.recover().map_err(|e| e.to_string())?;
    let fetcher = HttpsFetcher::default();
    let manifest = fetch_vec(&fetcher, COMPONENT_MANIFEST_URL, MAX_MANIFEST_BYTES as u64)?;
    let sig_bytes = fetch_vec(&fetcher, COMPONENT_MANIFEST_SIG_URL, MAX_SIG_BYTES)?;
    let sig = String::from_utf8(sig_bytes)
        .map_err(|_| "the manifest signature is not text".to_string())?;
    let seen = store.state().map_err(|e| e.to_string())?.last_manifest;
    let vm =
        verify_manifest(&manifest, &sig, key, seen.as_ref(), now).map_err(|e| e.to_string())?;
    store.record_manifest(&vm).map_err(|e| e.to_string())?;
    match store
        .install(&vm, name, platform, key, &fetcher, &EntrypointsPresent, now)
        .map_err(|e| e.to_string())?
    {
        InstallOutcome::Installed { version, previous } => Ok(match previous {
            Some(p) => format!("{name} updated from {p} to {version}"),
            None => format!("{name} {version} installed"),
        }),
        InstallOutcome::AlreadyCurrent { version } => {
            Ok(format!("{name} {version} is already current"))
        }
    }
}

pub(crate) fn components_rollback_sync(root: &Path, name: &str) -> Result<String, String> {
    valid_name(name)?;
    if !root.join("state.json").exists() {
        return Err(format!("{name} has no previous version to roll back to"));
    }
    with_store_lock(|| {
        let store = Store::open(root).map_err(|e| e.to_string())?;
        let v = store.rollback(name).map_err(|e| e.to_string())?;
        Ok(format!("{name} rolled back to {}", v.version))
    })
}

#[tauri::command]
pub async fn components_status(app: tauri::AppHandle) -> Result<ComponentsStatus, String> {
    crate::blocking::off_main(move || {
        let root = components_root(&app)?;
        components_status_sync(&root, now_secs())
    })
    .await
}

#[tauri::command]
pub async fn components_update(app: tauri::AppHandle, name: String) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let root = components_root(&app)?;
        components_update_sync(&root, &name, now_secs())
    })
    .await
}

#[tauri::command]
pub async fn components_rollback(app: tauri::AppHandle, name: String) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let root = components_root(&app)?;
        components_rollback_sync(&root, &name)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("components_tests.rs");
}
