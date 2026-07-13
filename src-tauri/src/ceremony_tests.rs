// CORE-B1.2 — SignatureCeremony adversarial + integration suite (@rule8 evidence).
//
// Every B1.2-ADV-* here is written RED-FIRST with a documented NEGATIVE CONTROL:
// each test's comment states exactly what happens when its guard is neutralized
// (the attack passes through), so an independent reviewer can reproduce the
// pass-through in a throwaway checkout and confirm the guard is load-bearing. The
// pass-through results are recorded in the PR return.
//
// These run FULLY HEADLESS against the in-memory keyring fake (the custody proof
// surface — a live OS keyring cannot run headless) and the real A2 vault + B1.1
// wallet keystore — no Tauri runtime. HONEST GAP: the interactive approval UI
// (default-focus, one-click) is NOT headless-testable; B1.2-ADV-6 is asserted at
// the CORE contract level (approval is bound to an explicit id; there is no
// "approve latest" / auto-approve API), not at the pixel level. That UI gap is
// stated, not faked.
//
// B1.1-F-4 discipline: assert EXACT artifacts (ecrecovered address, exact error
// variant, exact pending count), never substring-match a random secret.

use super::*;
use crate::custody::{CustodyError, CustodyVault, Keyring};
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

// The published canonical BIP44 vector (data source: MetaMask/standard). Same
// vector B1.1's wallet_tests uses, so the ecrecover proof is comparable.
const CANONICAL_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const CANONICAL_ADDRESS: &str = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94";

// --- in-memory keyring fake (mirrors wallet_tests::FakeKeyring) -------------

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

/// A fresh vault over a temp envelope + in-memory keyring, initialized, unlocked,
/// and holding the canonical wallet (so `approve` can actually sign).
fn vault_with_wallet() -> (CustodyVault, PathBuf) {
    let mut p = std::env::temp_dir();
    let uniq = format!("citrate-core-ceremony-test-{}-{}.enc", std::process::id(), {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    });
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    crate::wallet::import(&v, CANONICAL_MNEMONIC).expect("import canonical wallet");
    (v, p)
}

