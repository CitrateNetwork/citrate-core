// HGW-W3 (core #289, core half): the projects folder and its workspace grant, the projects core
// keeps and sends back as `projectRoots`, and the sidecar environment for the runtime's
// `project_create`. The tool itself lives in the runtime (owner decision 2026-10-15): core never
// offers a `project_create` of its own.
use super::*;
use crate::agent_grants::GrantStore;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "citrate-projects-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("home")).expect("mkdir home");
        std::fs::create_dir_all(p.join("data")).expect("mkdir data");
        // Canonical, so paths compare equal to what the stores record (macOS /var -> /private/var).
        Tmp(std::fs::canonicalize(&p).expect("canonical tmp"))
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn projects(&self) -> ProjectStore {
        ProjectStore::new(self.0.join("data/hermes"), self.home())
    }
    fn grants(&self) -> GrantStore {
        GrantStore::new(self.0.join("data/agent"), self.home())
    }
    /// Make `<root>/<name>` the way the runtime's project_create does (folder + marker).
    fn hermes_project(&self, name: &str) -> PathBuf {
        let p = projects_root(&self.home()).join(name);
        std::fs::create_dir_all(p.join(".citrate")).expect("mkdir project");
        let marker = serde_json::json!({
            "version": 1,
            "name": name,
            "root": p.display().to_string(),
            "createdBy": "hermes",
            "createdAt": 1,
        });
        std::fs::write(p.join(MARKER_FILE), marker.to_string()).expect("marker");
        p
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NOW: u64 = 1_790_000_000;

fn grant_rw(t: &Tmp, dir: &Path) {
    t.grants()
        .add_folder(dir, true, true, NOW - 10)
        .expect("grant read and write");
}

#[test]
fn the_projects_root_is_citrate_projects_in_home() {
    assert_eq!(
        projects_root(Path::new("/Users/m")),
        PathBuf::from("/Users/m/Citrate/Projects")
    );
}

#[test]
fn the_workspace_is_not_ready_until_created_and_granted() {
    let t = Tmp::new();
    let v = t.projects().view(&t.grants().view(NOW));
    assert!(!v.root_exists);
    assert!(!v.workspace_ready);
    assert!(v.projects.is_empty());
    assert_eq!(v.root, projects_root(&t.home()).display().to_string());
}

#[test]
fn enabling_the_workspace_creates_the_folder_and_grants_it_once() {
    let t = Tmp::new();
    let grants = t.grants();
    let mut adds = 0;
    let v = workspace_enable(&t.projects(), &grants.view(NOW), |root| {
        adds += 1;
        grants
            .add_folder(root, true, true, NOW)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .expect("enable");
    assert_eq!(adds, 1);
    assert!(projects_root(&t.home()).is_dir());
    let v2 = t.projects().view(&grants.view(NOW));
    assert!(v2.workspace_ready, "{v2:?}");
    assert!(v.root_exists);
    // Second time: already granted, so no second grant (the card is shown once, then remembered).
    workspace_enable(&t.projects(), &grants.view(NOW), |_| {
        adds += 1;
        Ok(())
    })
    .expect("enable again");
    assert_eq!(adds, 1);
}

#[test]
fn a_failed_grant_leaves_nothing_on_disk() {
    let t = Tmp::new();
    let r = workspace_enable(&t.projects(), &t.grants().view(NOW), |_| {
        Err("the decision record could not be written".to_string())
    });
    assert!(r.is_err());
    assert!(
        !projects_root(&t.home()).exists(),
        "the folder made for the grant is removed again"
    );
}

#[test]
fn a_created_project_is_recorded_only_inside_the_root_with_its_marker() {
    let t = Tmp::new();
    let p = t.hermes_project("demo");
    let e = t
        .projects()
        .record_created(&p.display().to_string(), NOW)
        .expect("record");
    assert_eq!(e.name, "demo");
    assert_eq!(e.origin, Origin::Hermes);
    assert_eq!(e.path, p.display().to_string());
    // Recording it again keeps one entry.
    t.projects()
        .record_created(&p.display().to_string(), NOW + 1)
        .expect("again");
    assert_eq!(t.projects().load().expect("load").projects.len(), 1);

    // Outside the projects root: refused, even with a marker.
    let outside = t.home().join("elsewhere");
    std::fs::create_dir_all(outside.join(".citrate")).expect("mkdir");
    std::fs::write(outside.join(MARKER_FILE), r#"{"createdBy":"hermes"}"#).expect("w");
    assert!(t
        .projects()
        .record_created(&outside.display().to_string(), NOW)
        .is_err());
    // Inside the root but with no marker: refused.
    let bare = projects_root(&t.home()).join("bare");
    std::fs::create_dir_all(&bare).expect("mkdir");
    assert!(t
        .projects()
        .record_created(&bare.display().to_string(), NOW)
        .is_err());
    // Traversal and relative paths: refused.
    for bad in [
        "relative/demo".to_string(),
        format!("{}/../demo", projects_root(&t.home()).display()),
        format!("{}/Citrate/Projects/demo/..", t.home().display()),
    ] {
        assert!(t.projects().record_created(&bad, NOW).is_err(), "{bad}");
    }
}

#[test]
fn a_member_marks_only_a_folder_granted_for_reading_and_writing() {
    let t = Tmp::new();
    let work = t.home().join("work/site");
    std::fs::create_dir_all(&work).expect("mkdir");
    let refused = t
        .projects()
        .mark(&work.display().to_string(), &t.grants().view(NOW), NOW);
    assert!(refused.is_err(), "not granted yet");
    t.grants()
        .add_folder(&t.home().join("work"), true, false, NOW - 10)
        .expect("read only");
    assert!(
        t.projects()
            .mark(&work.display().to_string(), &t.grants().view(NOW), NOW)
            .is_err(),
        "read alone is not enough"
    );
    t.grants()
        .add_folder(&t.home().join("work"), false, true, NOW - 10)
        .expect("write");
    let e = t
        .projects()
        .mark(&work.display().to_string(), &t.grants().view(NOW), NOW)
        .expect("mark");
    assert_eq!(e.origin, Origin::Member);
    assert_eq!(e.name, "site");
}

#[test]
fn only_live_projects_travel_with_a_session() {
    let t = Tmp::new();
    let root = projects_root(&t.home());
    let demo = t.hermes_project("demo");
    let gone = t.hermes_project("gone");
    t.projects()
        .record_created(&demo.display().to_string(), NOW)
        .expect("demo");
    t.projects()
        .record_created(&gone.display().to_string(), NOW)
        .expect("gone");
    // Not granted: nothing is sent (a refused root would refuse the whole session).
    assert!(t.projects().live_roots(&t.grants().view(NOW)).is_empty());
    grant_rw(&t, &root);
    std::fs::remove_dir_all(&gone).expect("rm");
    let live = t.projects().live_roots(&t.grants().view(NOW));
    assert_eq!(live, vec![demo.display().to_string()]);
    let v = t.projects().view(&t.grants().view(NOW));
    let row = |p: &Path| {
        v.projects
            .iter()
            .find(|r| r.path == p.display().to_string())
            .cloned()
            .expect("row")
    };
    assert!(row(&demo).exists && row(&demo).shared);
    assert!(!row(&gone).exists);
    // Revoking the write grant takes every project out of the next session.
    let write_id = t
        .grants()
        .view(NOW)
        .grants
        .iter()
        .find(|g| g.access == "write")
        .map(|g| g.id.clone())
        .expect("write grant");
    t.grants().revoke(&write_id, NOW).expect("revoke");
    assert!(t.projects().live_roots(&t.grants().view(NOW)).is_empty());
}

#[test]
fn forgetting_a_project_keeps_its_files() {
    let t = Tmp::new();
    let demo = t.hermes_project("demo");
    t.projects()
        .record_created(&demo.display().to_string(), NOW)
        .expect("record");
    t.projects()
        .forget(&demo.display().to_string())
        .expect("forget");
    assert!(t.projects().load().expect("load").projects.is_empty());
    assert!(demo.is_dir(), "forget never deletes files");
}

#[test]
fn a_corrupt_project_list_sends_no_roots_and_says_why() {
    let t = Tmp::new();
    std::fs::create_dir_all(t.0.join("data/hermes")).expect("mkdir");
    std::fs::write(t.0.join("data/hermes").join(PROJECTS_FILE), "{not json").expect("w");
    assert!(t.projects().load().is_err());
    assert!(t.projects().live_roots(&t.grants().view(NOW)).is_empty());
    assert!(t
        .projects()
        .view(&t.grants().view(NOW))
        .load_error
        .is_some());
}

#[test]
fn project_roots_join_the_session_body_only_when_there_are_some() {
    let body = r#"{"model":"m","hicAware":true}"#;
    assert_eq!(attach_project_roots(body, &[]).expect("none"), body);
    let with: serde_json::Value = serde_json::from_str(
        &attach_project_roots(body, &["/Users/m/Citrate/Projects/demo".to_string()]).expect("some"),
    )
    .expect("json");
    assert_eq!(
        with["projectRoots"],
        serde_json::json!(["/Users/m/Citrate/Projects/demo"])
    );
    assert_eq!(with["hicAware"], true);
}

#[test]
fn the_sidecar_env_turns_projects_on_and_names_the_renderer_only_with_both_paths() {
    let home = Path::new("/Users/m");
    let get = |env: &[(String, String)], k: &str| {
        env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone())
    };
    let env = sidecar_env(home, None, None);
    assert_eq!(get(&env, PROJECTS_ENV).as_deref(), Some("1"));
    assert_eq!(
        get(&env, PROJECTS_ROOT_ENV).as_deref(),
        Some("/Users/m/Citrate/Projects")
    );
    // No renderer: both pinned empty, so an inherited value can never name another program.
    assert_eq!(get(&env, TEMPLATE_RENDERER_ENV).as_deref(), Some(""));
    assert_eq!(get(&env, TEMPLATES_ROOT_ENV).as_deref(), Some(""));
    let r = Renderer {
        program: PathBuf::from("/Applications/Citrate Core.app/Contents/MacOS/citrate-templates"),
        templates: PathBuf::from(
            "/Applications/Citrate Core.app/Contents/Resources/_up_/templates",
        ),
        tier: "T1".into(),
    };
    let env = sidecar_env(home, Some(&r), None);
    assert_eq!(
        get(&env, TEMPLATE_RENDERER_ENV).as_deref(),
        Some("/Applications/Citrate Core.app/Contents/MacOS/citrate-templates")
    );
    assert_eq!(
        get(&env, TEMPLATES_ROOT_ENV).as_deref(),
        Some("/Applications/Citrate Core.app/Contents/Resources/_up_/templates")
    );
    assert_eq!(get(&env, TEMPLATE_TIER_ENV).as_deref(), Some("T1"));
    for (k, _) in &env {
        assert!(SIDECAR_ENV_KEYS.contains(&k.as_str()), "{k}");
    }
}

#[test]
fn the_members_git_identity_is_passed_only_when_valid() {
    let get = |env: &[(String, String)], k: &str| {
        env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone())
    };
    let home = Path::new("/Users/m");
    let id = valid_identity("Larry Klosowski", "larry@example.org");
    assert!(id.is_some());
    let env = sidecar_env(home, None, id);
    assert_eq!(
        get(&env, GIT_USER_NAME_ENV).as_deref(),
        Some("Larry Klosowski")
    );
    assert_eq!(
        get(&env, GIT_USER_EMAIL_ENV).as_deref(),
        Some("larry@example.org")
    );
    // Not set or not valid: pinned empty (the runtime then falls back to Hermes).
    let env = sidecar_env(home, None, None);
    assert_eq!(get(&env, GIT_USER_NAME_ENV).as_deref(), Some(""));
    assert_eq!(get(&env, GIT_USER_EMAIL_ENV).as_deref(), Some(""));
    for (n, e) in [
        ("", "a@b"),
        ("Name", ""),
        ("Na<me", "a@b"),
        ("Name", "not-an-address"),
        ("Name", "a b@c"),
        ("Line\nbreak", "a@b"),
        ("Name", "a@"),
    ] {
        assert!(valid_identity(n, e).is_none(), "{n:?} {e:?}");
    }
    assert!(valid_identity(&"n".repeat(257), "a@b").is_none());
}

#[test]
fn the_members_git_identity_is_read_from_their_global_config_only() {
    // A scratch HOME with a global config; no repository, no system config.
    let t = Tmp::new();
    let Some(git) = git_program() else {
        return; // no git on this machine: nothing to read, nothing is passed
    };
    assert!(git.is_absolute());
    assert_eq!(
        member_git_identity(&t.home()),
        None,
        "no config, no identity"
    );
    std::fs::write(
        t.home().join(".gitconfig"),
        "[user]\n\tname = Scratch Member\n\temail = scratch@example.org\n",
    )
    .expect("gitconfig");
    assert_eq!(
        member_git_identity(&t.home()),
        Some((
            "Scratch Member".to_string(),
            "scratch@example.org".to_string()
        ))
    );
}

#[test]
fn core_offers_no_project_create_tool_of_its_own() {
    // Owner decision 2026-10-15: the tool lives in the runtime. A core tool with the same name
    // would be refused at session create while CITRATE_HERMES_PROJECTS=1.
    let harness = include_str!("../../src/agent/harness.ts");
    assert!(!harness.contains("\"project_create\""));
    let annotations = include_str!("../../src/agent/toolAnnotations.ts");
    assert!(!annotations.contains("project_create"));
}

#[test]
fn the_new_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    for c in [
        "hermes_projects_view",
        "hermes_projects_workspace_enable",
        "hermes_projects_record_created",
        "hermes_projects_mark",
        "hermes_projects_forget",
    ] {
        assert!(acl.contains(&format!("\"{c}\"")), "{c}");
    }
}

#[test]
fn the_hermes_manager_passes_the_projects_env() {
    let t = Tmp::new();
    let m = crate::hermes::HermesManager::new(t.0.join("bin"), t.0.join("t"), t.0.join("c"))
        .with_env_source(std::sync::Arc::new(|| {
            sidecar_env(
                Path::new("/Users/m"),
                None,
                valid_identity("Member", "member@example.org"),
            )
        }));
    let env = m.spec_env_for_test();
    let has = |k: &str, v: &str| env.iter().any(|(key, val)| key == k && val == v);
    assert!(has(PROJECTS_ENV, "1"), "{env:?}");
    assert!(has(PROJECTS_ROOT_ENV, "/Users/m/Citrate/Projects"));
    assert!(has(GIT_USER_NAME_ENV, "Member"));
    assert!(has(GIT_USER_EMAIL_ENV, "member@example.org"));
}
