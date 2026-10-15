//! HGW-W3 (core half): Hermes's projects folder, `~/Citrate/Projects`.
//!
//! Owner decision 2026-10-15 (HGW runtime decisions): `project_create` lives in the runtime
//! (citrate-agent-runtime `agent-sidecar` `projects.rs`, runtime #81). Core does not host a second
//! tool with that name. Core's part:
//!
//! - **The switch and the folder:** the sidecar offers `project_create` when it starts with
//!   `CITRATE_HERMES_PROJECTS=1` and `CITRATE_HERMES_PROJECTS_ROOT=<abs>` ([`sidecar_env`]). When
//!   core ships the `citrate-templates` program next to the app, it also passes the renderer, the
//!   bundled templates folder and the machine's tier, so a project can start from a template.
//! - **The workspace card:** the sidecar creates nothing unless the projects folder exists and is
//!   granted for reading and writing (subtree). Core shows one card, "Let Hermes create and work in
//!   ~/Citrate/Projects"; on Approve it creates the folder and adds the two folder grants through
//!   the grant store ([`grant_workspace`]), recorded and sent to open conversations like any grant
//!   from Settings > Folder access, where the member can revoke them.
//! - **Known projects:** each folder Hermes created (the tool's returned `path`) is remembered in
//!   `<app local data>/hermes/projects.json` ([`record_project`]) after core checks it is a direct
//!   child of the projects folder carrying Hermes's project marker. Later conversations send them
//!   as `projectRoots` ([`with_project_roots`]), so Hermes may keep changing their build
//!   configuration. A folder is sent only while the workspace grant is live and the folder exists
//!   (the sidecar refuses a session whose project root is not granted).
//!
//! Data sources (Rule 7): the grant store (`agent-grants.json`, [`crate::agent_grants`]), the
//! filesystem (the projects folder and each project's `.citrate/project.json`), and the known
//! projects file. Nothing here runs a command, signs or holds a key (Rule 3).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent_grants::{Access, GrantKind, GrantState};

/// The sidecar's switch (runtime `projects::PROJECTS_ENV`): exactly `1` offers `project_create`.
pub const PROJECTS_ENV: &str = "CITRATE_HERMES_PROJECTS";
/// The folder new projects go in (runtime `projects::PROJECTS_ROOT_ENV`).
pub const PROJECTS_ROOT_ENV: &str = "CITRATE_HERMES_PROJECTS_ROOT";
/// The `citrate-templates` program (runtime `projects::TEMPLATE_RENDERER_ENV`).
pub const TEMPLATE_RENDERER_ENV: &str = "CITRATE_HERMES_TEMPLATE_RENDERER";
/// The bundled templates folder (runtime `projects::TEMPLATES_ROOT_ENV`).
pub const TEMPLATES_ROOT_ENV: &str = "CITRATE_HERMES_TEMPLATES_ROOT";
/// The machine's tier for the renderer (runtime `projects::TEMPLATE_TIER_ENV`).
pub const TEMPLATE_TIER_ENV: &str = "CITRATE_HERMES_TEMPLATE_TIER";
/// Every variable [`sidecar_env`] may emit; the Hermes manager drops anything else.
pub const SIDECAR_ENV_KEYS: &[&str] = &[
    PROJECTS_ENV,
    PROJECTS_ROOT_ENV,
    TEMPLATE_RENDERER_ENV,
    TEMPLATES_ROOT_ENV,
    TEMPLATE_TIER_ENV,
];
/// The projects folder under the member's home.
pub const PROJECTS_DIR: &str = "Citrate/Projects";
/// The known-projects file inside the Hermes data folder.
pub const PROJECTS_FILE: &str = "projects.json";
/// The marker the runtime writes into a project it created.
pub const MARKER: &str = ".citrate/project.json";
/// At most this many known projects are kept (and sent).
pub const MAX_KNOWN: usize = 256;

/// `~/Citrate/Projects` for `home`.
pub fn projects_root(home: &Path) -> PathBuf {
    home.join(PROJECTS_DIR)
}

/// What `project_create` needs to start a project from a bundled template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renderer {
    /// The bundled `citrate-templates` program.
    pub bin: PathBuf,
    /// The bundled templates folder.
    pub templates: PathBuf,
    /// `T0`, `T1` or `T2`.
    pub tier: String,
}

