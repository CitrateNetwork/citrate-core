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
    let uniq = format!(
        "citrate-core-ceremony-test-{}-{}.enc",
        std::process::id(),
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            N.fetch_add(1, Ordering::Relaxed)
        }
    );
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    crate::wallet::import(&v, CANONICAL_MNEMONIC).expect("import canonical wallet");
    (v, p)
}

/// A fresh vault over a temp envelope + in-memory keyring, initialized and
/// unlocked, but with NO wallet stored (for the B1.5-R2 absent-slot contrast).
fn vault_no_wallet() -> (CustodyVault, PathBuf) {
    let mut p = std::env::temp_dir();
    let uniq = format!(
        "citrate-core-ceremony-nowallet-{}-{}.enc",
        std::process::id(),
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            N.fetch_add(1, Ordering::Relaxed)
        }
    );
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
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
    use k256::ecdsa::{RecoveryId, Signature as K256Sig};

    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();

    let msg = "citrate-core B1.2 ceremony integration message";
    let intent = personal_sign_intent(msg);

    // request → PENDING view, NO signature, message decoded for display.
    let view = c.request(intent);
    assert!(
        view.decoded.action.contains("Sign message"),
        "decoded action shown: {}",
        view.decoded.action
    );
    assert!(
        view.decoded.action.contains(msg),
        "the true message is surfaced verbatim"
    );
    assert!(
        !view.requires_raw_ack,
        "a UTF-8 personal_sign is decodable (no raw-ack)"
    );
    assert_eq!(c.pending_count(), 1, "one ceremony pending after request");
    // The request path returns NO signature type at all (compile-enforced: the
    // return is a CeremonyView, which has no signature field).

    // approve → a signature.
    let sig = c
        .approve(&v, &view.id, false)
        .expect("approve a pending decodable ceremony");
    // 65 bytes: r||s||v. This asserted 128 hex chars (64 bytes, r||s) until
    // `personal_sign` became a real EIP-191 signature — the old form carried no
    // recovery id, so nothing outside this process could tell who signed. The
    // recovery below is the property that changed; the length is just its shadow.
    assert_eq!(sig.sig_hex.len(), 130, "r||s||v = 65 bytes = 130 hex chars");
    assert_eq!(
        c.pending_count(),
        0,
        "the ceremony is consumed on approve (single-use)"
    );

    // ecrecover, exactly as an external verifier would: EIP-191 prehash, the
    // recovery id carried IN the signature (v - 27), no knowledge of our key.
    // Previously this test had to re-derive the key from the canonical mnemonic
    // and re-sign to obtain a recovery id, because the ceremony's signature did
    // not carry one — which meant it proved the signature matched a key we
    // already held, not that a stranger could identify the signer.
    let raw = hex::decode(&sig.sig_hex).expect("hex");
    let prehash = crate::wallet::eip191_prehash(msg.as_bytes());
    let recid = RecoveryId::from_byte(raw[64] - 27).expect("v is 27/28");
    let rec_sig = K256Sig::from_slice(&raw[..64]).expect("r||s");
    let recovered = k256::ecdsa::VerifyingKey::recover_from_prehash(&prehash, &rec_sig, recid)
        .expect("recover");
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
    assert_eq!(
        c.approve(&v, "999999", false).err(),
        Some(CeremonyError::UnknownCeremony)
    );
    // reject on unknown id → error.
    assert_eq!(
        c.reject("999999").err(),
        Some(CeremonyError::UnknownCeremony)
    );
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
    // STRUCTURAL call-site proof against the real sources. BOTH gated signers —
    // `wallet::sign_message` (B1.2) AND `wallet::sign_transaction` (B1.4) — are
    // `pub(crate)`; the ONLY sanctioned call sites are in `ceremony.rs`
    // (`approve` signs messages; `approve_and_broadcast` signs transactions).
    // Enumerate every crate source and assert the only NON-DEFINITION, NON-TEST
    // reference to EITHER signer is inside ceremony.rs.
    //
    // F-1 (B1.5, from citrate-security #23 / B1.4 review): the B1.4 review noted
    // this guard covered ONLY `sign_message`, leaving `sign_transaction`
    // structurally ungated. Both signers must be reachable ONLY from ceremony
    // approval. This test now scans for BOTH so the tx signer cannot be invoked
    // from a sidecar/agent/command bypass either.
    //
    // NEGATIVE CONTROL (stated + reproduced in the PR return): add
    // `let _ = wallet::sign_message(vault, b"x");` OR
    // `let _ = wallet::sign_transaction(vault, &fields, 40204);` to ANY module
    // other than ceremony.rs (a seam command, an oidc helper), or register a
    // `#[tauri::command]` that calls either, and this test fails — that is the
    // "sidecar/agent/other-path signs" attack (ADV-1/ADV-7) we forbid. Also:
    // widening EITHER signer back to `pub` would let an out-of-crate sidecar call
    // it; `pub(crate)` (asserted below) closes that.
    // WP-S1.2 (kit extraction): this scan now covers the KIT modules. The app
    // modules that used to be scanned here (seam.rs, agent.rs, node.rs, lib.rs)
    // moved to a SEPARATE crate (citrate-core), and the gated signers are
    // `pub(crate)` to THIS crate — so app code CANNOT invoke `wallet::sign_*` at
    // all: the compiler rejects it, a strictly stronger guarantee than this grep.
    // The node-agent bridge (agent.rs) additionally keeps its own ADV-7 source
    // scan in citrate-core's `agent_tests.rs`
    // (`adv7_agent_module_never_calls_the_gated_signer`), and the lib.rs command-
    // registry scan moved to citrate-core's `lib.rs` test module. Coverage is
    // preserved and hardened, not dropped.
    let sources: &[(&str, &str)] = &[
        ("wallet.rs", include_str!("wallet.rs")),
        ("custody.rs", include_str!("custody.rs")),
        ("oidc.rs", include_str!("oidc.rs")),
        ("config.rs", include_str!("config.rs")),
        ("rpc.rs", include_str!("rpc.rs")),
        ("txdecode.rs", include_str!("txdecode.rs")),
        ("supervisor.rs", include_str!("supervisor.rs")),
    ];
    // Assemble each needle from parts so this test's own prose cannot self-match.
    // BOTH signer invocation forms are forbidden outside ceremony.rs.
    let calls = [
        "sign_".to_string() + "message(",
        "sign_".to_string() + "transaction(",
        // The EIP-191 personal_sign signer is gated identically. Added when it
        // landed: a third signer that nobody scanned for would be the obvious way
        // to reintroduce exactly the bypass this test exists to forbid.
        "sign_".to_string() + "personal(",
    ];
    for (name, src) in sources {
        // Strip the test module (wallet.rs's B1.1/B1.4 tests legitimately call the
        // signers) so we only scan non-test code. We look for the call forms
        // `sign_message(` / `sign_transaction(` used as invocations, not the
        // `fn sign_message` / `fn sign_transaction` defs.
        let non_test = strip_test_module(src);
        for line in non_test.lines() {
            let t = line.trim_start();
            for call in &calls {
                if t.contains(call) && !t.contains("fn sign_") && !t.starts_with("//") {
                    panic!(
                        "{name}: a gated signer is invoked outside ceremony approval: `{}`",
                        line.trim()
                    );
                }
            }
        }
    }
    // BOTH signers ARE invoked from ceremony.rs (positive control — the one path
    // each: approve → sign_message, approve_and_broadcast → sign_transaction).
    let ceremony_src = include_str!("ceremony.rs");
    let ceremony_non_test = strip_test_module(ceremony_src);
    for call in &calls {
        assert!(
            ceremony_non_test.contains(call),
            "the sanctioned path (ceremony approval) must invoke the gated signer `{call}`"
        );
    }
    // BOTH signers are `pub(crate)`, not `pub` — an out-of-crate sidecar cannot
    // reach either. NEGATIVE CONTROL: change `pub(crate) fn sign_message` or
    // `pub(crate) fn sign_transaction` back to `pub fn` and this fails.
    let wallet_src = include_str!("wallet.rs");
    assert!(
        wallet_src.contains("pub(crate) fn sign_message")
            && !wallet_src.contains("pub fn sign_message"),
        "the message signer must be pub(crate) (crate-private), never pub"
    );
    assert!(
        wallet_src.contains("pub(crate) fn sign_transaction")
            && !wallet_src.contains("pub fn sign_transaction"),
        "the transaction signer must be pub(crate) (crate-private), never pub"
    );
    assert!(
        wallet_src.contains("pub(crate) fn sign_personal")
            && !wallet_src.contains("pub fn sign_personal"),
        "the personal_sign signer must be pub(crate) (crate-private), never pub"
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
    // NOTE (WP-S1.2): the registry-enumeration assertions (the three B1.2 signing
    // commands ARE registered; no wallet secret-path fn IS registered) moved to
    // citrate-core's lib.rs test module (`signing_commands_are_registered` +
    // `no_wallet_secret_path_fn_is_an_invoke_command`) — the generate_handler!
    // registry lives in the app crate after the kit extraction. This test keeps
    // the COMPILE-BARRIER / secret-free-return-type assertions below, which are
    // properties of the kit's own ceremony types.
    //
    // COMPILE BARRIER (I-2): the ceremony command return types are all Serialize
    // and carry no secret. `Signature` holds only hex of the (non-secret) sig +
    // the kind; there is no key/seed/entropy field. Prove it round-trips as
    // metadata and contains no key bytes by construction.
    let sig = Signature {
        sig_hex: "ab".repeat(64),
        kind: IntentKind::PersonalSign,
    };
    let j = serde_json::to_string(&sig).expect("Signature is Serialize (non-secret)");
    assert!(
        j.contains("sigHex"),
        "Signature crosses the bridge as a signature, not a key"
    );
    // A CeremonyView carries origin + decoded + id — never key material.
    let view = CeremonyView {
        id: "1".into(),
        origin: "o".into(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        decoded: DecodedAction {
            action: "a".into(),
            cost: "".into(),
            destination: "".into(),
        },
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
    let info = crate::wallet::WalletInfo {
        address: "0xabc".into(),
        public_key_hex: "04ff".into(),
    };
    assert!(
        serde_json::to_string(&info).is_ok(),
        "WalletInfo (non-secret) is Serialize"
    );
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
        matches!(
            r,
            Err(CeremonyError::NoWallet) | Err(CeremonyError::VaultLocked)
        ),
        "approve on a locked vault must fail closed (no signature), got {r:?}"
    );
    // The ceremony was CONSUMED (approve removes first, then signs). A locked
    // approve therefore does not leave a re-approvable ceremony — fail closed AND
    // single-use. (This is the deliberate consume-first ordering.)
    assert_eq!(
        c.pending_count(),
        0,
        "a locked approve still consumes the id (fail-closed single-use)"
    );
    // Re-unlock: a NEW ceremony signs fine (the lock was the only gate).
    v.unlock(&mut PASS.to_vec()).expect("re-unlock");
    let v2 = c.request(personal_sign_intent("after unlock"));
    assert!(
        c.approve(&v, &v2.id, false).is_ok(),
        "signing works again after unlock"
    );
}

#[test]
fn b1_5_r2_locked_approve_surfaces_vault_locked_not_no_wallet() {
    // B1.2-R2 (CLOSED in B1.5): a LOCKED-vault approve on a vault that HAS a
    // wallet now surfaces the crisper `VaultLocked`, not `NoWallet`. Pre-B1.5,
    // `read_entropy` mapped a locked-vault `custody_get` Denial to `NotFound` →
    // `NoWallet` (no locked-vs-absent oracle in custody), so the user got a
    // misleading "no wallet" hint on a locked vault. B1.5 consults the
    // passphrase-INDEPENDENT `is_unlocked()` (no new oracle) so locked → Custody
    // → VaultLocked. Still fail-closed (no signature).
    //
    // NEGATIVE CONTROL (stated): revert `read_entropy`'s is_unlocked() branch
    // (map every Denied → NotFound) and this asserts NoWallet instead — the exact
    // R2 mislabel. The security property (a closed error, no sig) is unchanged;
    // only the label improves.
    let (v, _p) = vault_with_wallet(); // a wallet IS stored
    let c = SignatureCeremony::new();
    let view = c.request(personal_sign_intent("lock then approve"));
    v.lock();
    let r = c.approve(&v, &view.id, false);
    assert_eq!(
        r.err(),
        Some(CeremonyError::VaultLocked),
        "a locked approve on a wallet-bearing vault must surface VaultLocked (R2), not NoWallet"
    );
    // Contrast: a vault with NO wallet, UNLOCKED, still surfaces NoWallet (the
    // is_unlocked() branch correctly distinguishes absent-slot from locked).
    let (empty, _pe) = vault_no_wallet();
    let c2 = SignatureCeremony::new();
    let ev = c2.request(personal_sign_intent("no wallet here"));
    assert_eq!(
        c2.approve(&empty, &ev.id, false).err(),
        Some(CeremonyError::NoWallet),
        "an UNLOCKED vault with no wallet stored must surface NoWallet (absent slot)"
    );
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
    assert_eq!(
        view.origin, evil.origin,
        "the TRUE origin is surfaced verbatim"
    );
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
    assert_eq!(
        txview.decoded.action, UNRECOGNIZED_ACTION,
        "undecodable → Unrecognized"
    );
    assert!(
        txview.requires_raw_ack,
        "undecodable calldata requires a raw ack"
    );

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
    assert_eq!(
        c.pending_count(),
        2,
        "a missing-ack approve does not consume the ceremony"
    );

    // approve WITH the explicit raw ack → signs (the human took responsibility).
    let sig = c
        .approve(&v, &txview.id, true)
        .expect("raw-ack approve signs");
    assert_eq!(
        sig.sig_hex.len(),
        128,
        "raw-mode still produces a valid r||s signature"
    );
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
    assert!(
        view.decoded.action.contains("Permit"),
        "primaryType surfaced: {}",
        view.decoded.action
    );
    assert!(
        view.decoded.action.contains("Citrate"),
        "domain name surfaced"
    );
    assert_eq!(
        view.decoded.destination,
        "0x1234000000000000000000000000000000005678"
    );
    assert!(
        !view.requires_raw_ack,
        "well-formed typed data is decodable"
    );

    // Typed data that is not JSON → Unrecognized → raw-ack gated.
    let bad = SignatureIntent {
        origin: "https://app.citrate.ai".into(),
        kind: IntentKind::TypedData,
        chain_id: 40204,
        raw: hex::encode(b"not json at all"),
    };
    let badview = c.request(bad);
    assert_eq!(badview.decoded.action, UNRECOGNIZED_ACTION);
    assert!(
        badview.requires_raw_ack,
        "non-JSON typed data must be raw-gated"
    );
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
    c.approve(&v, &a.id, false)
        .expect("approve the explicitly-named (older) id");
    assert_eq!(c.pending_count(), 1, "only the named ceremony was consumed");
    assert!(
        c.status(&a.id).is_none(),
        "the EXPLICITLY-NAMED (older) ceremony was consumed"
    );
    assert!(
        c.status(&b.id).is_some(),
        "the LATEST ceremony is untouched — no approve-latest / auto-approve"
    );

    // The pixel-level default-focus / one-click property is the honest UI gap
    // (not headless-testable); the CORE contract (explicit-id binding) is proven.
    // The remaining latest id still approves independently when named.
    assert!(
        c.approve(&v, &b.id, false).is_ok(),
        "the latest id approves only when named"
    );
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
    assert_eq!(
        sig1.sig_hex.len(),
        130,
        "r||s||v — personal_sign is EIP-191 now"
    );

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
    assert_eq!(
        c.pending_count(),
        0,
        "one approval consumed the single-use ceremony"
    );
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
        handles.push(std::thread::spawn(move || {
            c.approve(&v, &id, false).is_ok()
        }));
    }
    let successes = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(
        successes, 1,
        "exactly one of the racing approvals signs; the rest error"
    );
    assert_eq!(
        c.pending_count(),
        0,
        "the ceremony is consumed exactly once"
    );
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
    assert!(
        d.action.contains("raw bytes"),
        "binary message shown as bytes: {}",
        d.action
    );
    assert_ne!(
        d.action, UNRECOGNIZED_ACTION,
        "a showable binary message is not raw-gated"
    );
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
    assert_eq!(
        d.action, UNRECOGNIZED_ACTION,
        "unparseable payload → Unrecognized (raw-gated)"
    );
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
        CeremonyError::UndecodableTransaction,
        CeremonyError::Broadcast("node: insufficient funds".into()),
        CeremonyError::FromMismatch {
            claimed: "0x000000000000000000000000000000000000dead".into(),
            wallet: "0x000000000000000000000000000000000000beef".into(),
        },
    ] {
        let s = format!("{e} {e:?}");
        // No error carries the canonical mnemonic or any hex-looking key blob.
        assert!(
            !s.to_lowercase().contains("abandon"),
            "no mnemonic in error text"
        );
        // NOTE: FromMismatch deliberately carries PUBLIC addresses (claimed +
        // actual) so the human can see why a tx was refused; those are not
        // secret. The canonical ADDRESS assertion is scoped to the variants that
        // must not echo it — FromMismatch is constructed above with placeholder
        // (0x…dead / 0x…beef) addresses precisely so this sweep still holds.
        assert!(
            !s.contains(CANONICAL_ADDRESS),
            "errors do not echo addresses/keys"
        );
    }
}

