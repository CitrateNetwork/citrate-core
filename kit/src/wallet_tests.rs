// CORE-B1.1 — wallet keystore adversarial + integration suite (@rule8 evidence).
//
// Every B1.1-ADV-* here is written RED-FIRST: the guard is neutralized (a
// documented NEGATIVE CONTROL below each test states what happens with the guard
// removed), the attack is shown to pass through, the guard restored, the test
// shown green. These run FULLY HEADLESS against an in-memory keyring fake — no
// live OS keyring, no Tauri runtime — so the create/import/derive/sign/seal/
// zeroize logic is proven in CI. The real OS-keyring path is honestly out of
// headless scope (custody.rs's `real_keyring_roundtrip_or_skip` covers that seam);
// the interactive one-time backup DISPLAY is not headless-testable (B1.2+ UI).

use super::*;
use crate::custody::{CustodyVault, Keyring};
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

// The published canonical BIP44 vector (data source: MetaMask/standard). The
// mnemonic `abandon×11 about` at m/44'/60'/0'/0/0 → this EVM address. The crate's
// `derive_address` returns lowercase hex, so we compare case-insensitively against
// the EIP-55 checksummed published form.
const CANONICAL_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const CANONICAL_ADDRESS: &str = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94";

// --- in-memory keyring fake (mirrors custody_tests::FakeKeyring) ------------

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

/// A fresh vault over a temp envelope + in-memory keyring.
fn vault() -> (CustodyVault, PathBuf) {
    let mut p = std::env::temp_dir();
    let uniq = format!("citrate-core-wallet-test-{}-{}.enc", std::process::id(), {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    });
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    (v, p)
}

fn init_and_unlock() -> (CustodyVault, PathBuf) {
    let (v, p) = vault();
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    (v, p)
}

// =========================================================================
// WP-1 — create: key sealed in the wallet- reserved slot
// =========================================================================

#[test]
fn create_seals_key_and_returns_backup_mnemonic() {
    let (v, _p) = init_and_unlock();
    let created = create(&v).expect("create must succeed on an unlocked vault");
    // Non-secret identity is populated.
    assert!(created.address.starts_with("0x"), "address is 0x-prefixed");
    assert_eq!(created.address.len(), 42, "20-byte EVM address");
    assert!(!created.public_key_hex.is_empty(), "pubkey present for display");
    // The mnemonic is shown ONCE for backup — 24 words.
    assert_eq!(
        created.mnemonic.split_whitespace().count(),
        24,
        "24-word BIP39 mnemonic for backup"
    );
    // The sealed slot is the wallet--prefixed reserved slot.
    assert!(WALLET_ENTROPY_SLOT.starts_with(WALLET_SLOT_PREFIX));
    // Re-deriving from the vault yields the SAME address (the seal round-trips).
    let info = address(&v).expect("re-derive from vault");
    assert_eq!(info.address, created.address, "sealed key re-derives to same addr");
}

#[test]
fn create_refuses_to_clobber_existing_wallet() {
    let (v, _p) = init_and_unlock();
    let _ = create(&v).expect("first create");
    let second = create(&v);
    assert_eq!(
        second.err(),
        Some(WalletError::AlreadyExists),
        "a second create must not overwrite the wallet"
    );
}

// =========================================================================
// WP-2 — import the canonical BIP44 vector reproduces the standard address
// =========================================================================

#[test]
fn import_canonical_vector_reproduces_standard_address() {
    let (v, _p) = init_and_unlock();
    let info = import(&v, CANONICAL_MNEMONIC).expect("import canonical vector");
    // Data source: the published BIP44 MetaMask vector. Case-insensitive because
    // the crate emits lowercase hex; the published form is EIP-55 checksummed.
    assert_eq!(
        info.address.to_lowercase(),
        CANONICAL_ADDRESS.to_lowercase(),
        "canonical mnemonic must derive the published EVM address"
    );
    // And the sealed entropy re-derives to the same address.
    let reread = address(&v).expect("re-derive");
    assert_eq!(reread.address.to_lowercase(), CANONICAL_ADDRESS.to_lowercase());
}

#[test]
fn import_rejects_invalid_mnemonic() {
    let (v, _p) = init_and_unlock();
    // Wrong checksum (last word changed).
    let bad = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
    assert_eq!(
        import(&v, bad).err(),
        Some(WalletError::InvalidMnemonic),
        "a checksum-invalid mnemonic must be rejected"
    );
    // Empty.
    assert_eq!(import(&v, "").err(), Some(WalletError::InvalidMnemonic));
}

// =========================================================================
// WP-3 — in-process sign round-trip: ecrecover(sig) == derived address
// =========================================================================

/// Keccak-256(uncompressed_pubkey[1..])[12..32] — the EVM address of a recovered
/// verifying key (matches wallet-core's `derive_address_from_secp256k1`).
fn address_of_verifying_key(vk: &k256::ecdsa::VerifyingKey) -> String {
    use sha3::{Digest, Keccak256};
    let uncompressed = vk.to_encoded_point(false);
    let hash = Keccak256::digest(&uncompressed.as_bytes()[1..]);
    format!("0x{}", hex::encode(&hash[12..32]))
}

#[test]
fn sign_message_ecrecovers_to_wallet_address() {
    use k256::ecdsa::{RecoveryId, Signature, SigningKey};

    let (v, _p) = init_and_unlock();
    let info = import(&v, CANONICAL_MNEMONIC).expect("import");

    let message = b"citrate-core B1.1 in-process sign round-trip";

    // The vault path: read sealed entropy -> re-derive -> sign (r||s, 64 bytes).
    let vault_sig = sign_message(&v, message).expect("sign via vault");
    assert_eq!(vault_sig.len(), 64, "non-recoverable r||s signature");

    // Independently derive the SAME key from the canonical mnemonic to obtain the
    // recovery id (UnifiedKey::sign drops it), sign RECOVERABLY over the same
    // prehash, recover the verifying key, and confirm it maps to the address.
    let unified = citrate_wallet_core::secp256k1_from_mnemonic(CANONICAL_MNEMONIC, 0)
        .expect("derive");
    let sk: SigningKey = match unified {
        citrate_wallet_core::UnifiedKey::Secp256k1(k) => k,
        _ => panic!("expected a secp256k1 key"),
    };
    // wallet-core signs the RAW message via k256's digest signer (SHA-256 prehash
    // under the hood for `Signer<Signature>`); reproduce that exact sig, then take
    // the matching recovery id via the prehash recoverable signer over SHA-256.
    use sha2::{Digest as Sha2Digest, Sha256};
    let prehash = Sha256::digest(message);
    let (rec_sig, recid): (Signature, RecoveryId) = sk
        .sign_prehash_recoverable(&prehash)
        .expect("recoverable sign");
    // The vault's non-recoverable sig must equal the r||s of the recoverable one
    // (same key, same message) — proving the vault path signs with the real key.
    assert_eq!(
        vault_sig,
        rec_sig.to_bytes().to_vec(),
        "vault sign() must equal the canonical key's signature over the message"
    );
    // ecrecover: recover the verifying key from (prehash, sig, recid).
    let recovered =
        k256::ecdsa::VerifyingKey::recover_from_prehash(&prehash, &rec_sig, recid)
            .expect("recover");
    assert_eq!(
        address_of_verifying_key(&recovered).to_lowercase(),
        info.address.to_lowercase(),
        "ecrecover(sig) must equal the derived wallet address"
    );
}

// B1.1-ADV-2 — "no wallet secret-path fn is an invoke command" — MOVED in the
// WP-S1.2 kit extraction to citrate-core's `lib.rs` test module. The scan reads
// the `generate_handler![...]` registry via `include_str!("lib.rs")`, and after
// the extraction the real registry lives in the app crate's lib.rs, not the
// kit's. The assertion is unchanged; only its home moved to the crate that owns
// the source it scans. See `no_wallet_secret_path_fn_is_an_invoke_command`.

// =========================================================================
// B1.1-ADV-3 — create/import/derive/sign while LOCKED → fail closed
// =========================================================================

#[test]
fn adv3_operations_fail_closed_when_locked() {
    // Create with a fresh (never-unlocked) wallet already stored, then lock.
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import while unlocked");
    let expected = address(&v).expect("addr while unlocked");
    v.lock();

    // create -> the pre-existence probe finds the imported wallet (custody_get
    // succeeds under a still-live session? no — we locked). When locked, the probe
    // `custody_get` denies, control falls through, and the seal `put` denies too
    // (custody.rs returns Denied when `inner.session` is None) -> fail closed.
    // NEGATIVE CONTROL: if `vault.put`/`custody_get` did not gate on the session,
    // the entropy would be readable/sealable on a locked vault; the guard is
    // custody.rs's session check, exercised here.
    let c = create(&v);
    assert!(
        matches!(c, Err(WalletError::Custody) | Err(WalletError::AlreadyExists)),
        "create on a locked vault must fail closed (no new seal), got {c:?}"
    );
    // address -> custody_get denies when locked -> NotFound/Custody (fail closed).
    let r = address(&v);
    assert!(
        matches!(r, Err(WalletError::NotFound) | Err(WalletError::Custody)),
        "address on a locked vault must fail closed, got {r:?}"
    );
    // sign -> read_entropy denies when locked -> fail closed, no signature.
    let s = sign_message(&v, b"x");
    assert!(
        matches!(s, Err(WalletError::NotFound) | Err(WalletError::Custody)),
        "sign on a locked vault must fail closed, got {s:?}"
    );
    // import -> the pre-existence probe `custody_get` denies (locked); even if it
    // reached the seal, `put` denies. Fail closed.
    let i = import(&v, CANONICAL_MNEMONIC);
    assert!(i.is_err(), "import on a locked vault must fail closed");

    // Sanity: unlocking again restores access (the lock was the only gate).
    v.unlock(&mut PASS.to_vec()).expect("re-unlock");
    assert_eq!(address(&v).expect("addr after re-unlock").address, expected.address);
}

// =========================================================================
// B1.1-ADV-4 — no mnemonic/seed/key plaintext at rest; no keys.json written
// =========================================================================

/// True if `needle` is recoverable from `haystack` — contiguous window OR the
/// serde_json number-array encoding of a `Vec<u8>` (mirrors custody_tests::leaked).
fn leaked(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return false;
    }
    if haystack.windows(needle.len()).any(|w| w == needle) {
        return true;
    }
    let num_seq = needle
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(",");
    String::from_utf8_lossy(haystack).contains(&num_seq)
}