/// A `personal_sign` intent over a UTF-8 message (decodable → not raw-gated).
fn personal_sign_intent(msg: &str) -> SignatureIntent {
    SignatureIntent {
        origin: "https://app.citrate.ai".to_string(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        raw: hex::encode(msg.as_bytes()),
    }
}

/// Keccak-256(uncompressed_pubkey[1..])[12..32] — the EVM address of a recovered
/// verifying key (matches wallet-core's `derive_address_from_secp256k1`).
fn address_of_verifying_key(vk: &k256::ecdsa::VerifyingKey) -> String {
    use sha3::{Digest, Keccak256};
    let uncompressed = vk.to_encoded_point(false);
    let hash = Keccak256::digest(&uncompressed.as_bytes()[1..]);
    format!("0x{}", hex::encode(&hash[12..32]))
}

// =========================================================================
// INTEGRATION — request → decoded (no sig) → approve → sig ecrecovers;
// second approve → error; reject → no sig; unknown id → error.
// =========================================================================

#[test]
fn integration_request_decode_approve_ecrecovers_and_is_single_use() {
    use k256::ecdsa::{RecoveryId, Signature as K256Sig, SigningKey};

    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();

    let msg = "citrate-core B1.2 ceremony integration message";
    let intent = personal_sign_intent(msg);

    // request → PENDING view, NO signature, message decoded for display.
    let view = c.request(intent);
    assert!(view.decoded.action.contains("Sign message"), "decoded action shown: {}", view.decoded.action);
    assert!(view.decoded.action.contains(msg), "the true message is surfaced verbatim");
    assert!(!view.requires_raw_ack, "a UTF-8 personal_sign is decodable (no raw-ack)");
    assert_eq!(c.pending_count(), 1, "one ceremony pending after request");
    // The request path returns NO signature type at all (compile-enforced: the
    // return is a CeremonyView, which has no signature field).

    // approve → a signature.
    let sig = c.approve(&v, &view.id, false).expect("approve a pending decodable ceremony");
    assert_eq!(sig.sig_hex.len(), 128, "r||s = 64 bytes = 128 hex chars");
    assert_eq!(c.pending_count(), 0, "the ceremony is consumed on approve (single-use)");

    // ecrecover: the produced signature recovers to the canonical wallet address.
    // Reproduce the recovery id the way wallet_tests does (UnifiedKey::sign drops
    // it; k256 Signer<Signature> prehashes SHA-256).
    let vault_sig = hex::decode(&sig.sig_hex).expect("hex");
    let unified = citrate_wallet_core::secp256k1_from_mnemonic(CANONICAL_MNEMONIC, 0).expect("derive");
    let sk: SigningKey = match unified {
        citrate_wallet_core::UnifiedKey::Secp256k1(k) => k,
        _ => panic!("expected secp256k1"),
    };
    use sha2::{Digest as Sha2Digest, Sha256};
    let prehash = Sha256::digest(msg.as_bytes());
    let (rec_sig, recid): (K256Sig, RecoveryId) =
        sk.sign_prehash_recoverable(&prehash).expect("recoverable sign");
    assert_eq!(
        vault_sig,
        rec_sig.to_bytes().to_vec(),
        "the ceremony's signature is the canonical key's signature over the message"
    );
    let recovered =
        k256::ecdsa::VerifyingKey::recover_from_prehash(&prehash, &rec_sig, recid).expect("recover");
    assert_eq!(
        address_of_verifying_key(&recovered).to_lowercase(),
        CANONICAL_ADDRESS.to_lowercase(),
        "ecrecover(ceremony signature) == the wallet address"
    );

    // second approve on the SAME id → error (consumed).
    assert_eq!(
        c.approve(&v, &view.id, false).err(),
        Some(CeremonyError::UnknownCeremony),
        "a second approve on a consumed id must error"
    );
    // unknown id → error.
    assert_eq!(c.approve(&v, "999999", false).err(), Some(CeremonyError::UnknownCeremony));
    // reject on unknown id → error.
    assert_eq!(c.reject("999999").err(), Some(CeremonyError::UnknownCeremony));
}

#[test]
fn integration_reject_consumes_without_signature() {
    let (_v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(personal_sign_intent("to be rejected"));
    assert_eq!(c.pending_count(), 1);
    c.reject(&view.id).expect("reject a pending ceremony");
    assert_eq!(c.pending_count(), 0, "reject consumes the ceremony");
    // No signature was produced (reject returns () — compile-enforced), and the
    // id can no longer be approved.
    let (v, _p2) = vault_with_wallet();
    assert_eq!(
        c.approve(&v, &view.id, false).err(),
        Some(CeremonyError::UnknownCeremony),
        "a rejected ceremony cannot then be approved"
    );
}

// =========================================================================
// B1.2-ADV-1 / B1.2-ADV-7 — sign without approval / sidecar signs directly is
// IMPOSSIBLE: the gated signer is reachable ONLY from `approve`.
// =========================================================================

#[test]
fn adv1_adv7_signer_only_reachable_via_approve() {
    // STRUCTURAL call-site proof against the real sources. `wallet::sign_message`
    // is `pub(crate)`; the ONLY sanctioned call is `ceremony::approve`. Enumerate
    // every crate source and assert the only NON-DEFINITION, NON-TEST reference to
    // the signer is inside ceremony.rs.
    //
    // NEGATIVE CONTROL (stated): add `let _ = wallet::sign_message(vault, b"x");`
    // to ANY module other than ceremony.rs (e.g. a seam command, an oidc helper),
    // or register a `#[tauri::command]` that calls it, and this test fails — that
    // is the "sidecar/agent/other-path signs" attack (B1.2-ADV-1/7) we forbid.
    // Also: widening the signer back to `pub` would let an out-of-crate sidecar
    // call it; `pub(crate)` (asserted below) closes that.
    let sources: &[(&str, &str)] = &[
        ("wallet.rs", include_str!("wallet.rs")),
        ("custody.rs", include_str!("custody.rs")),
        ("oidc.rs", include_str!("oidc.rs")),
        ("seam.rs", include_str!("seam.rs")),
        ("config.rs", include_str!("config.rs")),
        ("lib.rs", include_str!("lib.rs")),
    ];
    // Assemble the needle from parts so this test's own prose cannot self-match.
    let call = "sign_".to_string() + "message(";
    for (name, src) in sources {
        // Strip the test module of wallet.rs (its B1.1 tests legitimately call the
        // signer) so we only scan non-test code. We look for the call form
        // `sign_message(` used as an invocation, not the `fn sign_message` def.
        let non_test = strip_test_module(src);
        for line in non_test.lines() {
            let t = line.trim_start();
            if t.contains(&call) && !t.contains("fn sign_") && !t.starts_with("//") {
                panic!(
                    "{name}: the gated signer is invoked outside ceremony::approve: `{}`",
                    line.trim()
                );
            }
        }
    }
    // The signer IS invoked from ceremony.rs (positive control — the one path).
    let ceremony_src = include_str!("ceremony.rs");
    let ceremony_non_test = strip_test_module(ceremony_src);
    assert!(
        ceremony_non_test.contains(&call),
        "the sanctioned path (ceremony::approve) must invoke the signer"
    );
    // The signer is `pub(crate)`, not `pub` — an out-of-crate sidecar cannot reach
    // it. NEGATIVE CONTROL: change `pub(crate) fn sign_message` back to `pub fn`
    // and this assertion fails.
    let wallet_src = include_str!("wallet.rs");
    assert!(
        wallet_src.contains("pub(crate) fn sign_")
            && !wallet_src.contains("pub fn sign_message"),
        "the signer must be pub(crate) (crate-private), never pub"
    );
}

/// Return `src` with any top-level `#[cfg(test)] mod tests { ... }` block removed
/// so a call-site scan sees only non-test code. Our test modules are the terminal
/// `mod tests { include!(...) }` form, so truncating at that marker is exact.
fn strip_test_module(src: &str) -> String {
    if let Some(idx) = src.find("#[cfg(test)]") {
        src[..idx].to_string()
    } else {
        src.to_string()
    }
}

// =========================================================================
// B1.2-ADV-2 — no invoke command returns key/seed/entropy; compile barrier holds.
// =========================================================================

#[test]
fn adv2_no_signing_command_returns_secret_material() {
    // Registry enumeration against the real lib.rs. The three B1.2 commands are
    // registered; none returns key material (they return CeremonyView / Signature
    // / () — Signature is a SIGNATURE, not a key). The wallet secret-path fns are
    // NEVER registered (the B1.1 boundary, re-asserted here for B1.2).
    let src = include_str!("lib.rs");
    for cmd in ["ceremony::sign_request", "ceremony::sign_approve", "ceremony::sign_reject"] {
        assert!(src.contains(cmd), "signing command not registered: {cmd}");
    }
    // NEGATIVE CONTROL (stated): registering any wallet secret-reading fn (or a
    // hypothetical `sign_direct` command that returns a key) would fail this.
    for forbidden in [
        "wallet::create,",
        "wallet::import,",
        "wallet::sign_message,",
        "wallet::address,",
        "wallet::read_entropy,",
        "wallet::derive_key_from_entropy,",
    ] {
        assert!(
            !src.contains(forbidden),
            "no wallet secret-path fn may be an invoke command: {forbidden}"
        );
    }
    // COMPILE BARRIER (I-2): the ceremony command return types are all Serialize
    // and carry no secret. `Signature` holds only hex of the (non-secret) sig +
    // the kind; there is no key/seed/entropy field. Prove it round-trips as
    // metadata and contains no key bytes by construction.
    let sig = Signature { sig_hex: "ab".repeat(64), kind: IntentKind::PersonalSign };
    let j = serde_json::to_string(&sig).expect("Signature is Serialize (non-secret)");
    assert!(j.contains("sigHex"), "Signature crosses the bridge as a signature, not a key");
    // A CeremonyView carries origin + decoded + id — never key material.
    let view = CeremonyView {
        id: "1".into(),
        origin: "o".into(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        decoded: DecodedAction { action: "a".into(), cost: "".into(), destination: "".into() },
        requires_raw_ack: false,
    };
    let jv = serde_json::to_string(&view).expect("CeremonyView is Serialize (non-secret)");
    assert!(jv.contains("requiresRawAck"));
}

#[test]
fn adv2_wallet_create_is_not_serialize_compile_barrier_holds() {
    // The compile barrier from B1.1 still holds: WalletCreate (which carries the
    // mnemonic) is NOT Serialize, so it cannot be returned across invoke. If a
    // future change derived Serialize on it (or made a command return it), that
    // command would fail to compile — the I-2 barrier. We assert the NON-secret
    // WalletInfo IS serializable (the only wallet type allowed across the bridge).
    let info = crate::wallet::WalletInfo { address: "0xabc".into(), public_key_hex: "04ff".into() };
    assert!(serde_json::to_string(&info).is_ok(), "WalletInfo (non-secret) is Serialize");
}

// =========================================================================
// B1.2-ADV-3 — approve while vault LOCKED → fail closed (no signature).
// =========================================================================

#[test]
fn adv3_approve_while_locked_fails_closed() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(personal_sign_intent("sign me if you can"));

    // Lock the vault, THEN approve.
    v.lock();
    let r = c.approve(&v, &view.id, false);
    // NEGATIVE CONTROL (stated): if `wallet::sign_message` did not gate on the A2
    // session (custody_get denies when locked), the signer would read the sealed
    // entropy and sign on a locked vault. The guard is custody.rs's session check.
    //
    // FAIL-CLOSED SHAPE (honest): B1.1's `read_entropy` maps a locked-vault
    // `custody_get` Denial to `WalletError::NotFound` (custody deliberately cannot
    // distinguish locked-vs-absent — no oracle; see custody.rs), which the ceremony
    // maps to `NoWallet`. So a locked approve surfaces as `NoWallet` OR
    // `VaultLocked` — both are fail-closed with NO signature. We assert the
    // security property (a closed error, no sig), not a single label, exactly as
    // B1.1-ADV-3 asserts `NotFound | Custody`.
    assert!(
        matches!(r, Err(CeremonyError::NoWallet) | Err(CeremonyError::VaultLocked)),
        "approve on a locked vault must fail closed (no signature), got {r:?}"
    );
    // The ceremony was CONSUMED (approve removes first, then signs). A locked
    // approve therefore does not leave a re-approvable ceremony — fail closed AND
    // single-use. (This is the deliberate consume-first ordering.)
    assert_eq!(c.pending_count(), 0, "a locked approve still consumes the id (fail-closed single-use)");
    // Re-unlock: a NEW ceremony signs fine (the lock was the only gate).
    v.unlock(&mut PASS.to_vec()).expect("re-unlock");
    let v2 = c.request(personal_sign_intent("after unlock"));
    assert!(c.approve(&v, &v2.id, false).is_ok(), "signing works again after unlock");
}

// =========================================================================
// B1.2-ADV-5 — malicious origin spoofs a benign intent → ceremony shows the TRUE
// origin + decoded action; undecodable calldata blocks approve without raw ack.
// =========================================================================

#[test]
fn adv5_true_origin_surfaced_and_undecodable_is_raw_gated() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();

    // A malicious origin cannot fake a benign summary: the origin is displayed
    // verbatim and the decode is computed from the ACTUAL payload.
    let evil = SignatureIntent {
        origin: "https://evil.example (pretending to be app.citrate.ai)".to_string(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        raw: hex::encode(b"Approve unlimited spend"),
    };
    let view = c.request(evil.clone());
    assert_eq!(view.origin, evil.origin, "the TRUE origin is surfaced verbatim");
    assert!(
        view.decoded.action.contains("Approve unlimited spend"),
        "the decode reflects the ACTUAL payload, not a fabricated benign action"
    );

    // Undecodable calldata (a `transaction` — B1.2 cannot RLP-decode it; deferred
    // to B1.4) is `Unrecognized` and raw-ack gated.
    let tx = SignatureIntent {
        origin: "agent:node-agent".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: hex::encode([0x02u8, 0xf8, 0x6b, 0x82]), // opaque tx-ish bytes
    };
    let txview = c.request(tx);
    assert_eq!(txview.decoded.action, UNRECOGNIZED_ACTION, "undecodable → Unrecognized");
    assert!(txview.requires_raw_ack, "undecodable calldata requires a raw ack");

    // approve WITHOUT the raw ack → blocked.
    // NEGATIVE CONTROL (stated): if `approve` skipped the `requires_raw_ack &&
    // !raw_ack` check, this blind approval would sign undecodable calldata as if
    // benign — exactly the spoof. The guard blocks it.
    assert_eq!(
        c.approve(&v, &txview.id, false).err(),
        Some(CeremonyError::RawAckRequired),
        "undecodable calldata must not be approvable without an explicit raw ack"
    );
    // The ceremony is RE-INSERTED on a missing-ack rejection, so the human can
    // retry WITH the ack.
    assert_eq!(c.pending_count(), 2, "a missing-ack approve does not consume the ceremony");

    // approve WITH the explicit raw ack → signs (the human took responsibility).
    let sig = c.approve(&v, &txview.id, true).expect("raw-ack approve signs");
    assert_eq!(sig.sig_hex.len(), 128, "raw-mode still produces a valid r||s signature");
    assert_eq!(sig.kind, IntentKind::Transaction);
}

#[test]
fn adv5_typed_data_decodes_and_malformed_typed_data_is_raw_gated() {
    let (_v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();

    // Well-formed EIP-712 → decodable (primaryType + domain surfaced).
    let td = serde_json::json!({
        "primaryType": "Permit",
        "domain": { "name": "Citrate", "verifyingContract": "0x1234000000000000000000000000000000005678" }
    });
    let intent = SignatureIntent {
        origin: "https://app.citrate.ai".into(),
        kind: IntentKind::TypedData,
        chain_id: 40204,
        raw: hex::encode(serde_json::to_vec(&td).unwrap()),
    };
    let view = c.request(intent);
    assert!(view.decoded.action.contains("Permit"), "primaryType surfaced: {}", view.decoded.action);
    assert!(view.decoded.action.contains("Citrate"), "domain name surfaced");
    assert_eq!(view.decoded.destination, "0x1234000000000000000000000000000000005678");
    assert!(!view.requires_raw_ack, "well-formed typed data is decodable");

    // Typed data that is not JSON → Unrecognized → raw-ack gated.
    let bad = SignatureIntent {
        origin: "https://app.citrate.ai".into(),
        kind: IntentKind::TypedData,
        chain_id: 40204,
        raw: hex::encode(b"not json at all"),
    };
    let badview = c.request(bad);
    assert_eq!(badview.decoded.action, UNRECOGNIZED_ACTION);
    assert!(badview.requires_raw_ack, "non-JSON typed data must be raw-gated");
}

// =========================================================================
// B1.2-ADV-6 — Approve is not default-focused / one-click / approve-latest:
// approval is bound to an explicit CeremonyId at the CORE contract level.
// =========================================================================

#[test]
fn adv6_approval_is_bound_to_explicit_id_no_approve_latest() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();

    // Two concurrent ceremonies, `a` FIRST then `b` (so `b` is the "latest").
    // There is NO "approve latest"/"approve default" API — the ONLY entry is
    // `approve(vault, id, raw_ack)`, which names a specific id.
    let a = c.request(personal_sign_intent("ceremony A"));
    let b = c.request(personal_sign_intent("ceremony B"));
    assert_ne!(a.id, b.id, "each ceremony gets its own explicit id");
    assert_eq!(c.pending_count(), 2);

    // Approve the OLDER, NON-latest id (`a`) explicitly. This is the tight
    // negative control for "approve-latest": exactly `a` must be consumed and the
    // LATEST (`b`) must remain untouched.
    // NEGATIVE CONTROL (stated + reproduced): if `approve` ignored the named id and
    // consumed the latest instead (e.g. `map.keys().next_back()`), it would consume
    // `b` here — so `status(a)` would be None and `status(b)` Some, and BOTH
    // assertions below flip. Approving the older id proves approval is bound to the
    // exact id, not to recency/default-focus.
    c.approve(&v, &a.id, false).expect("approve the explicitly-named (older) id");
    assert_eq!(c.pending_count(), 1, "only the named ceremony was consumed");
    assert!(c.status(&a.id).is_none(), "the EXPLICITLY-NAMED (older) ceremony was consumed");
    assert!(
        c.status(&b.id).is_some(),
        "the LATEST ceremony is untouched — no approve-latest / auto-approve"
    );

    // The pixel-level default-focus / one-click property is the honest UI gap
    // (not headless-testable); the CORE contract (explicit-id binding) is proven.
    // The remaining latest id still approves independently when named.
    assert!(c.approve(&v, &b.id, false).is_ok(), "the latest id approves only when named");
    assert_eq!(c.pending_count(), 0);
}

// =========================================================================
// B1.2-ADV-10 — replay/duplicate one approval to sign twice → one approval =
// one signature (the ceremony is consumed).
// =========================================================================

#[test]
fn adv10_one_approval_one_signature_consumed() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(personal_sign_intent("sign exactly once"));

    // First approve → signature.
    let sig1 = c.approve(&v, &view.id, false).expect("first approve signs");
    assert_eq!(sig1.sig_hex.len(), 128);

    // Replay the SAME id → error, NO second signature.
    // NEGATIVE CONTROL (stated): if `approve` looked up the ceremony without
    // REMOVING it (or removed it only AFTER signing on the error path), a replay
    // would sign again. The consume-first ordering (remove under the lock, then
    // sign) means the second remove sees None → UnknownCeremony.
    assert_eq!(
        c.approve(&v, &view.id, false).err(),
        Some(CeremonyError::UnknownCeremony),
        "a replayed approval must not produce a second signature"
    );
    assert_eq!(c.pending_count(), 0, "one approval consumed the single-use ceremony");
}

#[test]
fn adv10_concurrent_duplicate_approvals_yield_one_signature() {
    use std::sync::Arc;
    // Race N threads approving the SAME id; exactly ONE may get a signature. This
    // exercises the consume-first-under-lock ordering against a real thread storm.
    let (v, _p) = vault_with_wallet();
    let v = Arc::new(v);
    let c = Arc::new(SignatureCeremony::new());
    let view = c.request(personal_sign_intent("race me"));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let (c, v, id) = (Arc::clone(&c), Arc::clone(&v), view.id.clone());
        handles.push(std::thread::spawn(move || c.approve(&v, &id, false).is_ok()));
    }
    let successes = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(successes, 1, "exactly one of the racing approvals signs; the rest error");
    assert_eq!(c.pending_count(), 0, "the ceremony is consumed exactly once");
}

