// Wallet-link ceremony tests.
//
// The load-bearing one is `the approved signature recovers to the wallet`: it
// rebuilds the authority's message format byte-for-byte, drives a REAL ceremony
// over a REAL vault key, and recovers the signer from the resulting EIP-191
// signature. If that passes, the proof this flow produces is one the authority's
// `verifyWalletLinkProof` (ethers `verifyMessage`) will accept — which is the
// only property that decides whether `wallet_address` ever moves.

use super::*;
use crate::ceremony::SignatureCeremony;
use crate::custody::{CustodyVault, CustodyError, Keyring};
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

// In-memory keyring fake (mirrors custody_tests::FakeKeyring — each test module
// defines its own; the type is test-private per module).
#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<std::collections::HashMap<String, Vec<u8>>>,
}

impl Keyring for FakeKeyring {
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

const PASS: &[u8] = b"correct horse battery staple";
const CANONICAL_MNEMONIC: &str =
    "test test test test test test test test test test test junk";

/// The authority's template shape (identity-registry.ts `buildWalletLinkMessage`),
/// with the placeholder it serves before an address is known.
fn template(sub: &str, nonce: &str, chain_id: u64) -> String {
    [
        "auth.citrate.ai wants to link a wallet to your Citrate identity.".to_string(),
        String::new(),
        format!("Identity: {sub}"),
        format!("Wallet: {}", crate::oidc::WALLET_LINK_PLACEHOLDER),
        format!("Chain ID: {chain_id}"),
        format!("Nonce: {nonce}"),
        "Version: citrate-wallet-link-1".to_string(),
    ]
    .join("\n")
}

fn vault_with_wallet() -> (CustodyVault, PathBuf) {
    let mut p = std::env::temp_dir();
    let uniq = format!("citrate-core-walletlink-test-{}-{}.enc", std::process::id(), {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    });
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    crate::wallet::import(&v, CANONICAL_MNEMONIC).expect("import wallet");
    (v, p)
}

#[test]
fn substitutes_the_address_into_the_authority_template() {
    let t = template("u-1", "n-1", 40204);
    let msg = crate::oidc::wallet_link_message(&t, "0xAbCdEf0123456789AbCdEf0123456789AbCdEf01")
        .expect("substituted");
    // The authority lowercases the address when it builds the message; a proof
    // over a checksummed variant would be a proof over a different string.
    assert!(msg.contains("Wallet: 0xabcdef0123456789abcdef0123456789abcdef01"));
    assert!(!msg.contains(crate::oidc::WALLET_LINK_PLACEHOLDER));
}

#[test]
fn refuses_a_template_whose_format_changed() {
    // No placeholder → we cannot build the message the authority will verify
    // against. Fail closed rather than ask a human to approve a guess.
    assert!(crate::oidc::wallet_link_message("some other message", "0x00").is_none());
}

#[test]
fn open_refuses_an_unexpected_challenge_and_records_nothing() {
    let ceremony = SignatureCeremony::new();
    let state = WalletLinkState::new();
    let err = state
        .open(&ceremony, 40204, "https://auth.citrate.ai", "0xab", "n-1", "bad template")
        .unwrap_err();
    assert_eq!(err, LinkError::UnexpectedChallenge);
    assert_eq!(state.pending_count(), 0);
}

#[test]
fn open_shows_the_human_the_real_message_and_needs_no_raw_ack() {
    let (vault, _p) = vault_with_wallet();
    let info = crate::wallet::address(&vault).expect("address");
    let ceremony = SignatureCeremony::new();
    let state = WalletLinkState::new();

    let view = state
        .open(
            &ceremony,
            40204,
            "https://auth.citrate.ai",
            &info.address,
            "n-1",
            &template("u-1", "n-1", 40204),
        )
        .expect("opened");

    // The human must see what they are signing — a UTF-8 message decodes, so this
    // is never a blind raw-ack approval.
    assert!(!view.requires_raw_ack);
    assert!(view.decoded.action.contains("wants to link a wallet"));
    assert_eq!(view.decoded.cost, "no funds moved");
    assert_eq!(view.origin, "https://auth.citrate.ai");
    assert_eq!(state.pending_count(), 1);
}

#[test]
fn nothing_is_signed_until_the_human_approves() {
    let (vault, _p) = vault_with_wallet();
    let info = crate::wallet::address(&vault).expect("address");
    let ceremony = SignatureCeremony::new();
    let state = WalletLinkState::new();

    let view = state
        .open(
            &ceremony,
            40204,
            "https://auth.citrate.ai",
            &info.address,
            "n-1",
            &template("u-1", "n-1", 40204),
        )
        .expect("opened");

    // Rejecting yields no proof, and a later approve cannot resurrect it.
    ceremony.reject(&view.id).expect("reject");
    state.forget(&view.id);
    let err = state
        .approve(&vault, &ceremony, &view.id, false)
        .unwrap_err();
    assert_eq!(err, LinkError::UnknownLink);
}

#[test]
fn the_approved_signature_recovers_to_the_wallet() {
    let (vault, _p) = vault_with_wallet();
    let info = crate::wallet::address(&vault).expect("address");
    let ceremony = SignatureCeremony::new();
    let state = WalletLinkState::new();
    let t = template("u-1", "n-1", 40204);

    let view = state
        .open(
            &ceremony,
            40204,
            "https://auth.citrate.ai",
            &info.address,
            "n-1",
            &t,
        )
        .expect("opened");

    let proof = state
        .approve(&vault, &ceremony, &view.id, false)
        .expect("approved");

    assert_eq!(proof.nonce, "n-1");
    assert_eq!(proof.address, info.address);
    assert!(proof.signature.starts_with("0x"));
    // EIP-191 recoverable: r||s||v = 65 bytes = 130 hex chars.
    assert_eq!(proof.signature.len(), 2 + 130);

    // THE PROPERTY THAT MATTERS: the authority recovers the signer from this
    // signature over the message it rebuilds. Recompute both sides here.
    let message = crate::oidc::wallet_link_message(&t, &info.address).expect("message");
    let recovered = recover_personal_sign(&message, &proof.signature);
    assert_eq!(
        recovered.to_lowercase(),
        info.address.to_lowercase(),
        "the proof must recover to the linked wallet, or the authority rejects it"
    );
}

#[test]
fn one_approval_yields_one_proof() {
    let (vault, _p) = vault_with_wallet();
    let info = crate::wallet::address(&vault).expect("address");
    let ceremony = SignatureCeremony::new();
    let state = WalletLinkState::new();

    let view = state
        .open(
            &ceremony,
            40204,
            "https://auth.citrate.ai",
            &info.address,
            "n-1",
            &template("u-1", "n-1", 40204),
        )
        .expect("opened");

    state
        .approve(&vault, &ceremony, &view.id, false)
        .expect("approved");
    assert_eq!(state.pending_count(), 0);

    // Single-use, both halves: the ceremony is consumed AND the nonce is gone.
    let err = state
        .approve(&vault, &ceremony, &view.id, false)
        .unwrap_err();
    assert_eq!(err, LinkError::UnknownLink);
}

#[test]
fn a_locked_vault_cannot_produce_a_proof_and_keeps_the_link_retriable() {
    let (vault, _p) = vault_with_wallet();
    let info = crate::wallet::address(&vault).expect("address");
    let ceremony = SignatureCeremony::new();
    let state = WalletLinkState::new();

    let view = state
        .open(
            &ceremony,
            40204,
            "https://auth.citrate.ai",
            &info.address,
            "n-1",
            &template("u-1", "n-1", 40204),
        )
        .expect("opened");

    vault.lock();
    let err = state
        .approve(&vault, &ceremony, &view.id, false)
        .unwrap_err();
    assert!(matches!(err, LinkError::Ceremony(_)), "got {err:?}");
    // The ceremony consumed itself, so the pending half is dead too — but it must
    // not have signed anything.
    assert_ne!(err, LinkError::UnknownLink);
}

#[test]
fn two_links_do_not_cross_wires() {
    let (vault, _p) = vault_with_wallet();
    let info = crate::wallet::address(&vault).expect("address");
    let ceremony = SignatureCeremony::new();
    let state = WalletLinkState::new();

    let a = state
        .open(&ceremony, 40204, "o", &info.address, "n-A", &template("u", "n-A", 40204))
        .expect("a");
    let b = state
        .open(&ceremony, 40204, "o", &info.address, "n-B", &template("u", "n-B", 40204))
        .expect("b");
    assert_ne!(a.id, b.id);
    assert_eq!(state.pending_count(), 2);

    let proof_b = state.approve(&vault, &ceremony, &b.id, false).expect("b");
    assert_eq!(proof_b.nonce, "n-B", "each ceremony must carry ITS own nonce");

    let proof_a = state.approve(&vault, &ceremony, &a.id, false).expect("a");
    assert_eq!(proof_a.nonce, "n-A");
}

/// Recover the EIP-191 signer address from a `personal_sign` signature, the way
/// the authority's `verifyMessage` does.
fn recover_personal_sign(message: &str, signature_0x: &str) -> String {
    use k256::ecdsa::{RecoveryId, Signature as K256Sig, VerifyingKey};
    use sha3::{Digest, Keccak256};

    let sig_bytes = hex::decode(signature_0x.trim_start_matches("0x")).expect("hex");
    assert_eq!(sig_bytes.len(), 65, "r||s||v");
    let sig = K256Sig::from_slice(&sig_bytes[..64]).expect("sig");
    // EIP-191 `v` is 27/28; RecoveryId wants 0/1.
    let rec = RecoveryId::from_byte(sig_bytes[64].saturating_sub(27)).expect("recovery id");

    let prefixed = format!("\x19Ethereum Signed Message:\n{}{}", message.len(), message);
    let digest = Keccak256::digest(prefixed.as_bytes());

    let vk = VerifyingKey::recover_from_prehash(&digest, &sig, rec).expect("recover");
    let uncompressed = vk.to_encoded_point(false);
    let hash = Keccak256::digest(&uncompressed.as_bytes()[1..]);
    format!("0x{}", hex::encode(&hash[12..]))
}

// --- issue #11: 409 re-link disambiguation (finish_link, pure) ---------------
use crate::oidc::AuthError;

#[test]
fn fresh_link_then_canonical_ok_is_linked_and_canonical() {
    let r = finish_link("0xabc".into(), Ok(()), || Ok(())).expect("ok");
    assert!(r.linked && r.canonical);
    assert_eq!(r.address, "0xabc");
}

#[test]
fn fresh_link_canonical_failure_is_non_fatal_and_reported() {
    // A fresh link whose canonical promotion fails still linked — reported, not fatal.
    let r = finish_link("0xabc".into(), Ok(()), || Err(AuthError::Rejected(500))).expect("ok");
    assert!(r.linked);
    assert!(!r.canonical, "canonical promotion failure is reported, not thrown");
}

#[test]
fn relink_409_then_canonical_ok_is_idempotent_success() {
    // The wallet was already linked to THIS sub — set_canonical succeeds → success.
    let r = finish_link("0xabc".into(), Err(AuthError::Rejected(409)), || Ok(())).expect("ok");
    assert!(r.linked && r.canonical, "re-linking my own wallet succeeds idempotently");
}

#[test]
fn relink_409_but_canonical_refused_is_an_honest_conflict() {
    // 409 + the authority refuses canonical → the wallet belongs to ANOTHER identity.
    let err = finish_link("0xabc".into(), Err(AuthError::Rejected(409)), || {
        Err(AuthError::Rejected(403))
    })
    .unwrap_err();
    assert!(err.contains("different Citrate identity"), "honest conflict, got: {err}");
}

#[test]
fn other_submit_error_stays_a_hard_failure() {
    // A non-409 non-2xx (e.g. 500) must NOT be swallowed as idempotent — canonical is
    // never even attempted.
    let mut canonical_called = false;
    let err = finish_link("0xabc".into(), Err(AuthError::Rejected(500)), || {
        canonical_called = true;
        Ok(())
    })
    .unwrap_err();
    assert!(!canonical_called, "canonical not attempted on a hard submit failure");
    assert!(err.contains("authority"), "surfaces the authority failure, got: {err}");
}
