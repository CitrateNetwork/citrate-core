// 0.6.0 L4-WP01: characterization of the social face seam used by group chat.
//
// Each test pins how `social.rs` behaves TODAY (citrate-core main a826e441) and passes on day 1.
// They are not red tests and not a statement of what is right: later L4 work packages change this
// on purpose, and may replace an assertion here only with a stricter one.
//
// The commands take a `tauri::AppHandle` on the Wry runtime, which a unit test cannot build (the
// mock runtime is a different type), so these read the shipped source of each blocking body, the
// same way `comms_tests.rs` checks a command's body. Included into `social::char_tests`.

const SOURCE: &str = include_str!("../../../social.rs");

/// The source of one `pub fn <name>(` in social.rs, up to its closing brace.
fn fn_body(name: &str) -> &'static str {
    let start = SOURCE
        .find(&format!("pub fn {name}("))
        .unwrap_or_else(|| panic!("{name} is a function in social.rs"));
    let len = SOURCE[start..].find("\n}\n").expect("closing brace");
    &SOURCE[start..start + len]
}

/// Where `needle` first appears in `body`, or a panic naming it.
fn at(body: &str, needle: &str) -> usize {
    body.find(needle)
        .unwrap_or_else(|| panic!("expected {needle:?} in:\n{body}"))
}

/// Ingest refuses a binding whose claimed address is not the relay sender, before it checks the
/// signature or stores anything. The sender is the address the comms daemon authenticated, so a
/// binding is accepted only when that address equals the binding's (wallet) address.
#[test]
fn char_ingest_refuses_sender_not_equal_binding_address() {
    let body = fn_body("social_ingest_binding_sync");
    let gate = at(
        body,
        "if sender.to_lowercase() != binding.address.to_lowercase() {\n        return Ok(false);\n    }",
    );
    let recover = at(body, "crate::wallet::recover_personal_hex(");
    let store = at(body, "save_foreign(&app, &foreign)?;");
    assert!(
        gate < recover,
        "the sender gate runs before the signature check"
    );
    assert!(
        recover < store,
        "the signature check runs before anything is stored"
    );
    // The second refusal: the signature must recover to the same claimed address.
    at(
        body,
        "if recovered.to_lowercase() != binding.address.to_lowercase() {\n        return Ok(false);\n    }",
    );
    // A refusal is `Ok(false)`, never an error, and only the end of the body says `Ok(true)`.
    assert_eq!(body.matches("return Ok(false);").count(), 2, "{body}");
    assert_eq!(body.matches("Ok(true)").count(), 1, "{body}");
    assert!(body.trim_end().ends_with("Ok(true)"), "{body}");
}

/// Resolve answers only for addresses that equal a binding's address: the member's own verified,
/// group-visible links (whose address is the wallet address the binding was signed with) and the
/// foreign faces ingest accepted. Matching is case-insensitive and exact; nothing maps a comms
/// address to a wallet address.
#[test]
fn char_resolve_matches_wallet_addresses_only() {
    let body = fn_body("social_resolve_sync");
    at(
        body,
        "addresses.iter().map(|a| a.to_lowercase()).collect();",
    );
    // Own links: verified only, group-visible only, keyed by the binding's address.
    let verified = at(body, "let b = l.binding.as_ref()?;");
    let visible = at(
        body,
        "if l.visibility != \"groups\" {\n                return None;",
    );
    let own = at(body, "if !want.contains(&b.address.to_lowercase()) {");
    assert!(verified < visible && visible < own, "{body}");
    // Foreign faces, keyed by the address ingest stored (the binding's address).
    at(body, "if want.contains(&f.address.to_lowercase()) {");
    assert_eq!(body.matches("want.contains(").count(), 2, "{body}");
    // No other lookup: no comms or roster mapping, no directory call.
    for absent in ["comms", "roster", "directory", "http"] {
        assert!(
            !body.contains(absent),
            "resolve mentions {absent:?}:\n{body}"
        );
    }
    // The binding's address is the wallet address it was signed with.
    let request = fn_body("social_verify_request_sync");
    at(
        request,
        "let info = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;",
    );
    at(request, "address: info.address,");
    at(
        fn_body("social_verify_approve_sync"),
        "address: pending.address,",
    );
}
