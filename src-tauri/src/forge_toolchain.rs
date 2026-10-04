//! HUP-S6.3 — the member's switch for Hermes's contract toolchain (forge, slither, aderyn,
//! medusa), and the sidecar environment it produces.
//!
//! The toolchain tools run in the Hermes sidecar (citrate-agent-runtime `agent-sidecar`
//! `toolchain.rs`) and are offered only when the sidecar starts with
//! `CITRATE_HERMES_TOOLCHAIN=1`. Core owns that choice: it stores the member's switch in
//! `<app local data>/hermes/toolchain-settings.json` and, when it is on, sets the variable and
//! the toolchain search path when it starts Hermes ([`sidecar_env`]). Changes apply the next time
//! Hermes starts.
//!
//! **Default off, and off changes nothing.** With the switch off no variable is passed, and the
//! sidecar behaves exactly as before.
//!
//! **Search path.** The installed component versions (`<app data>/components`, HUP-S6.1) come
//! first, then the usual per-user and system tool folders. A tool found nowhere stays an honest
//! "not installed", which the deploy gate counts as a fail (NOT READY), never a pass.
//!
//! Rule 3: nothing here signs or holds a key.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The settings file inside the Hermes data folder.
pub const SETTINGS_FILE: &str = "toolchain-settings.json";
/// `1` turns the toolchain tools on in the sidecar.
pub const TOOLCHAIN_ENV: &str = "CITRATE_HERMES_TOOLCHAIN";
/// The toolchain search path (absolute dirs, a platform path list).
pub const TOOLCHAIN_PATH_ENV: &str = "CITRATE_HERMES_TOOLCHAIN_PATH";
/// The solc binary forge uses.
pub const SOLC_ENV: &str = "CITRATE_HERMES_SOLC";
/// The chain's pinned compiler version.
pub const PINNED_SOLC: &str = "0.8.36";

/// Every variable [`sidecar_env`] may emit; the Hermes manager drops anything else.
pub const SIDECAR_ENV_KEYS: &[&str] = &[TOOLCHAIN_ENV, TOOLCHAIN_PATH_ENV, SOLC_ENV];

/// The programs the four tools run, and what each is for.
pub const PROGRAMS: [(&str, &str); 4] = [
    ("forge", "forge_test"),
    ("slither", "slither_scan"),
    ("aderyn", "aderyn_scan"),
    ("medusa", "medusa_fuzz"),
];

/// The member's choice. Off by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolchainSettings {
    pub enabled: bool,
}

/// One program and where it was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramStatus {
    pub program: String,
    pub tool: String,
    /// The absolute path found on the search path, or `None` (= "not installed").
    pub path: Option<String>,
}

/// What the Settings card shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainStatus {
    pub settings: ToolchainSettings,
    pub programs: Vec<ProgramStatus>,
    pub solc: Option<String>,
    pub search_path: Vec<String>,
    pub notices: Vec<String>,
    pub applies_on_restart: bool,
    pub load_error: Option<String>,
}

/// The stored settings; a missing file is the default, a corrupt one is an error.
pub fn load(dir: &Path) -> Result<ToolchainSettings, String> {
    match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| "the toolchain settings file is not valid; it stays off".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ToolchainSettings::default()),
        Err(_) => Err("the toolchain settings file could not be read; it stays off".into()),
    }
}

/// Store settings (written to a temp file, then renamed).
pub fn save(dir: &Path, s: &ToolchainSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "cannot create the Hermes data folder".to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{SETTINGS_FILE}.tmp"));
    std::fs::write(&tmp, text).map_err(|_| "cannot write the toolchain settings".to_string())?;
    std::fs::rename(&tmp, dir.join(SETTINGS_FILE))
        .map_err(|_| "cannot write the toolchain settings".to_string())
}

/// Where the toolchain components are installed and what the member's home is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainPlaces {
    /// `<app data>/components` (HUP-S6.1). May not exist.
    pub components_root: PathBuf,
    pub home: PathBuf,
}