// =========================================================================
// B1.5-ADV-8 (cross-cutting) — no key/seed/mnemonic/entropy in errors or Debug
// across the WHOLE custody path: wallet + ceremony + rpc + txdecode. Individual
// modules assert this locally (wallet_tests::adv_s, custody_tests::adv7,
// ceremony_errors_are_secret_free); B1.5 consolidates a single sweep spanning
// all four surfaces + the real ParsedTx Debug (which carries the `from` address
// but NOT calldata secrets) so the reviewer has ONE cross-cutting ADV-8 anchor.
// (This is the @rule8 consolidation the sprint asks for — not new product.)
// =========================================================================

#[test]
fn adv8_no_secret_across_wallet_ceremony_rpc_txdecode() {
    use crate::rpc::RpcError;

    // Drive a REAL create on a fresh vault so we have a live mnemonic/entropy to
    // hunt for, then assert NONE of it leaks through any error/Debug on the path.
    let mut p = std::env::temp_dir();
    p.push(format!(
        "citrate-core-adv8-{}-{}.enc",
        std::process::id(),
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            N.fetch_add(1, Ordering::Relaxed)
        }
    ));
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    let created =
        crate::wallet::create(&v).expect("create a fresh wallet for live secret material");
    let mnemonic = created.mnemonic.clone();
    let first_word = mnemonic.split_whitespace().next().unwrap().to_string();
    let three_word_prefix = mnemonic
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    let entropy_numseq = bip39::Mnemonic::parse(&*mnemonic)
        .expect("parse")
        .to_entropy()
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(",");

    // Collect the Debug + Display of every error surface + the ParsedTx Debug
    // across all four modules into one haystack.
    let mut haystack = String::new();

    // wallet errors
    for e in [
        crate::wallet::WalletError::Custody,
        crate::wallet::WalletError::InvalidMnemonic,
        crate::wallet::WalletError::AlreadyExists,
        crate::wallet::WalletError::NotFound,
        crate::wallet::WalletError::Derivation,
    ] {
        haystack.push_str(&format!("{e} {e:?} "));
    }
    // ceremony errors (incl. the new FromMismatch + a Broadcast carrying a node msg)
    for e in [
        CeremonyError::UnknownCeremony,
        CeremonyError::RawAckRequired,
        CeremonyError::VaultLocked,
        CeremonyError::NoWallet,
        CeremonyError::SignFailed,
        CeremonyError::UndecodableTransaction,
        CeremonyError::Broadcast("node: insufficient funds for gas * price".into()),
        CeremonyError::FromMismatch {
            claimed: "0x1111111111111111111111111111111111111111".into(),
            wallet: "0x2222222222222222222222222222222222222222".into(),
        },
    ] {
        haystack.push_str(&format!("{e} {e:?} "));
    }
    // rpc errors (the broadcast client's surface)
    for e in [
        RpcError::Transport("connect refused".into()),
        RpcError::BadResponse("not json".into()),
        RpcError::Node("nonce too low".into()),
        RpcError::MissingField("result".into()),
        RpcError::ReceiptTimeout,
    ] {
        haystack.push_str(&format!("{e} {e:?} "));
    }
    // txdecode ParsedTx Debug — carries the (public) `from`, `to`, value, and
    // calldata length shape, but is derived from a JSON tx object, never a key.
    // Prove a real decode's Debug carries no secret material.
    let (parsed, display) = crate::txdecode::decode_transaction(
        &serde_json::json!({
            "from": created.address,
            "to": "0x3535353535353535353535353535353535353535",
            "value": "0x1",
            "data": "0x",
        })
        .to_string(),
    )
    .expect("decode");
    haystack.push_str(&format!("{parsed:?} {display:?} "));

    // The whole cross-module haystack must be free of ANY secret needle.
    let hay_lower = haystack.to_lowercase();
    assert!(
        !hay_lower.contains(&first_word.to_lowercase()) || first_word.len() < 4,
        "no mnemonic word may appear across wallet/ceremony/rpc/txdecode surfaces"
    );
    assert!(
        !hay_lower.contains(&three_word_prefix.to_lowercase()),
        "no multi-word mnemonic prefix (high-entropy needle) may appear on any surface"
    );
    assert!(
        !haystack.contains(&entropy_numseq),
        "the sealed entropy (serde number-array form) must not appear on any surface"
    );
    // And the raw mnemonic string never appears verbatim anywhere.
    assert!(
        !haystack.contains(&*mnemonic),
        "the mnemonic must never appear verbatim on any error/Debug surface"
    );
}

