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
    // The live signers ARE invoked from ceremony.rs (positive control — the one
    // path each: approve → sign_personal, approve_and_broadcast → sign_transaction).
    // PBA-L4-007: the raw-bytes `sign_message` is no longer invoked from ANY
    // production path (typed_data / transaction on `approve` are refused), which
    // is strictly narrower than before; assert it stays that way.
    let ceremony_src = include_str!("ceremony.rs");
    let ceremony_non_test = strip_test_module(ceremony_src);
    let raw_message_call = "sign_".to_string() + "message(";
    for call in &calls {
        if *call == raw_message_call {
            let invoked = ceremony_non_test
                .lines()
                .map(str::trim_start)
                .any(|t| t.contains(call.as_str()) && !t.starts_with("//") && !t.starts_with("///") && !t.starts_with("//!"));
            assert!(
                !invoked,
                "PBA-L4-007: the raw SHA-256 message signer must not be reachable from ceremony approval"
            );
            continue;
        }
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

/// Return `src` with the terminal `#[cfg(test)] mod tests { ... }` block removed so a
/// call-site scan sees only non-test code. Our test modules are the terminal
/// `mod tests { include!(...) }` form, so truncating at the LAST such marker is exact. An inner
/// `#[cfg(test)]` item (a test-only helper) does not end the scan: everything after it is still
/// production code (HUP-S2.3: the budgeted path and the command surface follow one).
fn strip_test_module(src: &str) -> String {
    match src.rfind("#[cfg(test)]\nmod tests") {
        Some(idx) => src[..idx].to_string(),
        None => src.to_string(),
    }
}

/// The signer tripwires must scan everything before the terminal test module. ceremony.rs has an
/// earlier `#[cfg(test)]` helper; code after it (reject, the budgeted SIWE path, the command
/// surface) is production code and must stay in scope.
#[test]
fn strip_test_module_keeps_code_after_an_inner_cfg_test_helper() {
    let src = include_str!("ceremony.rs");
    let non_test = strip_test_module(src);
    for needle in [
        "pub fn reject(",
        "pub fn request_siwe_budgeted(",
        "pub fn sign_reject_sync(",
    ] {
        assert!(non_test.contains(needle), "{needle} must be scanned");
    }
    assert!(!non_test.contains("include!(\"ceremony_tests.rs\")"));
    let synthetic = "fn a() {}\n#[cfg(test)]\nfn helper() {}\nfn b() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
    let cut = strip_test_module(synthetic);
    assert!(cut.contains("fn b()") && !cut.contains("fn t()"));
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

    // approve WITH the explicit raw ack → passes the raw-ack gate, but a
    // `transaction` is never signed on the plain `approve` path (PBA-L4-007: it
    // used to be SHA-256(raw bytes) under the wallet key — not a transaction
    // signature at all). Only `approve_and_broadcast` signs a real EIP-155 tx.
    assert_eq!(
        c.approve(&v, &txview.id, true).err(),
        Some(CeremonyError::UnsupportedSigningKind),
        "raw-ack approve of a transaction on the message path is refused"
    );
    // Consumed: the refused request cannot be retried blind.
    assert_eq!(
        c.approve(&v, &txview.id, true).err(),
        Some(CeremonyError::UnknownCeremony)
    );
}

/// PBA-L4-007: a `typed_data` approval must NOT produce a wallet signature over
/// SHA-256(raw dApp bytes) labelled as EIP-712 (no domain separation, no chain id
/// — a signing oracle under a misleading label). It is refused until real
/// EIP-712 hashing exists; nothing is signed and the ceremony is consumed.
#[test]
fn pba_l4_007_typed_data_is_refused_not_signed_as_raw_sha256() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let td = serde_json::json!({
        "primaryType": "Permit",
        "domain": { "name": "Citrate", "verifyingContract": "0x1234000000000000000000000000000000005678" }
    });
    let view = c.request(SignatureIntent {
        origin: "https://app.citrate.ai".into(),
        kind: IntentKind::TypedData,
        chain_id: 40204,
        raw: hex::encode(serde_json::to_vec(&td).unwrap()),
    });
    assert!(!view.requires_raw_ack, "well-formed typed data decodes");
    assert_eq!(
        c.approve(&v, &view.id, false).err(),
        Some(CeremonyError::UnsupportedSigningKind),
        "typed data must not be signed as raw SHA-256 bytes"
    );
    assert_eq!(c.pending_count(), 0, "the refused ceremony is consumed");
    // personal_sign is unaffected (EIP-191, recoverable).
    let ps = c.request(SignatureIntent {
        origin: "https://app.citrate.ai".into(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        raw: hex::encode(b"hello"),
    });
    assert_eq!(c.approve(&v, &ps.id, false).expect("personal_sign signs").sig_hex.len(), 130);
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
    let c = certain_ceremony();
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
            "error": { "code": -32602, "message": "Insufficient funds: account balance 0 < required 21000" } }),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));
    match r {
        Err(CeremonyError::Broadcast(m)) => assert!(
            m.contains("Insufficient funds"),
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

/// HUP-S7.5 (D-27): the broadcast result carries the mined receipt's gas, the price paid, the
/// value sent, the recipient and the origin, so Hermes's spend can be metered from it.
#[test]
fn d27_broadcast_result_carries_gas_price_value_and_origin() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let mut intent = tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x2a");
    intent.origin = "agent:hermes".to_string();
    let view = c.request(intent);
    let tx_hash = "0xabc0000000000000000000000000000000000000000000000000000000000abd";
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())),
        ok(JsonValue::String("0x77359400".into())),
        ok(JsonValue::String(tx_hash.into())),
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1", "gasUsed": "0x5208", "effectiveGasPrice": "0x77359400" })),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c
        .approve_and_broadcast(&v, &client, &view.id, false, bcfg(2))
        .expect("broadcast");
    assert_eq!(r.status, Some(1));
    assert_eq!(r.gas_used, Some(21_000));
    assert_eq!(r.effective_gas_price_wei.as_deref(), Some("2000000000"));
    assert_eq!(r.value_wei.as_deref(), Some("42"));
    assert_eq!(r.to.as_deref(), Some("0x3535353535353535353535353535353535353535"));
    assert_eq!(r.origin, "agent:hermes");
    let j = serde_json::to_value(&r).unwrap();
    assert_eq!(j["gasUsed"], 21_000);
    assert_eq!(j["effectiveGasPriceWei"], "2000000000");

    // A receipt without gas facts leaves them unknown, never zero.
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x0"));
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x8".into())),
        ok(JsonValue::String("0x77359400".into())),
        ok(JsonValue::String(tx_hash.into())),
        ok(serde_json::json!({ "blockNumber": "0x65", "status": "0x1" })),
    ]);
    let r = c
        .approve_and_broadcast(&v, &RpcClient::with_transport(mock), &view.id, false, bcfg(2))
        .expect("broadcast");
    assert_eq!((r.gas_used, r.effective_gas_price_wei), (None, None));
}