/// The folders holding each installed component's programs, from the component store's state
/// and the bundle's entry points. Nothing is created.
fn component_bin_dirs(components_root: &Path) -> Vec<PathBuf> {
    use citrate_components::bundle::Bundle;
    use citrate_components::install::Store;
    use citrate_components::platform::Platform;
    if !components_root.join("state.json").is_file() {
        return Vec::new();
    }
    let Ok(store) = Store::open(components_root) else {
        return Vec::new();
    };
    let Ok(bundle) = Bundle::parse(crate::components::BUNDLE_JSON) else {
        return Vec::new();
    };
    let Some(platform) = Platform::current() else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    for tool in ["foundry", "slither", "aderyn", "medusa", "solc"] {
        let Ok(Some(dir)) = store.current_dir(tool) else {
            continue;
        };
        let Some(t) = bundle.tools.iter().find(|t| t.name == tool) else {
            continue;
        };
        // A per-platform artifact may name its own entry points (an archive's top folder).
        let entries = t
            .artifacts
            .get(platform.as_str())
            .and_then(|a| a.entrypoints.clone())
            .unwrap_or_else(|| t.entrypoints.clone());
        for entry in entries {
            if let Some(parent) = dir.join(&entry).parent() {
                let p = parent.to_path_buf();
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }
    }
    dirs
}

/// The toolchain search path: installed components first, then the per-user tool folders
/// (Foundry, pipx and slither's own environment, which also holds the `crytic-compile` medusa
/// needs, cargo, Go), then the system folders.
pub fn search_path(places: &ToolchainPlaces) -> Vec<PathBuf> {
    let mut out = component_bin_dirs(&places.components_root);
    let home = &places.home;
    let user = [
        home.join(".foundry/bin"),
        home.join(".local/bin"),
        home.join(".local/pipx/venvs/slither-analyzer/bin"),
        home.join(".cargo/bin"),
        home.join("go/bin"),
    ];
    let system = [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    for p in user.into_iter().chain(system) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// The pinned solc: the solc component when installed, else the per-user svm folder.
pub fn find_solc(places: &ToolchainPlaces) -> Option<PathBuf> {
    let from_component = component_bin_dirs(&places.components_root)
        .into_iter()
        .map(|d| d.join("solc"))
        .find(|p| p.is_file());
    from_component.or_else(|| {
        [
            places.home.join("Library/Application Support/svm"),
            places.home.join(".svm"),
            places.home.join(".local/share/svm"),
        ]
        .into_iter()
        .map(|d| d.join(PINNED_SOLC).join(format!("solc-{PINNED_SOLC}")))
        .find(|p| p.is_file())
    })
}

/// The first `program` on `path`.
pub fn which(program: &str, path: &[PathBuf]) -> Option<PathBuf> {
    path.iter().map(|d| d.join(program)).find(|p| p.is_file())
}

/// The sidecar environment for these settings. Off produces nothing.
pub fn sidecar_env(s: &ToolchainSettings, places: &ToolchainPlaces) -> Vec<(String, String)> {
    if !s.enabled {
        return Vec::new();
    }
    let mut env = vec![(TOOLCHAIN_ENV.to_string(), "1".to_string())];
    let path = search_path(places);
    if let Ok(joined) = std::env::join_paths(&path) {
        env.push((
            TOOLCHAIN_PATH_ENV.to_string(),
            joined.to_string_lossy().into_owned(),
        ));
    }
    if let Some(solc) = find_solc(places) {
        env.push((SOLC_ENV.to_string(), solc.to_string_lossy().into_owned()));
    }
    env
}

/// The status the Settings card renders.
pub fn status_for(
    s: ToolchainSettings,
    places: &ToolchainPlaces,
    load_error: Option<String>,
) -> ToolchainStatus {
    let path = search_path(places);
    let programs: Vec<ProgramStatus> = PROGRAMS
        .iter()
        .map(|(program, tool)| ProgramStatus {
            program: program.to_string(),
            tool: tool.to_string(),
            path: which(program, &path).map(|p| p.display().to_string()),
        })
        .collect();
    let solc = find_solc(places).map(|p| p.display().to_string());
    let mut notices = Vec::new();
    let missing: Vec<&str> = programs
        .iter()
        .filter(|p| p.path.is_none())
        .map(|p| p.program.as_str())
        .collect();
    if !missing.is_empty() {
        notices.push(format!(
            "Not installed: {}. Their deploy gate items fail (NOT READY) until they are installed.",
            missing.join(", ")
        ));
    }
    if solc.is_none() {
        notices.push(format!(
            "solc {PINNED_SOLC} was not found, so builds fail offline."
        ));
    }
    if s.enabled {
        notices.push(
            "Hermes can run these tools only inside folders you granted for reading and writing, with no network, in the OS sandbox when this machine has one.".to_string(),
        );
    }
    ToolchainStatus {
        settings: s,
        programs,
        solc,
        search_path: path.iter().map(|p| p.display().to_string()).collect(),
        notices,
        applies_on_restart: true,
        load_error,
    }
}

/// The production env source: the stored settings at each start (a corrupt file = off).
pub fn file_env_source(
    hermes_dir: PathBuf,
    places: ToolchainPlaces,
) -> crate::hermes_web::EnvSource {
    std::sync::Arc::new(move || sidecar_env(&load(&hermes_dir).unwrap_or_default(), &places))
}

/// The app's places.
pub fn places_for<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<ToolchainPlaces, String> {
    use tauri::Manager;
    Ok(ToolchainPlaces {
        components_root: app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("components"),
        home: app.path().home_dir().map_err(|e| e.to_string())?,
    })
}

fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .map(|d| d.join("hermes"))
        .map_err(|e| e.to_string())
}

/// **toolchain_settings_get**: the switch, which programs were found, and what it means.
#[tauri::command]
pub async fn toolchain_settings_get(app: tauri::AppHandle) -> Result<ToolchainStatus, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        let places = places_for(&app)?;
        Ok(match load(&dir) {
            Ok(s) => status_for(s, &places, None),
            Err(e) => status_for(ToolchainSettings::default(), &places, Some(e)),
        })
    })
    .await
}

/// **toolchain_settings_set**: store the switch. It applies the next time Hermes starts.
#[tauri::command]
pub async fn toolchain_settings_set(
    app: tauri::AppHandle,
    settings: ToolchainSettings,
) -> Result<ToolchainStatus, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        save(&dir, &settings)?;
        Ok(status_for(settings, &places_for(&app)?, None))
    })
    .await
}

#[cfg(test)]
#[path = "forge_toolchain_tests.rs"]
mod tests;
