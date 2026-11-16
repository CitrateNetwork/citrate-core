// V1 (owner decision 2026-11-16): the member's per-project opt-ins for `node <file>`, `npm run`
// and `git commit`. Core keeps the choice; the sidecar enforces it per session.

use super::*;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "citrate-auto-approve-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn rules(r: &[&str]) -> Vec<String> {
    r.iter().map(|s| s.to_string()).collect()
}

#[test]
fn nothing_is_opted_in_until_the_member_says_so() {
    let dir = scratch("none");
    assert_eq!(load(&dir).unwrap(), OptIns::default());
    let body = r#"{"model":"m"}"#;
    // No opt-ins: the body is sent unchanged (an older sidecar still opens the session).
    assert_eq!(attach(body, &dir).unwrap(), body);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_opt_in_travels_with_every_session_and_can_be_turned_off() {
    let dir = scratch("set");
    let proj = scratch("proj");
    set(
        &dir,
        &proj.to_string_lossy(),
        &rules(&["node-script", "git-commit", "node-script"]),
        7,
    )
    .unwrap();
    let body = attach(r#"{"model":"m"}"#, &dir).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v["autoApproveOptIns"],
        serde_json::json!([{ "root": proj.to_string_lossy(), "rules": ["node-script", "git-commit"] }])
    );
    assert_eq!(v["model"], "m");
    // Setting it again replaces the entry; empty rules remove it.
    set(&dir, &proj.to_string_lossy(), &rules(&["npm-run"]), 8).unwrap();
    assert_eq!(load(&dir).unwrap().projects.len(), 1);
    assert_eq!(load(&dir).unwrap().projects[0].rules, rules(&["npm-run"]));
    set(&dir, &proj.to_string_lossy(), &[], 9).unwrap();
    assert!(load(&dir).unwrap().projects.is_empty());
    assert_eq!(
        attach(r#"{"model":"m"}"#, &dir).unwrap(),
        r#"{"model":"m"}"#
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&proj);
}

#[test]
fn only_the_three_opt_in_commands_and_real_folders_are_accepted() {
    let dir = scratch("refuse");
    let proj = scratch("proj-refuse");
    let p = proj.to_string_lossy().to_string();
    for bad in [rules(&["ls"]), rules(&["rm"]), rules(&["npm-run", "curl"])] {
        assert!(set(&dir, &p, &bad, 1).is_err(), "{bad:?}");
    }
    assert!(set(&dir, "relative/proj", &rules(&["npm-run"]), 1).is_err());
    assert!(set(
        &dir,
        &proj.join("missing").to_string_lossy(),
        &rules(&["npm-run"]),
        1
    )
    .is_err());
    assert!(load(&dir).unwrap().projects.is_empty(), "nothing was saved");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&proj);
}

#[test]
fn a_damaged_file_or_a_deleted_folder_never_widens_anything() {
    let dir = scratch("damaged");
    std::fs::write(dir.join(OPT_INS_FILE), "{not json").unwrap();
    assert!(load(&dir).is_err());
    assert!(session_value(&dir).is_empty());
    std::fs::write(
        dir.join(OPT_INS_FILE),
        r#"{"version":2,"projects":[{"root":"/","rules":["node-script"],"changedAtMs":1}]}"#,
    )
    .unwrap();
    assert!(
        session_value(&dir).is_empty(),
        "another format applies as none"
    );
    // A folder deleted after it was opted in is left out of the session.
    let proj = scratch("gone");
    set(&dir, &proj.to_string_lossy(), &rules(&["npm-run"]), 1).unwrap();
    std::fs::remove_dir_all(&proj).unwrap();
    assert!(session_value(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_opt_in_ids_match_the_sidecar_rule_names() {
    // The ids are the sidecar list's (`agent-sidecar/policy/auto_approve.toml`, `opt_in = true`).
    assert_eq!(OPT_IN_RULES, &["node-script", "npm-run", "git-commit"]);
}
