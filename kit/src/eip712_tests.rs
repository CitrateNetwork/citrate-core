// HUP-S1.5 — EIP-712 hasher tests. Vectors: the EIP-712 specification's "Ether Mail" example
// (domain separator, struct hash, digest) and the EIP-3009 TransferWithAuthorization type hash.
// The TransferWithAuthorization digest vector was computed independently with Foundry `cast`
// (keccak + abi-encode), and the anvil dry-run checks the same hasher against the deployed
// WrappedSALT contract.

use super::*;

fn h(s: &str) -> Word {
    let v = hex::decode(s.trim_start_matches("0x")).unwrap_or_default();
    let mut w = [0u8; 32];
    if v.len() == 32 {
        w.copy_from_slice(&v);
    }
    w
}

fn addr(s: &str) -> [u8; 20] {
    parse_address(s).unwrap_or([0u8; 20])
}

#[test]
fn ether_mail_domain_separator_matches_the_eip712_spec_vector() {
    let d = Domain {
        name: "Ether Mail".into(),
        version: "1".into(),
        chain_id: 1,
        verifying_contract: addr("0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC"),
    };
    assert_eq!(
        d.separator(),
        h("0xf2cee375fa42b42143804025fc449deafd50cc031ca257e0b194a650a912090f")
    );
}

#[test]
fn ether_mail_struct_hash_and_digest_match_the_eip712_spec_vector() {
    let person = type_hash("Person(string name,address wallet)");
    let mail =
        type_hash("Mail(Person from,Person to,string contents)Person(string name,address wallet)");
    let from = hash_struct(
        &person,
        &[
            string_word("Cow"),
            address_word(&addr("0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826")),
        ],
    );
    let to = hash_struct(
        &person,
        &[
            string_word("Bob"),
            address_word(&addr("0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB")),
        ],
    );
    let s = hash_struct(&mail, &[from, to, string_word("Hello, Bob!")]);
    assert_eq!(
        s,
        h("0xc52c0ee5d84264471806290a3f2c4cecfc5490626bf912d01f240d7a274b371e")
    );
    let d = Domain {
        name: "Ether Mail".into(),
        version: "1".into(),
        chain_id: 1,
        verifying_contract: addr("0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC"),
    };
    assert_eq!(
        typed_data_digest(&d.separator(), &s),
        h("0xbe609aee343fb3c4b28e1df9e632fca64fcfaede20f02e86244efddf30957bd2")
    );
}

#[test]
fn transfer_with_authorization_type_hash_is_the_eip3009_value() {
    assert_eq!(
        type_hash(TRANSFER_WITH_AUTHORIZATION_TYPE),
        h("0x7c7c6cdb67a18743f49ec6fa9b35f50d52ed05cbed4cc592e13b44501c1a2267")
    );
}

fn sample() -> (Domain, TransferWithAuthorization) {
    let d = Domain {
        name: "Wrapped SALT".into(),
        version: "1".into(),
        chain_id: 40204,
        verifying_contract: [0x11; 20],
    };
    let a = TransferWithAuthorization::new(
        [0x22; 20],
        [0x33; 20],
        parse_u256_dec("1500000000000000000").unwrap_or([0u8; 32]),
        0,
        1_900_000_000,
        [0xab; 32],
    );
    (d, a.unwrap_or_else(|e| panic!("sample window: {e}")))
}

#[test]
fn transfer_with_authorization_digest_matches_the_cast_computed_vector() {
    let (d, a) = sample();
    assert_eq!(
        d.separator(),
        h("0x68ee0c5df1f36eae41ce491be21560bb17a3ca72dbe81adc062ca83b84a1d456")
    );
    assert_eq!(
        a.struct_hash(),
        h("0x92dae52feaa356f3c06fe3a3b6db8e3a739070f6372d586ed90f86f6d40d4160")
    );
    assert_eq!(
        a.digest(&d),
        h("0x4014b8a4c7568a910c4bca5d464726f8f1c79b50cf4d576514133473002c5e04")
    );
}

#[test]
fn every_signed_field_moves_the_digest() {
    let (d, a) = sample();
    let base = a.digest(&d);
    let mut m = a.clone();
    m.from[0] ^= 1;
    assert_ne!(m.digest(&d), base, "from");
    let mut m = a.clone();
    m.to[19] ^= 1;
    assert_ne!(m.digest(&d), base, "to");
    let mut m = a.clone();
    m.value[31] ^= 1;
    assert_ne!(m.digest(&d), base, "value");
    let mut m = a.clone();
    m.valid_after += 1;
    assert_ne!(m.digest(&d), base, "validAfter");
    let mut m = a.clone();
    m.valid_before += 1;
    assert_ne!(m.digest(&d), base, "validBefore");
    let mut m = a.clone();
    m.nonce[0] ^= 1;
    assert_ne!(m.digest(&d), base, "nonce");
    for (label, dd) in [
        (
            "name",
            Domain {
                name: "Wrapped SALT2".into(),
                ..d.clone()
            },
        ),
        (
            "version",
            Domain {
                version: "2".into(),
                ..d.clone()
            },
        ),
        (
            "chainId",
            Domain {
                chain_id: 40205,
                ..d.clone()
            },
        ),
        (
            "contract",
            Domain {
                verifying_contract: [0x12; 20],
                ..d.clone()
            },
        ),
    ] {
        assert_ne!(a.digest(&dd), base, "domain {label}");
    }
}

