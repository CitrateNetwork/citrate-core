//! HUP-S9.3 (core): the member's opt-in for training on their verified Hermes conversations.
//!
//! Data sources (Rule 7):
//! - **Recording:** the Hermes sidecar (citrate-agent-runtime `agent-sidecar` `trajectory.rs`)
//!   exports a session's verified, redacted turns when the session closes, into the folder
//!   `CITRATE_HERMES_TRAJECTORIES` names. Only turns every workflow verifier passed are kept,
//!   every turn of a session that read untrusted content is dropped, and what is kept is
//!   redacted on this device (`citrate-agent-trajectory` `export_verified`).
//! - **The switch:** `<app local data>/hermes/trajectory-settings.json`, written only by
//!   [`trajectories_settings_set`] from the member's toggle in the federated rounds panel.
//!
//! **Default off, and off changes nothing.** With the switch off core passes
//! `CITRATE_HERMES_TRAJECTORIES` to the sidecar pinned empty, overriding any inherited value (so
//! nothing is recorded), and
//! [`build_round_dataset`] refuses before reading or writing anything. A missing, unreadable or
//! corrupt settings file counts as off. The switch applies the next time Hermes starts, like the
//! other sidecar switches.
//!
//! **What "on" allows.** Recording of verified, redacted turns into `hermes/trajectories/`, and,
//! when the member asks, assembling those turns into one training file for a round under
//! `hermes/fl-datasets/` (newest first, capped by the round's `maxTrajectories`). That file is
//! what the device training worker reads through `CITRATE_FL_DATASET` after compute-pool's
//! `citrate-fl-dataset` converts it. Taking part in a given round still needs that round's own
//! consent (D-29, `fl_rounds.rs`): this switch alone never shares anything, and nothing here
//! uploads.
//!
//! **Turning it off** stops recording at the next Hermes start and deletes the assembled round
//! files at once. The member can also delete the recorded conversations.
//!
//! Rule 3: nothing here signs or holds a key.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The settings file inside the Hermes data folder.
pub const SETTINGS_FILE: &str = "trajectory-settings.json";
/// The sidecar's opt-in variable: an absolute folder for verified trajectory exports.
pub const TRAJECTORIES_ENV: &str = "CITRATE_HERMES_TRAJECTORIES";
/// Where the sidecar writes exports, inside the Hermes data folder.
pub const TRAJECTORIES_DIR: &str = "trajectories";
/// Where assembled round training files go, inside the Hermes data folder.
pub const DATASETS_DIR: &str = "fl-datasets";
/// Every variable [`sidecar_env`] may emit; the Hermes manager drops anything else.
pub const SIDECAR_ENV_KEYS: &[&str] = &[TRAJECTORIES_ENV];
/// Upper bound on one export file read while assembling (the sidecar writes one per session).
pub const MAX_EXPORT_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The member's choice. Off by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrajectorySettings {
    pub enabled: bool,
    pub changed_at_ms: Option<u64>,
}

/// Read the settings. Missing = off. Unreadable or corrupt is an error the caller reports; every
/// path that acts on the settings treats it as off.
pub fn load(dir: &Path) -> Result<TrajectorySettings, String> {
    match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| {
            "the training-data settings file is not valid; training on conversations stays off"
                .to_string()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TrajectorySettings::default()),
        Err(_) => Err(
            "the training-data settings file could not be read; training on conversations stays off"
                .into(),
        ),
    }
}

/// Whether the switch is on (an error reads as off).
pub fn enabled(dir: &Path) -> bool {
    load(dir).map(|s| s.enabled).unwrap_or(false)
}

/// Store settings (written to a temp file, then renamed).
pub fn save(dir: &Path, s: &TrajectorySettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "cannot create the Hermes data folder".to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{SETTINGS_FILE}.tmp"));
    std::fs::write(&tmp, text)
        .map_err(|_| "cannot write the training-data settings".to_string())?;
    std::fs::rename(&tmp, dir.join(SETTINGS_FILE))
        .map_err(|_| "cannot write the training-data settings".to_string())
}

