//! citrate-core 0.5.4 — **Hermes chat history, sealed on this device** (ADR-2026-10-26-byok-provider-
//! governance P-11: history stays local and encrypted at rest under a device-sealed key, with
//! retention, export and delete). Owner decision 2026-11-14: the same quantum-resistant standard as
//! the rest of the federation's data at rest, AES-256 and a KEM.
//!
//! **Crypto: REUSED, not invented.** The envelope is the federation's at-rest construction from
//! `citrate-homestead/crates/homestead-crypto` (AES-256-GCM envelope + FIPS 203 ML-KEM-768), with
//! the same crates and versions (`aes-gcm` 0.10, `ml-kem` 0.2, `hkdf` 0.12, `sha2` 0.10):
//!
//! ```text
//! Envelope { kem_ct, wrapped_dek, payload }          (u32-be length-prefixed parts)
//!   payload     = nonce || AES-256-GCM(DEK, AAD, plaintext)
//!   wrapped_dek = nonce || AES-256-GCM(KEK, AAD, DEK)     DEK: 32 fresh random bytes per file
//!   kem_ct      = ML-KEM-768 encapsulation to this device's history key
//!   KEK         = HKDF-SHA256(kem shared secret, info = "citrate.core.hermes-history.kek.v1")
//! ```
//!
//! It is not a dependency on that crate: homestead-crypto lives in another federation repo, and a
//! cross-repo crate needs a drift entry first (Rule 12). The construction is ported here verbatim
//! (only the HKDF info string differs, for domain separation) so the two can be unified later.
//!
//! **Key management: core's device-sealed key pattern** (as `deploy_records`, the node storage key
//! and the memory store key): the history key is minted from the OS RNG and kept in the OS keyring
//! (legacy `ai.citrate.core` service, account [`KEY_ACCOUNT`]). It is the 64-byte FIPS 203 key-
//! generation seed (d || z) of an ML-KEM-768 key pair, with a generation number. It is random, not
//! derived from the wallet, and it is NOT in the recovery kit, so history never follows a recovery
//! phrase to another machine. No key ever reaches the webview.
//!
//! **Files** (owner-only `0600` in a `0700` folder, replaced atomically):
//!
//! ```text
//! <app data>/hermes-history/
//!   t-<thread id>.sealed   one conversation
//!   settings.sealed        the retention setting
//!   rotate.pending         present only while an erase is in progress: the names of the
//!                          files being erased, sealed like a conversation
//! file = MAGIC "CITRHHS\0" | version (1) | key generation (u32-be) | envelope
//! AAD  = MAGIC | version | generation | "|" | file name stem
//! ```
//!
//! The AAD binds a file to its name and generation, so a file copied to another thread's name, or
//! edited, does not open.
//!
//! **Delete really erases (the design of draft #533).** A file removed from an SSD or APFS may
//! survive as old blocks, a snapshot or a backup, so delete is a key erase. The thread's file is
//! removed and the history key is ROTATED: every remaining file is resealed under a new key pair
//! and the old seed is removed from the keyring. Old copies of a deleted thread no longer open with
//! any key on the device. The steps are crash-safe:
//! 1. write `rotate.pending` (atomically, sealed under the current key), naming the files to erase;
//! 2. remove the deleted files;
//! 3. keyring = {new, prev: old};
//! 4. reseal every file under the new key;
//! 5. remove `rotate.pending` (while the key it is sealed under is still in the keyring);
//! 6. keyring = {new}.
//!
//! Any command that finds `rotate.pending`, or a `prev` key, finishes the erase first: it removes
//! every file the marker names BEFORE resealing anything, so a crash between steps 1 and 2 can
//! never bring a deleted conversation back under the new key. A marker that does not open fails
//! closed (nothing is resealed; "Delete all history" is the way out). "Delete everything" removes
//! every file and the key itself.
//!
//! **Scope of an erase: this device only.** The rotation makes every copy ON THIS DEVICE unopenable.
//! An OS backup taken earlier (Time Machine, Windows File History, a cloud or disk image) can hold
//! both an old history file and the old keychain item. On macOS the login keychain is in Time
//! Machine backups, so restoring such a backup can reopen conversations deleted since. The seed
//! cannot be kept out of backups here: the `keyring` crate writes the legacy file-based login
//! keychain, which has no `kSecAttrAccessible…ThisDeviceOnly` class (that exists only in the
//! data-protection keychain, which needs a keychain-access-groups entitlement in the signed app).
//! The delete confirmation says so.
//!
//! **Fail closed.**
//! - A file that does not open (tamper, a wrong name, a key generation this device no longer has)
//!   is never shown or overwritten. It is reported as unreadable.
//! - Files with no key in the keyring are refused outright. A new key is never minted over history
//!   it could not open, and "delete everything" is the way out.
//! - An unreachable keyring refuses every read and write.
//!
//! Rule 3: nothing here signs, holds a wallet key or sends anything. Rule 7: the data source of
//! every `hermes_history_*` command is this folder, opened with the history key in the OS keyring.
//! No network.
use std::path::PathBuf;
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{KemCore, MlKem768, B32};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::custody::Keyring;

