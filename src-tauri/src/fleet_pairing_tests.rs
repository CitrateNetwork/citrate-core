// HUP-S8.2 — fleet pairing tokens: signature, expiry, single use, link + QR encoding.
// Written red-first (module body absent, so these failed to compile), then brought green.

use super::*;

const NOW: u64 = 1_790_000_000;

fn issuer() -> PairIssuer {
    PairIssuer::from_seed([7u8; 32])
}

fn hints() -> Vec<String> {
    vec![
        "192.168.1.20:41234".to_string(),
        "100.64.0.9:41234".to_string(),
    ]
}

fn issue(iss: &mut PairIssuer) -> (PairClaim, String) {
    iss.issue("Studio", Some("T2"), hints(), NOW).unwrap()
}

// ---- signature ------------------------------------------------------------------------------

#[test]
fn issued_link_verifies_offline_against_the_embedded_issuer_key() {
    let mut iss = issuer();
    let (claim, link) = issue(&mut iss);
    assert!(link.starts_with(LINK_PREFIX));
    let got = verify_link(&link, NOW + 1).unwrap();
    assert_eq!(got, claim);
    assert_eq!(got.issuer_pub, iss.public_hex());
    assert_eq!(got.issuer_label, "Studio");
    assert_eq!(got.issuer_tier.as_deref(), Some("T2"));
    assert_eq!(got.hints, hints());
    assert_eq!(got.expires_at, NOW + PAIR_TTL_SECS);
}

#[test]
fn a_tampered_claim_fails_the_signature() {
    let mut iss = issuer();
    let (_claim, link) = issue(&mut iss);
    let (json, _c, sig) = decode_link(&link).unwrap();
    // Re-point the hints at another host without re-signing.
    let forged = String::from_utf8(json)
        .unwrap()
        .replace("192.168.1.20", "10.66.66.66");
    let forged_link = encode_link(forged.as_bytes(), &sig);
    assert_eq!(verify_link(&forged_link, NOW), Err(PairError::BadSignature));
    assert_eq!(iss.redeem(&forged_link, NOW), Err(PairError::BadSignature));
}

#[test]
fn a_link_signed_by_another_key_is_refused_by_the_issuer() {
    let mut mine = issuer();
    let mut other = PairIssuer::from_seed([9u8; 32]);
    let _ = issue(&mut mine);
    let (_c, foreign) = issue(&mut other);
    // Offline it is self-consistent (it IS validly signed by its own embedded key)...
    assert!(verify_link(&foreign, NOW).is_ok());
    // ...but the issuer only honours tokens it signed itself.
    assert_eq!(mine.redeem(&foreign, NOW), Err(PairError::UnknownToken));
}

#[test]
fn swapping_in_another_issuer_key_breaks_the_signature() {
    let mut a = issuer();
    let b = PairIssuer::from_seed([9u8; 32]);
    let (_c, link) = issue(&mut a);
    let (json, _c, sig) = decode_link(&link).unwrap();
    let swapped = String::from_utf8(json)
        .unwrap()
        .replace(&a.public_hex(), &b.public_hex());
    let swapped_link = encode_link(swapped.as_bytes(), &sig);
    assert_eq!(
        verify_link(&swapped_link, NOW),
        Err(PairError::BadSignature)
    );
}

#[test]
fn malformed_links_are_rejected_without_panicking() {
    for bad in [
        "",
        "https://example.com/pair?c=a&s=b",
        "citrate://pair?",
        "citrate://pair?c=!!!&s=???",
        "citrate://pair?c=e30&s=AAAA",
        "citrate://join?c=e30&s=AAAA",
    ] {
        assert_eq!(verify_link(bad, NOW), Err(PairError::Malformed), "{bad}");
    }
    let long = format!("{LINK_PREFIX}c={}&s=x", "A".repeat(MAX_LINK_LEN));
    assert_eq!(verify_link(&long, NOW), Err(PairError::Malformed));
}

#[test]
fn the_signature_is_domain_separated() {
    // A bare ed25519 signature over the claim JSON (no domain prefix) must not verify.
    use ed25519_dalek::Signer;
    let mut iss = issuer();
    let (_c, link) = issue(&mut iss);
    let (json, _c, _sig) = decode_link(&link).unwrap();
    let raw = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]).sign(&json);
    let undomained = encode_link(&json, &raw.to_bytes());
    assert_eq!(verify_link(&undomained, NOW), Err(PairError::BadSignature));
}

// ---- expiry ---------------------------------------------------------------------------------

#[test]
fn a_link_expires_after_its_ttl() {
    let mut iss = issuer();
    let (_c, link) = issue(&mut iss);
    assert!(verify_link(&link, NOW + PAIR_TTL_SECS - 1).is_ok());
    assert_eq!(
        verify_link(&link, NOW + PAIR_TTL_SECS),
        Err(PairError::Expired)
    );
    assert_eq!(
        iss.redeem(&link, NOW + PAIR_TTL_SECS),
        Err(PairError::Expired)
    );
}