/// The sidecar environment for these settings. Off (or a relative folder) pins the variable
/// empty rather than leaving it out: the sidecar inherits core's own environment, so a value set
/// outside the member's switch (a shell or `launchctl` export) would otherwise turn recording on.
/// The sidecar treats an empty value as off (`TrajectoryConfig::from_value`).
pub fn sidecar_env(hermes_dir: &Path, s: &TrajectorySettings) -> Vec<(String, String)> {
    if !s.enabled || !hermes_dir.is_absolute() {
        return vec![(TRAJECTORIES_ENV.to_string(), String::new())];
    }
    vec![(
        TRAJECTORIES_ENV.to_string(),
        hermes_dir
            .join(TRAJECTORIES_DIR)
            .to_string_lossy()
            .into_owned(),
    )]
}

/// The production env source: the stored settings at each Hermes start (any error = off).
pub fn file_env_source(hermes_dir: PathBuf) -> crate::hermes_web::EnvSource {
    std::sync::Arc::new(move || {
        let s = load(&hermes_dir).unwrap_or_default();
        sidecar_env(&hermes_dir, &s)
    })
}

/// One export file, newest first by the `<session>-<unix_ms>.jsonl` stamp the sidecar writes.
fn export_files(dir: &Path) -> Vec<(u64, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        // `<session>-<ms>.report.json` (counts only) is not a training file.
        .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .map(|p| {
            let stamp = p
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.rsplit_once('-'))
                .and_then(|(_, ms)| ms.parse::<u64>().ok())
                .unwrap_or(0);
            (stamp, p)
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    files
}

/// A line of the sidecar's export, checked for shape: messages from the member, the model and
/// tools only (never a system prompt), and at least one passing verifier.
fn is_verified_export_line(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) else {
        return false;
    };
    let roles_ok = !msgs.is_empty()
        && msgs.iter().all(|m| {
            matches!(
                m.get("role").and_then(|r| r.as_str()),
                Some("user" | "assistant" | "tool")
            )
        });
    let verified = v
        .get("metadata")
        .and_then(|m| m.get("verifiers"))
        .and_then(|x| x.as_array())
        .is_some_and(|a| !a.is_empty());
    roles_ok && verified
}

/// What [`build_round_dataset`] assembled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetSummary {
    pub path: String,
    pub sha256: String,
    pub examples: u32,
    pub files_read: u32,
    /// Lines that were not a verified export line, left out.
    pub skipped_lines: u32,
    /// Verified lines beyond the cap, left out.
    pub over_cap: u32,
}

/// Assemble the newest verified, redacted turns (at most `max`) into one new training file for
/// a round. Refuses, before reading or writing anything, when the switch is off.
pub fn build_round_dataset(
    hermes_dir: &Path,
    max: u32,
    now_ms: u64,
) -> Result<DatasetSummary, String> {
    if !enabled(hermes_dir) {
        return Err(
            "training on your conversations is off; turn it on first. Nothing was read or written."
                .into(),
        );
    }
    if !(1..=crate::fl_rounds::MAX_TRAJECTORIES).contains(&max) {
        return Err(format!(
            "the trajectory cap must be from 1 to {}",
            crate::fl_rounds::MAX_TRAJECTORIES
        ));
    }
    let mut lines: Vec<String> = Vec::new();
    let (mut files_read, mut skipped, mut over_cap) = (0u32, 0u32, 0u32);
    for (_, path) in export_files(&hermes_dir.join(TRAJECTORIES_DIR)) {
        let len = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(u64::MAX);
        if len > MAX_EXPORT_FILE_BYTES {
            skipped = skipped.saturating_add(1);
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            skipped = skipped.saturating_add(1);
            continue;
        };
        files_read = files_read.saturating_add(1);
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if !is_verified_export_line(line) {
                skipped = skipped.saturating_add(1);
            } else if lines.len() < max as usize {
                lines.push(line.to_string());
            } else {
                over_cap = over_cap.saturating_add(1);
            }
        }
    }
    if lines.is_empty() {
        return Err(
            "there are no verified conversations to train on yet. Hermes records them from its next start, when a workflow's checks pass."
                .into(),
        );
    }
    let mut body = lines.join("\n");
    body.push('\n');
    let out_dir = hermes_dir.join(DATASETS_DIR);
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let path = out_dir.join(format!("{now_ms}.jsonl"));
    write_new_private(&path, body.as_bytes())?;
    Ok(DatasetSummary {
        path: path.to_string_lossy().into_owned(),
        sha256: hex::encode(Sha256::digest(body.as_bytes())),
        examples: u32::try_from(lines.len()).unwrap_or(u32::MAX),
        files_read,
        skipped_lines: skipped,
        over_cap,
    })
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    f.write_all(bytes).map_err(|e| e.to_string())
}

