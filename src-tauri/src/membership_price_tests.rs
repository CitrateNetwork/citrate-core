// Tests for the membership price read (C1-P; UI truth contract §5.3).

use super::*;

const LIVE: &str = r#"{"currency":"usd","unitAmount":4800,"interval":"year","displayAmount":"$48","source":"stripe","priceId":"price_x","fetchedAt":"2026-10-19T20:41:08.815Z"}"#;

#[test]
fn the_live_body_parses() {
    let p = parse_price(200, LIVE.as_bytes()).expect("price");
    assert_eq!(
        p,
        MembershipPrice {
            currency: "usd".into(),
            unit_amount: 4800,
            interval: "year".into(),
            display_amount: "$48".into(),
        }
    );
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["unitAmount"], 4800);
    assert_eq!(j["displayAmount"], "$48");
}

#[test]
fn a_display_amount_with_cents_or_separators_matches_the_unit_amount() {
    let b = r#"{"currency":"usd","unitAmount":124800,"interval":"year","displayAmount":"$1,248.00"}"#;
    assert_eq!(parse_price(200, b.as_bytes()).expect("price").unit_amount, 124_800);
}

#[test]
fn a_503_or_any_non_200_is_unavailable_with_no_number() {
    for code in [503u16, 500, 404, 302] {
        let e = parse_price(code, br#"{"error":"price unavailable"}"#).expect_err("unavailable");
        assert!(e.contains(&code.to_string()), "{e}");
        assert!(!e.chars().any(|c| c == '$'), "{e}");
    }
}

#[test]
fn a_display_amount_that_disagrees_with_the_unit_amount_is_refused() {
    let b = r#"{"currency":"usd","unitAmount":4800,"interval":"year","displayAmount":"$49"}"#;
    assert!(parse_price(200, b.as_bytes()).is_err());
}

#[test]
fn malformed_bodies_are_refused() {
    for b in [
        "not json",
        r#"{"currency":"usd","interval":"year","displayAmount":"$48"}"#,
        r#"{"currency":"usd","unitAmount":0,"interval":"year","displayAmount":"$0"}"#,
        r#"{"currency":"usd","unitAmount":48.5,"interval":"year","displayAmount":"$0.485"}"#,
        r#"{"currency":"usd","unitAmount":4800,"interval":"fortnight","displayAmount":"$48"}"#,
        r#"{"currency":"US Dollars","unitAmount":4800,"interval":"year","displayAmount":"$48"}"#,
        r#"{"currency":"usd","unitAmount":4800,"interval":"year","displayAmount":""}"#,
        r#"{"currency":"usd","unitAmount":4800,"interval":"year","displayAmount":"$48 <b>"}"#,
        r#"{"currency":"usd","unitAmount":4800,"interval":"year"}"#,
    ] {
        assert!(parse_price(200, b.as_bytes()).is_err(), "{b}");
    }
}

#[test]
fn the_route_is_under_the_configured_membership_base() {
    assert_eq!(price_url("https://membership.citrate.ai"), "https://membership.citrate.ai/api/membership/price");
    assert_eq!(price_url("https://membership.citrate.ai/"), "https://membership.citrate.ai/api/membership/price");
}

// #379 review P2: the whole display string must match the declared currency's symbol and
// minor-unit scale, and state unitAmount exactly.
fn body(currency: &str, unit: u64, display: &str) -> String {
    format!(r#"{{"currency":"{currency}","unitAmount":{unit},"interval":"year","displayAmount":"{display}"}}"#)
}

#[test]
fn a_display_in_another_currency_is_refused() {
    for (cur, unit, display) in [
        ("usd", 4800, "€48"),
        ("usd", 4800, "£48"),
        ("eur", 4800, "$48"),
        ("usd", 4800, "USD 48"),
        ("usd", 4800, "48"),
        ("usd", 4800, "$48 USD"),
        ("usd", 4800, "$ 48"),
        ("usd", 4800, "$48.0"),
        ("usd", 4800, "$048"),
        ("usd", 480000, "$4,8000"),
        ("usd", 480000, "$48,00"),
        ("jpy", 4800, "¥48.00"),
        ("chf", 4800, "CHF48"),
    ] {
        assert!(parse_price(200, body(cur, unit, display).as_bytes()).is_err(), "{cur} {unit} {display}");
    }
}

#[test]
fn each_supported_currency_accepts_its_own_format() {
    for (cur, unit, display) in [
        ("usd", 4800, "$48"),
        ("usd", 4800, "$48.00"),
        ("usd", 124800, "$1,248.00"),
        ("eur", 4800, "€48"),
        ("gbp", 4850, "£48.50"),
        ("jpy", 4800, "¥4,800"),
        ("jpy", 4800, "¥4800"),
    ] {
        let p = parse_price(200, body(cur, unit, display).as_bytes()).unwrap_or_else(|e| panic!("{cur} {display}: {e}"));
        assert_eq!(p.unit_amount, unit);
    }
}
