//! HGW-W3 (core #289, core half): Hermes's projects folder, the projects core keeps, and the
//! sidecar environment for the runtime's `project_create` tool.
//!
//! **The tool lives in the runtime** (owner decision 2026-10-15; citrate-agent-runtime #81,
//! `agent-sidecar/src/projects.rs`). Core offers no `project_create` of its own: a core tool with
//! that name would be refused at session create while `CITRATE_HERMES_PROJECTS=1`. Core's part:
//!
//! - **The workspace** (`~/Citrate/Projects`): created only when the member allows it on the
//!   workspace card ("Let Hermes create and work in ~/Citrate/Projects"), and granted for reading
//!   and writing (subtree) through the member's grant store, so the decision is recorded and sent
//!   to every open Hermes conversation like any grant, and is revocable in Settings > Folder
//!   access. A grant that cannot be made leaves nothing on disk.
//! - **The projects list**: each folder Hermes created (`project_create`'s returned `path`, checked
//!   against the projects root and the `.citrate/project.json` marker the runtime writes) and each
//!   folder the member marked as a project (it must be granted for reading and writing). Kept in
//!   `<app local data>/hermes/projects.json`. Forgetting one never deletes its files.
//! - **`projectRoots`**: every listed project that still exists and is still granted for reading
//!   and writing travels with each new Hermes session (`POST /sessions`). Only live roots are sent,
//!   because the runtime refuses the whole session for a root it cannot accept.
//! - **The sidecar environment** at each Hermes start: `CITRATE_HERMES_PROJECTS=1`, the projects
//!   root, the bundled `citrate-templates` renderer with the bundled contract templates and this
//!   machine's tier (pinned empty when the renderer is not bundled), and the member's own git
//!   identity (`user.name` / `user.email` from their global git config) for the commits Hermes
//!   makes (owner decision 2026-10-15; pinned empty when unset, and the runtime then uses Hermes).
//!
//! Data sources (Rule 7): the grant store (`agent_grants.rs`), `projects.json`, the filesystem
//! (the projects root, each project's marker), the bundled renderer and templates
//! (`template_forge::templates_root`, the `citrate-templates` externalBin), the hardware tier
//! (`tier.rs`) and the member's global git config (read with git, outside any sandbox, with only
//! `HOME` set). Nothing here signs, spends or holds a key (Rule 3).

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent_grants::GrantsView;

/// The list of projects inside the Hermes data folder.
pub const PROJECTS_FILE: &str = "projects.json";
/// The projects folder under the member's home (runtime `projects::DEFAULT_PROJECTS_DIR`).
pub const DEFAULT_PROJECTS_DIR: &str = "Citrate/Projects";
/// The marker the runtime writes into every project it creates.
pub const MARKER_FILE: &str = ".citrate/project.json";
/// Most projects kept.
pub const MAX_PROJECTS: usize = 256;
/// Longest project path kept.
const MAX_PATH_CHARS: usize = 4096;

/// `1` offers the runtime's `project_create` (runtime `projects::PROJECTS_ENV`).
pub const PROJECTS_ENV: &str = "CITRATE_HERMES_PROJECTS";
/// The projects folder (absolute).
pub const PROJECTS_ROOT_ENV: &str = "CITRATE_HERMES_PROJECTS_ROOT";
/// The `citrate-templates` program (absolute).
pub const TEMPLATE_RENDERER_ENV: &str = "CITRATE_HERMES_TEMPLATE_RENDERER";
/// The bundled templates folder (absolute).
pub const TEMPLATES_ROOT_ENV: &str = "CITRATE_HERMES_TEMPLATES_ROOT";
/// This machine's tier for template budgets (`T0` | `T1` | `T2`).
pub const TEMPLATE_TIER_ENV: &str = "CITRATE_HERMES_TEMPLATE_TIER";
/// The member's commit identity (runtime `git_identity::GIT_USER_NAME_ENV` / `_EMAIL_ENV`).
pub const GIT_USER_NAME_ENV: &str = "CITRATE_HERMES_GIT_USER_NAME";
pub const GIT_USER_EMAIL_ENV: &str = "CITRATE_HERMES_GIT_USER_EMAIL";