// =========================================================================
// B1.5-ADV-5 (end-to-end, tx path) — a spoofing origin cannot fake a benign tx:
// the ceremony surfaces the TRUE origin verbatim AND decodes the ACTUAL tx; an
// undecodable tx is raw-gated on the broadcast path too. This extends B1.2's
// message-path adv5 to the NEW B1.4 transaction path (the sprint's ADV-5 note).
// =========================================================================

#[test]
fn adv5_tx_path_true_origin_and_undecodable_raw_gated_end_to_end() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();

    // A malicious origin names a benign-looking label but the DECODE is computed
    // from the actual tx object — the human sees the real destination + value,
    // and the TRUE origin verbatim (never a caller-claimed "benign" flag).
    let evil_origin = "https://evil.example (pretending to be app.citrate.ai)";
    let legible = SignatureIntent {
        origin: evil_origin.to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": canonical_addr_lower(),
            "to": "0x3535353535353535353535353535353535353535",
            "value": "0xde0b6b3a7640000", // 1e18 wei — the REAL amount
            "data": "0x",
        })
        .to_string(),
    };
    let view = c.request(legible);
    assert_eq!(
        view.origin, evil_origin,
        "the TRUE origin is surfaced verbatim on the tx path"
    );
    assert!(
        view.decoded.action.contains("1000000000000000000"),
        "the decode reflects the ACTUAL value, not a fabricated benign summary: {}",
        view.decoded.action
    );
    assert_eq!(
        view.decoded.destination,
        "0x3535353535353535353535353535353535353535"
    );
    assert!(
        !view.requires_raw_ack,
        "a legible tx is decodable (no raw-ack)"
    );

    // An UNDECODABLE tx (opaque non-JSON bytes) is raw-gated, and approve_and_broadcast
    // WITHOUT the ack broadcasts NOTHING — end-to-end on the new tx path.
    let opaque = SignatureIntent {
        origin: "agent:node-agent".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: hex::encode([0x02u8, 0xf8, 0x6b, 0x82]),
    };
    let ov = c.request(opaque);
    assert_eq!(
        ov.decoded.action, UNRECOGNIZED_ACTION,
        "undecodable tx → Unrecognized"
    );
    assert!(
        ov.requires_raw_ack,
        "undecodable calldata is raw-gated on the tx path"
    );

    // NEGATIVE CONTROL (stated): if approve_and_broadcast skipped the
    // `requires_raw_ack && !raw_ack` gate, this blind approval would sign +
    // broadcast undecodable calldata as if benign — the exact spoof. Blocked.
    let mock = MockRpc::new(vec![]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &ov.id, false, bcfg(1));
    assert_eq!(
        r.err(),
        Some(CeremonyError::RawAckRequired),
        "no ack → blocked, nothing signed"
    );
    assert!(
        client.transport.requests.borrow().is_empty(),
        "no RPC touched for a raw-gated tx"
    );
}

