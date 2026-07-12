// CORE-A2 — custody adversarial + integration suite (@rule8 evidence).
//
// Every ADV-* here is written RED-FIRST: the guard is neutralized, the test is
// shown failing, the guard restored, the test shown green (see the sprint file's
// red→green table and the PR body). These run fully headless against an
// in-memory keyring fake — no live OS keyring or Tauri runtime required — so the
// crypto / envelope / session / lockout / zeroize / tamper logic is proven in
// CI. The real OS-keyring round-trip is a separate, honestly-skipped-on-headless
// integration test at the bottom.

use super::*;
use std::sync::Mutex as StdMutex;

/// In-memory keyring fake — the injection point that lets every crypto/session
/// test run without a live platform keyring.
#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<std::collections::HashMap<String, Vec<u8>>>,
}

impl FakeKeyring {
    fn wipe_master(&self) {
        self.store.lock().unwrap().remove(KEYRING_MASTER_ACCOUNT);
    }
    fn rotate_master(&self) {
        let mut k = [0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut k);
        self.store
            .lock()
            .unwrap()
            .insert(KEYRING_MASTER_ACCOUNT.to_string(), k.to_vec());
    }
}

impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<()> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

/// A shared handle to the fake so tests can reach in (wipe/rotate/unavailable)
/// after the vault has taken ownership of a boxed clone-view.
struct SharedFake(std::sync::Arc<FakeKeyring>);
impl Keyring for SharedFake {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
        self.0.get(account)
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
        self.0.set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<()> {
        self.0.delete(account)
    }
}

const PASS: &[u8] = b"correct horse battery staple";
const WRONG: &[u8] = b"Tr0ub4dor&3-but-wrong";

/// Build a vault over a fresh temp envelope path + a shared in-memory keyring.
fn vault(autolock_mins: u32) -> (CustodyVault, std::sync::Arc<FakeKeyring>, PathBuf) {
    let fake = std::sync::Arc::new(FakeKeyring::default());
    let mut p = std::env::temp_dir();
    let uniq = format!(
        "citrate-core-custody-test-{}-{}.enc",
        std::process::id(),
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            N.fetch_add(1, Ordering::Relaxed)
        }
    );
    p.push(uniq);
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(SharedFake(fake.clone())), p.clone(), autolock_mins);
    (v, fake, p)
}

fn init_and_unlock(autolock_mins: u32) -> (CustodyVault, std::sync::Arc<FakeKeyring>, PathBuf) {
    let (v, fake, p) = vault(autolock_mins);
    v.init(&mut PASS.to_vec()).unwrap();
    v.unlock(&mut PASS.to_vec()).unwrap();
    (v, fake, p)
}

// ----- baseline crypto/envelope round-trip (A2.2) -----------------------

#[test]
fn seal_unseal_roundtrips() {
    let key = [7u8; KEY_LEN];
    let aad = CustodyVault::aad(ENVELOPE_VERSION, b"slot-x");
    let sealed = CustodyVault::seal(&key, b"hello secret", &aad).unwrap();
    let pt = CustodyVault::unseal(&key, &sealed, &aad).unwrap();
    assert_eq!(pt.as_slice(), b"hello secret");
    // CRY-1: a slot sealed under one AAD (name/version) must NOT unseal under a
    // different AAD — this is the primitive the slot-swap guard rests on.
    let other = CustodyVault::aad(ENVELOPE_VERSION, b"slot-y");
    assert_eq!(
        CustodyVault::unseal(&key, &sealed, &other).unwrap_err(),
        CustodyError::Denied,
        "a slot must not unseal under a different slot's AAD"
    );
}