#[test]
fn adv4_no_secret_plaintext_at_rest_and_no_keys_json() {
    let (v, p) = init_and_unlock();
    let created = import_then_capture(&v);
    let raw = std::fs::read(&p).expect("read custody.enc");

    // The mnemonic string must not appear on disk.
    assert!(
        !leaked(&raw, CANONICAL_MNEMONIC.as_bytes()),
        "mnemonic plaintext must not be in custody.enc"
    );
    // The raw entropy (what we seal) must not appear in the clear.
    let entropy = bip39::Mnemonic::parse(CANONICAL_MNEMONIC)
        .expect("parse")
        .to_entropy();
    assert!(
        !leaked(&raw, &entropy),
        "sealed entropy must be ciphertext-only at rest"
    );
    // The 64-byte seed must not appear.
    let seed = bip39::Mnemonic::parse(CANONICAL_MNEMONIC)
        .expect("parse")
        .to_seed("");
    assert!(!leaked(&raw, &seed), "BIP39 seed must not be at rest");
    // The 32-byte private key must not appear.
    let unified = citrate_wallet_core::secp256k1_from_mnemonic(CANONICAL_MNEMONIC, 0)
        .expect("derive");
    let secret = unified.secret_bytes();
    assert!(!leaked(&raw, secret.as_slice()), "private key must not be at rest");

    // NO keys.json anywhere near the envelope dir (Option A: no second store).
    let dir = p.parent().expect("parent dir");
    let keys_json = dir.join("keys.json");
    assert!(!keys_json.exists(), "Option A: no keys.json must be written");
    // Also assert the wallet slot exists as ciphertext (sanity: it was sealed).
    assert!(
        !created.address.is_empty(),
        "wallet was created (address populated)"
    );
}

/// Helper: import the canonical vector and return its info (keeps the test above
/// focused on the at-rest assertions).
fn import_then_capture(v: &CustodyVault) -> WalletInfo {
    import(v, CANONICAL_MNEMONIC).expect("import for at-rest scan")
}

// =========================================================================
// B1.1-ADV-9 — mnemonic/seed/key zeroized after create/import/sign/display
// =========================================================================

#[test]
fn adv9_secret_buffers_zeroize_after_use() {
    // The at-rest scan (ADV-4) proves no secret survives to disk. In-memory, the
    // secret buffers are `Zeroizing`/zeroized: `create`/`import` zeroize the
    // entropy Vec after `vault.put` (which itself zeroizes it); `read_entropy`
    // returns a `Zeroizing<Vec<u8>>` dropped at end of `address`/`sign_message`;
    // the intermediate mnemonic is a `Zeroizing<String>`; the derived SigningKey
    // zeroizes on drop (k256). Here we assert the observable contract: after a
    // full create+sign cycle, re-reading requires the vault (no lingering cached
    // key), and WalletCreate::mnemonic is a Zeroizing<String> (wiped on drop).
    let (v, _p) = init_and_unlock();
    let created = create(&v).expect("create");
    // The struct holds the mnemonic in a Zeroizing<String> (drop-wipes). Consume
    // it so the buffer is dropped/zeroized at end of scope.
    let mnem_len = created.mnemonic.len();
    assert!(mnem_len > 0);
    drop(created);
    // A subsequent sign must go back through the vault (no process-cached key):
    // lock the vault and confirm signing now fails closed — proving no key copy
    // lingered outside the vault after create.
    v.lock();
    assert!(
        sign_message(&v, b"post-create").is_err(),
        "no key may linger outside the vault after create (locked sign fails)"
    );
}

// =========================================================================
// B1.1-ADV-R — a webview-origin custody_put targeting a wallet- slot is REJECTED
// (backend-reserved) — mirrors A3-01
// =========================================================================

#[test]
fn adv_r_webview_custody_put_to_wallet_slot_is_rejected() {
    // The INVOKE-boundary guard is `custody::is_backend_reserved_slot`, which the
    // #[tauri::command] custody_put consults. B1.1 extended it to `wallet-`.
    // NEGATIVE CONTROL (stated): if BACKEND_SLOT_PREFIXES did NOT include
    // "wallet-", this assertion would fail (the slot would be writable from the
    // webview) — that is exactly the plant/overwrite attack we close.
    assert!(
        crate::custody::is_backend_reserved_slot("wallet-entropy-0"),
        "wallet- slots must be backend-reserved (invoke custody_put rejects them)"
    );
    assert!(
        crate::custody::is_backend_reserved_slot(WALLET_ENTROPY_SLOT),
        "the actual wallet slot constant must be reserved"
    );
    assert!(
        crate::custody::is_backend_reserved_slot("wallet-anything"),
        "the whole wallet- prefix is reserved, not just the one slot"
    );
    // The oidc- reservation still holds (we extended, did not replace).
    assert!(crate::custody::is_backend_reserved_slot("oidc-refresh"));
    // A non-reserved caller slot is still writable (guard is not over-broad).
    assert!(!crate::custody::is_backend_reserved_slot("user-note"));
    assert!(!crate::custody::is_backend_reserved_slot("wallets"));
}

// =========================================================================
// B1.1-ADV-S — mnemonic/seed absent from logs, error messages, and Debug output
// =========================================================================

#[test]
fn adv_s_no_secret_in_debug_or_errors() {
    let (v, _p) = init_and_unlock();
    let created = create(&v).expect("create");

    // Debug of WalletCreate must NOT contain the mnemonic (redacted).
    let dbg = format!("{:?}", created);
    assert!(
        dbg.contains("<redacted>"),
        "WalletCreate Debug must redact the mnemonic"
    );
    // The full secret phrase must never appear. Redaction is all-or-nothing, so
    // checking the whole phrase is the meaningful assertion. (B1.1-F-4: the prior
    // per-word loop substring-collided single dictionary words like "add"/"age"
    // against field names such as `address`, making the test flaky.)
    assert!(
        !dbg.contains(&*created.mnemonic),
        "the mnemonic phrase must not appear in Debug output"
    );
    // Belt-and-suspenders against a partial leak, without the flakiness: a
    // 3-word prefix is high-entropy and cannot collide with field names/hex.
    let prefix = created
        .mnemonic
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !dbg.contains(&prefix),
        "no multi-word mnemonic prefix may appear in Debug output"
    );

    // Error Display/Debug must be secret-free (coarse, no crate error echo).
    for e in [
        WalletError::Custody,
        WalletError::InvalidMnemonic,
        WalletError::AlreadyExists,
        WalletError::NotFound,
        WalletError::Derivation,
    ] {
        let s = format!("{e} {e:?}");
        assert!(!s.contains(&*created.mnemonic), "error text must not leak the mnemonic");
    }

    // An invalid-import error must not echo the supplied (secret) phrase back.
    let (v2, _p2) = init_and_unlock();
    let secret_phrase = "zebra zebra zebra zebra zebra zebra zebra zebra zebra zebra zebra zebra";
    let err = import(&v2, secret_phrase).unwrap_err();
    let es = format!("{err} {err:?}");
    assert!(
        !es.contains("zebra"),
        "an invalid mnemonic must not be echoed in the error"
    );
}

// =========================================================================
// WalletInfo is Serialize-safe (non-secret); WalletCreate is NOT Serialize.
// =========================================================================

#[test]
fn wallet_info_is_non_secret_serializable() {
    // WalletInfo (address + pubkey) is safe to cross the bridge.
    let info = WalletInfo {
        address: "0xabc".into(),
        public_key_hex: "04deadbeef".into(),
    };
    let j = serde_json::to_string(&info).expect("serialize WalletInfo");
    assert!(j.contains("0xabc"));
    assert!(j.contains("publicKeyHex"));
}

// ─────────────── EIP-191 personal_sign (the recoverable message path) ───────────────

/// The prefix and length encoding, against the vector every Ethereum
/// implementation agrees on.
///
/// `"hello world"` is 11 bytes, so the preimage is
/// `\x19Ethereum Signed Message:\n11hello world`. Hard-coding the expected digest
/// is the point: if the prefix, the decimal length, or the hash function ever
/// drifts, a signature still gets produced and verifies nowhere.
#[test]
fn eip191_prehash_matches_the_canonical_vector() {
    let got = eip191_prehash(b"hello world");
    assert_eq!(
        hex::encode(got),
        "d9eba16ed0ecae432b71fe008c98cc872bb4cc214d3220a36f365326cf807d68",
        "EIP-191 prehash of \"hello world\" drifted"
    );
    // The empty message still gets the prefix and a "0" length — a common
    // off-by-one place to skip the length entirely.
    let empty = eip191_prehash(b"");
    assert_ne!(hex::encode(empty), hex::encode(got));
}

/// The signature must RECOVER to the vault's own address. This is the whole
/// property the old `sign_message` path lacked: 64 bytes with a SHA-256 prehash
/// recovers to nobody, so no verifier — including the citrate-comms relay's SIWE
/// check — could ever accept it.
#[test]
fn personal_sign_recovers_to_the_wallet_address() {
    let (v, _p) = init_and_unlock();
    let info = import(&v, CANONICAL_MNEMONIC).expect("import");
    let msg = b"Sign in to citrate-comms";
    let sig = sign_personal(&v, msg).expect("sign");

    assert_eq!(sig.len(), 65, "personal_sign must be r||s||v");
    assert!(
        sig[64] == 27 || sig[64] == 28,
        "v must be 27/28 for personal_sign, got {}",
        sig[64]
    );

    // Recover, the way any EIP-191 verifier does.
    let prehash = eip191_prehash(msg);
    let rec_id = k256::ecdsa::RecoveryId::from_byte(sig[64] - 27).expect("recovery id");
    let signature = k256::ecdsa::Signature::from_slice(&sig[..64]).expect("sig");
    let vk = k256::ecdsa::VerifyingKey::recover_from_prehash(&prehash, &signature, rec_id)
        .expect("recover");
    assert_eq!(
        address_of_verifying_key(&vk).to_lowercase(),
        info.address.to_lowercase(),
        "the signature must recover to this vault's own address"
    );
}

/// A locked vault signs nothing — the same fail-closed rule the other two signers
/// hold (B1.2-ADV-3).
#[test]
fn personal_sign_fails_closed_on_a_locked_vault() {
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import");
    v.lock();
    assert!(
        sign_personal(&v, b"anything").is_err(),
        "a locked vault must not produce a personal_sign signature"
    );
}

/// Two different messages must not produce the same signature — a sanity check
/// that the message actually reaches the digest rather than a constant.
#[test]
fn personal_sign_commits_to_the_message() {
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import");
    let a = sign_personal(&v, b"transfer 1 SALT").expect("a");
    let b = sign_personal(&v, b"transfer 1000 SALT").expect("b");
    assert_ne!(a, b, "the signature must depend on the message");
}

// ─────────────────────────────────────────────────────────────────────────────
// AUTO-LOCK SELF-HEAL. The device-bound vault auto-locks (~30 min) and there is
// no passphrase to re-enter — only the keyring device secret reopens it. Every
// command that just wanted the member's own address therefore started failing
// with "custody vault locked or unavailable" on an idle app: validator
// activation, staking, withdrawals, transfers, the activity feed. Balances kept
// working because wallet_balances alone self-healed. `address_auto_unlocked` is
// that self-heal applied consistently (observed 2026-08-06 blocking a bond whose
// 32,000 SALT was already on chain).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn address_auto_unlocked_reopens_an_auto_locked_vault() {
    // DEVICE-BOUND vault (the shipped model): provisioned from the keyring device
    // secret, no user passphrase. `crate::custody::tests::vault` is that fixture.
    let (v, _fake, _p) = crate::custody::tests::vault(0);
    v.ensure_auto_unlocked().expect("device-bound provision");
    import(&v, CANONICAL_MNEMONIC).expect("import while unlocked");
    let expected = address(&v).expect("addr while unlocked");

    v.lock(); // what the ~30-minute auto-lock does

    // The plain read fails closed — this is what every caller hit.
    assert!(address(&v).is_err(), "a locked vault must fail closed for address()");

    // The self-healing read reopens from the keyring device secret and returns
    // the SAME wallet — never a different or freshly-minted one.
    let got = address_auto_unlocked(&v).expect("auto-unlock should reopen the vault");
    assert_eq!(got.address, expected.address, "must be the same wallet, not a new one");
}

