//! citrate-core — custody vault (CORE-A2). @rule8 · T1 key-custody surface.
//!
//! The encrypted, OS-keyring-backed vault every future secret lives in: OIDC
//! refresh tokens (A3), the wallet keystore (B1), gateway keys. It is the
//! on-chain-custody invariant (I-2) made real in the native backend — keys live
//! only in the app process and the OS keyring, nothing else.
//!
//! A2 is custody STORAGE ONLY. Per CLAUDE.md rule 3 there are **no signing code
//! paths** here, and no wallet-key generation/import (that is B1). A2 delivers
//! the vault and proves it, adversarially, with a generic secret slot.
//!
//! ## Design
//! - **Master key** in the OS keyring under service `ai.citrate.core`, account
//!   `custody-master-key`. The keyring holds the 32-byte wrapping key; the
//!   on-disk envelope holds the secrets. The keyring is abstracted behind the
//!   [`Keyring`] trait so the crypto/session logic is testable headless with an
//!   in-memory fake AND runs against the real platform backend in production.
//! - **Envelope** `custody.enc` in the app data dir: each slot sealed with
//!   AES-256-GCM. The per-vault **data key** is derived by Argon2id
//!   (m=65536 KiB, t=3, p=1, 32-byte — D-A2-1) from the user passphrase, then
//!   itself wrapped by the keyring master key. No passphrase hash is stored;
//!   unlock is trial-decrypt of a known check-slot — a wrong passphrase fails
//!   the GCM tag with no distinguishing oracle.
//! - **Session:** `unlock(passphrase)` derives + holds the data key in memory;
//!   `lock()` / auto-lock drop and zeroize it. Every secret buffer is zeroized.
//! - **Lockout:** N failed unlocks → cooloff (5 attempts → 5 minutes),
//!   constant-time tag check, no error oracle.
//! - **Boundary:** `custody_get` is a plain in-process `pub fn` for A3/B1, NOT a
//!   `#[tauri::command]`. No invoke command ever returns secret bytes (ADV-8).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use zeroize::{Zeroize, Zeroizing};

// ---------------------------------------------------------------------------
// D-A2-1 crypto parameters (match citrate-native / citrate-wallet-core exactly)
// ---------------------------------------------------------------------------

/// Argon2id memory cost, KiB.
const ARGON_M_COST: u32 = 65536;
/// Argon2id time cost (iterations).
const ARGON_T_COST: u32 = 3;
/// Argon2id parallelism.
const ARGON_P_COST: u32 = 1;
/// Derived-key / master-key length, bytes.
const KEY_LEN: usize = 32;
/// AES-256-GCM nonce length, bytes.
const NONCE_LEN: usize = 12;
/// Argon2id salt length, bytes.
const SALT_LEN: usize = 16;

/// Lockout policy: N failed unlocks → cooloff (citrate-native pattern).
const MAX_ATTEMPTS: u32 = 5;
const LOCKOUT_COOLOFF: Duration = Duration::from_secs(5 * 60);

/// Current on-disk envelope format version. Bumped to 2 for the integrity
/// rework (CRY-1/CRY-4): per-slot AAD binding + a DEK-sealed authenticated
/// header (generation counter + slot-set) + a master-key-sealed lockout block.
/// v1 envelopes have no integrity binding and are rejected on load (CRY-4).
const ENVELOPE_VERSION: u8 = 2;

/// AAD domain-separation tags. Each is combined with the version byte so a slot
/// sealed at one identity/version cannot be replayed at another (CRY-1/CRY-4).
const AAD_WRAPPED_DK: &[u8] = b"wrapped_dk";
const AAD_HEADER: &[u8] = b"header";
const AAD_LOCKOUT: &[u8] = b"lockout";

/// OS keyring coordinates for the wrapping (master) key.
const KEYRING_SERVICE: &str = "ai.citrate.core";
const KEYRING_MASTER_ACCOUNT: &str = "custody-master-key";

/// Envelope file name inside the app data dir.
const ENVELOPE_FILE: &str = "custody.enc";

/// The reserved check-slot: a known plaintext sealed under the data key. Unlock
/// trial-decrypts it; a wrong passphrase fails the GCM tag. Its name starts with
/// a NUL so it can never collide with a caller slot and is filtered from `list`.
const CHECK_SLOT: &str = "\0custody-check";
const CHECK_PLAINTEXT: &[u8] = b"citrate-core custody check-slot v1";

// ---------------------------------------------------------------------------
// Errors — deliberately coarse so there is no oracle (ADV-2). "wrong passphrase"
// and "no vault" both surface as the same opaque `Denied`.
// ---------------------------------------------------------------------------

/// Custody error. The variants that touch the unlock path all map to the same
/// user-facing string so a caller cannot distinguish wrong-passphrase from
/// empty-vault (ADV-2), nor learn anything from a lockout beyond "locked out".
#[derive(Debug, PartialEq, Eq)]
pub enum CustodyError {
    /// Denied: wrong passphrase, no vault, or session locked — no oracle.
    Denied,
    /// Too many failed attempts; retry after cooloff.
    LockedOut,
    /// The OS keyring backend is unreachable or the master key is absent.
    KeyringUnavailable,
    /// The on-disk envelope is malformed or tampered.
    Corrupt,
    /// The envelope declares an unknown or downgraded format version (CRY-4).
    /// Distinct from `Corrupt` because it is a load-path guard, not on the
    /// passphrase-guessing oracle surface.
    VersionUnsupported,
    /// I/O or serialization failure.
    Io(String),
}

