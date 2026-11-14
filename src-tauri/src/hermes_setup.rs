//! 0.5.4 (owner, 2026-11-14): "Ensure the skills, tools, prompts and resources that need to be
//! there are available to the Hermes agent on install."
//!
//! Two halves.
//!
//! **Provision** ([`provision`], run once at every launch from `lib.rs` setup, before the node
//! MCP server's state is built). Deterministic and idempotent: running it again changes nothing.
//! - the Hermes data folders under `<app_local_data>/hermes` ([`DATA_SUBDIRS`]): created up
//!   front, so the first start never depends on the sidecar creating them;
//! - the pinned starter capsules (`capsule_pins::seed_verified`): copied, kept, or repaired to the
//!   pinned bytes; the member's own capsules are never touched;
//! - first-run defaults, written only when the file does not exist yet, so a member's later choice
//!   always wins: Hermes is offered the node's MCP server (`hermes/mcp-settings.json`
//!   `node: true`, read tools only: [`crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS`] stays false),
//!   and the node's loopback MCP server at 127.0.0.1:47204 is on (`node-mcp/config.json`). The
//!   memory and CitrateScan entries stay off as before.
//!
//! **Self-check** ([`check`], the `hermes_setup_check` command). Provisions the folders and
//! capsules again, then reports every item Hermes needs, each `ok`, `missing` (with why) or `off`
//! (the member turned it off). Nothing is hidden in a log: the app shows the report on the Agent
//! page and says at launch when a required item is missing. What only the running sidecar can
//! prove (skills it loaded) and what lives in the webview (the tool list, the system prompt) is
//! added by the app (`src/shell/slices/hermesSetup.ts`).

use crate::capsule_pins::{SeedReport, BUNDLED_CAPSULES};
use crate::hermes_mcp::McpSettings;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The folders under `<app_local_data>/hermes` the sidecar is pointed at.
pub const DATA_SUBDIRS: &[&str] = &[
    "capsules",
    "learn",
    "skills",
    "checkpoints",
    "metering",
    "records",
    "anchor",
];

/// Whether Hermes cannot do its job without an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Need {
    Required,
    Optional,
}

/// What the check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemState {
    Ok,
    Missing,
    /// Turned off by the member (not a fault).
    Off,
}

/// One row of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupItem {
    pub id: &'static str,
    pub label: &'static str,
    pub need: Need,
    pub state: ItemState,
    pub detail: String,
}

/// The node MCP server as its state reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeMcpFacts {
    pub enabled: bool,
    pub running: bool,
    pub endpoint: String,
    pub last_error: Option<String>,
}

/// Everything [`items`] reads, gathered by the caller (the app, or a test).
#[derive(Debug, Clone)]
pub struct SetupFacts {
    pub hermes_dir: PathBuf,
    pub resources: Option<PathBuf>,
    pub hermes_bin: Result<PathBuf, String>,
    pub embed_model: Option<PathBuf>,
    pub llama_bin: Option<PathBuf>,
    pub seed: Option<SeedReport>,
    pub mcp_settings: McpSettings,
    /// `None`: the node MCP state is not managed (it always is in the app).
    pub node_mcp: Option<NodeMcpFacts>,
    /// Folders that could not be created, with why.
    pub dir_errors: Vec<(String, String)>,
}

/// The whole report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    pub items: Vec<SetupItem>,
    /// Required items that are `missing` (the app warns when above zero).
    pub missing_required: usize,
    /// What launch provisioning wrote as first-run defaults this session.
    pub first_run_defaults: Vec<String>,
}

/// Create every data folder. Returns the ones that could not be made.
pub fn provision_dirs(hermes_dir: &Path) -> Vec<(String, String)> {
    let mut errs = Vec::new();
    for d in DATA_SUBDIRS {
        if let Err(e) = std::fs::create_dir_all(hermes_dir.join(d)) {
            errs.push((d.to_string(), e.to_string()));
        }
    }
    errs
}

/// The first-run defaults: each written only when its file does not exist. Returns what was written.
pub fn provision_defaults(hermes_dir: &Path, node_mcp_dir: Option<&Path>) -> Vec<String> {
    let mut wrote = Vec::new();
    if !crate::hermes_mcp::settings_path(hermes_dir).exists() {
        let s = McpSettings {
            node: true,
            ..McpSettings::default()
        };
        match crate::hermes_mcp::save_settings(hermes_dir, &s) {
            Ok(()) => wrote.push("Hermes is offered the node's MCP read tools".to_string()),
            Err(e) => eprintln!("hermes setup: the MCP default was not written: {e}"),
        }
    }
    if let Some(dir) = node_mcp_dir {
        let cfg = dir.join("config.json");
        if !cfg.exists() {
            let body = serde_json::to_vec_pretty(&crate::node_mcp::NodeMcpConfig {
                enabled: true,
                port: None,
            });
            let written = std::fs::create_dir_all(dir)
                .map_err(|e| e.to_string())
                .and_then(|()| body.map_err(|e| e.to_string()))
                .and_then(|b| crate::node_mcp_token::write_private(&cfg, &b));
            match written {
                Ok(()) => wrote.push(
                    "the node's MCP server is on (127.0.0.1 only, token required)".to_string(),
                ),
                Err(e) => eprintln!("hermes setup: the node MCP default was not written: {e}"),
            }
        }
    }
    wrote
}

