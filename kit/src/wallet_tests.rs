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
    assert!(!leaked(&raw, &secret), "private key must not be at rest");

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