/// Every variable [`sidecar_env`] may emit; the Hermes manager drops anything else.
pub const SIDECAR_ENV_KEYS: &[&str] = &[
    PROJECTS_ENV,
    PROJECTS_ROOT_ENV,
    TEMPLATE_RENDERER_ENV,
    TEMPLATES_ROOT_ENV,
    TEMPLATE_TIER_ENV,
    GIT_USER_NAME_ENV,
    GIT_USER_EMAIL_ENV,
];

/// Longest git name or address passed (the runtime's own bound).
const MAX_IDENTITY_BYTES: usize = 256;

/// Who made a folder a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Hermes created it with `project_create` (the member approved its card).
    Hermes,
    /// The member marked an existing folder as a project.
    Member,
}

/// One kept project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectEntry {
    /// Absolute, symlink-resolved folder.
    pub path: String,
    pub name: String,
    pub origin: Origin,
    /// Unix seconds.
    pub added_at: u64,
}

/// `projects.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectsDoc {
    pub version: u32,
    pub projects: Vec<ProjectEntry>,
}

impl Default for ProjectsDoc {
    fn default() -> Self {
        ProjectsDoc {
            version: 1,
            projects: Vec::new(),
        }
    }
}

/// One row of the Projects list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub path: String,
    pub name: String,
    pub origin: Origin,
    pub added_at: u64,
    /// The folder is still there.
    pub exists: bool,
    /// It is granted for reading and writing, so it goes to Hermes as a project.
    pub shared: bool,
}

/// What the Projects list shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsView {
    /// The projects folder (`~/Citrate/Projects`).
    pub root: String,
    pub root_exists: bool,
    /// The folder exists and is granted for reading and writing: Hermes can create projects.
    pub workspace_ready: bool,
    pub projects: Vec<ProjectRow>,
    /// Set when `projects.json` could not be read (then no project goes to Hermes).
    pub load_error: Option<String>,
}

/// The projects folder under `home`.
pub fn projects_root(home: &Path) -> PathBuf {
    home.join(DEFAULT_PROJECTS_DIR)
}

/// An absolute, already normalized path (no `.`/`..`), at most [`MAX_PATH_CHARS`].
fn plain_absolute(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    let plain = p.components().all(|c| {
        matches!(
            c,
            Component::RootDir | Component::Prefix(_) | Component::Normal(_)
        )
    });
    if !p.is_absolute() || !plain || path.chars().count() > MAX_PATH_CHARS || path.contains('\0') {
        return Err(format!("{path} is not a plain absolute folder path"));
    }
    Ok(p)
}

/// Whether live (active, not blocked) read and write folder grants cover `canon`.
pub fn covered_rw(grants: &GrantsView, canon: &Path) -> bool {
    if grants.status != "ok" || crate::agent_grants::denied_location(canon).is_some() {
        return false;
    }
    let has = |access: &str| {
        grants.grants.iter().any(|g| {
            g.kind == "folder"
                && g.status == "active"
                && g.access == access
                && canon.starts_with(Path::new(&g.root))
        })
    };
    has("read") && has("write")
}

/// The member's projects list.
#[derive(Debug, Clone)]
pub struct ProjectStore {
    dir: PathBuf,
    home: PathBuf,
}

impl ProjectStore {
    /// A list in `dir` (the Hermes data folder) for the member whose home is `home`.
    pub fn new(dir: impl Into<PathBuf>, home: impl Into<PathBuf>) -> Self {
        ProjectStore {
            dir: dir.into(),
            home: home.into(),
        }
    }

    /// The production list: `<app local data>/hermes/projects.json`, home from the OS.
    pub fn for_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Self, String> {
        use tauri::Manager;
        let dir = app
            .path()
            .app_local_data_dir()
            .map_err(|e| e.to_string())?
            .join("hermes");
        let home = app.path().home_dir().map_err(|e| e.to_string())?;
        Ok(ProjectStore::new(dir, home))
    }

