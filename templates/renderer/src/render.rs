//! Loading a template root and rendering one template into an empty directory.
//!
//! Layout of a template root (`citrate-core/templates/`):
//!
//! ```text
//! deps.lock.json          pinned Solidity dependencies (recorded, never fetched here)
//! medusa-budgets.json     per-tier Medusa budgets (HUP-S6.9)
//! _common/<layer>/...     shared files a template pulls in by naming the layer
//! <id>/template.json      the manifest
//! <id>/files/...          the template's own files
//! ```
//!
//! Placeholders are `{{ct:<key>}}`. Any other `{{` passes through untouched (JSX
//! uses `style={{ ... }}`). An unknown key or an unterminated placeholder is an
//! error. Everything is rendered in memory first; nothing is written unless the
//! whole render succeeds and the output directory is empty or absent.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::budget::{MedusaBudget, MedusaBudgets, Tier};
use crate::params;

/// The provenance file written at the root of every rendered template.
pub const LOCK_FILE: &str = "citrate-template.lock.json";
/// The dependency pin file inside a template root.
pub const DEPS_FILE: &str = "deps.lock.json";

const PLACEHOLDER_OPEN: &str = "{{ct:";
const PLACEHOLDER_CLOSE: &str = "}}";
/// Parameter keys a manifest may declare.
const PARAM_KEYS: &[&str] = &["name", "symbol", "supply", "price", "owner"];

/// Why a render (or opening a template root) failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// Filesystem failure outside the template sources.
    Io(String),
    /// `medusa-budgets.json` or `deps.lock.json` is missing or invalid.
    Data(String),
    /// A `template.json` is invalid.
    Manifest { template: String, reason: String },
    /// No template with this id.
    UnknownTemplate(String),
    /// A caller-supplied parameter was refused.
    Param { param: String, reason: String },
    /// A template source is unusable (symlink, non-UTF-8, bad placeholder).
    Template {
        template: String,
        file: String,
        reason: String,
    },
    /// The output directory exists and is not empty.
    OutputNotEmpty(PathBuf),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Io(e) => write!(f, "io: {e}"),
            RenderError::Data(e) => write!(f, "template data: {e}"),
            RenderError::Manifest { template, reason } => {
                write!(f, "template {template}: manifest: {reason}")
            }
            RenderError::UnknownTemplate(id) => write!(f, "unknown template {id:?}"),
            RenderError::Param { param, reason } => write!(f, "parameter {param}: {reason}"),
            RenderError::Template {
                template,
                file,
                reason,
            } => write!(f, "template {template}, {file}: {reason}"),
            RenderError::OutputNotEmpty(p) => {
                write!(f, "output directory {} is not empty", p.display())
            }
        }
    }
}

impl std::error::Error for RenderError {}

impl From<params::ParamError> for RenderError {
    fn from(e: params::ParamError) -> Self {
        RenderError::Param {
            param: e.param.to_string(),
            reason: e.reason,
        }
    }
}

/// What a template produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateKind {
    /// A Foundry project.
    Contract,
    /// A web app (possibly with an included contract project).
    Dapp,
}

/// One declared parameter. `min`/`max` apply to `supply` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamSpec {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub min: Option<u64>,
    #[serde(default)]
    pub max: Option<u64>,
}

/// Another template rendered into a subdirectory with the same parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Include {
    pub template: String,
    pub into: String,
}

/// `<id>/template.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateManifest {
    pub id: String,
    pub kind: TemplateKind,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub layers: Vec<String>,
    pub params: BTreeMap<String, ParamSpec>,
    #[serde(default)]
    pub includes: Vec<Include>,
    /// True when the template ships Medusa property tests and a `medusa.json`.
    #[serde(default)]
    pub medusa: bool,
}

/// The result of a render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderReport {
    pub template: String,
    pub tier: String,
    /// sha256 (hex) over the template's sources, independent of the parameters.
    pub digest: String,
    /// Files written, relative to the output directory.
    pub files: Vec<String>,
    /// Normalized parameters, plus the derived `contract` identifier.
    pub params: BTreeMap<String, String>,
    pub medusa_budget: Option<MedusaBudget>,
    pub includes: Vec<RenderReport>,
}

#[derive(Debug, Clone)]
struct SourceFile {
    rel: String,
    body: String,
}

#[derive(Debug, Clone)]
struct Loaded {
    manifest: TemplateManifest,
    manifest_raw: String,
    files: Vec<SourceFile>,
}