/// The keyring account (legacy `ai.citrate.core` service) holding the history key.
pub const KEY_ACCOUNT: &str = "hermes-history-key-v1";
/// The app-data folder holding the sealed files.
pub const HISTORY_DIR: &str = "hermes-history";
/// How many conversations are kept; the least recently updated go first (and are erased).
pub const MAX_THREADS: usize = 50;
/// The largest sealed conversation (plaintext JSON), bytes.
pub const MAX_THREAD_BYTES: usize = 1024 * 1024;
/// The retention setting's bounds, in days.
pub const MIN_RETENTION_DAYS: u32 = 1;
pub const MAX_RETENTION_DAYS: u32 = 3650;

const EXT: &str = "sealed";
const THREAD_PREFIX: &str = "t-";
const SETTINGS_STEM: &str = "settings";
const PENDING_FILE: &str = "rotate.pending";
/// The AAD name the erase marker is sealed under (not a file stem, so it can't pose as one).
const PENDING_STEM: &str = "rotate.pending";
const MAGIC: &[u8; 8] = b"CITRHHS\0";
/// 0.5.4 FL datasets (`fl_datasets.rs`) seal their files with this same envelope, under their own
/// magic, HKDF info and keyring account, through [`SealDomain`].
pub(crate) struct SealDomain {
    pub magic: &'static [u8; 8],
    pub hkdf_info: &'static [u8],
}
const HISTORY: SealDomain = SealDomain {
    magic: MAGIC,
    hkdf_info: HKDF_INFO,
};
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = 8 + 1 + 4;
const NONCE_LEN: usize = 12;
const DEK_LEN: usize = 32;
const SEED_LEN: usize = 64;
const HKDF_INFO: &[u8] = b"citrate.core.hermes-history.kek.v1";
/// Keyring record magic and the record lengths (one key, or one key and the previous one).
const KEY_MAGIC: &[u8; 4] = b"HHK1";
const KEY_ONE_LEN: usize = 4 + 4 + SEED_LEN + 1;
const KEY_TWO_LEN: usize = KEY_ONE_LEN + 4 + SEED_LEN;
const MS_PER_DAY: u64 = 86_400_000;

/// One conversation as the webview keeps it. `msgs` is the thread's message list (JSON array);
/// core stores it sealed and never reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadDoc {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub msgs: serde_json::Value,
    /// FL design board 01: set on a branch (edit, retry, "Branch from here"): the thread it came
    /// from and how many of that thread's messages it kept. Absent on a fresh conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ThreadParent>,
}

/// Where a branch came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadParent {
    pub thread_id: String,
    pub at_index: u32,
}

/// The sealed settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    retention_days: Option<u32>,
}

/// What `hermes_history_list` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryList {
    /// Every conversation that opened, most recently updated first.
    pub threads: Vec<ThreadDoc>,
    /// Ids of files that did not open (tampered, moved, or sealed under a key this device no
    /// longer has). They are never shown and never overwritten.
    pub unreadable: Vec<String>,
    /// Keep conversations this many days after their last update; `None` = until deleted.
    /// Always `None` while `retention_unreadable` (the setting is unknown, not "keep forever").
    pub retention_days: Option<u32>,
    /// The sealed retention setting exists but does not open (tampered or damaged). Nothing is
    /// erased by age while it is unreadable, and it is not overwritten until the member chooses a
    /// setting again (`hermes_history_set_retention` with `replaceUnreadable`).
    pub retention_unreadable: bool,
    /// How many conversations the retention setting erased during this call.
    pub purged: usize,
}

/// What `hermes_history_save` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    /// Ids erased to keep [`MAX_THREADS`] conversations.
    pub evicted: Vec<String>,
}

