//! citrate-core — local instruction-skills (Hermes "write & run skills").
//!
//! The Hermes sidecar's built-in skills are compiled WASM capsules — the agent cannot author those.
//! This module gives Hermes the "Agent Skills" model instead: a skill is a markdown PLAYBOOK the agent
//! writes (a name, a description, and step-by-step instructions), stored on the member's own device.
//! Running a skill loads its instructions back into the agent's tool loop so it carries the task out
//! with its EXISTING tools — every chain/shell/signature effect still stops at the Signature Ceremony
//! (Rule 3). Nothing here signs, executes arbitrary code, or leaves the device.
//!
//! HUP-S3.2 (US-3.2 AC2, one format and one loader): a skill is an agentskills.io `SKILL.md` at
//! `<app_local_data>/agent-skills/<slug>/SKILL.md`, the format the sidecar's strict loader reads
//! (citrate-agent-runtime `agent-loop/src/skills.rs`). The frontmatter carries `name` (the slug),
//! `description`, and `metadata` with the member's display name; the body is the instructions.
//! The folder is one of the sidecar's skill sources, so a saved skill is offered in sessions like
//! every other skill. Skills saved in the older flat format (`agent-skills/<slug>.md`) are
//! converted once; the old file is kept as `<slug>.md.migrated`, and one that cannot be converted
//! is reported and left in place. Purely local files; fully reversible.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

const DIR: &str = "agent-skills";
/// The display name a member may give a skill.
const MAX_NAME: usize = 80;
const MAX_DESC: usize = 400;
/// The sidecar loader refuses a `SKILL.md` over 64 KiB; the frontmatter needs at most about 1.5
/// KiB (the name, description and display name are bounded above), so the body may use the rest.
const MAX_BODY: usize = 62 * 1024;
/// agentskills.io: a skill `name` is 1-64 chars of `[a-z0-9-]`.
const MAX_SLUG: usize = 64;
/// The suffix a converted legacy file is renamed to.
const MIGRATED_SUFFIX: &str = ".migrated";
/// Marks a `SKILL.md` as written here (the sidecar records metadata and never acts on it).
const ORIGIN: &str = "citrate-core-skill-write";

/// One authored local skill (metadata + optional body when read directly).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSkill {
    /// The human name as authored (e.g. "Weekly staking report").
    pub name: String,
    /// A one-line description of what the skill does.
    pub description: String,
    /// The filename slug (stable id used by skill_run; also the SKILL.md `name`).
    pub slug: String,
}

/// What converting the older flat-file skills did.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationReport {
    /// Slugs converted to `SKILL.md`.
    pub converted: Vec<String>,
    /// Legacy files left in place, each with the reason.
    pub failed: Vec<MigrationFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationFailure {
    pub file: String,
    pub reason: String,
}

/// Slugify a name into a safe, stable skill name: lowercase alnum, single dashes for runs of other
/// chars, at most 64 chars, never a leading or trailing dash (the agentskills.io `name` rule).
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let cut: String = out.chars().take(MAX_SLUG).collect();
    let s = cut.trim_matches('-').to_string();
    if s.is_empty() {
        "skill".to_string()
    } else {
        s
    }
}