/// A validated template root.
#[derive(Debug, Clone)]
pub struct TemplateSet {
    budgets: MedusaBudgets,
    deps: serde_json::Value,
    templates: BTreeMap<String, Loaded>,
}

fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn tmpl_err(template: &str, file: &str, reason: impl Into<String>) -> RenderError {
    RenderError::Template {
        template: template.to_string(),
        file: file.to_string(),
        reason: reason.into(),
    }
}

fn manifest_err(template: &str, reason: impl Into<String>) -> RenderError {
    RenderError::Manifest {
        template: template.to_string(),
        reason: reason.into(),
    }
}

/// Collect every regular file under `dir`, sorted by relative path. Symlinks and
/// non-UTF-8 files are refused.
fn collect(
    template: &str,
    dir: &Path,
    base: &Path,
    out: &mut Vec<SourceFile>,
) -> Result<(), RenderError> {
    let rd = fs::read_dir(dir)
        .map_err(|e| tmpl_err(template, &dir.display().to_string(), e.to_string()))?;
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in rd {
        let entry =
            entry.map_err(|e| tmpl_err(template, &dir.display().to_string(), e.to_string()))?;
        entries.push(entry.path());
    }
    entries.sort();
    for path in entries {
        let rel_path = path
            .strip_prefix(base)
            .map_err(|e| tmpl_err(template, &path.display().to_string(), e.to_string()))?;
        let rel = rel_path.to_string_lossy().replace('\\', "/");
        if !rel_path
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(tmpl_err(
                template,
                &rel,
                "path is not a plain relative path",
            ));
        }
        let meta =
            fs::symlink_metadata(&path).map_err(|e| tmpl_err(template, &rel, e.to_string()))?;
        if meta.file_type().is_symlink() {
            return Err(tmpl_err(
                template,
                &rel,
                "symlinks are not allowed in templates",
            ));
        }
        if meta.is_dir() {
            collect(template, &path, base, out)?;
        } else if meta.is_file() {
            let bytes = fs::read(&path).map_err(|e| tmpl_err(template, &rel, e.to_string()))?;
            let body =
                String::from_utf8(bytes).map_err(|_| tmpl_err(template, &rel, "not UTF-8 text"))?;
            out.push(SourceFile { rel, body });
        } else {
            return Err(tmpl_err(template, &rel, "not a regular file"));
        }
    }
    Ok(())
}

/// Replace every `{{ct:key}}` in `body` from `values`.
fn substitute(
    template: &str,
    file: &str,
    body: &str,
    values: &BTreeMap<&'static str, String>,
) -> Result<String, RenderError> {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find(PLACEHOLDER_OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + PLACEHOLDER_OPEN.len()..];
        let Some(end) = after.find(PLACEHOLDER_CLOSE) else {
            return Err(tmpl_err(template, file, "unterminated {{ct: placeholder"));
        };
        let key = &after[..end];
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return Err(tmpl_err(
                template,
                file,
                format!("malformed placeholder key {key:?}"),
            ));
        }
        let Some(value) = values.get(key) else {
            return Err(tmpl_err(
                template,
                file,
                format!("unknown placeholder {key:?}"),
            ));
        };
        if !params::is_inert(value) {
            // Unreachable while every value comes from a validator; kept as a hard stop.
            return Err(tmpl_err(
                template,
                file,
                format!("value for {key:?} is not inert"),
            ));
        }
        out.push_str(value);
        rest = &after[end + PLACEHOLDER_CLOSE.len()..];
    }
    out.push_str(rest);
    Ok(out)
}

