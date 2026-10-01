//! HUP-S6.2: strict parameter validation. Every value that reaches a template has
//! passed one of these validators, and every validator rejects the characters that
//! could break out of a Solidity, TypeScript, JSON or HTML string context.

use citrate_templates::params::{
    checksum_address, contract_identifier, validate_name, validate_owner, validate_price,
    validate_supply, validate_symbol, RESERVED_IDENTIFIERS,
};

#[test]
fn plain_names_pass_and_derive_a_pascal_case_identifier() {
    assert_eq!(validate_name("Lemon Drops").as_deref(), Ok("Lemon Drops"));
    assert_eq!(
        contract_identifier("Lemon Drops").as_deref(),
        Ok("LemonDrops")
    );
    assert_eq!(
        contract_identifier("lemon-drops 2").as_deref(),
        Ok("LemonDrops2")
    );
    assert_eq!(contract_identifier("Citrate").as_deref(), Ok("Citrate"));
}

#[test]
fn names_that_could_escape_a_string_literal_are_rejected() {
    for bad in [
        "Lemon\"Drops",
        "Lemon'Drops",
        "Lemon`Drops",
        "Lemon\\Drops",
        "Lemon\nDrops",
        "Lemon\rDrops",
        "Lemon\u{0}Drops",
        "Lemon<script>",
        "Lemon{{ct:owner}}",
        "Lemon${x}",
        "X\"); selfdestruct(payable(msg.sender)); //",
        "L\u{e9}mon",
        "Lemon\u{202e}Drops",
        "Lemon;Drops",
        "Lemon/Drops",
    ] {
        assert!(validate_name(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn name_shape_rules() {
    for bad in [
        "",
        " ",
        "1Lemon",
        " Lemon",
        "Lemon ",
        "Lemon  Drops",
        "-Lemon",
        "Lemon-",
        "Lemon--Drops",
    ] {
        assert!(validate_name(bad).is_err(), "accepted {bad:?}");
    }
    let long = "A".repeat(49);
    assert!(validate_name(&long).is_err());
    let ok = "A".repeat(48);
    assert!(validate_name(&ok).is_ok());
}

#[test]
fn names_that_shadow_an_imported_symbol_are_rejected() {
    for bad in ["Ownable", "ERC721", "Governor", "Test", "Reentrancy Guard"] {
        assert!(contract_identifier(bad).is_err(), "accepted {bad:?}");
    }
    assert!(RESERVED_IDENTIFIERS.contains(&"ERC20Votes"));
}

#[test]
fn symbols_are_upper_case_alphanumeric_up_to_11() {
    assert_eq!(validate_symbol("LEMON").as_deref(), Ok("LEMON"));
    assert_eq!(validate_symbol("L2").as_deref(), Ok("L2"));
    for bad in [
        "",
        "lemon",
        "LEM ON",
        "LEMONDROPS12",
        "2LEM",
        "LEM\"",
        "LEM-ON",
    ] {
        assert!(validate_symbol(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn supply_is_a_canonical_decimal_within_bounds() {
    assert_eq!(validate_supply("500", 1, 1_000_000), Ok(500));
    assert_eq!(validate_supply("1", 1, 1_000_000), Ok(1));
    assert_eq!(validate_supply("1000000", 1, 1_000_000), Ok(1_000_000));
    for bad in [
        "0",
        "0500",
        "-1",
        "1e3",
        "5_00",
        "5 00",
        "",
        "+5",
        "1000001",
        "0x10",
        "18446744073709551616",
    ] {
        assert!(
            validate_supply(bad, 1, 1_000_000).is_err(),
            "accepted {bad:?}"
        );
    }
}

#[test]
fn price_is_wei_as_a_canonical_decimal() {
    assert_eq!(validate_price("0").as_deref(), Ok("0"));
    assert_eq!(
        validate_price("5000000000000000000").as_deref(),
        Ok("5000000000000000000")
    );
    // 10^30 wei (one trillion SALT) is the ceiling.
    let ceiling = format!("1{}", "0".repeat(30));
    assert!(validate_price(&ceiling).is_ok());
    let over = format!("1{}1", "0".repeat(29));
    assert!(validate_price(&over).is_err());
    for bad in ["1.5", "00", "01", "-0", "5 SALT", "", "1e18", "0x1"] {
        assert!(validate_price(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn owner_addresses_are_normalized_to_eip55() {
    // EIP-55 reference vectors.
    for good in [
        "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
        "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
        "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
        "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
    ] {
        assert_eq!(validate_owner(&good.to_lowercase()).as_deref(), Ok(good));
        assert_eq!(validate_owner(good).as_deref(), Ok(good));
        assert_eq!(checksum_address(&good[2..].to_lowercase()), good);
    }
}

#[test]
fn bad_owner_addresses_are_rejected() {
    for bad in [
        "0x5AAeb6053F3E94C9b9A09f33669435E7Ef1BeAed", // broken checksum
        "0x0000000000000000000000000000000000000000",
        "5aaeb6053f3e94c9b9a09f33669435e7ef1beaed",
        "0x5aaeb6053f3e94c9b9a09f33669435e7ef1bea",
        "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaedd",
        "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaeg",
        "0X5aaeb6053f3e94c9b9a09f33669435e7ef1beaed",
        "",
    ] {
        assert!(validate_owner(bad).is_err(), "accepted {bad:?}");
    }
}