/// One generation of the history key: the ML-KEM-768 seed.
pub(crate) struct KeyGen {
    pub(crate) gen: u32,
    seed: Zeroizing<[u8; SEED_LEN]>,
}

impl KeyGen {
    pub(crate) fn mint(gen: u32) -> Self {
        let mut seed = Zeroizing::new([0u8; SEED_LEN]);
        OsRng.fill_bytes(&mut seed[..]);
        KeyGen { gen, seed }
    }

    fn pair(
        &self,
    ) -> (
        ml_kem::kem::DecapsulationKey<ml_kem::MlKem768Params>,
        ml_kem::kem::EncapsulationKey<ml_kem::MlKem768Params>,
    ) {
        let d = B32::try_from(&self.seed[..32]).expect("32-byte half");
        let z = B32::try_from(&self.seed[32..]).expect("32-byte half");
        MlKem768::generate_deterministic(&d, &z)
    }
}

/// The keyring record: the current key, and the previous one while an erase is finishing.
pub(crate) struct KeySet {
    pub(crate) cur: KeyGen,
    pub(crate) prev: Option<KeyGen>,
}

impl KeySet {
    pub(crate) fn encode(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(KEY_TWO_LEN));
        out.extend_from_slice(KEY_MAGIC);
        out.extend_from_slice(&self.cur.gen.to_be_bytes());
        out.extend_from_slice(&self.cur.seed[..]);
        match &self.prev {
            None => out.push(0),
            Some(p) => {
                out.push(1);
                out.extend_from_slice(&p.gen.to_be_bytes());
                out.extend_from_slice(&p.seed[..]);
            }
        }
        out
    }

    pub(crate) fn decode(b: &[u8]) -> Result<Self, String> {
        let bad = || "the history key in the keychain is malformed".to_string();
        if (b.len() != KEY_ONE_LEN && b.len() != KEY_TWO_LEN) || &b[..4] != KEY_MAGIC {
            return Err(bad());
        }
        let gen_at = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let seed_at = |i: usize| {
            let mut s = Zeroizing::new([0u8; SEED_LEN]);
            s.copy_from_slice(&b[i..i + SEED_LEN]);
            s
        };
        let cur = KeyGen {
            gen: gen_at(4),
            seed: seed_at(8),
        };
        let flag = b[8 + SEED_LEN];
        let prev = match (flag, b.len()) {
            (0, KEY_ONE_LEN) => None,
            (1, KEY_TWO_LEN) => Some(KeyGen {
                gen: gen_at(KEY_ONE_LEN),
                seed: seed_at(KEY_ONE_LEN + 4),
            }),
            _ => return Err(bad()),
        };
        Ok(KeySet { cur, prev })
    }

    fn by_gen(&self, gen: u32) -> Option<&KeyGen> {
        if self.cur.gen == gen {
            return Some(&self.cur);
        }
        self.prev.as_ref().filter(|p| p.gen == gen)
    }
}

// ----------------------------------------------------------------- the envelope (homestead-crypto)

fn aead_seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| "the history could not be sealed".to_string())?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn aead_open(key: &[u8; 32], aad: &[u8], blob: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if blob.len() < NONCE_LEN {
        return None;
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .ok()
        .map(Zeroizing::new)
}

fn kek_from_shared(info: &[u8], ss: &[u8]) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, ss);
    let mut kek = Zeroizing::new([0u8; 32]);
    hk.expand(info, &mut kek[..])
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    kek
}

fn put_part(out: &mut Vec<u8>, part: &[u8]) {
    out.extend_from_slice(&(part.len() as u32).to_be_bytes());
    out.extend_from_slice(part);
}

fn take_part<'b>(b: &'b [u8], i: &mut usize) -> Option<&'b [u8]> {
    let len = b.get(*i..*i + 4)?;
    let n = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
    *i += 4;
    let part = b.get(*i..i.checked_add(n)?)?;
    *i += n;
    Some(part)
}

fn header(magic: &[u8; 8], gen: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(magic);
    h.push(FORMAT_VERSION);
    h.extend_from_slice(&gen.to_be_bytes());
    h
}

fn aad(head: &[u8], stem: &str) -> Vec<u8> {
    let mut a = head.to_vec();
    a.push(b'|');
    a.extend_from_slice(stem.as_bytes());
    a
}

/// Seal `plaintext` as the file `stem` under `key`.
fn seal_file(key: &KeyGen, stem: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    seal_in(&HISTORY, key, stem, plaintext)
}

