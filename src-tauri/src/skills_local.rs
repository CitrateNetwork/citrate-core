//! citrate-core — local instruction-skills (Hermes "write & run skills").
//!
//! The Hermes sidecar's built-in skills are compiled WASM capsules — the agent cannot author those.
//! This module gives Hermes the "Agent Skills" model instead: a skill is a markdown PLAYBOOK the agent
//! writes (a name, a description, and step-by-step instructions), stored on the member's own device.
//! Running a skill loads its instructions back into the agent's tool loop so it carries the task out
//! with its EXISTING tools — every chain/shell/signature effect still stops at the Signature Ceremony
//! (Rule 3). Nothing here signs, executes arbitrary code, or leaves the device.
//!
//! Storage: `<app_local_data>/agent-skills/<slug>.md`, each with a small YAML frontmatter header
//! (`name`, `description`) followed by the instruction body. Purely local files; fully reversible.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

const DIR: &str = "agent-skills";
const MAX_NAME: usize = 80;
const MAX_DESC: usize = 400;
const MAX_BODY: usize = 64 * 1024; // 64 KiB of instructions is plenty; bound the write.

/// One authored local skill (metadata + optional body when read directly).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSkill {
    /// The human name as authored (e.g. "Weekly staking report").
    pub name: String,
    /// A one-line description of what the skill does.
    pub description: String,
    /// The filename slug (stable id used by skill_run).
    pub slug: String,
}

/// Slugify a name into a safe, stable filename stem: lowercase alnum, dashes for runs of other chars.
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
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "skill".to_string()
    } else {
        s.chars().take(MAX_NAME).collect()
    }
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

/// Parse `name`/`description` from a skill file's frontmatter; fall back to the slug if absent.
fn parse_meta(path: &Path, body: &str) -> LocalSkill {
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

/// Strip the frontmatter header, returning just the instruction body.
fn strip_frontmatter(body: &str) -> String {
    if let Some(rest) = body.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            return rest[end + 4..].trim_start_matches('\n').to_string();
        }
    }
    body.to_string()
}

/// **skills_local_list** — the skills the agent has authored on THIS device. Honest-empty, never
/// fabricated (Rule 1); an unreadable entry is skipped rather than failing the whole list.
#[tauri::command]
pub fn skills_local_list(app: tauri::AppHandle) -> Result<Vec<LocalSkill>, String> {
    let dir = skills_dir(&app)?;
    let mut out = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Ok(body) = fs::read_to_string(&path) {
            out.push(parse_meta(&path, &body));
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// **skills_local_write** — author (or overwrite) a local instruction-skill. Local file only; signs
/// nothing and runs nothing. Returns the stored metadata.
#[tauri::command]
pub fn skills_local_write(
    app: tauri::AppHandle,
    name: String,
    description: String,
    instructions: String,
) -> Result<LocalSkill, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a skill needs a name".into());
    }
    if name.len() > MAX_NAME || description.len() > MAX_DESC || instructions.len() > MAX_BODY {
        return Err("skill name/description/instructions exceed the allowed size".into());
    }
    let slug = slugify(name);
    let dir = skills_dir(&app)?;
    let path = dir.join(format!("{slug}.md"));
    // Escape any frontmatter-breaking newlines out of the single-line meta fields.
    let safe_name = name.replace('\n', " ");
    let safe_desc = description.trim().replace('\n', " ");
    let content = format!(
        "---\nname: {safe_name}\ndescription: {safe_desc}\n---\n\n{}\n",
        instructions.trim()
    );
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(LocalSkill {
        name: safe_name,
        description: safe_desc,
        slug,
    })
}

/// **skills_local_read** — the instruction body of an authored skill, by slug or name. Used by
/// skill_run to load the playbook into the agent's loop.
#[tauri::command]
pub fn skills_local_read(app: tauri::AppHandle, name: String) -> Result<String, String> {
    let slug = slugify(&name);
    let dir = skills_dir(&app)?;
    let path = dir.join(format!("{slug}.md"));
    let body = fs::read_to_string(&path).map_err(|_| format!("no local skill named \"{name}\""))?;
    Ok(strip_frontmatter(&body))
}

/// **skills_local_delete** — remove an authored skill. Idempotent (missing = ok).
#[tauri::command]
pub fn skills_local_delete(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let slug = slugify(&name);
    let dir = skills_dir(&app)?;
    let path = dir.join(format!("{slug}.md"));
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_is_safe_and_stable() {
        assert_eq!(slugify("Weekly Staking Report!"), "weekly-staking-report");
        assert_eq!(slugify("  ../etc/passwd  "), "etc-passwd");
        assert_eq!(slugify(""), "skill");
        assert_eq!(slugify("***"), "skill");
    }

    #[test]
    fn frontmatter_roundtrips() {
        let body = "---\nname: My Skill\ndescription: does a thing\n---\n\nStep 1. do it\n";
        let meta = parse_meta(Path::new("/x/my-skill.md"), body);
        assert_eq!(meta.name, "My Skill");
        assert_eq!(meta.description, "does a thing");
        assert_eq!(meta.slug, "my-skill");
        assert_eq!(strip_frontmatter(body), "Step 1. do it\n");
    }

    #[test]
    fn strip_without_frontmatter_is_identity() {
        assert_eq!(strip_frontmatter("just text"), "just text");
    }
}
