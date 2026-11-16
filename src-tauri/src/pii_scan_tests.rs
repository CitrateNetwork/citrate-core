// WP-H7: the deterministic private-information scanner. Every value below is assembled at run
// time or is a published test value (the Luhn test card 4111 1111 1111 1111, the IBAN registry's
// GB82 WEST 1234 5698 7654 32 example, RFC 5737 documentation addresses are NOT used because they
// are not public: 203.0.113.x is reserved, so a public-looking address is built from parts).

use super::*;

fn classes(s: &str) -> Vec<PiiClass> {
    scan(s).into_iter().map(|f| f.class).collect()
}

fn found(s: &str, class: PiiClass) -> Vec<String> {
    scan(s)
        .into_iter()
        .filter(|f| f.class == class)
        .map(|f| s[f.start..f.end].to_string())
        .collect()
}

#[test]
fn emails_and_secrets_come_from_the_shared_secret_shapes() {
    let mail = ["ana.lopez", "@", "example.org"].concat();
    assert_eq!(
        found(&format!("write to {mail} today"), PiiClass::Email),
        vec![mail.clone()]
    );
    let key = ["0x", &"ab".repeat(32)].concat();
    assert_eq!(classes(&format!("key {key}")), vec![PiiClass::Secret]);
    let gh = ["ghp_", "A1b2C3d4E5f6G7h8I9j0K1l2"].concat();
    assert_eq!(classes(&format!("token {gh}")), vec![PiiClass::Secret]);
}

#[test]
fn phone_numbers_in_their_written_forms() {
    for p in [
        "+1 202 555 0143",
        "+442079460958",
        "(202) 555-0143",
        "202-555-0143",
        "202.555.0143",
        "020 7946 0958 12",
    ] {
        assert_eq!(
            found(&format!("call {p} now"), PiiClass::Phone),
            vec![p.to_string()],
            "{p}"
        );
    }
}

#[test]
fn numbers_that_only_look_long_are_not_phone_numbers() {
    for s in [
        "block 1790000000 was final",
        "height 12345678901",
        "on 2026-11-15 at 10:30",
        "version 1.2.3.4",
        "pi is 3.14159265358979",
        "0x00000000000000000000000000000000000000ff",
        "round 42 of 100",
    ] {
        assert!(found(s, PiiClass::Phone).is_empty(), "{s}: {:?}", scan(s));
    }
}

#[test]
fn card_numbers_need_the_luhn_check() {
    assert_eq!(
        found("card 4111 1111 1111 1111 exp", PiiClass::Card),
        vec!["4111 1111 1111 1111"]
    );
    assert_eq!(
        found("card 4111-1111-1111-1111", PiiClass::Card),
        vec!["4111-1111-1111-1111"]
    );
    assert_eq!(
        found("pan 4111111111111111.", PiiClass::Card),
        vec!["4111111111111111"]
    );
    assert!(
        found("card 4111 1111 1111 1112", PiiClass::Card).is_empty(),
        "fails Luhn"
    );
}

#[test]
fn social_security_numbers_with_a_valid_area_group_and_serial() {
    let ssn = ["123", "-", "45", "-", "6789"].concat();
    assert_eq!(
        found(&format!("ssn {ssn}"), PiiClass::GovernmentId),
        vec![ssn]
    );
    for bad in [
        "000-12-3456",
        "666-12-3456",
        "912-12-3456",
        "123-00-4567",
        "123-45-0000",
    ] {
        assert!(found(bad, PiiClass::GovernmentId).is_empty(), "{bad}");
    }
}

#[test]
fn ibans_pass_mod_97() {
    assert_eq!(
        found("pay GB82 WEST 1234 5698 7654 32 please", PiiClass::Iban),
        vec!["GB82 WEST 1234 5698 7654 32"]
    );
    assert_eq!(
        found("GB82WEST12345698765432", PiiClass::Iban),
        vec!["GB82WEST12345698765432"]
    );
    assert!(found("GB83WEST12345698765432", PiiClass::Iban).is_empty());
}

#[test]
fn public_ipv4_addresses_but_not_private_ones() {
    let public = ["8", ".", "8", ".", "4", ".", "4"].concat();
    assert_eq!(
        found(&format!("from {public} today"), PiiClass::IpAddress),
        vec![public]
    );
    for private in [
        "10.0.0.4",
        "192.168.1.20",
        "172.20.0.1",
        "127.0.0.1",
        "169.254.3.3",
        "v8.8.4.4",
    ] {
        assert!(found(private, PiiClass::IpAddress).is_empty(), "{private}");
    }
}

#[test]
fn street_addresses_in_the_house_number_form() {
    for a in [
        "12 Elm Street",
        "400 N Main St",
        "1600 Pennsylvania Avenue",
        "7 Old Mill Rd.",
        "PO Box 4471",
    ] {
        assert_eq!(
            found(&format!("send it to {a}, thanks"), PiiClass::StreetAddress),
            vec![a.to_string()],
            "{a}"
        );
    }
    assert!(found("we have 3 Main ideas", PiiClass::StreetAddress).is_empty());
    assert!(found("chapter 12 street food", PiiClass::StreetAddress).is_empty());
}

#[test]
fn names_with_a_title_a_label_or_a_common_given_name() {
    assert_eq!(
        found("ask Dr. Rivera about it", PiiClass::Name),
        vec!["Dr. Rivera"]
    );
    assert_eq!(found("Name: Ana Lopez", PiiClass::Name), vec!["Ana Lopez"]);
    assert_eq!(
        found("hi, my name is Priya and", PiiClass::Name),
        vec!["Priya"]
    );
    assert_eq!(
        found("met Maria Gonzalez at noon", PiiClass::Name),
        vec!["Maria Gonzalez"]
    );
    // A given name alone, or before a lowercase word, is not flagged.
    assert!(found("Will this work? Grace periods apply.", PiiClass::Name).is_empty());
    assert!(found("Mark the round as done", PiiClass::Name).is_empty());
}