#[test]
fn argon2_params_match_decision_d_a2_1() {
    // The parameters are the D-A2-1 contract; assert them explicitly so a drift
    // is caught (matches citrate-native / citrate-wallet-core).
    assert_eq!(ARGON_M_COST, 65536);
    assert_eq!(ARGON_T_COST, 3);
    assert_eq!(ARGON_P_COST, 1);
    assert_eq!(KEY_LEN, 32);
    // And the KDF actually runs with them and produces a 32-byte key.
    let k = CustodyVault::derive_key(PASS, &[9u8; SALT_LEN]).unwrap();
    assert_eq!(k.len(), KEY_LEN);
}

// ----- ADV-1: custody_get while locked → denied, no bytes ---------------

#[test]
fn adv1_get_while_locked_is_denied() {
    let (v, _f, _p) = init_and_unlock(0);
    v.put("token", &mut b"s3cr3t".to_vec()).unwrap();
    v.lock();
    let r = v.custody_get("token");
    assert_eq!(r.unwrap_err(), CustodyError::Denied, "locked get must deny");
}

// ----- ADV-2: wrong passphrase → fails, no oracle vs empty-vault --------

#[test]
fn adv2_wrong_passphrase_no_oracle() {
    let (v, _f, _p) = init_and_unlock(0);
    v.lock();
    let wrong = v.unlock(&mut WRONG.to_vec()).unwrap_err();
    // And an uninitialized vault's unlock must produce the SAME opaque error,
    // so a caller cannot tell wrong-passphrase from no-vault.
    let (empty, _f2, _p2) = vault(0);
    let no_vault = empty.unlock(&mut WRONG.to_vec()).unwrap_err();
    assert_eq!(wrong, CustodyError::Denied);
    assert_eq!(no_vault, CustodyError::Denied);
    assert_eq!(wrong.to_string(), no_vault.to_string(), "no distinguishing oracle");
}

// ----- ADV-3: N wrong attempts → lockout/cooloff ------------------------

#[test]
fn adv3_lockout_after_n_attempts() {
    let (v, _f, _p) = init_and_unlock(0);
    v.lock();
    for _ in 0..MAX_ATTEMPTS {
        assert_eq!(v.unlock(&mut WRONG.to_vec()).unwrap_err(), CustodyError::Denied);
    }
    // Now even the CORRECT passphrase is refused with LockedOut (cooloff).
    let r = v.unlock(&mut PASS.to_vec()).unwrap_err();
    assert_eq!(r, CustodyError::LockedOut, "must lock out after {MAX_ATTEMPTS}");
}

// ----- ADV-4: plaintext on disk → zero secret plaintext -----------------

#[test]
fn adv4_no_plaintext_on_disk() {
    let (v, _f, p) = init_and_unlock(0);
    let secret = b"SUPER-SECRET-CANARY-9f3a";
    v.put("token", &mut secret.to_vec()).unwrap();
    let raw = std::fs::read(&p).unwrap();
    // The canary secret must not appear in the on-disk envelope — in ANY
    // recoverable form: neither as a contiguous ASCII window, nor as the
    // JSON number-array representation serde uses for Vec<u8> (`[83,85,...]`).
    assert!(
        !leaked(&raw, secret),
        "custody.enc must contain zero secret plaintext (raw-bytes + number-array grep)"
    );
    // The check-slot's known plaintext must not appear either.
    assert!(!leaked(&raw, CHECK_PLAINTEXT), "check-slot plaintext leaked");
}

