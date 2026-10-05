// Hermes terminal commands: the switch that sets `CITRATE_HERMES_SHELL_RUN` for the sidecar.
// On by default (owner decision 2026-10-04); off pins the variable empty; a corrupt file is off;
// a change restarts a running Hermes once, and re-sending the same value restarts nothing.
use super::*;
use std::cell::Cell;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hermes-term-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

fn shell_run_value(env: &[(String, String)]) -> Option<String> {
    env.iter()
        .find(|(k, _)| k == SHELL_RUN_ENV)
        .map(|(_, v)| v.clone())
}

#[test]
fn the_default_is_on_and_on_sets_the_variable_to_one() {
    let d = tmp("default");
    assert_eq!(load(&d).expect("load"), TerminalSettings::default());
    assert!(TerminalSettings::default().enabled, "owner default: on");
    assert!(enabled(&d));
    let env = (file_env_source(d.clone()))();
    assert_eq!(env, vec![(SHELL_RUN_ENV.to_string(), "1".to_string())]);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn off_pins_the_variable_empty_so_an_inherited_value_cannot_turn_it_on() {
    let d = tmp("off");
    save(
        &d,
        &TerminalSettings {
            enabled: false,
            changed_at_ms: Some(1),
        },
    )
    .expect("save");
    let env = (file_env_source(d.clone()))();
    assert_eq!(shell_run_value(&env).as_deref(), Some(""));
    assert_eq!(env.len(), 1);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_corrupt_or_unreadable_setting_counts_as_off() {
    let d = tmp("corrupt");
    std::fs::write(d.join(SETTINGS_FILE), "{not json").expect("write");
    assert!(load(&d).is_err());
    assert!(!enabled(&d));
    assert_eq!(
        shell_run_value(&(file_env_source(d.clone()))()).as_deref(),
        Some("")
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_env_carries_only_the_switch_never_a_path_or_secret() {
    for on in [true, false] {
        let env = sidecar_env(on);
        assert!(env.iter().all(|(k, _)| SIDECAR_ENV_KEYS.contains(&k.as_str())));
        assert!(env.iter().all(|(_, v)| v.is_empty() || v == "1"));
    }
}

#[test]
fn a_change_restarts_hermes_once_and_the_same_value_restarts_nothing() {
    let d = tmp("apply");
    let restarts = Cell::new(0u32);
    let restart = || {
        restarts.set(restarts.get() + 1);
        Ok(true)
    };
    // Fresh install: the default is on, so sending "on" only records it (no restart needed).
    let st = apply(&d, true, 10, restart).expect("apply on");
    assert_eq!((st.enabled, st.restarted, restarts.get()), (true, false, 0));
    assert!(d.join(SETTINGS_FILE).exists());
    // Off: a real change, so a running Hermes is restarted.
    let st = apply(&d, false, 11, || {
        restarts.set(restarts.get() + 1);
        Ok(true)
    })
    .expect("apply off");
    assert_eq!((st.enabled, st.restarted, restarts.get()), (false, true, 1));
    assert!(!enabled(&d));
    // The same value again (the launch sync): nothing written, nothing restarted.
    let st = apply(&d, false, 12, || {
        restarts.set(restarts.get() + 1);
        Ok(true)
    })
    .expect("apply off again");
    assert_eq!((st.enabled, st.restarted, restarts.get()), (false, false, 1));
    assert_eq!(load(&d).expect("load").changed_at_ms, Some(11));
    // On again, with Hermes stopped: stored, and the restart hook reports nothing restarted.
    let st = apply(&d, true, 13, || Ok(false)).expect("apply on");
    assert_eq!((st.enabled, st.restarted), (true, false));
    assert!(enabled(&d));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_corrupt_setting_is_replaced_by_the_members_choice_and_applied() {
    let d = tmp("repair");
    std::fs::write(d.join(SETTINGS_FILE), "garbage").expect("write");
    // Corrupt reads as off, so turning it on is a change and restarts a running Hermes.
    let st = apply(&d, true, 5, || Ok(true)).expect("apply");
    assert_eq!((st.enabled, st.restarted), (true, true));
    assert!(enabled(&d));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_failed_restart_is_reported_after_the_choice_is_stored() {
    let d = tmp("restart-fail");
    let e = apply(&d, false, 5, || Err("the port is held".into())).expect_err("reported");
    assert!(e.contains("port"), "{e}");
    assert!(!enabled(&d), "the member's choice is kept even when the restart failed");
    let _ = std::fs::remove_dir_all(d);
}

/// The manager passes the switch to the sidecar spawn: on = `1`, off = pinned empty, and the
/// env source cannot smuggle any other variable through this key list.
#[test]
fn the_hermes_spawn_env_follows_the_switch() {
    let d = tmp("spawn");
    let mgr = |dir: &Path| {
        crate::hermes::HermesManager::new(
            PathBuf::from("/nonexistent/hermes"),
            dir.join("bearer.token"),
            dir.join("crashes.log"),
        )
        .with_env_source(file_env_source(dir.to_path_buf()))
    };
    // Default (no file): on.
    let env = mgr(&d).spec_env_for_test();
    assert_eq!(shell_run_value(&env).as_deref(), Some("1"), "{env:?}");
    // Off.
    apply(&d, false, 1, || Ok(false)).expect("off");
    let env = mgr(&d).spec_env_for_test();
    assert_eq!(shell_run_value(&env).as_deref(), Some(""), "{env:?}");
    assert!(
        !env.iter().any(|(k, v)| k == SHELL_RUN_ENV && v == "1"),
        "off never offers shell_run"
    );
    // Back on.
    apply(&d, true, 2, || Ok(false)).expect("on");
    let env = mgr(&d).spec_env_for_test();
    assert_eq!(shell_run_value(&env).as_deref(), Some("1"));
    // A source that tries another key next to the switch: only the switch passes this module's list.
    let sneaky: crate::hermes_web::EnvSource = std::sync::Arc::new(|| {
        vec![
            (SHELL_RUN_ENV.to_string(), "1".to_string()),
            ("CITRATE_HERMES_TOKEN_FILE".to_string(), "/tmp/evil".to_string()),
        ]
    });
    let env = crate::hermes::HermesManager::new(
        PathBuf::from("/nonexistent/hermes"),
        d.join("bearer.token"),
        d.join("crashes.log"),
    )
    .with_env_source(sneaky)
    .spec_env_for_test();
    assert_eq!(
        env.iter()
            .filter(|(k, _)| k == "CITRATE_HERMES_TOKEN_FILE")
            .count(),
        1,
        "the bearer-file path is the manager's own, never the source's"
    );
    assert!(!env.iter().any(|(_, v)| v == "/tmp/evil"));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_commands_are_registered_and_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    let lib = include_str!("lib.rs");
    for cmd in ["hermes_terminal_get", "hermes_terminal_set"] {
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
        assert!(
            lib.contains(&format!("hermes_terminal::{cmd}")),
            "{cmd} registered in lib.rs"
        );
    }
}
