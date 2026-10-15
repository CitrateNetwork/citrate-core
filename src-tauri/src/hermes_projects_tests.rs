// HGW-W3 (core half): the projects folder, the workspace grant check, known projects and the
// `projectRoots` a conversation carries.

use super::*;
use crate::agent_grants::Grant;

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "hprojects-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("tmp");
        Tmp(canonical(&p))
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NOW: u64 = 1_800_000_000;

fn grant(id: &str, root: &Path, access: Access) -> Grant {
    Grant {
        id: id.into(),
        kind: GrantKind::Folder,
        root: root.display().to_string(),
        access,
        scope: "subtree".into(),
        granted_at: NOW - 10,
        expires_at: None,
        granted_by: "member".into(),
        reason: "test".into(),
        revoked_at: None,
    }
}

fn doc(grants: Vec<Grant>) -> GrantState {
    GrantState {
        version: 1,
        next_id: 9,
        grants,
    }
}

fn rw(root: &Path) -> GrantState {
    doc(vec![
        grant("g-1", root, Access::Read),
        grant("g-2", root, Access::Write),
    ])
}

/// A project as the runtime's `project_create` leaves it: the folder and its marker.
fn project(root: &Path, name: &str, created_by: &str) -> PathBuf {
    let p = root.join(name);
    std::fs::create_dir_all(p.join(".citrate")).expect("mkdir");
    std::fs::write(
        p.join(MARKER),
        serde_json::json!({"version": 1, "name": name, "root": p, "createdBy": created_by, "createdAt": 1})
            .to_string(),
    )
    .expect("marker");
    p
}

#[test]
fn the_projects_folder_is_under_home() {
    assert_eq!(
        projects_root(Path::new("/Users/m")),
        PathBuf::from("/Users/m/Citrate/Projects")
    );
}

#[test]
fn the_sidecar_gets_the_switch_and_the_folder_and_a_renderer_only_when_bundled() {
    let t = Tmp::new("env");
    let root = t.0.join("Projects");
    let env = sidecar_env(&root, None);
    assert!(env.contains(&(PROJECTS_ENV.to_string(), "1".to_string())));
    assert!(env.contains(&(PROJECTS_ROOT_ENV.to_string(), root.display().to_string())));
    assert!(!env.iter().any(|(k, _)| k == TEMPLATE_RENDERER_ENV));
    // A renderer that is not on disk is never passed.
    let missing = Renderer {
        bin: t.0.join("citrate-templates"),
        templates: t.0.join("templates"),
        tier: "T1".into(),
    };
    assert!(!sidecar_env(&root, Some(&missing))
        .iter()
        .any(|(k, _)| k == TEMPLATE_RENDERER_ENV));
    std::fs::write(&missing.bin, b"#!/bin/sh\n").expect("bin");
    std::fs::create_dir_all(&missing.templates).expect("templates");
    let env = sidecar_env(&root, Some(&missing));
    assert!(env.contains(&(TEMPLATE_TIER_ENV.to_string(), "T1".to_string())));
    assert!(env.iter().any(|(k, v)| k == TEMPLATES_ROOT_ENV && v.ends_with("templates")));
    for (k, _) in &env {
        assert!(SIDECAR_ENV_KEYS.contains(&k.as_str()), "{k}");
    }
}

#[test]
fn the_workspace_needs_live_read_and_write_subtree_grants() {
    let t = Tmp::new("granted");
    let root = t.0.join("Citrate/Projects");
    std::fs::create_dir_all(&root).expect("root");
    assert!(workspace_granted(&rw(&root), &root, NOW));
    // A grant on a parent folder covers it too.
    assert!(workspace_granted(&rw(&t.0), &root, NOW));
    // Read alone, write alone, revoked, expired, shallow or not yet active: not granted.
    assert!(!workspace_granted(&doc(vec![grant("g-1", &root, Access::Read)]), &root, NOW));
    assert!(!workspace_granted(&doc(vec![grant("g-2", &root, Access::Write)]), &root, NOW));
    let mut revoked = rw(&root);
    revoked.grants[1].revoked_at = Some(NOW - 1);
    assert!(!workspace_granted(&revoked, &root, NOW));
    let mut expired = rw(&root);
    expired.grants[0].expires_at = Some(NOW);
    assert!(!workspace_granted(&expired, &root, NOW));
    let mut shallow = rw(&root);
    shallow.grants[0].scope = "shallow".into();
    assert!(!workspace_granted(&shallow, &root, NOW));
    let mut later = rw(&root);
    later.grants[0].granted_at = NOW + 1;
    assert!(!workspace_granted(&later, &root, NOW));
    // A grant on a sibling never covers it.
    assert!(!workspace_granted(&rw(&t.0.join("Citrate/Other")), &root, NOW));
}

#[test]
fn the_view_reports_a_missing_folder_as_not_granted() {
    let t = Tmp::new("view");
    let root = t.0.join("Citrate/Projects");
    let v = workspace_view(&rw(&root), &root, NOW);
    assert!(!v.exists && !v.granted);
    ensure_root(&root).expect("create");
    let v = workspace_view(&rw(&root), &root, NOW);
    assert!(v.exists && v.granted);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&root).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[test]