impl std::fmt::Display for CustodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Denied is intentionally non-specific — no wrong-vs-empty oracle.
            CustodyError::Denied => write!(f, "denied"),
            CustodyError::LockedOut => write!(f, "locked out: too many attempts, retry later"),
            CustodyError::KeyringUnavailable => write!(f, "keyring unavailable"),
            CustodyError::Corrupt => write!(f, "custody envelope corrupt or tampered"),
            CustodyError::VersionUnsupported => {
                write!(f, "custody envelope version unsupported or downgraded")
            }
            CustodyError::Io(m) => write!(f, "custody io error: {m}"),
        }
    }
}

impl std::error::Error for CustodyError {}

type Result<T> = std::result::Result<T, CustodyError>;

// ---------------------------------------------------------------------------
// Keyring seam — the wrapping key lives in the OS keyring in production, an
// in-memory map in tests. This is the injection point the honesty note requires.
// ---------------------------------------------------------------------------

/// The OS-keyring seam. Production uses [`OsKeyring`]; tests use an in-memory
/// fake so all crypto/session logic runs headless without a live keyring.
///
/// `get`/`delete` are part of the seam surface and are exercised by the
/// keyring round-trip + master-key-rotation tests and by future A3/B1
/// consumers (master-key rotation); allow dead_code so the non-test lib build
/// (where the tests are cfg-gated out) does not flag the seam as unused.
#[allow(dead_code)]
pub trait Keyring: Send + Sync {
    /// Read the secret for `account`, or `None` if absent. `Err` means the
    /// backend itself is unreachable (fail closed).
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>>;
    /// Store the secret for `account`.
    fn set(&self, account: &str, secret: &[u8]) -> Result<()>;
    /// Delete the secret for `account` (idempotent).
    fn delete(&self, account: &str) -> Result<()>;
}

/// The real platform keyring, via the `keyring` v3 crate. Secrets are stored
/// base64-free as raw bytes through `set_secret`/`get_secret`.
pub struct OsKeyring;

impl Keyring for OsKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| CustodyError::KeyringUnavailable)?;
        match entry.get_secret() {
            Ok(bytes) => Ok(Some(bytes)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CustodyError::KeyringUnavailable),
        }
    }

    fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| CustodyError::KeyringUnavailable)?;
        entry
            .set_secret(secret)
            .map_err(|_| CustodyError::KeyringUnavailable)
    }

    fn delete(&self, account: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| CustodyError::KeyringUnavailable)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CustodyError::KeyringUnavailable),
        }
    }
}

// ---------------------------------------------------------------------------
// On-disk envelope
// ---------------------------------------------------------------------------

/// One sealed slot: AES-256-GCM ciphertext (tag appended) + its nonce. The slot
/// name is the map key. Only ciphertext is persisted — never plaintext.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SealedSlot {
    /// 12-byte GCM nonce.
    nonce: Vec<u8>,
    /// Ciphertext with the 16-byte GCM tag appended.
    ct: Vec<u8>,
}

/// The persisted envelope. `salt` derives the data key from the passphrase; the
/// data key is then wrapped by the keyring master key (`wrapped_dk`). Slots hold
/// only ciphertext. Format-versioned; version is validated + AAD-bound on load.
///
/// ## Integrity model (v2 — CRY-1)
/// GCM authenticates each slot's *bytes*, but nothing in v1 authenticated *where*
/// a slot sat or the slot set as a whole, so a disk-write attacker could swap or
/// roll back slots. v2 binds:
/// - **per-slot AAD** = `version || slot_name` (and `version || "wrapped_dk"`),
///   so a slot sealed under one name/version cannot be replayed under another;
/// - a **DEK-sealed `header`** carrying a monotonic **generation counter** and
///   the exact **slot-name set**, verified on unlock before any slot is trusted
///   — this catches whole-set swap/rollback and add/remove of slots;
/// - a **master-key-sealed `lockout`** block (`failures` + absolute deadline),
///   so the lockout survives a process restart (CRY-3/BND-2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Envelope {
    version: u8,
    /// Argon2id salt (public; a salt is not secret).
    salt: Vec<u8>,
    /// The data key sealed under the keyring master key: nonce + ciphertext.
    /// AAD = `version || "wrapped_dk"`.
    wrapped_dk: SealedSlot,
    /// Authenticated header sealed under the DEK. AAD = `version || "header"`.
    /// Plaintext is the JSON of [`Header`] (generation + slot-name set).
    header: SealedSlot,
    /// Lockout state sealed under the keyring MASTER key (not the DEK — it must
    /// be updatable on a *failed* unlock, where no DEK is available).
    /// AAD = `version || "lockout"`. Plaintext is the JSON of [`LockoutState`].
    lockout: SealedSlot,
    /// Sealed slots keyed by name (includes the reserved check-slot). Each slot's
    /// AAD = `version || slot_name`.
    slots: BTreeMap<String, SealedSlot>,
}

/// The DEK-sealed authenticated header. Binds the whole slot set — each slot's
/// name AND its exact sealed version (via its unique random nonce) — to a
/// generation counter, so any swap, rollback-in-place, or add/remove of a slot
/// is caught on unlock. Sealed under the DEK, which a disk-write attacker does
/// not have, so it cannot be forged to match tampered slots.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Header {
    /// Monotonic generation counter, incremented on every envelope mutation.
    generation: u64,
    /// `slot_name -> slot nonce` for every slot present when this header was
    /// sealed. The GCM nonce is fresh-random per seal, so it uniquely fingerprints
    /// a slot's exact sealed version. On unlock we require the on-disk map to
    /// equal this map: a swapped slot (wrong nonce under a name), a rolled-back
    /// slot (a prior nonce), or an added/removed slot all diverge and fail closed.
    slots: BTreeMap<String, Vec<u8>>,
}