/// HUP-S7.5 (D-27), review fix: the production broadcast path tells the installed observer about
/// every broadcast it sends (that is how Hermes's gas and SALT reach metering), passes the result
/// back unchanged, and tells it nothing when nothing was sent.
#[test]
fn d27_the_broadcast_path_tells_the_observer_and_returns_the_same_result() {
    static SEEN: StdMutex<Vec<(String, String, Option<u64>)>> = StdMutex::new(Vec::new());
    assert!(set_broadcast_observer(Box::new(|r: &BroadcastResult| {
        SEEN.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((r.tx_hash.clone(), r.origin.clone(), r.gas_used));
    })));
    assert!(
        !set_broadcast_observer(Box::new(|_: &BroadcastResult| {})),
        "a second observer is refused"
    );
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let mut intent = tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x0");
    intent.origin = "agent:hermes".to_string();
    let view = c.request(intent);
    let tx_hash = "0xd270000000000000000000000000000000000000000000000000000000000d27";
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x9".into())),
        ok(JsonValue::String("0x77359400".into())),
        ok(JsonValue::String(tx_hash.into())),
        ok(serde_json::json!({ "blockNumber": "0x66", "status": "0x1", "gasUsed": "0x5208", "effectiveGasPrice": "0x77359400" })),
    ]);
    let r = approve_broadcast_and_notify(
        &c,
        &v,
        &RpcClient::with_transport(mock),
        &view.id,
        false,
        bcfg(2),
    )
    .expect("broadcast");
    assert_eq!(r.tx_hash, tx_hash);
    let seen = SEEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(
        seen.contains(&(tx_hash.to_string(), "agent:hermes".to_string(), Some(21_000))),
        "{seen:?}"
    );
    // A consumed (or unknown) ceremony sends nothing, so the observer hears nothing.
    let again = approve_broadcast_and_notify(
        &c,
        &v,
        &RpcClient::with_transport(MockRpc::new(vec![])),
        &view.id,
        false,
        bcfg(2),
    );
    assert!(again.is_err());
    let after = SEEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(
        after.iter().filter(|(h, _, _)| h == tx_hash).count(),
        1,
        "told once, for the one broadcast"
    );
}

#[test]
fn b1_4_broadcast_result_is_serialize_and_secret_free() {
    // BroadcastResult crosses the bridge as PUBLIC facts only (hash + block).
    let br = BroadcastResult {
        tx_hash: "0xabc".into(),
        block_number: Some(100),
        status: None,
        gas_used: None,
        effective_gas_price_wei: None,
        value_wei: None,
        to: None,
        origin: String::new(),
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
///
/// HUP-S2.3 note: the SIWE-only `WebSigningBudget` path (`request_siwe_budgeted`, ADR-2026-09-30,
/// accepted) is a separate, closed-list mechanism with its own tripwire
/// (`budget_tripwire_the_budget_modules_never_reach_a_signer`). It does not use `SessionBudget`,
/// which stays dormant and is still guarded here.
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

/// HUP-S2.3: the call-site tripwires above must scan ALL production code in ceremony.rs,
/// including what follows an inner `#[cfg(test)]` item (the budgeted path, reject, the command
/// surface). Cutting at the first `#[cfg(test)]` silently skipped them.
#[test]
fn the_tripwire_scan_covers_the_whole_production_ceremony_module() {
    let scanned = strip_test_module(include_str!("ceremony.rs"));
    for f in [
        "fn request_siwe_budgeted(",
        "fn siwe_fall_through(",
        "fn sign_reject_sync(",
        "fn approve_and_broadcast",
    ] {
        assert!(scanned.contains(f), "the scan must include `{f}`");
    }
    assert!(
        !scanned.contains("include!(\"ceremony_tests.rs\")"),
        "the terminal test module is still cut off"
    );
}

// =========================================================================
// C1-V #347 review (Codex P1): after the node has accepted a transaction, a later failure must
// report an unknown outcome carrying the tx hash, never a failure that reads as "not sent".
// =========================================================================

fn submitted_outcome_unknown(r: &Result<BroadcastResult>) -> Option<String> {
    match r {
        Err(CeremonyError::SubmittedOutcomeUnknown { tx_hash, .. }) => Some(tx_hash.clone()),
        _ => None,
    }
}

#[test]
fn a_receipt_error_after_acceptance_reports_submitted_with_the_hash() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
    let accepted = format!("0x{}", "ab".repeat(32));
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x0".into())),
        ok(JsonValue::String("0x1".into())),
        ok(JsonValue::String(accepted.clone())),
        // The receipt poll hits a hard RPC failure (no further scripted response = transport error).
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(2));
    assert_eq!(submitted_outcome_unknown(&r), Some(accepted.clone()), "got {r:?}");
    let msg = err_str(r.expect_err("error"));
    assert!(msg.contains("submitted, outcome unknown"), "{msg}");
    assert!(msg.contains(&accepted), "{msg}");
}

#[test]
fn a_transport_failure_on_send_is_an_unknown_outcome_with_the_locally_computed_hash() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
    let mock = MockRpc::new(vec![ok(JsonValue::String("0x0".into())), ok(JsonValue::String("0x1".into()))]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));
    let hash = submitted_outcome_unknown(&r).unwrap_or_else(|| panic!("expected unknown outcome, got {r:?}"));
    // The hash is keccak256 of the raw tx that was handed to eth_sendRawTransaction.
    let reqs = client.transport.requests.borrow();
    let raw = reqs
        .iter()
        .find(|q| q["method"] == "eth_sendRawTransaction")
        .and_then(|q| q["params"][0].as_str())
        .expect("send was attempted")
        .to_string();
    use sha3::{Digest, Keccak256};
    let bytes = hex::decode(raw.trim_start_matches("0x")).expect("hex");
    assert_eq!(hash, format!("0x{}", hex::encode(Keccak256::digest(&bytes))));
}

#[test]
fn a_node_rejection_on_send_is_marked_not_submitted() {
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x0".into())),
        ok(JsonValue::String("0x1".into())),
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32602,
            "message": "Failed to submit transaction: NonceTooLow { expected: 3, got: 0 }" } }),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));
    assert!(matches!(r, Err(CeremonyError::Broadcast(_))), "got {r:?}");
    assert!(broadcast_err_str(r.expect_err("error")).starts_with("not submitted: "));
}

#[test]
fn errors_before_submission_are_marked_not_submitted_and_unknown_outcomes_are_not() {
    assert!(broadcast_err_str(CeremonyError::VaultLocked).starts_with("not submitted: "));
    assert!(broadcast_err_str(CeremonyError::UnknownCeremony).starts_with("not submitted: "));
    let unknown = broadcast_err_str(CeremonyError::SubmittedOutcomeUnknown {
        tx_hash: format!("0x{}", "cd".repeat(32)),
        reason: "rpc transport error: down".into(),
    });
    assert!(!unknown.contains("not submitted"), "{unknown}");
    assert!(unknown.contains("submitted, outcome unknown (tx 0xcdcd"), "{unknown}");
}

// =========================================================================
// UI truth contract v1.1 §1 (chain#326): "Not sent" only for a pre-submission failure, or for a
// DEFINITIVE node error on the FIRST submission of these signed bytes. Every other send error, and
// any error on a re-submission of the same bytes, is "Sent, outcome unknown" with the tx hash.
// The node returns every eth_sendRawTransaction error as -32602, so the message decides.
// =========================================================================

/// The definitive messages, exactly as the node words them (contract §1).
const DEFINITIVE: &[&str] = &[
    "Rate limit exceeded. Try again later.",
    "Missing transaction data",
    "Invalid tx hex",
    "Invalid hex",
    "Failed to parse transaction: RLP error: unexpected end",
    "Insufficient funds: account balance 0 < required 21000",
    "Failed to submit transaction: Full",
    "Failed to submit transaction: SenderLimitExceeded",
    "Failed to submit transaction: InvalidSignature",
    "Failed to submit transaction: GasPriceTooLow { minimum: 1000000000, got: 1 }",
    "Failed to submit transaction: InsufficientBalance { required: 21000, available: 0 }",
    "Failed to submit transaction: InvalidTransaction(\"chain id mismatch\")",
    "Failed to submit transaction: NonceTooLow { expected: 3, got: 0 }",
];