#[test]
fn a_signature_over_the_digest_recovers_the_authorizer() {
    use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    let vk = VerifyingKey::from(&key);
    let pubkey = vk.to_encoded_point(false);
    let hashed = keccak256(&pubkey.as_bytes()[1..]);
    let mut from = [0u8; 20];
    from.copy_from_slice(&hashed[12..]);
    let (d, mut a) = sample();
    a.from = from;
    let digest = a.digest(&d);
    let (sig, recid): (Signature, RecoveryId) = key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|e| panic!("sign: {e}"));
    let rec = VerifyingKey::recover_from_prehash(&digest, &sig, recid)
        .unwrap_or_else(|e| panic!("recover: {e}"));
    assert_eq!(rec, vk);
}

#[test]
fn u256_parsing_round_trips_and_fails_closed() {
    let max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
    let w = parse_u256_dec(max).unwrap_or([0u8; 32]);
    assert_eq!(w, [0xff; 32]);
    assert_eq!(u256_to_dec(&w), max);
    // One past the maximum overflows.
    let over = "115792089237316195423570985008687907853269984665640564039457584007913129639936";
    assert_eq!(parse_u256_dec(over), Err(Eip712Error::BadUint));
    for bad in ["", "-1", "+1", "1.5", "0x10", " 1", "1e18"] {
        assert_eq!(parse_u256_dec(bad), Err(Eip712Error::BadUint), "{bad:?}");
    }
    assert_eq!(parse_u256_dec("0"), Ok([0u8; 32]));
    assert_eq!(u256_to_dec(&[0u8; 32]), "0");
    assert_eq!(parse_u256_dec("256").map(|w| (w[30], w[31])), Ok((1, 0)));
    assert_eq!(parse_u256_dec("0007"), Ok(u64_word(7)));
    let v = u64::MAX;
    assert_eq!(u256_to_dec(&u64_word(v)), v.to_string());
    let big = "340282366920938463463374607431768211456"; // 2^128
    assert_eq!(u256_to_dec(&parse_u256_dec(big).unwrap_or([0u8; 32])), big);
}

#[test]
fn format_units_shows_whole_and_fractional_amounts() {
    let p = |s: &str| parse_u256_dec(s).unwrap_or([0u8; 32]);
    assert_eq!(format_units(&p("1500000000000000000"), 18), "1.5");
    assert_eq!(format_units(&p("1000000000000000000"), 18), "1");
    assert_eq!(format_units(&p("1"), 18), "0.000000000000000001");
    assert_eq!(format_units(&p("0"), 18), "0");
    assert_eq!(format_units(&p("123"), 0), "123");
    assert_eq!(format_units(&p("20000000000000000000"), 18), "20");
}

#[test]
fn addresses_parse_strictly() {
    assert_eq!(
        parse_address("0x00000000000000000000000000000000000000ff").map(|a| a[19]),
        Ok(0xff)
    );
    for bad in [
        "",
        "0x",
        "00000000000000000000000000000000000000ff",
        "0x00000000000000000000000000000000000000f",
        "0x00000000000000000000000000000000000000fff",
        "0x00000000000000000000000000000000000000gg",
    ] {
        assert_eq!(parse_address(bad), Err(Eip712Error::BadAddress), "{bad:?}");
    }
}

#[test]
fn an_empty_validity_window_is_refused() {
    assert_eq!(
        TransferWithAuthorization::new([1; 20], [2; 20], u64_word(1), 10, 10, [0; 32]),
        Err(Eip712Error::EmptyWindow)
    );
    assert_eq!(
        TransferWithAuthorization::new([1; 20], [2; 20], u64_word(1), 11, 10, [0; 32]),
        Err(Eip712Error::EmptyWindow)
    );
}

#[test]
fn the_card_view_shows_the_hashed_numbers() {
    let (d, a) = sample();
    let v = a.view(&d, 18);
    assert_eq!(v.primary_type, "TransferWithAuthorization");
    assert_eq!(v.asset_name, "Wrapped SALT");
    assert_eq!(v.asset_contract, format!("0x{}", "11".repeat(20)));
    assert_eq!(v.payee, format!("0x{}", "33".repeat(20)));
    assert_eq!(v.from, format!("0x{}", "22".repeat(20)));
    assert_eq!(v.amount, "1.5");
    assert_eq!(v.amount_base_units, "1500000000000000000");
    assert_eq!(v.chain_id, 40204);
    assert_eq!((v.valid_after, v.valid_before), (0, 1_900_000_000));
}

#[test]
fn canonical_vrs_is_low_s_and_still_recovers_the_signer() {
    use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
    // Half the curve order: a canonical s is at most this.
    let half = h("0x7fffffffffffffffffffffffffffffff5d576e7357a4501ddfe92f46681b20a0");
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    let vk = VerifyingKey::from(&key);
    let (_, a) = sample();
    let digest = a.digest(&sample().0);
    let (sig, recid) = key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|e| panic!("sign: {e}"));
    // Build the high-s twin of the signature (s' = n - s, parity flipped): same key, not canonical.
    let (r, s) = sig.split_scalars();
    let high = Signature::from_scalars(r.to_bytes(), (-*s).to_bytes())
        .unwrap_or_else(|e| panic!("twin: {e}"));
    let high_recid = RecoveryId::new(!recid.is_y_odd(), recid.is_x_reduced());
    for (sg, rid) in [(sig, recid), (high, high_recid)] {
        let (v, r_out, s_out) = canonical_vrs(&sg, rid);
        assert!(v == 27 || v == 28);
        assert!(s_out <= half, "s must be in the lower half");
        let mut rs = [0u8; 64];
        rs[..32].copy_from_slice(&r_out);
        rs[32..].copy_from_slice(&s_out);
        let back = Signature::from_slice(&rs).unwrap_or_else(|e| panic!("sig: {e}"));
        let rid = RecoveryId::new(v == 28, false);
        let rec = VerifyingKey::recover_from_prehash(&digest, &back, rid)
            .unwrap_or_else(|e| panic!("recover: {e}"));
        assert_eq!(rec, vk);
    }
}