impl Header {
    /// The `name -> nonce` fingerprint of a slot map.
    fn fingerprint(slots: &BTreeMap<String, SealedSlot>) -> BTreeMap<String, Vec<u8>> {
        slots
            .iter()
            .map(|(name, s)| (name.clone(), s.nonce.clone()))
            .collect()
    }
}

/// The master-key-sealed lockout block. Persisted so a process restart cannot
/// reset the failed-attempt counter (CRY-3/BND-2). `locked_until_ms` is an
/// ABSOLUTE wall-clock deadline (unix millis); see the clock-rollback caveat on
/// [`CustodyVault::unlock_inner`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct LockoutState {
    failures: u32,
    /// Absolute unix-ms deadline the current cooloff (if any) ends. `None` = no
    /// active cooloff.
    locked_until_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Session + vault
// ---------------------------------------------------------------------------

/// The in-memory data key held only while unlocked. Zeroized on drop / lock.
struct Session {
    /// The AES-256 data key, zeroized on drop.
    data_key: Zeroizing<[u8; KEY_LEN]>,
    /// When this session was established (for auto-lock).
    unlocked_at: Instant,
}

/// Metadata about a slot returned across the bridge — **never** the bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotInfo {
    pub name: String,
    /// Ciphertext length in bytes (metadata only, no plaintext leak).
    pub bytes: usize,
}

/// The custody vault. Holds the keyring seam + envelope path; guards a session
/// and attempt-state behind a mutex. All secret material is zeroized on drop.
pub struct CustodyVault {
    keyring: Box<dyn Keyring>,
    path: PathBuf,
    inner: Mutex<VaultInner>,
    /// Auto-lock window in seconds (from `config.autolock`, minutes → seconds).
    autolock_secs: Mutex<u64>,
    /// Count of Argon2id derivations performed (CRY-2 testability). Lets a test
    /// assert *structurally* that the absent-vault path still spends the KDF —
    /// no flaky wall-clock timing. Incremented inside `derive_key`.
    kdf_invocations: std::sync::atomic::AtomicU64,
}

/// In-memory vault state guarded by the mutex. The lockout counter is NOT here
/// — it is persisted (sealed under the master key) so a restart cannot reset it
/// (CRY-3/BND-2). The mutex serializes the whole unlock critical section so at
/// most one Argon2id trial is in flight (BND-1) and envelope mutations are
/// serialized + atomic (BND-3).
#[derive(Default)]
struct VaultInner {
    session: Option<Session>,
}