    /// `~/Citrate/Projects`.
    pub fn root(&self) -> PathBuf {
        projects_root(&self.home)
    }

    fn file(&self) -> PathBuf {
        self.dir.join(PROJECTS_FILE)
    }

    /// The list. Missing = empty; unreadable or invalid is an error (then no project is sent).
    pub fn load(&self) -> Result<ProjectsDoc, String> {
        let text = match std::fs::read_to_string(self.file()) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ProjectsDoc::default()),
            Err(e) => {
                return Err(format!(
                    "the projects list could not be read ({})",
                    e.kind()
                ))
            }
        };
        let doc: ProjectsDoc = serde_json::from_str(&text).map_err(|_| {
            "the projects list is not valid; no project is sent to Hermes".to_string()
        })?;
        if doc.version != 1 || doc.projects.len() > MAX_PROJECTS {
            return Err("the projects list is not valid; no project is sent to Hermes".into());
        }
        Ok(doc)
    }

    fn save(&self, doc: &ProjectsDoc) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|_| "cannot create the Hermes data folder".to_string())?;
        let text = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
        let tmp = self.dir.join(format!("{PROJECTS_FILE}.tmp"));
        std::fs::write(&tmp, text).map_err(|_| "cannot write the projects list".to_string())?;
        std::fs::rename(&tmp, self.file()).map_err(|_| "cannot write the projects list".to_string())
    }

    /// Add or refresh `entry` (one entry per path).
    fn upsert(&self, entry: ProjectEntry) -> Result<ProjectEntry, String> {
        let mut doc = self.load()?;
        if let Some(e) = doc.projects.iter_mut().find(|e| e.path == entry.path) {
            // A project keeps where it came from; the member marking a Hermes project changes
            // nothing.
            return Ok(e.clone());
        }
        if doc.projects.len() >= MAX_PROJECTS {
            return Err(format!(
                "at most {MAX_PROJECTS} projects are kept; forget one first"
            ));
        }
        doc.projects.push(entry.clone());
        self.save(&doc)?;
        Ok(entry)
    }

    /// Record a project Hermes created (`project_create`'s returned path). Only a folder directly
    /// inside the projects root that carries the runtime's marker is accepted.
    pub fn record_created(&self, path: &str, now: u64) -> Result<ProjectEntry, String> {
        let p = plain_absolute(path)?;
        let canon =
            std::fs::canonicalize(&p).map_err(|_| format!("{path} is not an existing folder"))?;
        let root = std::fs::canonicalize(self.root())
            .map_err(|_| "the projects folder does not exist".to_string())?;
        if !canon.is_dir() || canon.parent() != Some(root.as_path()) {
            return Err(format!(
                "{path} is not a project folder inside {}",
                root.display()
            ));
        }
        let marker: serde_json::Value = std::fs::read_to_string(canon.join(MARKER_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .ok_or_else(|| format!("{path} has no project marker from Hermes"))?;
        if marker.get("createdBy").and_then(|v| v.as_str()) != Some("hermes") {
            return Err(format!("{path} was not created by Hermes"));
        }
        let name = canon
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.upsert(ProjectEntry {
            path: canon.display().to_string(),
            name,
            origin: Origin::Hermes,
            added_at: now,
        })
    }

    /// The member marks an existing folder as a project. It must be granted for reading and
    /// writing (the runtime checks the same before it accepts the folder as a project root).
    pub fn mark(&self, path: &str, grants: &GrantsView, now: u64) -> Result<ProjectEntry, String> {
        let p = plain_absolute(path)?;
        let canon =
            std::fs::canonicalize(&p).map_err(|_| format!("{path} is not an existing folder"))?;
        if !canon.is_dir() {
            return Err(format!("{path} is not a folder"));
        }
        if !covered_rw(grants, &canon) {
            return Err(format!(
                "{} is not shared with Hermes for reading and writing; share it in Settings > Folder access first",
                canon.display()
            ));
        }
        let name = canon
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| canon.display().to_string());
        self.upsert(ProjectEntry {
            path: canon.display().to_string(),
            name,
            origin: Origin::Member,
            added_at: now,
        })
    }

    /// Take a project off the list. Its files are not touched.
    pub fn forget(&self, path: &str) -> Result<(), String> {
        let mut doc = self.load()?;
        let before = doc.projects.len();
        doc.projects.retain(|e| e.path != path);
        if doc.projects.len() == before {
            return Err(format!("{path} is not on the projects list"));
        }
        self.save(&doc)
    }

    fn row(e: &ProjectEntry, grants: &GrantsView) -> ProjectRow {
        let p = Path::new(&e.path);
        let exists = p.is_dir();
        // The stored path is canonical; a folder replaced by a link no longer counts.
        let canonical_now = std::fs::canonicalize(p).ok();
        let shared = exists && canonical_now.as_deref() == Some(p) && covered_rw(grants, p);
        ProjectRow {
            path: e.path.clone(),
            name: e.name.clone(),
            origin: e.origin,
            added_at: e.added_at,
            exists,
            shared,
        }
    }

    /// The Projects list.
    pub fn view(&self, grants: &GrantsView) -> ProjectsView {
        let root = self.root();
        let root_exists = root.is_dir();
        let workspace_ready = root_exists
            && std::fs::canonicalize(&root)
                .map(|c| covered_rw(grants, &c))
                .unwrap_or(false);
        let (projects, load_error) = match self.load() {
            Ok(doc) => (
                doc.projects.iter().map(|e| Self::row(e, grants)).collect(),
                None,
            ),
            Err(e) => (Vec::new(), Some(e)),
        };
        ProjectsView {
            root: root.display().to_string(),
            root_exists,
            workspace_ready,
            projects,
            load_error,
        }
    }

    /// The projects that go to a new Hermes session as `projectRoots`: on the list, still there
    /// and still granted for reading and writing.
    pub fn live_roots(&self, grants: &GrantsView) -> Vec<String> {
        self.view(grants)
            .projects
            .into_iter()
            .filter(|r| r.shared)
            .map(|r| r.path)
            .collect()
    }
}

