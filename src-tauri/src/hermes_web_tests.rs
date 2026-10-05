// HUP-S5.2 / S5.3: the member's web search, page-reading and decide() opt-ins, and how they
// reach the Hermes sidecar's environment. Included from `hermes_web.rs` (`mod tests`).

use super::*;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "n4-hermes-web-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("tmp dir");
    d
}

fn env_map(v: &[(String, String)]) -> std::collections::BTreeMap<String, String> {
    v.iter().cloned().collect()
}

#[test]
fn the_default_changes_nothing() {
    let s = HermesWebSettings::default();
    assert!(!s.browser_enabled, "Hermes's browser is off by default");
    assert!(!s.search_enabled);
    assert_eq!(s.reader, ReaderChoice::Local);
    assert!(!s.jev_enabled);
    assert!(sidecar_env(&s, Path::new("/data/hermes")).is_empty());
}

#[test]
fn search_on_passes_the_switch_the_searxng_path_and_a_private_data_dir() {
    let s = HermesWebSettings {
        search_enabled: true,
        searxng_path: Some("/opt/searxng/bin/searxng-run".into()),
        ..HermesWebSettings::default()
    };
    let env = env_map(&sidecar_env(&s, Path::new("/data/hermes")));
    assert_eq!(env["CITRATE_HERMES_SEARCH"], "1");
    assert_eq!(
        env["CITRATE_HERMES_SEARXNG"],
        "/opt/searxng/bin/searxng-run"
    );
    assert_eq!(env["CITRATE_HERMES_SEARXNG_DATA"], "/data/hermes/searxng");
    assert_eq!(
        env["CITRATE_HERMES_DECIDE_LOG"],
        "/data/hermes/decisions.jsonl"
    );
    assert!(
        !env.contains_key("CITRATE_HERMES_READER"),
        "local reading is the default"
    );
    assert!(!env.contains_key("CITRATE_HERMES_JEV"));
}

#[test]
fn the_jina_reader_needs_search_on_and_its_own_choice() {
    let s = HermesWebSettings {
        search_enabled: true,
        reader: ReaderChoice::Jina,
        jina_key_file: Some("/keys/jina".into()),
        ..HermesWebSettings::default()
    };
    let env = env_map(&sidecar_env(&s, Path::new("/d")));
    assert_eq!(env["CITRATE_HERMES_READER"], "jina");
    assert_eq!(env["CITRATE_HERMES_JINA_KEY_FILE"], "/keys/jina");
    // With search off, the reader choice is inert.
    let off = HermesWebSettings {
        search_enabled: false,
        ..s
    };
    assert!(sidecar_env(&off, Path::new("/d")).is_empty());
}

#[test]
fn jev_passes_its_opt_ins_and_nothing_else() {
    let s = HermesWebSettings {
        jev_enabled: true,
        jev_key_file: Some("/keys/jev".into()),
        jev_origins: vec![
            "https://shop.example".into(),
            "https://docs.example:8443".into(),
        ],
        jev_non_web: false,
        ..HermesWebSettings::default()
    };
    let env = env_map(&sidecar_env(&s, Path::new("/d")));
    assert_eq!(env["CITRATE_HERMES_JEV"], "1");
    assert_eq!(env["CITRATE_HERMES_JEV_KEY_FILE"], "/keys/jev");
    assert_eq!(
        env["CITRATE_HERMES_JEV_ORIGINS"],
        "https://shop.example,https://docs.example:8443"
    );
    assert!(!env.contains_key("CITRATE_HERMES_JEV_NON_WEB"));
    assert!(!env.contains_key("CITRATE_HERMES_SEARCH"));
    let nw = HermesWebSettings {
        jev_non_web: true,
        ..s
    };
    assert_eq!(
        env_map(&sidecar_env(&nw, Path::new("/d")))["CITRATE_HERMES_JEV_NON_WEB"],
        "1"
    );
}