impl CustodyVault {
    /// Build a vault over a keyring seam and an envelope path. `autolock_mins`
    /// comes from `config.autolock` (the A1 single source of truth).
    pub fn new(keyring: Box<dyn Keyring>, path: PathBuf, autolock_mins: u32) -> Self {
        CustodyVault {
            keyring,
            path,
            inner: Mutex::new(VaultInner::default()),
            autolock_secs: Mutex::new(autolock_mins as u64 * 60),
            kdf_invocations: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// How many Argon2id derivations this vault has performed. Test-only seam
    /// for CRY-2 (assert the absent-vault path still spends the KDF), and for
    /// BND-1 (assert at most `MAX_ATTEMPTS` derivations run under a concurrent
    /// wrong-guess storm). `#[cfg(test)]` keeps it out of the shipped surface.
    #[cfg(test)]
    fn kdf_count(&self) -> u64 {
        self.kdf_invocations
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether an envelope exists on disk (the vault has been initialized).
    pub fn is_initialized(&self) -> bool {
        self.path.exists()
    }

    /// Update the auto-lock window (config.autolock changed). Minutes → seconds.
    /// Consumed by the config-change wire (A3/B1) and the auto-lock tests; allow
    /// dead_code so the non-test lib build does not flag this seam as unused.
    #[allow(dead_code)]
    pub fn set_autolock_mins(&self, mins: u32) {
        *self.autolock_secs.lock().unwrap() = mins as u64 * 60;
    }

    // --- Argon2id KDF (D-A2-1) ------------------------------------------

    /// Derive the data key from the passphrase + salt with the D-A2-1 params.
    /// The passphrase is treated as caller-owned; we do not retain it.
    fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        let params = Params::new(ARGON_M_COST, ARGON_T_COST, ARGON_P_COST, Some(KEY_LEN))
            .map_err(|e| CustodyError::Io(format!("argon2 params: {e}")))?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut out = Zeroizing::new([0u8; KEY_LEN]);
        argon
            .hash_password_into(passphrase, salt, out.as_mut())
            .map_err(|_| CustodyError::Denied)?;
        Ok(out)
    }

    /// The single accounted KDF call site (CRY-2). Every real *and* dummy
    /// Argon2id derivation on the unlock path goes through here so the
    /// invocation counter reflects the true KDF work spent — this is the
    /// structural, non-flaky signal the timing-parity test asserts on.
    fn derive_key_counted(
        &self,
        passphrase: &[u8],
        salt: &[u8],
    ) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        self.kdf_invocations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self::derive_key(passphrase, salt)
    }

    // --- AES-256-GCM seal / unseal --------------------------------------

    /// Build the GCM AAD binding an item to `version || context`. `context` is
    /// the slot name for a caller/check slot, or one of the `AAD_*` domain tags
    /// for the wrapped DEK / header / lockout. Binding the version defeats a
    /// downgrade (CRY-4); binding the name/tag defeats slot swap (CRY-1).
    fn aad(version: u8, context: &[u8]) -> Vec<u8> {
        let mut aad = Vec::with_capacity(1 + context.len());
        aad.push(version);
        aad.extend_from_slice(context);
        aad
    }

    /// Seal `plaintext` under `key` with the given `aad`, returning a
    /// fresh-nonce sealed slot. The AAD is authenticated but not stored (it is
    /// reconstructed from `version || context` on unseal).
    fn seal(key: &[u8; KEY_LEN], plaintext: &[u8], aad: &[u8]) -> Result<SealedSlot> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CustodyError::Corrupt)?;
        Ok(SealedSlot {
            nonce: nonce_bytes.to_vec(),
            ct,
        })
    }

    /// Unseal a slot under `key`, authenticating `aad`. GCM tag failure (wrong
    /// key, tampered ciphertext, OR a slot replayed under the wrong name/version
    /// — a mismatched AAD) fails closed with no partial plaintext
    /// (ADV-2 / ADV-5 / CRY-1). The tag comparison inside `aes-gcm` is
    /// constant-time.
    fn unseal(key: &[u8; KEY_LEN], slot: &SealedSlot, aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if slot.nonce.len() != NONCE_LEN {
            return Err(CustodyError::Corrupt);
        }
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let nonce = Nonce::from_slice(&slot.nonce);
        let pt = cipher
            .decrypt(nonce, Payload { msg: &slot.ct, aad })
            .map_err(|_| CustodyError::Denied)?;
        Ok(Zeroizing::new(pt))
    }

    // --- envelope persistence -------------------------------------------

    fn load_envelope(&self) -> Result<Envelope> {
        let bytes = std::fs::read(&self.path).map_err(|_| CustodyError::Denied)?;
        let env: Envelope = serde_json::from_slice(&bytes).map_err(|_| CustodyError::Corrupt)?;
        // CRY-4: validate the format version explicitly. Only the current
        // version is accepted; an unknown or downgraded version is rejected up
        // front (before any key material is trusted) rather than silently taking
        // a weaker path. The version is ALSO bound into every slot's AAD (CRY-1),
        // so tampering it to `2` while keeping v1-shaped ciphertext still fails
        // the GCM tag — this check is the fast, explicit first line.
        if env.version != ENVELOPE_VERSION {
            return Err(CustodyError::VersionUnsupported);
        }
        Ok(env)
    }

    /// Persist the envelope atomically (BND-3): write a temp file, fsync it,
    /// then rename over the target. A crash mid-write leaves either the old or
    /// the new envelope, never a truncated one. Callers hold the vault mutex, so
    /// mutations are also serialized (no lost-write TOCTOU).
    fn save_envelope(&self, env: &Envelope) -> Result<()> {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CustodyError::Io(e.to_string()))?;
        }
        let bytes = serde_json::to_vec(env).map_err(|e| CustodyError::Io(e.to_string()))?;
        let tmp = self.path.with_extension("enc.tmp");
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| CustodyError::Io(e.to_string()))?;
            f.write_all(&bytes)
                .map_err(|e| CustodyError::Io(e.to_string()))?;
            f.sync_all().map_err(|e| CustodyError::Io(e.to_string()))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| CustodyError::Io(e.to_string()))?;
        Ok(())
    }

    /// Read (or lazily mint) the keyring master (wrapping) key.
    fn master_key(&self) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        match self.keyring.get(KEYRING_MASTER_ACCOUNT)? {
            Some(bytes) => {
                if bytes.len() != KEY_LEN {
                    return Err(CustodyError::KeyringUnavailable);
                }
                let mut k = [0u8; KEY_LEN];
                k.copy_from_slice(&bytes);
                let out = Zeroizing::new(k);
                // scrub the transient copy from the keyring read
                let mut bytes = bytes;
                bytes.zeroize();
                Ok(out)
            }
            None => Err(CustodyError::KeyringUnavailable),
        }
    }

    /// Mint a fresh keyring master key for a brand-new vault. CRY-5: refuses to
    /// overwrite an existing keyring master. If a master is already present the
    /// keyring is not clean — either a live vault owns it (and re-init would
    /// clobber it, orphaning every existing secret) or the envelope was deleted
    /// out from under a surviving master (tamper / partial-recovery). Either way
    /// we fail closed with `Corrupt` rather than silently taking over.
    fn mint_master_key(&self) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        if self.keyring.get(KEYRING_MASTER_ACCOUNT)?.is_some() {
            return Err(CustodyError::Corrupt);
        }
        let mut k = Zeroizing::new([0u8; KEY_LEN]);
        rand::thread_rng().fill_bytes(k.as_mut());
        self.keyring.set(KEYRING_MASTER_ACCOUNT, k.as_ref())?;
        Ok(k)
    }

    // --- public API ------------------------------------------------------

    /// Initialize a fresh vault from a passphrase: mint the keyring master key,
    /// derive the data key, wrap it under the master key, seal the check-slot.
    /// Fails closed if a vault already exists.
    pub fn init(&self, passphrase: &mut [u8]) -> Result<()> {
        // Serialize init under the same mutex as unlock/put so a concurrent
        // init cannot race the keyring-master mint (BND-1/BND-3).
        let _guard = self.inner.lock().unwrap();
        let result = self.init_inner(passphrase);
        passphrase.zeroize(); // scrub the caller's inbound passphrase (spec)
        result
    }

    fn init_inner(&self, passphrase: &[u8]) -> Result<()> {
        if self.is_initialized() {
            return Err(CustodyError::Io("vault already initialized".into()));
        }
        // Two independent secrets, both REQUIRED to recover the data key:
        //  - the keyring master key (`mek`, minted here, lives in the OS keyring)
        //  - the passphrase-derived key (`kek` = Argon2id(passphrase, salt)).
        // The wrapping key is their combination; the random data key (`dek`)
        // that actually seals slots is sealed under it. Losing either the
        // keyring entry OR the passphrase makes the vault unrecoverable — this
        // is what makes the keyring genuinely load-bearing (ADV-6). CRY-5:
        // mint refuses to clobber an existing keyring master.
        let mek = self.mint_master_key()?;
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        let kek = self.derive_key_counted(passphrase, &salt)?;
        let wrap_key = Self::combine_keys(&mek, &kek);

        let mut dek = Zeroizing::new([0u8; KEY_LEN]);
        rand::thread_rng().fill_bytes(dek.as_mut());
        let v = ENVELOPE_VERSION;
        let wrapped_dk = Self::seal(&wrap_key, dek.as_ref(), &Self::aad(v, AAD_WRAPPED_DK))?;

        // The check-slot, sealed under the DEK with per-slot AAD (CRY-1).
        let mut slots = BTreeMap::new();
        slots.insert(
            CHECK_SLOT.to_string(),
            Self::seal(&dek, CHECK_PLAINTEXT, &Self::aad(v, CHECK_SLOT.as_bytes()))?,
        );

        // Generation-0 header binds the slot set under the DEK (CRY-1 rollback).
        let header = Self::seal_header(&dek, v, 0, &slots)?;
        // Fresh lockout block, sealed under the master key (CRY-3/BND-2).
        let lockout = Self::seal_lockout(&mek, v, &LockoutState::default())?;

        let env = Envelope {
            version: v,
            salt: salt.to_vec(),
            wrapped_dk,
            header,
            lockout,
            slots,
        };
        self.save_envelope(&env)?;
        Ok(())
    }

    // --- header (generation + slot-set) sealed under the DEK ------------

    /// Seal the authenticated header binding `generation` + the slot fingerprint
    /// (`name -> nonce`) under the DEK. The check-slot is included; a v2 unlock
    /// requires the on-disk fingerprint to match exactly (CRY-1 swap/rollback).
    fn seal_header(
        dek: &[u8; KEY_LEN],
        version: u8,
        generation: u64,
        slots: &BTreeMap<String, SealedSlot>,
    ) -> Result<SealedSlot> {
        let header = Header {
            generation,
            slots: Header::fingerprint(slots),
        };
        let pt = serde_json::to_vec(&header).map_err(|e| CustodyError::Io(e.to_string()))?;
        Self::seal(dek, &pt, &Self::aad(version, AAD_HEADER))
    }

    /// Decrypt + verify the header against the actual on-disk slots. Fails closed
    /// (`Corrupt`) if the header does not unseal (tamper / rollback of the header
    /// itself) or if the recorded `name -> nonce` fingerprint differs from what
    /// is on disk (a slot was added, removed, swapped, or rolled back in place —
    /// a rolled-back slot carries a prior nonce). Returns the generation.
    fn open_and_verify_header(dek: &[u8; KEY_LEN], env: &Envelope) -> Result<u64> {
        let pt = Self::unseal(dek, &env.header, &Self::aad(env.version, AAD_HEADER))
            .map_err(|_| CustodyError::Corrupt)?;
        let header: Header = serde_json::from_slice(&pt).map_err(|_| CustodyError::Corrupt)?;
        if header.slots != Header::fingerprint(&env.slots) {
            return Err(CustodyError::Corrupt);
        }
        Ok(header.generation)
    }

    // --- lockout block sealed under the keyring master key --------------

    fn seal_lockout(mek: &[u8; KEY_LEN], version: u8, state: &LockoutState) -> Result<SealedSlot> {
        let pt = serde_json::to_vec(state).map_err(|e| CustodyError::Io(e.to_string()))?;
        Self::seal(mek, &pt, &Self::aad(version, AAD_LOCKOUT))
    }

    fn open_lockout(mek: &[u8; KEY_LEN], env: &Envelope) -> Result<LockoutState> {
        let pt = Self::unseal(mek, &env.lockout, &Self::aad(env.version, AAD_LOCKOUT))
            .map_err(|_| CustodyError::Corrupt)?;
        serde_json::from_slice(&pt).map_err(|_| CustodyError::Corrupt)
    }

    /// Current wall-clock time in unix milliseconds (for the persisted lockout
    /// deadline). Clamps a pre-epoch clock to 0.
    fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Combine the keyring master key and the passphrase-derived key into the
    /// data-key wrapping key. XOR of two independent, uniformly-random 32-byte
    /// secrets (a minted key + an Argon2id output) — both are required to
    /// reconstruct it, giving keyring-AND-passphrase semantics.
    fn combine_keys(mek: &[u8; KEY_LEN], kek: &[u8; KEY_LEN]) -> Zeroizing<[u8; KEY_LEN]> {
        let mut out = Zeroizing::new([0u8; KEY_LEN]);
        for i in 0..KEY_LEN {
            out[i] = mek[i] ^ kek[i];
        }
        out
    }

    /// Unlock the session: trial-decrypt the check-slot with the passphrase's
    /// derived key. Wrong passphrase / no vault → `Denied` (no oracle). Enforces
    /// the N-attempt lockout with a cooloff.
    pub fn unlock(&self, passphrase: &mut [u8]) -> Result<()> {
        let result = self.unlock_inner(passphrase);
        passphrase.zeroize(); // scrub the caller's inbound passphrase (spec)
        result
    }

    /// Fixed dummy salt for the absent-vault KDF (CRY-2). Not secret; its only
    /// job is to make the no-vault path spend the same Argon2id work as a
    /// wrong-passphrase path, so vault existence is not a timing oracle.
    const DUMMY_SALT: [u8; SALT_LEN] = [0x5A; SALT_LEN];

    fn unlock_inner(&self, passphrase: &[u8]) -> Result<()> {
        // BND-1: hold the mutex across the ENTIRE check→derive→verify→record
        // sequence. Unlock attempts are serialized, so at most one Argon2id
        // trial is ever in flight and the persisted lockout is read+written
        // atomically. This intentionally serializes unlocks (a human types one
        // passphrase at a time; a concurrent storm is the attack).
        let mut inner = self.inner.lock().unwrap();

        // CRY-2: if there is no vault, still spend one Argon2id derivation over
        // the supplied passphrase (fixed dummy salt) before denying, so
        // "no vault" and "wrong passphrase" cost the same KDF work. Routed
        // through the accounted call site so the timing-parity test can assert
        // structurally. There is no persisted lockout without an envelope.
        if !self.is_initialized() {
            let _ = self.derive_key_counted(passphrase, &Self::DUMMY_SALT);
            return Err(CustodyError::Denied);
        }

        // The keyring master is required to read/verify the persisted lockout
        // block AND to unwrap the DEK. Absent/unreachable keyring is an
        // environment fault (fail closed, do NOT count toward lockout).
        let mek = self.master_key()?;
        let env = self.load_envelope()?;
        let mut lockout = Self::open_lockout(&mek, &env)?;

        // CRY-3/BND-2: enforce the PERSISTED lockout. The deadline is an
        // absolute unix-ms time, so it survives a process restart. Clock-rollback
        // caveat: an attacker who can wind the wall clock back past the deadline
        // ends the cooloff early — an accepted limitation of persisting an
        // absolute deadline (a monotonic Instant cannot survive a restart, which
        // is the stronger attacker this defends against). Argon2id cost remains
        // the always-on per-guess brake.
        if let Some(until_ms) = lockout.locked_until_ms {
            if Self::now_unix_ms() < until_ms {
                return Err(CustodyError::LockedOut);
            }
            // Cooloff elapsed: clear it and let this attempt proceed fresh.
            lockout.failures = 0;
            lockout.locked_until_ms = None;
        }

        let outcome = self.try_derive_session(passphrase, &mek, &env);
        match outcome {
            Ok(session) => {
                // Success resets the persisted lockout.
                if lockout != LockoutState::default() {
                    let cleared = Self::seal_lockout(&mek, env.version, &LockoutState::default())?;
                    let mut env2 = env.clone();
                    env2.lockout = cleared;
                    self.save_envelope(&env2)?;
                }
                inner.session = Some(session);
                Ok(())
            }
            Err(e) => {
                // Only a genuine wrong-passphrase / corrupt-vault counts toward
                // lockout; a missing keyring is an environment fault, not an
                // attack, and must not brick the user. We count attempts
                // *started* under the held lock, so a concurrent storm cannot
                // bypass the ceiling (BND-1).
                if matches!(e, CustodyError::Denied | CustodyError::Corrupt) {
                    lockout.failures += 1;
                    if lockout.failures >= MAX_ATTEMPTS {
                        lockout.locked_until_ms =
                            Some(Self::now_unix_ms() + LOCKOUT_COOLOFF.as_millis() as u64);
                    }
                    let sealed = Self::seal_lockout(&mek, env.version, &lockout)?;
                    let mut env2 = env.clone();
                    env2.lockout = sealed;
                    // Best-effort persist; a save failure must not mask the
                    // original denial.
                    let _ = self.save_envelope(&env2);
                }
                Err(e)
            }
        }
    }

    /// Recover the data key (keyring master key + passphrase both required),
    /// verify the authenticated header (generation + slot set — CRY-1), then
    /// trial-decrypt the check-slot. Returns a live [`Session`] on success. The
    /// caller supplies the already-loaded `mek` + `env` (read once under the
    /// unlock lock).
    fn try_derive_session(
        &self,
        passphrase: &[u8],
        mek: &[u8; KEY_LEN],
        env: &Envelope,
    ) -> Result<Session> {
        // Recompose the wrapping key from the keyring master key + the
        // passphrase-derived key, then unwrap the data key. A wrong passphrase
        // OR an absent/rotated master key fails the GCM tag with no oracle. The
        // wrapped-DEK AAD binds the version (CRY-4) + domain tag (CRY-1).
        let kek = self.derive_key_counted(passphrase, &env.salt)?;
        let wrap_key = Self::combine_keys(mek, &kek);
        let dek_bytes = Self::unseal(
            &wrap_key,
            &env.wrapped_dk,
            &Self::aad(env.version, AAD_WRAPPED_DK),
        )?;
        if dek_bytes.len() != KEY_LEN {
            return Err(CustodyError::Corrupt);
        }
        let mut data_key = Zeroizing::new([0u8; KEY_LEN]);
        data_key.copy_from_slice(&dek_bytes);

        // CRY-1: verify the DEK-sealed header BEFORE trusting any slot. This
        // authenticates the whole slot set (a wholesale swap/rollback of the
        // envelope, or an add/remove/swap of any slot, changes the set and fails
        // closed). The generation is bound under the DEK the attacker cannot
        // forge.
        let _generation = Self::open_and_verify_header(&data_key, env)?;

        // Trial-decrypt the check-slot with its per-slot AAD (`version || name`).
        // A slot whose stored position no longer matches its sealed AAD fails
        // the tag here (CRY-1 slot-swap).
        let check = env.slots.get(CHECK_SLOT).ok_or(CustodyError::Corrupt)?;
        let pt = Self::unseal(
            &data_key,
            check,
            &Self::aad(env.version, CHECK_SLOT.as_bytes()),
        )?;
        if pt.as_slice() != CHECK_PLAINTEXT {
            return Err(CustodyError::Denied);
        }
        Ok(Session {
            data_key,
            unlocked_at: Instant::now(),
        })
    }

    /// Whether the session is currently unlocked (respecting auto-lock expiry).
    pub fn is_unlocked(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        self.expire_if_stale(&mut inner);
        inner.session.is_some()
    }

    /// Drop + zeroize the session immediately.
    pub fn lock(&self) {
        let mut inner = self.inner.lock().unwrap();
        // Zeroizing<[u8;N]> wipes on drop; take() drops the session here.
        inner.session = None;
    }

    /// Auto-lock: if the session is older than the configured window, drop it.
    /// Uses a monotonic `Instant`, so wall-clock rollback cannot extend it
    /// (ADV-10).
    fn expire_if_stale(&self, inner: &mut VaultInner) {
        let window = *self.autolock_secs.lock().unwrap();
        if window == 0 {
            return; // 0 disables auto-lock
        }
        let stale = inner
            .session
            .as_ref()
            .map(|s| s.unlocked_at.elapsed() >= Duration::from_secs(window))
            .unwrap_or(false);
        if stale {
            inner.session = None;
        }
    }

    /// Put a secret into a named slot. Requires an unlocked session. The inbound
    /// `bytes` are zeroized after sealing.
    pub fn put(&self, slot: &str, bytes: &mut [u8]) -> Result<()> {
        let result = self.put_inner(slot, bytes);
        bytes.zeroize();
        result
    }

    fn put_inner(&self, slot: &str, bytes: &[u8]) -> Result<()> {
        if slot.starts_with('\0') {
            return Err(CustodyError::Io("reserved slot name".into()));
        }
        // BND-3: hold the mutex across load→insert→reseal-header→save so
        // concurrent puts serialize (no lost-write TOCTOU) and the header
        // stays consistent with the slot set. `save_envelope` is atomic
        // (temp+fsync+rename).
        let mut inner = self.inner.lock().unwrap();
        self.expire_if_stale(&mut inner);
        let data_key = match inner.session.as_ref() {
            Some(s) => Zeroizing::new(*s.data_key),
            None => return Err(CustodyError::Denied),
        };
        // Per-slot AAD binds `version || slot_name` (CRY-1).
        let mut env = self.load_envelope()?;
        let sealed = Self::seal(&data_key, bytes, &Self::aad(env.version, slot.as_bytes()))?;
        // Confirm the current envelope integrity before mutating it (the header
        // + check-slot must verify under the session DEK); this prevents writing
        // a fresh slot on top of an envelope that was swapped/rolled back
        // out-of-band since unlock.
        let generation = Self::open_and_verify_header(&data_key, &env)?;
        env.slots.insert(slot.to_string(), sealed);
        // Re-seal the header with the new slot set + an incremented generation
        // so the mutation is authenticated and cannot be rolled back to the
        // pre-put state (CRY-1 rollback).
        env.header = Self::seal_header(&data_key, env.version, generation + 1, &env.slots)?;
        self.save_envelope(&env)
    }

    /// **In-process only** secret read for A3/B1 consumers. This is deliberately
    /// a plain `pub fn` and is NEVER registered as a `#[tauri::command]`
    /// (ADV-8: no invoke command returns secret bytes). Returns a zeroizing
    /// buffer. Requires an unlocked session. Its only current callers are A3/B1
    /// (not yet landed) and the custody tests; allow dead_code so the non-test
    /// lib build does not flag this deliberately-in-process API as unused.
    #[allow(dead_code)]
    pub fn custody_get(&self, slot: &str) -> Result<Zeroizing<Vec<u8>>> {
        if slot.starts_with('\0') {
            return Err(CustodyError::Denied);
        }
        let data_key = self.with_session_key()?;
        let env = self.load_envelope()?;
        // CRY-1: re-verify the authenticated header before serving a slot, so a
        // swap/rollback of the envelope on disk AFTER unlock is caught at read
        // time too (not only at unlock). A slot read then authenticates its own
        // per-slot AAD (`version || slot_name`), so a swapped slot fails closed
        // rather than returning another slot's plaintext.
        Self::open_and_verify_header(&data_key, &env)?;
        let sealed = env.slots.get(slot).ok_or(CustodyError::Denied)?;
        Self::unseal(&data_key, sealed, &Self::aad(env.version, slot.as_bytes()))
    }

    /// Slot metadata only (never bytes). The reserved check-slot is hidden.
    pub fn list(&self) -> Result<Vec<SlotInfo>> {
        // Listing metadata does not require an unlock — but an uninitialized
        // vault has nothing to list.
        if !self.is_initialized() {
            return Ok(vec![]);
        }
        let env = self.load_envelope()?;
        Ok(env
            .slots
            .iter()
            .filter(|(name, _)| !name.starts_with('\0'))
            .map(|(name, s)| SlotInfo {
                name: name.clone(),
                bytes: s.ct.len(),
            })
            .collect())
    }

    /// Fetch a copy of the current session data key, enforcing auto-lock. Errors
    /// `Denied` if locked. The returned key is zeroized on drop.
    fn with_session_key(&self) -> Result<Zeroizing<[u8; KEY_LEN]>> {
        let mut inner = self.inner.lock().unwrap();
        self.expire_if_stale(&mut inner);
        match inner.session.as_ref() {
            Some(s) => Ok(Zeroizing::new(*s.data_key)),
            None => Err(CustodyError::Denied),
        }
    }
}

