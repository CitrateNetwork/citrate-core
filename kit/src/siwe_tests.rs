// HUP-S2.3 — hardened EIP-4361 (SIWE) parse + budget checks (ADR-2026-09-30 D2 #6-18).
//
// Red-first: each test names the D2 check it pins. A failing check never signs: in the budgeted
// path it falls through to an HIC-1 ceremony (see ceremony_budget_tests.rs).

use super::*;

// The canonical BIP44 test wallet (same vector as ceremony_tests / wallet_tests), EIP-55 form.
const ADDR: &str = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94";
const ORIGIN: &str = "https://app.example.org";
// 2026-10-01T12:00:00Z in epoch ms.
const NOW: u64 = 1_790_856_000_000;

fn base_fields() -> SiweFields {
    SiweFields {
        scheme: None,
        domain: "app.example.org".into(),
        address: ADDR.into(),
        statement: Some("Sign in to Example.".into()),
        uri: "https://app.example.org/login".into(),
        version: "1".into(),
        chain_id: "40204".into(),
        nonce: "abcdef0123456789XY".into(),
        issued_at: "2026-10-01T11:59:30Z".into(),
        expiration_time: Some("2026-10-01T12:10:00Z".into()),
        not_before: None,
        request_id: None,
        resources: vec![],
    }
}

fn ctx() -> SiweCheckContext<'static> {
    SiweCheckContext {
        attested_origin: ORIGIN,
        wallet_address: "0x9858effd232b4033e47d90003d41ec34ecaeda94",
        now_ms: NOW,
    }
}

fn text(f: &SiweFields) -> String {
    f.to_message()
}

#[test]
fn rfc3339_parses_to_epoch_ms() {
    assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_rfc3339_ms("2026-10-01T12:00:00Z"), Some(NOW as i64));
    assert_eq!(
        parse_rfc3339_ms("2026-10-01T12:00:00.250Z"),
        Some(NOW as i64 + 250)
    );
    assert_eq!(
        parse_rfc3339_ms("2026-10-01T14:00:00+02:00"),
        Some(NOW as i64)
    );
    assert_eq!(parse_rfc3339_ms("2026-10-01 12:00:00Z"), None);
    assert_eq!(parse_rfc3339_ms("2026-13-01T12:00:00Z"), None);
    assert_eq!(parse_rfc3339_ms("2026-02-30T12:00:00Z"), None);
    assert_eq!(parse_rfc3339_ms("2026-10-01T12:00:00"), None);
}

#[test]
fn eip55_checksum_is_validated() {
    assert!(is_eip55(ADDR));
    assert!(!is_eip55("0x9858effd232b4033e47d90003d41ec34ecaeda94"));
    assert!(!is_eip55("0x9858EFFD232B4033E47D90003D41EC34ECAEDA94"));
    assert!(!is_eip55("0x9858EfFD232B4033E47d90003D41EC34EcaEda9"));
}

#[test]
fn d2_6_a_well_formed_message_round_trips_byte_for_byte() {
    let f = base_fields();
    let t = text(&f);
    let parsed = parse_strict(&t).expect("parses");
    assert_eq!(parsed.to_message(), t);
    assert_eq!(parsed, f);
    // Without a statement the ABNF keeps the blank line: address LF LF LF "URI: ".
    let mut g = base_fields();
    g.statement = None;
    let t2 = text(&g);
    assert!(t2.contains(&format!("{ADDR}\n\n\nURI: ")), "{t2}");
    assert_eq!(parse_strict(&t2).expect("parses").to_message(), t2);
}

#[test]
fn d2_6_strict_parse_rejects_malformed_input() {
    let t = text(&base_fields());
    // CRLF line endings.
    assert_eq!(
        parse_strict(&t.replace('\n', "\r\n")),
        Err(SiweReject::Malformed)
    );
    // Trailing bytes.
    assert_eq!(parse_strict(&format!("{t}\n")), Err(SiweReject::Malformed));
    // Leading bytes.
    assert_eq!(parse_strict(&format!(" {t}")), Err(SiweReject::Malformed));
    // Unknown field.
    let unknown = t.replace("Version: 1", "Version: 1\nColor: blue");
    assert_eq!(parse_strict(&unknown), Err(SiweReject::Malformed));
    // Duplicate field.
    let dup = t.replace("Version: 1", "Version: 1\nVersion: 1");
    assert_eq!(parse_strict(&dup), Err(SiweReject::Malformed));
    // Out-of-order fields.
    let swapped = t.replace("Version: 1\nChain ID: 40204", "Chain ID: 40204\nVersion: 1");
    assert_eq!(parse_strict(&swapped), Err(SiweReject::Malformed));
    // Not a SIWE message at all (plain personal_sign text, or EIP-712 JSON).
    assert_eq!(parse_strict("hello world"), Err(SiweReject::Malformed));
    assert_eq!(
        parse_strict("{\"primaryType\":\"Permit\",\"domain\":{}}"),
        Err(SiweReject::Malformed)
    );
    // Over 2048 bytes.
    let mut big = base_fields();
    big.request_id = Some("r".repeat(2100));
    assert_eq!(parse_strict(&text(&big)), Err(SiweReject::TooLarge));
}

