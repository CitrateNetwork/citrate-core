//! HUP-S9.3 (core): the "train on my verified conversations" switch. Default off; off means no
//! recording variable for the sidecar and no training set, with nothing read or written.
use super::*;
use std::collections::BTreeMap;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fl-traj-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

/// One line as the sidecar's `export_verified` writes it (redacted, verified).
fn export_line(prompt: &str) -> String {
    serde_json::json!({
        "messages": [
            {"role": "user", "content": prompt},
            {"role": "assistant", "content": "", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "node_status", "arguments": "{}"}}]},
            {"role": "tool", "content": "{\"peer\":\"[REDACTED:address]\"}", "tool_call_id": "c1"},
            {"role": "assistant", "content": "Synced."}
        ],
        "metadata": {"model": "gemma", "workflow": null, "step": null,
                     "verifiers": ["tool_succeeded node_status"]}
    })
    .to_string()
}

fn record(hermes: &Path, name: &str, lines: &[String]) {
    let d = hermes.join(TRAJECTORIES_DIR);
    std::fs::create_dir_all(&d).expect("dir");
    std::fs::write(d.join(name), format!("{}\n", lines.join("\n"))).expect("write");
}

fn env_map(v: &[(String, String)]) -> BTreeMap<String, String> {
    v.iter().cloned().collect()
}

/// Off is not "say nothing": the variable is pinned empty so a value inherited from core's own
/// environment cannot turn recording on (the sidecar treats an empty value as off).
fn pinned_off(env: &[(String, String)]) -> bool {
    env == [(TRAJECTORIES_ENV.to_string(), String::new())]
}