// Explicit zeroize-on-drop for the session data key (defense in depth beyond
// the Zeroizing wrapper — ADV-9).
//
// BND-4 (honest scope): this wiring establishes the Zeroizing type contract +
// an explicit drop. It does NOT by itself prove the compiler emits the wipe
// (dead-store elimination could drop it). A physical memory-residue proof is
// DEFERRED to a zeroize-audit MIR/LLVM pass before B1 stores real wallet keys;
// the ADV-9 test is scoped to teardown + the type contract, not a byte wipe.
impl Drop for Session {
    fn drop(&mut self) {
        self.data_key.zeroize();
    }
}

// ---------------------------------------------------------------------------
// Tauri command surface. NOTE: `custody_get` is NOT here (ADV-8 boundary).
// Commands return status / metadata only, never secret bytes.
// ---------------------------------------------------------------------------

/// Status shape returned across the bridge (metadata only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustodyStatus {
    pub initialized: bool,
    pub unlocked: bool,
    #[serde(rename = "autolockMins")]
    pub autolock_mins: u32,
    #[serde(rename = "keyringStatus")]
    pub keyring_status: String,
}

/// Managed Tauri state: the process-wide custody vault.
pub struct CustodyState(pub CustodyVault);

/// Resolve the on-disk envelope path from the app data dir.
fn envelope_path<R: Runtime>(app: &AppHandle<R>) -> std::result::Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join(ENVELOPE_FILE))
}