// =========================================================================
// CORE-B-003 tripwire — a contract call is legible (one-click) ONLY when its
// selector is on the known-ABI allowlist. The B1.4 decoder made the tx ENVELOPE
// decodable and silently re-scoped "calldata" to mean the envelope, so ANY
// well-formed tx got a one-click "N bytes calldata" summary that decoded neither
// selector, spender, nor amount. This asserts (a) an UNKNOWN selector is now
// raw-ack gated, and (b) the named regression vector — ERC-20
// `approve(spender, 2^256-1)` — is no longer a blind approve: its decode surfaces
// the spender AND flags the amount UNLIMITED. A plain value transfer stays legible.
// =========================================================================

#[test]
fn opaque_contract_calldata_gate_and_erc20_decode() {
    let c = SignatureCeremony::new();
    let token = "0x1111111111111111111111111111111111111111";
    let spender = "2222222222222222222222222222222222222222";

    // (a) An arbitrary UNKNOWN selector must be raw-ack gated (no benign summary).
    let unknown = SignatureIntent {
        origin: "agent:hermes".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": canonical_addr_lower(),
            "to": token,
            "value": "0x0",
            "data": "0xdeadbeef00112233",
        })
        .to_string(),
    };
    let uview = c.request(unknown);
    assert_eq!(
        uview.decoded.action, UNRECOGNIZED_ACTION,
        "an unknown-selector contract call must not surface a benign action"
    );
    assert!(
        uview.requires_raw_ack,
        "an unknown-selector contract call must require an explicit raw ack"
    );

    // (b) ERC-20 approve(spender, MAX) — the named regression vector. It is a KNOWN
    // selector, so it stays one-click, but it is NO LONGER blind: the decode names
    // the spender and flags the amount UNLIMITED (was an opaque "68 bytes calldata").
    let approve_data = format!("0x095ea7b3{:0>64}{}", spender, "f".repeat(64));
    let approve = SignatureIntent {
        origin: "agent:hermes".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": canonical_addr_lower(),
            "to": token,
            "value": "0x0",
            "data": approve_data,
        })
        .to_string(),
    };
    let aview = c.request(approve);
    assert_ne!(
        aview.decoded.action, UNRECOGNIZED_ACTION,
        "a known ERC-20 approve is decoded, not raw-gated"
    );
    assert!(
        aview.decoded.action.to_lowercase().contains("approve"),
        "approve decode names the operation: {}",
        aview.decoded.action
    );
    assert!(
        aview.decoded.action.to_lowercase().contains(spender),
        "approve decode surfaces the spender (no longer blind): {}",
        aview.decoded.action
    );
    assert!(
        aview.decoded.action.contains("UNLIMITED"),
        "an infinite (max-uint) approval must be flagged UNLIMITED: {}",
        aview.decoded.action
    );

    // A plain value transfer (empty calldata) stays legible — no false positive.
    let transfer = SignatureIntent {
        origin: "app.citrate.ai".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": canonical_addr_lower(),
            "to": token,
            "value": "0xde0b6b3a7640000",
            "data": "0x",
        })
        .to_string(),
    };
    assert!(
        !c.request(transfer).requires_raw_ack,
        "a plain value transfer with empty calldata remains legible (no raw ack)"
    );
}