#[test]
fn the_default_is_off_and_off_pins_the_variable_empty() {
    let d = tmp("default");
    assert_eq!(load(&d).expect("load"), TrajectorySettings::default());
    assert!(!enabled(&d));
    assert!(pinned_off(&sidecar_env(&d, &TrajectorySettings::default())));
    assert!(pinned_off(&(file_env_source(d.clone()))()));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn on_points_the_sidecar_at_the_trajectories_folder() {
    let d = tmp("on");
    set(&d, true, false, 7).expect("set");
    let env = env_map(&(file_env_source(d.clone()))());
    assert_eq!(
        env.get(TRAJECTORIES_ENV).map(String::as_str),
        Some(d.join(TRAJECTORIES_DIR).to_string_lossy().as_ref())
    );
    assert_eq!(env.len(), 1);
    assert_eq!(load(&d).expect("load").changed_at_ms, Some(7));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_corrupt_settings_file_reads_as_off() {
    let d = tmp("corrupt");
    std::fs::write(d.join(SETTINGS_FILE), "{\"enabled\": tru").expect("write");
    assert!(load(&d).is_err());
    assert!(!enabled(&d));
    assert!(pinned_off(&(file_env_source(d.clone()))()));
    assert!(status(&d).load_error.is_some());
    assert!(build_round_dataset(&d, 10, 1).is_err());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_relative_hermes_folder_never_turns_recording_on() {
    let on = TrajectorySettings {
        enabled: true,
        changed_at_ms: None,
    };
    assert!(pinned_off(&sidecar_env(Path::new("relative/hermes"), &on)));
}

#[test]
fn with_consent_off_no_training_set_is_built_and_nothing_is_written() {
    let d = tmp("off-build");
    record(&d, "s-100.jsonl", &[export_line("Is my node synced?")]);
    let err = build_round_dataset(&d, 500, 9).expect_err("refused");
    assert!(err.contains("off"), "{err}");
    assert!(!d.join(DATASETS_DIR).exists());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn with_consent_on_the_newest_verified_turns_are_assembled_up_to_the_cap() {
    let d = tmp("on-build");
    set(&d, true, false, 1).expect("on");
    record(
        &d,
        "sess-a-100.jsonl",
        &[export_line("old one"), export_line("old two")],
    );
    record(&d, "sess-b-200.jsonl", &[export_line("new one")]);
    // A report file and an unverified or system-prompt line are never used.
    std::fs::write(
        d.join(TRAJECTORIES_DIR).join("sess-b-200.report.json"),
        "{\"exported\":1}",
    )
    .expect("report");
    let unverified = export_line("x").replace(r#"["tool_succeeded node_status"]"#, "[]");
    let system = export_line("y").replace(r#""role":"user""#, r#""role":"system""#);
    record(&d, "sess-c-50.jsonl", &[unverified, system]);

    let s = build_round_dataset(&d, 2, 42).expect("built");
    assert_eq!(s.examples, 2);
    assert_eq!(s.over_cap, 1);
    assert_eq!(s.skipped_lines, 2);
    assert_eq!(s.files_read, 3);
    let body = std::fs::read_to_string(&s.path).expect("read");
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("new one"), "newest session first");
    assert!(lines[1].contains("old one"));
    assert!(
        body.contains("[REDACTED:address]"),
        "redaction kept verbatim"
    );
    assert_eq!(s.sha256, hex::encode(Sha256::digest(body.as_bytes())));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&s.path)
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // The same millisecond never overwrites an earlier set.
    assert!(build_round_dataset(&d, 2, 42).is_err());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn on_with_nothing_recorded_says_so() {
    let d = tmp("empty");
    set(&d, true, false, 1).expect("on");
    let err = build_round_dataset(&d, 10, 1).expect_err("nothing");
    assert!(err.contains("no verified conversations"), "{err}");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_cap_is_the_round_proposal_range() {
    let d = tmp("cap");
    set(&d, true, false, 1).expect("on");
    record(&d, "s-1.jsonl", &[export_line("a")]);
    assert!(build_round_dataset(&d, 0, 1).is_err());
    assert!(build_round_dataset(&d, crate::fl_rounds::MAX_TRAJECTORIES + 1, 1).is_err());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn turning_off_deletes_training_sets_and_on_request_the_recordings() {
    let d = tmp("withdraw");
    set(&d, true, false, 1).expect("on");
    record(&d, "s-1.jsonl", &[export_line("a")]);
    build_round_dataset(&d, 10, 2).expect("built");
    assert_eq!(status(&d).datasets, 1);

    let st = set(&d, false, false, 3).expect("off");
    assert!(!st.settings.enabled);
    assert_eq!(st.datasets, 0);
    assert_eq!(st.recorded_files, 1, "recordings stay unless asked");
    assert!(pinned_off(&(file_env_source(d.clone()))()));

    set(&d, true, false, 4).expect("on again");
    let st = set(&d, false, true, 5).expect("off and delete");
    assert_eq!(st.recorded_files, 0);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_hermes_manager_passes_the_variable_only_when_the_switch_is_on() {
    let d = tmp("manager");
    let mgr = |dir: PathBuf| {
        crate::hermes::HermesManager::new(
            PathBuf::from("/nonexistent/hermes"),
            PathBuf::from("/tmp/n7-token"),
            PathBuf::from("/tmp/n7-crash"),
        )
        .with_env_source(file_env_source(dir))
    };
    let off = env_map(&mgr(d.clone()).spec_env_for_test());
    // Off overrides a value core itself inherited (a shell or launchctl export), so the
    // sidecar never records just because the variable was set outside the member's switch.
    assert_eq!(off.get(TRAJECTORIES_ENV).map(String::as_str), Some(""));
    set(&d, true, false, 1).expect("on");
    let on = env_map(&mgr(d.clone()).spec_env_for_test());
    assert_eq!(
        on.get(TRAJECTORIES_ENV).map(String::as_str),
        Some(d.join(TRAJECTORIES_DIR).to_string_lossy().as_ref())
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in [
        "trajectories_settings_get",
        "trajectories_settings_set",
        "trajectories_dataset_build",
    ] {
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
    }
}