/// The sidecar environment: the switch, the projects folder and, when bundled, the renderer.
pub fn sidecar_env(root: &Path, renderer: Option<&Renderer>) -> Vec<(String, String)> {
    let mut env = vec![
        (PROJECTS_ENV.to_string(), "1".to_string()),
        (PROJECTS_ROOT_ENV.to_string(), root.display().to_string()),
    ];
    if let Some(r) = renderer.filter(|r| r.bin.is_file() && r.templates.is_dir()) {
        env.push((
            TEMPLATE_RENDERER_ENV.to_string(),
            r.bin.display().to_string(),
        ));
        env.push((
            TEMPLATES_ROOT_ENV.to_string(),
            r.templates.display().to_string(),
        ));
        env.push((TEMPLATE_TIER_ENV.to_string(), r.tier.clone()));
    }
    env
}

/// The production env source, read at each Hermes start.
pub fn file_env_source(
    root: PathBuf,
    renderer: impl Fn() -> Option<Renderer> + Send + Sync + 'static,
) -> crate::hermes_web::EnvSource {
    std::sync::Arc::new(move || sidecar_env(&root, renderer().as_ref()))
}

/// The folder as the grant store compares it (symlinks resolved), or the path itself when it
/// does not exist yet.
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Whether `doc` has live read and write subtree folder grants covering `root`.
pub fn workspace_granted(doc: &GrantState, root: &Path, now: u64) -> bool {
    let root = canonical(root);
    let covers = |access: Access| {
        doc.grants.iter().any(|g| {
            g.kind == GrantKind::Folder
                && g.access == access
                && g.scope == "subtree"
                && g.revoked_at.is_none()
                && g.granted_at <= now
                && g.expires_at.is_none_or(|t| now < t)
                && root.starts_with(Path::new(&g.root))
        })
    };
    covers(Access::Read) && covers(Access::Write)
}

/// What the workspace card and Settings read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    pub root: String,
    pub exists: bool,
    pub granted: bool,
}

/// The projects folder's state against the grant document.
pub fn workspace_view(doc: &GrantState, root: &Path, now: u64) -> WorkspaceView {
    WorkspaceView {
        root: root.display().to_string(),
        exists: root.is_dir(),
        granted: root.is_dir() && workspace_granted(doc, root, now),
    }
}

