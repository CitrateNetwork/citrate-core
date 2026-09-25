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

/// PBA-L7b-014: `telemetry_send` re-scrubs in Rust — a bundle the webview tampered with (PII
/// re-inserted after review) is scrubbed again, and a non-bundle payload is refused outright.
#[test]
fn pba_l7b_014_send_path_rescrubs_and_rejects_foreign_json() {
    // Assembled at runtime so the fixture is not a secret-scanner hit (it is a fake token).
    let fake_tok = ["sk", "_live_", "abcdefghijklmnop"].concat();
    let tampered = serde_json::json!({
        "reportId": "rpt_0123456789abcdef01234567",
        "appVersion": "0.2.9",
        "os": "macos",
        "crashTail": "panic at /Users/alice/x.rs from alice@example.com key 0x4a86659BDab24dc444C72fbbaD4cd83491820E40",
        "nodeLogTail": "",
        "uiErrors": [format!("Bearer {fake_tok}")]
    })
    .to_string();
    let out = rescrub_bundle_json(&tampered, "/Users/alice").expect("a bundle-shaped payload is accepted");
    assert!(!out.contains("/Users/alice"), "{out}");
    assert!(!out.contains("alice@example.com"), "{out}");
    assert!(!out.contains("4a86659BDab24dc444C72fbbaD4cd83491820E40"), "{out}");
    assert!(!out.contains(&fake_tok), "{out}");
    // Anything that is not exactly the reviewed bundle shape is refused (no verbatim egress).
    for bad in [
        r#"{"anything":"else"}"#.to_string(),
        "not json".to_string(),
        serde_json::json!({"reportId":"rpt_00","appVersion":"","os":"","crashTail":"","nodeLogTail":"","uiErrors":[],"extra":"smuggled"}).to_string(),
        serde_json::json!({"reportId":"member-wallet-0xabc","appVersion":"","os":"","crashTail":"","nodeLogTail":"","uiErrors":[]}).to_string(),
        "x".repeat(300 * 1024),
    ] {
        assert!(rescrub_bundle_json(&bad, "/Users/alice").is_err(), "must refuse: {}", &bad[..bad.len().min(80)]);
    }
    // An honest, already-scrubbed bundle round-trips unchanged (scrub is idempotent).
    let honest = build_bundle("rpt_ab".into(), "0.2.9", "macos", "ok", "", &[], "/Users/alice");
    let j = serde_json::to_string(&honest).unwrap();
    assert_eq!(rescrub_bundle_json(&j, "/Users/alice").unwrap(), j);
}

/// PBA-L7b-014 wiring: the one egress command re-scrubs before it posts, and posts only the
/// re-scrubbed value (never the raw webview string).
#[test]
fn pba_l7b_014_telemetry_send_posts_only_the_rescrubbed_bundle() {
    let src = include_str!("telemetry.rs");
    let start = src.find("pub async fn telemetry_send(").expect("command exists");
    let body = &src[start..];
    let body = &body[..body.find("\n}\n").unwrap_or(body.len())];
    let rescrub = body.find("rescrub_bundle_json(&bundle_json").expect("send must re-scrub");
    let post = body.find(".send(&clean)").expect("send must post the re-scrubbed value");
    assert!(rescrub < post);
    assert!(!body.contains(".send(&bundle_json)"), "never post the raw webview JSON");
}

/// Mutation hardening (cargo-mutants on rescrub_bundle_json): exact size bound and a strict
/// `rpt_<1..=64 hex>` report id.
#[test]
fn pba_l7b_014_rescrub_bounds_are_exact() {
    let make = |id: &str, pad: usize| {
        serde_json::json!({
            "reportId": id, "appVersion": "0.2.9", "os": "macos",
            "crashTail": "x".repeat(pad), "nodeLogTail": "", "uiErrors": []
        })
        .to_string()
    };
    let base = make("rpt_ab", 0).len();
    let exact = make("rpt_ab", MAX_BUNDLE_BYTES - base);
    assert_eq!(exact.len(), MAX_BUNDLE_BYTES);
    assert!(rescrub_bundle_json(&exact, "/Users/a").is_ok(), "exactly at the cap is accepted");
    let over = make("rpt_ab", MAX_BUNDLE_BYTES - base + 1);
    assert!(rescrub_bundle_json(&over, "/Users/a").is_err(), "one byte over is refused");
    let big = make("rpt_ab", MAX_BUNDLE_BYTES);
    assert!(rescrub_bundle_json(&big, "/Users/a").is_err());
    for bad_id in ["rpt_", "rpt_zz", "rpt_0g", "xpt_ab"] {
        assert!(rescrub_bundle_json(&make(bad_id, 0), "/Users/a").is_err(), "{bad_id}");
    }
    let long = format!("rpt_{}", "a".repeat(65));
    assert!(rescrub_bundle_json(&make(&long, 0), "/Users/a").is_err());
    let max = format!("rpt_{}", "a".repeat(64));
    assert!(rescrub_bundle_json(&make(&max, 0), "/Users/a").is_ok());
}