/// The member allowed the workspace card: create `~/Citrate/Projects` and, unless it is already
/// granted for reading and writing, grant it with `add` (core's grant store: recorded, sent to
/// the open sessions). When the grant cannot be made, a folder made for it is removed again.
pub fn workspace_enable(
    store: &ProjectStore,
    grants: &GrantsView,
    add: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<ProjectsView, String> {
    let root = store.root();
    let created = !root.exists();
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("{} could not be created: {}", root.display(), e.kind()))?;
    let canon = std::fs::canonicalize(&root).map_err(|e| e.to_string())?;
    if !covered_rw(grants, &canon) {
        if let Err(e) = add(&canon) {
            if created {
                // `remove_dir` only removes an empty folder: nothing else is ever deleted.
                let _ = std::fs::remove_dir(&canon);
            }
            return Err(e);
        }
    }
    Ok(store.view(grants))
}

/// Add `projectRoots` to a `POST /sessions` body (only when there are some).
pub fn attach_project_roots(body: &str, roots: &[String]) -> Result<String, String> {
    if roots.is_empty() {
        return Ok(body.to_string());
    }
    let mut v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "internal: session body is not JSON".to_string())?;
    let obj = v
        .as_object_mut()
        .ok_or("internal: session body is not an object")?;
    obj.insert("projectRoots".into(), serde_json::json!(roots));
    Ok(v.to_string())
}

/// The bundled template renderer the runtime calls for `project_create {template}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renderer {
    /// The `citrate-templates` program.
    pub program: PathBuf,
    /// The bundled templates folder.
    pub templates: PathBuf,
    /// `T0` | `T1` | `T2`.
    pub tier: String,
}

