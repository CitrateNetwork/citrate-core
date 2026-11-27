// 0.5.4 cleanup tripwire: commands that nothing in the webview called, or seam stubs that only
// returned "unavailable", are not registered or allowed again. Each one widened the IPC surface
// for nothing; `custody_put` was an invoke-path secret write.

const REMOVED: [&str; 10] = [
    "custody_put",
    "custody_keyring_status",
    "ipfs_status",
    "ipfs_stop",
    "model_serve_status",
    "node_proposer_identity",
    "chat_backend",
    "membership_entitlement",
    "commissary_catalog",
    "comms_connections",
];

#[test]
fn removed_commands_are_not_registered_or_allowed() {
    let lib = crate::commands::registry_source();
    let acl = crate::commands::acl_source();
    for c in REMOVED {
        assert!(
            !lib.contains(&format!("::{c},")),
            "{c} is back in generate_handler!"
        );
        assert!(
            !acl.contains(&format!("\"{c}\"")),
            "{c} is back in main-window.toml"
        );
    }
}

#[test]
fn the_seam_module_is_gone() {
    let lib = crate::commands::registry_source();
    assert!(
        !lib.contains("mod seam;"),
        "the seam stub module is not rebuilt"
    );
    assert!(!lib.contains("seam::"));
}