/// Seal `plaintext` as the file `stem` under `key`, in `dom`'s format.
pub(crate) fn seal_in(
    dom: &SealDomain,
    key: &KeyGen,
    stem: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, String> {
    let (_, ek) = key.pair();
    let (kem_ct, shared) = ek
        .encapsulate(&mut OsRng)
        .map_err(|_| "the history could not be sealed".to_string())?;
    let kek = kek_from_shared(dom.hkdf_info, shared.as_slice());
    let mut dek = Zeroizing::new([0u8; DEK_LEN]);
    OsRng.fill_bytes(&mut dek[..]);
    let head = header(dom.magic, key.gen);
    let aad = aad(&head, stem);
    let payload = aead_seal(&dek, &aad, plaintext)?;
    let wrapped = aead_seal(&kek, &aad, &dek[..])?;
    let mut out = head;
    put_part(&mut out, kem_ct.as_slice());
    put_part(&mut out, &wrapped);
    put_part(&mut out, &payload);
    Ok(out)
}

/// The key generation a sealed file names, or `None` when it is not a history file.
fn file_gen(bytes: &[u8]) -> Option<u32> {
    file_gen_in(&HISTORY, bytes)
}

/// The key generation a sealed file of `dom` names.
pub(crate) fn file_gen_in(dom: &SealDomain, bytes: &[u8]) -> Option<u32> {
    if bytes.len() < HEADER_LEN || &bytes[..8] != dom.magic || bytes[8] != FORMAT_VERSION {
        return None;
    }
    Some(u32::from_be_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12],
    ]))
}

/// Open the file `stem`. `None` for any failure: wrong key, wrong name, or a changed byte.
fn open_sealed(keys: &KeySet, stem: &str, bytes: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    open_in(&HISTORY, keys, stem, bytes)
}

/// Open the file `stem` of `dom`. `None` for any failure.
pub(crate) fn open_in(
    dom: &SealDomain,
    keys: &KeySet,
    stem: &str,
    bytes: &[u8],
) -> Option<Zeroizing<Vec<u8>>> {
    let key = keys.by_gen(file_gen_in(dom, bytes)?)?;
    let head = &bytes[..HEADER_LEN];
    let mut i = HEADER_LEN;
    let kem_ct = take_part(bytes, &mut i)?;
    let wrapped = take_part(bytes, &mut i)?;
    let payload = take_part(bytes, &mut i)?;
    if i != bytes.len() {
        return None;
    }
    let ct = ml_kem::Ciphertext::<MlKem768>::try_from(kem_ct).ok()?;
    let (dk, _) = key.pair();
    let shared = dk.decapsulate(&ct).ok()?;
    let kek = kek_from_shared(dom.hkdf_info, shared.as_slice());
    let aad = aad(head, stem);
    let mut dek_vec = aead_open(&kek, &aad, wrapped)?;
    if dek_vec.len() != DEK_LEN {
        dek_vec.zeroize();
        return None;
    }
    let mut dek = Zeroizing::new([0u8; DEK_LEN]);
    dek.copy_from_slice(&dek_vec);
    aead_open(&dek, &aad, payload)
}

// ------------------------------------------------------------------------------ the store

#[cfg(test)]
thread_local! {
    /// Crash injection (tests only): stop an erase at the numbered point ([`CrashPoint`]); 0 = off.
    static CRASH_AT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// The points an erase can stop at, in order. Every one must leave a state the next open
/// recovers from (hermes_history_tests: `every_crash_point_in_an_erase_is_recoverable`).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum CrashPoint {
    /// Step 1 done: the marker is written; no file is removed.
    MarkerWritten = 1,
    /// Step 2 done: the erased files are removed; the key is not rotated.
    FilesRemoved = 2,
    /// Step 3 done: the keyring holds {new, prev}; nothing is resealed.
    NewKeyStored = 3,
    /// Part of step 4: one remaining file is resealed under the new key, the rest are not.
    FirstResealed = 4,
    /// Step 4 done: every file is resealed; the marker is still there.
    AllResealed = 5,
    /// Step 5 done: the marker is removed; the keyring still holds the previous key.
    MarkerRemoved = 6,
}

#[cfg(test)]
impl CrashPoint {
    pub(crate) const ALL: [CrashPoint; 6] = [
        CrashPoint::MarkerWritten,
        CrashPoint::FilesRemoved,
        CrashPoint::NewKeyStored,
        CrashPoint::FirstResealed,
        CrashPoint::AllResealed,
        CrashPoint::MarkerRemoved,
    ];
}