impl TemplateSet {
    /// Open and validate a template root: budgets, dependency pins, every manifest
    /// and every source file.
    pub fn open(root: &Path) -> Result<TemplateSet, RenderError> {
        let budgets = MedusaBudgets::load(root).map_err(RenderError::Data)?;
        let deps_path = root.join(DEPS_FILE);
        let deps_raw = fs::read_to_string(&deps_path)
            .map_err(|e| RenderError::Data(format!("read {}: {e}", deps_path.display())))?;
        let deps: serde_json::Value = serde_json::from_str(&deps_raw)
            .map_err(|e| RenderError::Data(format!("{DEPS_FILE}: {e}")))?;
        if !deps["deps"].is_object() {
            return Err(RenderError::Data(format!("{DEPS_FILE}: no deps object")));
        }

        let rd = fs::read_dir(root)
            .map_err(|e| RenderError::Io(format!("read {}: {e}", root.display())))?;
        let mut dirs: Vec<PathBuf> = Vec::new();
        for entry in rd {
            let entry = entry.map_err(|e| RenderError::Io(e.to_string()))?;
            let p = entry.path();
            if p.join("template.json").is_file() {
                dirs.push(p);
            }
        }
        dirs.sort();

        let mut templates = BTreeMap::new();
        for dir in dirs {
            let dir_name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let manifest_raw = fs::read_to_string(dir.join("template.json"))
                .map_err(|e| manifest_err(&dir_name, e.to_string()))?;
            let manifest: TemplateManifest = serde_json::from_str(&manifest_raw)
                .map_err(|e| manifest_err(&dir_name, e.to_string()))?;
            if manifest.id != dir_name || !valid_slug(&manifest.id) {
                return Err(manifest_err(
                    &dir_name,
                    "id must equal the directory name and be a lower-case slug",
                ));
            }
            for (key, spec) in &manifest.params {
                if !PARAM_KEYS.contains(&key.as_str()) {
                    return Err(manifest_err(
                        &dir_name,
                        format!("unknown parameter {key:?}"),
                    ));
                }
                if key == "supply" {
                    match (spec.min, spec.max) {
                        (Some(min), Some(max)) if min >= 1 && min <= max => {}
                        _ => {
                            return Err(manifest_err(
                                &dir_name,
                                "supply needs min >= 1 and max >= min",
                            ))
                        }
                    }
                } else if spec.min.is_some() || spec.max.is_some() {
                    return Err(manifest_err(
                        &dir_name,
                        format!("{key} does not take min/max"),
                    ));
                }
            }
            if manifest.params.contains_key("symbol") && !manifest.params.contains_key("name") {
                return Err(manifest_err(&dir_name, "symbol requires name"));
            }
            let mut files = Vec::new();
            for layer in &manifest.layers {
                if !valid_slug(layer) {
                    return Err(manifest_err(&dir_name, format!("bad layer name {layer:?}")));
                }
                let base = root.join("_common").join(layer);
                if !base.is_dir() {
                    return Err(manifest_err(&dir_name, format!("missing layer {layer:?}")));
                }
                collect(&dir_name, &base, &base, &mut files)?;
            }
            let own = dir.join("files");
            if own.is_dir() {
                collect(&dir_name, &own, &own, &mut files)?;
            }
            let mut seen = std::collections::BTreeSet::new();
            for f in &files {
                if f.rel == LOCK_FILE {
                    return Err(tmpl_err(&dir_name, &f.rel, "reserved file name"));
                }
                if !seen.insert(f.rel.clone()) {
                    return Err(tmpl_err(
                        &dir_name,
                        &f.rel,
                        "provided by more than one layer",
                    ));
                }
            }
            templates.insert(
                dir_name,
                Loaded {
                    manifest,
                    manifest_raw,
                    files,
                },
            );
        }

        // Includes: one level deep, parameters a subset of the parent's.
        for (id, t) in &templates {
            let mut intos = std::collections::BTreeSet::new();
            for inc in &t.manifest.includes {
                let Some(child) = templates.get(&inc.template) else {
                    return Err(manifest_err(
                        id,
                        format!("includes unknown template {:?}", inc.template),
                    ));
                };
                if !child.manifest.includes.is_empty() {
                    return Err(manifest_err(
                        id,
                        "included templates may not include others",
                    ));
                }
                if !valid_slug(&inc.into) || !intos.insert(inc.into.clone()) {
                    return Err(manifest_err(
                        id,
                        format!("bad or repeated include directory {:?}", inc.into),
                    ));
                }
                if t.files
                    .iter()
                    .any(|f| f.rel == inc.into || f.rel.starts_with(&format!("{}/", inc.into)))
                {
                    return Err(manifest_err(
                        id,
                        format!(
                            "include directory {:?} collides with template files",
                            inc.into
                        ),
                    ));
                }
                for key in child.manifest.params.keys() {
                    if !t.manifest.params.contains_key(key) {
                        return Err(manifest_err(
                            id,
                            format!(
                                "included {} needs {key}, which is not declared",
                                inc.template
                            ),
                        ));
                    }
                }
            }
        }

        Ok(TemplateSet {
            budgets,
            deps,
            templates,
        })
    }