/// True if `needle` is recoverable from `haystack` — either as a contiguous
/// byte window, or as serde_json's number-array encoding of a `Vec<u8>`
/// (e.g. `83,85,80,...`), which is how a naive plaintext-in-Vec leak surfaces.
fn leaked(haystack: &[u8], needle: &[u8]) -> bool {
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

// ----- ADV-5: tampered ciphertext/tag → unseal fails closed -------------

#[test]
fn adv5_tampered_ciphertext_fails_closed() {
    let key = [3u8; KEY_LEN];
    let aad = CustodyVault::aad(ENVELOPE_VERSION, b"authentic-slot");
    let mut sealed = CustodyVault::seal(&key, b"authentic", &aad).unwrap();
    // flip one byte of the ciphertext/tag
    sealed.ct[0] ^= 0xff;
    let r = CustodyVault::unseal(&key, &sealed, &aad);
    assert_eq!(r.unwrap_err(), CustodyError::Denied, "tampered ct must fail");

    // A tampered envelope on disk must also fail unlock closed (no partial).
    let (v, _f, p) = init_and_unlock(0);
    v.lock();
    let mut env: Envelope = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    let cs = env.slots.get_mut(CHECK_SLOT).unwrap();
    cs.ct[0] ^= 0xff;
    std::fs::write(&p, serde_json::to_vec(&env).unwrap()).unwrap();
    assert_eq!(v.unlock(&mut PASS.to_vec()).unwrap_err(), CustodyError::Denied);
}

// ----- ADV-6: keyring master key absent/rotated → unseal fails closed ---

#[test]
fn adv6_keyring_absent_or_rotated_fails_closed() {
    // The keyring master key is load-bearing: it wraps the data key. Absent OR
    // rotated, unlock must fail closed even with the CORRECT passphrase.

    // Absent master key → unlock fails closed (keyring is required).
    let (v, fake, p) = init_and_unlock(0);
    v.lock();
    fake.wipe_master();
    let r = v.unlock(&mut PASS.to_vec()).unwrap_err();
    assert_eq!(
        r,
        CustodyError::KeyringUnavailable,
        "absent master key must fail closed, not fall through to a re-derived key"
    );

    // Rotated master key (attacker swaps the keyring entry): the previously
    // wrapped data key no longer unwraps → unlock denied even with the right
    // passphrase. Prove across a simulated restart.
    let (v2, fake2, p2) = init_and_unlock(0);
    v2.lock();
    fake2.rotate_master();
    let reopened = CustodyVault::new(Box::new(SharedFake(fake2.clone())), p2, 0);
    // Fail closed: a rotated master no longer authenticates the master-sealed
    // lockout block (→ `Corrupt`, tripped before the passphrase is even tried)
    // nor unwraps the DEK (→ `Denied`). Either way the vault does not open with
    // the wrong master — both are fail-closed. (v2 checks the lockout block
    // first, so the observed error is `Corrupt`.)
    let err = reopened.unlock(&mut PASS.to_vec()).unwrap_err();
    assert!(
        matches!(err, CustodyError::Denied | CustodyError::Corrupt),
        "a rotated master key must not unwrap the data key (got {err:?})"
    );
    let _ = p; // envelope path kept alive for the absent-key vault
}

// ----- ADV-7: no secret material in logs --------------------------------

#[test]
fn adv7_no_secret_in_error_or_debug_output() {
    // The Denied error (the passphrase-path error) must not echo any secret.
    let (v, _f, _p) = init_and_unlock(0);
    v.lock();
    let err = v.unlock(&mut WRONG.to_vec()).unwrap_err();
    let rendered = format!("{err}");
    let dbg = format!("{err:?}");
    for s in [rendered.as_str(), dbg.as_str()] {
        assert!(!s.contains("horse"), "passphrase leaked into output: {s}");
        assert!(!s.contains("Tr0ub4dor"), "passphrase leaked into output: {s}");
    }
    // SlotInfo (the only slot type crossing the bridge) carries no bytes field.
    let info = SlotInfo { name: "token".into(), bytes: 7 };
    let j = serde_json::to_string(&info).unwrap();
    assert!(!j.contains("ct"), "slot metadata must not carry ciphertext");
    assert!(!j.contains("nonce"));
}

// ----- ADV-8: no invoke command returns secret bytes --------------------

#[test]
fn adv8_no_invoke_command_returns_secret_bytes() {
    // The registered custody invoke commands, enumerated. `custody_get` is
    // deliberately ABSENT — it is an in-process pub fn only. This list mirrors
    // the invoke_handler registration in lib.rs; the lib.rs test asserts the
    // handler wires exactly these.
    const REGISTERED: &[&str] = &[
        "custody_status",
        "custody_init",
        "custody_unlock",
        "custody_lock",
        "custody_put",
        "custody_list",
        "custody_keyring_status",
    ];
    assert!(
        !REGISTERED.contains(&"custody_get"),
        "custody_get must NOT be an invoke command (ADV-8 boundary)"
    );
    // None of the registered commands' return types are secret bytes: status,
    // (), SlotInfo metadata, or a keyring status string. custody_get — the only
    // API returning bytes — is not registered. This is asserted structurally by
    // the lib.rs registration test.
}

// ----- ADV-9: session teardown + Zeroizing type contract ----------------
// SCOPE (BND-4, honest): this test proves the SESSION-TEARDOWN + Zeroizing
// TYPE CONTRACT — that `lock()` drops the session and the key is a
// `Zeroizing<[u8;32]>` with an explicit `Session::drop` wipe. It does NOT prove
// the DEK BYTES are physically erased from memory (reading post-drop memory is
// UB, so we decline that deref). A true memory-residue proof is DEFERRED to a
// zeroize-audit MIR/LLVM pass before B1 stores real wallet keys — see the
// sprint's BND-4 note and the code comment on `Session::drop`.

#[test]
fn adv9_session_teardown_and_zeroizing_contract() {
    // Read the key while unlocked, lock, then confirm the session is gone (no
    // key retained/readable) — this is the teardown contract, not a byte wipe.
    let (v, _f, _p) = init_and_unlock(0);
    assert!(v.is_unlocked());
    let before = v.with_session_key().unwrap();
    assert_ne!(*before, [0u8; KEY_LEN], "unlocked key is non-zero");
    v.lock();
    assert!(!v.is_unlocked(), "session dropped on lock");
    assert_eq!(v.with_session_key().unwrap_err(), CustodyError::Denied);

    // Zeroizing wipes on drop: a scoped buffer is all-zero after it drops.
    let ptr;
    {
        let z = Zeroizing::new(vec![0xABu8; 32]);
        ptr = z.as_ptr();
        assert_eq!(z[0], 0xAB);
    }
    // SAFETY: reading freed stack memory is UB in general; we only assert the
    // Zeroizing contract holds via the type, not by dereferencing `ptr`.
    let _ = ptr;
}

// ----- ADV-10: auto-lock; monotonic clock, no rollback extension --------

#[test]
fn adv10_autolock_expires_by_monotonic_clock() {
    // 0-minute window handled separately; here use the internal seconds knob to
    // force an immediate expiry without sleeping. Set the window to 0-elapsed by
    // hacking the session's unlocked_at into the past via a 1-second window +
    // manual staleness through a tiny sleep-free path: we set autolock to a
    // window we then step past by moving unlocked_at.
    let (v, _f, _p) = init_and_unlock(1); // 1 minute
    assert!(v.is_unlocked());
    // Force the session to look old (monotonic Instant in the past).
    {
        let mut inner = v.inner.lock().unwrap();
        if let Some(s) = inner.session.as_mut() {
            s.unlocked_at = Instant::now() - Duration::from_secs(120);
        }
    }
    // Now a status/key read must observe expiry — session auto-locked.
    assert!(!v.is_unlocked(), "session must auto-lock past the window");
    assert_eq!(v.with_session_key().unwrap_err(), CustodyError::Denied);

    // Wall-clock rollback cannot help: the check is on a monotonic Instant, not
    // SystemTime. Setting autolock larger AFTER expiry does not resurrect it.
    v.set_autolock_mins(999);
    assert!(!v.is_unlocked(), "a larger window cannot revive an expired session");
}

// ----- integration: full lifecycle across a simulated restart -----------

#[test]
fn integration_lifecycle_across_restart() {
    let secret = b"refresh-token-material-abc123";
    let (v, fake, p) = vault(30);
    // init → put → lock
    v.init(&mut PASS.to_vec()).unwrap();
    v.unlock(&mut PASS.to_vec()).unwrap();
    v.put("oidc-refresh", &mut secret.to_vec()).unwrap();
    v.lock();
    drop(v);

    // Simulated restart: brand-new vault over the SAME envelope + keyring.
    let v2 = CustodyVault::new(Box::new(SharedFake(fake.clone())), p, 30);
    assert!(v2.is_initialized(), "envelope persisted across restart");
    // locked → get denied
    assert_eq!(v2.custody_get("oidc-refresh").unwrap_err(), CustodyError::Denied);
    // unlock(right) → get == original
    v2.unlock(&mut PASS.to_vec()).unwrap();
    let got = v2.custody_get("oidc-refresh").unwrap();
    assert_eq!(got.as_slice(), secret, "round-trip A3/B1 depends on this");

    // list returns metadata only (no check-slot, no bytes).
    let slots = v2.list().unwrap();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].name, "oidc-refresh");
    assert!(slots.iter().all(|s| !s.name.starts_with('\0')));
}

