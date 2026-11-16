//! 0.5.4 (owner, 2026-11-14): "Ensure the skills, tools, prompts and resources that need to be
//! there are available to the Hermes agent on install."
//!
//! Two halves.
//!
//! **Provision** ([`provision`], run at every launch from `lib.rs` setup, before the node MCP
//! server's state is built). Deterministic and idempotent: running it again changes nothing.
//! - the Hermes data folders under `<app_local_data>/hermes` ([`DATA_SUBDIRS`]): created up
//!   front, so the first start never depends on the sidecar creating them;
//! - the pinned starter capsules (`capsule_pins::seed_verified`): copied, kept, or repaired to the
//!   pinned bytes; the member's own capsules are never touched;
//! - **owner 2026-11-14: on for new installs only.** On a real first run (no app data at all when
//!   setup began, and no marker yet) Hermes is offered the node's MCP read tools
//!   (`hermes/mcp-settings.json` `node: true`; [`crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS`]
//!   stays false) and the node's loopback MCP server at 127.0.0.1:47204 is on
//!   (`node-mcp/config.json`). An upgraded install (data already there) keeps both off; its Setup
//!   card offers "Turn on node tools" (`hermes_setup_enable_node_tools`). Which case applied is
//!   written once, in [`MARKER_FILE`], so it is decided exactly once per install. Every default
//!   file is created without ever replacing one ([`write_new_file`]): a member's save that lands
//!   first always wins. The memory and CitrateScan entries stay off as before.
//!
//! **Self-check** ([`check`], the `hermes_setup_check` command). Read only: it repairs nothing.
//! It reports every item Hermes needs, each `ok`, `missing` (with why) or `off` (the member turned
//! it off). Repair is the explicit setup path ([`repair`], `hermes_setup_repair`: folders and
//! capsules). Nothing is hidden in a log: the app shows the report on the Agent page and says at
//! launch when a required item is missing. What only the running sidecar can prove (skills it
//! loaded) and what lives in the webview (the tool list, the system prompt) is added by the app
//! (`src/shell/slices/hermesSetup.ts`).

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

/// Citrate's own SKILL.md skills every build ships (resource `skills/<name>/SKILL.md`). Each one is
/// checked by name; a test keeps this list equal to `src-tauri/skills/`.
pub const FIRST_PARTY_SKILLS: &[&str] = &[
    "citrate-belnap-aggregate",
    // 0.5.4 Hermes x FL WP-H11: the federated-learning skills (names reserved, see skills_local.rs).
    "citrate-fl-adapters",
    "citrate-fl-collect",
    "citrate-fl-datasets",
    "citrate-fl-diagnose",
    "citrate-fl-member",
    "citrate-fl-organizer",
    "citrate-fl-payouts",
    "citrate-paraconsensus",
    "citrate-precompiles",
    "citrate-sidecar-consensus",
];

/// Written once per install, under `<app_local_data>/hermes`: which first-run case applied.
pub const MARKER_FILE: &str = ".provisioned-v1";

/// What kind of install this launch found, decided once and kept in [`MARKER_FILE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    /// A real first run: the first-run defaults were applied.
    New,
    /// Data from an earlier version was there: nothing was turned on.
    Upgrade,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
struct Marker {
    install: InstallKind,
}

/// Whether app data from an earlier run exists: any entry in any of `dirs`. Read FIRST in setup,
/// before this launch writes anything. Errs towards "existing" (an unreadable folder counts), so a
/// doubtful case keeps the defaults off.
pub fn had_existing_data(dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|d| match std::fs::read_dir(d) {
        Ok(mut rd) => rd.next().is_some(),
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    })
}