/// A plain git name and address (the runtime's rule): name 1..=256 bytes and address
/// `local@domain` of at most 256 bytes, neither with `<`, `>` or control characters, the address
/// with no spaces.
pub fn valid_identity(name: &str, email: &str) -> Option<(String, String)> {
    let name = name.trim();
    let email = email.trim();
    let plain = |s: &str| !s.chars().any(|c| c.is_control() || c == '<' || c == '>');
    let address = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty());
    let ok = !name.is_empty()
        && name.len() <= MAX_IDENTITY_BYTES
        && plain(name)
        && address
        && email.len() <= MAX_IDENTITY_BYTES
        && plain(email)
        && !email.chars().any(char::is_whitespace);
    ok.then(|| (name.to_string(), email.to_string()))
}

/// The sidecar environment for projects at one Hermes start.
pub fn sidecar_env(
    home: &Path,
    renderer: Option<&Renderer>,
    identity: Option<(String, String)>,
) -> Vec<(String, String)> {
    let mut env = vec![
        (PROJECTS_ENV.to_string(), "1".to_string()),
        (
            PROJECTS_ROOT_ENV.to_string(),
            projects_root(home).display().to_string(),
        ),
    ];
    match renderer {
        Some(r) => {
            env.push((
                TEMPLATE_RENDERER_ENV.to_string(),
                r.program.display().to_string(),
            ));
            env.push((
                TEMPLATES_ROOT_ENV.to_string(),
                r.templates.display().to_string(),
            ));
            env.push((TEMPLATE_TIER_ENV.to_string(), r.tier.clone()));
        }
        None => {
            // Pinned empty: the runtime reads empty as unset, and an inherited value can never
            // name another program.
            env.push((TEMPLATE_RENDERER_ENV.to_string(), String::new()));
            env.push((TEMPLATES_ROOT_ENV.to_string(), String::new()));
        }
    }
    let (name, email) = identity.unwrap_or_default();
    env.push((GIT_USER_NAME_ENV.to_string(), name));
    env.push((GIT_USER_EMAIL_ENV.to_string(), email));
    env
}

