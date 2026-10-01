//! citrate-core — HUP-S10.5 "delete my local data" (uninstall support).
//!
//! Two steps, both built in Rust from the app's own path API, never from paths the webview
//! supplies:
//!
//! 1. **Dry run** (`local_data_plan`): every top-level item under each app-owned folder,
//!    with its size and whether it will be deleted or kept, plus every keychain entry the
//!    app owns and whether it is present. Nothing changes.
//! 2. **Delete** (`local_data_delete`): needs the typed confirmation phrase (and a second
//!    phrase to include the wallet), stops every sidecar, rebuilds the same plan, deletes
//!    exactly the planned items, reports what failed, and closes the app.
//!
//! Safety rails:
//! - A folder is app-owned only if its last path component is the bundle id
//!   (`ai.citrate.core`); anything else is skipped with a note, never deleted.
//! - Symlinks are removed as links and never followed.
//! - The wallet vault (`custody.enc` and the `ai.citrate.core.custody` keychain entries) is
//!   kept unless the member ticks "also delete my wallet" and types the second phrase. The
//!   legacy `ai.citrate.core` service's `custody-*` accounts belong to another Citrate app
//!   and are never listed or touched.
//! - Signs nothing, sends nothing (Rule 3).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The bundle id every app-owned folder is named after (`tauri.conf.json` identifier).
pub(crate) const BUNDLE_ID: &str = "ai.citrate.core";
/// The phrase the member types to confirm.
pub(crate) const CONFIRM_PHRASE: &str = "delete my local data";
/// The second phrase that also deletes the wallet.
pub(crate) const WALLET_CONFIRM_PHRASE: &str = "delete my wallet";

const LEGACY_SERVICE: &str = "ai.citrate.core";
const CUSTODY_SERVICE: &str = crate::CUSTODY_KEYRING_SERVICE;
/// Files in the data dir that hold the wallet vault.
const WALLET_FILES: &[&str] = &["custody.enc", "custody.enc.tmp"];
/// The data-dir folder that holds downloaded models.
const MODELS_DIR: &str = "models";

/// Keychain accounts this app owns on the legacy service (not the wallet).
const OWNED_LEGACY_ACCOUNTS: &[(&str, &str)] = &[
    ("node-storage-key", "Node storage key"),
    ("memory-store-key", "Journal and memory key"),
    (
        "comms-member-key-v2",
        "Messaging key (re-derived from your wallet)",
    ),
    ("comms-member-key", "Older messaging key (no longer used)"),
    ("ai-default", "Default AI provider choice"),
    ("ai-openai", "OpenAI provider key"),
    ("ai-gateway", "Citrate gateway key"),
    ("ai-custom", "Custom AI provider key"),
];

/// Keychain accounts of the wallet vault (custody service).
const WALLET_ACCOUNTS: &[(&str, &str)] = &[
    ("custody-master-key", "Wallet vault master key"),
    ("custody-auto-passphrase", "Wallet vault device passphrase"),
    ("custody-generation", "Wallet vault version anchor"),
    ("custody-lockout-generation", "Wallet vault lockout anchor"),
];

/// What the member chose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteOptions {
    #[serde(rename = "includeWallet")]
    pub include_wallet: bool,
    #[serde(rename = "keepModels")]
    pub keep_models: bool,
}

/// Delete or keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Delete,
    Keep,
}

/// The kind of app folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RootKind {
    Data,
    Cache,
    Logs,
    Webview,
}

/// The app's folders: the data dir plus any others.
#[derive(Debug, Clone)]
pub(crate) struct AppRoots {
    pub data_dir: PathBuf,
    pub others: Vec<(PathBuf, RootKind)>,
}

/// One item on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanEntry {
    pub path: String,
    pub kind: RootKind,
    pub bytes: u64,
    pub action: Action,
    /// Why it is kept, when it is.
    pub reason: Option<String>,
}

/// One keychain entry the app owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeychainEntry {
    pub service: String,
    pub account: String,
    pub label: String,
    /// `None` when the keychain could not be asked.
    pub present: Option<bool>,
    pub action: Action,
}

/// The dry-run result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataPlan {
    pub options: DeleteOptions,
    pub entries: Vec<PlanEntry>,
    pub keychain: Vec<KeychainEntry>,
    #[serde(rename = "deleteBytes")]
    pub delete_bytes: u64,
    #[serde(rename = "keepBytes")]
    pub keep_bytes: u64,
    #[serde(rename = "confirmPhrase")]
    pub confirm_phrase: &'static str,
    #[serde(rename = "walletConfirmPhrase")]
    pub wallet_confirm_phrase: Option<&'static str>,
    pub notes: Vec<String>,
}

/// A failed deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub item: String,
    pub error: String,
}

/// What the delete did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DeleteReport {
    pub deleted: Vec<String>,
    pub failed: Vec<Failure>,
    #[serde(rename = "keychainDeleted")]
    pub keychain_deleted: Vec<String>,
    #[serde(rename = "keychainFailed")]
    pub keychain_failed: Vec<Failure>,
}

