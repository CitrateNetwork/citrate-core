// HUP-S1.5: the x402 TransferWithAuthorization hasher and builder.
//
// Golden vectors (data source):
// - WSALT_40204_DOMAIN_SEPARATOR: read-only `cast call <WrappedSALT> "DOMAIN_SEPARATOR()(bytes32)"`
//   against https://rpc.citrate.ai on 2026-10-01 (WrappedSALT at the address in
//   citrate-chain contracts/addresses/40204.json, fresh-keys reroll book).
// - TYPEHASH: the same contract's `TRANSFER_WITH_AUTHORIZATION_TYPEHASH()`.
// - STRUCT_HASH / DIGEST: `cast abi-encode` + `cast keccak` (foundry 1.5.1) over the inputs below.

use super::*;

const WSALT_40204: &str = "0xAa918302B94a4B0E75E01e019cc6b819B4F7c906";
const WSALT_40204_DOMAIN_SEPARATOR: &str =
    "0815ebdf151087b2c66632b9e944b56ebfbdb264283fc95e278526d2145419c1";
const TYPEHASH: &str = "7c7c6cdb67a18743f49ec6fa9b35f50d52ed05cbed4cc592e13b44501c1a2267";
const STRUCT_HASH: &str = "5bd67e1dbe9e76796dfa3470eb2e9bcb4d5f3f7f66b412c4b9caae49f252b9b5";
const DIGEST: &str = "fccd731abee79b4c820c34ed0ca82a4841f1d60bb445380a675d6c03494aeee1";
const PAYER: &str = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94";
const PAYEE: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";

fn wsalt_domain() -> X402Domain {
    X402Domain::new("Wrapped SALT", "1", 40204, WSALT_40204).expect("domain")
}

fn golden_auth() -> X402Authorization {
    X402Authorization {
        from: PAYER.to_ascii_lowercase(),
        to: PAYEE.to_ascii_lowercase(),
        value: "10000000000000000".into(),
        valid_after: 1_790_000_000,
        valid_before: 1_790_000_600,
        nonce: format!("0x{}", "01".repeat(32)),
    }
}

#[test]
fn type_hash_matches_the_deployed_wrapped_salt() {
    assert_eq!(hex::encode(type_hash()), TYPEHASH);
}

#[test]
fn domain_separator_matches_the_deployed_wrapped_salt_on_40204() {
    let ds = domain_separator(&wsalt_domain()).expect("ds");
    assert_eq!(hex::encode(ds), WSALT_40204_DOMAIN_SEPARATOR);
}

#[test]
fn domain_separator_binds_chain_contract_name_and_version() {
    let base = domain_separator(&wsalt_domain()).expect("ds");
    let other_chain = X402Domain::new("Wrapped SALT", "1", 31337, WSALT_40204).expect("d");
    let other_contract =
        X402Domain::new("Wrapped SALT", "1", 40204, "0x5fbdb2315678afecb367f032d93f642f64180aa3")
            .expect("d");
    let other_name = X402Domain::new("Wrapped SALT2", "1", 40204, WSALT_40204).expect("d");
    let other_version = X402Domain::new("Wrapped SALT", "2", 40204, WSALT_40204).expect("d");
    for d in [other_chain, other_contract, other_name, other_version] {
        assert_ne!(domain_separator(&d).expect("ds"), base, "{d:?}");
    }
}

#[test]
fn struct_hash_and_digest_match_cast() {
    let a = golden_auth();
    assert_eq!(hex::encode(struct_hash(&a).expect("sh")), STRUCT_HASH);
    assert_eq!(
        hex::encode(signing_digest(&wsalt_domain(), &a).expect("digest")),
        DIGEST
    );
}

#[test]
fn every_signed_field_changes_the_digest() {
    let d = wsalt_domain();
    let base = signing_digest(&d, &golden_auth()).expect("digest");
    let mut variants = Vec::new();
    let mut a = golden_auth();
    a.from = "0x0000000000000000000000000000000000000001".into();
    variants.push(a);
    let mut a = golden_auth();
    a.to = "0x0000000000000000000000000000000000000002".into();
    variants.push(a);
    let mut a = golden_auth();
    a.value = "10000000000000001".into();
    variants.push(a);
    let mut a = golden_auth();
    a.valid_after += 1;
    variants.push(a);
    let mut a = golden_auth();
    a.valid_before += 1;
    variants.push(a);
    let mut a = golden_auth();
    a.nonce = format!("0x{}", "02".repeat(32));
    variants.push(a);
    for v in variants {
        assert_ne!(signing_digest(&d, &v).expect("digest"), base, "{v:?}");
    }
}