// =========================================================================
// CORE-B1.4 — the transaction path: request decodes a REAL legacy tx, approve
// signs it with the vault key via `sign_eip155_legacy_tx`, broadcasts over a
// MOCK RPC (CI-safe), and the signer ecrecovers to the wallet address. All B1.2
// invariants (single-use, locked→fail-closed, raw-gate) still hold on this path.
// =========================================================================

use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::VecDeque;

/// A scripted mock RPC transport (mirrors rpc_tests): records requests, replies
/// with queued responses. Rule 1: a TEST transport, never the production default.
struct MockRpc {
    requests: RefCell<Vec<JsonValue>>,
    responses: RefCell<VecDeque<JsonValue>>,
}
impl MockRpc {
    fn new(responses: Vec<JsonValue>) -> Self {
        MockRpc {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }
}
impl RpcTransport for MockRpc {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.requests.borrow_mut().push(body.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}
fn ok(result: JsonValue) -> JsonValue {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

/// A fast broadcast config for tests: chain 40204, `attempts` receipt polls at a
/// 1ms interval (so timeout tests do not stall).
fn bcfg(attempts: u32) -> BroadcastConfig {
    BroadcastConfig {
        chain_id: 40204,
        poll_attempts: attempts,
        poll_interval: std::time::Duration::from_millis(1),
    }
}

/// A value-transfer tx intent (the shape the wagmi connector marshals: a JSON tx
/// object with hex fields). Sends 1 wei to `to` from the canonical wallet.
fn tx_intent(from: &str, to: &str, value_hex: &str) -> SignatureIntent {
    SignatureIntent {
        origin: "https://app.citrate.ai".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({ "from": from, "to": to, "value": value_hex, "data": "0x" })
            .to_string(),
    }
}

/// The canonical wallet's EVM address (checksummed lowercase for comparison).
fn canonical_addr_lower() -> String {
    CANONICAL_ADDRESS.to_lowercase()
}

#[test]
fn b1_4_transaction_decodes_to_human_action_not_raw_gated() {
    let (_v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let intent = tx_intent(
        &canonical_addr_lower(),
        "0x3535353535353535353535353535353535353535",
        "0x1",
    );
    let view = c.request(intent);
    // B1.4 REPLACES B1.2's blanket "Unrecognized" for a legible tx: the human
    // now sees the action/cost/destination, and it is NOT raw-gated.
    assert!(
        view.decoded.action.contains("Send"),
        "tx decoded for display: {}",
        view.decoded.action
    );
    assert_eq!(
        view.decoded.destination,
        "0x3535353535353535353535353535353535353535"
    );
    assert!(
        !view.requires_raw_ack,
        "a legible tx is decodable (no raw-ack)"
    );
}

#[test]
fn b1_4_approve_and_broadcast_signs_real_tx_and_ecrecovers_to_wallet() {
    use k256::ecdsa::{RecoveryId, Signature as K256Sig, VerifyingKey};

    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let intent = tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1");
    let view = c.request(intent);

    // Script the RPC: nonce (0x7) → gasPrice (2e9) → sendRawTransaction (hash)
    // → receipt (block 0x64). Chain id 40204.
    let tx_hash = "0xabc0000000000000000000000000000000000000000000000000000000000abc";
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())), // eth_getTransactionCount
        ok(JsonValue::String("0x77359400".into())), // eth_gasPrice (2e9)
        ok(JsonValue::String(tx_hash.into())), // eth_sendRawTransaction
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })), // receipt
    ]);
    let client = RpcClient::with_transport(mock);

    let result = c
        .approve_and_broadcast(&v, &client, &view.id, false, bcfg(2))
        .expect("approve + sign + broadcast a real tx");
    assert_eq!(
        result.tx_hash, tx_hash,
        "the node-accepted hash is returned"
    );
    assert_eq!(
        result.block_number,
        Some(100),
        "0x64 → block 100 (inclusion proof)"
    );
    assert_eq!(
        c.pending_count(),
        0,
        "the tx ceremony is consumed (single-use)"
    );

    // Reconstruct the EXACT raw tx that was broadcast and prove it ecrecovers to
    // the wallet address. We know the fields: nonce 7, gasPrice 2e9, gas 21000,
    // to 0x35..35, value 1, empty data, chain 40204.
    let fields = citrate_wallet_core::LegacyTxFields {
        nonce: 7,
        gas_price: 2_000_000_000,
        gas_limit: 21_000,
        to: Some([0x35u8; 20]),
        value: 1,
        data: vec![],
    };
    let unified =
        citrate_wallet_core::secp256k1_from_mnemonic(CANONICAL_MNEMONIC, 0).expect("derive");
    let sk = match unified {
        citrate_wallet_core::UnifiedKey::Secp256k1(k) => k,
        _ => panic!("expected secp256k1"),
    };
    let signed = citrate_wallet_core::sign_eip155_legacy_tx(&sk, &fields, 40204).expect("sign");

    // The v/r/s recover to the canonical wallet address (mirrors wallet-core's
    // own ecrecover proof, done here at the citrate-core layer).
    let recid_byte = (signed.v - 40204 * 2 - 35) as u8;
    let recovery_id = RecoveryId::from_byte(recid_byte).expect("valid recovery id");
    let mut sig_bytes = [0u8; 64];
    sig_bytes[..32].copy_from_slice(&signed.r);
    sig_bytes[32..].copy_from_slice(&signed.s);
    let signature = K256Sig::from_bytes((&sig_bytes).into()).expect("sig bytes");
    // Recompute the signing hash: keccak(rlp([nonce,gasPrice,gasLimit,to,value,data,chainId,0,0])).
    use sha3::{Digest, Keccak256};
    let mut stream = rlp::RlpStream::new_list(9);
    stream.append(&fields.nonce);
    stream.append(&fields.gas_price);
    stream.append(&fields.gas_limit);
    stream.append(&[0x35u8; 20].as_slice());
    stream.append(&fields.value);
    stream.append(&Vec::<u8>::new().as_slice());
    stream.append(&40204u64);
    stream.append(&0u8);
    stream.append(&0u8);
    let signing_hash = Keccak256::digest(stream.out());
    let recovered = VerifyingKey::recover_from_prehash(&signing_hash, &signature, recovery_id)
        .expect("ecrecover");
    let uncompressed = recovered.to_encoded_point(false);
    let addr_hash = Keccak256::digest(&uncompressed.as_bytes()[1..]);
    let recovered_addr = format!("0x{}", hex::encode(&addr_hash[12..32]));
    assert_eq!(
        recovered_addr,
        canonical_addr_lower(),
        "the broadcast tx's signature ecrecovers to the wallet address (real signing, not a mock)"
    );
}

