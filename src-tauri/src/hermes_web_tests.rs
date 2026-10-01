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
        search_enabled: true,
        searxng_path: Some("/a".into()),
        reader: ReaderChoice::Jina,
        jina_key_file: Some("/b".into()),
        jev_enabled: true,
        jev_key_file: Some("/c".into()),
        jev_origins: vec!["https://x.example".into()],
        jev_non_web: true,
    };
    for (k, _) in sidecar_env(&s, Path::new("/d")) {
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