#[test]
fn address_auto_unlocked_is_idempotent_on_an_already_open_vault() {
    let (v, _fake, _p) = crate::custody::tests::vault(0);
    v.ensure_auto_unlocked().expect("device-bound provision");
    import(&v, CANONICAL_MNEMONIC).expect("import while unlocked");
    let a = address_auto_unlocked(&v).expect("open vault");
    let b = address_auto_unlocked(&v).expect("still open");
    assert_eq!(a.address, b.address);
}

// ADR-2026-08-30 (D1) — recover_personal is the inverse of sign_personal, used to verify a peer's
// foreign identity binding: recover the signer and confirm it equals the claimed address.
#[test]
fn recover_personal_round_trips_and_resists_spoofing() {
    let phrase = std::iter::repeat_n("abandon", 11)
        .chain(std::iter::once("about"))
        .collect::<Vec<_>>()
        .join(" ");
    let key = secp256k1_from_mnemonic(&phrase, DEFAULT_ACCOUNT_INDEX).unwrap();
    let addr = info_of(&key).address;
    let sk = match &key {
        UnifiedKey::Secp256k1(sk) => sk,
        _ => panic!("secp256k1"),
    };
    let msg = b"Citrate identity binding\nNetwork: discord\nHandle: @dana\nAddress: 0xabc\nNonce: n";
    let prehash = eip191_prehash(msg);
    let (r, s, rec) = sign_recoverable(sk, &prehash).unwrap();
    let mut sig = [0u8; 65];
    sig[..32].copy_from_slice(&r);
    sig[32..64].copy_from_slice(&s);
    sig[64] = rec + 27;

    // Genuine: recovers to the signer.
    assert_eq!(
        recover_personal(msg, &sig).unwrap().to_lowercase(),
        addr.to_lowercase()
    );
    // Spoof resistance: the same signature over a DIFFERENT message recovers to a different address.
    assert_ne!(
        recover_personal(b"a different binding", &sig).unwrap().to_lowercase(),
        addr.to_lowercase()
    );
}

// =========================================================================
// CONNECT-S5 — wallet-derived scoped secret (portable comms identity)
// =========================================================================

#[test]
fn derive_scoped_secret_is_deterministic_domain_separated_and_valid() {
    const INFO: &[u8] = b"citrate-comms-member-identity-v1";
    // Two independent vaults holding the SAME wallet (the cross-device reinstall scenario) must
    // derive the SAME scoped secret — this is exactly what makes the comms identity portable.
    let (v1, _p1) = init_and_unlock();
    import(&v1, CANONICAL_MNEMONIC).expect("import v1");
    let (v2, _p2) = init_and_unlock();
    import(&v2, CANONICAL_MNEMONIC).expect("import v2");

    let a = derive_scoped_secret(&v1, INFO).expect("derive v1");
    let b = derive_scoped_secret(&v2, INFO).expect("derive v2");
    assert_eq!(&a[..], &b[..], "same wallet -> same scoped secret on every device");

    // Stable across repeated calls on the same vault.
    let a2 = derive_scoped_secret(&v1, INFO).expect("derive v1 again");
    assert_eq!(&a[..], &a2[..], "stable across calls");

    // Domain separation: a different info yields a different secret from the same wallet.
    let other = derive_scoped_secret(&v1, b"citrate-other-domain-v1").expect("derive other domain");
    assert_ne!(&a[..], &other[..], "different domain -> different secret");

    // The output is directly usable as a secp256k1 key.
    assert!(
        k256::ecdsa::SigningKey::from_slice(&a[..]).is_ok(),
        "derived secret is a valid secp256k1 scalar"
    );

    // A DIFFERENT wallet yields a different identity (bound to the wallet, not the device).
    let (v3, _p3) = init_and_unlock();
    create(&v3).expect("fresh random wallet");
    let c = derive_scoped_secret(&v3, INFO).expect("derive v3");
    assert_ne!(&a[..], &c[..], "different wallet -> different comms identity");
}

#[test]
fn derive_scoped_secret_fails_closed_when_no_wallet_or_locked() {
    // A vault with no wallet (never initialized/unlocked) must error, never return a value — the
    // comms provisioner turns this into WalletNotReady rather than a throwaway identity.
    let (v, _p) = vault();
    assert!(derive_scoped_secret(&v, b"x").is_err(), "no wallet -> fail closed");
}

// =========================================================================
// K6 — the one network gate on wallet-key signing
// =========================================================================

const K6_TEST_GENESIS: &str = "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1";

/// The next-genesis profile as it ships today: nothing set.
fn k6_unset() -> crate::rpc::ExpectedNetwork {
    crate::rpc::ExpectedNetwork {
        name: "genesis-2026-12".into(),
        chain_id: None,
        genesis_hash: None,
    }
}

fn k6_set(chain_id: u64) -> crate::rpc::ExpectedNetwork {
    crate::rpc::ExpectedNetwork {
        name: "alpha-r1005".into(),
        chain_id: Some(chain_id),
        genesis_hash: Some(K6_TEST_GENESIS.into()),
    }
}

fn k6_tx() -> LegacyTxFields {
    LegacyTxFields {
        nonce: 0,
        gas_price: 1,
        gas_limit: 21_000,
        to: Some([0x35; 20]),
        value: 1,
        data: Vec::new(),
    }
}

fn k6_siwe(chain: &str) -> String {
    format!(
        "app.example.org wants you to sign in with your Ethereum account:\n{CANONICAL_ADDRESS}\n\nSign in.\n\nURI: https://app.example.org/login\nVersion: 1\nChain ID: {chain}\nNonce: abcdef0123456789\nIssued At: 2026-10-19T00:00:00Z"
    )
}

#[test]
fn k6_personal_message_scope_reads_the_siwe_chain_id_and_nothing_else() {
    use crate::rpc::SignatureScope::{Chain, Unbound};
    assert_eq!(personal_message_scope(k6_siwe("40204").as_bytes()), Chain(40204));
    assert_eq!(personal_message_scope(k6_siwe("1").as_bytes()), Chain(1));
    // A value that does not parse, or two lines that disagree, never matches a target chain.
    assert_eq!(personal_message_scope(k6_siwe("40204x").as_bytes()), Chain(0));
    let two = format!("{}\nChain ID: 1", k6_siwe("40204"));
    assert_eq!(personal_message_scope(two.as_bytes()), Chain(0));
    assert_eq!(personal_message_scope(b"chain id: 7"), Chain(7));
    // Plain text and non-UTF-8 bytes commit to no chain.
    assert_eq!(personal_message_scope(b"Citrate identity binding\nNetwork: discord"), Unbound);
    assert_eq!(personal_message_scope(&[0xff, 0xfe, 0x00]), Unbound);
}

/// With no network set, every signer refuses before the key is read: the error is the network
/// refusal even on a LOCKED vault, which would otherwise fail on the key read.
#[test]
fn k6_every_signer_refuses_when_no_network_is_set() {
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import");
    crate::rpc::with_added_network_gate(k6_unset(), || {
        assert!(matches!(sign_personal(&v, b"hello"), Err(WalletError::WrongNetwork(_))));
        assert!(matches!(
            sign_personal(&v, k6_siwe("40204").as_bytes()),
            Err(WalletError::WrongNetwork(_))
        ));
        assert!(matches!(sign_message(&v, b"hello"), Err(WalletError::WrongNetwork(_))));
        assert!(matches!(
            sign_transaction(&v, &k6_tx(), 40204),
            Err(WalletError::WrongNetwork(_))
        ));
        v.lock();
        assert!(matches!(sign_personal(&v, b"hello"), Err(WalletError::WrongNetwork(_))));
    });
    // The added gate is gone after the closure; this process installs no gate, so it signs.
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    assert!(sign_personal(&v, b"hello").is_ok());
}

/// With the network set: chain-committing signatures must name its chain id; plain messages
/// sign (they are not network-bound).
#[test]
fn k6_a_set_network_checks_chain_committing_signatures() {
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import");
    crate::rpc::with_added_network_gate(k6_set(40204), || {
        let plain = sign_personal(&v, b"hello").expect("plain personal_sign is not network-bound");
        assert!(recover_personal(b"hello", &plain)
            .expect("recovers")
            .eq_ignore_ascii_case(CANONICAL_ADDRESS));
        sign_personal(&v, k6_siwe("40204").as_bytes()).expect("SIWE for the target chain");
        assert!(matches!(
            sign_personal(&v, k6_siwe("1").as_bytes()),
            Err(WalletError::WrongNetwork(m)) if m.contains("chain 1")
        ));
        sign_transaction(&v, &k6_tx(), 40204).expect("tx for the target chain");
        assert!(matches!(
            sign_transaction(&v, &k6_tx(), 1),
            Err(WalletError::WrongNetwork(_))
        ));
    });
}

/// An added gate can only add a check: a set gate on top of a not-set one still refuses.
#[test]
fn k6_an_added_gate_never_loosens_signing() {
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import");
    crate::rpc::with_added_network_gate(k6_unset(), || {
        crate::rpc::with_added_network_gate(k6_set(40204), || {
            assert!(matches!(sign_personal(&v, b"hello"), Err(WalletError::WrongNetwork(_))));
        })
    });
}

