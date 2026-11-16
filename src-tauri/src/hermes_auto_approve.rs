//! Hermes per-project opt-ins for the shell auto-approve list (V1, owner decision 2026-11-16).
//!
//! The sidecar's auto-approve list (citrate-agent-runtime `agent-sidecar/policy/auto_approve.toml`)
//! runs a few read commands inside a project without an approval card. Every rule that can run
//! code the project defines, or that the project's config can point at (`node <file>`, npm
//! scripts, the project's tsc, cargo, forge, and git other than `init`), is marked `opt_in` in the
//! list (citrate-agent-runtime#107) and does not run without a card unless the member turned it on
//! for that project here. A sidecar that predates opt-in rules lists none, the panel then offers
//! nothing, and core sends no opt-ins.
//!
//! Data sources (Rule 7):
//! - **The rules:** the sidecar's list in effect, read through `GET /shell/auto-approve`
//!   ([`hermes_auto_approve_rules`]); a rule with `opt_in: true` is one the member can turn on.
//!   A sidecar that predates opt-in rules lists none, and the app then offers nothing to turn on.
//! - **The member's choice:** `<app local data>/hermes/auto-approve-opt-ins.json`, written only by
//!   [`hermes_auto_approve_set`] from Settings (Hermes, Terminal and folders). Hermes has no tool
//!   that reaches it, and the sidecar's file tools cannot write it (Citrate app data is on the
//!   central deny list, `citrate-agent-guard`).
//!
//! Each session opened with folder grants carries the choice as `autoApproveOptIns`; the sidecar
//! checks every entry against its list and refuses an id that is not an opt-in rule. A change
//! applies to the next conversation. An unreadable or damaged file counts as no opt-ins (a
//! permission never widens on a read error).
//!
//! Nothing here runs a command, signs or holds a key (Rule 3).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file inside the Hermes data folder.
pub const OPT_INS_FILE: &str = "auto-approve-opt-ins.json";
/// The rules of the shipped list that are opt-in (`opt_in = true` in the sidecar's list). Core
/// refuses any other id before the sidecar does.
pub const OPT_IN_RULES: &[&str] = &[
    "node-script",
    "npm-run",
    "npm-test",
    "npx-tsc",
    "forge-build-test",
    "cargo-offline",
    "git-status",
    "git-diff",
    "git-add",
    "git-commit",
];
/// Most project folders the member may opt in (the sidecar's `MAX_OPT_INS`).
pub const MAX_PROJECTS: usize = 64;
const FORMAT: u32 = 1;

/// One project folder and the opt-in rules turned on for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectOptIn {
    pub root: String,
    pub rules: Vec<String>,
    pub changed_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptIns {
    pub version: u32,
    pub projects: Vec<ProjectOptIn>,
}

impl Default for OptIns {
    fn default() -> Self {
        OptIns {
            version: FORMAT,
            projects: Vec::new(),
        }
    }
}

/// Read the opt-ins. Missing = none. Unreadable, damaged or another format is an error; every
/// caller that acts on it then sends none.
pub fn load(dir: &Path) -> Result<OptIns, String> {
    match std::fs::read_to_string(dir.join(OPT_INS_FILE)) {
        Ok(text) => match serde_json::from_str::<OptIns>(&text) {
            Ok(o) if o.version == FORMAT => Ok(o),
            _ => Err(
                "the saved terminal opt-ins are not valid; none apply until you set them again"
                    .into(),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(OptIns::default()),
        Err(_) => Err("the saved terminal opt-ins could not be read; none apply".into()),
    }
}

fn save(dir: &Path, o: &OptIns) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "cannot create the Hermes data folder".to_string())?;
    let text = serde_json::to_string_pretty(o).map_err(|e| e.to_string())?;
    crate::atomic_file::replace(&dir.join(OPT_INS_FILE), text.as_bytes())
        .map_err(|_| "cannot write the terminal opt-ins".to_string())
}

/// Check a member's choice: an absolute, existing folder, and only opt-in rule ids (no repeats).
fn check(root: &str, rules: &[String]) -> Result<(PathBuf, Vec<String>), String> {
    let p = Path::new(root);
    if !p.is_absolute() {
        return Err("the project folder must be an absolute path".into());
    }
    let canonical = p
        .canonicalize()
        .map_err(|_| "the project folder does not exist".to_string())?;
    if !canonical.is_dir() {
        return Err("the project folder is not a folder".into());
    }
    let mut out: Vec<String> = Vec::new();
    for r in rules {
        if !OPT_IN_RULES.contains(&r.as_str()) {
            return Err(format!(
                "{r} is not a command that can be turned on per project"
            ));
        }
        if !out.contains(r) {
            out.push(r.clone());
        }
    }
    Ok((canonical, out))
}

