//! HUP-S6.2 / US-6.4 — contract templates for members.
//!
//! The citrate-templates renderer (`templates/renderer`) fills a template under `templates/<id>/`
//! with strictly validated parameters (name, symbol, supply, price, owner) and this machine's
//! Medusa budget, and writes the project into an empty folder. This module is the member's path
//! to it:
//!
//! - **`template_list`** returns every bundled template with its parameter form (the fields, their
//!   defaults and bounds, in the style of the OpenZeppelin Wizard), plus the hardware tier in
//!   effect and its Medusa budget.
//! - **`template_render`** renders one template into a folder the member granted for writing
//!   (Settings > Folder access). A folder outside the grants is refused before anything is
//!   written. The renderer validates every value, writes nothing on a refusal and removes what it
//!   wrote if a write fails partway. The pinned Solidity libraries (OpenZeppelin, Solady,
//!   forge-std, at the commits in `templates/deps.lock.json`) are copied into `lib/` when the
//!   toolchain's library cache holds them at exactly those commits; otherwise the result says
//!   which are missing. Nothing is downloaded, compiled, deployed or signed here.
//!
//! The templates ship as bundle resources (`../templates/...` in every `tauri*.conf.json`, which
//! Tauri places under `_up_/templates`). A debug build falls back to the source tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use citrate_templates::{MedusaBudget, TemplateKind, TemplateSet, Tier as TemplateTier};
use serde::{Deserialize, Serialize};

/// Where the toolchain's pinned library checkouts live, under the app's local data folder.
pub const DEPS_CACHE_DIR: &str = "toolchain/deps";

/// The bundled template root inside the resource folder (Tauri maps `../templates` to
/// `_up_/templates`).
fn bundled_root(resource_dir: &Path) -> PathBuf {
    resource_dir.join("_up_").join("templates")
}

/// The template root: the bundled one, or (debug builds only) the source tree.
pub fn templates_root(resource_dir: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(res) = resource_dir {
        let p = bundled_root(res);
        if p.join("medusa-budgets.json").is_file() {
            return Ok(p);
        }
    }
    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
        if dev.join("medusa-budgets.json").is_file() {
            return Ok(dev);
        }
    }
    Err("the contract templates are not bundled in this build".to_string())
}

/// This machine's tier as the renderer names it.
pub fn renderer_tier(t: crate::tier::Tier) -> TemplateTier {
    match t {
        crate::tier::Tier::T0 => TemplateTier::T0,
        crate::tier::Tier::T1 => TemplateTier::T1,
        crate::tier::Tier::T2 => TemplateTier::T2,
    }
}

/// One field of a template's parameter form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateField {
    /// `name`, `symbol`, `supply`, `price` or `owner`.
    pub key: String,
    pub label: String,
    pub help: String,
    pub default: Option<String>,
    pub min: Option<u64>,
    pub max: Option<u64>,
    /// Every declared field is required unless it has a default.
    pub required: bool,
}

/// A template as the form shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEntry {
    pub id: String,
    /// `contract` or `dapp`.
    pub kind: String,
    pub title: String,
    pub description: String,
    pub fields: Vec<TemplateField>,
    /// Ships Medusa property tests.
    pub medusa: bool,
}

/// What `template_list` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateCatalog {
    pub templates: Vec<TemplateEntry>,
    /// The hardware tier in effect (`T0`/`T1`/`T2`).
    pub tier: String,
    /// This tier's Medusa budget (starting values, pending owner sign-off).
    pub medusa_budget: MedusaBudget,
}

/// The label and help line of a parameter (the renderer's rules, in plain words).
fn field_text(key: &str) -> (&'static str, &'static str) {
    match key {
        "name" => (
            "Name",
            "Letters, digits and spaces, up to 48 characters, starting with a letter.",
        ),
        "symbol" => (
            "Symbol",
            "Upper-case letters and digits, up to 11 characters.",
        ),
        "supply" => (
            "Supply",
            "A whole number of tokens within the shown bounds.",
        ),
        "price" => (
            "Price (SALT)",
            "The mint price in SALT, for example 0 or 0.01.",
        ),
        "owner" => (
            "Owner address",
            "The 0x address that receives the supply or owns the contract.",
        ),
        _ => ("Value", ""),
    }
}

/// The form fields of a template, in a fixed order.
pub fn fields_of(manifest: &citrate_templates::TemplateManifest) -> Vec<TemplateField> {
    const ORDER: [&str; 5] = ["name", "symbol", "supply", "price", "owner"];
    ORDER
        .iter()
        .filter_map(|k| manifest.params.get(*k).map(|spec| (*k, spec)))
        .map(|(k, spec)| {
            let (label, help) = field_text(k);
            TemplateField {
                key: k.to_string(),
                label: label.to_string(),
                help: help.to_string(),
                default: spec.default.clone(),
                min: spec.min,
                max: spec.max,
                required: spec.default.is_none(),
            }
        })
        .collect()
}