#[test]
fn ordinary_fl_text_passes() {
    let rows = [
        "Group rounds not live yet: FederatedRoundLedger is deployed with no rounds.",
        "The adapter scored 0.71 on the held-out set, above the 0.65 floor.",
        "Round 3 needs 16 GB of VRAM and mains power, from 22:00 to 06:00.",
        "Budget 1200 SALT, rate 4 SALT per unit, at most 8 workers.",
    ];
    let r = scan_rows(rows);
    assert!(r.passed, "{r:?}");
    assert_eq!(r.rows_scanned, 4);
    assert_eq!(r.scanner, SCANNER);
}

#[test]
fn a_report_counts_every_class_and_points_at_rows_not_text() {
    let mail = ["bo", "@", "example.org"].concat();
    let rows = [
        format!("contact {mail}"),
        "nothing here".to_string(),
        "call +1 202 555 0143 or 202-555-0144".to_string(),
    ];
    let r = scan_rows(rows.iter().map(String::as_str));
    assert!(!r.passed);
    assert_eq!(r.counts.get(&PiiClass::Email), Some(&1));
    assert_eq!(r.counts.get(&PiiClass::Phone), Some(&2));
    assert_eq!(
        r.findings.iter().map(|f| f.row).collect::<Vec<_>>(),
        vec![0, 2, 2]
    );
    // The report never carries the matched text.
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains(&mail), "{json}");
    assert!(!json.contains("555"), "{json}");
    assert_eq!(r.coverage, COVERAGE);
}

#[test]
fn row_references_are_capped_but_counts_are_complete() {
    let mail = ["x", "@", "example.org"].concat();
    let rows: Vec<String> = (0..MAX_ROW_REFS + 5).map(|_| mail.clone()).collect();
    let r = scan_rows(rows.iter().map(String::as_str));
    assert_eq!(r.findings.len(), MAX_ROW_REFS);
    assert!(r.truncated);
    assert_eq!(r.counts.get(&PiiClass::Email), Some(&(MAX_ROW_REFS + 5)));
}

#[test]
fn redact_replaces_each_finding_with_its_class_and_is_idempotent() {
    let mail = ["ana", "@", "example.org"].concat();
    let s = format!("Dr. Rivera ({mail}, 202-555-0143) lives at 12 Elm Street.");
    let r = redact(&s);
    assert_eq!(r, "<name> (<email>, <phone>) lives at <street-address>.");
    assert_eq!(redact(&r), r);
    assert!(scan(&r).is_empty());
}

#[test]
fn spans_are_valid_utf8_slices_around_non_ascii_text() {
    let mail = ["zoë", "@", "example.org"].concat();
    let s = format!("café — {mail} — naïve 202-555-0143 ünd");
    for f in scan(&s) {
        assert!(
            s.is_char_boundary(f.start) && s.is_char_boundary(f.end),
            "{f:?}"
        );
    }
    let _ = redact(&s);
}

#[test]
fn the_given_name_list_is_unique_and_capitalised() {
    let mut seen = std::collections::BTreeSet::new();
    for n in GIVEN_NAMES {
        assert!(seen.insert(*n), "{n} twice");
        assert!(
            n.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
            "{n}"
        );
    }
}

#[test]
fn a_text_scan_counts_classes_and_redacts_without_spans() {
    let mail = ["ana", "@", "example.org"].concat();
    let t = scan_text(&format!("Ask Dr. Rivera at {mail} or {mail}."));
    assert!(!t.passed);
    assert_eq!(
        t.found,
        vec![
            ClassCount {
                class: PiiClass::Email,
                label: "email address",
                count: 2
            },
            ClassCount {
                class: PiiClass::Name,
                label: "person's name",
                count: 1
            },
        ]
    );
    assert_eq!(t.redacted, "Ask <name> at <email> or <email>.");
    let ok = scan_text("Group rounds not live yet.");
    assert!(ok.passed && ok.found.is_empty());
    assert_eq!(ok.redacted, "Group rounds not live yet.");
}

#[test]
fn class_keys_match_their_json_names() {
    for c in [
        PiiClass::Email,
        PiiClass::Phone,
        PiiClass::Card,
        PiiClass::GovernmentId,
        PiiClass::Iban,
        PiiClass::IpAddress,
        PiiClass::StreetAddress,
        PiiClass::Name,
        PiiClass::Secret,
    ] {
        assert_eq!(serde_json::to_value(c).unwrap(), serde_json::json!(c.key()));
    }
}

#[test]
fn an_amex_number_in_its_printed_4_6_5_grouping_is_a_card() {
    // Codex review of #734 (P2): the published Amex test number, as printed on the card.
    let amex = ["3782", " ", "822463", " ", "10005"].concat();
    assert_eq!(found(&format!("amex {amex} exp"), PiiClass::Card), vec![amex.clone()]);
    // Other uneven groupings are still not cards.
    assert!(found("ref 37828 22463 10005", PiiClass::Card).is_empty());
}

#[test]
fn two_phone_numbers_side_by_side_are_both_found() {
    // Codex review of #734 (P2): the run of both was consumed and rejected as one too-long number.
    let a = "202-555-0143";
    let b = "202-555-0144";
    assert_eq!(found(&format!("call {a} {b} today"), PiiClass::Phone), vec![a, b]);
    assert_eq!(found(&format!("call {a} or {b}"), PiiClass::Phone), vec![a, b]);
}
