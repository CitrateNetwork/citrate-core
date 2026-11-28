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

// ---- L7-WP02: persistent pairing key + single-use ledger ------------------------------------
// Written red-first: `from_keyring`, the ledger and `Unavailable` were absent.

use crate::custody::CustodyError;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct MemKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
}

impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

struct DeadKeyring;

impl Keyring for DeadKeyring {
    fn get(&self, _a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> std::result::Result<(), CustodyError> {
        Ok(())
    }
}

/// A fresh directory for one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("cpair-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TempDir(d)
    }
    fn ledger(&self) -> PathBuf {
        self.0.join(LEDGER_FILE)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_pairing_key_is_the_same_after_restart() {
    let d = TempDir::new("same-key");
    let kr = MemKeyring::default();
    let first = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let second = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    assert_eq!(first.public_hex(), second.public_hex());
    assert!(first.persistent() && second.persistent());
    // A different keychain is a different machine.
    let other = PairIssuer::from_keyring(&MemKeyring::default(), &d.ledger()).unwrap();
    assert_ne!(other.public_hex(), first.public_hex());
}

#[test]
fn the_pairing_key_never_touches_disk_outside_the_keyring() {
    let d = TempDir::new("no-key-on-disk");
    let kr = MemKeyring::default();
    let mut iss = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let (_c, link) = issue(&mut iss);
    iss.redeem(&link, NOW + 1).unwrap();
    let _ = issue(&mut iss);
    let seed = kr
        .get(PAIRING_KEY_ACCOUNT)
        .unwrap()
        .expect("seed in the keychain");
    assert_eq!(seed.len(), 32);
    let seed_hex = hex::encode(&seed);
    for entry in std::fs::read_dir(&d.0).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(&seed_hex), "seed hex on disk");
        assert!(
            !bytes.windows(32).any(|w| w == seed.as_slice()),
            "seed bytes on disk"
        );
    }
    // Only nonces and times are in the ledger.
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(d.ledger()).unwrap()).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["outstanding", "used", "v"]);
}

#[cfg(unix)]
#[test]
fn the_ledger_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let d = TempDir::new("owner-only");
    let mut iss = PairIssuer::from_keyring(&MemKeyring::default(), &d.ledger()).unwrap();
    let _ = issue(&mut iss);
    let mode = std::fs::metadata(d.ledger()).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");
}

#[test]
fn a_redeemed_token_stays_used_after_restart() {
    let d = TempDir::new("used-after-restart");
    let kr = MemKeyring::default();
    let mut iss = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let (_c, link) = issue(&mut iss);
    assert!(iss.redeem(&link, NOW + 1).is_ok());
    drop(iss);
    let mut again = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    assert_eq!(again.redeem(&link, NOW + 2), Err(PairError::AlreadyUsed));
    drop(again);
    // And after another restart.
    let mut third = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    assert_eq!(third.redeem(&link, NOW + 3), Err(PairError::AlreadyUsed));
}

#[test]
fn an_outstanding_token_survives_restart_and_redeems_once() {
    let d = TempDir::new("outstanding-survives");
    let kr = MemKeyring::default();
    let mut iss = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let (claim, link) = issue(&mut iss);
    drop(iss);
    let mut after = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    assert_eq!(after.outstanding(NOW + 1), 1);
    assert_eq!(after.redeem(&link, NOW + 1).unwrap(), claim);
    assert_eq!(after.redeem(&link, NOW + 2), Err(PairError::AlreadyUsed));
}

#[test]
fn a_corrupt_ledger_fails_closed() {
    let d = TempDir::new("corrupt");
    let kr = MemKeyring::default();
    let mut iss = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let (_c, open) = issue(&mut iss);
    drop(iss);
    std::fs::write(d.ledger(), b"{not json").unwrap();
    let mut after = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    // Every earlier link is unknown, and the unreadable file is left as it is until a write.
    assert_eq!(after.redeem(&open, NOW + 1), Err(PairError::UnknownToken));
    assert_eq!(std::fs::read(d.ledger()).unwrap(), b"{not json");
    // Issuing still works, and that write replaces the unreadable file with a good one.
    let (_c, fresh) = issue(&mut after);
    assert!(after.redeem(&fresh, NOW + 1).is_ok());
    drop(after);
    let mut third = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    assert_eq!(third.redeem(&fresh, NOW + 2), Err(PairError::AlreadyUsed));
    // An oversized ledger is unreadable too.
    std::fs::write(d.ledger(), vec![b' '; 64 * 1024 + 1]).unwrap();
    let mut big = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    assert_eq!(big.outstanding(NOW), 0);
    assert_eq!(big.redeem(&fresh, NOW + 2), Err(PairError::UnknownToken));
}

