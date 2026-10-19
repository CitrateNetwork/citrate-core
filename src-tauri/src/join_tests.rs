// GROW-S1 task 10 — the v2 invite-link command surface. Included into `join::tests`.
//
// The crypto, signature and HTTP behaviour is tested in the kit (`kit/src/join_tests.rs`, incl. the
// loopback e2e). These tests pin what this crate adds: registration + ACL, the exact field set that
// crosses `invoke`, and that a rebuilt relay link feeds the existing redeem parser unchanged.

use super::*;
use citrate_core_kit::join::JoinKind;
use std::collections::BTreeSet;

const COMMANDS: &[&str] = &[
    "join_mint",
    "join_attach",
    "join_resolve",
    "join_redeem",
    "join_revoke",
    "join_list",
];

fn keys(v: &serde_json::Value) -> BTreeSet<String> {
    v.as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn join_commands_are_registered_and_allowed_for_the_main_window() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in COMMANDS {
        assert!(lib.contains(&format!("join::{cmd},")), "{cmd} in lib.rs");
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
    }
    assert!(lib.contains("app.manage(join::build_join_state());"));
}

#[test]
fn only_signed_display_data_and_the_final_share_url_cross_invoke() {
    let mint = MintView {
        code: "Ab3dE7gh".into(),
        kind: JoinKind::Direct,
        status: "valid".into(),
        exp: "2100-01-01T00:00:00.000Z".into(),
        share_url: Some("https://citrate.ai/join/Ab3dE7gh#AAAAAAAAAAAAAAAAAAAAAA".into()),
        attach_error: None,
    };
    let v = serde_json::to_value(&mint).expect("json");
    assert_eq!(
        keys(&v),
        set(&["code", "kind", "status", "exp", "shareUrl", "attachError"])
    );
    let resolved = ResolvedView {
        code: "Ab3dE7gh".into(),
        kind: JoinKind::Direct,
        cluster_name: Some("Garden club".into()),
        goal: Some("storage".into()),
        inviter: None,
        inviter_verified: false,
        exp: "2100-01-01T00:00:00.000Z".into(),
        actionable: true,
        key_missing: false,
    };
    let v = serde_json::to_value(&resolved).expect("json");
    assert_eq!(
        keys(&v),
        set(&[
            "code",
            "kind",
            "clusterName",
            "goal",
            "inviter",
            "inviterVerified",
            "exp",
            "actionable",
            "keyMissing",
        ])
    );
    let row = JoinCodeEntry {
        code: "Ab3dE7gh".into(),
        kind: JoinKind::Referral,
        status: "valid".into(),
        exp: "2100-01-01T00:00:00.000Z".into(),
        resolve_count: 3,
    };
    let v = serde_json::to_value(&row).expect("json");
    assert_eq!(
        keys(&v),
        set(&["code", "kind", "status", "exp", "resolveCount"])
    );
}

#[test]
fn a_rebuilt_relay_link_feeds_the_existing_redeem_parser() {
    let link =
        "citrate://invite?g=grp_0a1b&t=Zt9QxV2mK7pL0aRr4sYuWbNcEdFg&k=02a1b2c3&n=47617264656e";
    let p = InvitePayload::from_invite_link(link).expect("parse");
    let rebuilt = p.to_invite_link();
    assert_eq!(rebuilt.as_str(), link);
    for (k, want) in [
        ("g", "grp_0a1b"),
        ("t", "Zt9QxV2mK7pL0aRr4sYuWbNcEdFg"),
        ("k", "02a1b2c3"),
        ("n", "47617264656e"),
    ] {
        assert_eq!(
            crate::invites::link_param(&rebuilt, k).as_deref(),
            Some(want)
        );
    }
}

#[test]
fn the_command_layer_logs_nothing_itself() {
    // All join logging goes through the kit's redacting `log_line`; this layer must add none.
    let src = include_str!("join.rs");
    let prod = src.split("#[cfg(test)]").next().expect("prod part");
    assert!(!prod.contains("println!"));
    assert!(!prod.contains("dbg!"));
    assert!(!prod.contains("log::"));
}

#[test]
fn production_state_uses_the_redirect_free_client_and_redeem_keeps_on_failure() {
    let src = include_str!("join.rs");
    let prod = src.split("#[cfg(test)]").next().expect("prod part");
    assert!(prod.contains("Box::new(JoinHttp)"));
    assert!(!prod.contains("UreqClient"));
    // join_redeem goes through redeem_opened (kept on failure), never take_opened (dropped first).
    assert!(prod.contains("redeem_opened(&code"));
    assert!(!prod.contains("take_opened("));
}