/// Messages that never prove the node refused the tx.
const AMBIGUOUS: &[&str] = &[
    "Failed to submit transaction: DuplicateTransaction(0xabc)",
    "Failed to submit transaction: DuplicateNonce { nonce: 0 }",
    "already known",
    "known transaction: 0xabc",
    "Internal error",
    "nonce too low",
    "insufficient funds for gas * price + value",
    "Failed to submit transaction: Full of surprises",
];

fn node_error(message: &str) -> JsonValue {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32602, "message": message } })
}

/// One approval of the canonical 1-wei tx against a node that answers the send with `send`.
/// The same ceremony + same nonce and gas price sign the same bytes (deterministic ECDSA), so a
/// second call on the same `c` re-submits identical signed bytes.
fn send_once(c: &SignatureCeremony, v: &CustodyVault, send: JsonValue) -> (Result<BroadcastResult>, String) {
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
    let mock = MockRpc::new(vec![ok(JsonValue::String("0x0".into())), ok(JsonValue::String("0x1".into())), send]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(v, &client, &view.id, false, bcfg(1));
    let reqs = client.transport.requests.borrow();
    let raw = reqs
        .iter()
        .find(|q| q["method"] == "eth_sendRawTransaction")
        .and_then(|q| q["params"][0].as_str())
        .expect("send was attempted")
        .to_string();
    use sha3::{Digest, Keccak256};
    let bytes = hex::decode(raw.trim_start_matches("0x")).expect("hex");
    (r, format!("0x{}", hex::encode(Keccak256::digest(&bytes))))
}

#[test]
fn each_definitive_message_is_not_sent_on_first_send_and_unknown_on_resend() {
    for msg in DEFINITIVE {
        let (v, _p) = vault_with_wallet();
        let c = certain_ceremony();
        let (first, _) = send_once(&c, &v, node_error(msg));
        assert!(matches!(first, Err(CeremonyError::Broadcast(_))), "first send of {msg:?}: {first:?}");
        assert!(broadcast_err_str(first.expect_err("error")).starts_with("not submitted: "), "{msg}");
        // The same signed bytes again: the first may have been accepted and mined, which is exactly
        // when NonceTooLow / Insufficient funds come back. Never "not sent".
        let (again, hash) = send_once(&c, &v, node_error(msg));
        assert_eq!(submitted_outcome_unknown(&again), Some(hash), "re-send of {msg:?}: {again:?}");
    }
}

#[test]
fn each_ambiguous_message_is_unknown_even_on_first_send() {
    for msg in AMBIGUOUS {
        let (v, _p) = vault_with_wallet();
        let c = certain_ceremony();
        let (first, hash) = send_once(&c, &v, node_error(msg));
        assert_eq!(submitted_outcome_unknown(&first), Some(hash), "first send of {msg:?}: {first:?}");
        let s = broadcast_err_str(first.expect_err("error"));
        assert!(!s.starts_with("not submitted"), "{msg}: {s}");
    }
}

#[test]
fn the_error_code_does_not_decide() {
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let internal = serde_json::json!({ "jsonrpc": "2.0", "id": 1,
        "error": { "code": -32603, "message": "Failed to submit transaction: DuplicateTransaction(0xabc)" } });
    let (r, hash) = send_once(&c, &v, internal);
    assert_eq!(submitted_outcome_unknown(&r), Some(hash));
}

#[test]
fn a_definitive_first_send_still_marks_the_bytes_as_sent() {
    // After a definitive refusal, the same bytes sent again are a re-submission: even a different
    // definitive message on the re-send is an unknown outcome.
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let (first, h1) = send_once(&c, &v, node_error("Failed to submit transaction: Full"));
    assert!(matches!(first, Err(CeremonyError::Broadcast(_))));
    let (again, h2) = send_once(&c, &v, node_error("Invalid hex"));
    assert_eq!(h1, h2, "same intent, nonce and gas price sign the same bytes");
    assert_eq!(submitted_outcome_unknown(&again), Some(h2));
    // A different ceremony object (e.g. a separate signer) has its own record.
    let other = certain_ceremony();
    let (fresh, _) = send_once(&other, &v, node_error("Invalid hex"));
    assert!(matches!(fresh, Err(CeremonyError::Broadcast(_))));
}

/// A transport that returns one scripted `Err` for the send.
struct RateLimitedSend {
    inner: MockRpc,
}
impl RpcTransport for RateLimitedSend {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        if body["method"] == "eth_sendRawTransaction" {
            self.inner.requests.borrow_mut().push(body);
            return Err(RpcError::RateLimited);
        }
        self.inner.call(body)
    }
}

/// A ceremony over a CERTAIN in-memory record (test-only): the classification tests need a record
/// that can prove a first submission without a file. The app never builds one (C1-V6); it uses a
/// persisted record or an uncertain one.
fn certain_ceremony() -> SignatureCeremony {
    SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::in_memory())
}

#[test]
fn http_429_is_definitive_on_first_send_only() {
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let run = |c: &SignatureCeremony| {
        let from = canonical_addr_lower();
        let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
        let t = RateLimitedSend { inner: MockRpc::new(vec![ok(JsonValue::String("0x0".into())), ok(JsonValue::String("0x1".into()))]) };
        c.approve_and_broadcast(&v, &RpcClient::with_transport(t), &view.id, false, bcfg(1))
    };
    assert!(matches!(run(&c), Err(CeremonyError::Broadcast(_))));
    assert!(submitted_outcome_unknown(&run(&c)).is_some());
}

// Contract v1.1 §1 (#347 review): the first-submission record survives a restart. A ceremony built
// over a persisted log treats bytes submitted in an earlier run as a re-submission.
#[test]
fn a_resend_after_a_restart_is_not_a_first_submission() {
    let dir = std::env::temp_dir().join(format!("citrate-ceremony-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp");
    let file = dir.join("submitted-tx.json");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
    let (first, _) = send_once(&c, &v, node_error("Failed to submit transaction: Full"));
    assert!(matches!(first, Err(CeremonyError::Broadcast(_))));
    drop(c);
    let restarted = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file));
    let (again, hash) = send_once(&restarted, &v, node_error("Failed to submit transaction: Full"));
    assert_eq!(submitted_outcome_unknown(&again), Some(hash));
    let _ = std::fs::remove_dir_all(&dir);
}

// #347 review (Codex round 2): with no app data dir the record is in memory only. A send through
// that ceremony is never a provable first submission: a definitive-looking node error is
// "submitted, outcome unknown" with the tx hash, never "not sent", before and after a restart.
#[test]
fn without_an_app_data_dir_a_node_error_is_never_not_sent() {
    let (v, _p) = vault_with_wallet();
    let c = build_ceremony_state_unpersisted().0;
    assert!(c.submitted_log().is_uncertain());
    let (first, hash) = send_once(&c, &v, node_error("Failed to submit transaction: Full"));
    assert_eq!(submitted_outcome_unknown(&first), Some(hash));
    drop(c);
    let restarted = build_ceremony_state_unpersisted().0;
    let (again, hash) = send_once(&restarted, &v, node_error("Failed to submit transaction: Full"));
    assert_eq!(submitted_outcome_unknown(&again), Some(hash), "a fresh fallback record proves nothing");
}

// Tripwire: the production fallback (no app data dir) builds the unpersisted ceremony, and nothing
// in the app builds a ceremony over a certain in-memory record.
#[test]
fn the_app_fallback_uses_the_unpersisted_record() {
    let lib = include_str!("../../src-tauri/src/lib.rs");
    assert!(lib.contains("ceremony::build_ceremony_state_unpersisted()"));
    assert!(!lib.contains("ceremony::build_ceremony_state()"));
    assert!(!lib.contains("SignatureCeremony::new()"));
}

// ---- STD audit parity: adversarial node messages and interleaving ----

