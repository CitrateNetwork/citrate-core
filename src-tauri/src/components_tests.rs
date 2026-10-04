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

#[test]
fn store_writes_are_serialized_and_a_second_one_is_refused_while_one_runs() {
    // An update runs `recover()` (which clears staging) and rewrites state.json; a second
    // update or a rollback running at the same time would delete the first one's staging tree
    // or lose its state write. Only one store writer at a time.
    let first = with_store_lock(|| {
        let second = with_store_lock(|| Ok("ran".to_string()));
        assert!(second.unwrap_err().contains("already running"));
        Ok("first".to_string())
    });
    assert_eq!(first.as_deref(), Ok("first"));
    assert_eq!(with_store_lock(|| Ok("again".to_string())).as_deref(), Ok("again"));
}

// HUP-S5.1: the managed Chromium is found only when the signed `chromium` component is installed
// and its executable for this platform exists; otherwise "not installed" (None).
#[test]
fn the_managed_chromium_is_found_only_when_installed() {
    use citrate_components::install::{InstalledComponent, InstalledVersion, StoreState};
    let root = tmp("chromium");
    assert_eq!(managed_chromium(&root), None, "no store");
    assert!(!root.exists(), "looking must not create the store");
    let Some(platform) = Platform::current() else {
        return;
    };
    std::fs::create_dir_all(&root).expect("root");
    let mut st = StoreState::default();
    let write = |st: &StoreState| {
        std::fs::write(
            root.join("state.json"),
            serde_json::to_vec_pretty(st).expect("json"),
        )
        .expect("state")
    };
    write(&st);
    assert_eq!(managed_chromium(&root), None, "nothing installed");
    st.components.insert(
        CHROMIUM_COMPONENT.to_string(),
        InstalledComponent {
            current: InstalledVersion {
                version: "154.0.8037.92".into(),
                sha256: "ab".repeat(32),
                dir: "154.0.8037.92-ab".into(),
                platform,
                installed_at: 1_790_000_000,
                manifest_sequence: 1,
            },
            previous: None,
        },
    );
    write(&st);
    assert_eq!(
        managed_chromium(&root),
        None,
        "recorded but the executable is missing"
    );
    let bundle = Bundle::parse(BUNDLE_JSON).expect("bundle");
    let tool = bundle
        .tools
        .iter()
        .find(|t| t.name == CHROMIUM_COMPONENT)
        .expect("chromium is in the bundle");
    let ep = tool
        .artifacts
        .get(platform.as_str())
        .and_then(|a| a.entrypoints.clone())
        .unwrap_or_else(|| tool.entrypoints.clone());
    let exe = root
        .join(CHROMIUM_COMPONENT)
        .join("154.0.8037.92-ab")
        .join(&ep[0]);
    std::fs::create_dir_all(exe.parent().expect("parent")).expect("dirs");
    std::fs::write(&exe, b"#!/bin/sh\n").expect("exe");
    assert_eq!(managed_chromium(&root), Some(exe));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_bundle_lists_the_browser_and_search_components_honestly() {
    let st = components_status_sync(&tmp("web"), 1_790_000_000).expect("status");
    let chromium = st
        .bundle
        .iter()
        .find(|t| t.name == "chromium")
        .expect("chromium");
    assert_eq!(chromium.version, "154.0.8037.92");
    assert_eq!(chromium.measured_platforms, vec!["macos-arm64".to_string()]);
    let searxng = st.bundle.iter().find(|t| t.name == "searxng").expect("searxng");
    assert_eq!(searxng.license, "AGPL-3.0-or-later");
    assert!(searxng.measured_platforms.is_empty(), "packed by the release step");
}