/// Create `path` with `bytes` (0600), never replacing a file already there, even one created a
/// moment ago by a member's save: the bytes go to a private temp file, which is hard-linked into
/// place (the link fails when `path` exists) and then removed. `Ok(false)`: `path` already existed.
pub fn write_new_file(path: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "new-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let res = opts
        .open(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::hard_link(&tmp, path));
    let _ = std::fs::remove_file(&tmp);
    match res {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

/// The install kind kept in the marker, or `None` when there is no readable marker.
pub fn read_marker(hermes_dir: &Path) -> Option<InstallKind> {
    let t = std::fs::read_to_string(hermes_dir.join(MARKER_FILE)).ok()?;
    serde_json::from_str::<Marker>(&t).ok().map(|m| m.install)
}

/// Decide the install kind exactly once. With a marker, it is what the marker says. Without one:
/// `New` only when `had_existing_data` is false, then the first-run defaults are written; the
/// marker is written either way (without replacing one another launch wrote first). Returns the
/// kind and what was written.
pub fn first_run(
    hermes_dir: &Path,
    node_mcp_dir: Option<&Path>,
    had_existing_data: bool,
) -> (InstallKind, Vec<String>) {
    if let Some(k) = read_marker(hermes_dir) {
        return (k, Vec::new());
    }
    // A marker that exists but does not parse is not rewritten (and counts as an upgrade).
    if hermes_dir.join(MARKER_FILE).exists() {
        return (InstallKind::Upgrade, Vec::new());
    }
    let kind = if had_existing_data {
        InstallKind::Upgrade
    } else {
        InstallKind::New
    };
    let body = serde_json::to_vec(&Marker { install: kind }).unwrap_or_default();
    match write_new_file(&hermes_dir.join(MARKER_FILE), &body) {
        Ok(true) => {}
        // Another launch decided first: follow it.
        Ok(false) => {
            return (
                read_marker(hermes_dir).unwrap_or(InstallKind::Upgrade),
                Vec::new(),
            )
        }
        Err(e) => {
            eprintln!(
                "hermes setup: the install marker was not written ({e}); nothing is turned on"
            );
            return (InstallKind::Upgrade, Vec::new());
        }
    }
    let wrote = if kind == InstallKind::New {
        provision_defaults(hermes_dir, node_mcp_dir)
    } else {
        Vec::new()
    };
    (kind, wrote)
}

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
    /// `hermes/mcp-settings.json` exists but does not parse (it then counts as all off).
    pub mcp_settings_error: Option<String>,
    /// `node-mcp/config.json` exists but does not parse (the server then counts as off).
    pub node_config_error: Option<String>,
    /// The install kind from the marker (`None` when there is none yet).
    pub install: Option<InstallKind>,
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
    /// The install kind from the marker (`None` when there is none yet).
    pub install: Option<InstallKind>,
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

/// The first-run defaults (new installs only, from [`first_run`]): each file is created only when
/// it does not exist, atomically, never replacing one. Returns what was written.
pub fn provision_defaults(hermes_dir: &Path, node_mcp_dir: Option<&Path>) -> Vec<String> {
    let mut wrote = Vec::new();
    let s = McpSettings {
        node: true,
        ..McpSettings::default()
    };
    let body = serde_json::to_vec_pretty(&s).unwrap_or_default();
    match write_new_file(&crate::hermes_mcp::settings_path(hermes_dir), &body) {
        Ok(true) => wrote.push("Hermes is offered the node's MCP read tools".to_string()),
        Ok(false) => {}
        Err(e) => eprintln!("hermes setup: the MCP default was not written: {e}"),
    }
    if let Some(dir) = node_mcp_dir {
        let body = serde_json::to_vec_pretty(&crate::node_mcp::NodeMcpConfig {
            enabled: true,
            port: None,
        })
        .unwrap_or_default();
        match write_new_file(&dir.join("config.json"), &body) {
            Ok(true) => wrote
                .push("the node's MCP server is on (127.0.0.1 only, token required)".to_string()),
            Ok(false) => {}
            Err(e) => eprintln!("hermes setup: the node MCP default was not written: {e}"),
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

/// Check the installed starter capsules against this build's pins, writing nothing.
pub fn verify_capsules(hermes_dir: &Path) -> SeedReport {
    let mut r = SeedReport::default();
    for (name, pin) in BUNDLED_CAPSULES {
        match crate::capsule_pins::verify_cps(&hermes_dir.join("capsules").join(name), name, pin) {
            Ok(()) => r.kept.push(name.to_string()),
            Err(e) => r.refused.push((name.to_string(), e)),
        }
    }
    r.present = r.kept.len();
    r
}

/// Why a JSON file that exists does not parse as `T` (`None`: absent or fine).
fn parse_error<T: serde::de::DeserializeOwned>(path: &Path) -> Option<String> {
    let t = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<T>(&t)
        .err()
        .map(|e| format!("{} does not parse ({e})", path.display()))
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
    out.push(match &first_party {
        None => item(
            "first-party-skills",
            "Citrate skills (SKILL.md)",
            Required,
            Missing,
            "the app's resource folder could not be read",
        ),
        Some(dir) => {
            let gone: Vec<&str> = FIRST_PARTY_SKILLS
                .iter()
                .copied()
                .filter(|n| !dir.join(n).join("SKILL.md").is_file())
                .collect();
            if gone.is_empty() {
                item(
                    "first-party-skills",
                    "Citrate skills (SKILL.md)",
                    Required,
                    ItemState::Ok,
                    format!("all {} bundled", FIRST_PARTY_SKILLS.len()),
                )
            } else {
                item(
                    "first-party-skills",
                    "Citrate skills (SKILL.md)",
                    Required,
                    Missing,
                    format!("not found under {}: {}", dir.display(), gone.join(", ")),
                )
            }
        }
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

    let corrupt: Vec<&String> = [&f.mcp_settings_error, &f.node_config_error]
        .into_iter()
        .flatten()
        .collect();
    out.push(match &f.node_mcp {
        _ if !corrupt.is_empty() => item(
            "node-mcp",
            "Node MCP tools",
            Required,
            Missing,
            format!(
                "{}; it counts as off. Turn node tools on again to rewrite it",
                corrupt
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ),
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
pub fn report(
    items: Vec<SetupItem>,
    first_run_defaults: Vec<String>,
    install: Option<InstallKind>,
) -> SetupReport {
    let missing_required = items
        .iter()
        .filter(|i| i.need == Need::Required && i.state == ItemState::Missing)
        .count();
    SetupReport {
        items,
        missing_required,
        first_run_defaults,
        install,
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

/// Folders and capsules (the repair half of provisioning). Never fails: what could not be done
/// is reported by the self-check.
fn provision_files<R: tauri::Runtime>(app: &tauri::AppHandle<R>, dir: &Path) {
    use tauri::Manager;
    for (d, e) in provision_dirs(dir) {
        eprintln!("hermes setup: folder {d} not created: {e}");
    }
    let resources = app.path().resource_dir().ok();
    if let Some(r) = provision_capsules(dir, resources.as_deref()) {
        for (name, why) in &r.refused {
            eprintln!("hermes setup: starter capsule {name} not installed: {why}");
        }
    }
}

/// Launch provisioning (see the module doc). `had_existing_data` is [`had_existing_data`] read at
/// the very start of setup. Never fails the launch.
pub fn provision<R: tauri::Runtime>(app: &tauri::AppHandle<R>, had_existing_data: bool) {
    let Ok(dir) = hermes_dir(app) else {
        eprintln!("hermes setup: no app data folder; the self-check will say so");
        return;
    };
    provision_files(app, &dir);
    let (_, wrote) = first_run(&dir, node_mcp_dir(app).as_deref(), had_existing_data);
    *LAUNCH_DEFAULTS.lock().unwrap_or_else(|e| e.into_inner()) = wrote;
}

/// The folders `had_existing_data` reads at launch (app data and app local data).
pub fn data_roots<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Vec<PathBuf> {
    use tauri::Manager;
    let mut v: Vec<PathBuf> = [
        app.path().app_data_dir().ok(),
        app.path().app_local_data_dir().ok(),
    ]
    .into_iter()
    .flatten()
    .collect();
    v.dedup();
    v
}

/// Gather the facts and build the report. Read only.
pub fn check<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<SetupReport, String> {
    use tauri::Manager;
    let dir = hermes_dir(app)?;
    let resources = app.path().resource_dir().ok();
    let seed = resources.as_ref().map(|_| verify_capsules(&dir));
    let node_mcp = app.try_state::<crate::node_mcp::NodeMcpState>().map(|s| {
        let st = s.status();
        NodeMcpFacts {
            enabled: st.enabled,
            running: st.running,
            endpoint: st.endpoint,
            last_error: st.last_error,
        }
    });
    let node_config_error = node_mcp_dir(app)
        .and_then(|d| parse_error::<crate::node_mcp::NodeMcpConfig>(&d.join("config.json")));
    let facts = SetupFacts {
        hermes_bin: crate::hermes::resolve_hermes_bin(app),
        embed_model: crate::embed_serve::resolve_model(resources.as_deref()),
        llama_bin: crate::serve::resolve_llama_bin(app).ok(),
        mcp_settings: crate::hermes_mcp::load_settings(&dir),
        mcp_settings_error: parse_error::<McpSettings>(&crate::hermes_mcp::settings_path(&dir)),
        node_config_error,
        install: read_marker(&dir),
        hermes_dir: dir,
        resources,
        seed,
        node_mcp,
        dir_errors: Vec::new(),
    };
    let defaults = LAUNCH_DEFAULTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let install = facts.install;
    Ok(report(items(&facts), defaults, install))
}

/// The explicit repair: create the folders and reinstall the pinned capsules, then check.
pub fn repair<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<SetupReport, String> {
    let dir = hermes_dir(app)?;
    provision_files(app, &dir);
    check(app)
}

/// The member's "Turn on node tools": the node's MCP server on, and Hermes offered its read tools
/// (the member's other MCP choices kept). Applies at Hermes's next start.
pub fn enable_node_tools(app: &tauri::AppHandle) -> Result<SetupReport, String> {
    use tauri::Manager;
    let node = app
        .try_state::<crate::node_mcp::NodeMcpState>()
        .ok_or_else(|| "the node MCP server is not set up in this app".to_string())?;
    node.set_enabled(true)?;
    let dir = hermes_dir(app)?;
    let s = McpSettings {
        node: true,
        ..crate::hermes_mcp::load_settings(&dir)
    };
    crate::hermes_mcp::hermes_mcp_set_sync(app.clone(), s)?;
    check(app)
}

/// `hermes_setup_check` — the Hermes self-check (read only).
#[tauri::command]
pub async fn hermes_setup_check(app_h: tauri::AppHandle) -> Result<SetupReport, String> {
    crate::blocking::off_main(move || check(&app_h)).await
}

/// `hermes_setup_repair` — recreate the folders and reinstall the pinned capsules, then check.
#[tauri::command]
pub async fn hermes_setup_repair(app_h: tauri::AppHandle) -> Result<SetupReport, String> {
    crate::blocking::off_main(move || repair(&app_h)).await
}

/// `hermes_setup_enable_node_tools` — the member turns the node tools on (upgraded installs).
#[tauri::command]
pub async fn hermes_setup_enable_node_tools(
    app_h: tauri::AppHandle,
) -> Result<SetupReport, String> {
    crate::blocking::off_main(move || enable_node_tools(&app_h)).await
}

#[cfg(test)]
mod tests {
    include!("hermes_setup_tests.rs");
}