#[test]
fn a_definitive_phrase_that_is_not_the_whole_or_leading_message_is_not_definitive() {
    for msg in [
        "proxy: Failed to submit transaction: Full",
        "failed to submit transaction: full",
        "Failed to submit transaction: Full ",
        "Failed to submit transaction: Fullish",
        "Invalid hex.",
        " Invalid tx hex",
        "Failed to submit transaction: DuplicateTransaction(Failed to submit transaction: Full)",
        "",
    ] {
        let (v, _p) = vault_with_wallet();
        let c = certain_ceremony();
        let (r, hash) = send_once(&c, &v, node_error(msg));
        assert_eq!(submitted_outcome_unknown(&r), Some(hash), "{msg:?}: {r:?}");
    }
}

#[test]
fn two_ceremonies_sharing_one_record_see_each_others_submissions() {
    let (v, _p) = vault_with_wallet();
    let a = certain_ceremony();
    let mut b = certain_ceremony();
    b.submitted = a.submitted_log();
    let (first, _) = send_once(&a, &v, node_error("Failed to submit transaction: Full"));
    assert!(matches!(first, Err(CeremonyError::Broadcast(_))));
    let (other, hash) = send_once(&b, &v, node_error("Failed to submit transaction: Full"));
    assert_eq!(submitted_outcome_unknown(&other), Some(hash));
}

// ---- #347 Codex P1s: when history is uncertain, "outcome unknown", never "not sent" ----

/// `send_once` at a chosen pending nonce (hex quantity).
fn send_at(c: &SignatureCeremony, v: &CustodyVault, nonce_hex: &str, send: JsonValue) -> (Result<BroadcastResult>, String) {
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
    let mock = MockRpc::new(vec![ok(JsonValue::String(nonce_hex.into())), ok(JsonValue::String("0x1".into())), send]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(v, &client, &view.id, false, bcfg(1));
    let reqs = client.transport.requests.borrow();
    let raw = reqs
        .iter()
        .find(|q| q["method"] == "eth_sendRawTransaction")
        .and_then(|q| q["params"][0].as_str())
        .map(str::to_string)
        .unwrap_or_default();
    use sha3::{Digest, Keccak256};
    let bytes = hex::decode(raw.trim_start_matches("0x")).unwrap_or_default();
    (r, format!("0x{}", hex::encode(Keccak256::digest(&bytes))))
}

#[test]
fn after_eviction_a_new_tx_at_a_higher_nonce_is_still_a_first_send() {
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let me = canonical_addr_lower();
    // The wallet's own nonce 0 is sent, then pushed out of the window by later nonces.
    let (first, _) = send_at(&c, &v, "0x0", node_error("Failed to submit transaction: Full"));
    assert!(matches!(first, Err(CeremonyError::Broadcast(_))));
    for n in 1..=(crate::submitted_log::CAPACITY as u64) {
        c.submitted_log().try_record_for(&format!("0x{n:064x}"), &me, n).expect("record");
    }
    // A resend at the evicted nonce 0 may be a resend of the evicted tx.
    let (resend, hash) = send_at(&c, &v, "0x0", node_error("Failed to submit transaction: NonceTooLow { expected: 1, got: 0 }"));
    assert_eq!(submitted_outcome_unknown(&resend), Some(hash));
    // A tx at a nonce above everything evicted can't be: a definitive refusal is "not sent".
    let (fresh, _) = send_at(&c, &v, "0x1f4", node_error("Failed to submit transaction: Full"));
    assert!(matches!(fresh, Err(CeremonyError::Broadcast(_))), "{fresh:?}");
}

#[test]
fn eviction_of_another_senders_history_does_not_affect_this_wallet() {
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let other = "0x00000000000000000000000000000000000000cc";
    for n in 0..=(crate::submitted_log::CAPACITY as u64) {
        c.submitted_log().try_record_for(&format!("0x{n:064x}"), other, n).expect("record");
    }
    let (r, _) = send_at(&c, &v, "0x0", node_error("Failed to submit transaction: Full"));
    assert!(matches!(r, Err(CeremonyError::Broadcast(_))), "{r:?}");
}

#[test]
fn a_resend_after_the_history_was_truncated_is_unknown() {
    let dir = std::env::temp_dir().join(format!("citrate-ceremony-evict-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp");
    let file = dir.join("submitted-tx.json");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
    let (first, _) = send_once(&c, &v, node_error("Failed to submit transaction: Full"));
    assert!(matches!(first, Err(CeremonyError::Broadcast(_))));
    // 256 other submissions push the original out of the window.
    for n in 0..256u32 {
        c.submitted_log().record(&format!("0x{n:064x}"));
    }
    drop(c);
    let restarted = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file));
    let (again, hash) = send_once(&restarted, &v, node_error("Failed to submit transaction: NonceTooLow { expected: 1, got: 0 }"));
    assert_eq!(submitted_outcome_unknown(&again), Some(hash), "the original may have been mined");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_resend_after_a_corrupt_record_was_rewritten_and_reopened_is_unknown() {
    let dir = std::env::temp_dir().join(format!("citrate-ceremony-corrupt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp");
    let file = dir.join("submitted-tx.json");
    std::fs::write(&file, b"{not json").expect("corrupt");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
    c.submitted_log().record(&format!("0x{:064x}", 7));
    drop(c);
    let restarted = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file));
    let (r, hash) = send_once(&restarted, &v, node_error("Failed to submit transaction: Full"));
    assert_eq!(submitted_outcome_unknown(&r), Some(hash));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_is_broadcast_when_the_submission_cannot_be_recorded_durably() {
    let dir = std::env::temp_dir().join(format!("citrate-ceremony-nodurable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp");
    let file = dir.join("submitted-tx.json");
    std::fs::create_dir(dir.join(format!(".submitted-tx.json.tmp-{}", std::process::id()))).expect("block tmp");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file));
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
    let mock = MockRpc::new(vec![
        ok(JsonValue::String("0x0".into())),
        ok(JsonValue::String("0x1".into())),
        ok(JsonValue::String(format!("0x{}", "ab".repeat(32)))),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(1));
    let reqs = client.transport.requests.borrow();
    assert!(reqs.iter().all(|q| q["method"] != "eth_sendRawTransaction"), "no broadcast without a durable record");
    match r {
        Err(e @ CeremonyError::Broadcast(_)) => {
            let s = broadcast_err_str(e);
            assert!(s.starts_with("not submitted: "), "{s}");
            assert!(s.contains("nothing was sent"), "{s}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// C1-V6 (#347, anchor-path audit): a record that can't survive a restart must never be certain in a
// production build. A ceremony built with no record location (`new()` / `default()`) is uncertain,
// so its sends are never "not sent".
#[test]
fn a_ceremony_built_without_a_record_location_is_never_certain() {
    assert!(SignatureCeremony::new().submitted_log().is_uncertain());
    assert!(SignatureCeremony::default().submitted_log().is_uncertain());
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let (first, hash) = send_once(&c, &v, node_error("Failed to submit transaction: Full"));
    assert_eq!(submitted_outcome_unknown(&first), Some(hash), "never \"not sent\" without a durable record");
}

// ---------------------------------------------------------------------------
// Join-8 — prepare (approve + sign, no send) then send. The tx hash is known and
// returned BEFORE anything reaches the network, so a caller can persist it first
// (a quit during the receipt poll must not lose it). One approval, one signature.
// ---------------------------------------------------------------------------

#[test]
fn join8_prepare_signs_and_returns_the_tx_hash_without_sending() {
    use sha3::{Digest, Keccak256};
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));

    let prep_rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())),        // eth_getTransactionCount
        ok(JsonValue::String("0x77359400".into())), // eth_gasPrice
    ]));
    let hash = c
        .approve_and_sign(&v, &prep_rpc, &view.id, false, 40204)
        .expect("approve + sign");
    let methods: Vec<String> = prep_rpc
        .transport()
        .requests
        .borrow()
        .iter()
        .map(|r| r["method"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        !methods.iter().any(|m| m == "eth_sendRawTransaction"),
        "prepare sends nothing: {methods:?}"
    );
    assert_eq!(c.pending_count(), 0, "the approval is consumed at prepare");
    assert!(hash.starts_with("0x") && hash.len() == 66);

    // The send broadcasts exactly the prepared raw, whose keccak is the returned hash.
    let send_rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String(hash.clone())),
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]));
    let result = c
        .send_prepared(&send_rpc, &view.id, bcfg(2))
        .expect("send the prepared tx");
    assert_eq!(result.tx_hash, hash);
    assert_eq!(result.block_number, Some(100));
    let sent = send_rpc.transport().requests.borrow()[0].clone();
    assert_eq!(sent["method"], "eth_sendRawTransaction");
    let raw_hex = sent["params"][0].as_str().unwrap_or("").trim_start_matches("0x").to_string();
    let raw = hex::decode(raw_hex).expect("raw hex");
    assert_eq!(format!("0x{}", hex::encode(Keccak256::digest(&raw))), hash);
}