#[test]
fn an_expired_token_cannot_be_redeemed_later_either() {
    let mut iss = issuer();
    let (_c, link) = issue(&mut iss);
    assert_eq!(
        iss.redeem(&link, NOW + PAIR_TTL_SECS + 5),
        Err(PairError::Expired)
    );
    assert_eq!(iss.redeem(&link, NOW + 1), Err(PairError::UnknownToken));
}

#[test]
fn a_link_from_the_future_is_refused() {
    let mut iss = issuer();
    let (_c, link) = issue(&mut iss);
    assert_eq!(
        verify_link(&link, NOW - CLOCK_SKEW_SECS - 1),
        Err(PairError::NotYetValid)
    );
    assert!(verify_link(&link, NOW - CLOCK_SKEW_SECS).is_ok());
}

#[test]
fn a_signed_claim_with_an_overlong_lifetime_is_refused() {
    // Even a validly signed claim may not ask for more than the maximum lifetime.
    use ed25519_dalek::Signer;
    let sk = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
    let claim = PairClaim {
        v: 1,
        nonce: "00112233445566778899aabbccddeeff".into(),
        issuer_pub: hex::encode(sk.verifying_key().to_bytes()),
        issuer_label: "x".into(),
        issuer_tier: None,
        issued_at: NOW,
        expires_at: NOW + PAIR_TTL_SECS + 1,
        hints: vec![],
    };
    let json = serde_json::to_vec(&claim).unwrap();
    let sig = sk.sign(&signing_bytes(&json));
    let link = encode_link(&json, &sig.to_bytes());
    assert_eq!(verify_link(&link, NOW), Err(PairError::Malformed));
}

// ---- single use -----------------------------------------------------------------------------

#[test]
fn a_token_redeems_exactly_once() {
    let mut iss = issuer();
    let (claim, link) = issue(&mut iss);
    assert_eq!(iss.outstanding(NOW), 1);
    assert_eq!(iss.redeem(&link, NOW + 3), Ok(claim));
    assert_eq!(iss.redeem(&link, NOW + 4), Err(PairError::AlreadyUsed));
    assert_eq!(iss.outstanding(NOW + 4), 0);
}

#[test]
fn each_issue_mints_a_fresh_nonce() {
    let mut iss = issuer();
    let (a, la) = issue(&mut iss);
    let (b, lb) = issue(&mut iss);
    assert_ne!(a.nonce, b.nonce);
    assert_eq!(a.nonce.len(), 32);
    assert_ne!(la, lb);
    assert_eq!(iss.outstanding(NOW), 2);
    // Redeeming one leaves the other usable.
    assert!(iss.redeem(&la, NOW).is_ok());
    assert!(iss.redeem(&lb, NOW).is_ok());
}

#[test]
fn outstanding_tokens_are_capped() {
    let mut iss = issuer();
    for _ in 0..MAX_OUTSTANDING {
        issue(&mut iss);
    }
    assert_eq!(
        iss.issue("Studio", None, vec![], NOW).map(|_| ()),
        Err(PairError::TooManyOutstanding)
    );
    // Once they lapse, issuing works again.
    assert!(iss
        .issue("Studio", None, vec![], NOW + PAIR_TTL_SECS)
        .is_ok());
}

#[test]
fn issue_validates_label_and_hints() {
    let mut iss = issuer();
    assert_eq!(
        iss.issue(&"x".repeat(MAX_LABEL_LEN + 1), None, vec![], NOW)
            .map(|_| ()),
        Err(PairError::Malformed)
    );
    let many: Vec<String> = (0..MAX_HINTS + 1)
        .map(|i| format!("10.0.0.{i}:1"))
        .collect();
    assert_eq!(
        iss.issue("a", None, many, NOW).map(|_| ()),
        Err(PairError::Malformed)
    );
    assert_eq!(
        iss.issue("a", None, vec!["not an address".into()], NOW)
            .map(|_| ()),
        Err(PairError::Malformed)
    );
}

#[test]
fn the_issuer_key_is_not_derivable_from_the_link() {
    // The link carries the public key and a signature only; no secret-named field.
    let mut iss = issuer();
    let (_c, link) = issue(&mut iss);
    let (json, _c, _s) = decode_link(&link).unwrap();
    let text = String::from_utf8(json).unwrap();
    assert!(!text.contains(&hex::encode([7u8; 32])));
    for f in ["secret", "seed", "priv"] {
        assert!(!text.contains(f), "{f}");
    }
}

// ---- QR -------------------------------------------------------------------------------------

#[test]
fn the_link_encodes_to_a_square_qr_matrix() {
    let mut iss = issuer();
    let (_c, link) = issue(&mut iss);
    let qr = qr_matrix(&link).unwrap();
    assert_eq!(qr.rows.len(), qr.size);
    assert!(qr.rows.iter().all(|r| r.len() == qr.size));
    assert!(qr
        .rows
        .iter()
        .all(|r| r.chars().all(|c| c == '0' || c == '1')));
    // A version-1 QR is 21 modules; a signed link is far bigger than that.
    assert!(qr.size > 21);
    // Finder pattern: the top-left 7 modules of row 0 are dark.
    assert_eq!(&qr.rows[0][..7], "1111111");
    // Deterministic for the same input.
    assert_eq!(qr_matrix(&link).unwrap(), qr);
}