/// The catalog of `set` for `tier`.
pub fn catalog(set: &TemplateSet, tier: TemplateTier) -> TemplateCatalog {
    let templates = set
        .ids()
        .filter_map(|id| set.manifest(id))
        .map(|m| TemplateEntry {
            id: m.id.clone(),
            kind: match m.kind {
                TemplateKind::Contract => "contract".to_string(),
                TemplateKind::Dapp => "dapp".to_string(),
            },
            title: m.title.clone(),
            description: m.description.clone(),
            fields: fields_of(m),
            medusa: m.medusa,
        })
        .collect();
    TemplateCatalog {
        templates,
        tier: tier.id().to_string(),
        medusa_budget: set.budgets().for_tier(tier).clone(),
    }
}

// ------------------------------------------------------------------------ folder grants

/// The folder `path` will be written in, made canonical: `path` itself when it exists, else its
/// nearest existing ancestor joined with the missing rest. `..` is refused.
fn resolve_target(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("the output folder must be an absolute path".to_string());
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("the output folder may not contain '..'".to_string());
    }
    let mut existing = path.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name().map(|n| n.to_os_string()) else {
            return Err("the output folder has no existing parent".to_string());
        };
        rest.push(name);
        if !existing.pop() {
            return Err("the output folder has no existing parent".to_string());
        }
    }
    let mut out = existing
        .canonicalize()
        .map_err(|e| format!("cannot resolve the output folder: {}", e.kind()))?;
    for name in rest.into_iter().rev() {
        out.push(name);
    }
    Ok(out)
}

/// Whether an active folder grant with `access` covers `target` at `now` (unix seconds).
pub fn grant_covers(
    state: &crate::agent_grants::GrantState,
    target: &Path,
    access: crate::agent_grants::Access,
    now: u64,
) -> bool {
    use crate::agent_grants::GrantKind;
    state.grants.iter().any(|g| {
        g.kind == GrantKind::Folder
            && g.access == access
            && g.revoked_at.is_none()
            && now >= g.granted_at
            && g.expires_at.is_none_or(|t| now < t)
            && (g.scope == "subtree" && target.starts_with(&g.root)
                || g.scope == "shallow"
                    && (target == Path::new(&g.root)
                        || target.parent() == Some(Path::new(&g.root))))
    })
}

/// `path` (canonical) when an active folder grant with `access` covers it, else the refusal.
pub fn require_grant(
    store: &crate::agent_grants::GrantStore,
    path: &Path,
    access: crate::agent_grants::Access,
    now: u64,
) -> Result<PathBuf, String> {
    let target = resolve_target(path)?;
    let state = match store.load() {
        crate::agent_grants::Loaded::Ok(s) => s,
        crate::agent_grants::Loaded::Corrupted(_) => {
            return Err(
                "the folder-access settings could not be read, so nothing was written".to_string(),
            )
        }
    };
    if grant_covers(&state, &target, access, now) {
        Ok(target)
    } else {
        let what = match access {
            crate::agent_grants::Access::Read => "reading",
            crate::agent_grants::Access::Write => "writing",
        };
        Err(format!(
            "{} is not inside a folder you granted for {what}. Grant the folder in Settings > Folder access first.",
            target.display()
        ))
    }
}

// ------------------------------------------------------------------------ libraries

/// One pinned library and whether it was copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepStatus {
    pub name: String,
    pub commit: String,
    /// `lib/<name>` relative to the project.
    pub dir: String,
    pub installed: bool,
    /// Why it was not installed.
    pub note: Option<String>,
}

/// The commit a checkout's `HEAD` names (a detached head or a branch ref).
fn checkout_commit(dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(dir.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(r) = head.strip_prefix("ref: ") {
        let r = std::fs::read_to_string(dir.join(".git").join(r)).ok()?;
        return Some(r.trim().to_string());
    }
    Some(head.to_string())
}

/// Copy `src` to `dst` (regular files and folders only; `.git` and symlinks are skipped).
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut n = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let ft = entry.file_type()?;
        let to = dst.join(&name);
        if ft.is_dir() {
            n += copy_tree(&entry.path(), &to)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &to)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Copy each pinned library from `cache/<name>` into `project/<dir>` when the checkout is at the
/// pinned commit. `deps` is `templates/deps.lock.json`'s `deps` object.
pub fn install_deps(
    deps: &serde_json::Value,
    cache: Option<&Path>,
    project: &Path,
) -> Vec<DepStatus> {
    let mut out = Vec::new();
    let Some(map) = deps.as_object() else {
        return out;
    };
    for (name, d) in map {
        let commit = d
            .get("commit")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
        let dir = d
            .get("dir")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
        let mut st = DepStatus {
            name: name.clone(),
            commit: commit.clone(),
            dir: dir.clone(),
            installed: false,
            note: None,
        };
        let dir_ok = dir.starts_with("lib/")
            && Path::new(&dir)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)));
        if !dir_ok {
            st.note = Some("the lock names an unexpected folder".into());
            out.push(st);
            continue;
        }
        let Some(cache) = cache else {
            st.note = Some(
                "no library cache on this machine yet (the toolchain bundle supplies it)".into(),
            );
            out.push(st);
            continue;
        };
        let src = cache.join(name);
        match checkout_commit(&src) {
            None => st.note = Some("not in the library cache".into()),
            Some(c) if c != commit => {
                st.note = Some(format!(
                    "the cached checkout is at {c}, not the pinned commit"
                ))
            }
            Some(_) => match copy_tree(&src, &project.join(&dir)) {
                Ok(_) => st.installed = true,
                Err(e) => st.note = Some(format!("copy failed: {}", e.kind())),
            },
        }
        out.push(st);
    }
    out
}