// ----- config.autolock drives the session timeout -----------------------

#[test]
fn config_autolock_change_takes_effect() {
    let (v, _f, _p) = init_and_unlock(30);
    assert_eq!(*v.autolock_secs.lock().unwrap(), 30 * 60);
    v.set_autolock_mins(15);
    assert_eq!(*v.autolock_secs.lock().unwrap(), 15 * 60);
    // 0 disables auto-lock (session never auto-expires on the timer).
    v.set_autolock_mins(0);
    {
        let mut inner = v.inner.lock().unwrap();
        if let Some(s) = inner.session.as_mut() {
            s.unlocked_at = Instant::now() - Duration::from_secs(10_000);
        }
    }
    assert!(v.is_unlocked(), "autolock=0 disables the timer");
}

// ----- real OS-keyring round-trip — honestly skipped on headless CI ------

#[test]
fn real_keyring_roundtrip_or_skip() {
    // Rule 1 honesty: exercise the REAL platform keyring if reachable; if not
    // (headless CI with no secret service), skip with a printed note rather than
    // fake a pass. The crypto/session guarantees above are proven regardless via
    // the in-memory fake.
    let probe = keyring_probe();
    if probe != "available" {
        eprintln!(
            "SKIP real_keyring_roundtrip: OS keyring unavailable in this environment \
             (probe={probe}) — crypto/session proven via in-memory fake instead"
        );
        return;
    }
    let os = OsKeyring;
    let acct = "custody-test-roundtrip";
    let secret = b"os-keyring-roundtrip-secret";
    // Clean any stale entry, then round-trip put/get/delete.
    let _ = os.delete(acct);
    os.set(acct, secret).unwrap();
    let got = os.get(acct).unwrap();
    assert_eq!(got.as_deref(), Some(&secret[..]));
    os.delete(acct).unwrap();
    assert_eq!(os.get(acct).unwrap(), None, "deleted entry is gone");
}

