// HGW-W11 + the member's commit identity: the settings that set CITRATE_HERMES_AUTO_APPROVE and
// CITRATE_HERMES_GIT_USER_{NAME,EMAIL} for the sidecar. On by default with no identity; a
// corrupt file is off with no identity; every variable is pinned; a change restarts Hermes once.
use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesManager};
use std::cell::Cell;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hermes-build-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

fn val(env: &[(String, String)], k: &str) -> Option<String> {
    env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}

#[test]
fn the_defaults_pin_the_shipped_list_and_no_identity() {
    let d = tmp("default");
    assert_eq!(load(&d).expect("load"), BuildSettings::default());
    let env = (file_env_source(d.clone()))();
    assert_eq!(val(&env, AUTO_APPROVE_ENV).as_deref(), Some(""), "empty = the shipped list");
    assert_eq!(val(&env, GIT_USER_NAME_ENV).as_deref(), Some(""));
    assert_eq!(val(&env, GIT_USER_EMAIL_ENV).as_deref(), Some(""));
    for (k, _) in &env {
        assert!(SIDECAR_ENV_KEYS.contains(&k.as_str()), "{k}");
    }
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn off_passes_zero_and_a_corrupt_file_is_off_with_no_identity() {
    let d = tmp("off");
    let st = apply(&d, false, None, None, || Ok(true)).expect("apply");
    assert!(!st.auto_approve);
    assert_eq!(val(&(file_env_source(d.clone()))(), AUTO_APPROVE_ENV).as_deref(), Some("0"));
    std::fs::write(
        d.join(SETTINGS_FILE),
        r#"{"autoApprove": true, "gitName": "M", "gitEmail": "m@x.y", "extra": 1}"#,
    )
    .expect("write");
    let env = (file_env_source(d.clone()))();
    assert_eq!(val(&env, AUTO_APPROVE_ENV).as_deref(), Some("0"));
    assert_eq!(val(&env, GIT_USER_NAME_ENV).as_deref(), Some(""));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn an_identity_is_passed_only_when_both_parts_are_set_and_valid() {
    let d = tmp("identity");
    apply(&d, true, Some(" Ada Lovelace ".into()), Some("ada@example.org".into()), || Ok(false))
        .expect("apply");
    let env = (file_env_source(d.clone()))();
    assert_eq!(val(&env, GIT_USER_NAME_ENV).as_deref(), Some("Ada Lovelace"));
    assert_eq!(val(&env, GIT_USER_EMAIL_ENV).as_deref(), Some("ada@example.org"));
    // Half an identity, a bad name or a bad address is refused and nothing changes.
    for (n, e) in [
        (Some("Ada".to_string()), None),
        (None, Some("ada@example.org".to_string())),
        (Some("Ada <x>".to_string()), Some("ada@example.org".to_string())),
        (Some("Ada".to_string()), Some("not an address".to_string())),
        (Some("Ada".to_string()), Some("@example.org".to_string())),
        (Some("A\nB".to_string()), Some("ada@example.org".to_string())),
        (Some("x".repeat(257)), Some("ada@example.org".to_string())),
    ] {
        assert!(apply(&d, true, n.clone(), e.clone(), || Ok(false)).is_err(), "{n:?} {e:?}");
    }
    assert_eq!(load(&d).expect("load").git_name.as_deref(), Some("Ada Lovelace"));
    // Clearing both goes back to the member's git config.
    apply(&d, true, Some("  ".into()), None, || Ok(false)).expect("clear");
    assert_eq!(val(&(file_env_source(d.clone()))(), GIT_USER_NAME_ENV).as_deref(), Some(""));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_change_restarts_hermes_once_and_resending_restarts_nothing() {
    let d = tmp("restart");
    let n = Cell::new(0);
    let r = || {
        n.set(n.get() + 1);
        Ok(true)
    };
    // The first explicit "on" writes the file; what Hermes receives is unchanged: no restart.
    assert!(!apply(&d, true, None, None, r).expect("first").restarted);
    assert_eq!(n.get(), 0);
    assert!(apply(&d, false, None, None, || {
        n.set(n.get() + 1);
        Ok(true)
    })
    .expect("off")
    .restarted);
    assert_eq!(n.get(), 1);
    assert!(!apply(&d, false, None, None, || {
        n.set(n.get() + 1);
        Ok(true)
    })
    .expect("again")
    .restarted);
    assert_eq!(n.get(), 1);
    let _ = std::fs::remove_dir_all(d);
}

struct ListControl(u16, &'static str);
impl HermesControl for ListControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        assert!(url.ends_with("/shell/auto-approve"), "{url}");
        Ok(ControlResp {
            status: self.0,
            body: self.1.to_string(),
        })
    }
    fn post(&self, _u: &str, _b: &str, _body: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp {
            status: 500,
            body: String::new(),
        })
    }
}

fn mgr(c: ListControl) -> HermesManager {
    let dir = std::env::temp_dir().join(format!("hbuild-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(c));
    m.set_token_for_test("deadbeef");
    m
}

#[test]
fn the_list_is_read_from_the_running_sidecar_and_checked() {
    let body = r#"{"enabled":true,"conditions":["untainted"],"rules":[{"program":"npm"}]}"#;
    let v = auto_approve_list(&mgr(ListControl(200, body))).expect("list");
    assert_eq!(v["rules"][0]["program"], "npm");
    assert!(auto_approve_list(&mgr(ListControl(404, ""))).expect_err("old").contains("no auto-approve list"));
    assert!(auto_approve_list(&mgr(ListControl(200, r#"{"rules":[]}"#))).is_err());
    assert!(auto_approve_list(&mgr(ListControl(500, ""))).is_err());
}

#[test]
fn the_new_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    for c in ["hermes_build_settings_get", "hermes_build_settings_set", "hermes_auto_approve_list"] {
        assert!(acl.contains(&format!("\"{c}\"")), "{c}");
    }
}