#[test]
fn b1_4_broadcast_uses_real_rpc_nonce_and_gas() {
    // Rule 1: nonce + gas come from the RPC, not hardcoded. Assert the exact
    // request methods/params the broadcast issued.
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(
        &from,
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));

    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x9".into())),
        ok(JsonValue::String("0x3b9aca00".into())), // 1e9
        ok(JsonValue::String("0x111".into())),      // hash
        ok(serde_json::json!({ "blockNumber": "0x1", "status": "0x1" })),
    ]);
    let client = RpcClient::with_transport(mock);
    c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(2))
        .expect("broadcast");

    // Peek the recorded requests via a second borrow of the transport.
    // (approve_and_broadcast consumed the client by ref; the transport is inside.)
    // We assert method order: count(pending) → gasPrice → sendRaw → receipt.
    let reqs = client.transport.requests.borrow();
    assert_eq!(reqs[0]["method"], "eth_getTransactionCount");
    assert_eq!(reqs[0]["params"][1], "pending");
    assert_eq!(reqs[0]["params"][0].as_str().unwrap().to_lowercase(), from);
    assert_eq!(reqs[1]["method"], "eth_gasPrice");
    assert_eq!(reqs[2]["method"], "eth_sendRawTransaction");
    assert!(
        reqs[2]["params"][0].as_str().unwrap().starts_with("0x"),
        "raw hex tx"
    );
    assert_eq!(reqs[3]["method"], "eth_getTransactionReceipt");
}

#[test]
fn b1_4_broadcast_single_use_replay_yields_no_second_tx() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(
        &from,
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));

    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x0".into())),
        ok(JsonValue::String("0x1".into())),
        ok(JsonValue::String("0xhash1".into())),
        ok(serde_json::json!({ "blockNumber": "0x2", "status": "0x1" })),
    ]);
    let client = RpcClient::with_transport(mock);
    c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1))
        .expect("first broadcast");

    // Replay the SAME id → UnknownCeremony (consumed-first), NO second broadcast.
    // NEGATIVE CONTROL: if the ceremony were not removed before signing, this
    // would broadcast a second tx (double-spend of the nonce).
    let mock2 = MockRpc::new(vec![ok(JsonValue::String("0x0".into()))]);
    let client2 = RpcClient::with_transport(mock2);
    let r = c.approve_and_broadcast(&v, &client2, &view.id, false, bcfg(1));
    assert_eq!(
        r.err(),
        Some(CeremonyError::UnknownCeremony),
        "replay must not broadcast again"
    );
    assert!(
        client2.transport.requests.borrow().is_empty(),
        "no RPC call on a consumed id"
    );
}

#[test]
fn b1_4_broadcast_fails_closed_when_locked() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(
        &from,
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));

    v.lock();
    // Nonce+gas fetch precede signing; script them so we reach the vault signer,
    // which must fail closed on the locked vault (no tx signed/broadcast).
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x0".into())),
        ok(JsonValue::String("0x1".into())),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));
    assert!(
        matches!(
            r,
            Err(CeremonyError::NoWallet) | Err(CeremonyError::VaultLocked)
        ),
        "a locked vault must fail closed (no signature, no broadcast), got {r:?}"
    );
    // send/receipt were NEVER called (signing failed before broadcast).
    let reqs = client.transport.requests.borrow();
    assert!(
        reqs.iter().all(|r| r["method"] != "eth_sendRawTransaction"),
        "no raw tx broadcast on a locked vault"
    );
    assert_eq!(
        c.pending_count(),
        0,
        "locked approve still consumes the id (fail-closed single-use)"
    );
}

#[test]
fn b1_4_undecodable_tx_calldata_still_raw_gated() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    // Opaque, non-JSON tx bytes (a raw RLP blob) — NOT a legible tx object.
    let intent = SignatureIntent {
        origin: "agent:node-agent".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: hex::encode([0x02u8, 0xf8, 0x6b, 0x82]),
    };
    let view = c.request(intent);
    assert_eq!(
        view.decoded.action, UNRECOGNIZED_ACTION,
        "undecodable → Unrecognized"
    );
    assert!(
        view.requires_raw_ack,
        "undecodable tx calldata still raw-gates (B1.2-ADV-5 preserved)"
    );

    // approve_and_broadcast WITHOUT the raw ack → blocked, no RPC touched.
    let mock = MockRpc::new(vec![]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));
    assert_eq!(
        r.err(),
        Some(CeremonyError::RawAckRequired),
        "no raw-ack → blocked"
    );
    assert!(
        client.transport.requests.borrow().is_empty(),
        "no broadcast for a gated tx"
    );
    assert_eq!(
        c.pending_count(),
        1,
        "missing-ack re-inserts the ceremony for retry"
    );
}

#[test]
fn b1_4_broadcast_node_error_surfaces_no_fake_hash() {
    // The honest funded-account gap: a real node rejects an unfunded tx with
    // "insufficient funds". The ceremony surfaces that (proving the round-trip
    // reached the node) and returns NO fabricated tx hash (Rule 1).
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(
        &from,
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));

    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x0".into())),
        ok(JsonValue::String("0x1".into())),
        serde_json::json!({ "jsonrpc": "2.0", "id": 1,
            "error": { "code": -32000, "message": "insufficient funds for gas * price + value" } }),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));
    match r {
        Err(CeremonyError::Broadcast(m)) => assert!(
            m.contains("insufficient funds"),
            "node reason surfaced: {m}"
        ),
        other => panic!("expected a Broadcast error carrying the node reason, got {other:?}"),
    }
}

// =========================================================================
// B1.5-F-2 — approve_and_broadcast asserts `from == the vault wallet address`
// BEFORE nonce-fetch/sign. Match → proceeds; mismatch → a clear FromMismatch
// error, NO sign, NO broadcast, no RPC touched (not a downstream nonce reject).
// (Carried from citrate-security #23 / the B1.4 review.)
// =========================================================================

#[test]
fn b1_5_f2_from_matching_wallet_proceeds_to_broadcast() {
    // The MATCH branch: `from` == the canonical wallet address → the tx signs and
    // broadcasts exactly as before (F-2 does not regress the happy path).
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower(); // == the vault wallet address
    let view = c.request(tx_intent(
        &from,
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));

    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x1".into())),        // nonce
        ok(JsonValue::String("0x3b9aca00".into())), // gas price
        ok(JsonValue::String("0xhashok".into())),   // sendRaw
        ok(serde_json::json!({ "blockNumber": "0x5", "status": "0x1" })),
    ]);
    let client = RpcClient::with_transport(mock);
    let result = c
        .approve_and_broadcast(&v, &client, &view.id, false, bcfg(1))
        .expect("a from==wallet tx must broadcast");
    assert_eq!(result.tx_hash, "0xhashok");
    assert_eq!(result.block_number, Some(5));
    // The RPC WAS reached (F-2 passed, then the normal flow ran).
    assert!(
        !client.transport.requests.borrow().is_empty(),
        "a matching-from tx must reach the RPC (F-2 must not block the happy path)"
    );
}