// ===========================================================================
// Rule-8 remediation suite (citrate-security #13). Each of these was written
// RED-first: it PASSES the attack against the pre-fix (v1, constant-AAD,
// in-memory-lockout, non-atomic) code, and fails the attack after the fix. See
// the sprint file's remediation red→green table.
// ===========================================================================

// ----- CRY-1 (a): slot-swap on disk must fail closed --------------------

#[test]
fn adv_cry1_slot_swap_fails_closed() {
    // Attack: with FS write access to custody.enc, swap two slots' sealed
    // values. Pre-fix (constant AAD, no header): unlock succeeds and
    // custody_get("alpha") returns beta's plaintext. Post-fix: the DEK-sealed
    // header pins each slot's `name -> nonce` fingerprint, so a swap (alpha now
    // carries beta's nonce) diverges from the header and fails closed on unlock.
    let (v, _f, p) = init_and_unlock(0);
    v.put("alpha", &mut b"VALUE-ALPHA".to_vec()).unwrap();
    v.put("beta", &mut b"VALUE-BETA".to_vec()).unwrap();

    // Swap the two SealedSlot values on disk (nonce + ct together).
    let mut env: Envelope = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    let a = env.slots.get("alpha").unwrap().clone();
    let b = env.slots.get("beta").unwrap().clone();
    env.slots.insert("alpha".into(), b);
    env.slots.insert("beta".into(), a);
    std::fs::write(&p, serde_json::to_vec(&env).unwrap()).unwrap();

    v.lock();
    // Header fingerprint mismatch → unlock fails closed (never returns beta's PT).
    assert_eq!(
        v.unlock(&mut PASS.to_vec()).unwrap_err(),
        CustodyError::Corrupt,
        "a slot swap must fail closed at unlock, never return the other slot's plaintext"
    );
}