/// Tests only: fail here when the injected crash point is `at`.
#[cfg(test)]
fn crash_point(at: CrashPoint) -> Result<(), String> {
    if CRASH_AT.with(|c| c.get()) == at as u8 {
        return Err(format!("injected crash: {at:?}"));
    }
    Ok(())
}

/// A conversation id the webview may use: 1 to 64 of `[A-Za-z0-9_-]`.
pub fn valid_thread_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn thread_stem(id: &str) -> String {
    format!("{THREAD_PREFIX}{id}")
}

/// Serializes every read-modify-write across commands in this process.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// The sealed history under one folder, keyed from `keyring`.
pub struct HistoryStore<'a> {
    dir: PathBuf,
    keyring: &'a dyn Keyring,
}

impl<'a> HistoryStore<'a> {
    pub fn new(dir: impl Into<PathBuf>, keyring: &'a dyn Keyring) -> Self {
        Self {
            dir: dir.into(),
            keyring,
        }
    }

    fn path(&self, stem: &str) -> PathBuf {
        self.dir.join(format!("{stem}.{EXT}"))
    }

    fn unreachable() -> String {
        "the keychain is not reachable; the chat history stays closed".to_string()
    }

    /// Every sealed file's stem.
    fn sealed_stems(&self) -> Result<Vec<String>, String> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(format!(
                    "the chat history folder cannot be read: {}",
                    e.kind()
                ))
            }
        };
        let mut out = Vec::new();
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.extension() != Some(std::ffi::OsStr::new(EXT)) {
                continue;
            }
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                out.push(stem.to_string());
            }
        }
        out.sort();
        Ok(out)
    }

    fn read_stem(&self, stem: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.path(stem);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("a chat history file cannot be read: {}", e.kind())),
            // A link or a folder in its place is not ours: treated as unreadable, never followed.
            Ok(m) if !m.is_file() => Ok(Some(Vec::new())),
            Ok(m) if m.len() as usize > MAX_THREAD_BYTES * 2 => Ok(Some(Vec::new())),
            Ok(_) => std::fs::read(&path)
                .map(Some)
                .map_err(|e| format!("a chat history file cannot be read: {}", e.kind())),
        }
    }

    fn write_stem(&self, stem: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.path(stem);
        let staged = crate::atomic_file::stage_secret(&path, bytes)
            .map_err(|e| format!("the chat history could not be written: {}", e.kind()))?;
        crate::atomic_file::commit_staged(&staged, &path)
            .map_err(|e| format!("the chat history could not be written: {}", e.kind()))
    }

    fn unlink_stem(&self, stem: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(stem)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!(
                "a chat history file could not be removed: {}",
                e.kind()
            )),
        }
    }

    fn pending_path(&self) -> PathBuf {
        self.dir.join(PENDING_FILE)
    }

    fn store_history_keys(&self, keys: &KeySet) -> Result<(), String> {
        self.keyring
            .set(KEY_ACCOUNT, &keys.encode())
            .map_err(|_| Self::unreachable())
    }

    /// The history key. Minted only when `mint` and no sealed file exists. An erase left half done
    /// (`rotate.pending`, or a `prev` key) is finished first.
    fn history_keys(&self, mint: bool) -> Result<Option<KeySet>, String> {
        let stored = self
            .keyring
            .get(KEY_ACCOUNT)
            .map_err(|_| Self::unreachable())?;
        let Some(raw) = stored.map(Zeroizing::new) else {
            if !self.sealed_stems()?.is_empty() {
                return Err(
                    "the chat history exists but its key is not in the keychain; \
                     it cannot be opened (Delete all history starts over)"
                        .to_string(),
                );
            }
            if !mint {
                return Ok(None);
            }
            let keys = KeySet {
                cur: KeyGen::mint(1),
                prev: None,
            };
            self.store_history_keys(&keys)?;
            return Ok(Some(keys));
        };
        let keys = KeySet::decode(&raw)?;
        if keys.prev.is_some() || self.pending_path().exists() {
            // An interrupted erase: the files it named go first, before anything is resealed.
            let named = self.read_erase_marker(&keys)?;
            for stem in &named {
                self.unlink_stem(stem)?;
            }
            return self.finish_erase(keys).map(Some);
        }
        Ok(Some(keys))
    }

    /// Step 1: record, sealed under the current key and written atomically, which files this erase
    /// removes.
    fn write_erase_marker(&self, keys: &KeySet, stems: &[String]) -> Result<(), String> {
        let plain = Zeroizing::new(serde_json::to_vec(stems).map_err(|e| e.to_string())?);
        let sealed = seal_file(&keys.cur, PENDING_STEM, &plain)?;
        let staged = crate::atomic_file::stage_secret(&self.pending_path(), &sealed)
            .map_err(|e| format!("the chat history could not be erased: {}", e.kind()))?;
        crate::atomic_file::commit_staged(&staged, &self.pending_path())
            .map_err(|e| format!("the chat history could not be erased: {}", e.kind()))
    }

    /// The files an interrupted erase named ([] when there is no marker). A marker that does not
    /// open, or names anything but a conversation, fails closed: resealing without knowing what
    /// was being erased could bring a deleted conversation back.
    fn read_erase_marker(&self, keys: &KeySet) -> Result<Vec<String>, String> {
        let bytes = match std::fs::read(self.pending_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("the chat history cannot be read: {}", e.kind())),
        };
        let damaged = || {
            "an interrupted delete left a record that does not open; the chat history stays \
             closed (Delete all history starts over)"
                .to_string()
        };
        let plain = open_sealed(keys, PENDING_STEM, &bytes).ok_or_else(damaged)?;
        let stems: Vec<String> = serde_json::from_slice(&plain).map_err(|_| damaged())?;
        let ok = stems
            .iter()
            .all(|s| s.strip_prefix(THREAD_PREFIX).is_some_and(valid_thread_id));
        if !ok {
            return Err(damaged());
        }
        Ok(stems)
    }

    /// Erase steps 3 to 6: reseal every file under a key the erased files never had, then forget
    /// the previous key. Files that opened under no key are left as they are (unreadable).
    fn finish_erase(&self, keys: KeySet) -> Result<KeySet, String> {
        let keys = if keys.prev.is_some() {
            keys
        } else {
            // Step 3 (an erase stopped before the new key was stored).
            let next = KeySet {
                cur: KeyGen::mint(keys.cur.gen.wrapping_add(1).max(1)),
                prev: Some(keys.cur),
            };
            self.store_history_keys(&next)?;
            #[cfg(test)]
            crash_point(CrashPoint::NewKeyStored)?;
            next
        };
        // Step 4.
        #[cfg(test)]
        let mut resealed = 0usize;
        for stem in self.sealed_stems()? {
            let Some(bytes) = self.read_stem(&stem)? else {
                continue;
            };
            if file_gen(&bytes) == Some(keys.cur.gen) {
                continue;
            }
            if let Some(plain) = open_sealed(&keys, &stem, &bytes) {
                let sealed = seal_file(&keys.cur, &stem, &plain)?;
                self.write_stem(&stem, &sealed)?;
                #[cfg(test)]
                {
                    resealed += 1;
                    if resealed == 1 {
                        crash_point(CrashPoint::FirstResealed)?;
                    }
                }
            }
        }
        #[cfg(test)]
        crash_point(CrashPoint::AllResealed)?;
        // Step 5: the marker goes while the key it is sealed under is still in the keyring, so a
        // crash before step 6 never leaves a marker no key opens. Every file it named was removed
        // in step 2 (or by the recovery, before anything was resealed), so nothing needs it now.
        match std::fs::remove_file(self.pending_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "the chat history could not be finished: {}",
                    e.kind()
                ))
            }
        }
        #[cfg(test)]
        crash_point(CrashPoint::MarkerRemoved)?;
        // Step 6: the previous seed leaves the keyring.
        let done = KeySet {
            cur: keys.cur,
            prev: None,
        };
        self.store_history_keys(&done)?;
        Ok(done)
    }

    /// Erase `stems` for good: steps 1 and 2, then the rotation.
    fn erase_stems(&self, keys: KeySet, stems: &[String]) -> Result<KeySet, String> {
        if stems.is_empty() {
            return Ok(keys);
        }
        self.write_erase_marker(&keys, stems)?;
        #[cfg(test)]
        crash_point(CrashPoint::MarkerWritten)?;
        for stem in stems {
            self.unlink_stem(stem)?;
        }
        #[cfg(test)]
        crash_point(CrashPoint::FilesRemoved)?;
        self.finish_erase(keys)
    }

    /// The retention setting: `Ok(None)` when it does not open. Never defaults an unreadable
    /// setting to "keep until deleted" (that would fail open).
    fn read_settings(&self, keys: &KeySet) -> Result<Option<Settings>, String> {
        match self.read_stem(SETTINGS_STEM)? {
            None => Ok(Some(Settings::default())),
            Some(bytes) => Ok(open_sealed(keys, SETTINGS_STEM, &bytes)
                .and_then(|p| serde_json::from_slice(&p).ok())),
        }
    }

    /// Every conversation that opens, most recently updated first. Conversations past the
    /// retention setting are erased first.
    pub fn list_threads(&self, now_ms: u64) -> Result<HistoryList, String> {
        let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(keys) = self.history_keys(false)? else {
            return Ok(HistoryList {
                threads: Vec::new(),
                unreadable: Vec::new(),
                retention_days: None,
                retention_unreadable: false,
                purged: 0,
            });
        };
        let settings = self.read_settings(&keys)?;
        let retention_unreadable = settings.is_none();
        let retention_days = settings.and_then(|s| s.retention_days);
        let (mut threads, unreadable) = self.open_threads(&keys)?;
        let mut purged = 0;
        if let Some(days) = retention_days {
            let cutoff = now_ms.saturating_sub(u64::from(days) * MS_PER_DAY);
            let old: Vec<String> = threads
                .iter()
                .filter(|t| t.updated_at < cutoff)
                .map(|t| thread_stem(&t.id))
                .collect();
            if !old.is_empty() {
                purged = old.len();
                self.erase_stems(keys, &old)?;
                threads.retain(|t| t.updated_at >= cutoff);
            }
        }
        Ok(HistoryList {
            threads,
            unreadable,
            retention_days,
            retention_unreadable,
            purged,
        })
    }

    fn open_threads(&self, keys: &KeySet) -> Result<(Vec<ThreadDoc>, Vec<String>), String> {
        let mut threads = Vec::new();
        let mut unreadable = Vec::new();
        for stem in self.sealed_stems()? {
            let Some(id) = stem.strip_prefix(THREAD_PREFIX) else {
                continue;
            };
            let Some(bytes) = self.read_stem(&stem)? else {
                continue;
            };
            let doc = open_sealed(keys, &stem, &bytes)
                .and_then(|p| serde_json::from_slice::<ThreadDoc>(&p).ok())
                .filter(|d| d.id == id);
            match doc {
                Some(d) => threads.push(d),
                None => unreadable.push(id.to_string()),
            }
        }
        threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok((threads, unreadable))
    }

    /// Seal one conversation. Beyond [`MAX_THREADS`], the least recently updated are erased.
    pub fn save_thread(&self, doc: ThreadDoc) -> Result<SaveOutcome, String> {
        if !valid_thread_id(&doc.id) {
            return Err("that conversation id is not valid".to_string());
        }
        if !doc.msgs.is_array() {
            return Err("a conversation's messages must be a list".to_string());
        }
        if let Some(p) = &doc.parent {
            if !valid_thread_id(&p.thread_id) || p.thread_id == doc.id {
                return Err("that conversation's parent id is not valid".to_string());
            }
        }
        let plain = Zeroizing::new(serde_json::to_vec(&doc).map_err(|e| e.to_string())?);
        if plain.len() > MAX_THREAD_BYTES {
            return Err("this conversation is too large to keep".to_string());
        }
        let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = self.history_keys(true)?.ok_or_else(Self::unreachable)?;
        let stem = thread_stem(&doc.id);
        // A file that does not open is never replaced: it may be evidence of tampering, or the
        // member's history under a key that comes back.
        if let Some(bytes) = self.read_stem(&stem)? {
            if open_sealed(&keys, &stem, &bytes).is_none() {
                return Err(
                    "this conversation's saved copy does not open; it is left untouched"
                        .to_string(),
                );
            }
        }
        let sealed = seal_file(&keys.cur, &stem, &plain)?;
        self.write_stem(&stem, &sealed)?;
        let (threads, _) = self.open_threads(&keys)?;
        let evicted: Vec<String> = threads
            .iter()
            .skip(MAX_THREADS)
            .map(|t| t.id.clone())
            .collect();
        if !evicted.is_empty() {
            let stems: Vec<String> = evicted.iter().map(|id| thread_stem(id)).collect();
            self.erase_stems(keys, &stems)?;
        }
        Ok(SaveOutcome { evicted })
    }

    /// Erase one conversation (file removed and the key rotated).
    pub fn erase_thread(&self, id: &str) -> Result<(), String> {
        if !valid_thread_id(id) {
            return Err("that conversation id is not valid".to_string());
        }
        let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(keys) = self.history_keys(false)? else {
            return Ok(());
        };
        let stem = thread_stem(id);
        if self.read_stem(&stem)?.is_none() {
            return Ok(());
        }
        self.erase_stems(keys, &[stem]).map(|_| ())
    }

    /// Erase everything: every file, then the key. Works without a key (the way out of a history
    /// whose key is gone).
    /// The key goes FIRST: a crash part way leaves files no key opens (refused at the next open,
    /// and "Delete all" again finishes), never a key with some of the files.
    pub fn erase_all(&self) -> Result<(), String> {
        let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.keyring
            .delete(KEY_ACCOUNT)
            .map_err(|_| Self::unreachable())?;
        for stem in self.sealed_stems()? {
            self.unlink_stem(&stem)?;
        }
        match std::fs::remove_file(self.pending_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "the chat history could not be erased: {}",
                    e.kind()
                ))
            }
        }
        Ok(())
    }

    /// Keep conversations `days` after their last update, or until deleted (`None`). A sealed
    /// setting that does not open is replaced only when `replace_unreadable` (the member chose a
    /// setting again after being told it is unreadable).
    pub fn set_retention_days(
        &self,
        days: Option<u32>,
        replace_unreadable: bool,
    ) -> Result<(), String> {
        if let Some(d) = days {
            if !(MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&d) {
                return Err(format!(
                    "keep history between {MIN_RETENTION_DAYS} and {MAX_RETENTION_DAYS} days, or until deleted"
                ));
            }
        }
        let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = self.history_keys(true)?.ok_or_else(Self::unreachable)?;
        if self.read_settings(&keys)?.is_none() && !replace_unreadable {
            return Err(
                "the saved retention setting does not open; choose a setting again to replace it"
                    .to_string(),
            );
        }
        let plain = Zeroizing::new(
            serde_json::to_vec(&Settings {
                retention_days: days,
            })
            .map_err(|e| e.to_string())?,
        );
        let sealed = seal_file(&keys.cur, SETTINGS_STEM, &plain)?;
        self.write_stem(SETTINGS_STEM, &sealed)
    }
}