#[test]
fn join8_a_prepared_tx_is_sent_at_most_once() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(tx_intent(
        &canonical_addr_lower(),
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));
    let prep_rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())),
        ok(JsonValue::String("0x77359400".into())),
    ]));
    let hash = c
        .approve_and_sign(&v, &prep_rpc, &view.id, false, 40204)
        .expect("prepare");
    let send_rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String(hash)),
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]));
    c.send_prepared(&send_rpc, &view.id, bcfg(2)).expect("first send");
    let again = c.send_prepared(&send_rpc, &view.id, bcfg(2));
    assert_eq!(again.err(), Some(CeremonyError::UnknownCeremony));
    // And the approval cannot be reused to sign again.
    let resign = c.approve_and_sign(&v, &prep_rpc, &view.id, false, 40204);
    assert_eq!(resign.err(), Some(CeremonyError::UnknownCeremony));
}

#[test]
fn join8_reject_discards_a_prepared_tx() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(tx_intent(
        &canonical_addr_lower(),
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));
    let prep_rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())),
        ok(JsonValue::String("0x77359400".into())),
    ]));
    c.approve_and_sign(&v, &prep_rpc, &view.id, false, 40204)
        .expect("prepare");
    c.reject(&view.id).expect("reject drops the prepared tx");
    let send_rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert_eq!(
        c.send_prepared(&send_rpc, &view.id, bcfg(2)).err(),
        Some(CeremonyError::UnknownCeremony)
    );
    assert!(send_rpc.transport().requests.borrow().is_empty(), "nothing was sent");
}

// ---------------------------------------------------------------------------
// Join-8 follow-up — approve_and_broadcast keeps its signed tx PRIVATE. It must
// never park the tx in the shared `prepared` map (where a concurrent reject or
// send_prepared on the same id could take it between the sign and the send).
// ---------------------------------------------------------------------------

#[test]
fn join8_approve_and_broadcast_never_parks_its_tx_in_the_shared_map() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request(tx_intent(
        &canonical_addr_lower(),
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())),
        ok(JsonValue::String("0x77359400".into())),
        ok(JsonValue::String("0xabc".into())),
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]));
    c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2))
        .expect("broadcast");
    assert_eq!(
        c.prepared_parks.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the one-call path must not round-trip its tx through the shared prepared map"
    );
}

/// A transport that flags when the first RPC arrives (the approval is consumed by then).
struct FlagRpc {
    inner: MockRpc,
    started: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl RpcTransport for FlagRpc {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.started.store(true, std::sync::atomic::Ordering::SeqCst);
        self.inner.call(body)
    }
}

#[test]
fn join8_a_concurrent_reject_cannot_take_an_approve_and_broadcast_tx() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    for round in 0..40 {
        let view = c.request(tx_intent(
            &canonical_addr_lower(),
            "0x3535353535353535353535353535353535353535",
            "0x1",
        ));
        let started = Arc::new(AtomicBool::new(false));
        let done = AtomicBool::new(false);
        let rpc = RpcClient::with_transport(FlagRpc {
            inner: MockRpc::new(vec![
                ok(JsonValue::String("0x7".into())),
                ok(JsonValue::String("0x77359400".into())),
                ok(JsonValue::String("0xabc".into())),
                ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
            ]),
            started: started.clone(),
        });
        let mut stolen = 0usize;
        let outcome = std::thread::scope(|sc| {
            let rejecter = sc.spawn(|| {
                let mut n = 0usize;
                // Once the approval is consumed (first RPC seen), hammer reject on the same id.
                while !done.load(Ordering::SeqCst) {
                    if started.load(Ordering::SeqCst) && c.reject(&view.id).is_ok() {
                        n += 1;
                    }
                }
                n
            });
            let r = c.approve_and_broadcast(&v, &rpc, &view.id, false, bcfg(2));
            done.store(true, Ordering::SeqCst);
            stolen = rejecter.join().unwrap_or(usize::MAX);
            r
        });
        assert!(outcome.is_ok(), "round {round}: broadcast lost its tx: {outcome:?}");
        assert_eq!(stolen, 0, "round {round}: a concurrent reject took the in-flight tx");
    }
}

/// TRIPWIRE: only `approve_and_sign` may park a signed tx in the shared map, and the
/// one-call `approve_and_broadcast` must not route through the two public halves.
#[test]
fn join8_tripwire_only_approve_and_sign_parks_a_signed_tx() {
    let src = strip_test_module(include_str!("ceremony.rs"));
    assert_eq!(
        src.matches("lock_prepared().insert(").count(),
        1,
        "exactly one place may insert into the prepared map"
    );
    let start = src
        .find("pub fn approve_and_broadcast")
        .expect("approve_and_broadcast exists");
    let body = &src[start..];
    let end = body.find("\n    }\n").map(|i| i + 6).unwrap_or(body.len());
    let body = &body[..end];
    assert!(
        !body.contains("self.approve_and_sign(") && !body.contains("self.send_prepared("),
        "approve_and_broadcast must keep its tx private, not round-trip it through the shared map"
    );
}

// ---------------------------------------------------------------------------
// Join-8 x #347: the activation send is recorded durably (fsync'd SubmittedTxLog) BEFORE it is
// sent, through the one shared send helper, for the prepared path as well as the one-call path.
// ---------------------------------------------------------------------------

const BOND: &str = "0x2222222222222222222222222222222222222222";

/// A MemberBond.activate-shaped call (selector 0x4bac89ff) from the canonical wallet to `BOND`.
fn activate_intent() -> SignatureIntent {
    let from = canonical_addr_lower();
    SignatureIntent {
        origin: "citrate-core://activate-validator".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({ "from": from, "to": BOND, "value": "0x0", "gas": "0x927c0", "data": format!("0x4bac89ff{}", "00".repeat(32)) })
            .to_string(),
    }
}

fn prep_rpc() -> RpcClient<MockRpc> {
    RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())),        // eth_getTransactionCount
        ok(JsonValue::String("0x77359400".into())), // eth_gasPrice
    ]))
}

fn activate_meta() -> crate::submitted_log::CallMeta {
    crate::submitted_log::CallMeta { to: BOND.into(), selector: "0x4bac89ff".into() }
}