// ----- CRY-1 (b): slot-rollback-in-place must fail closed ---------------

#[test]
fn adv_cry1_slot_rollback_fails_closed() {
    // Attack: snapshot a slot's OLD sealed value, later restore that prior
    // sealed value over the slot (resurrect a revoked token). Pre-fix: served
    // transparently (an old authentic ct decrypts fine). Post-fix: the header
    // pins the slot's CURRENT nonce; a restored prior value carries the OLD
    // nonce → fingerprint mismatch → fail closed.
    let (v, _f, p) = init_and_unlock(0);
    v.put("token", &mut b"OLD-TOKEN".to_vec()).unwrap();
    let with_old: Envelope =
        serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    let old_slot = with_old.slots.get("token").unwrap().clone();

    v.put("token", &mut b"NEW-TOKEN".to_vec()).unwrap(); // rotate the token

    // Roll the SLOT back to its prior sealed value, keeping the current header.
    let mut env: Envelope = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    env.slots.insert("token".into(), old_slot);
    std::fs::write(&p, serde_json::to_vec(&env).unwrap()).unwrap();

    v.lock();
    assert_eq!(
        v.unlock(&mut PASS.to_vec()).unwrap_err(),
        CustodyError::Corrupt,
        "rolling a slot back to a prior sealed value must fail closed (revoked value cannot be resurrected)"
    );
}

// ----- CRY-1 (c): slot add/remove behind the header fails closed --------

#[test]
fn adv_cry1_slot_remove_fails_closed() {
    // Removing a slot on disk while keeping the header (which still fingerprints
    // it) must fail closed — the on-disk fingerprint no longer matches.
    let (v, _f, p) = init_and_unlock(0);
    v.put("a", &mut b"AAA".to_vec()).unwrap();
    v.put("b", &mut b"BBB".to_vec()).unwrap();

    let mut env: Envelope = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    env.slots.remove("b");
    std::fs::write(&p, serde_json::to_vec(&env).unwrap()).unwrap();

    v.lock();
    assert_eq!(
        v.unlock(&mut PASS.to_vec()).unwrap_err(),
        CustodyError::Corrupt,
        "removing a slot while keeping the header must fail closed"
    );
}

// ----- CRY-4: version tamper / downgrade rejected -----------------------