#[test]
fn ledger_entries_expire_and_are_pruned() {
    let d = TempDir::new("prune");
    let kr = MemKeyring::default();
    let mut iss = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let (old_used, used_link) = issue(&mut iss);
    let (old_open, _open_link) = issue(&mut iss);
    iss.redeem(&used_link, NOW + 1).unwrap();
    // After both have expired, the next write drops them.
    let later = NOW + PAIR_TTL_SECS + 1;
    let (fresh, _l) = iss.issue("Studio", Some("T2"), hints(), later).unwrap();
    let text = String::from_utf8(std::fs::read(d.ledger()).unwrap()).unwrap();
    assert!(!text.contains(&old_used.nonce), "{text}");
    assert!(!text.contains(&old_open.nonce), "{text}");
    assert!(text.contains(&fresh.nonce), "{text}");
}

#[test]
fn the_pairing_key_is_not_the_device_key_or_comms_key() {
    assert_eq!(PAIRING_KEY_ACCOUNT, "fleet-pairing-key-v1");
    assert_ne!(PAIRING_KEY_ACCOUNT, crate::device_link::DEVICE_KEY_ACCOUNT);
    assert!(!PAIRING_KEY_ACCOUNT.contains("comms"));
    let d = TempDir::new("own-account");
    let kr = MemKeyring::default();
    let iss = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let store = kr.store.lock().unwrap();
    assert_eq!(store.keys().collect::<Vec<_>>(), [PAIRING_KEY_ACCOUNT]);
    // The pairing public key is ed25519 over the stored seed, nothing shared with another key.
    let seed: [u8; 32] = store[PAIRING_KEY_ACCOUNT].as_slice().try_into().unwrap();
    assert_eq!(PairIssuer::from_seed(seed).public_hex(), iss.public_hex());
}

#[test]
fn a_keychain_failure_is_an_error_not_a_silent_key() {
    let d = TempDir::new("dead-keychain");
    assert!(PairIssuer::from_keyring(&DeadKeyring, &d.ledger()).is_err());
    // A stored value of the wrong length is refused, never replaced.
    let kr = MemKeyring::default();
    kr.set(PAIRING_KEY_ACCOUNT, &[1u8; 16]).unwrap();
    assert!(PairIssuer::from_keyring(&kr, &d.ledger()).is_err());
    assert_eq!(kr.get(PAIRING_KEY_ACCOUNT).unwrap().unwrap(), vec![1u8; 16]);
    // The in-memory fallback says it is not persistent.
    assert!(!PairIssuer::new_random().persistent());
    assert!(!crate::fleet::pairing_issuer(&DeadKeyring, &d.ledger()).persistent());
    assert!(crate::fleet::pairing_issuer(&MemKeyring::default(), &d.ledger()).persistent());
}

#[test]
fn a_redeem_that_cannot_be_recorded_is_refused_and_the_token_stays_open() {
    let d = TempDir::new("unwritable");
    let kr = MemKeyring::default();
    let mut iss = PairIssuer::from_keyring(&kr, &d.ledger()).unwrap();
    let (claim, link) = issue(&mut iss);
    // Make the ledger path unwritable: a directory with content sits where the file goes.
    std::fs::remove_file(d.ledger()).unwrap();
    std::fs::create_dir_all(d.ledger().join("block")).unwrap();
    assert_eq!(iss.redeem(&link, NOW + 1), Err(PairError::Unavailable));
    assert_eq!(iss.outstanding(NOW + 1), 1, "the token is still open");
    // Once the ledger can be written again, the same link redeems once.
    std::fs::remove_dir_all(d.ledger()).unwrap();
    assert_eq!(iss.redeem(&link, NOW + 2).unwrap(), claim);
    assert_eq!(iss.redeem(&link, NOW + 3), Err(PairError::AlreadyUsed));
    assert!(!PairError::Unavailable.message().is_empty());
}