/// Split a source into its top-level (column-0) functions, comments dropped: `(name, body)`.
fn k6_top_level_fns(src: &str) -> Vec<(String, String)> {
    let mut fns: Vec<(String, String)> = Vec::new();
    for line in src.lines() {
        let t = line.trim_start();
        let is_fn_head = line == t
            && (t.starts_with("fn ") || t.starts_with("pub fn ") || t.starts_with("pub(crate) fn ")
                || t.starts_with("pub(super) fn "));
        if is_fn_head {
            let name = t
                .split("fn ")
                .nth(1)
                .and_then(|r| r.split(['(', '<']).next())
                .unwrap_or("")
                .to_string();
            fns.push((name, String::new()));
        }
        if let Some((_, body)) = fns.last_mut() {
            if !t.starts_with("//") {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    fns
}

/// Every k256 / wallet-core signing primitive, assembled from parts so this file's prose and
/// needles cannot self-match.
fn k6_signing_primitives() -> Vec<String> {
    vec![
        "sign_recoverable".to_string() + "(",
        "sign_eip155_legacy_tx".to_string() + "(",
        "sign_prehash_recoverable".to_string() + "(",
        "sign_prehash".to_string() + "(",
        "try_sign".to_string() + "(",
        "sign_digest".to_string(),
        ".sign".to_string() + "(",
        // wallet-core: the raw key bytes and its chain signers.
        ".secret_".to_string() + "bytes(",
        "sign_".to_string() + "secp256k1(",
        "sign_".to_string() + "v2(",
        "sign_for".to_string() + "_tip(",
    ]
}

/// STRUCTURAL: every function in wallet.rs that signs takes its key from `signing_key`, which runs
/// the network gate first. Since K6 the raw entropy and the entropy-to-key derivation live in the
/// private key module (`wallet_key.rs`) and do not compile from wallet.rs; this test is the
/// backstop for what remains: a wallet.rs function that signs (any k256 signing method) or is
/// named `sign_*` must call `signing_key(`, and only create/import (which derive from a fresh or
/// user-supplied mnemonic, never the stored one) may call the mnemonic derivation.
///
/// NEGATIVE CONTROL: add a wallet.rs `typed_data_signature` that calls `sk.sign_prehash_recoverable`
/// on a key it did not take from `signing_key` (or a `sign_typed_data` that skips it), and this
/// test fails.
#[test]
fn every_signer_takes_the_key_through_the_network_gate() {
    let src = include_str!("wallet.rs");
    let non_test = match src.rfind("#[cfg(test)]\nmod tests") {
        Some(i) => &src[..i],
        None => src,
    };
    let fns = k6_top_level_fns(non_test);
    let gate = "signing_key".to_string() + "(";
    let mnemonic_derive = "secp256k1_from_mnemonic".to_string() + "(";
    let entropy_decode = "from_entropy".to_string() + "(";
    let primitives = k6_signing_primitives();
    let mut signers = 0;
    for (name, body) in &fns {
        let body_after_head = body.split_once('\n').map(|x| x.1).unwrap_or("");
        assert!(
            !body_after_head.contains(&entropy_decode),
            "wallet.rs `{name}` decodes entropy; only the key module may"
        );
        if body_after_head.contains(&mnemonic_derive) {
            assert!(
                name == "create" || name == "import",
                "wallet.rs `{name}` derives a key from a mnemonic outside create/import"
            );
        }
        let signs = primitives.iter().any(|p| body_after_head.contains(p.as_str()));
        if signs || name.starts_with("sign_") {
            assert!(
                body_after_head.contains(&gate),
                "wallet.rs `{name}` signs without taking the key from the network gate"
            );
            signers += 1;
        }
    }
    assert!(signers >= 3, "sign_message, sign_transaction and sign_personal were found");

    // The gate itself, in the key module: the network check runs before the key is read.
    let key_src = include_str!("wallet_key.rs");
    let key_fns = k6_top_level_fns(key_src);
    let gate_fn = key_fns
        .iter()
        .find(|(n, _)| n == "signing_key")
        .map(|(_, b)| b.as_str())
        .expect("wallet_key.rs defines signing_key");
    let check = gate_fn.find("check_wallet_signing").expect("the gate checks the network");
    let read = gate_fn.find("read_entropy").expect("then reads the key");
    assert!(check < read, "the network check runs before the key is read");
    // Inside the key module, the raw-key functions stay private and only the four sanctioned
    // functions are exported to `wallet`.
    for (name, body) in &key_fns {
        let head = body.lines().next().unwrap_or("");
        let exported = head.starts_with("pub");
        let sanctioned = ["signing_key", "stored_info", "scoped_secret", "refuse_if_present"];
        assert_eq!(
            exported,
            sanctioned.contains(&name.as_str()),
            "wallet_key.rs `{name}`: only {sanctioned:?} may be visible outside the key module"
        );
        if exported {
            assert!(head.starts_with("pub(super) fn "), "`{name}` must be pub(super), no wider");
        }
    }
}

/// Lex `src` once and return two views of it, both the same length in chars as `src`, with every
/// newline kept in place:
/// - `code`: comments (line, block, nested block, doc) blanked to spaces; string and char literals
///   kept verbatim, so a needle written inside a literal is still seen;
/// - `shape`: as `code`, and the contents of every string, raw string, byte string and char
///   literal blanked too, so a `{`, `}`, `//` or `#[cfg(test)]` inside a literal is not structure.
fn k6_lex(src: &str) -> (Vec<char>, Vec<char>) {
    let s: Vec<char> = src.chars().collect();
    let n = s.len();
    let mut code = s.clone();
    let mut shape = s.clone();
    let blank = |v: &mut Vec<char>, i: usize| {
        if v[i] != '\n' {
            v[i] = ' ';
        }
    };
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut i = 0;
    while i < n {
        let c = s[i];
        let next = if i + 1 < n { s[i + 1] } else { '\0' };
        if c == '/' && next == '/' {
            while i < n && s[i] != '\n' {
                blank(&mut code, i);
                blank(&mut shape, i);
                i += 1;
            }
            continue;
        }
        if c == '/' && next == '*' {
            let mut depth = 0usize;
            while i < n {
                if s[i] == '/' && i + 1 < n && s[i + 1] == '*' {
                    depth += 1;
                    for k in i..i + 2 {
                        blank(&mut code, k);
                        blank(&mut shape, k);
                    }
                    i += 2;
                } else if s[i] == '*' && i + 1 < n && s[i + 1] == '/' {
                    depth -= 1;
                    for k in i..i + 2 {
                        blank(&mut code, k);
                        blank(&mut shape, k);
                    }
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    blank(&mut code, i);
                    blank(&mut shape, i);
                    i += 1;
                }
            }
            continue;
        }
        // Raw string: `r"…"`, `r#"…"#`, with an optional `b` / `c` prefix, not inside an identifier.
        if c == 'r' {
            let prefix_ok = match i.checked_sub(1).map(|j| s[j]) {
                None => true,
                Some(p) if !ident(p) => true,
                Some('b') | Some('c') => i < 2 || !ident(s[i - 2]),
                _ => false,
            };
            let mut j = i + 1;
            while j < n && s[j] == '#' {
                j += 1;
            }
            if prefix_ok && j < n && s[j] == '"' {
                let hashes = j - i - 1;
                let mut k = j + 1;
                loop {
                    if k >= n {
                        break;
                    }
                    if s[k] == '"' && k + hashes < n && s[k + 1..=k + hashes].iter().all(|&h| h == '#')
                    {
                        break;
                    }
                    blank(&mut shape, k);
                    k += 1;
                }
                i = (k + 1 + hashes).min(n);
                continue;
            }
        }
        if c == '"' {
            let mut k = i + 1;
            while k < n && s[k] != '"' {
                if s[k] == '\\' && k + 1 < n {
                    blank(&mut shape, k);
                    k += 1;
                }
                blank(&mut shape, k);
                k += 1;
            }
            i = k + 1;
            continue;
        }
        // Char literal (`'x'`, `'\n'`, `'\u{7b}'`, `'{'`); a lifetime or label (`'a`) is left alone.
        if c == '\'' {
            if next == '\\' {
                let mut k = i + 2;
                while k < n && s[k] != '\'' && s[k] != '\n' {
                    k += 1;
                }
                if k < n && s[k] == '\'' {
                    for m in i + 1..k {
                        blank(&mut shape, m);
                    }
                    i = k + 1;
                    continue;
                }
            } else if i + 2 < n && s[i + 2] == '\'' && next != '\n' {
                blank(&mut shape, i + 1);
                i += 3;
                continue;
            }
        }
        i += 1;
    }
    (code, shape)
}

/// The words that start an item (or a `let`) after any visibility: an attributed item ends at its
/// `;` or its `{ .. }`, never at a `,` (which may sit in its generics).
const K6_ITEM_WORDS: &[&str] = &[
    "async", "const", "enum", "extern", "fn", "impl", "let", "macro_rules", "mod", "static",
    "struct", "trait", "type", "union", "unsafe", "use",
];

/// Drop every `#[cfg(test)]` item and every comment, keeping all other code. Only the item the
/// attribute applies to is removed, by token position rather than by line: the attribute, any
/// further attributes, then the item up to its first `;` outside brackets or through its
/// matching `}`; a struct field, enum variant or match arm ends at its `,` or before the closing
/// bracket of what encloses it. Code before or after that item on the same line stays in scope, so
/// `#[cfg(test)] const A: u8 = 0; fn prod() { .. }` keeps `fn prod`. String-aware: brackets,
/// `;`, `//` and attributes inside string, raw-string and char literals are not structure (see
/// [`k6_lex`]), while the literals themselves stay in the output so needles inside them are
/// still counted. An attribute or item the walk cannot close (unbalanced input) is kept.
fn k6_strip_test_items(src: &str) -> String {
    let (code, shape) = k6_lex(src);
    let n = shape.len();
    let attr: Vec<char> = "#[cfg(test)]".chars().collect();
    let at = |i: usize, pat: &[char]| i + pat.len() <= n && shape[i..i + pat.len()] == *pat;
    let skip_ws = |mut i: usize| {
        while i < n && shape[i].is_whitespace() {
            i += 1;
        }
        i
    };
    // From an opening bracket at `i`, the index just past its match; `None` if unbalanced.
    let close = |i: usize| -> Option<usize> {
        let mut depth = 0i64;
        for (k, &c) in shape.iter().enumerate().skip(i) {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(k + 1);
                    }
                }
                _ => {}
            }
        }
        None
    };
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut dropped = vec![false; n];
    let mut i = 0;
    while i < n {
        if !at(i, &attr) {
            i += 1;
            continue;
        }
        let begin = i;
        let mut j = skip_ws(i + attr.len());
        // Further outer attributes on the same item.
        let mut ok = true;
        while j < n && at(j, &['#', '[']) {
            match close(j + 1) {
                Some(e) => j = skip_ws(e),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        // What is attributed: an item or `let` (after any visibility), or else a struct field,
        // enum variant, match arm or expression, which also ends at a `,` at depth 0.
        let item_like = {
            let mut v = j;
            if at(v, &['p', 'u', 'b']) && !(v + 3 < n && ident(shape[v + 3])) {
                v = skip_ws(v + 3);
                if v < n && shape[v] == '(' {
                    v = close(v).map(skip_ws).unwrap_or(n);
                }
            }
            let w: String = shape[v.min(n)..].iter().take_while(|&&c| ident(c)).collect();
            K6_ITEM_WORDS.contains(&w.as_str())
        };
        // The item: to its first `;` (or, for a field, variant or arm, `,`) at bracket depth 0,
        // or through the `{ .. }` it opens. A closer at depth 0 belongs to the enclosing struct,
        // enum or block, so the item ends before it.
        let mut end = None;
        if ok {
            let mut depth = 0i64;
            let mut k = j;
            while k < n {
                match shape[k] {
                    '(' | '[' => depth += 1,
                    ')' | ']' | '}' if depth == 0 => {
                        end = Some(k);
                        break;
                    }
                    ')' | ']' => depth -= 1,
                    ';' if depth == 0 => {
                        end = Some(k + 1);
                        break;
                    }
                    ',' if depth == 0 && !item_like => {
                        end = Some(k + 1);
                        break;
                    }
                    '{' if depth == 0 => {
                        end = close(k);
                        break;
                    }
                    _ => {}
                }
                k += 1;
            }
        }
        match end {
            Some(e) => {
                for d in dropped.iter_mut().take(e).skip(begin) {
                    *d = true;
                }
                i = e;
            }
            None => i += attr.len(),
        }
    }
    let kept: String = code
        .iter()
        .zip(dropped.iter())
        .map(|(&c, &d)| if d && c != '\n' { ' ' } else { c })
        .collect();
    let mut out = String::new();
    for line in kept.split('\n') {
        if !line.trim().is_empty() {
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out
}

/// What a source compiles in, by token: the file name of every resolvable path or include (see
/// [`k6_module_refs`]; an `include_str!` reads a file as text and compiles nothing) and the name
/// of every `mod <name>` declaration.
#[derive(Default)]
struct K6Refs {
    files: Vec<String>,
    mods: Vec<String>,
}

fn k6_refs(src: &str) -> K6Refs {
    let files = k6_module_refs(src)
        .into_iter()
        .flatten()
        .filter_map(|lit| std::path::Path::new(&lit).file_name().and_then(|f| f.to_str()).map(String::from))
        .collect();
    let t = k6_tokens(src);
    let mods = t
        .windows(2)
        .filter_map(|w| match (&w[0], &w[1]) {
            (K6Tok::Ident(m), K6Tok::Ident(name)) if m == "mod" => Some(name.clone()),
            _ => None,
        })
        .collect();
    K6Refs { files, mods }
}

impl K6Refs {
    fn count(&self, name: &str, stem: &str) -> usize {
        self.files.iter().filter(|f| *f == name).count() + self.mods.iter().filter(|m| *m == stem).count()
    }
}

/// One scanned source, lexed once.
struct K6Source {
    path: std::path::PathBuf,
    /// The source with comments blanked.
    code: String,
    /// The source with comments and `#[cfg(test)]` items removed.
    non_test: String,
    refs: K6Refs,
    non_test_refs: K6Refs,
}

fn k6_prep(path: std::path::PathBuf, src: &str) -> K6Source {
    let code: String = k6_lex(src).0.into_iter().collect();
    let non_test = k6_strip_test_items(src);
    let refs = k6_refs(&code);
    let non_test_refs = k6_refs(&non_test);
    K6Source { path, code, non_test, refs, non_test_refs }
}

/// Whether a `*_tests.rs` file is compiled only under `cfg(test)`: it starts with `#![cfg(test)]`,
/// or some file in the same crate brings it in (`include!("x_tests.rs")`, `#[path = "x_tests.rs"]`,
/// a `path` inside `#[cfg_attr(..)]`, or `mod x_tests`, in any spacing) and every such reference
/// sits inside a `#[cfg(test)]` item. A reference the scan cannot see (an
/// `include!(concat!(..))`, say) leaves the file unreferenced, so it is scanned. A file brought
/// in by non-test code (say `pub mod x_tests;`) is NOT exempt, and neither is an orphan.
fn k6_is_cfg_test_file(path: &std::path::Path, crate_files: &[K6Source]) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let Some(stem) = name.strip_suffix(".rs") else {
        return false;
    };
    let Some(own) = crate_files.iter().find(|f| f.path == path) else {
        return false;
    };
    if own.code.trim_start().starts_with("#![cfg(test)]") {
        return true;
    }
    let mut referenced = false;
    for other in crate_files {
        if other.path == path || other.refs.count(name, stem) == 0 {
            continue;
        }
        referenced = true;
        if other.non_test_refs.count(name, stem) > 0 {
            return false;
        }
    }
    referenced
}

fn k6_collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name != "target" && name != "tests" {
                k6_collect_rs(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// STRUCTURAL (K6, lexical backstop): wallet key material is reachable only through the key
/// module. The compiler already enforces most of this: `read_entropy` and
/// `derive_key_from_entropy` are private to `wallet_key.rs`, `custody_get` refuses `wallet-`
/// slots, and `wallet_slot_get` needs a `WalletKeyAccess` only that module can build. This scan
/// covers the rest, in the non-test code of every .rs file under kit/src and src-tauri/src:
/// - the entropy and key-derivation names, the mnemonic derivation, `SigningKey::from*`, and every
///   signing primitive appear only where listed below, with the count listed;
/// - any new caller (a file not listed, or one more call in a listed file) fails, so it gets
///   reviewed and either goes through `signing_key` or is added here with its key named.
///
/// The wallet.rs signers are further pinned per function by
/// `every_signer_takes_the_key_through_the_network_gate`.
///
/// NEGATIVE CONTROL: add `bip39::Mnemonic::from_entropy`, `secp256k1_from_mnemonic(` or
/// `.sign_prehash_recoverable(` to any kit or app module (or one more in a listed file), or name
/// `WALLET_ENTROPY_SLOT` / `wallet_slot_get` outside the files below, and this test fails.
#[test]
fn wallet_key_material_is_reachable_only_through_the_key_module() {
    let kit_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let app_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src-tauri/src");
    assert!(app_src.is_dir(), "the app sources must be scanned too: {}", app_src.display());
    let mut kit_files = Vec::new();
    k6_collect_rs(&kit_src, &mut kit_files);
    let mut app_files = Vec::new();
    k6_collect_rs(&app_src, &mut app_files);
    let read_all = |v: &[std::path::PathBuf]| -> Vec<K6Source> {
        v.iter()
            .map(|p| k6_prep(p.clone(), &std::fs::read_to_string(p).expect("read source")))
            .collect()
    };
    let kit_sources = read_all(&kit_files);
    let app_sources = read_all(&app_files);
    let files: Vec<std::path::PathBuf> = kit_files.iter().chain(app_files.iter()).cloned().collect();

    let mut needles: Vec<String> = vec![
        "read_".to_string() + "entropy",
        "derive_key_".to_string() + "from_entropy",
        "from_".to_string() + "entropy(",
        "secp256k1_".to_string() + "from_mnemonic(",
        "SigningKey::".to_string() + "from",
        "WALLET_ENTROPY".to_string() + "_SLOT",
        "wallet_slot".to_string() + "_get",
        "WalletKey".to_string() + "Access",
        // Forging a value (K6): the compiler refuses these in kit (`deny(unsafe_code)`), and the
        // reviewed `allow`s are pinned here so a new one is seen.
        "allow(".to_string() + "unsafe_code)",
        "trans".to_string() + "mute",
        "mem::".to_string() + "zeroed",
    ];
    needles.extend(k6_signing_primitives());
    // Counted with no test item stripped (see below).
    let strict: Vec<String> = vec![
        "wallet_slot".to_string() + "_get",
        "WalletKey".to_string() + "Access",
        "allow(".to_string() + "unsafe_code)",
        "trans".to_string() + "mute",
        "mem::".to_string() + "zeroed",
    ];

    // (file, needle, count): every sanctioned non-test occurrence outside the key module.
    let allowed: &[(&str, &str, usize)] = &[
        // wallet.rs: the slot name constant, and create/import sealing the new entropy (a write).
        ("kit/src/wallet.rs", "WALLET_ENTROPY_SLOT", 3),
        // wallet.rs: create/import derive the display identity from the fresh / supplied mnemonic.
        ("kit/src/wallet.rs", "secp256k1_from_mnemonic(", 2),
        // wallet.rs: re-export of the capability type so custody.rs can name it.
        ("kit/src/wallet.rs", "WalletKeyAccess", 1),
        // wallet.rs signers, each taking its key from `signing_key` (pinned per function).
        ("kit/src/wallet.rs", ".sign(", 1),
        ("kit/src/wallet.rs", "sign_eip155_legacy_tx(", 1),
        ("kit/src/wallet.rs", "sign_recoverable(", 1),
        // custody.rs: the capability-gated read itself.
        ("kit/src/custody.rs", "wallet_slot_get", 1),
        ("kit/src/custody.rs", "WalletKeyAccess", 1),
        // anchor.rs: the anchor key from its own keyring account, not the wallet key.
        ("kit/src/ceremony/anchor.rs", "SigningKey::from", 1),
        ("kit/src/ceremony/anchor.rs", "sign_eip155_legacy_tx(", 1),
        // comms.rs: the comms identity key (an HKDF output via `derive_scoped_secret`).
        ("src-tauri/src/comms.rs", "SigningKey::from", 3),
        // device_link.rs: the device-link key, not the wallet key.
        ("src-tauri/src/device_link.rs", "SigningKey::from", 4),
        ("src-tauri/src/device_link.rs", "sign_prehash_recoverable(", 1),
        // validator.rs: the block proposer key.
        ("src-tauri/src/validator.rs", "SigningKey::from", 2),
        ("src-tauri/src/validator.rs", ".sign(", 1),
        // fleet_pairing.rs: the fleet pairing key.
        ("src-tauri/src/fleet_pairing.rs", "SigningKey::from", 1),
        ("src-tauri/src/fleet_pairing.rs", ".sign(", 2),
        // recovery_kit.rs: renders the node storage keys as words; not the wallet entropy.
        ("src-tauri/src/recovery_kit.rs", "from_entropy(", 1),
        // supervisor.rs: the two reviewed libc `kill` calls (SAFETY notes inline).
        ("kit/src/supervisor.rs", "allow(unsafe_code)", 2),
        // App crate libc out-params (`flock`, `proc_bsdinfo`, `timeval`), zero-initialised for
        // FFI. The app crate cannot name `WalletKeyAccess` or `wallet_slot_get` (both
        // `pub(crate)` in kit), so these cannot reach the wallet key.
        ("src-tauri/src/node_holder.rs", "mem::zeroed", 1),
        ("src-tauri/src/owned_browser.rs", "mem::zeroed", 2),
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let root = root.canonicalize().expect("workspace root");
    let mut found: Vec<(String, String, usize)> = Vec::new();
    for path in &files {
        let rel = path
            .canonicalize()
            .expect("source path")
            .strip_prefix(&root)
            .expect("under the workspace")
            .to_string_lossy()
            .replace('\\', "/");
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if rel == "kit/src/wallet_key.rs" {
            continue;
        }
        if name.ends_with("_tests.rs") {
            let crate_sources = if rel.starts_with("kit/") { &kit_sources } else { &app_sources };
            if k6_is_cfg_test_file(path, crate_sources) {
                continue;
            }
        }
        let src = kit_sources
            .iter()
            .chain(app_sources.iter())
            .find(|f| f.path == *path)
            .expect("prepared source");
        let (all_code, non_test) = (&src.code, &src.non_test);
        for needle in &needles {
            // The capability and forging needles are counted on the whole file (comments
            // blanked, nothing stripped): a test item can never hide one (MAC round 4).
            let code = if strict.contains(needle) { all_code } else { non_test };
            let n = code.matches(needle.as_str()).count();
            if n > 0 {
                found.push((rel.clone(), needle.clone(), n));
            }
        }
    }
    let mut unexpected = Vec::new();
    for (file, needle, n) in &found {
        let ok = allowed
            .iter()
            .any(|(f, nd, c)| f == file && nd == needle && c == n);
        if !ok {
            unexpected.push(format!("{file}: `{needle}` x{n}"));
        }
    }
    assert!(
        unexpected.is_empty(),
        "wallet key material or a signing primitive is used outside the key module, or a listed \
         count changed; route it through `signing_key` or review and list it: {unexpected:?}"
    );
    // Every listed entry is still present, so the list cannot rot into a blanket pass.
    for (file, needle, c) in allowed {
        assert!(
            found.iter().any(|(f, nd, n)| f == file && nd == needle && n == c),
            "allowlist entry no longer matches the source: {file} `{needle}` x{c}"
        );
    }
    // The kit crate refuses `unsafe` by default; only the allows pinned above (and the one in the
    // supervisor test helper) re-open it.
    let lib = k6_strip_test_items(include_str!("lib.rs"));
    assert!(lib.contains("#![deny(unsafe_code)]"), "kit/src/lib.rs must deny(unsafe_code)");
    for (f, src) in [("wallet.rs", include_str!("wallet.rs")), ("wallet_key.rs", include_str!("wallet_key.rs")), ("custody.rs", include_str!("custody.rs"))] {
        assert!(k6_strip_test_items(src).contains("#![forbid(unsafe_code)]"), "{f} must forbid(unsafe_code)");
    }
    let allow = "allow(".to_string() + "unsafe_code)";
    assert_eq!(
        k6_strip_test_items(include_str!("supervisor_tests.rs")).matches(allow.as_str()).count(),
        1,
        "supervisor_tests.rs: one reviewed allow (the waitid probe)"
    );
    // The scan saw the key module's neighbours (positive control).
    assert!(files.iter().any(|p| p.ends_with("wallet.rs")) && files.len() > 50);
}

/// K6 strip helper: a `#[cfg(test)]` helper is removed without dropping the code after it.
#[test]
fn k6_strip_test_items_keeps_code_after_a_test_item() {
    let src = "fn a() {}\n#[cfg(test)]\nmod x_tests;\n#[cfg(test)]\nfn helper() {\n    t();\n}\nfn b() {}\n// c()\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn d() {}\n";
    let out = k6_strip_test_items(src);
    assert!(out.contains("fn a()") && out.contains("fn b()") && out.contains("fn d()"));
    assert!(!out.contains("x_tests") && !out.contains("t()") && !out.contains("c()"));
}

/// K6 strip helper is string-aware: a brace, `//` or `#[cfg(test)]` inside a string, raw string or
/// char literal is not structure, so code after such a test helper is still scanned, and a needle
/// inside a literal in production code is still seen.
#[test]
fn k6_strip_test_items_is_string_aware() {
    let src = concat!(
        "#[cfg(test)]\n",
        "fn helper() {\n",
        "    let a = \"{\";\n",
        "    let b = r#\"{{ \"} \"#;\n",
        "    let c = '{';\n",
        "    let d = b\"{\\\"{\";\n",
        "    let e = '\\u{7b}';\n",
        "    let f: &'static str = \"/* {\";\n",
        "}\n",
        "fn after() { let s = \"// #[cfg(test)] {\"; keep(s); }\n",
        "/* block { /* nested { */ } */\n",
        "fn tail() {}\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() { let x = '}'; }\n",
        "}\n",
        "fn last() {}\n",
    );
    let out = k6_strip_test_items(src);
    assert!(!out.contains("helper") && !out.contains("fn t()"), "test items dropped: {out}");
    assert!(out.contains("fn after()") && out.contains("keep(s)"), "code after kept: {out}");
    assert!(out.contains("\"// #[cfg(test)] {\""), "a literal is kept verbatim: {out}");
    assert!(out.contains("fn tail()") && out.contains("fn last()"), "{out}");
    assert!(!out.contains("block") && !out.contains("nested"), "comments dropped: {out}");
}

/// MAC round 4 on core#363: code after a `#[cfg(test)]` item on the same line is still scanned,
/// and an attribute line that carries its own item is not mistaken for a bare attribute.
#[test]
fn k6_strip_test_items_keeps_same_line_code_after_a_test_item() {
    let tm = "trans".to_string() + "mute";
    let allow = "allow(".to_string() + "unsafe_code)";
    let src = format!(
        "#[cfg(test)]\nconst _Z: u8 = 0; #[{allow}] pub(crate) fn f(v: &V) {{ let x = {tm}(v); g(x); }}\n\
         #[cfg(test)] fn t() {{ }} fn prod_a() {{}}\n\
         #[cfg(test)]\n#[inline] fn t2() {{}}\nfn prod_b() {{}}\n\
         #[cfg(test)]\nconst A: [u8; 2] = [0; 2]; fn prod_c() {{}}\n\
         #[cfg(test)]\n#[allow(\n    dead_code\n)]\nmod tests {{\n    fn t3() {{}}\n}}\nfn prod_d() {{}}\n"
    );
    let out = k6_strip_test_items(&src);
    assert!(!out.contains("_Z") && !out.contains("fn t()") && !out.contains("t2") && !out.contains("t3"), "{out}");
    assert!(!out.contains("const A"), "{out}");
    assert!(out.contains(allow.as_str()) && out.contains(tm.as_str()) && out.contains("fn f("), "{out}");
    for f in ["prod_a", "prod_b", "prod_c", "prod_d"] {
        assert!(out.contains(f), "{f} kept: {out}");
    }
}

/// MAC round 5 on core#363: a `#[cfg(test)]` on a struct field, an enum variant or a match arm
/// removes only that field, variant or arm; the item after the enclosing struct, enum or fn is
/// still scanned.
#[test]
fn k6_strip_test_items_bounds_a_test_field_variant_or_arm() {
    let src = concat!(
        "struct S { #[cfg(test)] t_s: u8, u_s: u8 } fn prod_s() { keep_s(); }\n",
        "struct P {\n    #[cfg(test)]\n    pub t_p: u8,\n    pub u_p: u8,\n}\nfn prod_p() {}\n",
        "struct L { u_l: u8, #[cfg(test)] t_l: u8 }\nfn prod_l() {}\n",
        "struct T(#[cfg(test)] u8, u16);\nfn prod_t() {}\n",
        "enum E { #[cfg(test)] TE, UE }\nfn prod_e() {}\n",
        "enum F { #[cfg(test)] TF { x: u8 }, UF(u8) }\nfn prod_f() {}\n",
        "fn m(v: u8) -> u8 {\n    match v {\n        #[cfg(test)]\n        0 => t_m0(),\n",
        "        #[cfg(test)]\n        1 => { t_m1() }\n        _ => u_m(),\n    }\n}\nfn prod_m() {}\n",
    );
    let out = k6_strip_test_items(src);
    for gone in ["t_s", "t_p", "t_l", "TE", "TF", "t_m0", "t_m1"] {
        assert!(!out.contains(gone), "{gone} dropped: {out}");
    }
    for kept in ["u_s", "keep_s", "u_p", "u_l", "u16", "UE", "UF", "u_m"] {
        assert!(out.contains(kept), "{kept} kept: {out}");
    }
    for f in ["prod_s", "prod_p", "prod_l", "prod_t", "prod_e", "prod_f", "prod_m"] {
        assert!(out.contains(f), "{f} kept: {out}");
    }
}

/// `unsafe` sites in `src`, by token: `(unsafe keywords, lint attributes that lower `unsafe_code`
/// below deny)`. Comments and literals are not tokens, so neither counts.
fn k6_unsafe_sites(src: &str) -> (usize, usize) {
    let t = k6_tokens(src);
    let word = |k: usize, w: &str| matches!(t.get(k), Some(K6Tok::Ident(x)) if x == w);
    let keywords = (0..t.len()).filter(|&k| word(k, "unsafe")).count();
    let mut overrides = 0;
    for k in 0..t.len() {
        let level = ["allow", "warn", "expect"].iter().any(|l| word(k, l));
        if level && matches!(t.get(k + 1), Some(K6Tok::Punct('('))) {
            let end = k6_tok_close(&t, k + 1);
            if (k + 2..end).any(|m| word(m, "unsafe_code")) {
                overrides += 1;
            }
        }
    }
    (keywords, overrides)
}

/// The unsafe-site counter on in-memory sources: every spelling of a lint override is seen.
#[test]
fn k6_unsafe_sites_sees_every_form() {
    let src = concat!(
        "#[allow(dead_code, unsafe_code)]\nfn a() { unsafe { x() } }\n",
        "#[cfg_attr(all(), expect ( unsafe_code ))]\nunsafe fn b() {}\n",
        "#![warn(\n    unsafe_code\n)]\n",
        "#[allow(unsafe_code)]\nfn c() {}\n",
        "unsafe impl Send for X {}\n",
        "// unsafe { } allow(unsafe_code)\n",
        "fn e() { let s = \"unsafe { allow(unsafe_code) }\"; }\n",
        "#[deny(unsafe_code)]\n#[forbid(unsafe_code)]\n#[allow(unsafe_op_in_unsafe_fn)]\nfn d() {}\n",
    );
    assert_eq!(k6_unsafe_sites(src), (3, 4), "keywords: a, b, impl; overrides: a, b, warn, c");
}

/// STRUCTURAL (K6): the kit crate's `deny(unsafe_code)` is what makes a private field a
/// capability. `forbid` would refuse the reviewed libc calls in `supervisor`, so instead every
/// `unsafe` keyword and every attribute that lowers the lint is pinned per file, in every .rs
/// under kit/src (comments and literals excluded, any spelling). With
/// `k6_no_module_or_include_reaches_outside_the_scanned_trees`, no kit code is outside this scan.
#[test]
fn k6_kit_unsafe_is_only_the_reviewed_blocks() {
    let kit_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    k6_collect_rs(&kit_src, &mut files);
    let allowed: &[(&str, (usize, usize))] =
        &[("supervisor.rs", (2, 2)), ("supervisor_tests.rs", (1, 1))];
    let mut bad = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&kit_src).expect("under kit/src").to_string_lossy().replace('\\', "/");
        let got = k6_unsafe_sites(&std::fs::read_to_string(f).expect("read source"));
        let want = allowed.iter().find(|(n, _)| *n == rel).map(|(_, c)| *c).unwrap_or((0, 0));
        if got != want {
            bad.push(format!("{rel}: {got:?}, expected {want:?}"));
        }
    }
    assert!(bad.is_empty(), "an unreviewed `unsafe` or unsafe_code lint override in kit: {bad:?}");
    assert!(files.len() > 30, "positive control: kit/src was walked");
}

/// One token of a lexed source (see [`k6_tokens`]).
#[derive(Debug, Clone, PartialEq)]
enum K6Tok {
    Ident(String),
    Punct(char),
    /// A string literal: its text for a plain or raw string (`"…"`, `r"…"`, `r#"…"#`) with no
    /// escape; `None` for any other (escaped, byte or C string).
    Lit(Option<String>),
}

/// The tokens of `src`, whitespace-insensitive: comments are dropped and every string literal is
/// one token (so nothing inside a literal or a comment is structure), raw identifiers (`r#x`)
/// read as their name, and everything else is an identifier or a single punctuation character.
fn k6_tokens(src: &str) -> Vec<K6Tok> {
    let (code, shape) = k6_lex(src);
    let n = shape.len();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    // From the opening quote at `q`: the index of the closing quote (literal contents are blank
    // in `shape`, so it is the next quote).
    let close_quote = |q: usize| {
        let mut k = q + 1;
        while k < n && shape[k] != '"' {
            k += 1;
        }
        k
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = shape[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' {
            let k = close_quote(i);
            let v: String = code[i + 1..k.min(n)].iter().collect();
            out.push(K6Tok::Lit((k < n && !v.contains('\\')).then_some(v)));
            i = k + 1;
            continue;
        }
        if !ident(c) {
            out.push(K6Tok::Punct(c));
            i += 1;
            continue;
        }
        let start = i;
        while i < n && ident(shape[i]) {
            i += 1;
        }
        let w: String = shape[start..i].iter().collect();
        let mut j = i;
        while j < n && shape[j] == '#' {
            j += 1;
        }
        let quoted = j < n && shape[j] == '"';
        match w.as_str() {
            "r" | "br" | "cr" if quoted => {
                let k = close_quote(j);
                let v: String = code[j + 1..k.min(n)].iter().collect();
                out.push(K6Tok::Lit((w == "r" && k < n).then_some(v)));
                i = (k + 1 + (j - i)).min(n);
            }
            "b" | "c" if i < n && shape[i] == '"' => {
                out.push(K6Tok::Lit(None));
                i = close_quote(i) + 1;
            }
            "r" if j == i + 1 && j < n && ident(shape[j]) => {
                let s2 = j;
                while j < n && ident(shape[j]) {
                    j += 1;
                }
                out.push(K6Tok::Ident(shape[s2..j].iter().collect()));
                i = j;
            }
            _ => out.push(K6Tok::Ident(w)),
        }
    }
    out
}

/// From an opening bracket token at `i`, the index of its matching closer (`toks.len()` if none).
fn k6_tok_close(toks: &[K6Tok], i: usize) -> usize {
    let mut depth = 0i64;
    for (k, t) in toks.iter().enumerate().skip(i) {
        match t {
            K6Tok::Punct('(' | '[' | '{') => depth += 1,
            K6Tok::Punct(')' | ']' | '}') => {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
            _ => {}
        }
    }
    toks.len()
}

/// Every source file `src` compiles in, with the literal that names it; `None` for a form the
/// scan cannot resolve. Token-based, so spacing, line breaks, delimiters and literal kind do not
/// matter, and nothing inside a comment or a literal counts:
/// - `include!` (any path prefix, any delimiter): its literal argument;
/// - `#[path = …]`, and every `path = …` inside a `#[cfg_attr(…)]` (at any nesting);
/// - a `use` that names `include` (a renamed `include!` the scan cannot follow): `None`.
/// - an attribute whose body is a macro fragment (`#[$a]`, `#![$a]`, `#[$($t)*]`), or a
///   `cfg_attr` holding one: `None`, since the fragment can expand to a `path`.
///
/// `include_str!` / `include_bytes!` read a file as data and compile nothing, so they are not
/// references.
fn k6_module_refs(src: &str) -> Vec<Option<String>> {
    let t = k6_tokens(src);
    let punct = |k: usize, p: char| matches!(t.get(k), Some(K6Tok::Punct(c)) if *c == p);
    let word = |k: usize, w: &str| matches!(t.get(k), Some(K6Tok::Ident(x)) if x == w);
    let lit = |k: usize| match t.get(k) {
        Some(K6Tok::Lit(v)) => v.clone(),
        _ => None,
    };
    let mut out = Vec::new();
    let mut in_use = false;
    for k in 0..t.len() {
        if word(k, "use") {
            in_use = true;
        } else if punct(k, ';') {
            in_use = false;
        }
        if word(k, "include") {
            if punct(k + 1, '!') && (punct(k + 2, '(') || punct(k + 2, '[') || punct(k + 2, '{')) {
                out.push(lit(k + 3));
            } else if in_use {
                out.push(None);
            }
            continue;
        }
        if !punct(k, '#') {
            continue;
        }
        let open = if punct(k + 1, '!') { k + 2 } else { k + 1 };
        if !punct(open, '[') {
            continue;
        }
        let end = k6_tok_close(&t, open);
        if punct(open + 1, '$') {
            // `#[$a]`, `#[$($t)*]`: the whole attribute is a macro fragment.
            out.push(None);
        } else if word(open + 1, "path") && punct(open + 2, '=') {
            out.push(lit(open + 3));
        } else if (open + 1..end).any(|m| word(m, "cfg_attr")) {
            for m in open + 1..end {
                if word(m, "path") && punct(m + 1, '=') {
                    out.push(lit(m + 2));
                }
            }
            // A fragment inside `cfg_attr` can expand to a `path`.
            if (open + 1..end).any(|m| punct(m, '$')) {
                out.push(None);
            }
        }
    }
    out
}

/// Whether a module/include path literal stays under the crate's source root when resolved from
/// `file`: relative, no `..` component, and (if it exists) canonically under `root`.
fn k6_ref_in_tree(file: &std::path::Path, lit: &str, root: &std::path::Path) -> bool {
    let p = std::path::Path::new(lit);
    if p.is_absolute() || lit.starts_with('/') || lit.starts_with('\\') || lit.contains(':') {
        return false;
    }
    if p.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return false;
    }
    let dir = file.parent().unwrap_or(root);
    match dir.join(p).canonicalize() {
        Ok(abs) => root.canonicalize().map(|r| abs.starts_with(r)).unwrap_or(false),
        Err(_) => true, // not a file on disk: the build would fail before shipping it
    }
}

/// The source files a rustc dep-info (`.d`) file lists: the prerequisites of every rule, in
/// order, with `\ ` read as a space. Comment lines (`# env-dep:…`) carry no sources.
fn k6_dep_info_sources(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        // The rule's target ends at the first `:` followed by a space or the line end (a
        // Windows drive colon is followed by `\`).
        let b = line.as_bytes();
        let Some(colon) =
            (0..b.len()).find(|&i| b[i] == b':' && (i + 1 == b.len() || b[i + 1] == b' '))
        else {
            continue;
        };
        let mut cur = String::new();
        let mut chars = line[colon + 1..].chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' && chars.peek() == Some(&' ') {
                cur.push(' ');
                chars.next();
            } else if c.is_whitespace() {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            } else {
                cur.push(c);
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

/// Whether a dep-info path (relative to the workspace root `ws`, or absolute) lies under one of
/// `roots`. A path that exists is compared canonically, so a symlink counts as its target; a
/// missing one (a stale entry for a deleted file) is compared lexically, with `..` applied.
fn k6_dep_in(p: &str, ws: &std::path::Path, roots: &[std::path::PathBuf]) -> bool {
    use std::path::{Component, Path, PathBuf};
    let lexical = |p: &Path| {
        let mut n = PathBuf::new();
        for c in p.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    n.pop();
                }
                other => n.push(other),
            }
        }
        n
    };
    let path = ws.join(p);
    match path.canonicalize() {
        Ok(abs) => roots
            .iter()
            .any(|r| r.canonicalize().map(|r| abs.starts_with(r)).unwrap_or(false)),
        Err(_) => {
            let n = lexical(&path);
            roots.iter().any(|r| n.starts_with(lexical(r)))
        }
    }
}

/// STRUCTURAL (K6, MAC round 4 on core#363): the key-access scan reads every .rs under kit/src
/// and src-tauri/src, so no source may compile in a file from outside those trees: every
/// `#[path = "…"]` and `include!("…")` must name a plain relative literal that resolves under its
/// crate's source root. That includes a `path` inside `#[cfg_attr(…)]` and any spacing or
/// delimiter of `include!` (see [`k6_module_refs`]).
#[test]
fn k6_no_module_or_include_reaches_outside_the_scanned_trees() {
    let kit_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let app_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src-tauri/src");
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for root in [&kit_src, &app_src] {
        let mut files = Vec::new();
        k6_collect_rs(root, &mut files);
        for f in &files {
            let src = std::fs::read_to_string(f).expect("read source");
            for r in k6_module_refs(&src) {
                seen += 1;
                match r {
                    Some(lit) if k6_ref_in_tree(f, &lit, root) => {}
                    other => bad.push(format!("{}: {other:?}", f.display())),
                }
            }
        }
    }
    assert!(bad.is_empty(), "a module or include outside the scanned source trees: {bad:?}");
    assert!(seen > 30, "positive control: the scan saw the in-tree includes ({seen})");
}

/// The reference finder and resolver used above, on in-memory sources.
#[test]
fn k6_module_refs_flags_out_of_tree_and_unresolvable_forms() {
    let src = concat!(
        "#[path = \"a_tests.rs\"]\nmod a;\n",
        "#[ path=\"../x.rs\" ]\nmod x;\n",
        "mod t { include!(\"sub/b.rs\"); }\n",
        "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n",
        "include!(\"/abs/c.rs\");\n",
        "let s = \"include!(\\\"../hidden.rs\\\")\";\n",
        "// include!(\"../comment.rs\");\n",
        "#[pathological = \"../no.rs\"]\nfn z() {}\n",
        // MAC round 5: a conditional path, spacing, other delimiters, raw literals, renames.
        "#[cfg_attr(not(test), path = \"../cfg_a.rs\")]\nmod ca;\n",
        "#[cfg_attr(all(), inline, path=\"../cfg_b.rs\")]\nmod cb;\n",
        "#[cfg_attr(\n    unix,\n    cfg_attr(all(), path\n        = \"../cfg_c.rs\")\n)]\nmod cc;\n",
        "#[cfg_attr(any(), path = r\"../cfg_d.rs\")]\nmod cd;\n",
        "#[path = r#\"../raw.rs\"#]\nmod r;\n",
        "# [\n  path = \"../split.rs\"\n]\nmod s;\n",
        "include ! (\"../spaced.rs\");\n",
        "core::include![\"../bracket.rs\"];\n",
        "std::include! {\"../brace.rs\"}\n",
        "use std::include as inc;\n",
        "#[path = $p]\nmod m;\n",
        // MAC round 6: an attribute built from a macro fragment names no literal the scan can
        // read, so it is unresolvable wherever the fragment sits.
        "macro_rules! m { ($a:meta) => { #[$a] mod x; } }\nm!(path = \"../outside.rs\");\n",
        "macro_rules! n { ($($t:tt)*) => { #[$($t)*] mod y; } }\n",
        "macro_rules! o { ($a:meta) => { #![$a] }; }\n",
        "macro_rules! q { ($b:meta) => { #[cfg_attr(all(), $b)] mod z; } }\n",
        // Data, not code: read as text or bytes, never compiled.
        "let d = include_str ! (\"../data.txt\");\nlet b = include_bytes!(\"../blob.bin\");\n",
        // A conditional attribute with no path is not a reference.
        "#[cfg_attr(test, allow(dead_code))]\nfn y() {}\n",
    );
    let refs = k6_module_refs(src);
    let s = |v: &str| Some(v.to_string());
    assert_eq!(
        refs,
        vec![
            s("a_tests.rs"),
            s("../x.rs"),
            s("sub/b.rs"),
            None,
            s("/abs/c.rs"),
            s("../cfg_a.rs"),
            s("../cfg_b.rs"),
            s("../cfg_c.rs"),
            s("../cfg_d.rs"),
            s("../raw.rs"),
            s("../split.rs"),
            s("../spaced.rs"),
            s("../bracket.rs"),
            s("../brace.rs"),
            None,
            None,
            None,
            None,
            None,
            None,
        ],
        "literals, comments and look-alike attributes are not references; every path and \
         include form is, whatever its spacing, delimiter or literal kind"
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let file = root.join("wallet.rs");
    assert!(k6_ref_in_tree(&file, "wallet_key.rs", &root));
    assert!(k6_ref_in_tree(&file, "ceremony/anchor.rs", &root));
    assert!(!k6_ref_in_tree(&file, "../Cargo.toml", &root));
    assert!(!k6_ref_in_tree(&file, "./wallet_key.rs", &root));
    assert!(!k6_ref_in_tree(&file, "/etc/hosts", &root));
}

/// STRUCTURAL (K6, MAC round 6 on core#363): what rustc actually read. The token scans above
/// look for every way a source can pull in another file; this asks the compiler instead. rustc
/// writes the full source list of each build to a dep-info file beside the binary, and every
/// macro, attribute and include form ends up there. So every source of this test binary must lie
/// under kit/src. Any other kit build whose dep-info sits in the same directory (the kit lib,
/// which `cargo test --workspace` builds first) is held to kit/src too.
///
/// The app's dep-info is checked as far as it can be. It also lists the many data files the app
/// reads with `include_str!` / `include_bytes!` (fixtures, JSON, TypeScript sources for parity
/// tests), and dep-info does not say which entries were compiled. So for an app build: every `.rs`
/// entry lies under src-tauri/src or the app build script's OUT_DIR, and every other entry lies
/// inside this checkout. A non-`.rs` file compiled through `#[path]` or `include!` is left to the
/// token scans above.
#[test]
fn k6_compiled_sources_stay_in_the_scanned_trees() {
    use std::path::{Path, PathBuf};
    let kit = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ws = kit.parent().expect("workspace root");
    let exe = std::env::current_exe().expect("test binary path");
    let own = exe.with_extension("d");
    let deps = exe.parent().expect("deps dir");
    let mut app_roots = vec![ws.join("src-tauri/src")];
    if let Ok(rd) = std::fs::read_dir(deps.join("../build")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("citrate-core-") && e.path().join("out").is_dir() {
                app_roots.push(e.path().join("out"));
            }
        }
    }
    let kit_roots = vec![kit.join("src")];
    let mut data_roots = app_roots[1..].to_vec();
    data_roots.push(ws.to_path_buf());
    let mut files = vec![own.clone()];
    for e in std::fs::read_dir(deps).expect("read deps dir").flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let crate_dep_info = name.ends_with(".d")
            && ["citrate_core_kit-", "citrate_core_lib-", "citrate_core-"]
                .iter()
                .any(|pre| name.starts_with(pre));
        if crate_dep_info && p != own {
            files.push(p);
        }
    }
    let mut bad = Vec::new();
    let mut own_sources = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f)
            .unwrap_or_else(|e| panic!("read dep-info {}: {e}", f.display()));
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_kit = name.starts_with("citrate_core_kit-");
        let srcs = k6_dep_info_sources(&text);
        for s in &srcs {
            let roots: &[PathBuf] = if is_kit {
                &kit_roots
            } else if s.ends_with(".rs") {
                &app_roots
            } else {
                &data_roots
            };
            if !k6_dep_in(s, ws, roots) {
                bad.push(format!("{}: {s}", f.display()));
            }
        }
        if *f == own {
            own_sources = srcs;
        }
    }
    assert!(bad.is_empty(), "a build compiled a source outside its scanned tree: {bad:?}");
    assert!(
        own_sources.iter().any(|s| s.ends_with("wallet_tests.rs")) && own_sources.len() > 20,
        "positive control: this binary's dep-info lists its own sources ({own_sources:?})"
    );
}