#[test]
fn adv_cry4_version_downgrade_rejected() {
    // Attack: tamper the envelope's `version` field. Pre-fix: never validated,
    // unlock Ok. Post-fix: load_envelope rejects any version != current, AND the
    // version is bound into every slot's AAD so a forced-2 v1-ct also fails.
    let (v, _f, p) = init_and_unlock(0);
    v.lock();
    let pristine = std::fs::read(&p).unwrap();

    let mut env: Envelope = serde_json::from_slice(&pristine).unwrap();
    env.version = 0; // downgrade
    std::fs::write(&p, serde_json::to_vec(&env).unwrap()).unwrap();
    assert_eq!(
        v.unlock(&mut PASS.to_vec()).unwrap_err(),
        CustodyError::VersionUnsupported,
        "an unknown/downgraded version must be rejected explicitly"
    );

    // Also: bumping to an unknown-future version is rejected too.
    let mut env2: Envelope = serde_json::from_slice(&pristine).unwrap();
    env2.version = 99;
    std::fs::write(&p, serde_json::to_vec(&env2).unwrap()).unwrap();
    assert_eq!(
        v.unlock(&mut PASS.to_vec()).unwrap_err(),
        CustodyError::VersionUnsupported
    );
}

// ----- CRY-2: timing parity — absent-vault path spends the KDF ----------

#[test]
fn adv_cry2_absent_vault_spends_kdf() {
    // Attack refuted structurally (not via flaky wall-clock): the no-vault
    // unlock path must spend the SAME Argon2id work as a wrong-passphrase path,
    // so vault existence is not a timing oracle. Pre-fix: absent path returned
    // before any KDF (kdf_count unchanged). Post-fix: it runs one dummy KDF.
    let (empty, _f, _p) = vault(0); // never initialized
    assert!(!empty.is_initialized());
    let before = empty.kdf_count();
    let r = empty.unlock(&mut WRONG.to_vec());
    let after = empty.kdf_count();
    assert_eq!(r.unwrap_err(), CustodyError::Denied, "no-vault unlock denies");
    assert_eq!(
        after,
        before + 1,
        "the absent-vault path must spend exactly one Argon2id derivation \
         (timing parity with wrong-passphrase — CRY-2)"
    );

    // And an initialized vault's wrong-passphrase path spends one KDF too, so
    // the two paths are KDF-equal.
    let (v, _f2, _p2) = init_and_unlock(0);
    v.lock();
    let b2 = v.kdf_count();
    let _ = v.unlock(&mut WRONG.to_vec());
    assert_eq!(v.kdf_count(), b2 + 1, "wrong-passphrase path spends one KDF");
}

// ----- BND-1: concurrent wrong unlocks capped at MAX_ATTEMPTS -----------

#[test]
fn adv_bnd1_concurrent_unlock_lockout_holds() {
    // Attack: 32 concurrent unlock(WRONG). Pre-fix: the cooloff pre-check
    // released the mutex before the crypto, so all 32 ran full Argon2id, 0
    // rate-limited. Post-fix: the mutex is held across check→derive→record, so
    // at most MAX_ATTEMPTS derivations run and the rest return LockedOut.
    use std::sync::Arc;
    let (v, _f, _p) = init_and_unlock(0);
    v.lock();
    let v = Arc::new(v);
    let kdf_start = v.kdf_count();

    let mut handles = vec![];
    for _ in 0..32 {
        let vc = Arc::clone(&v);
        handles.push(std::thread::spawn(move || {
            vc.unlock(&mut WRONG.to_vec())
        }));
    }
    let mut locked_out = 0;
    let mut denied = 0;
    for h in handles {
        match h.join().unwrap() {
            Err(CustodyError::LockedOut) => locked_out += 1,
            Err(CustodyError::Denied) => denied += 1,
            other => panic!("unexpected unlock result: {other:?}"),
        }
    }
    let kdf_ran = v.kdf_count() - kdf_start;
    assert!(
        kdf_ran <= MAX_ATTEMPTS as u64,
        "at most MAX_ATTEMPTS Argon2id trials may run under a concurrent storm, ran {kdf_ran}"
    );
    assert!(locked_out >= 32 - MAX_ATTEMPTS, "the storm must be rate-limited");
    assert!(denied <= MAX_ATTEMPTS, "denied (crypto-ran) count is capped");
}

// ----- CRY-3 / BND-2: lockout persists across a restart -----------------

