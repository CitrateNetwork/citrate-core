// #613 review (P1): the secret shapes a sidecar log line or a diagnostics bundle must not carry.
// Every planted value is assembled at run time from parts, so no scanner reads this file as
// holding a credential; none of them is a real key, token or password.
use super::*;

fn cat(parts: &[&str]) -> String {
    parts.concat()
}

fn corpus() -> Vec<(&'static str, String, String)> {
    test_corpus()
}

#[test]
fn every_corpus_shape_is_found_and_redacted_and_nothing_survives() {
    for (shape, line, secret) in corpus() {
        assert!(first_shape(&line).is_some(), "{shape}: not detected in {line:?}");
        let out = redact(&line);
        assert!(!out.contains(&secret), "{shape}: {secret:?} survived in {out:?}");
        assert_eq!(first_shape(&out), None, "{shape}: a shape is still found in {out:?}");
        assert!(out.contains(REDACTED) || out.contains("<hex64 "), "{shape}: {out:?}");
    }
}

/// The six lines the review ran, as they were reported.
#[test]
fn the_reviewers_lines_lose_every_token() {
    let opaque = cat(&["opaqueSession", "Token1234567890"]);
    let out = redact(&format!("Authorization: Bearer {opaque}"));
    assert!(!out.contains(&opaque), "{out}");
    let jwt = cat(&["eyJhbGciOiJSUzI1NiJ9", ".", "eyJzdWIiOiJ4In0", ".", "c2lnbmF0dXJlc2lnbmF0dXJl"]);
    assert!(!redact(&format!("{{\"id_token\":\"{jwt}\"}}")).contains("eyJ"));
    assert!(!redact(&format!("GET /mcp?token={jwt}")).contains("eyJ"));
    let pw = cat(&["hunter2", "hunter2"]);
    let api = cat(&["3f9a8b7c6d5e", "4f3a2b1c0d9e8f7a6b5c"]);
    let out = redact(&format!("password={pw} api_key: {api}"));
    assert!(!out.contains(&pw) && !out.contains(&api), "{out}");
    assert_eq!(redact(&format!("password={pw} api_key: {api}")), "password=<redacted> api_key: <redacted>");
}

#[test]
fn ordinary_log_lines_are_unchanged() {
    for line in [
        "citrate_network::sync: Validated and imported 32/32 blocks (height 1540-1571)",
        "n_tokens = 512, max_tokens: 2048",
        "listening on 127.0.0.1:18090",
        "version 0.5.3 (rev 7a7a7a7)",
        "peer 12D3KooWQ1x connected from 10.0.0.5:30303",
        "contract at 0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed",
        "basic setup done",
        "proceed with the next seed_nodes refresh",
        "tokens processed: 12",
    ] {
        assert_eq!(redact(line), line, "{line}");
        assert_eq!(first_shape(line), None, "{line}");
    }
}

#[test]
fn a_64_hex_value_becomes_a_short_stable_tag_so_equal_hashes_still_line_up() {
    let a = format!("block 0x{} imported", "ab".repeat(32));
    let b = format!("parent {} ok", "AB".repeat(32));
    let ra = redact(&a);
    let rb = redact(&b);
    let tag = |s: &str| s.split("<hex64 ").nth(1).and_then(|t| t.split('>').next()).map(str::to_string);
    assert_eq!(tag(&ra), tag(&rb), "the same value gets the same tag, whatever its case or prefix");
    assert_eq!(tag(&ra).map(|t| t.len()), Some(6));
    assert!(!ra.contains(&"ab".repeat(32)));
    assert_ne!(tag(&ra), tag(&redact(&"cd".repeat(32))));
}

#[test]
fn redaction_is_idempotent_and_keeps_non_ascii_text_intact() {
    for (_, line, _) in corpus() {
        let once = redact(&line);
        assert_eq!(redact(&once), once);
    }
    let pw = cat(&["hunter2", "hunter2"]);
    let line = format!("état: café password={pw} » fin");
    assert_eq!(redact(&line), "état: café password=<redacted> » fin");
}
