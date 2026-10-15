//! HGW-W11 (core half) and the member's commit identity: how Hermes builds in a project.
//!
//! Data sources (Rule 7):
//! - **The auto-approve list:** the Hermes sidecar's compiled-in list (citrate-agent-runtime
//!   `agent-sidecar/policy/auto_approve.toml`, runtime #83), read live from the running sidecar
//!   (`GET /shell/auto-approve` → `{enabled, conditions, rules}`) for Settings. A listed command
//!   runs without a card only when the session is untainted, the OS sandbox is enforced and the
//!   folder is inside a project (D-16 as amended, owner decision 2026-10-15). Core never edits the
//!   list; the member can switch it off, and then every command asks.
//! - **The commit identity:** the member's own name and email for Hermes's commits (runtime #84,
//!   `CITRATE_HERMES_GIT_USER_NAME` / `CITRATE_HERMES_GIT_USER_EMAIL`). Unset, the sidecar uses the
//!   member's global git config, else `Hermes <hermes@citrate.invalid>`. Every Hermes commit carries
//!   a `Co-authored-by: Hermes` trailer either way.
//! - **The settings file:** `<app local data>/hermes/build-settings.json`, written only by
//!   [`hermes_build_settings_set`] from Settings > Hermes · building.
//!
//! A missing file means the defaults (auto-approve on, no identity). An unreadable or corrupt
//! file turns auto-approve **off** and passes no identity: a permission never widens on a read
//! error. Both variables are always passed, pinned, so a value inherited from core's own
//! environment can never replace the list or the identity. A change restarts a running Hermes.
//!
//! Nothing here runs a command, signs or holds a key (Rule 3).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{manager, HermesError, HermesManager, Result as HResult};

/// The settings file inside the Hermes data folder.
pub const SETTINGS_FILE: &str = "build-settings.json";
/// The sidecar's list switch (runtime `auto_approve::AUTO_APPROVE_ENV`): `0` = off; empty = the
/// shipped list.
pub const AUTO_APPROVE_ENV: &str = "CITRATE_HERMES_AUTO_APPROVE";
/// The commit author name (runtime `git_identity::GIT_USER_NAME_ENV`).
pub const GIT_USER_NAME_ENV: &str = "CITRATE_HERMES_GIT_USER_NAME";
/// The commit author email (runtime `git_identity::GIT_USER_EMAIL_ENV`).
pub const GIT_USER_EMAIL_ENV: &str = "CITRATE_HERMES_GIT_USER_EMAIL";
/// Every variable [`sidecar_env`] may emit; the Hermes manager drops anything else.
pub const SIDECAR_ENV_KEYS: &[&str] = &[AUTO_APPROVE_ENV, GIT_USER_NAME_ENV, GIT_USER_EMAIL_ENV];
/// Longest name or email (the runtime's bound).
pub const MAX_IDENTITY_BYTES: usize = 256;

/// The member's choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct BuildSettings {
    /// Listed sandboxed project commands run without a card (D-16 as amended). On by default.
    pub auto_approve: bool,
    /// Hermes's commits use this author; `None` = the member's git config, else Hermes.
    pub git_name: Option<String>,
    pub git_email: Option<String>,
}

impl Default for BuildSettings {
    fn default() -> Self {
        BuildSettings {
            auto_approve: true,
            git_name: None,
            git_email: None,
        }
    }
}

/// The identity rules the runtime applies (`GitIdentity::new`), checked before anything is saved,
/// so a saved identity is never silently dropped by the sidecar.
pub fn valid_identity(name: &str, email: &str) -> Result<(String, String), String> {
    let name = name.trim();
    let email = email.trim();
    let plain = |s: &str| !s.chars().any(|c| c.is_control() || c == '<' || c == '>');
    if name.is_empty() || name.len() > MAX_IDENTITY_BYTES || !plain(name) {
        return Err(
            "the name must be 1 to 256 bytes with no <, > or control characters".to_string(),
        );
    }
    let address = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty());
    if email.len() > MAX_IDENTITY_BYTES
        || !address
        || !plain(email)
        || email.chars().any(char::is_whitespace)
    {
        return Err(
            "the email must be an address (local@domain) of at most 256 bytes, with no spaces, <, > or control characters"
                .to_string(),
        );
    }
    Ok((name.to_string(), email.to_string()))
}

/// Read the settings. Missing = the defaults. Unreadable or corrupt is an error.
pub fn load(dir: &Path) -> Result<BuildSettings, String> {
    match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| {
            "the Hermes building settings are not valid; auto-approve stays off until you set them again"
                .to_string()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BuildSettings::default()),
        Err(_) => Err(
            "the Hermes building settings could not be read; auto-approve stays off".to_string(),
        ),
    }
}

/// Store settings (written to a temp file, then renamed).
pub fn save(dir: &Path, s: &BuildSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "cannot create the Hermes data folder".to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{SETTINGS_FILE}.tmp"));
    std::fs::write(&tmp, text)
        .map_err(|_| "cannot write the Hermes building settings".to_string())?;
    std::fs::rename(&tmp, dir.join(SETTINGS_FILE))
        .map_err(|_| "cannot write the Hermes building settings".to_string())
}