// ------------------------------------------------------------------------------ commands

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn history_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(HISTORY_DIR))
}

fn with_store<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&HistoryStore<'_>) -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    crate::blocking::off_main(move || {
        let keyring = crate::custody::OsKeyring::legacy();
        let dir = history_dir(&app)?;
        f(&HistoryStore::new(dir, &keyring))
    })
}

/// **hermes_history_list** — every conversation kept on this device, decrypted in memory, plus
/// the retention setting; conversations past it are erased first. Data source: the sealed files in
/// `<app data>/hermes-history/`, opened with the history key in the OS keyring.
#[tauri::command]
pub async fn hermes_history_list(app_h: tauri::AppHandle) -> Result<HistoryList, String> {
    with_store(app_h, |s| s.list_threads(now_ms())).await
}

/// **hermes_history_save** — seal one conversation (the webview's thread) into
/// `<app data>/hermes-history/`. Beyond 50, the least recently updated are erased.
#[tauri::command]
pub async fn hermes_history_save(
    app_h: tauri::AppHandle,
    thread: ThreadDoc,
) -> Result<SaveOutcome, String> {
    with_store(app_h, move |s| s.save_thread(thread)).await
}

/// **hermes_history_delete** — erase one conversation: its file is removed and the history key
/// rotated, so no copy of it on this device opens again (an earlier OS backup is out of reach).
#[tauri::command]
pub async fn hermes_history_delete(app_h: tauri::AppHandle, id: String) -> Result<(), String> {
    with_store(app_h, move |s| s.erase_thread(&id)).await
}

/// **hermes_history_delete_all** — erase every conversation and the history key.
#[tauri::command]
pub async fn hermes_history_delete_all(app_h: tauri::AppHandle) -> Result<(), String> {
    with_store(app_h, |s| s.erase_all()).await
}

/// **hermes_history_set_retention** — keep conversations `days` (1 to 3650) after their last
/// update, or until deleted (`null`). Sealed like the conversations.
#[tauri::command]
pub async fn hermes_history_set_retention(
    app_h: tauri::AppHandle,
    days: Option<u32>,
    replace_unreadable: Option<bool>,
) -> Result<(), String> {
    with_store(app_h, move |s| {
        s.set_retention_days(days, replace_unreadable.unwrap_or(false))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_history_tests.rs");
}
