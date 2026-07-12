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
use std::time::{Duration, Instant};

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
/// only ciphertext. Format-versioned so B1/A3 can migrate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Envelope {
    version: u8,
    /// Argon2id salt (public; a salt is not secret).
    salt: Vec<u8>,
    /// The data key sealed under the keyring master key: nonce + ciphertext.
    wrapped_dk: SealedSlot,
    /// Sealed slots keyed by name (includes the reserved check-slot).
    slots: BTreeMap<String, SealedSlot>,
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

/// Failed-unlock tracking for the lockout guard.
#[derive(Default)]
struct AttemptState {
    failures: u32,
    /// When the current cooloff (if any) ends.
    locked_until: Option<Instant>,
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
}

#[derive(Default)]
struct VaultInner {
    session: Option<Session>,
    attempts: AttemptState,
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
        }
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

    // --- AES-256-GCM seal / unseal --------------------------------------

    /// Seal `plaintext` under `key`, returning a fresh-nonce sealed slot.
    fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<SealedSlot> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: b"citrate-core-custody-v1",
                },
            )
            .map_err(|_| CustodyError::Corrupt)?;
        Ok(SealedSlot {
            nonce: nonce_bytes.to_vec(),
            ct,
        })
    }

    /// Unseal a slot under `key`. GCM tag failure (wrong key OR tampered
    /// ciphertext) fails closed with no partial plaintext (ADV-2 / ADV-5). The
    /// tag comparison inside `aes-gcm` is constant-time.
    fn unseal(key: &[u8; KEY_LEN], slot: &SealedSlot) -> Result<Zeroizing<Vec<u8>>> {
        if slot.nonce.len() != NONCE_LEN {
            return Err(CustodyError::Corrupt);
        }
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let nonce = Nonce::from_slice(&slot.nonce);
        let pt = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &slot.ct,
                    aad: b"citrate-core-custody-v1",
                },
            )
            .map_err(|_| CustodyError::Denied)?;
        Ok(Zeroizing::new(pt))
    }

    // --- envelope persistence -------------------------------------------

    fn load_envelope(&self) -> Result<Envelope> {
        let bytes = std::fs::read(&self.path).map_err(|_| CustodyError::Denied)?;
        serde_json::from_slice(&bytes).map_err(|_| CustodyError::Corrupt)
    }

    fn save_envelope(&self, env: &Envelope) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CustodyError::Io(e.to_string()))?;
        }
        let bytes = serde_json::to_vec(env).map_err(|e| CustodyError::Io(e.to_string()))?;
        std::fs::write(&self.path, bytes).map_err(|e| CustodyError::Io(e.to_string()))?;
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

    fn mint_master_key(&self) -> Result<Zeroizing<[u8; KEY_LEN]>> {
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
        // is what makes the keyring genuinely load-bearing (ADV-6).
        let mek = self.mint_master_key()?;
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        let kek = Self::derive_key(passphrase, &salt)?;
        let wrap_key = Self::combine_keys(&mek, &kek);

        let mut dek = Zeroizing::new([0u8; KEY_LEN]);
        rand::thread_rng().fill_bytes(dek.as_mut());
        let wrapped_dk = Self::seal(&wrap_key, dek.as_ref())?;

        let mut slots = BTreeMap::new();
        slots.insert(CHECK_SLOT.to_string(), Self::seal(&dek, CHECK_PLAINTEXT)?);

        let env = Envelope {
            version: 1,
            salt: salt.to_vec(),
            wrapped_dk,
            slots,
        };
        self.save_envelope(&env)?;
        Ok(())
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

    fn unlock_inner(&self, passphrase: &[u8]) -> Result<()> {
        {
            let inner = self.inner.lock().unwrap();
            if let Some(until) = inner.attempts.locked_until {
                if Instant::now() < until {
                    return Err(CustodyError::LockedOut);
                }
            }
        }

        // Do the crypto WITHOUT holding the lock; then record the outcome.
        let outcome = self.try_derive_session(passphrase);

        let mut inner = self.inner.lock().unwrap();
        match outcome {
            Ok(session) => {
                inner.attempts = AttemptState::default();
                inner.session = Some(session);
                Ok(())
            }
            Err(e) => {
                // Only a genuine wrong-passphrase / corrupt-vault counts toward
                // lockout; a missing keyring is an environment fault, not an
                // attack, and must not brick the user.
                if matches!(e, CustodyError::Denied | CustodyError::Corrupt) {
                    inner.attempts.failures += 1;
                    if inner.attempts.failures >= MAX_ATTEMPTS {
                        inner.attempts.locked_until = Some(Instant::now() + LOCKOUT_COOLOFF);
                    }
                }
                Err(e)
            }
        }
    }

    /// Recover the data key (keyring master key + passphrase both required) and
    /// verify it against the check-slot. Returns a live [`Session`] on success.
    fn try_derive_session(&self, passphrase: &[u8]) -> Result<Session> {
        let env = self.load_envelope()?;
        // Recompose the wrapping key from the keyring master key + the
        // passphrase-derived key, then unwrap the data key. A wrong passphrase
        // OR an absent/rotated master key fails the GCM tag with no oracle.
        let mek = self.master_key()?;
        let kek = Self::derive_key(passphrase, &env.salt)?;
        let wrap_key = Self::combine_keys(&mek, &kek);
        let dek_bytes = Self::unseal(&wrap_key, &env.wrapped_dk)?;
        if dek_bytes.len() != KEY_LEN {
            return Err(CustodyError::Corrupt);
        }
        let mut data_key = Zeroizing::new([0u8; KEY_LEN]);
        data_key.copy_from_slice(&dek_bytes);

        // Trial-decrypt the check-slot: defense-in-depth confirmation the
        // recovered data key is the right one.
        let check = env.slots.get(CHECK_SLOT).ok_or(CustodyError::Corrupt)?;
        let pt = Self::unseal(&data_key, check)?;
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
        let data_key = self.with_session_key()?;
        let sealed = Self::seal(&data_key, bytes)?;
        let mut env = self.load_envelope()?;
        env.slots.insert(slot.to_string(), sealed);
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
        let sealed = env.slots.get(slot).ok_or(CustodyError::Denied)?;
        Self::unseal(&data_key, sealed)
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
