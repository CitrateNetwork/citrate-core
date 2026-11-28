// 0.6.0 L6-WP05: OAuth client v2 tests (lanes/L6 WP05 RED list, plus the lead's additions:
// long tokens, OAuth state, declined consent, refresh, revoke, and no credential in an IPC
// payload or a log line). The provider and the relay are test seams (Rule 1: a TEST seam, never
// a live socket presented as real); the vault runs headless over an in-memory keyring; the
// listener tests use a real loopback socket on an OS-assigned port.
//
// L6-WP05 stack, part 1: the compiled provider registry and the relay client only. The connect
// flow, refresh and revoke tests arrive with the code that uses them.

use super::relay::{self, RelayOp};
use super::*;

// ---------------------------------------------------------------------------
// 05a: registry, dev file, relay exchange
// ---------------------------------------------------------------------------

#[test]
fn release_build_ignores_dev_file() {
    // The registry and the relay client never read a file or the dev-file variable.
    for (name, file) in [
        ("providers.rs", include_str!("providers.rs")),
        ("relay.rs", include_str!("relay.rs")),
    ] {
        assert!(!file.contains("std::fs"), "{name} reads a file");
        assert!(
            !file.contains("CITRATE_OAUTH_DEV_FILE"),
            "{name} names the dev file"
        );
        assert!(
            !file.contains("client_secret"),
            "{name} names a client secret"
        );
    }
    // provider_setup answers from the compiled table only.
    let gh = provider_setup("github").expect("github row");
    assert_eq!(gh.mode, ClientMode::Relay);
    assert_eq!(gh.client_id, Some("Ov23liUgI4U92ZzDXwnt"));
    assert!(gh.configured);
}

#[test]
fn not_configured_reason_names_the_missing_registration() {
    for (id, provider, name) in [
        ("notion", "notion", "Notion"),
        ("gdrive", "google", "Google"),
        ("gsheets", "google", "Google"),
        ("gcal", "google", "Google"),
        ("hf", "huggingface", "Hugging Face"),
    ] {
        let s = provider_setup(id).expect(id);
        assert_eq!(s.provider, provider);
        assert!(!s.configured, "{id}");
        assert_eq!(s.client_id, None, "{id}");
        let reason = s.reason.expect("a reason");
        assert!(reason.contains(name), "{id}: {reason}");
        assert!(reason.contains("CONNECTIONS_RUNBOOK"), "{id}: {reason}");
        assert!(!reason.contains('\u{2014}'), "no em-dash: {reason}");
    }
    assert!(provider_setup("github").and_then(|s| s.reason).is_none());
    assert!(provider_setup("slack").is_none());
}

#[test]
fn relay_paths_are_the_frozen_endpoints_on_the_pinned_issuer() {
    let issuer = crate::oidc::AuthorityConfig::production().issuer;
    for (op, seg) in [
        (RelayOp::Token, "token"),
        (RelayOp::Refresh, "refresh"),
        (RelayOp::Revoke, "revoke"),
    ] {
        assert_eq!(
            format!("{issuer}{}", relay::relay_path("github", op)),
            format!("https://auth.citrate.ai/oauth/relay/v1/github/{seg}")
        );
    }
}