/// The dep-info reader and path check used above, on an in-memory dep-info file.
#[test]
fn k6_dep_info_sources_reads_every_listed_file() {
    let d = concat!(
        "/t/deps/x-1: kit/src/lib.rs kit/src/a\\ b.rs kit/src/../zz.rs /abs/out.rs ",
        "C:\\w\\kit\\src\\c.rs\n",
        "\n",
        "/t/deps/x-1.d: kit/src/lib.rs\n",
        "\n",
        "kit/src/lib.rs:\n",
        "kit/src/a\\ b.rs:\n",
        "\n",
        "# env-dep:CARGO_MANIFEST_DIR=/t/kit: not/a/source.rs\n",
    );
    assert_eq!(
        k6_dep_info_sources(d),
        vec![
            "kit/src/lib.rs",
            "kit/src/a b.rs",
            "kit/src/../zz.rs",
            "/abs/out.rs",
            "C:\\w\\kit\\src\\c.rs",
            "kit/src/lib.rs",
        ],
        "every prerequisite of every rule, escaped spaces read, comment lines skipped"
    );
    let kit = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ws = kit.parent().expect("workspace root");
    let roots = [kit.join("src")];
    assert!(k6_dep_in("kit/src/wallet.rs", ws, &roots));
    assert!(k6_dep_in("kit/src/ceremony/anchor.rs", ws, &roots));
    assert!(k6_dep_in("kit/src/deleted_since.rs", ws, &roots), "a stale in-tree entry");
    assert!(!k6_dep_in("kit/src/../zz_outside.rs", ws, &roots), "`..` out of the tree");
    assert!(!k6_dep_in("kit/src/../Cargo.toml", ws, &roots), "an existing file outside");
    assert!(!k6_dep_in("kit/Cargo.toml", ws, &roots));
    assert!(!k6_dep_in("src-tauri/src/lib.rs", ws, &roots), "another crate's tree");
    assert!(!k6_dep_in("/etc/hosts", ws, &roots));
    let abs = kit.join("src/rpc.rs");
    assert!(k6_dep_in(abs.to_str().expect("utf-8 path"), ws, &roots), "an absolute in-tree path");
}