    /// Template ids, sorted.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.templates.keys().map(String::as_str)
    }

    /// A template's manifest.
    pub fn manifest(&self, id: &str) -> Option<&TemplateManifest> {
        self.templates.get(id).map(|t| &t.manifest)
    }

    /// The loaded Medusa budgets.
    pub fn budgets(&self) -> &MedusaBudgets {
        &self.budgets
    }

    /// sha256 over a template's manifest, sources and included templates.
    pub fn digest(&self, id: &str) -> Result<String, RenderError> {
        let t = self
            .templates
            .get(id)
            .ok_or_else(|| RenderError::UnknownTemplate(id.to_string()))?;
        let mut h = Sha256::new();
        h.update(b"citrate-template-v1\0");
        h.update(t.manifest_raw.as_bytes());
        h.update([0u8]);
        for f in &t.files {
            h.update(f.rel.as_bytes());
            h.update([0u8]);
            h.update((f.body.len() as u64).to_be_bytes());
            h.update(f.body.as_bytes());
        }
        for inc in &t.manifest.includes {
            h.update(inc.template.as_bytes());
            h.update([0u8]);
            h.update(inc.into.as_bytes());
            h.update([0u8]);
            h.update(self.digest(&inc.template)?.as_bytes());
        }
        Ok(hex(&h.finalize()))
    }

    /// Validate `raw` against a manifest: unknown keys refused, defaults applied,
    /// every value normalized. Adds the derived `contract` identifier.
    fn resolve(
        &self,
        m: &TemplateManifest,
        raw: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, RenderError> {
        for key in raw.keys() {
            if !m.params.contains_key(key) {
                return Err(RenderError::Param {
                    param: key.clone(),
                    reason: format!("is not a parameter of template {}", m.id),
                });
            }
        }
        let mut out = BTreeMap::new();
        for (key, spec) in &m.params {
            let value = match (raw.get(key), &spec.default) {
                (Some(v), _) => v.clone(),
                (None, Some(d)) => d.clone(),
                (None, None) => {
                    return Err(RenderError::Param {
                        param: key.clone(),
                        reason: "is required".into(),
                    });
                }
            };
            let normalized = match key.as_str() {
                "name" => params::validate_name(&value)?,
                "symbol" => params::validate_symbol(&value)?,
                "supply" => {
                    let (min, max) = (spec.min.unwrap_or(1), spec.max.unwrap_or(1));
                    params::validate_supply(&value, min, max)?.to_string()
                }
                "price" => params::validate_price(&value)?,
                "owner" => params::validate_owner(&value)?,
                other => {
                    return Err(RenderError::Param {
                        param: other.to_string(),
                        reason: "is not a known parameter".into(),
                    });
                }
            };
            out.insert(key.clone(), normalized);
        }
        if let Some(name) = out.get("name") {
            let ident = params::contract_identifier(name)?;
            out.insert("contract".to_string(), ident);
        }
        Ok(out)
    }

    /// Render one template (and its includes) into memory.
    fn plan(
        &self,
        id: &str,
        raw: &BTreeMap<String, String>,
        tier: Tier,
        prefix: &str,
        files: &mut Vec<(String, String)>,
    ) -> Result<RenderReport, RenderError> {
        let t = self
            .templates
            .get(id)
            .ok_or_else(|| RenderError::UnknownTemplate(id.to_string()))?;
        let normalized = self.resolve(&t.manifest, raw)?;
        let budget = if t.manifest.medusa {
            Some(self.budgets.for_tier(tier).clone())
        } else {
            None
        };

        let mut values: BTreeMap<&'static str, String> = BTreeMap::new();
        for key in ["name", "symbol", "supply", "price", "owner", "contract"] {
            if let Some(v) = normalized.get(key) {
                values.insert(key, v.clone());
            }
        }
        if let Some(b) = &budget {
            values.insert("medusa_test_limit", b.test_limit.to_string());
            values.insert("medusa_workers", b.workers.to_string());
            values.insert(
                "medusa_call_sequence_length",
                b.call_sequence_length.to_string(),
            );
            values.insert("medusa_timeout_secs", b.timeout_secs.to_string());
        }

        let mut written = Vec::new();
        for f in &t.files {
            let body = substitute(id, &f.rel, &f.body, &values)?;
            let rel = format!("{prefix}{}", f.rel);
            written.push(rel.clone());
            files.push((rel, body));
        }

        let mut includes = Vec::new();
        for inc in &t.manifest.includes {
            let child = self
                .templates
                .get(&inc.template)
                .ok_or_else(|| RenderError::UnknownTemplate(inc.template.clone()))?;
            let child_raw: BTreeMap<String, String> = normalized
                .iter()
                .filter(|(k, _)| child.manifest.params.contains_key(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let child_prefix = format!("{prefix}{}/", inc.into);
            includes.push(self.plan(&inc.template, &child_raw, tier, &child_prefix, files)?);
        }

        let digest = self.digest(id)?;
        let lock = serde_json::json!({
            "schema": 1,
            "renderer": concat!("citrate-templates ", env!("CARGO_PKG_VERSION")),
            "template": id,
            "digest": digest,
            "tier": tier.id(),
            "params": normalized,
            "medusa_budget": budget,
            "deps": self.deps["deps"],
            "solc": self.deps["solc"],
            "includes": t.manifest.includes,
        });
        let lock_body =
            serde_json::to_string_pretty(&lock).map_err(|e| RenderError::Io(e.to_string()))?;
        let lock_rel = format!("{prefix}{LOCK_FILE}");
        written.push(lock_rel.clone());
        files.push((lock_rel, format!("{lock_body}\n")));

        Ok(RenderReport {
            template: id.to_string(),
            tier: tier.id().to_string(),
            digest,
            files: written,
            params: normalized,
            medusa_budget: budget,
            includes,
        })
    }

    /// Render template `id` with parameters `raw` for `tier` into `out`, which must
    /// be absent or an empty directory. On any error nothing is left behind: a
    /// refusal happens before the first write, and if a write fails partway every
    /// file and directory this call created is removed again.
    pub fn render(
        &self,
        id: &str,
        raw: &BTreeMap<String, String>,
        tier: Tier,
        out: &Path,
    ) -> Result<RenderReport, RenderError> {
        let mut files = Vec::new();
        let report = self.plan(id, raw, tier, "", &mut files)?;

        if out.exists() {
            let meta = fs::symlink_metadata(out).map_err(|e| RenderError::Io(e.to_string()))?;
            if !meta.is_dir() {
                return Err(RenderError::OutputNotEmpty(out.to_path_buf()));
            }
            let mut rd = fs::read_dir(out).map_err(|e| RenderError::Io(e.to_string()))?;
            if rd.next().is_some() {
                return Err(RenderError::OutputNotEmpty(out.to_path_buf()));
            }
        }

        let mut made = Created::default();
        match write_tree(out, &files, &mut made) {
            Ok(()) => Ok(report),
            Err(e) => {
                made.undo();
                Err(e)
            }
        }
    }
}

/// Files and directories one render created, in creation order.
#[derive(Default)]
struct Created {
    dirs: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Created {
    /// Remove everything recorded, newest first. Directories are removed only
    /// when empty, so nothing this render did not create is ever deleted.
    fn undo(&self) {
        for f in self.files.iter().rev() {
            let _ = fs::remove_file(f);
        }
        for d in self.dirs.iter().rev() {
            let _ = fs::remove_dir(d);
        }
    }
}

/// Create `dir` and any missing ancestors one level at a time, recording each.
fn make_dirs(dir: &Path, made: &mut Created) -> Result<(), RenderError> {
    let mut missing = Vec::new();
    let mut cur = Some(dir);
    while let Some(p) = cur {
        if p.as_os_str().is_empty() || p.is_dir() {
            break;
        }
        missing.push(p.to_path_buf());
        cur = p.parent();
    }
    for p in missing.into_iter().rev() {
        fs::create_dir(&p).map_err(|e| RenderError::Io(format!("create {}: {e}", p.display())))?;
        made.dirs.push(p);
    }
    Ok(())
}

/// Write every planned file under `out` with `create_new`, recording what it made.
fn write_tree(
    out: &Path,
    files: &[(String, String)],
    made: &mut Created,
) -> Result<(), RenderError> {
    make_dirs(out, made)?;
    for (rel, body) in files {
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            make_dirs(parent, made)?;
        }
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| RenderError::Io(format!("create {}: {e}", path.display())))?;
        made.files.push(path.clone());
        f.write_all(body.as_bytes())
            .map_err(|e| RenderError::Io(format!("write {}: {e}", path.display())))?;
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(HEX[usize::from(b >> 4)]));
        s.push(char::from(HEX[usize::from(b & 0x0f)]));
    }
    s
}