/// Seed the pinned starter capsules (idempotent). `None` when the resource folder is unknown.
pub fn provision_capsules(hermes_dir: &Path, resources: Option<&Path>) -> Option<SeedReport> {
    let r = resources?;
    Some(crate::capsule_pins::seed_verified(
        &r.join("capsules"),
        &hermes_dir.join("capsules"),
    ))
}

/// Count `<dir>/<skill>/SKILL.md`.
fn skill_md_count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().join("SKILL.md").is_file())
                .count()
        })
        .unwrap_or(0)
}

fn item(
    id: &'static str,
    label: &'static str,
    need: Need,
    state: ItemState,
    detail: impl Into<String>,
) -> SetupItem {
    SetupItem {
        id,
        label,
        need,
        state,
        detail: detail.into(),
    }
}

/// The report rows from `f`. Pure: reads only the paths `f` names.
pub fn items(f: &SetupFacts) -> Vec<SetupItem> {
    use ItemState::{Missing, Off};
    use Need::*;
    let mut out = Vec::new();

    out.push(match &f.hermes_bin {
        Ok(p) if p.exists() => item(
            "hermes-binary",
            "Hermes sidecar",
            Required,
            ItemState::Ok,
            "bundled with the app",
        ),
        Ok(p) => item(
            "hermes-binary",
            "Hermes sidecar",
            Required,
            Missing,
            format!("not found at {}", p.display()),
        ),
        Err(e) => item(
            "hermes-binary",
            "Hermes sidecar",
            Required,
            Missing,
            e.clone(),
        ),
    });

    let absent: Vec<&str> = DATA_SUBDIRS
        .iter()
        .copied()
        .filter(|d| !f.hermes_dir.join(d).is_dir())
        .collect();
    out.push(if absent.is_empty() {
        item(
            "data-folders",
            "Hermes data folders",
            Required,
            ItemState::Ok,
            format!("{} folders ready", DATA_SUBDIRS.len()),
        )
    } else {
        let why: Vec<String> = absent
            .iter()
            .map(|d| match f.dir_errors.iter().find(|(n, _)| n == d) {
                Some((_, e)) => format!("{d} ({e})"),
                None => d.to_string(),
            })
            .collect();
        item(
            "data-folders",
            "Hermes data folders",
            Required,
            Missing,
            format!("could not be created: {}", why.join(", ")),
        )
    });

    out.push(match &f.seed {
        None => item(
            "starter-capsules",
            "Starter skills (capsules)",
            Required,
            Missing,
            "the app's resource folder could not be read, so the starter capsules were not installed",
        ),
        Some(r) => {
            let mut bad: Vec<String> = Vec::new();
            for (name, _) in BUNDLED_CAPSULES {
                if let Some((_, why)) = r.refused.iter().find(|(n, _)| n == name) {
                    bad.push(format!("{name}: {why}"));
                } else if !(r.seeded.iter().chain(&r.kept).chain(&r.repaired)).any(|n| n == name) {
                    bad.push(format!("{name}: not installed"));
                }
            }
            if bad.is_empty() {
                let mut d = format!("{} installed and verified against this build's pins", BUNDLED_CAPSULES.len());
                if !r.repaired.is_empty() {
                    d.push_str(&format!(" ({} repaired)", r.repaired.join(", ")));
                }
                item("starter-capsules", "Starter skills (capsules)", Required, ItemState::Ok, d)
            } else {
                item("starter-capsules", "Starter skills (capsules)", Required, Missing, bad.join("; "))
            }
        }
    });

    let first_party = f.resources.as_ref().map(|r| r.join("skills"));
    let n = first_party.as_deref().map(skill_md_count).unwrap_or(0);
    out.push(if n > 0 {
        item(
            "first-party-skills",
            "Citrate skills (SKILL.md)",
            Required,
            ItemState::Ok,
            format!("{n} bundled"),
        )
    } else {
        item(
            "first-party-skills",
            "Citrate skills (SKILL.md)",
            Required,
            Missing,
            match &first_party {
                Some(p) => format!("no SKILL.md found under {}", p.display()),
                None => "the app's resource folder could not be read".to_string(),
            },
        )
    });

    let third = f.resources.as_ref().map(|r| r.join("skills-bundle"));
    out.push(match third {
        Some(root) if root.join("skills.lock").is_file() && root.is_dir() => item(
            "third-party-skills",
            "Reviewed third-party skills",
            Optional,
            ItemState::Ok,
            "staged with skills.lock",
        ),
        _ => item(
            "third-party-skills",
            "Reviewed third-party skills",
            Optional,
            Missing,
            "not staged in this build (skills-bundle/ with skills.lock)",
        ),
    });

    let model_ok = f.embed_model.as_ref().is_some_and(|p| p.is_file());
    let llama_ok = f.llama_bin.as_ref().is_some_and(|p| p.exists());
    out.push(if model_ok && llama_ok {
        item(
            "embedding",
            "Memory search model",
            Optional,
            ItemState::Ok,
            "bundled; its pin is checked when Hermes starts",
        )
    } else {
        let mut gone = Vec::new();
        if !model_ok {
            gone.push("the embedding model");
        }
        if !llama_ok {
            gone.push("llama-server");
        }
        item(
            "embedding",
            "Memory search model",
            Optional,
            Missing,
            format!(
                "{} not found, so Hermes ranks memory by words instead",
                gone.join(" and ")
            ),
        )
    });

    out.push(match &f.node_mcp {
        None => item(
            "node-mcp",
            "Node MCP tools",
            Required,
            Missing,
            "the node MCP server is not set up in this app",
        ),
        Some(_) if !f.mcp_settings.node => item(
            "node-mcp",
            "Node MCP tools",
            Required,
            Off,
            "turned off for Hermes in Hermes › Controls › MCP servers",
        ),
        Some(n) if !n.enabled => item(
            "node-mcp",
            "Node MCP tools",
            Required,
            Off,
            "the node's MCP server is turned off in Settings",
        ),
        Some(n) if !n.running => item(
            "node-mcp",
            "Node MCP tools",
            Required,
            Missing,
            format!(
                "the node's MCP server is on but not listening{}",
                n.last_error
                    .as_deref()
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default()
            ),
        ),
        Some(n) => item(
            "node-mcp",
            "Node MCP tools",
            Required,
            ItemState::Ok,
            format!("read tools at {}", n.endpoint),
        ),
    });
    out
}