fn err_str(e: CustodyError) -> String {
    e.to_string()
}

#[tauri::command]
pub fn custody_status(
    state: State<'_, CustodyState>,
) -> std::result::Result<CustodyStatus, String> {
    let v = &state.0;
    Ok(CustodyStatus {
        initialized: v.is_initialized(),
        unlocked: v.is_unlocked(),
        autolock_mins: (*v.autolock_secs.lock().unwrap() / 60) as u32,
        keyring_status: keyring_probe(),
    })
}

#[tauri::command]
pub fn custody_init(
    state: State<'_, CustodyState>,
    mut passphrase: String,
) -> std::result::Result<(), String> {
    // Operate on the owned String's bytes so we can zeroize the inbound copy.
    let mut bytes = std::mem::take(&mut passphrase).into_bytes();
    let r = state.0.init(&mut bytes).map_err(err_str);
    bytes.zeroize();
    r
}

#[tauri::command]
pub fn custody_unlock(
    state: State<'_, CustodyState>,
    mut passphrase: String,
) -> std::result::Result<(), String> {
    let mut bytes = std::mem::take(&mut passphrase).into_bytes();
    let r = state.0.unlock(&mut bytes).map_err(err_str);
    bytes.zeroize();
    r
}

#[tauri::command]
pub fn custody_lock(state: State<'_, CustodyState>) -> std::result::Result<(), String> {
    state.0.lock();
    Ok(())
}

