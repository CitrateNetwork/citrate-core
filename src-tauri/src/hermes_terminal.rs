//! Hermes terminal commands: the member's switch for the sidecar's `shell_run` tool.
//!
//! Data sources (Rule 7):
//! - **The tool:** the Hermes sidecar (citrate-agent-runtime `agent-sidecar` `shell_run.rs`). It
//!   is offered only when the sidecar starts with `CITRATE_HERMES_SHELL_RUN=1`, and then only to a
//!   conversation opened with the member's folder grants (`agent_grants.rs`). Every command is held
//!   for the member's HIC decision (`hermes_shell.rs` + the approval card), runs inside the OS
//!   sandbox (Seatbelt on macOS, bubblewrap on Linux) with no network and writes only in the
//!   granted folder, and is refused when no sandbox works.
//! - **The switch:** `<app local data>/hermes/terminal-settings.json`, written only by
//!   [`hermes_terminal_set`] from Settings > App > "Let Hermes run terminal commands".
//!
//! **On by default** (owner decision 2026-10-04): a missing file counts as on. An unreadable or
//! corrupt file counts as **off** (a permission never widens on a read error). With the switch off
//! core passes `CITRATE_HERMES_SHELL_RUN` pinned empty, overriding any value inherited from core's
//! own environment, and the sidecar treats anything but `1` as off.
//!
//! **Applies at once.** Changing the switch restarts a running Hermes so the next conversation
//! has (or no longer has) the tool; a stopped Hermes picks it up when it starts.
//!
//! Nothing here runs a command, signs or holds a key (Rule 3); the env carries only `1` or empty.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The settings file inside the Hermes data folder.
pub const SETTINGS_FILE: &str = "terminal-settings.json";
/// The sidecar's switch (runtime `shell_run::SHELL_RUN_ENV`): exactly `1` offers `shell_run`.
pub const SHELL_RUN_ENV: &str = "CITRATE_HERMES_SHELL_RUN";
/// Every variable [`sidecar_env`] may emit; the Hermes manager drops anything else.
pub const SIDECAR_ENV_KEYS: &[&str] = &[SHELL_RUN_ENV];

/// The member's choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalSettings {
    pub enabled: bool,
    pub changed_at_ms: Option<u64>,
}

impl Default for TerminalSettings {
    /// Owner decision 2026-10-04: on.
    fn default() -> Self {
        TerminalSettings {
            enabled: true,
            changed_at_ms: None,
        }
    }
}

/// Read the settings. Missing = the default (on). Unreadable or corrupt is an error; every path
/// that acts on the settings then treats it as off.
pub fn load(dir: &Path) -> Result<TerminalSettings, String> {
    match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| {
            "the terminal-commands setting is not valid; Hermes terminal commands stay off until you set it again"
                .to_string()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TerminalSettings::default()),
        Err(_) => Err(
            "the terminal-commands setting could not be read; Hermes terminal commands stay off"
                .into(),
        ),
    }
}

/// Whether the switch is on (a read error counts as off).
pub fn enabled(dir: &Path) -> bool {
    load(dir).map(|s| s.enabled).unwrap_or(false)
}

/// Store settings (written to a temp file, then renamed).
pub fn save(dir: &Path, s: &TerminalSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "cannot create the Hermes data folder".to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{SETTINGS_FILE}.tmp"));
    std::fs::write(&tmp, text)
        .map_err(|_| "cannot write the terminal-commands setting".to_string())?;
    std::fs::rename(&tmp, dir.join(SETTINGS_FILE))
        .map_err(|_| "cannot write the terminal-commands setting".to_string())
}

/// The sidecar environment for the switch: `1` when on, pinned empty when off (so a value set
/// outside the member's switch, a shell or `launchctl` export, can never turn it on).
pub fn sidecar_env(enabled: bool) -> Vec<(String, String)> {
    let v = if enabled { "1" } else { "" };
    vec![(SHELL_RUN_ENV.to_string(), v.to_string())]
}

/// The production env source: the stored switch at each Hermes start (a read error = off).
pub fn file_env_source(hermes_dir: PathBuf) -> crate::hermes_web::EnvSource {
    std::sync::Arc::new(move || sidecar_env(enabled(&hermes_dir)))
}

/// What the Settings switch reads back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStatus {
    pub enabled: bool,
    /// True when a running Hermes was restarted to apply the change.
    pub restarted: bool,
    pub load_error: Option<String>,
}

/// Store `enabled` and, when it changed, run `restart` (which restarts Hermes only if it is
/// running and says whether it did). Setting the value it already has writes nothing and
/// restarts nothing, so the app can send the member's choice at every launch.
pub fn apply(
    dir: &Path,
    enabled: bool,
    now_ms: u64,
    restart: impl FnOnce() -> Result<bool, String>,
) -> Result<TerminalStatus, String> {
    let current = load(dir);
    let stored = dir.join(SETTINGS_FILE).exists();
    if let Ok(cur) = &current {
        if stored && cur.enabled == enabled {
            return Ok(TerminalStatus {
                enabled,
                restarted: false,
                load_error: None,
            });
        }
    }
    // A missing file already means "on"; writing it makes the member's choice explicit.
    let effective_before = current.as_ref().map(|s| s.enabled).unwrap_or(false);
    save(
        dir,
        &TerminalSettings {
            enabled,
            changed_at_ms: Some(now_ms),
        },
    )?;
    let restarted = if effective_before != enabled {
        restart()?
    } else {
        false
    };
    Ok(TerminalStatus {
        enabled,
        restarted,
        load_error: None,
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes"))
}

/// **hermes_terminal_get**: the stored switch (main window only).
#[tauri::command]
pub async fn hermes_terminal_get(app: tauri::AppHandle) -> Result<TerminalStatus, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        Ok(match load(&dir) {
            Ok(s) => TerminalStatus {
                enabled: s.enabled,
                restarted: false,
                load_error: None,
            },
            Err(e) => TerminalStatus {
                enabled: false,
                restarted: false,
                load_error: Some(e),
            },
        })
    })
    .await
}

/// **hermes_terminal_set**: store the member's switch and restart a running Hermes when the
/// effective value changed (main window only). Takes a boolean; no path, command or secret.
#[tauri::command]
pub async fn hermes_terminal_set(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<TerminalStatus, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        apply(&dir, enabled, now_ms(), || {
            crate::hermes::restart_if_running(&app)
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_terminal_tests.rs");
}

// The live proof with the real sidecar (env-gated: CITRATE_E2E_SHELL_SIDECAR_BIN).
#[cfg(test)]
mod e2e_tests {
    include!("hermes_terminal_e2e_tests.rs");
}