fn count_files(dir: &Path, ext: &str) -> u32 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    let p = e.path();
                    p.is_file() && p.extension().and_then(|x| x.to_str()) == Some(ext)
                })
                .count()
        })
        .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

fn remove_dir_files(dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot delete {}: {e}", dir.display())),
    }
}

/// What the panel renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryStatus {
    pub settings: TrajectorySettings,
    /// Session export files recorded so far.
    pub recorded_files: u32,
    /// Assembled round training files.
    pub datasets: u32,
    pub applies_on_restart: bool,
    pub load_error: Option<String>,
}

pub fn status(hermes_dir: &Path) -> TrajectoryStatus {
    let (settings, load_error) = match load(hermes_dir) {
        Ok(s) => (s, None),
        Err(e) => (TrajectorySettings::default(), Some(e)),
    };
    TrajectoryStatus {
        settings,
        recorded_files: count_files(&hermes_dir.join(TRAJECTORIES_DIR), "jsonl"),
        datasets: count_files(&hermes_dir.join(DATASETS_DIR), "jsonl"),
        applies_on_restart: true,
        load_error,
    }
}

/// Set the switch. Turning it off deletes the assembled round files at once and, when
/// `delete_recorded` is set, the recorded conversations too.
pub fn set(
    hermes_dir: &Path,
    on: bool,
    delete_recorded: bool,
    now_ms: u64,
) -> Result<TrajectoryStatus, String> {
    save(
        hermes_dir,
        &TrajectorySettings {
            enabled: on,
            changed_at_ms: Some(now_ms),
        },
    )?;
    if !on {
        remove_dir_files(&hermes_dir.join(DATASETS_DIR))?;
        if delete_recorded {
            remove_dir_files(&hermes_dir.join(TRAJECTORIES_DIR))?;
        }
    }
    Ok(status(hermes_dir))
}

fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .map(|d| d.join("hermes"))
        .map_err(|e| e.to_string())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// **trajectories_settings_get**: the switch and what is stored. Local only.
#[tauri::command]
pub async fn trajectories_settings_get(app: tauri::AppHandle) -> Result<TrajectoryStatus, String> {
    crate::blocking::off_main(move || Ok(status(&hermes_dir(&app)?))).await
}

/// **trajectories_settings_set**: the member's toggle. Applies to recording at the next Hermes
/// start; turning it off deletes assembled round files now.
#[tauri::command]
pub async fn trajectories_settings_set(
    app: tauri::AppHandle,
    enabled: bool,
    delete_recorded: Option<bool>,
) -> Result<TrajectoryStatus, String> {
    crate::blocking::off_main(move || {
        set(
            &hermes_dir(&app)?,
            enabled,
            delete_recorded.unwrap_or(false),
            now_ms(),
        )
    })
    .await
}

/// **trajectories_dataset_build**: assemble the newest verified turns into one training file
/// for a round. Refused when the switch is off.
#[tauri::command]
pub async fn trajectories_dataset_build(
    app: tauri::AppHandle,
    max_trajectories: u32,
) -> Result<DatasetSummary, String> {
    crate::blocking::off_main(move || {
        build_round_dataset(&hermes_dir(&app)?, max_trajectories, now_ms())
    })
    .await
}

#[cfg(test)]
#[path = "fl_trajectories_tests.rs"]
mod tests;