#[test]
fn builder_sets_a_bounded_window_and_normalizes_addresses() {
    let a = build_authorization(PAYER, PAYEE, "250", 1_000_000, 600, format!("0x{}", "AB".repeat(32)))
        .expect("auth");
    assert_eq!(a.from, PAYER.to_ascii_lowercase());
    assert_eq!(a.to, PAYEE.to_ascii_lowercase());
    assert_eq!(a.value, "250");
    assert_eq!(a.valid_after, 1_000_000 - VALID_AFTER_SKEW_SECS);
    assert_eq!(a.valid_before, 1_000_600);
    assert_eq!(a.nonce, format!("0x{}", "ab".repeat(32)));
}

#[test]
fn builder_refuses_a_window_longer_than_the_adr_cap_or_too_short() {
    let n = || format!("0x{}", "01".repeat(32));
    assert_eq!(
        build_authorization(PAYER, PAYEE, "1", 1_000, VALIDITY_MAX_SECS + 1, n()),
        Err(X402Error::BadWindow)
    );
    assert_eq!(
        build_authorization(PAYER, PAYEE, "1", 1_000, VALIDITY_MIN_SECS - 1, n()),
        Err(X402Error::BadWindow)
    );
    assert!(build_authorization(PAYER, PAYEE, "1", 1_000, VALIDITY_MAX_SECS, n()).is_ok());
}

#[test]
fn builder_refuses_bad_amounts_addresses_and_nonces() {
    let n = || format!("0x{}", "01".repeat(32));
    for bad in ["", "0", "000", "-1", "1.5", "1e18", "340282366920938463463374607431768211456"] {
        assert_eq!(
            build_authorization(PAYER, PAYEE, bad, 1_000, 600, n()),
            Err(X402Error::BadAmount),
            "{bad}"
        );
    }
    assert!(matches!(
        build_authorization("0x1234", PAYEE, "1", 1_000, 600, n()),
        Err(X402Error::BadAddress(_))
    ));
    assert!(matches!(
        build_authorization(PAYER, "0x0000000000000000000000000000000000000000", "1", 1_000, 600, n()),
        Err(X402Error::BadAddress(_))
    ));
    assert_eq!(
        build_authorization(PAYER, PAYEE, "1", 1_000, 600, "0x01".into()),
        Err(X402Error::BadAmount)
    );
}

#[test]
fn fresh_nonces_are_32_bytes_and_distinct() {
    let a = fresh_nonce();
    let b = fresh_nonce();
    assert_eq!(a.len(), 66);
    assert!(a.starts_with("0x"));
    assert_ne!(a, b);
}

#[test]
fn domain_refuses_incomplete_input() {
    assert_eq!(X402Domain::new("", "1", 40204, WSALT_40204), Err(X402Error::BadDomain));
    assert_eq!(X402Domain::new("Wrapped SALT", "", 40204, WSALT_40204), Err(X402Error::BadDomain));
    assert_eq!(X402Domain::new("Wrapped SALT", "1", 0, WSALT_40204), Err(X402Error::BadDomain));
    assert_eq!(X402Domain::new("W\u{7}", "1", 40204, WSALT_40204), Err(X402Error::BadDomain));
    assert!(matches!(
        X402Domain::new("Wrapped SALT", "1", 40204, "0xnothex"),
        Err(X402Error::BadAddress(_))
    ));
}

#[test]
fn verify_refuses_a_malformed_or_foreign_signature() {
    let d = wsalt_domain();
    let a = golden_auth();
    assert_eq!(verify(&d, &a, "00"), Err(X402Error::BadSignature));
    assert_eq!(verify(&d, &a, &"11".repeat(65)), Err(X402Error::BadSignature));
}

#[test]
fn format_units_trims_and_keeps_whole_values() {
    assert_eq!(format_units(10_000_000_000_000_000, 18), "0.01");
    assert_eq!(format_units(1_000_000_000_000_000_000, 18), "1");
    assert_eq!(format_units(1_234_500_000_000_000_000, 18), "1.2345");
    assert_eq!(format_units(1, 18), "0.000000000000000001");
}