/// The sidecar environment, every variable pinned: auto-approve `0` when off (or unreadable),
/// empty (the shipped list) when on; the identity only when both parts are set and valid.
pub fn sidecar_env(loaded: &Result<BuildSettings, String>) -> Vec<(String, String)> {
    let (auto, identity) = match loaded {
        Ok(s) => {
            let id = match (&s.git_name, &s.git_email) {
                (Some(n), Some(e)) => valid_identity(n, e).ok(),
                _ => None,
            };
            (s.auto_approve, id)
        }
        Err(_) => (false, None),
    };
    let (name, email) = identity.unwrap_or_default();
    vec![
        (
            AUTO_APPROVE_ENV.to_string(),
            if auto { "" } else { "0" }.to_string(),
        ),
        (GIT_USER_NAME_ENV.to_string(), name),
        (GIT_USER_EMAIL_ENV.to_string(), email),
    ]
}

/// The production env source: the stored settings at each Hermes start.
pub fn file_env_source(hermes_dir: PathBuf) -> crate::hermes_web::EnvSource {
    std::sync::Arc::new(move || sidecar_env(&load(&hermes_dir)))
}

/// What Settings reads back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildSettingsStatus {
    pub auto_approve: bool,
    pub git_name: Option<String>,
    pub git_email: Option<String>,
    /// True when a running Hermes was restarted to apply the change.
    pub restarted: bool,
    pub load_error: Option<String>,
}

fn status(loaded: Result<BuildSettings, String>, restarted: bool) -> BuildSettingsStatus {
    match loaded {
        Ok(s) => BuildSettingsStatus {
            auto_approve: s.auto_approve,
            git_name: s.git_name,
            git_email: s.git_email,
            restarted,
            load_error: None,
        },
        Err(e) => BuildSettingsStatus {
            auto_approve: false,
            git_name: None,
            git_email: None,
            restarted,
            load_error: Some(e),
        },
    }
}

/// Validate and store the member's choices; restart a running Hermes when what it would receive
/// changed. `git_name` and `git_email` are set together or cleared together.
pub fn apply(
    dir: &Path,
    auto_approve: bool,
    git_name: Option<String>,
    git_email: Option<String>,
    restart: impl FnOnce() -> Result<bool, String>,
) -> Result<BuildSettingsStatus, String> {
    let blank = |v: &Option<String>| v.as_deref().map(str::trim).unwrap_or("").is_empty();
    let (git_name, git_email) = match (blank(&git_name), blank(&git_email)) {
        (true, true) => (None, None),
        (false, false) => {
            let (n, e) = valid_identity(
                git_name.as_deref().unwrap_or_default(),
                git_email.as_deref().unwrap_or_default(),
            )?;
            (Some(n), Some(e))
        }
        _ => return Err("set both the name and the email, or clear both".to_string()),
    };
    let before = load(dir);
    let next = BuildSettings {
        auto_approve,
        git_name,
        git_email,
    };
    if before.as_ref().ok() == Some(&next) && dir.join(SETTINGS_FILE).exists() {
        return Ok(status(Ok(next), false));
    }
    let env_before = sidecar_env(&before);
    save(dir, &next)?;
    let loaded = Ok(next);
    let restarted = if sidecar_env(&loaded) != env_before {
        restart()?
    } else {
        false
    };
    Ok(status(loaded, restarted))
}

fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes"))
}

/// `GET /shell/auto-approve` on the running sidecar: `{enabled, conditions, rules}`.
pub(crate) fn auto_approve_list(m: &HermesManager) -> Result<serde_json::Value, String> {
    let run = || -> HResult<super::ControlResp> {
        let bearer = m.bearer()?;
        m.control
            .get(&format!("{}/shell/auto-approve", m.control_url()), &bearer)
    };
    let resp = run().map_err(|e| match e {
        HermesError::NotRunning => "Hermes is not running; start it to see the list".to_string(),
        other => other.to_string(),
    })?;
    if resp.status == 404 {
        return Err("this Hermes version has no auto-approve list".to_string());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!("the sidecar answered {}", resp.status));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.body)
        .map_err(|_| "the auto-approve list could not be read".to_string())?;
    if v.get("enabled").and_then(|e| e.as_bool()).is_none()
        || !v.get("rules").is_some_and(|r| r.is_array())
    {
        return Err("the auto-approve list could not be read".to_string());
    }
    Ok(v)
}

/// **hermes_build_settings_get**: the stored choices (main window only).
#[tauri::command]
pub async fn hermes_build_settings_get(
    app: tauri::AppHandle,
) -> Result<BuildSettingsStatus, String> {
    crate::blocking::off_main(move || Ok(status(load(&hermes_dir(&app)?), false))).await
}

/// **hermes_build_settings_set**: store the member's choices and restart a running Hermes when
/// they change what it receives (main window only).
#[tauri::command]
pub async fn hermes_build_settings_set(
    app: tauri::AppHandle,
    auto_approve: bool,
    git_name: Option<String>,
    git_email: Option<String>,
) -> Result<BuildSettingsStatus, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        apply(&dir, auto_approve, git_name, git_email, || {
            super::restart_if_running(&app)
        })
    })
    .await
}

/// **hermes_auto_approve_list**: the commands the running Hermes runs without a card, and when
/// (main window only).
#[tauri::command]
pub async fn hermes_auto_approve_list(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    crate::blocking::off_main(move || auto_approve_list(manager(&app)?)).await
}

#[cfg(test)]
mod tests {
    include!("hermes_build_settings_tests.rs");
}