/// K6, MAC round 6 on core#363: the scans read `#[cfg(test)]` as test-only, which holds only
/// while nothing sets `cfg(test)` for a non-test build. A build script could (`cargo:rustc-cfg=
/// test`); kit has none, and Cargo sets `OUT_DIR` for exactly the packages that do, so the
/// compiler records the answer here. (The app's build script is pinned in src-tauri.)
#[test]
fn k6_kit_has_no_build_script() {
    assert_eq!(
        option_env!("OUT_DIR"),
        None,
        "kit gained a build script; the cfg(test) exemptions in these scans assume it has none"
    );
    let kit = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(!kit.join("build.rs").exists(), "kit/build.rs");
}

/// K6 scan exemption: a `*_tests.rs` file is skipped only when it is compiled under `cfg(test)`.
#[test]
fn k6_tests_file_is_exempt_only_under_cfg_test() {
    // In-memory sources under a path that is never read or written.
    let d = std::path::Path::new("/k6-scan-fixture");
    let lib = concat!(
        "#[cfg(test)]\nmod a_tests;\n",
        "pub mod b_tests;\n",
        "#[cfg(test)]\nmod tests {\n    include!(\"e_tests.rs\");\n}\n",
        "#[cfg(test)]\nfn h() { let s = \"{\"; }\n",
        "mod tests2 {\n    include!(\"f_tests.rs\");\n}\n",
        "#[cfg(test)]\nmod t3 {\n    include!(\"g_tests.rs\");\n}\n",
        "mod p3 {\n    include ! (\"g_tests.rs\");\n}\n",
        "#[cfg(test)]\nmod h_tests;\n",
        "#[cfg_attr(not(test), path = \"h_tests.rs\")]\nmod h_prod;\n",
        "#[cfg(test)]\nmod i_tests;\n",
        "pub mod\n    i_tests;\n",
    );
    let srcs: Vec<K6Source> = [
        ("lib.rs", lib),
        ("a_tests.rs", "fn x() {}\n"),
        ("b_tests.rs", "fn x() {}\n"),
        ("c_tests.rs", "#![cfg(test)]\nfn x() {}\n"),
        ("d_tests.rs", "fn x() {}\n"),
        ("e_tests.rs", "fn x() {}\n"),
        ("f_tests.rs", "fn x() {}\n"),
        ("g_tests.rs", "fn x() {}\n"),
        ("h_tests.rs", "fn x() {}\n"),
        ("i_tests.rs", "fn x() {}\n"),
    ]
    .iter()
    .map(|(n, s)| k6_prep(d.join(n), s))
    .collect();
    let exempt = |n: &str| k6_is_cfg_test_file(&d.join(n), &srcs);
    assert!(exempt("a_tests.rs"), "#[cfg(test)] mod");
    assert!(!exempt("b_tests.rs"), "a non-test `pub mod x_tests;` is scanned");
    assert!(exempt("c_tests.rs"), "#![cfg(test)] at the top");
    assert!(!exempt("d_tests.rs"), "an orphan is scanned");
    assert!(exempt("e_tests.rs"), "include! inside a #[cfg(test)] mod");
    assert!(!exempt("f_tests.rs"), "include! after a test helper holding a brace literal");
    assert!(!exempt("g_tests.rs"), "a spaced `include ! (..)` in non-test code is a reference");
    assert!(!exempt("h_tests.rs"), "a conditional `path` in non-test code is a reference");
    assert!(!exempt("i_tests.rs"), "`mod` and the name on separate lines is a reference");
}