#[test]
fn b1_5_f2_from_mismatch_fails_closed_before_sign_or_broadcast() {
    // The MISMATCH branch (the F-2 fix). A spoofing origin names a `from` that is
    // NOT the vault wallet. Pre-fix: the ceremony would fetch the nonce for the
    // WRONG account, sign a tx the vault key cannot author, and let the NODE
    // reject it (opaque, a live RPC round-trip already spent). Post-fix: caught
    // BEFORE any nonce-fetch/sign with a clear `FromMismatch`, NO RPC touched.
    //
    // NEGATIVE CONTROL (stated): remove the `parsed.from == wallet` assertion in
    // approve_and_broadcast and this test flips — the mock would be hit (nonce
    // fetch), the vault would sign, and the failure would surface as a downstream
    // node/broadcast error (or, with a scripted node, a wrong-sender tx) instead
    // of a clean pre-sign FromMismatch. The guard is that assertion.
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let spoof_from = "0x000000000000000000000000000000000000dead"; // NOT the wallet
    let view = c.request(tx_intent(
        spoof_from,
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));

    // Script responses that MUST NOT be consumed (proves no RPC round-trip).
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x1".into())),
        ok(JsonValue::String("0x1".into())),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));

    // Exact error variant + the (public, key-free) claimed/actual addresses.
    match r {
        Err(CeremonyError::FromMismatch { claimed, wallet }) => {
            assert_eq!(
                claimed.to_lowercase(),
                spoof_from.to_lowercase(),
                "claimed from echoed"
            );
            assert_eq!(
                wallet.to_lowercase(),
                canonical_addr_lower(),
                "the actual vault wallet address is surfaced"
            );
        }
        other => panic!("expected FromMismatch, got {other:?}"),
    }
    // NO RPC was touched — not a nonce fetch, not a broadcast (fail closed BEFORE
    // the live round-trip; not a downstream node rejection).
    assert!(
        client.transport.requests.borrow().is_empty(),
        "a from-mismatch must not fetch a nonce, sign, or broadcast (no RPC touched)"
    );
    // The ceremony is CONSUMED (consume-first single-use), so the spoofed id
    // cannot be retried — fail closed AND single-use.
    assert_eq!(
        c.pending_count(),
        0,
        "a from-mismatch approve still consumes the id (fail-closed single-use)"
    );
}

#[test]
fn b1_5_f2_from_mismatch_error_is_secret_free() {
    // The FromMismatch error carries only the two PUBLIC addresses — never key,
    // seed, entropy, or mnemonic material.
    let e = CeremonyError::FromMismatch {
        claimed: "0x000000000000000000000000000000000000dead".into(),
        wallet: CANONICAL_ADDRESS.into(),
    };
    let s = format!("{e} {e:?}");
    assert!(
        !s.to_lowercase().contains("abandon"),
        "no mnemonic in FromMismatch"
    );
    assert!(
        s.contains("dead") && s.contains("9858"),
        "both addresses surfaced for the human"
    );
}

// NOTE (WP-S1.2): `b1_4_sign_and_broadcast_command_registered` moved to
// citrate-core's lib.rs test module as part of `signing_commands_are_registered`
// — the generate_handler! registry lives in the app crate after the kit
// extraction, so a registry check there can see it (here it could only see the
// kit's own lib.rs). No coverage lost.

#[test]
fn b1_4_broadcast_result_is_serialize_and_secret_free() {
    // BroadcastResult crosses the bridge as PUBLIC facts only (hash + block).
    let br = BroadcastResult {
        tx_hash: "0xabc".into(),
        block_number: Some(100),
    };
    let j = serde_json::to_string(&br).expect("BroadcastResult is Serialize");
    assert!(
        j.contains("txHash") && j.contains("blockNumber"),
        "public tx facts: {j}"
    );
    assert!(
        !j.to_lowercase().contains("abandon"),
        "no mnemonic/key material in the result"
    );
}

// =========================================================================
// CORE-B1.4 — the LIVE PROOF (Rule 11 acceptance). #[ignore]d so it never runs
// in CI (it needs the live 40204 RPC + a funded key). Run explicitly via
// `src-tauri/scripts/b1_4_live_proof.sh`, which exports DEPLOY_KEY and calls:
//   cargo test --release b1_4_live_broadcast_real_40204_tx -- --ignored --nocapture
//
// What it does, end-to-end, with NO mocks (Rule 1):
//   1. Create a REAL A2 vault + a FRESH test wallet (fresh mnemonic sealed in the
//      vault); derive its EVM address.
//   2. Fund that address from 0x98a3… (DEPLOY_KEY) via `cast send` — just enough
//      for gas + a tiny transfer. Wait for the funding receipt.
//   3. Drive request → approve_and_broadcast PROGRAMMATICALLY (the approval fn
//      stands in for the human — the HITL UI is not headless; noted honestly).
//      This signs a REAL 40204 legacy tx with the vault key via the ceremony and
//      broadcasts it through the new live HttpTransport client.
//   4. Poll the receipt and PRINT the confirmed tx hash + block number.
//
// HONESTY: the live OS keyring + the interactive approval UI are not headless.
// This test uses the in-memory keyring fake (same custody vault crypto) and calls
// the approval function directly in place of a human click — the SIGNING +
// BROADCAST + on-chain confirmation are fully real.
#[test]
#[ignore = "live: needs 40204 RPC + funded DEPLOY_KEY; run via scripts/b1_4_live_proof.sh"]
fn b1_4_live_broadcast_real_40204_tx() {
    use crate::rpc::{HttpTransport, RpcClient};

    const FUNDER: &str = "0x98a32D944e9138B14A35b5D4dcE53339570F371A";
    let deploy_key = std::env::var("DEPLOY_KEY")
        .expect("DEPLOY_KEY must be exported (see scripts/b1_4_live_proof.sh)");

    // 1) Real vault + a FRESH test wallet (fresh mnemonic sealed in the vault).
    let mut p = std::env::temp_dir();
    p.push(format!("citrate-core-b1_4-live-{}.enc", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let vault = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    vault.init(&mut PASS.to_vec()).expect("init vault");
    vault.unlock(&mut PASS.to_vec()).expect("unlock vault");
    let created = crate::wallet::create(&vault).expect("create fresh wallet");
    let app_addr = created.address.clone();
    drop(created); // the mnemonic (Zeroizing) is dropped here — shown never, sealed only
    println!("[b1.4-live] app test wallet address: {app_addr}");

    // 2) Fund the app wallet from the funder via `cast send` (0.001 SALT).
    let cast = format!("{}/.foundry/bin/cast", std::env::var("HOME").expect("HOME"));
    let fund = std::process::Command::new(&cast)
        .args([
            "send",
            &app_addr,
            "--value",
            "1000000000000000", // 0.001 SALT
            "--private-key",
            &deploy_key,
            "--rpc-url",
            crate::rpc::CITRATE_RPC_URL,
            "--json",
        ])
        .output()
        .expect("run cast send to fund the app wallet");
    assert!(
        fund.status.success(),
        "funding tx failed: {}",
        String::from_utf8_lossy(&fund.stderr)
    );
    println!(
        "[b1.4-live] funding tx: {}",
        String::from_utf8_lossy(&fund.stdout).trim()
    );

    // 3) Request a tx intent: send a tiny amount (0.0001 SALT) back to the funder.
    let intent = SignatureIntent {
        origin: "b1.4-live-proof".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": app_addr,
            "to": FUNDER,
            "value": "0x5af3107a4000", // 0.0001 SALT
            "data": "0x",
        })
        .to_string(),
    };
    let c = SignatureCeremony::new();
    let view = c.request(intent);
    println!(
        "[b1.4-live] ceremony decoded action: {}",
        view.decoded.action
    );
    assert!(
        !view.requires_raw_ack,
        "a legible transfer must not be raw-gated"
    );

    // 4) Approve → sign the REAL tx with the vault key → broadcast → confirm.
    //    (The approval call here stands in for the human — HITL UI not headless.)
    let client = RpcClient::with_transport(HttpTransport::citrate());
    let result = c
        .approve_and_broadcast(
            &vault,
            &client,
            &view.id,
            false,
            BroadcastConfig {
                chain_id: 40204,
                poll_attempts: 60,
                poll_interval: std::time::Duration::from_secs(2),
            },
        )
        .expect("sign + broadcast a REAL 40204 tx through the ceremony");

    println!("[b1.4-live] ===== LIVE PROOF =====");
    println!("[b1.4-live] CONFIRMED tx hash : {}", result.tx_hash);
    println!("[b1.4-live] block number      : {:?}", result.block_number);
    println!("[b1.4-live] app wallet (sender): {app_addr}");
    println!("[b1.4-live] ======================");

    assert!(
        result.tx_hash.starts_with("0x") && result.tx_hash.len() == 66,
        "real tx hash"
    );
    assert!(
        result.block_number.is_some(),
        "the tx was confirmed by block inclusion"
    );
    let _ = std::fs::remove_file(&p);
}

