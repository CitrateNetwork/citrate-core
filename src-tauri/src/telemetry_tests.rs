// Telemetry WP-T.3 — scrub tests, against fixtures with PLANTED secrets. Pure, no network.
use super::*;

#[test]
fn scrub_replaces_home_with_tilde() {
    let out = scrub("reading /Users/alice/Library/Application Support/ai.citrate.core/x", "/Users/alice");
    assert!(out.contains("~/Library/Application Support/ai.citrate.core/x"), "got: {out}");
    assert!(!out.contains("/Users/alice"), "home path leaked: {out}");
}

#[test]
fn scrub_redacts_0x_addresses_and_hashes() {
    let addr = "0x2655d9fbbe599e75ff6e53790f99ebc9a20c93bf"; // 40 hex
    let hash = "0x98e0d72f1c3b4a5e6d7f8091a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f90123"; // 64 hex
    let out = scrub(&format!("from {addr} tx {hash} done"), "");
    assert!(!out.contains("2655d9fb"), "address leaked: {out}");
    assert!(!out.contains("98e0d72f"), "hash leaked: {out}");
    assert!(out.contains("0x<redacted>"), "expected redaction marker: {out}");
    // A short 0x value (not an address) is left alone.
    assert_eq!(scrub("gas 0x4331ed1f", ""), "gas 0x4331ed1f");
}

#[test]
fn scrub_redacts_emails() {
    let out = scrub("member mbell@protonmail.com hit an error", "");
    assert!(!out.contains("mbell@protonmail.com"), "email leaked: {out}");
    assert!(out.contains("<redacted>"), "got: {out}");
    // surrounding words are preserved
    assert!(out.contains("member") && out.contains("hit an error"), "over-scrubbed: {out}");
}

#[test]
fn scrub_redacts_bearer_and_secret_tokens() {
    let out = scrub("auth Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig and key sk-live_ABCDEF123456", "");
    assert!(!out.contains("eyJhbGciOiJIUzI1NiJ9"), "JWT leaked: {out}");
    assert!(!out.contains("sk-live_ABCDEF123456"), "secret key leaked: {out}");
    assert!(out.contains("<redacted>"));
}

#[test]
fn scrub_is_idempotent_and_keeps_benign_text() {
    let benign = "node started at height 12345, 4 peers, epoch 176";
    assert_eq!(scrub(benign, "/Users/x"), benign, "benign text must pass through unchanged");
    let once = scrub("0x2655d9fbbe599e75ff6e53790f99ebc9a20c93bf @ /Users/x", "/Users/x");
    assert_eq!(scrub(&once, "/Users/x"), once, "scrub is idempotent");
}

#[test]
fn build_bundle_scrubs_every_field_and_keeps_the_id() {
    let b = build_bundle(
        "rpt_deadbeef".into(),
        "0.2.7",
        "macos",
        "panic at ~/x: boom 0x2655d9fbbe599e75ff6e53790f99ebc9a20c93bf",
        "crash from mbell@protonmail.com",
        &["render error at /Users/alice/app".to_string()],
        "/Users/alice",
    );
    assert_eq!(b.report_id, "rpt_deadbeef");
    assert_eq!(b.app_version, "0.2.7");
    assert!(b.crash_tail.contains("0x<redacted>"), "crash not scrubbed: {}", b.crash_tail);
    assert!(!b.node_log_tail.contains("mbell@protonmail.com"), "email leaked in node tail");
    assert!(b.ui_errors[0].contains("~/app"), "home not scrubbed in ui error: {}", b.ui_errors[0]);
}
