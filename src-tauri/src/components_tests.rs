// HUP-S5.5 core wiring: status is honest about the empty key slot, and update / rollback refuse
// before touching the network or the disk while the key is not configured.
use super::*;

fn tmp(tag: &str) -> std::path::PathBuf {
    let n: u64 = rand::random();
    std::env::temp_dir().join(format!("citrate-core-components-{tag}-{n:016x}"))
}

#[test]
fn status_reports_the_empty_key_slot_and_changes_nothing_on_disk() {
    let root = tmp("status");
    let st = components_status_sync(&root, 1_790_000_000).expect("status");
    assert!(!st.key_configured);
    assert!(st.key_fingerprint.is_none());
    assert!(st.key_note.contains("not configured"), "{}", st.key_note);
    assert_eq!(st.freshness, "never_checked");
    assert!(!st.browser_may_open_web);
    assert!(st.installed.is_empty());
    assert!(st.sla.pending_owner_signoff);
    assert!(!root.exists(), "reading status must not create the store");
}

#[test]
fn status_lists_the_toolchain_bundle_with_honest_per_platform_states() {
    let st = components_status_sync(&tmp("bundle"), 1_790_000_000).expect("status");
    let names: Vec<&str> = st.bundle.iter().map(|t| t.name.as_str()).collect();
    for want in ["solc", "foundry", "slither", "aderyn", "medusa", "node", "python"] {
        assert!(names.contains(&want), "{want} missing: {names:?}");
    }
    let slither = st.bundle.iter().find(|t| t.name == "slither").expect("slither");
    assert!(slither.measured_platforms.is_empty(), "slither is not built yet");
    let foundry = st.bundle.iter().find(|t| t.name == "foundry").expect("foundry");
    assert_eq!(foundry.measured_platforms, vec!["macos-arm64".to_string()]);
    assert_eq!(st.libraries.len(), 3);
}

#[test]
fn update_refuses_without_the_key_and_creates_nothing() {
    let root = tmp("update");
    let err = components_update_sync(&root, "foundry", 1_790_000_000).unwrap_err();
    assert!(err.contains("not configured"), "{err}");
    assert!(!root.exists(), "no store, no staging, no download");
}

#[test]
fn update_refuses_an_invalid_component_name_first() {
    let err = components_update_sync(&tmp("name"), "../foundry", 1_790_000_000).unwrap_err();
    assert!(err.contains("component name"), "{err}");
}

#[test]
fn rollback_with_nothing_installed_is_an_honest_error() {
    let root = tmp("rollback");
    let err = components_rollback_sync(&root, "foundry").unwrap_err();
    assert!(err.contains("no previous version"), "{err}");
    assert!(!root.exists());
}

#[test]
fn the_manifest_urls_are_https_and_separate_from_the_app_updater() {
    assert!(COMPONENT_MANIFEST_URL.starts_with("https://"));
    assert!(COMPONENT_MANIFEST_SIG_URL.starts_with(COMPONENT_MANIFEST_URL));
    assert!(!COMPONENT_MANIFEST_URL.contains("/updater/"), "component manifests live apart from the app updater feed");
}