/// Set the opt-in rules for one project folder (`rules` empty = remove the folder).
pub fn set(dir: &Path, root: &str, rules: &[String], now_ms: u64) -> Result<OptIns, String> {
    if rules.is_empty() {
        // Turning a folder off never needs it to exist (deleted, moved, volume disconnected):
        // the saved entry is removed by the path as given and, when it still resolves, by its
        // canonical path.
        if !Path::new(root).is_absolute() {
            return Err("the project folder must be an absolute path".into());
        }
        let canonical = Path::new(root)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .ok();
        let mut o = load(dir).unwrap_or_default();
        o.projects
            .retain(|p| p.root != root && Some(&p.root) != canonical.as_ref());
        save(dir, &o)?;
        return Ok(o);
    }
    let (canonical, rules) = check(root, rules)?;
    let root = canonical.to_string_lossy().to_string();
    // A damaged file is replaced by the member's new choice (it applied as none meanwhile).
    let mut o = load(dir).unwrap_or_default();
    o.projects.retain(|p| p.root != root);
    if !rules.is_empty() {
        if o.projects.len() >= MAX_PROJECTS {
            return Err(format!(
                "at most {MAX_PROJECTS} project folders can have commands turned on"
            ));
        }
        o.projects.push(ProjectOptIn {
            root,
            rules,
            changed_at_ms: now_ms,
        });
    }
    save(dir, &o)?;
    Ok(o)
}

/// The `autoApproveOptIns` value for a session: every saved folder that still exists, with its
/// rules. An unreadable file sends none.
pub fn session_value(dir: &Path) -> Vec<serde_json::Value> {
    load(dir)
        .map(|o| {
            o.projects
                .into_iter()
                .filter(|p| !p.rules.is_empty() && Path::new(&p.root).is_dir())
                .map(|p| serde_json::json!({ "root": p.root, "rules": p.rules }))
                .collect()
        })
        .unwrap_or_default()
}

/// Add `autoApproveOptIns` to a `POST /sessions` body when the member opted any project in. With
/// none the body is unchanged, so a sidecar that predates the field still opens the session.
pub fn attach(body: &str, dir: &Path) -> Result<String, String> {
    let value = session_value(dir);
    if value.is_empty() {
        return Ok(body.to_string());
    }
    let mut v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "internal: session body is not JSON".to_string())?;
    v.as_object_mut()
        .ok_or("internal: session body is not an object")?
        .insert("autoApproveOptIns".into(), serde_json::Value::Array(value));
    Ok(v.to_string())
}

pub(crate) fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes"))
}

/// Whether a running sidecar's `GET /shell/auto-approve` answer lists any opt-in rule. An older
/// sidecar (none listed, or no such route) refuses an unknown `autoApproveOptIns` key, so core
/// sends opt-ins only to one that has them; a read error counts as no (its commands then ask).
pub fn supports_opt_ins(list_json: Option<&str>) -> bool {
    list_json
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v.get("rules").and_then(|r| r.as_array()).cloned())
        .is_some_and(|rules| {
            rules
                .iter()
                .any(|r| r.get("opt_in") == Some(&serde_json::Value::Bool(true)))
        })
}

/// [`attach`] for the app's Hermes folder, only when the running sidecar has opt-in rules.
pub(crate) fn attach_for_app<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    body: &str,
    m: &crate::hermes::HermesManager,
) -> Result<String, String> {
    let list = m
        .control_get_path("/shell/auto-approve")
        .ok()
        .filter(|r| r.status == 200)
        .map(|r| r.body);
    if !supports_opt_ins(list.as_deref()) {
        return Ok(body.to_string());
    }
    attach(body, &hermes_dir(app)?)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// What Settings reads back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptInsView {
    pub projects: Vec<ProjectOptIn>,
    pub load_error: Option<String>,
}

/// **hermes_auto_approve_get**: the saved per-project opt-ins (main window only).
#[tauri::command]
pub async fn hermes_auto_approve_get(app: tauri::AppHandle) -> Result<OptInsView, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        Ok(match load(&dir) {
            Ok(o) => OptInsView {
                projects: o.projects,
                load_error: None,
            },
            Err(e) => OptInsView {
                projects: Vec::new(),
                load_error: Some(e),
            },
        })
    })
    .await
}

/// **hermes_auto_approve_set**: the member turns opt-in commands on or off for one project folder
/// (main window only; Hermes has no tool for this). Applies to the next conversation.
#[tauri::command]
pub async fn hermes_auto_approve_set(
    app: tauri::AppHandle,
    root: String,
    rules: Vec<String>,
) -> Result<OptInsView, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        let o = set(&dir, &root, &rules, now_ms())?;
        Ok(OptInsView {
            projects: o.projects,
            load_error: None,
        })
    })
    .await
}

/// **hermes_auto_approve_rules**: the sidecar's auto-approve list in effect (`GET
/// /shell/auto-approve`), passed through for Settings. `null` when Hermes is not running.
#[tauri::command]
pub async fn hermes_auto_approve_rules(
    app: tauri::AppHandle,
) -> Result<Option<serde_json::Value>, String> {
    crate::blocking::off_main(move || {
        let m = crate::hermes::manager(&app)?;
        if !m.is_running() {
            return Ok(None);
        }
        let resp = m
            .control_get_path("/shell/auto-approve")
            .map_err(|e| e.to_string())?;
        if resp.status != 200 {
            return Ok(None);
        }
        serde_json::from_str(&resp.body)
            .map(Some)
            .map_err(|_| "the auto-approve list Hermes returned is not JSON".to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_auto_approve_tests.rs");
}
