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

/// DR-3: a caller slot name is forbidden if it aliases a domain-tag AAD, so the
/// per-slot AAD (`version || name`) namespace stays disjoint from the domain
/// fields (`version || "wrapped_dk" | "header" | "lockout"`). Without this a
/// slot named e.g. `header` seals under an AAD byte-identical to the header
/// field's, making ciphertext potentially interchangeable across contexts.
fn is_reserved_domain_tag(name: &str) -> bool {
    let n = name.as_bytes();
    n == AAD_WRAPPED_DK || n == AAD_HEADER || n == AAD_LOCKOUT
}

/// OS keyring coordinates for the wrapping (master) key.
const KEYRING_SERVICE: &str = "ai.citrate.core";
const KEYRING_MASTER_ACCOUNT: &str = "custody-master-key";
/// OS keyring account for the monotonic high-water generation anchor (DR-1).
/// Lives OUTSIDE the attacker-writable `custody.enc`, so a **filesystem-only**
/// whole-envelope rollback (an older, internally-consistent `custody.enc`
/// restored while the keyring is untouched) is rejected: any envelope whose
/// header generation is BELOW this high-water fails closed. Stored as 8-byte
/// big-endian `u64`.
///
/// SCOPE (F-1 — honest limit): this anchor defends FS-only rollback, NOT an
/// attacker who can WRITE OR DELETE keyring entries. An attacker with a
/// keyring-write/delete capability who DELETES this entry (leaving
/// `custody-master-key`) downgrades the vault to NO rollback protection,
/// because a deleted/absent anchor is intentionally treated as first-init (see
/// `read_anchor`) so a legitimately-lost anchor — keychain reset, machine
/// migration, backup-restore — still opens the vault. From inside the vault a
/// maliciously-deleted anchor is indistinguishable from a legitimately-lost
/// one. Closing that gap needs a second, master-sealed "anchor-initialized"
/// bit (owner-decision follow-up; it trades keychain-reset/migration
/// recoverability — see the sprint's Day-4 note), not shipped here. Stored as
/// 8-byte big-endian `u64`.
const KEYRING_GENERATION_ACCOUNT: &str = "custody-generation";
/// OS keyring account for the lockout block's OWN monotonic high-water (DR-2).
/// Separate from the envelope generation anchor because failed-unlock lockout
/// writes advance on attempts that do NOT mutate the envelope (the header
/// generation stays put), so the two counters must not share a lane — otherwise
/// a burst of failed unlocks would push the envelope anchor past the header
/// generation and fail closed on the next legitimate unlock. A **filesystem-only**
/// rollback of the lockout block to an older generation is below this anchor and
/// is caught. Same F-1 delete-downgrade limit as `custody-generation`: an
/// attacker who can DELETE this entry reverts the throttle to no-anchor
/// (first-init), resetting the lockout — deletion is treated as first-init for
/// availability. Additionally (already-known limit), a DUMPED keyring master
/// lets the throttle be forged at the current generation, leaving only Argon2id
/// as the per-guess brake. Stored as 8-byte big-endian `u64`.
const KEYRING_LOCKOUT_GEN_ACCOUNT: &str = "custody-lockout-generation";

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
/// ## Integrity model (v2 — CRY-1, DR-1/DR-2)
/// GCM authenticates each slot's *bytes*, but nothing in v1 authenticated *where*
/// a slot sat or the slot set as a whole, so a disk-write attacker could swap or
/// roll back slots. v2 binds:
/// - **per-slot AAD** = `version || slot_name` (and `version || "wrapped_dk"`),
///   so a slot sealed under one name/version cannot be replayed under another;
/// - a **DEK-sealed `header`** carrying a **generation counter** and the exact
///   **slot-name→nonce fingerprint**, verified on unlock before any slot is
///   trusted — this catches an IN-PLACE swap, add/remove, or rollback of an
///   INDIVIDUAL slot (the on-disk fingerprint diverges from the sealed header).
///   It does NOT by itself catch a WHOLE-envelope rollback: an older, internally
///   consistent `custody.enc` (old header + old slots together) still verifies,
///   because the header only proves the envelope is self-consistent, not that it
///   is the newest one. Whole-envelope rollback is caught by the EXTERNAL anchor
///   below (DR-1), not by this header;
/// - a **keyring high-water generation anchor** (`custody-generation`, DR-1)
///   OUTSIDE the attacker-writable envelope: on unlock and on every `custody_get`,
///   any envelope whose header generation is BELOW the high-water is rejected
///   (`Corrupt`). This defeats a **filesystem-only** whole-envelope rollback (the
///   envelope restored while the keyring anchor is untouched). SCOPE (F-1): it
///   does NOT defend an attacker who can DELETE the keyring anchor — a
///   deleted/absent anchor is intentionally treated as first-init for
///   availability (a legitimately-lost anchor must still open the vault), so a
///   delete-then-rollback downgrades to no rollback protection. See
///   `custody-generation` / `read_anchor` for the full limit;
/// - a **master-key-sealed `lockout`** block (`failures` + absolute deadline +
///   its own `generation`), so the lockout survives a process restart
///   (CRY-3/BND-2) AND is bound to a SEPARATE keyring high-water
///   (`custody-lockout-generation`, DR-2) so a **filesystem-only** rollback of the
///   lockout block (resetting the throttle) is detected. HONEST LIMITATIONS: an
///   attacker who can DELETE the lockout anchor resets it to first-init (same F-1
///   delete-downgrade as DR-1); and an attacker who has DUMPED the keyring master
///   can forge a fresh lockout block at the current generation and remove the
///   throttle — in either case only Argon2id remains the per-guess brake (see the
///   sprint's DR-2 / Day-4 notes).
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
/// generation counter, so any INDIVIDUAL-slot swap, rollback-in-place, or
/// add/remove is caught on unlock. Sealed under the DEK, which a disk-write
/// attacker does not have, so it cannot be forged to match tampered slots.
/// It does NOT catch a whole-envelope rollback (an older, self-consistent
/// header+slots pair) on its own — that is the job of the keyring high-water
/// generation anchor (DR-1), which compares this `generation` against a value
/// held outside the envelope. That anchor defends only a FILESYSTEM-only
/// rollback; an attacker who can DELETE the keyring anchor downgrades past it
/// (F-1 — see `custody-generation` / `read_anchor`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Header {
    /// Generation counter, incremented on every envelope mutation. Compared
    /// against the keyring high-water anchor (DR-1) on load/read to reject a
    /// whole-envelope rollback.
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
    /// DR-2: the envelope generation this lockout block belongs to. Bound so a
    /// rollback of the lockout block alone (a plain file write copying a pristine
    /// block over a high-failure one) diverges from the header generation and
    /// the keyring high-water, and is caught. `#[serde(default)]` so a v2
    /// envelope written before this field is read as generation 0.
    #[serde(default)]
    generation: u64,
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

    // --- DR-5: mutex poison policy — recover, do not brick --------------
    //
    // Policy (DR-5): a panic somewhere in the (now long) critical section
    // poisons the mutex. We RECOVER the inner guard (`into_inner`) and continue
    // rather than let every subsequent custody call panic and brick the vault
    // until an app restart. This is safe because the vault holds no invariant
    // that a mid-op panic can leave half-updated in memory in a way a later op
    // trusts: every mutating op re-reads the envelope from disk (atomic
    // temp+fsync+rename) and re-verifies the header + anchors before acting, and
    // fails CLOSED on the operation if anything is inconsistent. Recovering the
    // in-memory session lock therefore fails the *operation* closed at worst,
    // never the process. The only in-memory state under the lock is the
    // `Option<Session>`; a poisoned guard yields it intact (or `None`), and a
    // corrupt/absent session simply denies.

    /// Lock `inner`, recovering from poison (DR-5) instead of panicking.
    fn lock_inner(&self) -> std::sync::MutexGuard<'_, VaultInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Read the auto-lock window (seconds), recovering from poison (DR-5).
    fn autolock_secs(&self) -> u64 {
        *self.autolock_secs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Update the auto-lock window (config.autolock changed). Minutes → seconds.
    /// Consumed by the config-change wire (A3/B1) and the auto-lock tests; allow
    /// dead_code so the non-test lib build does not flag this seam as unused.
    #[allow(dead_code)]
    pub fn set_autolock_mins(&self, mins: u32) {
        *self.autolock_secs.lock().unwrap_or_else(|e| e.into_inner()) = mins as u64 * 60;
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

    // --- DR-1/DR-2: keyring high-water generation anchor ----------------

    /// Read a monotonic high-water counter (`account`) from the OS keyring, or
    /// `None` if it has never been set (first init). An unreachable keyring is a
    /// hard fault (`KeyringUnavailable`) — fail closed, never treat "can't read
    /// the anchor" as "no anchor". A malformed entry (wrong length) is treated as
    /// tamper → `Corrupt`.
    ///
    /// F-1 (honest limit): a DELETED/absent entry maps to `Ok(None)`, which both
    /// guards (`enforce_high_water`, the lockout-gen check) treat as benign
    /// first-init. This is a deliberate AVAILABILITY choice — a legitimately-lost
    /// anchor (keychain reset, machine migration, backup-restore) must still open
    /// the vault — but it means the rollback defense is FS-only: an attacker who
    /// can DELETE this keyring entry (a keyring-write/delete capability), while
    /// leaving `custody-master-key`, downgrades a whole-envelope rollback back
    /// into a re-seed with NO rollback protection. From inside the vault we cannot
    /// distinguish maliciously-deleted from legitimately-lost without a second,
    /// master-sealed "anchor-initialized" bit (owner-decision follow-up, not
    /// shipped here — see the sprint Day-4 note).
    fn read_anchor(&self, account: &str) -> Result<Option<u64>> {
        match self.keyring.get(account)? {
            Some(bytes) => {
                if bytes.len() != 8 {
                    return Err(CustodyError::Corrupt);
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&bytes);
                Ok(Some(u64::from_be_bytes(b)))
            }
            None => Ok(None),
        }
    }

    /// Advance the keyring counter `account` to `gen` if it is higher than the
    /// current value (monotone; never lowers it). A keyring write failure is a
    /// hard fault (fail closed) — a mutation whose anchor cannot be persisted
    /// must not be treated as committed.
    fn bump_anchor(&self, account: &str, gen: u64) -> Result<()> {
        let cur = self.read_anchor(account)?.unwrap_or(0);
        if gen > cur {
            self.keyring.set(account, &gen.to_be_bytes())?;
        }
        Ok(())
    }

    /// The envelope-generation high-water (DR-1).
    fn read_high_water(&self) -> Result<Option<u64>> {
        self.read_anchor(KEYRING_GENERATION_ACCOUNT)
    }

    fn bump_high_water(&self, gen: u64) -> Result<()> {
        self.bump_anchor(KEYRING_GENERATION_ACCOUNT, gen)
    }

    /// The lockout high-water (DR-2), a SEPARATE keyring counter from the
    /// envelope generation so failed-unlock lockout writes (which do not mutate
    /// the envelope) cannot push the envelope anchor past the header generation.
    fn read_lockout_hw(&self) -> Result<Option<u64>> {
        self.read_anchor(KEYRING_LOCKOUT_GEN_ACCOUNT)
    }

    fn bump_lockout_hw(&self, gen: u64) -> Result<()> {
        self.bump_anchor(KEYRING_LOCKOUT_GEN_ACCOUNT, gen)
    }

    /// DR-1: reject any envelope whose generation is BELOW the keyring high-water
    /// (a **filesystem-only** whole-envelope rollback to an older,
    /// internally-consistent state). If the envelope generation is at or above the
    /// anchor it is fresh; adopt the (possibly higher) value into the high-water so
    /// the anchor tracks forward. First-init OR a deleted/absent anchor (`None`)
    /// seeds the anchor from the envelope generation.
    ///
    /// F-1 (honest limit): because `None` is treated as first-init (an availability
    /// choice — see `read_anchor`), this guard does NOT defend against an attacker
    /// who can DELETE the `custody-generation` keyring entry: deleting the anchor
    /// then restoring an older envelope re-seeds instead of rejecting, downgrading
    /// to no rollback protection. The defense holds only against an attacker
    /// limited to FILESYSTEM writes (anchor intact). Fails closed (`Corrupt`) on
    /// an in-place rollback, or an unreachable/tampered anchor.
    fn enforce_high_water(&self, generation: u64) -> Result<()> {
        match self.read_high_water()? {
            Some(hw) if generation < hw => Err(CustodyError::Corrupt),
            _ => self.bump_high_water(generation),
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
        let _guard = self.lock_inner();
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
        // DR-1: seed the keyring high-water at the generation-0 anchor. A fresh
        // vault's newest (and only) validly-sealed generation is 0. Written
        // AFTER the envelope so a mid-init crash cannot leave an anchor ahead of
        // a nonexistent envelope. mint_master_key already guaranteed the keyring
        // is clean of a stale master; if a stale high-water lingers from a prior
        // wiped vault, `bump_high_water` only raises it (monotone), so the new
        // gen-0 vault will still be rejected on unlock until it advances past
        // that stale mark — an acceptable fail-closed (a leftover anchor from a
        // deleted vault, not an availability path we optimize for).
        self.bump_high_water(0)?;
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
        let mut inner = self.lock_inner();

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

        // DR-2: the master-sealed lockout block is otherwise unanchored — a plain
        // file write can copy a pristine (failures=0) block over a high-failure
        // one and reset the throttle undetectably. It carries the lockout
        // generation it was written at; reject any lockout block whose generation
        // is BELOW the lockout high-water (a rolled-back block), before it can
        // clear the throttle. This is pre-DEK (the block is master-sealed), so it
        // gates a wrong-passphrase attempt too. Honest limitation (see sprint): an
        // attacker who has DUMPED the keyring master can forge a fresh block at
        // the current generation, removing the throttle — only Argon2id remains
        // the brake in that case. A blind FS-only rollback is caught here.
        if let Some(hw) = self.read_lockout_hw()? {
            if lockout.generation < hw {
                return Err(CustodyError::Corrupt);
            }
        }

        // CRY-3/BND-2: enforce the PERSISTED lockout. The deadline is an
        // absolute unix-ms time, so it survives a process restart. Clock-rollback
        // caveat: an attacker who can wind the wall clock back past the deadline
        // ends the cooloff early — an accepted limitation of persisting an
        // absolute deadline (a monotonic Instant cannot survive a restart, which
        // is the stronger attacker this defends against). Argon2id cost remains
        // the always-on per-guess brake.
        // DR-2: EVERY lockout write advances the keyring high-water and stamps
        // that new value into `lockout.generation`. This gives the lockout block
        // its own monotone anchor (piggybacked on the DR-1 high-water) that ticks
        // on failed attempts even when no `put` mutation occurs — so a plain
        // file-write rollback of the lockout block (copying a pristine, older
        // block over a high-failure one) has a generation below the high-water
        // and is caught by the `generation < high-water` guard above. Helper that
        // bumps the anchor, stamps the new generation, seals + persists the block.
        let write_lockout = |state: &LockoutState| -> Result<()> {
            let next = self.read_lockout_hw()?.unwrap_or(0) + 1;
            // Advance the lockout anchor BEFORE persisting the block, so a crash
            // between the two fails closed (anchor ahead of block → next unlock
            // rejects the now-stale block as a rollback) rather than open.
            self.bump_lockout_hw(next)?;
            let mut stamped = state.clone();
            stamped.generation = next;
            let sealed = Self::seal_lockout(&mek, env.version, &stamped)?;
            let mut env2 = env.clone();
            env2.lockout = sealed;
            self.save_envelope(&env2)
        };

        // DR-4: track whether the elapsed-cooloff branch fired. If it did, the
        // on-disk block is stale ({failures:MAX, locked_until:past}) and MUST be
        // written cleared unconditionally on success — not skipped just because
        // the in-memory `lockout` now equals a fresh state.
        let mut cooloff_elapsed = false;
        if let Some(until_ms) = lockout.locked_until_ms {
            if Self::now_unix_ms() < until_ms {
                return Err(CustodyError::LockedOut);
            }
            // Cooloff elapsed: clear it and let this attempt proceed fresh.
            lockout.failures = 0;
            lockout.locked_until_ms = None;
            cooloff_elapsed = true;
        }

        let outcome = self.try_derive_session(passphrase, &mek, &env);
        match outcome {
            Ok(session) => {
                // Success resets the persisted lockout. DR-4: also write the
                // cleared block whenever the cooloff-elapsed branch fired, so no
                // stale {failures, past-deadline} block is left on disk. The
                // write advances the high-water (DR-2), so the cleared block is
                // itself anchored and cannot be rolled back to a prior state.
                let had_state = lockout.failures != 0 || lockout.locked_until_ms.is_some();
                if had_state || cooloff_elapsed {
                    write_lockout(&LockoutState::default())?;
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
                    // Best-effort persist; a save failure must not mask the
                    // original denial. DR-2: advances + stamps the high-water so
                    // the block is anchored.
                    let _ = write_lockout(&lockout);
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
        let generation = Self::open_and_verify_header(&data_key, env)?;
        // DR-1: the in-envelope header only proves internal consistency; a
        // whole-envelope rollback to an OLDER, internally-consistent state
        // (old header + old slots together) still verifies. Enforce the keyring
        // high-water anchor OUTSIDE the envelope: reject any envelope whose
        // generation is below the newest one we have ever observed. This is the
        // load-time anti-rollback check. (First-init has no anchor yet →
        // enforce_high_water seeds it; a genuinely-newer envelope adopts forward.)
        self.enforce_high_water(generation)?;

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
        let mut inner = self.lock_inner();
        self.expire_if_stale(&mut inner);
        inner.session.is_some()
    }

    /// Drop + zeroize the session immediately.
    pub fn lock(&self) {
        let mut inner = self.lock_inner();
        // Zeroizing<[u8;N]> wipes on drop; take() drops the session here.
        inner.session = None;
    }

    /// Auto-lock: if the session is older than the configured window, drop it.
    /// Uses a monotonic `Instant`, so wall-clock rollback cannot extend it
    /// (ADV-10).
    fn expire_if_stale(&self, inner: &mut VaultInner) {
        let window = self.autolock_secs();
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
        // DR-3: reject the reserved domain-tag names whose per-slot AAD
        // (`version || name`) would alias the wrapped-DK / header / lockout
        // domain-field AAD. Even though those are separate serde fields (and an
        // injected slot trips the header fingerprint → Corrupt), refusing them
        // keeps every per-slot AAD disjoint from the domain-tag AAD namespace,
        // so no ciphertext is ever interchangeable across contexts.
        if is_reserved_domain_tag(slot) {
            return Err(CustodyError::Io("reserved slot name".into()));
        }
        // BND-3: hold the mutex across load→insert→reseal-header→save so
        // concurrent puts serialize (no lost-write TOCTOU) and the header
        // stays consistent with the slot set. `save_envelope` is atomic
        // (temp+fsync+rename).
        let mut inner = self.lock_inner();
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
        // DR-1: this envelope must not itself be a rollback (checked at unlock,
        // re-checked here so a between-unlock-and-put rollback cannot laundry a
        // mutation onto a stale base). enforce_high_water also adopts a
        // higher-than-anchor generation forward.
        self.enforce_high_water(generation)?;
        env.slots.insert(slot.to_string(), sealed);
        // `checked_add` hygiene: unreachable in practice (~1.8e19 puts to
        // overflow a u64 generation) but fail closed (`Corrupt`) rather than
        // debug-panic / release-wrap if the counter ever saturates.
        let next_gen = generation.checked_add(1).ok_or(CustodyError::Corrupt)?;
        // Re-seal the header with the new slot set + an incremented generation
        // so the mutation is authenticated and cannot be rolled back to the
        // pre-put state (CRY-1 rollback). The lockout block is NOT touched here —
        // it is anchored to its OWN keyring counter (DR-2), a separate lane from
        // the envelope generation, so a `put` leaves it as-is.
        env.header = Self::seal_header(&data_key, env.version, next_gen, &env.slots)?;
        // Advance the keyring high-water to the new generation BEFORE persisting
        // the envelope, so a crash between the anchor bump and the envelope
        // write fails closed (anchor ahead of envelope → next unlock rejects the
        // now-stale envelope) rather than open (envelope ahead of anchor →
        // rollback window). Fail-closed is the correct bias for @rule8 custody.
        self.bump_high_water(next_gen)?;
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
        let generation = Self::open_and_verify_header(&data_key, &env)?;
        // DR-1 (mid-live-session): the in-envelope header is internally
        // consistent even in a rolled-back whole envelope, so re-check the
        // keyring high-water on EVERY get. A whole-envelope rollback under a live
        // session (older file swapped in after unlock) has a generation below the
        // anchor → fail closed, rather than serving the resurrected OLD value.
        self.enforce_high_water(generation)?;
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
        let mut inner = self.lock_inner();
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