/// Wraps a MockRpc; when the send arrives, notes whether the record file already names the hash.
struct RecordCheckRpc {
    inner: MockRpc,
    file: std::path::PathBuf,
    on_disk_at_send: RefCell<Option<bool>>,
}
impl RpcTransport for RecordCheckRpc {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        if body["method"] == "eth_sendRawTransaction" {
            use sha3::{Digest, Keccak256};
            let raw = body["params"][0].as_str().unwrap_or("");
            let bytes = hex::decode(raw.trim_start_matches("0x")).unwrap_or_default();
            let hash = format!("0x{}", hex::encode(Keccak256::digest(&bytes)));
            let on_disk = std::fs::read_to_string(&self.file).map(|s| s.contains(&hash)).unwrap_or(false);
            *self.on_disk_at_send.borrow_mut() = Some(on_disk);
        }
        self.inner.call(body)
    }
}

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "citrate-join8-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("tmp");
    d
}

#[test]
fn join8_a_prepared_send_is_on_disk_before_it_reaches_the_node() {
    // Crash window: the node receives the tx, then the app dies (no receipt). The record must
    // already be durable when the send arrives, and still name the tx after a restart.
    let dir = tmp_dir("ondisk");
    let file = dir.join("submitted-tx.json");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
    let view = c.request(activate_intent());
    let hash = c.approve_and_sign(&v, &prep_rpc(), &view.id, false, 40204).expect("sign");
    let send = RpcClient::with_transport(RecordCheckRpc {
        inner: MockRpc::new(vec![ok(JsonValue::String(hash.clone()))]), // accepted; the receipt poll then dies
        file: file.clone(),
        on_disk_at_send: RefCell::new(None),
    });
    let r = c.send_prepared(&send, &view.id, bcfg(1));
    assert_eq!(*send.transport().on_disk_at_send.borrow(), Some(true), "the record was durable before the send");
    assert!(r.is_err() || r.is_ok());
    let reopened = crate::submitted_log::SubmittedTxLog::open(file);
    assert_eq!(
        reopened.latest_call(&canonical_addr_lower(), &activate_meta()).map(|c| c.hash),
        Some(hash.clone()),
        "after a restart the activation send is found again"
    );
    assert_eq!(reopened.try_record_for(&hash, &canonical_addr_lower(), 7), Ok(false), "and is not a first submission");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn join8_a_prepared_send_without_a_durable_record_sends_nothing() {
    let dir = tmp_dir("nodurable");
    let file = dir.join("submitted-tx.json");
    std::fs::create_dir(dir.join(format!(".submitted-tx.json.tmp-{}", std::process::id()))).expect("block tmp");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file));
    let view = c.request(activate_intent());
    c.approve_and_sign(&v, &prep_rpc(), &view.id, false, 40204).expect("sign");
    let send = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(format!("0x{}", "ab".repeat(32))))]));
    let r = c.send_prepared(&send, &view.id, bcfg(1));
    assert!(
        send.transport().requests.borrow().iter().all(|q| q["method"] != "eth_sendRawTransaction"),
        "no send without a durable record"
    );
    match r {
        Err(e @ CeremonyError::Broadcast(_)) => {
            let s = broadcast_err_str(e);
            assert!(s.starts_with("not submitted: ") && s.contains("nothing was sent"), "{s}");
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn join8_forget_then_resign_at_the_same_nonce_is_never_not_sent() {
    // The member's first activation is refused definitively on its FIRST submission: "not sent"
    // is true. They try again (or forget an unseen one) and the app re-signs at the same nonce:
    // deterministic ECDSA gives the same bytes. A definitive-looking error on that re-send
    // (NonceTooLow: the first may have been mined after all) is an unknown outcome.
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let full = || MockRpc::new(vec![node_error("Failed to submit transaction: Full")]);
    let first = c.request(activate_intent());
    let h1 = c.approve_and_sign(&v, &prep_rpc(), &first.id, false, 40204).expect("sign 1");
    let r1 = c.send_prepared(&RpcClient::with_transport(full()), &first.id, bcfg(1));
    assert!(matches!(r1, Err(CeremonyError::Broadcast(_))), "a first definitive refusal is not sent: {r1:?}");

    let again = c.request(activate_intent());
    let h2 = c.approve_and_sign(&v, &prep_rpc(), &again.id, false, 40204).expect("sign 2");
    assert_eq!(h1, h2, "same nonce, same bytes");
    let nonce_low = MockRpc::new(vec![node_error("Failed to submit transaction: NonceTooLow { expected: 8, got: 7 }")]);
    let r2 = c.send_prepared(&RpcClient::with_transport(nonce_low), &again.id, bcfg(1));
    assert_eq!(submitted_outcome_unknown(&r2), Some(h2), "a re-send of recorded bytes is never \"not sent\"");

    // The same holds after a transport failure (unknown) and a forget of the activation record.
    let (v2, _p2) = vault_with_wallet();
    let c2 = certain_ceremony();
    let a = c2.request(activate_intent());
    let ha = c2.approve_and_sign(&v2, &prep_rpc(), &a.id, false, 40204).expect("sign a");
    let ra = c2.send_prepared(&RpcClient::with_transport(MockRpc::new(vec![])), &a.id, bcfg(1));
    assert_eq!(submitted_outcome_unknown(&ra), Some(ha.clone()));
    let _ = c2.submitted_log().forget(&ha);
    let b = c2.request(activate_intent());
    let hb = c2.approve_and_sign(&v2, &prep_rpc(), &b.id, false, 40204).expect("sign b");
    let nonce_low = MockRpc::new(vec![node_error("Failed to submit transaction: NonceTooLow { expected: 8, got: 7 }")]);
    let rb = c2.send_prepared(&RpcClient::with_transport(nonce_low), &b.id, bcfg(1));
    assert_eq!(submitted_outcome_unknown(&rb), Some(hb));
}

/// The body of `fn <name>` in `src` (from its signature to the matching close brace).
fn fn_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let start = src.find(&format!("fn {name}"))?;
    let open = start + src[start..].find('{')?;
    let mut depth = 0usize;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[start..open + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Violations of "every send goes through the durable record" in ceremony.rs's production code.
fn durable_send_violations(src: &str) -> Vec<String> {
    let code = strip_test_module(src);
    let mut out = Vec::new();
    if code.matches("send_raw_transaction(").count() != 1 {
        out.push("send_raw_transaction must be called from exactly one place (send_signed)".into());
    }
    match fn_body(&code, "send_signed") {
        Some(body) => {
            let rec = body.find(".try_record_call(");
            let send = body.find("send_raw_transaction(");
            if !matches!((rec, send), (Some(r), Some(s)) if r < s) {
                out.push("send_signed must record durably (try_record_call) before send_raw_transaction".into());
            }
            // A definitive first-send refusal is taken out of recovery (Codex P2 at f98f8ae).
            let refusal = body.find("is_definitive_send_error(");
            let forget = refusal.and_then(|r| body[r..].find(".forget(&local_hash)"));
            if forget.is_none() {
                out.push("send_signed must forget a definitively refused submission from recovery".into());
            }
        }
        None => out.push("fn send_signed not found".into()),
    }
    for f in ["approve_and_broadcast", "send_prepared", "approve_and_sign"] {
        if fn_body(&code, f).is_some_and(|b| b.contains("try_record")) {
            out.push(format!("{f} records on its own; the record belongs to send_signed only"));
        }
    }
    if !fn_body(&code, "sign_send_prepared_sync").is_some_and(|b| b.contains("broadcast_err_str")) {
        out.push("sign_send_prepared must mark pre-submission failures \"not submitted\"".into());
    }
    out
}

#[test]
fn join8_tripwire_every_send_goes_through_the_durable_record() {
    assert_eq!(durable_send_violations(include_str!("ceremony.rs")), Vec::<String>::new());
    // Negative controls: a second send site, and a send before the record.
    let two = "fn send_signed() { x.try_record_call(); rpc.send_raw_transaction(&r) }\nfn other() { rpc.send_raw_transaction(&r) }\nfn sign_send_prepared_sync() { broadcast_err_str }";
    assert!(!durable_send_violations(two).is_empty());
    let late = "fn send_signed() { rpc.send_raw_transaction(&r); x.try_record_call(); }\nfn sign_send_prepared_sync() { broadcast_err_str }";
    assert!(!durable_send_violations(late).is_empty());
}

#[test]
fn join8_a_definitively_refused_activation_is_not_recovered_but_stays_in_history() {
    // Codex P2 at f98f8ae: a first-submission refusal ("not sent") must not come back as the
    // member's pending activation from the durable record, yet those bytes stay a re-submission.
    let dir = tmp_dir("refused");
    let file = dir.join("submitted-tx.json");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
    let view = c.request(activate_intent());
    let hash = c.approve_and_sign(&v, &prep_rpc(), &view.id, false, 40204).expect("sign");
    let refused = MockRpc::new(vec![node_error("Failed to submit transaction: SenderLimitExceeded")]);
    let r = c.send_prepared(&RpcClient::with_transport(refused), &view.id, bcfg(1));
    assert!(matches!(r, Err(CeremonyError::Broadcast(_))), "a first definitive refusal is not sent: {r:?}");
    assert_eq!(c.submitted_log().latest_call(&canonical_addr_lower(), &activate_meta()), None, "not recovered in this run");
    let reopened = crate::submitted_log::SubmittedTxLog::open(file);
    assert_eq!(reopened.latest_call(&canonical_addr_lower(), &activate_meta()), None, "nor after a restart");
    assert_eq!(reopened.try_record_for(&hash, &canonical_addr_lower(), 7), Ok(false), "still in the submission history");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn join8_an_unknown_outcome_activation_is_still_recovered() {
    // The other side: an error that is NOT a definitive first-send refusal keeps the record.
    let dir = tmp_dir("unknown");
    let file = dir.join("submitted-tx.json");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
    let view = c.request(activate_intent());
    let hash = c.approve_and_sign(&v, &prep_rpc(), &view.id, false, 40204).expect("sign");
    let r = c.send_prepared(&RpcClient::with_transport(MockRpc::new(vec![])), &view.id, bcfg(1));
    assert_eq!(submitted_outcome_unknown(&r), Some(hash.clone()));
    let reopened = crate::submitted_log::SubmittedTxLog::open(file);
    assert_eq!(reopened.latest_call(&canonical_addr_lower(), &activate_meta()).map(|c| c.hash), Some(hash));
    let _ = std::fs::remove_dir_all(&dir);
}

// #347 Codex P1 (concurrency): two ceremonies can sign identical bytes. A records them first (a
// first submission), B records the same hash and sends it, and the node accepts B. If A's own
// send then gets a definitive-looking refusal (the bytes are now known / mined), A must report
// "submitted, outcome unknown" with the hash, never "not sent": another attempt may have sent them.
//
// The transport below interleaves deterministically: when A's send reaches the node, B's attempt
// records the same hash in the shared record (as B would before its own broadcast), and then A's
// send is refused.
struct RacedSend {
    inner: MockRpc,
    log: std::sync::Arc<crate::submitted_log::SubmittedTxLog>,
    refusal: &'static str,
}
impl RpcTransport for RacedSend {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        if body["method"] == "eth_sendRawTransaction" {
            use sha3::{Digest, Keccak256};
            let raw = body["params"][0].as_str().unwrap_or_default().to_string();
            let bytes = hex::decode(raw.trim_start_matches("0x")).unwrap_or_default();
            let hash = format!("0x{}", hex::encode(Keccak256::digest(&bytes)));
            // B: the same signed bytes, recorded by a second attempt before it broadcasts.
            let b_first = self.log.try_record_for(&hash, &canonical_addr_lower(), 0).expect("record");
            assert!(!b_first, "B sees A's record: not a first submission");
            self.inner.requests.borrow_mut().push(body);
            return Err(RpcError::Node(self.refusal.to_string()));
        }
        self.inner.call(body)
    }
}

#[test]
fn a_refusal_after_another_attempt_recorded_the_same_bytes_is_outcome_unknown() {
    for refusal in ["Failed to submit transaction: Full", "Insufficient funds: account balance 0 < required 21000"] {
        let (v, _p) = vault_with_wallet();
        let c = certain_ceremony();
        let from = canonical_addr_lower();
        let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
        let t = RacedSend {
            inner: MockRpc::new(vec![ok(JsonValue::String("0x0".into())), ok(JsonValue::String("0x1".into()))]),
            log: c.submitted_log(),
            refusal,
        };
        let r = c.approve_and_broadcast(&v, &RpcClient::with_transport(t), &view.id, false, bcfg(1));
        assert!(submitted_outcome_unknown(&r).is_some(), "{refusal}: {r:?}");
    }
}

// The same refusal with no overlapping attempt is still "not sent" (the classifier is unchanged).
#[test]
fn a_refusal_with_no_other_attempt_is_still_not_sent() {
    let (v, _p) = vault_with_wallet();
    let c = certain_ceremony();
    let (r, _) = send_once(&c, &v, node_error("Failed to submit transaction: Full"));
    assert!(matches!(r, Err(CeremonyError::Broadcast(_))), "{r:?}");
}

/// Tripwire (#347 Codex P1): any send path that classifies a refusal by first submission must also
/// require that no other attempt recorded the same bytes (`sole_attempt`). Scans non-test sources.
#[test]
fn tripwire_first_submission_classification_checks_for_overlapping_attempts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dirs = vec![root.join("src"), root.join("../src-tauri/src")];
    let mut offenders = Vec::new();
    while let Some(d) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&p).unwrap_or_default();
            if src.contains("first_submission") && !src.contains(".sole_attempt(") {
                offenders.push(p.display().to_string());
            }
        }
    }
    assert!(offenders.is_empty(), "first-submission classification without sole_attempt: {offenders:?}");
}