/// One line of text: newlines, tabs and other control characters become single spaces.
fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A YAML double-quoted scalar in the subset the loader accepts (`\"` and `\\` escapes only).
fn yaml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in one_line(s).chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Undo [`yaml_quote`] (and accept a plain scalar).
fn yaml_unquote(raw: &str) -> String {
    let t = raw.trim();
    let Some(inner) = t.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
        return t.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The description the loader requires (it refuses an empty one): the member's, or else the first
/// line of the instructions, said to be that.
fn description_for(description: &str, instructions: &str) -> String {
    let d = one_line(description);
    if !d.is_empty() {
        return d;
    }
    let first = instructions
        .lines()
        .map(one_line)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let first: String = first.chars().take(200).collect();
    if first.is_empty() {
        "A skill the member saved.".to_string()
    } else {
        format!("A skill the member saved: {first}")
    }
}

/// The `SKILL.md` text for a member skill, in the strict loader format.
fn skill_md(slug: &str, display: &str, description: &str, instructions: &str) -> String {
    format!(
        "---\nname: {slug}\ndescription: {}\nmetadata:\n  display-name: {}\n  origin: {ORIGIN}\n---\n\n{}\n",
        yaml_quote(description),
        yaml_quote(display),
        instructions.trim()
    )
}

/// Read back a `SKILL.md` this module wrote: (display name, description, body). The sidecar's
/// loader is the authority on the format; this reads only the three fields core shows and runs.
fn read_skill_md(slug: &str, text: &str) -> Option<(String, String, String)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    let fm = &rest[..end];
    let body = rest[end + 5..].trim_start_matches('\n').to_string();
    let mut display = String::new();
    let mut description = String::new();
    for line in fm.lines() {
        if let Some(v) = line.strip_prefix("description:") {
            description = yaml_unquote(v);
        } else if let Some(v) = line.strip_prefix("  display-name:") {
            display = yaml_unquote(v);
        }
    }
    if display.is_empty() {
        display = slug.to_string();
    }
    Some((display, description, body))
}

fn skills_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join(DIR);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// The folder member-authored skills live in (a sidecar skill source, see `hermes.rs`).
pub fn authored_skills_dir(app_local_data: &Path) -> PathBuf {
    app_local_data.join(DIR)
}

/// Parse `name`/`description` from a LEGACY flat skill file's frontmatter; fall back to the slug.
fn parse_legacy_meta(path: &Path, body: &str) -> LocalSkill {
    let slug = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("skill")
        .to_string();
    let mut name = slug.clone();
    let mut description = String::new();
    if let Some(rest) = body.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    description = v.trim().to_string();
                }
            }
        }
    }
    LocalSkill {
        name,
        description,
        slug,
    }
}

/// Strip a LEGACY frontmatter header, returning just the instruction body.
fn strip_legacy_frontmatter(body: &str) -> String {
    if let Some(rest) = body.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            return rest[end + 4..].trim_start_matches('\n').to_string();
        }
    }
    body.to_string()
}

/// Convert every legacy `agent-skills/<slug>.md` in `dir` to `agent-skills/<slug>/SKILL.md`.
/// Idempotent: a converted file is renamed `<slug>.md.migrated` and never read again. A file that
/// cannot be converted (a `SKILL.md` skill of that name already exists, or it is too large or
/// unreadable) stays where it is and is reported.
fn migrate_legacy(dir: &Path) -> MigrationReport {
    let mut report = MigrationReport::default();
    let Ok(entries) = fs::read_dir(dir) else {
        return report;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    files.sort();
    for path in files {
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fail = |reason: String| MigrationFailure {
            file: file.clone(),
            reason,
        };
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                report
                    .failed
                    .push(fail(format!("unreadable: {}", e.kind())));
                continue;
            }
        };
        let meta = parse_legacy_meta(&path, &text);
        let body = strip_legacy_frontmatter(&text);
        let display = one_line(&meta.name);
        let display: String = display.chars().take(MAX_NAME).collect();
        let description: String = description_for(&meta.description, &body)
            .chars()
            .take(MAX_DESC)
            .collect();
        if body.len() > MAX_BODY {
            report.failed.push(fail(format!(
                "its instructions are over {MAX_BODY} bytes, the most a SKILL.md can hold"
            )));
            continue;
        }
        let slug = slugify(&display);
        match create_skill_md(dir, &slug, &display, &description, &body) {
            Ok(()) => {
                let mut kept = path.clone().into_os_string();
                kept.push(MIGRATED_SUFFIX);
                if let Err(e) = fs::rename(&path, PathBuf::from(kept)) {
                    report.failed.push(fail(format!(
                        "converted to {slug}/SKILL.md, but the old file could not be renamed: {}",
                        e.kind()
                    )));
                    continue;
                }
                report.converted.push(slug);
            }
            Err(e) => report.failed.push(fail(e)),
        }
    }
    report
}