#[test]
fn d2_ok_path_passes_every_check() {
    let t = text(&base_fields());
    let ok = check_budgetable(&t, &ctx()).expect("budgetable");
    assert_eq!(ok.nonce, "abcdef0123456789XY");
    assert_eq!(
        ok.expiration_ms,
        parse_rfc3339_ms("2026-10-01T12:10:00Z")
            .map(|v| v as u64)
            .unwrap_or(0)
    );
    assert_eq!(ok.statement.as_deref(), Some("Sign in to Example."));
}

#[test]
fn d2_7_domain_must_equal_the_attested_origin() {
    let mut f = base_fields();
    f.domain = "evil.example.org".into();
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::DomainMismatch)
    );
    // Upper-case is normalized (host comparison is case-insensitive after normalization).
    let mut g = base_fields();
    g.domain = "APP.example.org".into();
    assert!(check_budgetable(&text(&g), &ctx()).is_ok());
    // A non-default port that the origin does not have.
    let mut h = base_fields();
    h.domain = "app.example.org:8443".into();
    assert_eq!(
        check_budgetable(&text(&h), &ctx()),
        Err(SiweReject::DomainMismatch)
    );
    // Userinfo in the authority.
    let mut u = base_fields();
    u.domain = "user@app.example.org".into();
    assert_eq!(
        check_budgetable(&text(&u), &ctx()),
        Err(SiweReject::DomainMismatch)
    );
    // A scheme prefix, if present, must be https.
    let mut s = base_fields();
    s.scheme = Some("http".into());
    assert_eq!(
        check_budgetable(&text(&s), &ctx()),
        Err(SiweReject::DomainMismatch)
    );
    let mut s2 = base_fields();
    s2.scheme = Some("https".into());
    assert!(check_budgetable(&text(&s2), &ctx()).is_ok());
}

#[test]
fn d2_8_uri_origin_must_equal_the_attested_origin() {
    let mut f = base_fields();
    f.uri = "https://other.example.org/login".into();
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::UriMismatch)
    );
    let mut g = base_fields();
    g.uri = "http://app.example.org/login".into();
    assert_eq!(
        check_budgetable(&text(&g), &ctx()),
        Err(SiweReject::UriMismatch)
    );
    let mut h = base_fields();
    h.uri = "/login".into();
    assert_eq!(
        check_budgetable(&text(&h), &ctx()),
        Err(SiweReject::UriMismatch)
    );
}

#[test]
fn d2_9_address_must_be_checksummed_and_the_member_wallet() {
    let mut f = base_fields();
    f.address = "0x9858effd232b4033e47d90003d41ec34ecaeda94".into();
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::AddressMismatch)
    );
    let mut g = base_fields();
    g.address = "0x52908400098527886E0F7030069857D2E4169EE7".into();
    assert_eq!(
        check_budgetable(&text(&g), &ctx()),
        Err(SiweReject::AddressMismatch)
    );
}

#[test]
fn d2_10_11_version_and_chain() {
    let mut f = base_fields();
    f.version = "2".into();
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::BadVersion)
    );
    let mut g = base_fields();
    g.chain_id = "1".into();
    assert_eq!(
        check_budgetable(&text(&g), &ctx()),
        Err(SiweReject::ChainNotAllowed)
    );
    let mut h = base_fields();
    h.chain_id = "040204".into();
    assert_eq!(
        check_budgetable(&text(&h), &ctx()),
        Err(SiweReject::Malformed)
    );
}

#[test]
fn d2_12_nonce_floor_is_sixteen_alphanumerics() {
    let mut f = base_fields();
    f.nonce = "abcdef012345678".into(); // 15
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::WeakNonce)
    );
    let mut g = base_fields();
    g.nonce = "abcdef0123456789-".into();
    assert_eq!(
        check_budgetable(&text(&g), &ctx()),
        Err(SiweReject::WeakNonce)
    );
}

#[test]
fn d2_13_issued_at_window() {
    let mut f = base_fields();
    f.issued_at = "2026-10-01T11:54:59Z".into(); // > 5 min old
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::IssuedAtOutOfRange)
    );
    let mut g = base_fields();
    g.issued_at = "2026-10-01T12:01:01Z".into(); // > 60 s ahead
    assert_eq!(
        check_budgetable(&text(&g), &ctx()),
        Err(SiweReject::IssuedAtOutOfRange)
    );
    let mut h = base_fields();
    h.issued_at = "2026-10-01T12:01:00Z".into(); // exactly +60 s is allowed
    assert!(check_budgetable(&text(&h), &ctx()).is_ok());
}

