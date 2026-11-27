// L1-WP01 CH-11 (characterization, citrate-core a826e441). Included into `agent_grants::tests`.
//
// Pins today's folder-grant rules (agent_grants.rs): core's own data folder, and anything inside
// it, can never be granted (checked on the resolved path, by path component); a document that does
// not load grants nothing, refuses every change and is never written over. The store is built the
// way `GrantStore::for_app` builds it (`<app data>/agent`, protecting all of `<app data>`). WP21
// (default workspace grant) must keep these and names CH-11 if it adds a stricter rule.

mod lq_ch11_grants_fail_closed {
    use super::*;

    fn app_store(fx: &Fx) -> GrantStore {
        let data = fx.base.join("appdata");
        std::fs::create_dir_all(data.join("agent")).expect("agent dir");
        GrantStore::new(data.join("agent"), fx.home()).protecting(data)
    }

    fn own_folder_refusal(r: Result<Vec<String>, GrantsError>) -> bool {
        matches!(r, Err(GrantsError::Invalid(ref m)) if m.contains("own data folder"))
    }

    #[test]
    fn lq_ch11_core_data_and_everything_inside_it_is_refused() {
        let fx = Fx::new();
        let store = app_store(&fx);
        let data = fx.base.join("appdata");
        for d in ["models", "hermes/workspace", "agent/sub"] {
            std::fs::create_dir_all(data.join(d)).expect("mkdir");
        }
        for target in [
            data.clone(),
            data.join("agent"),
            data.join("agent/sub"),
            data.join("models"),
            data.join("hermes/workspace"),
            data.join("models/../hermes"),
        ] {
            for (read, write) in [(true, false), (false, true), (true, true)] {
                let r = store.add_folder(&target, read, write, T0);
                assert!(
                    own_folder_refusal(r.clone()),
                    "{} ({read}, {write}) must be refused as core's own folder: {r:?}",
                    target.display()
                );
            }
        }
        assert!(ok(store.load()).grants.is_empty(), "nothing was stored");
    }

    #[test]
    fn lq_ch11_a_sibling_that_only_shares_the_name_prefix_is_not_inside() {
        let fx = Fx::new();
        let store = app_store(&fx);
        let sibling = fx.base.join("appdata-other");
        std::fs::create_dir_all(&sibling).expect("mkdir");
        let ids = store.add_folder(&sibling, true, false, T0).expect("granted");
        assert_eq!(ids.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn lq_ch11_a_link_into_core_data_is_refused_by_its_target() {
        let fx = Fx::new();
        let store = app_store(&fx);
        std::fs::create_dir_all(fx.base.join("appdata/models")).expect("mkdir");
        let link = fx.home().join("work/models-link");
        std::os::unix::fs::symlink(fx.base.join("appdata/models"), &link).expect("symlink");
        assert!(own_folder_refusal(store.add_folder(&link, true, false, T0)));
        assert!(ok(store.load()).grants.is_empty());
    }

    #[test]
    fn lq_ch11_a_corrupted_document_grants_nothing_and_is_never_written_over() {
        let fx = Fx::new();
        let store = app_store(&fx);
        let file = fx.base.join("appdata/agent").join(GRANTS_FILE);
        for bad in [
            "{not json".to_string(),
            r#"{"version": 99, "grants": []}"#.to_string(),
            String::new(),
        ] {
            std::fs::write(&file, &bad).expect("write");
            assert!(matches!(store.load(), Loaded::Corrupted(_)), "{bad:?}");
            assert_eq!(store.document_for_agent(), GrantState::default(), "{bad:?}");
            let view = store.view(T0);
            assert_eq!(view.status, "corrupted", "{bad:?}");
            assert!(view.grants.is_empty());
            assert!(matches!(
                store.add_folder(&fx.app(), true, true, T0),
                Err(GrantsError::Corrupted(_))
            ));
            assert!(matches!(store.revoke("g-1", T0), Err(GrantsError::Corrupted(_))));
            assert!(matches!(
                store.full_access_prepare(T0),
                Err(GrantsError::Corrupted(_))
            ));
            assert_eq!(
                std::fs::read_to_string(&file).expect("read"),
                bad,
                "the stored document is left as it was"
            );
        }
    }

    #[test]
    fn lq_ch11_neither_read_nor_write_and_relative_paths_are_refused() {
        let fx = Fx::new();
        let store = app_store(&fx);
        assert!(matches!(
            store.add_folder(&fx.app(), false, false, T0),
            Err(GrantsError::Invalid(_))
        ));
        assert!(matches!(
            store.add_folder(std::path::Path::new("work/app"), true, false, T0),
            Err(GrantsError::Invalid(_))
        ));
        assert!(ok(store.load()).grants.is_empty());
    }
}
