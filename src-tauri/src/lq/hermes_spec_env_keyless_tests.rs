// L1-WP01 CH-5 (characterization, citrate-core a826e441). Included into `hermes::tests`.
//
// Pins today's Hermes sidecar environment rule (`HermesManager::build_spec`, hermes.rs): only the
// allowlisted env-source keys cross, nothing shaped like a vendor key crosses in any of them, an
// env source cannot override the control bind or the bearer-file path, and the vendor SDK key
// variables are stripped from what the child inherits. An L1 PR that changes this names CH-5.

mod lq_ch5_hermes_spec_env_keyless {
    use super::*;
    use std::collections::BTreeMap;

    /// The vendor key prefixes `telemetry::has_vendor_key_shape` knows at a826e441.
    const VENDOR_PREFIXES: &[&str] = &[
        "sk-", "sk_live_", "sk_test_", "cgk_", "xai-", "AIza", "apk_", "cog_", "LLM|", "LLM_",
    ];

    /// Every key an env source may hand the sidecar today (the four allowlists build_spec reads).
    fn allowlisted_keys() -> Vec<&'static str> {
        let mut keys: Vec<&'static str> = Vec::new();
        keys.extend_from_slice(crate::hermes_web::SIDECAR_ENV_KEYS);
        keys.extend_from_slice(crate::forge_toolchain::SIDECAR_ENV_KEYS);
        keys.extend_from_slice(crate::fl_trajectories::SIDECAR_ENV_KEYS);
        keys.extend_from_slice(crate::hermes_terminal::SIDECAR_ENV_KEYS);
        keys
    }

    fn shaped(prefix: &str) -> String {
        format!("{prefix}{}", "Q7".repeat(16))
    }

    fn env_with(tag: &str, offered: Vec<(String, String)>) -> Vec<(String, String)> {
        let (mgr, _dir) = stub_manager(tag);
        let src: crate::hermes_web::EnvSource = std::sync::Arc::new(move || offered.clone());
        mgr.with_env_source(src).spec_env_for_test()
    }

    #[test]
    fn lq_ch5_every_vendor_prefix_is_a_key_shape() {
        for p in VENDOR_PREFIXES {
            assert!(
                crate::telemetry::has_vendor_key_shape(&shaped(p)),
                "{p} is no longer treated as a vendor key prefix"
            );
            assert!(
                crate::hermes::cloud::ensure_keyless("environment", &shaped(p)).is_err(),
                "{p}"
            );
        }
        // A short run after a prefix, or a prefix inside a word, is not a key.
        assert!(!crate::telemetry::has_vendor_key_shape("sk-short"));
        assert!(!crate::telemetry::has_vendor_key_shape("task-Q7Q7Q7Q7Q7Q7Q7Q7"));
    }

    #[test]
    fn lq_ch5_allowlisted_keys_with_plain_values_cross() {
        let keys = allowlisted_keys();
        assert!(keys.len() >= 4, "the allowlists are not empty: {keys:?}");
        let offered: Vec<(String, String)> = keys
            .iter()
            .map(|k| (k.to_string(), "1".to_string()))
            .collect();
        let env: BTreeMap<String, String> = env_with("ch5-plain", offered).into_iter().collect();
        for k in &keys {
            assert_eq!(env.get(*k).map(String::as_str), Some("1"), "{k} was dropped");
        }
    }

    #[test]
    fn lq_ch5_no_allowlisted_key_carries_a_key_shaped_value_for_any_prefix() {
        let keys = allowlisted_keys();
        for (i, p) in VENDOR_PREFIXES.iter().enumerate() {
            let offered: Vec<(String, String)> = keys
                .iter()
                .map(|k| (k.to_string(), shaped(p)))
                .collect();
            let env = env_with(&format!("ch5-shaped-{i}"), offered);
            for k in &keys {
                assert!(
                    !env.iter().any(|(name, _)| name == k),
                    "{k} crossed with a {p} key-shaped value"
                );
            }
            for (k, v) in &env {
                assert!(
                    crate::hermes::cloud::ensure_keyless("environment", v).is_ok(),
                    "{k} carries a key shape"
                );
            }
        }
    }

    #[test]
    fn lq_ch5_an_env_source_cannot_add_or_override_other_keys() {
        let offered = vec![
            (
                crate::hermes::HERMES_ADDR_ENV.to_string(),
                "0.0.0.0:1".to_string(),
            ),
            (
                crate::hermes::HERMES_TOKEN_FILE_ENV.to_string(),
                "/tmp/elsewhere".to_string(),
            ),
            (
                crate::hermes::HERMES_CAPSULES_ENV.to_string(),
                "/tmp/caps".to_string(),
            ),
            ("OPENAI_API_KEY".to_string(), "plain".to_string()),
            ("PATH".to_string(), "/tmp/bin".to_string()),
            ("CITRATE_HERMES_UNLISTED".to_string(), "1".to_string()),
        ];
        let env = env_with("ch5-override", offered);
        let addr: Vec<&str> = env
            .iter()
            .filter(|(k, _)| k == crate::hermes::HERMES_ADDR_ENV)
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(addr, [crate::hermes::HERMES_CONTROL_ADDR]);
        let token_files = env
            .iter()
            .filter(|(k, _)| k == crate::hermes::HERMES_TOKEN_FILE_ENV)
            .count();
        assert_eq!(token_files, 1);
        assert!(!env.iter().any(|(_, v)| v == "/tmp/elsewhere"));
        for k in [
            crate::hermes::HERMES_CAPSULES_ENV,
            "OPENAI_API_KEY",
            "PATH",
            "CITRATE_HERMES_UNLISTED",
        ] {
            assert!(!env.iter().any(|(name, _)| name == k), "{k} crossed");
        }
    }

    #[test]
    fn lq_ch5_vendor_sdk_key_variables_are_never_inherited() {
        for name in crate::hermes::cloud::VENDOR_KEY_ENV {
            for spelled in [name.to_string(), name.to_ascii_lowercase()] {
                let out = crate::hermes::cloud::inherited_key_vars(
                    [(spelled.clone(), "x".to_string())].into_iter(),
                );
                assert_eq!(out, [spelled.clone()], "{spelled} would be inherited");
            }
        }
        for p in VENDOR_PREFIXES {
            let out = crate::hermes::cloud::inherited_key_vars(
                [("SOME_SETTING".to_string(), shaped(p))].into_iter(),
            );
            assert_eq!(out, ["SOME_SETTING".to_string()], "{p}");
        }
        let kept = crate::hermes::cloud::inherited_key_vars(
            [
                ("HOME".to_string(), "/home/m".to_string()),
                ("CITRATE_HERMES_SEARCH".to_string(), "1".to_string()),
            ]
            .into_iter(),
        );
        assert!(kept.is_empty(), "{kept:?}");
    }
}