/// Create `dir/<slug>/SKILL.md` only if no skill of that slug exists (atomic `create_new`).
fn create_skill_md(
    dir: &Path,
    slug: &str,
    display: &str,
    description: &str,
    instructions: &str,
) -> Result<(), String> {
    use std::io::Write;
    let skill_dir = dir.join(slug);
    fs::create_dir_all(&skill_dir).map_err(|e| e.to_string())?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(skill_dir.join("SKILL.md"))
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!(
                    "{SKILL_EXISTS_PREFIX}: a skill named \"{display}\" already exists; replacing it needs the member's approval"
                )
            } else {
                e.to_string()
            }
        })?;
    f.write_all(skill_md(slug, display, description, instructions).as_bytes())
        .map_err(|e| e.to_string())
}

/// Every `SKILL.md` skill in `dir`, sorted by display name. Unreadable entries are skipped.
fn list_skills(dir: &Path) -> Vec<LocalSkill> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(slug) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(text) = fs::read_to_string(path.join("SKILL.md")) else {
            continue;
        };
        if let Some((name, description, _)) = read_skill_md(&slug, &text) {
            out.push(LocalSkill {
                name,
                description,
                slug,
            });
        }
    }
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// **skills_local_list** — the skills the agent has authored on THIS device. Honest-empty, never
/// fabricated (Rule 1); an unreadable entry is skipped rather than failing the whole list. Older
/// flat-file skills are converted first.
#[tauri::command]
pub async fn skills_local_list(app_h: tauri::AppHandle) -> Result<Vec<LocalSkill>, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || skills_local_list_sync(app_h.clone())).await
}

/// Blocking body of [`skills_local_list`]; reached only through [`crate::blocking::off_main`].
pub fn skills_local_list_sync(app: tauri::AppHandle) -> Result<Vec<LocalSkill>, String> {
    let dir = skills_dir(&app)?;
    let _ = migrate_legacy(&dir);
    Ok(list_skills(&dir))
}

/// **skills_local_migrate**, HUP-S3.2: convert older flat-file skills to `SKILL.md` and say what
/// happened (the webview reports any that could not be converted).
#[tauri::command]
pub async fn skills_local_migrate(app_h: tauri::AppHandle) -> Result<MigrationReport, String> {
    crate::blocking::off_main(move || {
        let dir = skills_dir(&app_h)?;
        Ok(migrate_legacy(&dir))
    })
    .await
}

/// PBA-L7b-002: the error prefix a create-only write returns when the skill already exists. The
/// webview keys the member-approval prompt off this prefix.
pub const SKILL_EXISTS_PREFIX: &str = "SKILL_EXISTS";

/// Write a skill into `dir`. With `overwrite == false` an existing skill is NEVER replaced
/// (atomic `create_new`): a saved skill is a persistent instruction the agent later runs, so a
/// silent overwrite is a persistence vector for an injected instruction (PBA-L7b-002). Replacing
/// one is an explicit, member-approved `overwrite == true` call.
fn write_skill_file(
    dir: &Path,
    name: &str,
    description: &str,
    instructions: &str,
    overwrite: bool,
) -> Result<LocalSkill, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a skill needs a name".into());
    }
    if name.len() > MAX_NAME || description.len() > MAX_DESC || instructions.len() > MAX_BODY {
        return Err("skill name/description/instructions exceed the allowed size".into());
    }
    let slug = slugify(name);
    let display = one_line(name);
    let desc = description_for(description, instructions);
    if overwrite {
        let skill_dir = dir.join(&slug);
        fs::create_dir_all(&skill_dir).map_err(|e| e.to_string())?;
        fs::write(
            skill_dir.join("SKILL.md"),
            skill_md(&slug, &display, &desc, instructions),
        )
        .map_err(|e| e.to_string())?;
    } else {
        create_skill_md(dir, &slug, &display, &desc, instructions)?;
    }
    Ok(LocalSkill {
        name: display,
        description: desc,
        slug,
    })
}