/// Why a delete was refused before it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DataError {
    NotConfirmed,
    WalletNotConfirmed,
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfirmed => write!(f, "Type \"{CONFIRM_PHRASE}\" to confirm. Nothing was deleted."),
            Self::WalletNotConfirmed => write!(
                f,
                "To also delete your wallet, type \"{WALLET_CONFIRM_PHRASE}\" in the second box. Nothing was deleted."
            ),
        }
    }
}

/// The keychain operations deletion needs (the OS keychain in production, a fake in tests).
pub(crate) trait KeychainOps {
    /// Whether the entry exists; `None` when the keychain cannot be asked.
    fn present(&self, service: &str, account: &str) -> Option<bool>;
    /// A non-secret text value (only used for the default AI provider id).
    fn read_text(&self, service: &str, account: &str) -> Option<String>;
    /// Delete the entry (absent is fine).
    fn delete(&self, service: &str, account: &str) -> std::result::Result<(), String>;
}

/// The OS keychain.
pub(crate) struct OsKeychain;

impl KeychainOps for OsKeychain {
    fn present(&self, service: &str, account: &str) -> Option<bool> {
        let entry = keyring::Entry::new(service, account).ok()?;
        match entry.get_secret() {
            Ok(mut s) => {
                use zeroize::Zeroize;
                s.zeroize();
                Some(true)
            }
            Err(keyring::Error::NoEntry) => Some(false),
            Err(_) => None,
        }
    }
    fn read_text(&self, service: &str, account: &str) -> Option<String> {
        keyring::Entry::new(service, account)
            .ok()?
            .get_password()
            .ok()
    }
    fn delete(&self, service: &str, account: &str) -> std::result::Result<(), String> {
        let entry = keyring::Entry::new(service, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Normalize a typed phrase: trim, collapse spaces, lowercase.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Check the typed confirmations against the chosen options.
pub(crate) fn check_confirmations(
    opts: DeleteOptions,
    confirm: &str,
    wallet_confirm: Option<&str>,
) -> std::result::Result<(), DataError> {
    if normalize(confirm) != CONFIRM_PHRASE {
        return Err(DataError::NotConfirmed);
    }
    if opts.include_wallet
        && wallet_confirm.map(normalize).as_deref() != Some(WALLET_CONFIRM_PHRASE)
    {
        return Err(DataError::WalletNotConfirmed);
    }
    Ok(())
}

fn is_owned_root(p: &Path) -> bool {
    p.is_absolute() && p.file_name().and_then(|n| n.to_str()) == Some(BUNDLE_ID)
}

/// Bytes under `p`, counting files only and never following symlinks.
fn size_of(p: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            total = total.saturating_add(size_of(&e.path()));
        }
    }
    total
}

fn valid_provider_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id != "default"
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// Build the dry-run plan. Reads sizes and keychain presence only; changes nothing.
pub(crate) fn build_plan(
    roots: &AppRoots,
    keychain: &dyn KeychainOps,
    opts: DeleteOptions,
) -> DataPlan {
    let mut notes = Vec::new();
    let mut entries = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let all = std::iter::once((roots.data_dir.clone(), RootKind::Data))
        .chain(roots.others.iter().cloned());
    for (root, kind) in all {
        // A folder inside one already listed (the Linux and Windows log dir sits in the data
        // dir) is covered by that entry.
        if seen.iter().any(|s| root.starts_with(s)) {
            continue;
        }
        seen.push(root.clone());
        if !is_owned_root(&root) {
            notes.push(format!(
                "skipped {} because it is not named for this app",
                root.display()
            ));
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue; // absent folder: nothing to delete
        };
        let mut children: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children {
            let name = child.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let (action, reason) = if kind == RootKind::Data
                && WALLET_FILES.contains(&name)
                && !opts.include_wallet
            {
                (
                    Action::Keep,
                    Some(
                        "your wallet vault, which also holds your sign-in session and connected-account tokens (kept unless you also delete the wallet)"
                            .to_string(),
                    ),
                )
            } else if kind == RootKind::Data && name == MODELS_DIR && opts.keep_models {
                (
                    Action::Keep,
                    Some("downloaded models (you chose to keep them)".to_string()),
                )
            } else {
                (Action::Delete, None)
            };
            entries.push(PlanEntry {
                path: child.display().to_string(),
                kind,
                bytes: size_of(&child),
                action,
                reason,
            });
        }
    }

    let mut keychain_entries = Vec::new();
    let mut legacy: Vec<(String, String)> = OWNED_LEGACY_ACCOUNTS
        .iter()
        .map(|(a, l)| (a.to_string(), l.to_string()))
        .collect();
    if let Some(id) = keychain.read_text(LEGACY_SERVICE, "ai-default") {
        let id = id.trim().to_string();
        let account = format!("ai-{id}");
        if valid_provider_id(&id) && !legacy.iter().any(|(a, _)| *a == account) {
            legacy.push((account, format!("AI provider key ({id})")));
        }
    }
    for (account, label) in legacy {
        keychain_entries.push(KeychainEntry {
            present: keychain.present(LEGACY_SERVICE, &account),
            service: LEGACY_SERVICE.to_string(),
            account,
            label,
            action: Action::Delete,
        });
    }
    for (account, label) in WALLET_ACCOUNTS {
        keychain_entries.push(KeychainEntry {
            present: keychain.present(CUSTODY_SERVICE, account),
            service: CUSTODY_SERVICE.to_string(),
            account: account.to_string(),
            label: label.to_string(),
            action: if opts.include_wallet {
                Action::Delete
            } else {
                Action::Keep
            },
        });
    }
    if keychain_entries.iter().any(|k| k.present.is_none()) {
        notes.push("the system keychain could not be asked, so presence is unknown; deletion will still try each entry".to_string());
    }

    let delete_bytes = entries
        .iter()
        .filter(|e| e.action == Action::Delete)
        .map(|e| e.bytes)
        .sum();
    let keep_bytes = entries
        .iter()
        .filter(|e| e.action == Action::Keep)
        .map(|e| e.bytes)
        .sum();
    DataPlan {
        options: opts,
        entries,
        keychain: keychain_entries,
        delete_bytes,
        keep_bytes,
        confirm_phrase: CONFIRM_PHRASE,
        wallet_confirm_phrase: opts.include_wallet.then_some(WALLET_CONFIRM_PHRASE),
        notes,
    }
}

fn remove_path(p: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(p)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// Delete exactly the plan's `Delete` items, then its `Delete` keychain entries. Every item
/// is attempted; failures are reported, never hidden.
pub(crate) fn execute_plan(plan: &DataPlan, keychain: &dyn KeychainOps) -> DeleteReport {
    let mut report = DeleteReport::default();
    for e in plan.entries.iter().filter(|e| e.action == Action::Delete) {
        let p = Path::new(&e.path);
        // Defence in depth: only a direct child of an app-owned folder.
        if !p.parent().is_some_and(is_owned_root) {
            report.failed.push(Failure {
                item: e.path.clone(),
                error: "not inside an app folder".into(),
            });
            continue;
        }
        match remove_path(p) {
            Ok(()) => report.deleted.push(e.path.clone()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                report.deleted.push(e.path.clone())
            }
            Err(err) => report.failed.push(Failure {
                item: e.path.clone(),
                error: err.to_string(),
            }),
        }
    }
    for k in plan.keychain.iter().filter(|k| k.action == Action::Delete) {
        let item = format!("{}/{}", k.service, k.account);
        match keychain.delete(&k.service, &k.account) {
            Ok(()) => report.keychain_deleted.push(item),
            Err(error) => report.keychain_failed.push(Failure { item, error }),
        }
    }
    report
}

/// The app's folders from the Tauri path API, plus the macOS WebKit storage folder.
fn app_roots<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<AppRoots, String> {
    use tauri::Manager;
    let p = app.path();
    let data_dir = p.app_data_dir().map_err(|e| e.to_string())?;
    let mut others = Vec::new();
    for (dir, kind) in [
        (p.app_local_data_dir(), RootKind::Data),
        (p.app_config_dir(), RootKind::Data),
        (p.app_cache_dir(), RootKind::Cache),
        (p.app_log_dir(), RootKind::Logs),
    ] {
        if let Ok(d) = dir {
            if d != data_dir {
                others.push((d, kind));
            }
        }
    }
    if cfg!(target_os = "macos") {
        if let Ok(home) = p.home_dir() {
            others.push((
                home.join("Library").join("WebKit").join(BUNDLE_ID),
                RootKind::Webview,
            ));
        }
    }
    Ok(AppRoots { data_dir, others })
}

/// **local_data_plan** — the dry run: what would be deleted and kept. Changes nothing.
#[tauri::command]
pub async fn local_data_plan<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    options: DeleteOptions,
) -> std::result::Result<DataPlan, String> {
    crate::blocking::off_main(move || {
        let roots = app_roots(&app)?;
        Ok(build_plan(&roots, &OsKeychain, options))
    })
    .await
}

/// **local_data_delete** — check the typed confirmations, stop every sidecar, delete the
/// plan rebuilt here (never a webview-supplied list), and close the app shortly after
/// returning the report. The close skips the normal exit hooks on purpose so nothing writes
/// a fresh settings file back into the folders just deleted.
#[tauri::command]
pub async fn local_data_delete(
    app: tauri::AppHandle,
    options: DeleteOptions,
    confirm: String,
    wallet_confirm: Option<String>,
) -> std::result::Result<DeleteReport, String> {
    check_confirmations(options, &confirm, wallet_confirm.as_deref()).map_err(|e| e.to_string())?;
    crate::blocking::off_main(move || {
        crate::shutdown_all_sidecars(&app);
        let roots = app_roots(&app)?;
        let plan = build_plan(&roots, &OsKeychain, options);
        let report = execute_plan(&plan, &OsKeychain);
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(2500));
            std::process::exit(0);
        });
        Ok(report)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("local_data_tests.rs");
}