#[test]
fn every_emitted_key_is_on_the_allowlist() {
    let s = HermesWebSettings {
        browser_enabled: true,
        search_enabled: true,
        searxng_path: Some("/a".into()),
        reader: ReaderChoice::Jina,
        jina_key_file: Some("/b".into()),
        jev_enabled: true,
        jev_key_file: Some("/c".into()),
        jev_origins: vec!["https://x.example".into()],
        jev_non_web: true,
    };
    let emitted = sidecar_env_with(&s, Path::new("/d"), Some(Path::new("/c/chrome")));
    assert!(emitted.iter().any(|(k, _)| k == "CITRATE_BROWSER_CHROMIUM"));
    for (k, _) in emitted {
        assert!(SIDECAR_ENV_KEYS.contains(&k.as_str()), "{k}");
    }
    for reserved in [
        "CITRATE_HERMES_ADDR",
        "CITRATE_HERMES_TOKEN_FILE",
        "CITRATE_HERMES_CAPSULES",
        "CITRATE_HERMES_BIN",
    ] {
        assert!(!SIDECAR_ENV_KEYS.contains(&reserved));
    }
}

#[test]
fn validation_normalizes_and_refuses_bad_values() {
    let ok = validate(HermesWebSettings {
        search_enabled: true,
        searxng_path: Some("  /opt/sx/bin/searxng-run ".into()),
        jev_origins: vec![
            "https://Shop.Example/".into(),
            "https://shop.example".into(),
            " https://docs.example:8443 ".into(),
        ],
        ..HermesWebSettings::default()
    })
    .expect("valid");
    assert_eq!(ok.searxng_path.as_deref(), Some("/opt/sx/bin/searxng-run"));
    assert_eq!(
        ok.jev_origins,
        vec!["https://shop.example", "https://docs.example:8443"]
    );
    let blank = validate(HermesWebSettings {
        searxng_path: Some("   ".into()),
        ..HermesWebSettings::default()
    })
    .expect("blank path is none");
    assert!(blank.searxng_path.is_none());

    for bad in [
        HermesWebSettings {
            searxng_path: Some("searxng-run".into()),
            ..HermesWebSettings::default()
        },
        HermesWebSettings {
            jina_key_file: Some("relative/key".into()),
            ..HermesWebSettings::default()
        },
        HermesWebSettings {
            jev_key_file: Some("key".into()),
            ..HermesWebSettings::default()
        },
        HermesWebSettings {
            jev_origins: vec!["http://shop.example".into()],
            ..HermesWebSettings::default()
        },
        HermesWebSettings {
            jev_origins: vec!["https://shop.example/path".into()],
            ..HermesWebSettings::default()
        },
        HermesWebSettings {
            jev_origins: vec!["https://user@shop.example".into()],
            ..HermesWebSettings::default()
        },
        HermesWebSettings {
            jev_origins: (0..=MAX_JEV_ORIGINS)
                .map(|i| format!("https://o{i}.example"))
                .collect(),
            ..HermesWebSettings::default()
        },
    ] {
        assert!(validate(bad.clone()).is_err(), "{bad:?}");
    }
}