/// Create the projects folder (owner-only on Unix) when it is missing.
pub fn ensure_root(root: &Path) -> Result<(), String> {
    if root.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(root)
        .map_err(|e| format!("{} could not be created: {e}", root.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("{} could not be made private: {e}", root.display()))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct KnownFile {
    version: u32,
    projects: Vec<String>,
}

fn load_known(hermes_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(hermes_dir.join(PROJECTS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<KnownFile>(&t).ok())
        .map(|k| k.projects)
        .unwrap_or_default()
}

fn save_known(hermes_dir: &Path, projects: &[String]) -> Result<(), String> {
    std::fs::create_dir_all(hermes_dir)
        .map_err(|_| "cannot create the Hermes data folder".to_string())?;
    let text = serde_json::to_string_pretty(&KnownFile {
        version: 1,
        projects: projects.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    let tmp = hermes_dir.join(format!("{PROJECTS_FILE}.tmp"));
    std::fs::write(&tmp, text).map_err(|_| "cannot write the projects list".to_string())?;
    std::fs::rename(&tmp, hermes_dir.join(PROJECTS_FILE))
        .map_err(|_| "cannot write the projects list".to_string())
}

/// `path` as a project Hermes created: a direct child of the projects folder, a real folder (not
/// a symlink), carrying a marker that says Hermes created it under this name. Its canonical path.
fn checked_project(root: &Path, path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("a project path must be absolute".into());
    }
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| format!("{} does not exist", path.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let canon = canonical(path);
    let root = canonical(root);
    if canon.parent() != Some(root.as_path()) {
        return Err(format!(
            "{} is not a project folder directly inside {}",
            path.display(),
            root.display()
        ));
    }
    let marker: serde_json::Value = std::fs::read_to_string(canon.join(MARKER))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or_else(|| format!("{} has no Hermes project marker", path.display()))?;
    let name = canon
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if marker.get("createdBy").and_then(|v| v.as_str()) != Some("hermes")
        || marker.get("name").and_then(|v| v.as_str()) != Some(name)
    {
        return Err(format!(
            "{} was not created by Hermes under this name",
            path.display()
        ));
    }
    Ok(canon)
}

/// Remember a project Hermes created (newest last; duplicates and missing folders dropped).
/// Returns the known list.
pub fn record_project(hermes_dir: &Path, root: &Path, path: &Path) -> Result<Vec<String>, String> {
    let canon = checked_project(root, path)?.display().to_string();
    let mut known: Vec<String> = load_known(hermes_dir)
        .into_iter()
        .filter(|p| p != &canon && Path::new(p).is_dir())
        .collect();
    known.push(canon);
    if known.len() > MAX_KNOWN {
        known.drain(..known.len() - MAX_KNOWN);
    }
    save_known(hermes_dir, &known)?;
    Ok(known)
}

/// The known projects to send with a conversation: only while the workspace grant is live, and
/// only folders that still pass [`checked_project`].
pub fn project_roots(hermes_dir: &Path, root: &Path, doc: &GrantState, now: u64) -> Vec<String> {
    if !root.is_dir() || !workspace_granted(doc, root, now) {
        return Vec::new();
    }
    load_known(hermes_dir)
        .into_iter()
        .filter_map(|p| checked_project(root, Path::new(&p)).ok())
        .map(|p| p.display().to_string())
        .collect()
}

/// Add `projectRoots` to a `POST /sessions` body that carries its grant document (`grants`).
/// Nothing is added when there are none to send.
pub fn with_project_roots(
    body: &str,
    hermes_dir: &Path,
    root: &Path,
    now: u64,
) -> Result<String, String> {
    let mut v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "internal: session body is not JSON".to_string())?;
    let doc: GrantState = match v.get("grants") {
        Some(g) => serde_json::from_value(g.clone()).unwrap_or_default(),
        None => return Ok(body.to_string()),
    };
    let roots = project_roots(hermes_dir, root, &doc, now);
    if roots.is_empty() {
        return Ok(body.to_string());
    }
    if let Some(o) = v.as_object_mut() {
        o.insert("projectRoots".into(), serde_json::json!(roots));
    }
    Ok(v.to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn app_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(projects_root(
        &app.path().home_dir().map_err(|e| e.to_string())?,
    ))
}

/// Grant the projects folder for reading and writing (creating it first), unless it already is.
/// Recorded and sent to open conversations like a grant made in Settings > Folder access.
pub fn grant_workspace(app: &tauri::AppHandle) -> Result<WorkspaceView, String> {
    let root = app_root(app)?;
    let store = crate::agent_grants::GrantStore::for_app(app)?;
    if workspace_view(&store.document_for_agent(), &root, now_secs()).granted {
        return Ok(workspace_view(
            &store.document_for_agent(),
            &root,
            now_secs(),
        ));
    }
    ensure_root(&root)?;
    crate::agent_grants::grant_folder_for_app(app, &root, true, true)?;
    Ok(workspace_view(
        &store.document_for_agent(),
        &root,
        now_secs(),
    ))
}

/// **hermes_projects_workspace**: the projects folder, whether it exists and whether Hermes may
/// work in it (main window only).
#[tauri::command]
pub async fn hermes_projects_workspace(app: tauri::AppHandle) -> Result<WorkspaceView, String> {
    crate::blocking::off_main(move || {
        let root = app_root(&app)?;
        let store = crate::agent_grants::GrantStore::for_app(&app)?;
        Ok(workspace_view(
            &store.document_for_agent(),
            &root,
            now_secs(),
        ))
    })
    .await
}

/// **hermes_projects_workspace_grant**: the member approved the workspace card: create
/// `~/Citrate/Projects` and grant it for reading and writing (main window only).
#[tauri::command]
pub async fn hermes_projects_workspace_grant(
    app: tauri::AppHandle,
) -> Result<WorkspaceView, String> {
    crate::blocking::off_main(move || grant_workspace(&app)).await
}

/// **hermes_projects_record**: remember a project Hermes just created (the path `project_create`
/// returned), so later conversations may keep working on it (main window only).
#[tauri::command]
pub async fn hermes_projects_record(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<String>, String> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        let dir = app
            .path()
            .app_local_data_dir()
            .map_err(|e| e.to_string())?
            .join("hermes");
        record_project(&dir, &app_root(&app)?, Path::new(&path))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_projects_tests.rs");
}
