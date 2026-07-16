// @rule8 — the native-transfer intent builder. The load-bearing property is that
// the `raw` JSON a Send produces DECODES (via the same txdecode B1.4 uses) to the
// exact native transfer the human intended — right recipient, exact value (no
// truncation), no calldata, standard gas. If this drifts, the ceremony would sign
// a different tx than the user approved.
use super::*;

#[test]
fn validate_address_canonicalizes_and_rejects_malformed() {
    assert_eq!(
        validate_address("0x9858EFFD232B4033E47D90003D41EC34ECAEDA94").unwrap(),
        "0x9858effd232b4033e47d90003d41ec34ecaeda94",
        "canonical lowercased 0x form"
    );
    assert!(validate_address("0x1234").is_err(), "too short");
    assert!(validate_address("notanaddr").is_err(), "no 0x / not hex");
    assert!(
        validate_address("0xZZ58effd232b4033e47d90003d41ec34ecaeda94").is_err(),
        "non-hex digits"
    );
}

#[test]
fn transfer_json_decodes_to_the_expected_native_transfer() {
    let value: u128 = 1_500_000_000_000_000_000; // 1.5 SALT
    let json = encode_transfer_json(
        "0x9858effd232b4033e47d90003d41ec34ecaeda94",
        "0x1111111111111111111111111111111111111111",
        value,
    );
    let (parsed, display) =
        crate::txdecode::decode_transaction(&json).expect("native-transfer json must decode");
    assert_eq!(parsed.value, value, "value round-trips exactly (no truncation)");
    assert_eq!(parsed.to, Some([0x11u8; 20]), "to = recipient");
    assert!(parsed.data.is_empty(), "native transfer carries no calldata");
    assert_eq!(parsed.gas_limit, Some(21_000), "standard 21k transfer gas");
    assert_eq!(
        parsed.from.as_deref(),
        Some("0x9858effd232b4033e47d90003d41ec34ecaeda94"),
        "from = the sending vault wallet"
    );
    // The human-facing decode surfaces the real recipient (not fabricated).
    assert!(
        display.destination.to_ascii_lowercase().contains("1111"),
        "recipient surfaced in the decoded action: {}",
        display.destination
    );
}

#[test]
fn transfer_json_handles_a_large_value_beyond_u64() {
    // 32,000 SALT ~ 3.2e22 wei — beyond u64; must survive the 0x-hex round-trip.
    let value: u128 = 32_000u128 * 1_000_000_000_000_000_000u128;
    let json = encode_transfer_json(
        "0x9858effd232b4033e47d90003d41ec34ecaeda94",
        "0x2222222222222222222222222222222222222222",
        value,
    );
    let (parsed, _display) =
        crate::txdecode::decode_transaction(&json).expect("large-value json must decode");
    assert_eq!(parsed.value, value, "u128 value beyond u64 round-trips exactly");
}