// ------------------------------------------------------------------------ render

/// What the form sends.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateRenderInput {
    pub template: String,
    /// Parameter key → the member's text. Unknown keys are refused by the renderer.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Absolute folder to create (absent or empty), inside a folder granted for writing.
    pub out_dir: String,
}

/// What `template_render` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateRenderView {
    pub template: String,
    pub tier: String,
    pub out_dir: String,
    /// sha256 over the template's sources.
    pub digest: String,
    pub files: Vec<String>,
    /// Normalized parameters, plus the derived `contract` identifier.
    pub params: BTreeMap<String, String>,
    pub medusa_budget: Option<MedusaBudget>,
    /// The contract project inside the output (the output itself, or `contracts/` for a dApp).
    pub contract_dir: String,
    pub deps: Vec<DepStatus>,
}

fn deps_lock(root: &Path) -> Result<serde_json::Value, String> {
    let raw = std::fs::read_to_string(root.join("deps.lock.json"))
        .map_err(|e| format!("templates/deps.lock.json: {}", e.kind()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("templates/deps.lock.json: {e}"))?;
    Ok(v.get("deps").cloned().unwrap_or(serde_json::Value::Null))
}

/// Render after the grant check (`target` is the canonical output folder).
pub fn render_into(
    root: &Path,
    input: &TemplateRenderInput,
    tier: TemplateTier,
    target: &Path,
    deps_cache: Option<&Path>,
) -> Result<TemplateRenderView, String> {
    let set = TemplateSet::open(root).map_err(|e| e.to_string())?;
    let report = set
        .render(&input.template, &input.params, tier, target)
        .map_err(|e| e.to_string())?;
    // A dApp template renders its contract project into the include folder.
    let contract_dir = report
        .includes
        .first()
        .and_then(|inc| {
            set.manifest(&input.template).and_then(|m| {
                m.includes
                    .iter()
                    .find(|i| i.template == inc.template)
                    .map(|i| target.join(&i.into))
            })
        })
        .unwrap_or_else(|| target.to_path_buf());
    let deps = install_deps(&deps_lock(root)?, deps_cache, &contract_dir);
    Ok(TemplateRenderView {
        template: report.template,
        tier: report.tier,
        out_dir: target.display().to_string(),
        digest: report.digest,
        files: report.files,
        params: report.params,
        medusa_budget: report.medusa_budget,
        contract_dir: contract_dir.display().to_string(),
        deps,
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn effective_tier(app: &tauri::AppHandle) -> Result<TemplateTier, String> {
    Ok(renderer_tier(
        crate::tier::tier_recommend_sync(app.clone())?.effective,
    ))
}

fn app_templates_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let res = app.path().resource_dir().ok();
    templates_root(res.as_deref())
}

/// **Command — template_list.** The bundled templates with their parameter forms, the tier in
/// effect and its Medusa budget. Read-only.
#[tauri::command]
pub async fn template_list(app_h: tauri::AppHandle) -> Result<TemplateCatalog, String> {
    crate::blocking::off_main(move || {
        let root = app_templates_root(&app_h)?;
        let set = TemplateSet::open(&root).map_err(|e| e.to_string())?;
        Ok(catalog(&set, effective_tier(&app_h)?))
    })
    .await
}

/// **Command — template_render.** Render a template into a folder the member granted for
/// writing, at this machine's tier. Refuses before writing anything when the folder is not
/// granted, not empty, or a parameter fails validation.
#[tauri::command]
pub async fn template_render(
    app_h: tauri::AppHandle,
    input: TemplateRenderInput,
) -> Result<TemplateRenderView, String> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        let store = crate::agent_grants::GrantStore::for_app(&app_h)?;
        let target = require_grant(
            &store,
            Path::new(&input.out_dir),
            crate::agent_grants::Access::Write,
            now_secs(),
        )?;
        let root = app_templates_root(&app_h)?;
        let cache = app_h
            .path()
            .app_local_data_dir()
            .ok()
            .map(|d| d.join(DEPS_CACHE_DIR))
            .filter(|d| d.is_dir());
        render_into(
            &root,
            &input,
            effective_tier(&app_h)?,
            &target,
            cache.as_deref(),
        )
    })
    .await
}

#[cfg(test)]
#[path = "template_forge_tests.rs"]
mod tests;