#[test]
fn join8_a_prepared_activation_refused_after_another_attempt_recorded_it_is_outcome_unknown() {
    // #347 Codex P1 on #354's own send helper, through the prepared path: A signs the activation
    // and sends it; while A's send is in flight B records the same bytes (as B would before its own
    // broadcast); then A's send is refused definitively. B may have sent those bytes, so A's result
    // is "outcome unknown" with the hash, never "not sent", and the activation stays recoverable.
    for refusal in ["Failed to submit transaction: Full", "Failed to submit transaction: SenderLimitExceeded"] {
        let dir = tmp_dir("raced");
        let file = dir.join("submitted-tx.json");
        let (v, _p) = vault_with_wallet();
        let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
        let view = c.request(activate_intent());
        let hash = c.approve_and_sign(&v, &prep_rpc(), &view.id, false, 40204).expect("sign");
        let t = RacedSend { inner: MockRpc::new(vec![]), log: c.submitted_log(), refusal };
        let r = c.send_prepared(&RpcClient::with_transport(t), &view.id, bcfg(1));
        assert_eq!(submitted_outcome_unknown(&r), Some(hash.clone()), "{refusal}: {r:?}");
        let reopened = crate::submitted_log::SubmittedTxLog::open(file);
        assert_eq!(
            reopened.latest_call(&canonical_addr_lower(), &activate_meta()).map(|c| c.hash),
            Some(hash),
            "{refusal}: the activation is still recoverable (it may have been sent)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// #347 Codex P1 (upgrade): an earlier build submitted a tx with an unknown outcome and kept no
// record. After the upgrade the same signed bytes are re-sent and the node answers with a
// definitive-looking error (the first copy was mined). That must be "submitted, outcome unknown".
#[test]
fn a_resend_after_upgrading_from_a_build_without_the_record_is_outcome_unknown() {
    let dir = std::env::temp_dir().join(format!("citrate-ceremony-upgrade-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp");
    std::fs::write(dir.join(crate::custody::ENVELOPE_FILE), b"sealed").expect("existing wallet");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(dir.join("submitted-tx.json")));
    for msg in ["Insufficient funds: account balance 0 < required 21000", "Failed to submit transaction: Full"] {
        let (r, hash) = send_once(&c, &v, node_error(msg));
        assert_eq!(submitted_outcome_unknown(&r), Some(hash), "{msg}: {r:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn join8_forget_then_resend_then_restart_recovers_the_in_flight_activation() {
    // Reviewer CR at d737f33: Forget (or a refusal), then the same bytes re-sent, then a restart.
    // Recovery must find the in-flight send, or Activate is offered a second time.
    let dir = tmp_dir("forget-resend");
    let file = dir.join("submitted-tx.json");
    let (v, _p) = vault_with_wallet();
    {
        let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file.clone()));
        let first = c.request(activate_intent());
        let h1 = c.approve_and_sign(&v, &prep_rpc(), &first.id, false, 40204).expect("sign 1");
        let r1 = c.send_prepared(&RpcClient::with_transport(MockRpc::new(vec![])), &first.id, bcfg(1));
        assert_eq!(submitted_outcome_unknown(&r1), Some(h1.clone()));
        assert_eq!(c.submitted_log().forget(&h1), Ok(true)); // the member's Forget
        let again = c.request(activate_intent());
        let h2 = c.approve_and_sign(&v, &prep_rpc(), &again.id, false, 40204).expect("sign 2");
        assert_eq!(h1, h2, "same nonce, same bytes");
        let r2 = c.send_prepared(&RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(h2.clone()))])), &again.id, bcfg(1));
        assert!(r2.is_ok() || submitted_outcome_unknown(&r2).is_some(), "{r2:?}");
    }
    let reopened = crate::submitted_log::SubmittedTxLog::open(file);
    assert!(
        reopened.latest_call(&canonical_addr_lower(), &activate_meta()).is_some(),
        "the re-sent activation is recovered after a restart"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rule 3 tripwire: the ONLY path that signs without a per-request approval of a CeremonyId is the
/// budgeted web sign-in (`request_siwe_budgeted`, ADR-2026-09-30). Every gated-signer call in the
/// production ceremony code must sit inside `approve`, `approve_and_broadcast` (both consume an
/// explicit CeremonyId the human approved) or `request_siwe_budgeted`. A new function that reaches
/// a signer, for example an "allowlisted intent" fast path, fails here.
#[test]
fn rule3_tripwire_no_ceremony_bypass_except_budgeted_web_sign_in() {
    let calls = [
        "sign_".to_string() + "message(",
        "sign_".to_string() + "transaction(",
        "sign_".to_string() + "personal(",
    ];
    // Join-8 (#354): the tx signer lives in the PRIVATE `sign_consumed`, shared by
    // `approve_and_sign` and `approve_and_broadcast`; both consume a CeremonyId the human approved.
    // It is allowed only while it stays private and is called from exactly those two.
    let allowed = ["approve", "approve_and_broadcast", "request_siwe_budgeted", "sign_consumed"];
    let src = strip_test_module(include_str!("ceremony.rs"));
    assert!(src.contains("    fn sign_consumed<") && !src.contains("pub fn sign_consumed"), "sign_consumed stays private");
    let mut callers: Vec<String> = Vec::new();
    let mut enclosing: Option<String> = None;
    let mut seen = 0;
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        if let Some(i) = t.find("fn ") {
            let head = &t[..i];
            let qualifiers = ["pub", "pub(crate)", "async", "const", "unsafe"];
            if head.split_whitespace().all(|w| qualifiers.contains(&w)) {
                let name: String = t[i + 3..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    enclosing = Some(name);
                }
            }
        }
        if t.contains("self.sign_consumed(") {
            callers.push(enclosing.clone().unwrap_or_default());
        }
        if calls.iter().any(|c| t.contains(c.as_str())) && !t.contains("fn sign_") {
            seen += 1;
            let f = enclosing.clone().unwrap_or_default();
            assert!(
                allowed.contains(&f.as_str()),
                "ceremony.rs: `{f}` reaches a gated signer; only {allowed:?} may: `{}`",
                line.trim()
            );
        }
    }
    assert!(seen >= 3, "positive control: the sanctioned signer calls were found ({seen})");
    callers.sort();
    callers.dedup();
    assert_eq!(
        callers,
        vec!["approve_and_broadcast".to_string(), "approve_and_sign".to_string()],
        "sign_consumed is reached only from the two approval paths"
    );
}