fn only_a_hermes_project_directly_in_the_folder_is_remembered() {
    let t = Tmp::new("record");
    let hermes = t.0.join("hermes");
    let root = t.0.join("Projects");
    std::fs::create_dir_all(&root).expect("root");
    let good = project(&root, "demo", "hermes");
    let known = record_project(&hermes, &root, &good).expect("record");
    assert_eq!(known, vec![good.display().to_string()]);
    // Twice is once.
    assert_eq!(record_project(&hermes, &root, &good).expect("again").len(), 1);
    // Not created by Hermes, no marker, nested, outside, relative, a file, a symlink: refused.
    let other = project(&root, "theirs", "member");
    assert!(record_project(&hermes, &root, &other).is_err());
    std::fs::create_dir_all(root.join("bare")).expect("bare");
    assert!(record_project(&hermes, &root, &root.join("bare")).is_err());
    let nested = project(&root.join("demo"), "inner", "hermes");
    assert!(record_project(&hermes, &root, &nested).is_err());
    let outside = project(&t.0, "elsewhere", "hermes");
    assert!(record_project(&hermes, &root, &outside).is_err());
    assert!(record_project(&hermes, &root, Path::new("Projects/demo")).is_err());
    std::fs::write(root.join("file"), b"x").expect("file");
    assert!(record_project(&hermes, &root, &root.join("file")).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");
        assert!(record_project(&hermes, &root, &root.join("link")).is_err());
    }
    // A marker whose name does not match the folder is refused.
    let renamed = project(&root, "a", "hermes");
    std::fs::rename(&renamed, root.join("b")).expect("rename");
    assert!(record_project(&hermes, &root, &root.join("b")).is_err());
    assert_eq!(load_known(&hermes), vec![good.display().to_string()]);
}

#[test]
fn project_roots_are_sent_only_while_the_workspace_is_granted_and_the_folder_exists() {
    let t = Tmp::new("roots");
    let hermes = t.0.join("hermes");
    let root = t.0.join("Projects");
    std::fs::create_dir_all(&root).expect("root");
    let a = project(&root, "a", "hermes");
    let b = project(&root, "b", "hermes");
    record_project(&hermes, &root, &a).expect("a");
    record_project(&hermes, &root, &b).expect("b");
    assert_eq!(project_roots(&hermes, &root, &rw(&root), NOW).len(), 2);
    assert!(project_roots(&hermes, &root, &doc(vec![]), NOW).is_empty());
    std::fs::remove_dir_all(&b).expect("rm");
    assert_eq!(
        project_roots(&hermes, &root, &rw(&root), NOW),
        vec![a.display().to_string()]
    );
}

#[test]
fn the_session_body_carries_project_roots_from_its_own_grant_document() {
    let t = Tmp::new("body");
    let hermes = t.0.join("hermes");
    let root = t.0.join("Projects");
    std::fs::create_dir_all(&root).expect("root");
    let a = project(&root, "a", "hermes");
    record_project(&hermes, &root, &a).expect("a");
    let body = serde_json::json!({"model": "m", "grants": rw(&root)}).to_string();
    let out: serde_json::Value =
        serde_json::from_str(&with_project_roots(&body, &hermes, &root, NOW).expect("body"))
            .expect("json");
    assert_eq!(out["projectRoots"], serde_json::json!([a.display().to_string()]));
    // No grants in the body (or no live workspace grant): the body is unchanged.
    let bare = serde_json::json!({"model": "m"}).to_string();
    assert_eq!(with_project_roots(&bare, &hermes, &root, NOW).expect("bare"), bare);
    let ungranted = serde_json::json!({"model": "m", "grants": doc(vec![])}).to_string();
    assert_eq!(
        with_project_roots(&ungranted, &hermes, &root, NOW).expect("ungranted"),
        ungranted
    );
    assert!(with_project_roots("not json", &hermes, &root, NOW).is_err());
}

#[test]
fn the_new_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    for c in [
        "hermes_projects_workspace",
        "hermes_projects_workspace_grant",
        "hermes_projects_record",
    ] {
        assert!(acl.contains(&format!("\"{c}\"")), "{c}");
    }
}

// ---------------------------------------------------------------------------
// HGW-W3 add-on: folders the member marked as projects, the Projects list, and Forget.
// ---------------------------------------------------------------------------

#[test]
fn a_member_marks_only_a_folder_granted_for_reading_and_writing() {
    let t = Tmp::new("mark");
    let hermes = t.0.join("hermes");
    let site = t.0.join("work/site");
    std::fs::create_dir_all(&site).expect("site");
    let work = t.0.join("work");
    let read_only = doc(vec![grant("g-1", &work, Access::Read)]);
    assert!(mark_project(&hermes, &read_only, &site, NOW).is_err(), "read alone is not enough");
    assert!(mark_project(&hermes, &doc(vec![]), &site, NOW).is_err(), "not granted");
    assert!(mark_project(&hermes, &rw(&work), Path::new("relative/site"), NOW).is_err());
    assert!(mark_project(&hermes, &rw(&work), &work.join("missing"), NOW).is_err());
    mark_project(&hermes, &rw(&work), &site, NOW).expect("mark");
    mark_project(&hermes, &rw(&work), &site, NOW).expect("again");
    let list = projects_list(&hermes, &t.0.join("Projects"), &rw(&work), NOW);
    assert_eq!(list.projects.len(), 1, "one row per folder");
    let row = &list.projects[0];
    assert_eq!(row.path, site.display().to_string());
    assert_eq!(row.name, "site");
    assert_eq!(row.origin, "member");
    assert!(row.exists && row.shared);
}