/// K6: plain `custody_get` refuses the wallet slot even on an unlocked vault holding a wallet, so
/// a module that knows the slot name still cannot read the entropy. The capability-gated read in
/// the key module still works (address derivation succeeds).
#[test]
fn k6_custody_get_refuses_the_wallet_slot() {
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import");
    assert!(v.is_unlocked());
    assert_eq!(v.custody_get(WALLET_ENTROPY_SLOT).unwrap_err(), CustodyError::Denied);
    assert_eq!(v.custody_get("wallet-anything").unwrap_err(), CustodyError::Denied);
    let info = address(&v).expect("the key module still reads it");
    assert!(info.address.eq_ignore_ascii_case(CANONICAL_ADDRESS));
}

/// K6: create still refuses to clobber an existing wallet now that its probe goes through the key
/// module, and a non-wallet slot is still readable through plain `custody_get`.
#[test]
fn k6_create_probe_still_sees_an_existing_wallet() {
    let (v, _p) = init_and_unlock();
    import(&v, CANONICAL_MNEMONIC).expect("import");
    assert_eq!(create(&v).unwrap_err(), WalletError::AlreadyExists);
    assert_eq!(import(&v, CANONICAL_MNEMONIC).unwrap_err(), WalletError::AlreadyExists);
    let mut other = b"not-a-wallet".to_vec();
    v.put("app-other", &mut other).expect("put");
    assert_eq!(v.custody_get("app-other").expect("read").as_slice(), b"not-a-wallet");
}