#[test]
fn settings_round_trip_and_a_missing_file_is_the_default() {
    let dir = tmp("rt");
    assert_eq!(
        load(&dir).expect("missing ok"),
        HermesWebSettings::default()
    );
    let s = HermesWebSettings {
        search_enabled: true,
        reader: ReaderChoice::Jina,
        jev_origins: vec!["https://a.example".into()],
        ..HermesWebSettings::default()
    };
    save(&dir, &s).expect("save");
    assert_eq!(load(&dir).expect("load"), s);
    let text = std::fs::read_to_string(dir.join(SETTINGS_FILE)).expect("read");
    assert!(text.contains("\"searchEnabled\": true"), "{text}");
    std::fs::write(dir.join(SETTINGS_FILE), "{not json").expect("write");
    assert!(
        load(&dir).is_err(),
        "a corrupt file is reported, not silently reset"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn status_reports_what_is_found_and_says_what_leaves_the_machine() {
    let dir = tmp("status");
    let prog = dir.join("searxng-run");
    std::fs::write(&prog, "#!/bin/sh\n").expect("prog");
    let s = HermesWebSettings {
        search_enabled: true,
        searxng_path: Some(prog.to_string_lossy().into_owned()),
        reader: ReaderChoice::Jina,
        jev_enabled: true,
        jev_origins: vec!["https://shop.example".into()],
        ..HermesWebSettings::default()
    };
    let st = status_for(s, None);
    assert!(st.searxng_found);
    assert!(!st.jev_key_file_found);
    assert!(st
        .notices
        .iter()
        .any(|n| n.contains("Jina Reader") && n.contains("third-party")));
    assert!(st.notices.iter().any(|n| n.contains("TypeSafe")));
    assert!(st
        .notices
        .iter()
        .any(|n| n.contains("no key file") && n.contains("stays off")));
    assert!(st.applies_on_restart);
    let quiet = status_for(HermesWebSettings::default(), None);
    assert!(quiet.notices.is_empty());
    assert!(!quiet.searxng_found);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_manager_passes_the_settings_env_to_the_sidecar_spec() {
    let s = HermesWebSettings {
        search_enabled: true,
        ..HermesWebSettings::default()
    };
    let src = env_source_from(move || s.clone(), PathBuf::from("/d/hermes"));
    let m = crate::hermes::HermesManager::new(
        PathBuf::from("/nonexistent/hermes"),
        PathBuf::from("/tmp/n4-token"),
        PathBuf::from("/tmp/n4-crash"),
    )
    .with_env_source(src);
    let env = env_map(&m.spec_env_for_test());
    assert_eq!(env["CITRATE_HERMES_SEARCH"], "1");
    assert!(env.contains_key("CITRATE_HERMES_ADDR"));
    let plain = crate::hermes::HermesManager::new(
        PathBuf::from("/nonexistent/hermes"),
        PathBuf::from("/tmp/n4-token"),
        PathBuf::from("/tmp/n4-crash"),
    );
    assert!(!env_map(&plain.spec_env_for_test()).contains_key("CITRATE_HERMES_SEARCH"));
}

#[test]
fn an_env_source_cannot_override_the_control_bind_or_token() {
    let src: EnvSource = std::sync::Arc::new(|| {
        vec![
            ("CITRATE_HERMES_ADDR".to_string(), "0.0.0.0:1".to_string()),
            ("CITRATE_HERMES_TOKEN_FILE".to_string(), "/evil".to_string()),
            ("CITRATE_HERMES_SEARCH".to_string(), "1".to_string()),
        ]
    });
    let m = crate::hermes::HermesManager::new(
        PathBuf::from("/nonexistent/hermes"),
        PathBuf::from("/tmp/n4-token"),
        PathBuf::from("/tmp/n4-crash"),
    )
    .with_env_source(src);
    let env = m.spec_env_for_test();
    assert_eq!(
        env.iter()
            .filter(|(k, _)| k == "CITRATE_HERMES_ADDR")
            .count(),
        1
    );
    assert!(env
        .iter()
        .all(|(k, v)| !(k == "CITRATE_HERMES_ADDR" && v == "0.0.0.0:1")));
    assert!(env.iter().all(|(_, v)| v != "/evil"));
    assert!(env.iter().any(|(k, _)| k == "CITRATE_HERMES_SEARCH"));
}

#[test]
fn the_browser_switch_passes_only_its_own_variables() {
    let on = HermesWebSettings {
        browser_enabled: true,
        ..HermesWebSettings::default()
    };
    let env = env_map(&sidecar_env(&on, Path::new("/d")));
    assert_eq!(env["CITRATE_HERMES_BROWSER"], "1");
    assert!(
        !env.contains_key("CITRATE_BROWSER_CHROMIUM"),
        "no managed Chromium installed: the sidecar looks for a system one"
    );
    assert!(!env.contains_key("CITRATE_HERMES_SEARCH"));
    assert!(!env.contains_key("CITRATE_HERMES_DECIDE_LOG"));
    assert_eq!(env.len(), 1);
    // With the managed Chromium installed, the sidecar is pointed at it.
    let managed = Path::new("/data/components/chromium/154/chrome");
    let env = env_map(&sidecar_env_with(&on, Path::new("/d"), Some(managed)));
    assert_eq!(
        env["CITRATE_BROWSER_CHROMIUM"],
        "/data/components/chromium/154/chrome"
    );
    // A relative path is never passed.
    let env = env_map(&sidecar_env_with(
        &on,
        Path::new("/d"),
        Some(Path::new("chrome")),
    ));
    assert!(!env.contains_key("CITRATE_BROWSER_CHROMIUM"));
    // With the switch off, an installed managed Chromium changes nothing.
    assert!(sidecar_env_with(&HermesWebSettings::default(), Path::new("/d"), Some(managed)).is_empty());
}

#[test]
fn an_old_settings_file_without_the_browser_switch_loads_with_it_off() {
    let dir = tmp("old");
    std::fs::write(
        dir.join(SETTINGS_FILE),
        r#"{"searchEnabled": true, "reader": "local"}"#,
    )
    .expect("write");
    let s = load(&dir).expect("load");
    assert!(s.search_enabled);
    assert!(!s.browser_enabled);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_status_says_where_the_browser_comes_from() {
    let on = HermesWebSettings {
        browser_enabled: true,
        ..HermesWebSettings::default()
    };
    let st = status_with(on.clone(), None, None);
    assert_eq!(st.managed_chromium, None);
    assert!(st
        .notices
        .iter()
        .any(|n| n.contains("not installed yet") && n.contains("fresh private profile")));
    assert!(st
        .notices
        .iter()
        .any(|n| n.contains("untrusted") && n.contains("each session")));
    let st = status_with(on, None, Some(Path::new("/c/chrome")));
    assert_eq!(st.managed_chromium.as_deref(), Some("/c/chrome"));
    assert!(st.notices.iter().any(|n| n.contains("managed Chromium with a fresh")));
    assert!(status_for(HermesWebSettings::default(), None).notices.is_empty());
}

#[test]
fn the_file_env_source_reads_the_browser_switch_at_each_start() {
    let dir = tmp("file-src");
    let components = tmp("file-src-components");
    let src = file_env_source(dir.clone(), Some(components.clone()));
    assert!(src().is_empty(), "no settings file: nothing is passed");
    save(
        &dir,
        &HermesWebSettings {
            browser_enabled: true,
            ..HermesWebSettings::default()
        },
    )
    .expect("save");
    let env = env_map(&src());
    assert_eq!(env["CITRATE_HERMES_BROWSER"], "1");
    assert!(
        !env.contains_key("CITRATE_BROWSER_CHROMIUM"),
        "no chromium component installed"
    );
    assert!(env_map(&file_env_source(dir.clone(), None)()).contains_key("CITRATE_HERMES_BROWSER"));
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(components);
}

// HUP-S5.5: the component updater's open-web rule reaches the managed browser.
#[test]
fn an_expired_manifest_keeps_the_managed_browser_off_the_open_web() {
    let on = HermesWebSettings {
        browser_enabled: true,
        ..HermesWebSettings::default()
    };
    let managed = Path::new("/data/components/chromium/154/chrome");
    let env = env_map(&sidecar_env_managed(&on, Path::new("/d"), Some(managed), false, None));
    assert_eq!(env["CITRATE_BROWSER_OPEN_WEB"], "0");
    assert_eq!(env["CITRATE_BROWSER_CHROMIUM"], "/data/components/chromium/154/chrome");
    assert!(SIDECAR_ENV_KEYS.contains(&"CITRATE_BROWSER_OPEN_WEB"), "the manager passes it");
    // Current updates: nothing extra, the sidecar's default (open) applies.
    let env = env_map(&sidecar_env_managed(&on, Path::new("/d"), Some(managed), true, None));
    assert!(!env.contains_key("CITRATE_BROWSER_OPEN_WEB"));
    // The rule follows the managed Chromium only: a system Chrome (no managed one installed,
    // every machine while the component key slot is empty) is not affected.
    let env = env_map(&sidecar_env_managed(&on, Path::new("/d"), None, false, None));
    assert!(!env.contains_key("CITRATE_BROWSER_OPEN_WEB"));
    // A relative path is never passed, and neither is the rule without it.
    let env = env_map(&sidecar_env_managed(&on, Path::new("/d"), Some(Path::new("chrome")), false, None));
    assert!(!env.contains_key("CITRATE_BROWSER_OPEN_WEB"));
    // With the switch off, nothing at all.
    assert!(sidecar_env_managed(&HermesWebSettings::default(), Path::new("/d"), Some(managed), false, None).is_empty());
}

#[test]
fn the_status_says_when_the_managed_browser_is_kept_off_the_open_web() {
    let on = HermesWebSettings {
        browser_enabled: true,
        ..HermesWebSettings::default()
    };
    let off_web = |n: &String| n.contains("stays off the open web");
    let st = status_managed(on.clone(), None, Some(Path::new("/c/chrome")), false, None);
    assert!(st.notices.iter().any(off_web));
    let st = status_managed(on.clone(), None, Some(Path::new("/c/chrome")), true, None);
    assert!(!st.notices.iter().any(off_web));
    let st = status_managed(on, None, None, false, None);
    assert!(!st.notices.iter().any(off_web), "no managed Chromium: nothing is held back");
}

#[test]
fn the_installed_searxng_component_is_used_only_with_search_on_and_no_path_of_the_members() {
    let managed = Path::new("/data/components/searxng/2026.10.4-ab/bin/searxng-run");
    // Search off (the default): an installed component changes nothing.
    assert!(sidecar_env_managed(&HermesWebSettings::default(), Path::new("/d"), None, true, Some(managed)).is_empty());
    let on = HermesWebSettings {
        search_enabled: true,
        ..HermesWebSettings::default()
    };
    let env = env_map(&sidecar_env_managed(&on, Path::new("/d"), None, true, Some(managed)));
    assert_eq!(env["CITRATE_HERMES_SEARCH"], "1");
    assert_eq!(env["CITRATE_HERMES_SEARXNG"], managed.to_string_lossy());
    // Not installed: search is on but reports "not installed", as before.
    let env = env_map(&sidecar_env_managed(&on, Path::new("/d"), None, true, None));
    assert!(!env.contains_key("CITRATE_HERMES_SEARXNG"));
    // A relative path is never passed.
    let env = env_map(&sidecar_env_managed(&on, Path::new("/d"), None, true, Some(Path::new("bin/searxng-run"))));
    assert!(!env.contains_key("CITRATE_HERMES_SEARXNG"));
    // The member's own SearXNG wins over the component.
    let mine = HermesWebSettings {
        search_enabled: true,
        searxng_path: Some("/opt/searxng".into()),
        ..HermesWebSettings::default()
    };
    let env = env_map(&sidecar_env_managed(&mine, Path::new("/d"), None, true, Some(managed)));
    assert_eq!(env["CITRATE_HERMES_SEARXNG"], "/opt/searxng");
}

#[test]
fn the_status_says_when_search_uses_the_installed_component() {
    let on = HermesWebSettings {
        search_enabled: true,
        ..HermesWebSettings::default()
    };
    let st = status_managed(on.clone(), None, None, true, None);
    assert!(!st.searxng_found);
    assert!(st.notices.iter().any(|n| n.contains("no SearXNG program was found")));
    let st = status_managed(on, None, None, true, Some(Path::new("/c/searxng-run")));
    assert!(st.searxng_found);
    assert!(st
        .notices
        .iter()
        .any(|n| n.contains("installed private search component")));
    assert!(st.notices.iter().all(|n| !n.contains("no SearXNG program was found")));
}

#[test]
fn the_file_env_source_finds_the_installed_searxng_component() {
    use citrate_components::install::{InstalledComponent, InstalledVersion, StoreState};
    let Some(platform) = citrate_components::platform::Platform::current() else {
        return;
    };
    let dir = tmp("file-src-searxng");
    let components = tmp("file-src-searxng-components");
    let exe = components
        .join(crate::components::SEARXNG_COMPONENT)
        .join("2026.10.4-ab")
        .join("bin")
        .join("searxng-run");
    std::fs::create_dir_all(exe.parent().expect("parent")).expect("dirs");
    std::fs::write(&exe, b"#!/bin/sh\n").expect("exe");
    let mut st = StoreState::default();
    st.components.insert(
        crate::components::SEARXNG_COMPONENT.to_string(),
        InstalledComponent {
            current: InstalledVersion {
                version: "2026.10.4+d48c4b555".into(),
                sha256: "ab".repeat(32),
                dir: "2026.10.4-ab".into(),
                platform,
                installed_at: 1_790_000_000,
                manifest_sequence: 1,
            },
            previous: None,
        },
    );
    std::fs::write(
        components.join("state.json"),
        serde_json::to_vec_pretty(&st).expect("json"),
    )
    .expect("state");
    let src = file_env_source(dir.clone(), Some(components.clone()));
    assert!(src().is_empty(), "search off: nothing is passed");
    save(
        &dir,
        &HermesWebSettings {
            search_enabled: true,
            ..HermesWebSettings::default()
        },
    )
    .expect("save");
    let env = env_map(&src());
    // The bundle's searxng entrypoint is the same on every platform (bin/searxng-run).
    assert_eq!(env["CITRATE_HERMES_SEARXNG"], exe.to_string_lossy());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(components);
}