#[tauri::command]
pub fn custody_put(
    state: State<'_, CustodyState>,
    slot: String,
    mut bytes: Vec<u8>,
) -> std::result::Result<(), String> {
    let r = state.0.put(&slot, &mut bytes).map_err(err_str);
    bytes.zeroize();
    r
}

#[tauri::command]
pub fn custody_list(state: State<'_, CustodyState>) -> std::result::Result<Vec<SlotInfo>, String> {
    state.0.list().map_err(err_str)
}

#[tauri::command]
pub fn custody_keyring_status() -> String {
    keyring_probe()
}

/// Probe the real platform keyring (same honest semantics as A1's config probe).
fn keyring_probe() -> String {
    match keyring::Entry::new(KEYRING_SERVICE, "keyring-probe") {
        Ok(entry) => match entry.get_password() {
            Ok(_) => "available".into(),
            Err(keyring::Error::NoEntry) => "available".into(),
            Err(_) => "unavailable".into(),
        },
        Err(_) => "unavailable".into(),
    }
}

/// Build the managed custody state from a live app handle: the real OS keyring +
/// the app-data envelope path + the persisted `config.autolock`.
pub fn build_custody_state<R: Runtime>(
    app: &AppHandle<R>,
    autolock_mins: u32,
) -> std::result::Result<CustodyState, String> {
    let path = envelope_path(app)?;
    Ok(CustodyState(CustodyVault::new(
        Box::new(OsKeyring),
        path,
        autolock_mins,
    )))
}

#[cfg(test)]
mod tests {
    include!("custody_tests.rs");
}