/// The report for `items` plus what launch provisioning wrote.
pub fn report(items: Vec<SetupItem>, first_run_defaults: Vec<String>) -> SetupReport {
    let missing_required = items
        .iter()
        .filter(|i| i.need == Need::Required && i.state == ItemState::Missing)
        .count();
    SetupReport {
        items,
        missing_required,
        first_run_defaults,
    }
}

/// What launch provisioning wrote this session (shown with the report).
static LAUNCH_DEFAULTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    crate::hermes_mcp::hermes_dir(app)
}

fn node_mcp_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join("node-mcp"))
}

/// Launch provisioning (see the module doc). Never fails the launch: what could not be done is
/// reported by the self-check.
pub fn provision<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let Ok(dir) = hermes_dir(app) else {
        eprintln!("hermes setup: no app data folder; the self-check will say so");
        return;
    };
    for (d, e) in provision_dirs(&dir) {
        eprintln!("hermes setup: folder {d} not created: {e}");
    }
    let resources = app.path().resource_dir().ok();
    if let Some(r) = provision_capsules(&dir, resources.as_deref()) {
        for (name, why) in &r.refused {
            eprintln!("hermes setup: starter capsule {name} not installed: {why}");
        }
    }
    let wrote = provision_defaults(&dir, node_mcp_dir(app).as_deref());
    *LAUNCH_DEFAULTS.lock().unwrap_or_else(|e| e.into_inner()) = wrote;
}

/// Gather the facts (provisioning folders and capsules again first) and build the report.
pub fn check<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<SetupReport, String> {
    use tauri::Manager;
    let dir = hermes_dir(app)?;
    let dir_errors = provision_dirs(&dir);
    let resources = app.path().resource_dir().ok();
    let seed = provision_capsules(&dir, resources.as_deref());
    let node_mcp = app.try_state::<crate::node_mcp::NodeMcpState>().map(|s| {
        let st = s.status();
        NodeMcpFacts {
            enabled: st.enabled,
            running: st.running,
            endpoint: st.endpoint,
            last_error: st.last_error,
        }
    });
    let facts = SetupFacts {
        hermes_bin: crate::hermes::resolve_hermes_bin(app),
        embed_model: crate::embed_serve::resolve_model(resources.as_deref()),
        llama_bin: crate::serve::resolve_llama_bin(app).ok(),
        mcp_settings: crate::hermes_mcp::load_settings(&dir),
        hermes_dir: dir,
        resources,
        seed,
        node_mcp,
        dir_errors,
    };
    let defaults = LAUNCH_DEFAULTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Ok(report(items(&facts), defaults))
}

/// `hermes_setup_check` — the Hermes self-check (provisions folders and capsules again first).
#[tauri::command]
pub async fn hermes_setup_check(app_h: tauri::AppHandle) -> Result<SetupReport, String> {
    crate::blocking::off_main(move || check(&app_h)).await
}

#[cfg(test)]
mod tests {
    include!("hermes_setup_tests.rs");
}