/// HUP-S3.2: ask a running sidecar to reload its skills so a saved or removed skill applies to the
/// next session. Best-effort: with the sidecar off there is nothing to reload, and it reads the
/// folder at its next start.
fn reload_sidecar_skills(app: &tauri::AppHandle) {
    if let Ok(m) = crate::hermes::manager(app) {
        if m.is_running() {
            let _ = m.control_post("/instruction-skills/reload", "{}");
        }
    }
}

/// **skills_local_write** — author a local instruction-skill. Local file only; signs nothing and
/// runs nothing. Returns the stored metadata. `overwrite` defaults to `false`: an existing skill is
/// refused with a `SKILL_EXISTS` error unless the caller (after the member approved) passes `true`.
#[tauri::command]
pub async fn skills_local_write(
    app_h: tauri::AppHandle,
    name: String,
    description: String,
    instructions: String,
    overwrite: Option<bool>,
) -> Result<LocalSkill, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        skills_local_write_sync(app_h.clone(), name, description, instructions, overwrite)
    })
    .await
}

/// Blocking body of [`skills_local_write`]; reached only through [`crate::blocking::off_main`].
pub fn skills_local_write_sync(
    app: tauri::AppHandle,
    name: String,
    description: String,
    instructions: String,
    overwrite: Option<bool>,
) -> Result<LocalSkill, String> {
    let dir = skills_dir(&app)?;
    let out = write_skill_file(
        &dir,
        &name,
        &description,
        &instructions,
        overwrite.unwrap_or(false),
    )?;
    reload_sidecar_skills(&app);
    Ok(out)
}

/// The instruction body of the skill named (or slugged) `name` in `dir`.
fn read_body(dir: &Path, name: &str) -> Result<String, String> {
    let slug = slugify(name);
    let text = fs::read_to_string(dir.join(&slug).join("SKILL.md"))
        .map_err(|_| format!("no local skill named \"{name}\""))?;
    read_skill_md(&slug, &text)
        .map(|(_, _, body)| body)
        .ok_or_else(|| format!("the skill \"{name}\" has no SKILL.md frontmatter"))
}

/// **skills_local_read** — the instruction body of an authored skill, by slug or name. Used by
/// skill_run to load the playbook into the agent's loop.
#[tauri::command]
pub async fn skills_local_read(app_h: tauri::AppHandle, name: String) -> Result<String, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || skills_local_read_sync(app_h.clone(), name)).await
}

/// Blocking body of [`skills_local_read`]; reached only through [`crate::blocking::off_main`].
pub fn skills_local_read_sync(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let dir = skills_dir(&app)?;
    let _ = migrate_legacy(&dir);
    read_body(&dir, &name)
}

/// Remove the skill named (or slugged) `name` from `dir`. Idempotent (missing = ok). Only the
/// skill's own `SKILL.md` folder is removed.
fn delete_skill(dir: &Path, name: &str) -> Result<(), String> {
    let slug = slugify(name);
    let skill_dir = dir.join(&slug);
    if skill_dir.join("SKILL.md").is_file() {
        fs::remove_dir_all(&skill_dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// **skills_local_delete** — remove an authored skill. Idempotent (missing = ok).
#[tauri::command]
pub async fn skills_local_delete(app_h: tauri::AppHandle, name: String) -> Result<(), String> {
    // HUP-S0.1: the blocking body (file removal and the sidecar reload call) runs off the main
    // thread.
    crate::blocking::off_main(move || {
        let dir = skills_dir(&app_h)?;
        delete_skill(&dir, &name)?;
        reload_sidecar_skills(&app_h);
        Ok(())
    })
    .await
}

#[cfg(test)]
#[path = "skills_local_tests.rs"]
mod tests;