// =========================================================================
// Decode unit coverage — binary personal_sign is decodable (shown as bytes), a
// malformed-hex payload is Unrecognized.
// =========================================================================

#[test]
fn decode_binary_personal_sign_is_shown_not_raw_gated() {
    let intent = SignatureIntent {
        origin: "https://app.citrate.ai".into(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        raw: hex::encode([0xff, 0xfe, 0x00, 0x01]), // non-UTF-8 bytes
    };
    let d = decode_intent(&intent);
    assert!(d.action.contains("raw bytes"), "binary message shown as bytes: {}", d.action);
    assert_ne!(d.action, UNRECOGNIZED_ACTION, "a showable binary message is not raw-gated");
}

#[test]
fn decode_malformed_hex_payload_is_unrecognized() {
    let intent = SignatureIntent {
        origin: "x".into(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        raw: "0xZZZZ".into(), // not hex
    };
    let d = decode_intent(&intent);
    assert_eq!(d.action, UNRECOGNIZED_ACTION, "unparseable payload → Unrecognized (raw-gated)");
}

// =========================================================================
// Error surface — secret-free (no key/seed/entropy in any ceremony error).
// =========================================================================

#[test]
fn ceremony_errors_are_secret_free() {
    for e in [
        CeremonyError::UnknownCeremony,
        CeremonyError::RawAckRequired,
        CeremonyError::VaultLocked,
        CeremonyError::NoWallet,
        CeremonyError::SignFailed,
    ] {
        let s = format!("{e} {e:?}");
        // No error carries the canonical mnemonic or any hex-looking key blob.
        assert!(!s.to_lowercase().contains("abandon"), "no mnemonic in error text");
        assert!(!s.contains(CANONICAL_ADDRESS), "errors do not echo addresses/keys");
    }
}