#[test]
fn d2_14_expiration_is_required_future_and_within_24h() {
    let mut f = base_fields();
    f.expiration_time = None;
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::ExpirationMissing)
    );
    let mut g = base_fields();
    g.expiration_time = Some("2026-10-01T12:00:00Z".into()); // == now: not later than now
    assert_eq!(
        check_budgetable(&text(&g), &ctx()),
        Err(SiweReject::ExpirationOutOfRange)
    );
    let mut h = base_fields();
    h.expiration_time = Some("2026-10-02T11:59:31Z".into()); // issued + 24h + 1s
    assert_eq!(
        check_budgetable(&text(&h), &ctx()),
        Err(SiweReject::ExpirationOutOfRange)
    );
}

#[test]
fn d2_15_not_before_at_most_sixty_seconds_ahead() {
    let mut f = base_fields();
    f.not_before = Some("2026-10-01T12:01:01Z".into());
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::NotBeforeOutOfRange)
    );
    let mut g = base_fields();
    g.not_before = Some("2026-10-01T12:00:30Z".into());
    assert!(check_budgetable(&text(&g), &ctx()).is_ok());
}

#[test]
fn d2_16_statement_is_short_printable_and_has_no_bidi_controls() {
    let mut f = base_fields();
    f.statement = Some("x".repeat(281));
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::BadStatement)
    );
    let mut g = base_fields();
    g.statement = Some("Sign in \u{202E}evil".into());
    assert_eq!(
        check_budgetable(&text(&g), &ctx()),
        Err(SiweReject::BadStatement)
    );
    let mut h = base_fields();
    h.statement = Some("tab\there".into());
    assert_eq!(
        check_budgetable(&text(&h), &ctx()),
        Err(SiweReject::BadStatement)
    );
}

#[test]
fn d2_17_any_resources_line_is_never_budgetable() {
    let mut f = base_fields();
    f.resources = vec!["urn:recap:eyJhdHQiOnt9fQ".into()];
    let t = text(&f);
    assert!(t.ends_with("Resources:\n- urn:recap:eyJhdHQiOnt9fQ"), "{t}");
    // It still parses (strictly), but the budget check refuses it.
    assert!(parse_strict(&t).is_ok());
    assert_eq!(check_budgetable(&t, &ctx()), Err(SiweReject::HasResources));
}

#[test]
fn d2_18_request_id_at_most_128_chars() {
    let mut f = base_fields();
    f.request_id = Some("r".repeat(129));
    assert_eq!(
        check_budgetable(&text(&f), &ctx()),
        Err(SiweReject::BadRequestId)
    );
    let mut g = base_fields();
    g.request_id = Some("req-42".into());
    let ok = check_budgetable(&text(&g), &ctx()).expect("ok");
    assert_eq!(ok.request_id.as_deref(), Some("req-42"));
}

#[test]
fn d2_4_allowlistable_origins_are_plain_https_hosts() {
    assert_eq!(
        normalize_allowlist_origin("https://App.Example.org/").as_deref(),
        Ok("https://app.example.org")
    );
    assert_eq!(
        normalize_allowlist_origin("https://app.example.org:8443").as_deref(),
        Ok("https://app.example.org:8443")
    );
    for bad in [
        "http://app.example.org",
        "file:///etc/passwd",
        "https://127.0.0.1",
        "https://[::1]",
        "https://localhost",
        "https://foo.localhost",
        "https://app.example.org/path",
        "https://user@app.example.org",
        "chrome-extension://abc",
        "https://app.example.org?x=1",
        "https://singlelabel",
    ] {
        assert!(
            normalize_allowlist_origin(bad).is_err(),
            "{bad} must not be allowlistable"
        );
    }
}

#[test]
fn every_reject_reason_reads_as_plain_language() {
    for r in [
        SiweReject::Malformed,
        SiweReject::TooLarge,
        SiweReject::DomainMismatch,
        SiweReject::UriMismatch,
        SiweReject::AddressMismatch,
        SiweReject::BadVersion,
        SiweReject::ChainNotAllowed,
        SiweReject::WeakNonce,
        SiweReject::IssuedAtOutOfRange,
        SiweReject::ExpirationMissing,
        SiweReject::ExpirationOutOfRange,
        SiweReject::NotBeforeOutOfRange,
        SiweReject::BadStatement,
        SiweReject::HasResources,
        SiweReject::BadRequestId,
    ] {
        let s = r.to_string();
        assert!(!s.is_empty() && !s.contains('\u{2014}'), "{s}");
    }
}

/// HUP-S2.3: core shares the member's address with a budgeted site in EIP-55 form, because the
/// sign-in message the site builds from it must carry a valid checksum (D2 #9).
#[test]
fn to_eip55_checksums_any_case_and_refuses_non_addresses() {
    assert_eq!(
        to_eip55("0x9858effd232b4033e47d90003d41ec34ecaeda94").as_deref(),
        Some(ADDR)
    );
    assert_eq!(
        to_eip55("0x9858EFFD232B4033E47D90003D41EC34ECAEDA94").as_deref(),
        Some(ADDR)
    );
    assert!(is_eip55(&to_eip55(ADDR).unwrap()));
    assert_eq!(to_eip55("0x9858effd"), None);
    assert_eq!(to_eip55("9858effd232b4033e47d90003d41ec34ecaeda94aa"), None);
}