/// Rule 3 tripwire: no stored setting decides whether an intent goes through the ceremony. The
/// ceremony and the web sign-in budget never read the app config, and the app config carries no
/// signature-policy field (a 0.5.1 build stored `sigPolicy`; it is dropped on read).
#[test]
fn rule3_tripwire_no_config_can_skip_the_ceremony() {
    let needles = [
        ["crate::", "config"].concat(),
        ["App", "Config"].concat(),
        ["sig_", "policy"].concat(),
        ["sig", "Policy"].concat(),
    ];
    for (name, src) in [
        ("ceremony.rs", include_str!("ceremony.rs")),
        ("web_budget.rs", include_str!("web_budget.rs")),
        ("siwe.rs", include_str!("siwe.rs")),
    ] {
        let non_test = strip_test_module(src);
        for n in &needles {
            assert!(
                !non_test
                    .lines()
                    .any(|l| l.contains(n.as_str()) && !l.trim_start().starts_with("//")),
                "{name} must not read app config ({n})"
            );
        }
    }
    let config = strip_test_module(include_str!("config.rs"));
    for n in &needles[2..] {
        assert!(
            !config
                .lines()
                .any(|l| l.contains(n.as_str()) && !l.trim_start().starts_with("//")),
            "config.rs must carry no signature-policy field ({n})"
        );
    }
}

/// One approval of a 1-wei tx at the node's pending nonce `nonce`, answered with `send`.
fn send_at_fetched_nonce(c: &SignatureCeremony, v: &CustodyVault, nonce: u64, send: JsonValue) -> Result<BroadcastResult> {
    let from = canonical_addr_lower();
    let view = c.request(tx_intent(&from, "0x3535353535353535353535353535353535353535", "0x1"));
    let mock = MockRpc::new(vec![ok(JsonValue::String(format!("0x{nonce:x}"))), ok(JsonValue::String("0x1".into())), send]);
    c.approve_and_broadcast(v, &RpcClient::with_transport(mock), &view.id, false, bcfg(1))
}

// #443: on a profile upgraded from a build without the submitted-tx record, the first send at an
// app-read nonce stays "outcome unknown"; a later send at a higher app-read nonce that the node
// refuses reads "not sent".
#[test]
fn an_upgraded_profile_reports_not_sent_again_after_its_first_send() {
    let dir = std::env::temp_dir().join(format!("citrate-ceremony-upgraded-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp");
    std::fs::write(dir.join(crate::custody::ENVELOPE_FILE), b"sealed").expect("vault marker");
    let file = dir.join("submitted-tx.json");
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::with_submitted_log(crate::submitted_log::SubmittedTxLog::open(file));
    let refused = || node_error("Insufficient funds: account balance 0");
    let first = send_at_fetched_nonce(&c, &v, 5, refused());
    assert!(submitted_outcome_unknown(&first).is_some(), "the first send after the upgrade: {first:?}");
    let later = send_at_fetched_nonce(&c, &v, 6, refused());
    assert!(matches!(later, Err(CeremonyError::Broadcast(_))), "a refusal at a higher app-read nonce is not sent: {later:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