// ── S7.5 (RT-5) — SessionBudget: the scoped, bounded authorization primitive. Default-off + fail-
// closed. These pin the covers/consume/expiry bounds; the live request/approve wiring is post-ADR.

fn intent(origin: &str, kind: IntentKind, chain_id: u64) -> SignatureIntent {
    SignatureIntent {
        origin: origin.into(),
        kind,
        chain_id,
        raw: "0x00".into(),
    }
}

fn budget() -> SessionBudget {
    SessionBudget::new(
        vec!["local-user".into(), "surface:groups".into()],
        vec![IntentKind::Transaction],
        40204,
        3,
        10_000, // expires at t=10s
    )
}

#[test]
fn covers_a_matching_intent_within_all_bounds() {
    let b = budget();
    assert!(b.covers(&intent("local-user", IntentKind::Transaction, 40204), 5_000));
    assert!(b.covers(
        &intent("surface:groups", IntentKind::Transaction, 40204),
        5_000
    ));
    assert_eq!(b.remaining(), 3);
}

#[test]
fn refuses_a_wrong_origin_kind_or_chain() {
    let b = budget();
    assert!(
        !b.covers(
            &intent("evil.example", IntentKind::Transaction, 40204),
            5_000
        ),
        "origin not allowed"
    );
    assert!(
        !b.covers(
            &intent("local-user", IntentKind::PersonalSign, 40204),
            5_000
        ),
        "kind not allowed"
    );
    assert!(
        !b.covers(&intent("local-user", IntentKind::Transaction, 1), 5_000),
        "wrong chain"
    );
}

#[test]
fn refuses_after_expiry() {
    let b = budget();
    assert!(
        !b.covers(
            &intent("local-user", IntentKind::Transaction, 40204),
            10_000
        ),
        "at expiry"
    );
    assert!(
        !b.covers(
            &intent("local-user", IntentKind::Transaction, 40204),
            99_999
        ),
        "past expiry"
    );
    assert!(b.is_expired(10_000));
}

#[test]
fn consume_decrements_and_exhausts_at_max_ops() {
    let mut b = budget();
    assert_eq!(b.consume(), Ok(2));
    assert_eq!(b.consume(), Ok(1));
    assert_eq!(b.consume(), Ok(0));
    assert_eq!(b.remaining(), 0);
    // Exhausted → covers is false + consume errors (defensive).
    assert!(
        !b.covers(&intent("local-user", IntentKind::Transaction, 40204), 5_000),
        "no ops left"
    );
    assert_eq!(b.consume(), Err("session budget exhausted"));
}

#[test]
fn a_zero_op_budget_covers_nothing() {
    let b = SessionBudget::new(
        vec!["local-user".into()],
        vec![IntentKind::Transaction],
        40204,
        0,
        10_000,
    );
    assert!(!b.covers(&intent("local-user", IntentKind::Transaction, 40204), 5_000));
}

// =========================================================================
// CORE-G2 tripwire (gateSec / @rule8) — SessionBudget auto-approve stays DORMANT.
// =========================================================================

/// `SessionBudget` is a vetted-but-dormant auto-approve primitive (`ceremony.rs`): it is defined and
/// unit-tested, but it must NOT be consulted by any production signer path (`request` / `approve` /
/// `approve_and_broadcast`) until ADR-2026-08-29 lands AND a fresh @rule8 security sign-off approves
/// the budget semantics. Wiring it would relax the per-transaction human-in-the-ceremony property
/// (Rule 3) into a scoped "one approval covers N" budget — the single largest Rule-3 regression
/// surface (citrate-core gateSec finding CORE-G2, 2026-08-30).
///
/// This guard fails if a `covers(` or `consume(` CALL appears in the NON-TEST source of `ceremony.rs`
/// (the definitions are `fn covers` / `fn consume`, no leading `.`, so they do not match). Scoped to
/// `ceremony.rs` on purpose: that is the only module from which the gated `pub(crate)` signer is
/// reachable, so it is the only place wiring the budget into auto-approve can take effect — and
/// scoping avoids false positives from unrelated `.consume(` calls elsewhere. To lift this gate,
/// wire the budget AND update this test in the same reviewed change.
#[test]
fn core_g2_session_budget_is_not_wired_into_the_production_signer() {
    let ceremony_non_test = strip_test_module(include_str!("ceremony.rs"));
    // Assemble the needles from parts so this guard's own text can never be what trips it.
    let covers_call = [".", "covers", "("].concat();
    let consume_call = [".", "consume", "("].concat();
    assert!(
        !ceremony_non_test.contains(&covers_call),
        "CORE-G2 tripwire: SessionBudget::covers is CALLED from production ceremony code. \
         Auto-approve must stay unwired until ADR-2026-08-29 + a fresh @rule8 sign-off. \
         (If this is a test helper, move it into `mod tests`.)"
    );
    assert!(
        !ceremony_non_test.contains(&consume_call),
        "CORE-G2 tripwire: SessionBudget::consume is CALLED from production ceremony code. \
         Auto-approve must stay unwired until ADR-2026-08-29 + a fresh @rule8 sign-off."
    );
}