/// The git program to read the member's config with. On macOS `/usr/bin/git` is used only when
/// the command line tools are installed (otherwise it asks the member to install them).
pub fn git_program() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/git"),
        PathBuf::from("/usr/local/bin/git"),
    ];
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Library/Developer/CommandLineTools/usr/bin/git",
        ));
        if Path::new("/Library/Developer/CommandLineTools/usr/bin/git").is_file()
            || Path::new("/Applications/Xcode.app").is_dir()
        {
            candidates.push(PathBuf::from("/usr/bin/git"));
        }
    } else {
        candidates.push(PathBuf::from("/usr/bin/git"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// The member's `user.name` and `user.email` from their global git config, read with git outside
/// any sandbox, with an empty environment but `HOME` (no system config, no repository). `None`
/// unless both are set and valid.
pub fn member_git_identity(home: &Path) -> Option<(String, String)> {
    let git = git_program()?;
    let get = |key: &str| -> Option<String> {
        let out = std::process::Command::new(&git)
            .args(["config", "--global", "--get", key])
            .env_clear()
            .env("HOME", home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(home)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let v = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (!v.is_empty()).then_some(v)
    };
    let (name, email) = (get("user.name")?, get("user.email")?);
    valid_identity(&name, &email)
}

/// The bundled renderer, when both the program and the templates are in this build.
fn renderer_for<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<Renderer> {
    use tauri::Manager;
    let program = match std::env::var(TEMPLATES_BIN_ENV) {
        Ok(p) if Path::new(&p).is_absolute() && Path::new(&p).is_file() => PathBuf::from(p),
        _ => crate::supervisor::resolve_external_bin(app, "citrate-templates").ok()?,
    };
    let res = app.path().resource_dir().ok();
    let templates = crate::template_forge::templates_root(res.as_deref()).ok()?;
    // The machine's effective tier (the member's override, else the recommendation); T0 when it
    // cannot be read, the smallest template budgets.
    let tier = crate::tier::tier_recommend_sync(app.clone())
        .map(|r| {
            crate::template_forge::renderer_tier(r.effective)
                .id()
                .to_string()
        })
        .unwrap_or_else(|_| "T0".to_string());
    Some(Renderer {
        program,
        templates,
        tier,
    })
}

/// A developer override for the renderer program (an absolute path), like `CITRATE_HERMES_BIN`.
pub const TEMPLATES_BIN_ENV: &str = "CITRATE_TEMPLATES_BIN";

/// The production env source: projects on, the bundled renderer, and the member's git identity,
/// computed at each Hermes start.
pub fn file_env_source<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    home: PathBuf,
) -> crate::hermes_web::EnvSource {
    std::sync::Arc::new(move || {
        let renderer = renderer_for(&app);
        sidecar_env(&home, renderer.as_ref(), member_git_identity(&home))
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn grants_view<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<GrantsView, String> {
    Ok(crate::agent_grants::GrantStore::for_app(app)?.view(now_secs()))
}

/// A new session body with the live projects attached (called under the grant store's lock, so
/// the grants checked here are the ones the session receives).
pub fn session_body_with_projects<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    body: &str,
) -> Result<String, String> {
    let roots = ProjectStore::for_app(app)?.live_roots(&grants_view(app)?);
    attach_project_roots(body, &roots)
}

/// **hermes_projects_view**: the projects folder and the Projects list (main window only).
#[tauri::command]
pub async fn hermes_projects_view(app: tauri::AppHandle) -> Result<ProjectsView, String> {
    crate::blocking::off_main(move || Ok(ProjectStore::for_app(&app)?.view(&grants_view(&app)?)))
        .await
}

/// **hermes_projects_workspace_enable**: the member allowed the workspace card. Creates
/// `~/Citrate/Projects` and grants it for reading and writing through the grant store (recorded,
/// sent to open sessions). No path comes from the webview.
#[tauri::command]
pub async fn hermes_projects_workspace_enable(
    app: tauri::AppHandle,
) -> Result<ProjectsView, String> {
    crate::blocking::off_main(move || {
        let store = ProjectStore::for_app(&app)?;
        workspace_enable(&store, &grants_view(&app)?, |root| {
            crate::agent_grants::grant_folder_for_app(&app, root, true, true).map(|_| ())
        })?;
        Ok(store.view(&grants_view(&app)?))
    })
    .await
}

/// **hermes_projects_record_created**: keep a project Hermes created (the path from a
/// `project_create` result; checked against the projects root and the runtime's marker).
#[tauri::command]
pub async fn hermes_projects_record_created(
    app: tauri::AppHandle,
    path: String,
) -> Result<ProjectsView, String> {
    crate::blocking::off_main(move || {
        let store = ProjectStore::for_app(&app)?;
        store.record_created(&path, now_secs())?;
        Ok(store.view(&grants_view(&app)?))
    })
    .await
}

/// **hermes_projects_mark**: the member marks a folder (picked in the native dialog) as a project.
#[tauri::command]
pub async fn hermes_projects_mark(
    app: tauri::AppHandle,
    path: String,
) -> Result<ProjectsView, String> {
    crate::blocking::off_main(move || {
        let store = ProjectStore::for_app(&app)?;
        let grants = grants_view(&app)?;
        store.mark(&path, &grants, now_secs())?;
        Ok(store.view(&grants))
    })
    .await
}

/// **hermes_projects_forget**: take a project off the list (its files stay).
#[tauri::command]
pub async fn hermes_projects_forget(
    app: tauri::AppHandle,
    path: String,
) -> Result<ProjectsView, String> {
    crate::blocking::off_main(move || {
        let store = ProjectStore::for_app(&app)?;
        store.forget(&path)?;
        Ok(store.view(&grants_view(&app)?))
    })
    .await
}

#[cfg(test)]
#[path = "hermes_projects_tests.rs"]
mod tests;

// The live proof with the real sidecar and renderer (env-gated: CITRATE_E2E_PROJECTS_SIDECAR_BIN
// and CITRATE_E2E_TEMPLATES_BIN).
#[cfg(test)]
#[path = "hermes_projects_e2e_tests.rs"]
mod e2e_tests;
