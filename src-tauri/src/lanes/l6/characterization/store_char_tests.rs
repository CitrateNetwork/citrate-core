// 0.6.0 L6-WP02: the Connections & Marketplace lane's characterization suite, Store half
// (L6-CH-3). Included into `store_catalog::tests` after `store_catalog_tests.rs`, so it signs
// fixture catalogs with the module's throwaway test key. Select the whole lane suite with
// `cargo test -p citrate-core --lib l6_char`.
//
// Each test pins a guarantee held at citrate-core a826e441, before the Marketplace work packages
// (WP08 subregistry, WP10 install, WP20 the remaining kinds) change the Store. A later L6 PR that
// changes a pinned behavior names the test it supersedes and replaces the assertion only with a
// stricter one (ci/characterization.d/l6.json).
//
// Data source (Rule 7): the current code only. The production key slot is read as it ships;
// fixture catalogs are signed per test and served from memory.

mod l6_char {
    use super::*;

    /// A Store fetcher that fails the test if it is called.
    struct PanicFetcher;

    impl Fetcher for PanicFetcher {
        fn fetch(
            &self,
            url: &str,
            _max: u64,
            _sink: &mut dyn std::io::Write,
        ) -> Result<u64, ComponentError> {
            panic!("the Store fetched {url} with no catalog key")
        }
    }

    #[test]
    fn char_store_no_key_refuses_before_network() {
        // The slot as it ships is empty, and an empty slot refuses before any fetch or write.
        assert!(
            matches!(catalog_trust_root(), Err(CatalogError::KeyNotIssued)),
            "the production catalog key slot is no longer empty"
        );
        let root = tmp("l6-char-nokey");
        for key in [catalog_trust_root(), Err(CatalogError::KeyNotIssued)] {
            let v = catalog_view_sync(&root, &key, &PanicFetcher, 2, NOW);
            assert_eq!(v.state, "no_key");
            assert_eq!(
                v.message.as_deref(),
                Some("Store catalog not available yet: signing key not issued")
            );
            assert!(v.listings.is_empty());
            let err = stage_sync(&root, &key, &PanicFetcher, &root, 2, "daily-brief", NOW)
                .expect_err("no key, no stage");
            assert_eq!(err.code, "no_key");
        }
        assert!(!root.exists(), "nothing written");
    }

    #[test]
    fn char_store_first_party_only() {
        let key = TestKey::generate();
        let third_party = listing_json(
            "outside",
            "app",
            "public",
            "https://store.test/outside.dmg",
            b"app",
            "",
        )
        .replace("\"first_party\":true", "\"first_party\":false");
        assert!(third_party.contains("\"first_party\":false"), "{third_party}");
        let json = catalog_json(1, &[third_party]);
        let sig = key.sign(json.as_bytes());

        // The verifier refuses it even with a good signature.
        let root_key = key.root().expect("test key");
        match verify_catalog(json.as_bytes(), &sig, &root_key, None, NOW) {
            Err(CatalogError::Malformed(m)) => assert!(m.contains("first-party"), "{m}"),
            Err(e) => panic!("refused for another reason: {e}"),
            Ok(_) => panic!("a third-party listing was accepted"),
        }

        // Negative control: the same catalog with the listing first-party verifies.
        let first_party = listing_json(
            "outside",
            "app",
            "public",
            "https://store.test/outside.dmg",
            b"app",
            "",
        );
        let good = catalog_json(1, &[first_party]);
        assert!(
            verify_catalog(good.as_bytes(), &key.sign(good.as_bytes()), &root_key, None, NOW)
                .is_ok(),
            "the fixture itself is refused"
        );

        // And the app's catalog path shows nothing from it and keeps no copy.
        let fetcher = MemFetcher::default();
        fetcher.put(STORE_CATALOG_URL, json.as_bytes());
        fetcher.put(STORE_CATALOG_SIG_URL, sig.as_bytes());
        let root = tmp("l6-char-3p");
        let v = catalog_view_sync(&root, &key.root(), &fetcher, 2, NOW);
        assert_eq!(v.state, "invalid", "{v:?}");
        assert!(v.listings.is_empty());
        assert!(
            v.message
                .as_deref()
                .is_some_and(|m| m.contains("first-party")),
            "{v:?}"
        );
        assert!(!root.join(CATALOG_FILE).exists(), "a refused catalog was kept");
        let err = stage_sync(&root, &key.root(), &fetcher, &root, 2, "outside", NOW)
            .expect_err("nothing verified to stage from");
        assert_eq!(err.code, "fetch_failed");
        let _ = std::fs::remove_dir_all(&root);
    }
}