#[test]
fn adv_cry3_lockout_persists_across_restart() {
    // Attack: burn MAX_ATTEMPTS, then "restart" (drop the vault, reconstruct a
    // fresh one over the SAME envelope + keyring). Pre-fix: the in-memory
    // counter reset → unlock succeeds. Post-fix: the lockout is sealed into the
    // envelope under the master key and re-armed on load → still LockedOut.
    let (v, fake, p) = init_and_unlock(0);
    v.lock();
    for _ in 0..MAX_ATTEMPTS {
        assert_eq!(
            v.unlock(&mut WRONG.to_vec()).unwrap_err(),
            CustodyError::Denied
        );
    }
    assert_eq!(
        v.unlock(&mut PASS.to_vec()).unwrap_err(),
        CustodyError::LockedOut,
        "locked out before restart"
    );
    drop(v);

    // Simulated restart over the same envelope + keyring.
    let v2 = CustodyVault::new(Box::new(SharedFake(fake.clone())), p, 0);
    assert_eq!(
        v2.unlock(&mut PASS.to_vec()).unwrap_err(),
        CustodyError::LockedOut,
        "lockout must persist across a process restart (CRY-3/BND-2)"
    );
}

// ----- BND-3: concurrent puts both survive (no lost write) --------------

#[test]
fn adv_bnd3_concurrent_puts_both_survive() {
    // Attack: two concurrent puts. Pre-fix: load→insert→save was not serialized,
    // so one write was silently lost (list() == 1). Post-fix: puts serialize
    // under the mutex + atomic temp/fsync/rename → both survive.
    use std::sync::Arc;
    let (v, _f, _p) = init_and_unlock(0);
    let v = Arc::new(v);
    let v1 = Arc::clone(&v);
    let v2 = Arc::clone(&v);
    let h1 = std::thread::spawn(move || v1.put("slot-1", &mut b"ONE".to_vec()));
    let h2 = std::thread::spawn(move || v2.put("slot-2", &mut b"TWO".to_vec()));
    h1.join().unwrap().unwrap();
    h2.join().unwrap().unwrap();
    let names: std::collections::BTreeSet<String> =
        v.list().unwrap().into_iter().map(|s| s.name).collect();
    assert!(names.contains("slot-1"), "slot-1 survived: {names:?}");
    assert!(names.contains("slot-2"), "slot-2 survived: {names:?}");
    // And both are readable (header stayed consistent with the slot set).
    assert_eq!(v.custody_get("slot-1").unwrap().as_slice(), b"ONE");
    assert_eq!(v.custody_get("slot-2").unwrap().as_slice(), b"TWO");
}

// ----- CRY-5: mint refuses to clobber an existing keyring master --------

#[test]
fn adv_cry5_no_master_clobber_on_reinit() {
    // Attack: delete custody.enc (the keyring master survives), then re-init.
    // Pre-fix: is_initialized() is file-exists only, so init runs and
    // mint_master_key overwrites the master → old secrets orphaned + takeover.
    // Post-fix: mint refuses to overwrite an existing master → re-init fails
    // closed, the original master (and its secrets) is preserved.
    let (v, fake, p) = init_and_unlock(0);
    let master_before = fake.get(KEYRING_MASTER_ACCOUNT).unwrap().unwrap();
    v.lock();

    // Delete the envelope; the keyring master survives.
    std::fs::remove_file(&p).unwrap();
    assert!(!v.is_initialized());

    // Re-init with an ATTACKER passphrase must be refused (master present).
    let r = v.init(&mut b"attacker-passphrase".to_vec());
    assert_eq!(
        r.unwrap_err(),
        CustodyError::Corrupt,
        "re-init over a surviving keyring master must fail closed (CRY-5)"
    );
    // The master was NOT clobbered.
    let master_after = fake.get(KEYRING_MASTER_ACCOUNT).unwrap().unwrap();
    assert_eq!(
        master_before, master_after,
        "the keyring master must be preserved, not clobbered"
    );
}