#[test]
fn marked_folders_travel_as_project_roots_while_granted_without_the_workspace() {
    let t = Tmp::new("markroots");
    let hermes = t.0.join("hermes");
    let root = t.0.join("Projects");
    let work = t.0.join("work");
    let site = work.join("site");
    std::fs::create_dir_all(&site).expect("site");
    mark_project(&hermes, &rw(&work), &site, NOW).expect("mark");
    // The projects folder does not exist and is not granted; the marked folder still goes.
    assert_eq!(
        project_roots(&hermes, &root, &rw(&work), NOW),
        vec![site.display().to_string()]
    );
    // Its grant revoked: it stays on the list, but is no longer sent (the runtime would refuse
    // the whole session for it).
    let mut revoked = rw(&work);
    revoked.grants[1].revoked_at = Some(NOW - 1);
    assert!(project_roots(&hermes, &root, &revoked, NOW).is_empty());
    let list = projects_list(&hermes, &root, &revoked, NOW);
    assert!(list.projects[0].exists && !list.projects[0].shared);
}

#[test]
fn recording_a_hermes_project_keeps_the_marked_folders() {
    let t = Tmp::new("keep");
    let hermes = t.0.join("hermes");
    let root = t.0.join("Projects");
    std::fs::create_dir_all(&root).expect("root");
    let work = t.0.join("work");
    std::fs::create_dir_all(&work).expect("work");
    mark_project(&hermes, &rw(&work), &work, NOW).expect("mark");
    let a = project(&root, "a", "hermes");
    record_project(&hermes, &root, &a).expect("record");
    let mut both = rw(&root);
    both.grants.extend(rw(&work).grants.into_iter().map(|mut g| {
        g.id = format!("{}w", g.id);
        g
    }));
    let list = projects_list(&hermes, &root, &both, NOW);
    let origins: Vec<(&str, &str)> = list
        .projects
        .iter()
        .map(|r| (r.name.as_str(), r.origin.as_str()))
        .collect();
    assert_eq!(origins, vec![("a", "hermes"), ("work", "member")]);
    assert!(list.workspace.granted);
    assert_eq!(project_roots(&hermes, &root, &both, NOW).len(), 2);
}

#[test]
fn forget_takes_a_project_off_the_list_and_keeps_its_files() {
    let t = Tmp::new("forget");
    let hermes = t.0.join("hermes");
    let root = t.0.join("Projects");
    std::fs::create_dir_all(&root).expect("root");
    let a = project(&root, "a", "hermes");
    record_project(&hermes, &root, &a).expect("record");
    let work = t.0.join("work");
    std::fs::create_dir_all(&work).expect("work");
    mark_project(&hermes, &rw(&work), &work, NOW).expect("mark");
    forget_project(&hermes, &a.display().to_string()).expect("forget a");
    forget_project(&hermes, &work.display().to_string()).expect("forget work");
    assert!(forget_project(&hermes, "/not/on/the/list").is_err());
    assert!(projects_list(&hermes, &root, &rw(&root), NOW).projects.is_empty());
    assert!(a.is_dir() && work.is_dir(), "forget never deletes files");
    assert!(project_roots(&hermes, &root, &rw(&root), NOW).is_empty());
}

#[test]
fn the_list_shows_a_missing_hermes_project_without_sending_it() {
    let t = Tmp::new("missing");
    let hermes = t.0.join("hermes");
    let root = t.0.join("Projects");
    std::fs::create_dir_all(&root).expect("root");
    let a = project(&root, "a", "hermes");
    record_project(&hermes, &root, &a).expect("record");
    std::fs::remove_dir_all(&a).expect("rm");
    let list = projects_list(&hermes, &root, &rw(&root), NOW);
    assert_eq!(list.projects.len(), 1);
    assert!(!list.projects[0].exists && !list.projects[0].shared);
    assert!(project_roots(&hermes, &root, &rw(&root), NOW).is_empty());
}

#[test]
fn the_list_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    for c in [
        "hermes_projects_list",
        "hermes_projects_mark",
        "hermes_projects_forget",
    ] {
        assert!(acl.contains(&format!("\"{c}\"")), "{c}");
    }
}

#[test]
fn core_offers_no_project_create_tool_of_its_own() {
    // Owner decision 2026-10-15: the tool lives in the runtime; a core tool with the same name
    // would be refused at session create while CITRATE_HERMES_PROJECTS=1.
    assert!(!include_str!("../../src/agent/harness.ts").contains("name: \"project_create\""));
    assert!(!include_str!("../../src/agent/toolAnnotations.ts").contains("project_create"));
}
