//! citrate-core — HUP-S10.5 device-key recovery kit.
//!
//! The app mints two random keys of its own in the OS keychain (service
//! `ai.citrate.core`): the node storage key, which encrypts the node's chain data at rest,
//! and the memory-store key, which wraps the memory store that the journal's daily entry
//! draws from. Neither can be derived from anything else, so a lost keychain (a new
//! computer, a keychain reset) makes that data unreadable. This module lets the member back
//! them up in the form they choose:
//!
//! - **Recovery phrase:** a sheet with one 24-word BIP39 phrase per key, written to a file
//!   the member picks (owner-only mode), to print and then delete. The words never cross
//!   into the webview (I-2): Rust writes the file directly.
//! - **Recovery file:** a passphrase-sealed file (the custody vault's Argon2id derivation
//!   and AES-256-GCM, the same construction as the S10.4 journal export, with its own
//!   magic so the two can never be confused).
//!
//! Restore is all-or-nothing and refuses a wrong kit: a phrase whose checksum or printed
//! fingerprint does not match, a file opened with the wrong passphrase, a kit whose keys do
//! not match the fingerprints this install recorded when it made its own kit, and (unless
//! the member asks to replace) a kit that would overwrite a different live key.
//!
//! Out of scope, stated honestly in the UI and docs: the wallet (its own recovery path, a
//! separate owner decision), the comms key (re-derived from the wallet), and journal export
//! files (opened with the passphrase they were exported with, which nothing here stores).
//! No signing, no wallet material (Rule 3).

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::custody::Keyring;

/// One device-minted key the kit covers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeviceKeySpec {
    /// The keychain account (service `ai.citrate.core`).
    pub account: &'static str,
    /// What the member sees.
    pub label: &'static str,
    /// What the key protects, in plain words.
    pub covers: &'static str,
    /// Key length, bytes.
    pub len: usize,
}

/// The keys the kit backs up. Names match `node.rs` and `memory.rs`.
pub(crate) const DEVICE_KEYS: &[DeviceKeySpec] = &[
    DeviceKeySpec {
        account: "node-storage-key",
        label: "Node storage key",
        covers: "the node's encrypted chain data on this computer",
        len: 32,
    },
    DeviceKeySpec {
        account: "memory-store-key",
        label: "Journal and memory key",
        covers: "the memory store your journal's daily entry draws from",
        len: 32,
    },
];

/// Sealed-file magic.
pub(crate) const KIT_MAGIC: &[u8; 8] = b"CITRKEY\0";
const KIT_VERSION: u8 = 1;
const KDF_ARGON2ID_V13: u8 = 1;
const AEAD_AES256GCM: u8 = 1;
const SALT_LEN: usize = citrate_core_kit::custody::PASSPHRASE_SALT_LEN;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = 8 + 1 + 1 + 12 + 1 + SALT_LEN + 1 + 1 + NONCE_LEN;
const TAG_LEN: usize = 16;
/// A kit is tiny; anything over this is not one.
const MAX_KIT_BYTES: u64 = 64 * 1024;
/// Minimum recovery-file passphrase length, in characters.
pub(crate) const MIN_PASSPHRASE_CHARS: usize = 12;
/// Extension of the sealed recovery file.
pub(crate) const FILE_EXT: &str = "citrate-recovery";
/// Suffix of the recovery sheet (phrase form).
pub(crate) const SHEET_SUFFIX: &str = ".citrate-recovery.txt";
/// Where this install records the fingerprints of the keys its last kit held.
const RECORD_FILE: &str = "recovery/fingerprints.json";
/// Words per phrase (32 bytes of entropy).
const WORDS_PER_KEY: usize = 24;
const SHEET_TITLE: &str = "CITRATE DEVICE KEY RECOVERY SHEET";
const SHEET_END: &str = "END OF SHEET";

/// Why a backup or restore failed. Member-facing, no internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecoveryError {
    NothingToBackUp,
    KeyringUnavailable,
    PhraseInvalid,
    NotARecoveryKit,
    UnknownKey,
    WrongPassphraseOrDamaged,
    WrongRecovery,
    WouldReplaceLiveKey(String),
    PassphraseTooShort,
    UnsafePath,
    Io,
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToBackUp => f.write_str(
                "There are no device keys to back up yet. They are created the first time the node or memory store starts.",
            ),
            Self::KeyringUnavailable => {
                f.write_str("The system keychain is not reachable, so nothing was read or changed.")
            }
            Self::PhraseInvalid => f.write_str(
                "One of the recovery phrases does not check out. Look for a mistyped or missing word.",
            ),
            Self::NotARecoveryKit => f.write_str("This is not a Citrate device key recovery kit."),
            Self::UnknownKey => {
                f.write_str("This recovery kit names a key this app does not use.")
            }
            Self::WrongPassphraseOrDamaged => f.write_str(
                "That passphrase does not open this recovery file, or the file was changed or damaged.",
            ),
            Self::WrongRecovery => f.write_str(
                "This recovery kit belongs to a different install. Its keys do not match the data on this computer, so nothing was changed.",
            ),
            Self::WouldReplaceLiveKey(account) => write!(
                f,
                "This computer already has a different {account}. Restoring would replace it; confirm the replace option to continue."
            ),
            Self::PassphraseTooShort => write!(
                f,
                "Use a passphrase of at least {MIN_PASSPHRASE_CHARS} characters."
            ),
            Self::UnsafePath => f.write_str(
                "Choose a file name ending in .citrate-recovery (file) or .citrate-recovery.txt (phrase sheet) in a normal folder.",
            ),
            Self::Io => f.write_str("The recovery kit could not be read or written."),
        }
    }
}

type Result<T> = std::result::Result<T, RecoveryError>;

/// A key held for backup or restore. The bytes wipe on drop.
pub(crate) struct KitKey {
    pub account: String,
    pub key: Zeroizing<Vec<u8>>,
}

/// Debug never prints the key bytes.
impl std::fmt::Debug for KitKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KitKey")
            .field("account", &self.account)
            .field("key", &"<redacted>")
            .finish()
    }
}

fn spec_for(account: &str) -> Option<&'static DeviceKeySpec> {
    DEVICE_KEYS.iter().find(|s| s.account == account)
}

/// A short, non-secret fingerprint of a key: the first 8 bytes of
/// SHA-256(domain || account || 0x00 || key), hex. Used to tell kits apart; it reveals
/// nothing useful about the key.
pub(crate) fn fingerprint(account: &str, key: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"citrate-device-key-fingerprint-v1");
    h.update(account.as_bytes());
    h.update([0u8]);
    h.update(key);
    hex::encode(&h.finalize()[..8])
}

/// Fingerprints of a set of keys, by account.
pub(crate) fn fingerprints_of(keys: &[KitKey]) -> BTreeMap<String, String> {
    keys.iter()
        .map(|k| (k.account.clone(), fingerprint(&k.account, &k.key)))
        .collect()
}

/// Read every covered key that exists. An unreachable keychain fails closed; a stored key
/// of the wrong length is refused (it would not restore cleanly either).
pub(crate) fn collect_keys(keyring: &dyn Keyring) -> Result<Vec<KitKey>> {
    let mut out = Vec::new();
    for spec in DEVICE_KEYS {
        if let Some(mut bytes) = keyring
            .get(spec.account)
            .map_err(|_| RecoveryError::KeyringUnavailable)?
        {
            if bytes.len() != spec.len {
                bytes.zeroize();
                return Err(RecoveryError::Io);
            }
            out.push(KitKey {
                account: spec.account.to_string(),
                key: Zeroizing::new(bytes),
            });
        }
    }
    if out.is_empty() {
        return Err(RecoveryError::NothingToBackUp);
    }
    Ok(out)
}

// --------------------------------------------------------------------------
// Phrase sheet
// --------------------------------------------------------------------------

/// Render the recovery sheet. `made_on` is a date string for the member's reference.
pub(crate) fn render_sheet(keys: &[KitKey], made_on: &str) -> Zeroizing<String> {
    let mut s = Zeroizing::new(String::new());
    s.push_str(SHEET_TITLE);
    s.push_str("\nFormat 1, made ");
    s.push_str(made_on);
    s.push_str(
        "\n\nKeep this sheet offline: print it, store it somewhere safe, then delete this file.\n\
         Anyone holding it and a copy of this computer's app data can read what these keys protect.\n\
         It does not contain your wallet, and it cannot open journal export files.\n\
         To restore: Settings, Privacy and recovery, Restore from recovery phrase.\n",
    );
    for k in keys {
        let label = spec_for(&k.account).map(|s| s.label).unwrap_or("Key");
        let covers = spec_for(&k.account).map(|s| s.covers).unwrap_or("");
        s.push_str("\n[");
        s.push_str(&k.account);
        s.push_str("] fingerprint ");
        s.push_str(&fingerprint(&k.account, &k.key));
        s.push_str("\n# ");
        s.push_str(label);
        s.push_str(": protects ");
        s.push_str(covers);
        s.push('\n');
        // from_entropy only fails on an unsupported length; covered keys are 32 bytes.
        if let Ok(m) = bip39::Mnemonic::from_entropy(&k.key) {
            let phrase = Zeroizing::new(m.to_string());
            for (i, w) in phrase.split(' ').enumerate() {
                s.push_str(w);
                s.push(if (i + 1) % 6 == 0 { '\n' } else { ' ' });
            }
        }
    }
    s.push('\n');
    s.push_str(SHEET_END);
    s.push('\n');
    s
}

/// Parse a recovery sheet (or a member's typed copy of it). Case and spacing are
/// tolerated; lines starting with `#` and the header text are ignored.
pub(crate) fn parse_sheet(text: &str) -> Result<Vec<KitKey>> {
    struct Section {
        account: String,
        printed_fp: Option<String>,
        words: Zeroizing<Vec<String>>,
    }
    let mut sections: Vec<Section> = Vec::new();
    for raw in text.lines() {
        let line = Zeroizing::new(
            raw.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
        );
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.as_str() == SHEET_END.to_lowercase() {
            break;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let Some(close) = rest.find(']') else {
                return Err(RecoveryError::NotARecoveryKit);
            };
            let account = rest[..close].trim().to_string();
            let printed_fp = rest[close + 1..]
                .trim()
                .strip_prefix("fingerprint")
                .map(|f| f.trim().to_string())
                .filter(|f| !f.is_empty());
            sections.push(Section {
                account,
                printed_fp,
                words: Zeroizing::new(Vec::new()),
            });
            continue;
        }
        if let Some(cur) = sections.last_mut() {
            for w in line.split(' ') {
                cur.words.push(w.to_string());
            }
        }
    }
    if sections.is_empty() {
        return Err(RecoveryError::NotARecoveryKit);
    }
    let mut out: Vec<KitKey> = Vec::new();
    for sec in sections.iter() {
        let spec = spec_for(&sec.account).ok_or(RecoveryError::UnknownKey)?;
        if out.iter().any(|k| k.account == sec.account) {
            return Err(RecoveryError::NotARecoveryKit);
        }
        if sec.words.len() != WORDS_PER_KEY {
            return Err(RecoveryError::PhraseInvalid);
        }
        let phrase = Zeroizing::new(sec.words.join(" "));
        let m = bip39::Mnemonic::parse_in(bip39::Language::English, phrase.as_str())
            .map_err(|_| RecoveryError::PhraseInvalid)?;
        let key = Zeroizing::new(m.to_entropy());
        if key.len() != spec.len {
            return Err(RecoveryError::PhraseInvalid);
        }
        if let Some(fp) = &sec.printed_fp {
            if *fp != fingerprint(&sec.account, &key) {
                return Err(RecoveryError::PhraseInvalid);
            }
        }
        out.push(KitKey {
            account: sec.account.clone(),
            key,
        });
    }
    Ok(out)
}

// --------------------------------------------------------------------------
// Sealed recovery file
// --------------------------------------------------------------------------

fn header(salt: &[u8; SALT_LEN], nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
    let (m, t, p) = citrate_core_kit::custody::PASSPHRASE_KDF_PARAMS;
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(KIT_MAGIC);
    h.push(KIT_VERSION);
    h.push(KDF_ARGON2ID_V13);
    h.extend_from_slice(&m.to_be_bytes());
    h.extend_from_slice(&t.to_be_bytes());
    h.extend_from_slice(&p.to_be_bytes());
    h.push(SALT_LEN as u8);
    h.extend_from_slice(salt);
    h.push(AEAD_AES256GCM);
    h.push(NONCE_LEN as u8);
    h.extend_from_slice(nonce);
    h
}

fn derive_file_key(passphrase: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    citrate_core_kit::custody::derive_passphrase_key(passphrase, salt)
        .map_err(|_| RecoveryError::WrongPassphraseOrDamaged)
}

/// The sealed plaintext: a small JSON document.
fn kit_json(keys: &[KitKey], made_on: &str) -> Zeroizing<String> {
    let entries: Vec<serde_json::Value> = keys
        .iter()
        .map(|k| {
            serde_json::json!({
                "account": k.account,
                "keyHex": hex::encode(k.key.as_slice()),
                "fingerprint": fingerprint(&k.account, &k.key),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "format": "citrate-device-key-kit",
        "version": 1,
        "madeOn": made_on,
        "keys": entries,
    });
    Zeroizing::new(doc.to_string())
}

fn parse_kit_json(text: &str) -> Result<Vec<KitKey>> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|_| RecoveryError::NotARecoveryKit)?;
    if v["format"] != "citrate-device-key-kit" || v["version"] != 1 {
        return Err(RecoveryError::NotARecoveryKit);
    }
    let arr = v["keys"].as_array().ok_or(RecoveryError::NotARecoveryKit)?;
    let mut out: Vec<KitKey> = Vec::new();
    for e in arr {
        let account = e["account"]
            .as_str()
            .ok_or(RecoveryError::NotARecoveryKit)?;
        let spec = spec_for(account).ok_or(RecoveryError::UnknownKey)?;
        if out.iter().any(|k| k.account == account) {
            return Err(RecoveryError::NotARecoveryKit);
        }
        let hex_str = e["keyHex"].as_str().ok_or(RecoveryError::NotARecoveryKit)?;
        let key = Zeroizing::new(hex::decode(hex_str).map_err(|_| RecoveryError::NotARecoveryKit)?);
        if key.len() != spec.len {
            return Err(RecoveryError::NotARecoveryKit);
        }
        if e["fingerprint"].as_str() != Some(fingerprint(account, &key).as_str()) {
            return Err(RecoveryError::WrongPassphraseOrDamaged);
        }
        out.push(KitKey {
            account: account.to_string(),
            key,
        });
    }
    if out.is_empty() {
        return Err(RecoveryError::NotARecoveryKit);
    }
    Ok(out)
}

/// Seal the kit under `passphrase` into a complete file image.
pub(crate) fn seal_kit(passphrase: &[u8], keys: &[KitKey], made_on: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce);
    let head = header(&salt, &nonce);
    let key = derive_file_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let pt = kit_json(keys, made_on);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: pt.as_bytes(),
                aad: &head,
            },
        )
        .map_err(|_| RecoveryError::Io)?;
    let mut out = head;
    out.extend_from_slice(&ct);
    Ok(out)
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    let arr: [u8; 4] = s.try_into().ok()?;
    Some(u32::from_be_bytes(arr))
}

/// Open a sealed kit. Header fields are checked before any key derivation, and the header
/// is the AEAD associated data.
pub(crate) fn open_kit(passphrase: &[u8], file: &[u8]) -> Result<Vec<KitKey>> {
    if file.len() < 9 || &file[..8] != KIT_MAGIC {
        return Err(RecoveryError::NotARecoveryKit);
    }
    if file[8] != KIT_VERSION || file.len() < HEADER_LEN + TAG_LEN {
        return Err(RecoveryError::NotARecoveryKit);
    }
    let params = (be_u32(file, 10), be_u32(file, 14), be_u32(file, 18));
    let (m, t, p) = citrate_core_kit::custody::PASSPHRASE_KDF_PARAMS;
    if file[9] != KDF_ARGON2ID_V13
        || params != (Some(m), Some(t), Some(p))
        || file[22] as usize != SALT_LEN
        || file[23 + SALT_LEN] != AEAD_AES256GCM
        || file[24 + SALT_LEN] as usize != NONCE_LEN
    {
        return Err(RecoveryError::NotARecoveryKit);
    }
    let head = &file[..HEADER_LEN];
    let salt = &file[23..23 + SALT_LEN];
    let nonce = &file[25 + SALT_LEN..HEADER_LEN];
    let key = derive_file_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let pt = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: &file[HEADER_LEN..],
                    aad: head,
                },
            )
            .map_err(|_| RecoveryError::WrongPassphraseOrDamaged)?,
    );
    let text = std::str::from_utf8(&pt).map_err(|_| RecoveryError::NotARecoveryKit)?;
    parse_kit_json(text)
}

// --------------------------------------------------------------------------
// Files on disk
// --------------------------------------------------------------------------

/// Which form a kit path is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KitForm {
    Sheet,
    File,
}

/// A kit path must be absolute and carry the form's exact suffix, so these commands can only
/// ever create or replace recovery kit files.
pub(crate) fn check_kit_path(path: &Path, form: KitForm) -> Result<()> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ok = match form {
        KitForm::Sheet => name.len() > SHEET_SUFFIX.len() && name.ends_with(SHEET_SUFFIX),
        KitForm::File => {
            path.extension().and_then(|e| e.to_str()) == Some(FILE_EXT)
                && name.len() > FILE_EXT.len() + 1
        }
    };
    if !path.is_absolute() || !ok {
        return Err(RecoveryError::UnsafePath);
    }
    if std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(RecoveryError::UnsafePath);
    }
    Ok(())
}

/// Recovery-file passphrase policy.
pub(crate) fn check_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(RecoveryError::PassphraseTooShort);
    }
    Ok(())
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = crate::journal_export::open_export_file(path).map_err(|_| RecoveryError::Io)?;
    f.write_all(bytes).map_err(|_| RecoveryError::Io)?;
    f.sync_all().map_err(|_| RecoveryError::Io)?;
    Ok(())
}

fn read_small(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let f = std::fs::File::open(path).map_err(|_| RecoveryError::Io)?;
    let meta = f.metadata().map_err(|_| RecoveryError::Io)?;
    if !meta.is_file() {
        return Err(RecoveryError::NotARecoveryKit);
    }
    if meta.len() > MAX_KIT_BYTES {
        return Err(RecoveryError::NotARecoveryKit);
    }
    let mut buf = Zeroizing::new(Vec::new());
    f.take(MAX_KIT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| RecoveryError::Io)?;
    Ok(buf)
}

/// Write the phrase sheet to `path` (owner-only).
pub(crate) fn write_sheet(path: &Path, keys: &[KitKey], made_on: &str) -> Result<()> {
    check_kit_path(path, KitForm::Sheet)?;
    let sheet = render_sheet(keys, made_on);
    write_owner_only(path, sheet.as_bytes())
}

/// Seal the kit and write it to `path` (owner-only).
pub(crate) fn write_sealed(
    path: &Path,
    passphrase: &str,
    keys: &[KitKey],
    made_on: &str,
) -> Result<()> {
    check_passphrase(passphrase)?;
    check_kit_path(path, KitForm::File)?;
    let sealed = seal_kit(passphrase.as_bytes(), keys, made_on)?;
    write_owner_only(path, &sealed)
}

/// Read and open a sealed kit file.
pub(crate) fn read_sealed(path: &Path, passphrase: &str) -> Result<Vec<KitKey>> {
    if !path.is_absolute() {
        return Err(RecoveryError::UnsafePath);
    }
    let bytes = read_small(path)?;
    open_kit(passphrase.as_bytes(), &bytes)
}

// --------------------------------------------------------------------------
// Recorded fingerprints (non-secret) in the app data dir
// --------------------------------------------------------------------------

/// Record the fingerprints of the keys a kit was just made from.
pub(crate) fn record_fingerprints(data_dir: &Path, keys: &[KitKey]) -> Result<()> {
    let path = data_dir.join(RECORD_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| RecoveryError::Io)?;
    }
    let body =
        serde_json::to_string_pretty(&fingerprints_of(keys)).map_err(|_| RecoveryError::Io)?;
    std::fs::write(&path, body).map_err(|_| RecoveryError::Io)
}

/// The fingerprints this install recorded, if it ever made a kit.
pub(crate) fn read_recorded(data_dir: &Path) -> Option<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(data_dir.join(RECORD_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

// --------------------------------------------------------------------------
// Restore
// --------------------------------------------------------------------------

/// What a restore did.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RestoreReport {
    /// Keys written to the keychain.
    pub restored: Vec<String>,
    /// Keys the keychain already held with the same value.
    pub unchanged: Vec<String>,
}

/// Restore `keys` into the keychain. Every check runs before any write (all-or-nothing):
/// a key that does not match this install's recorded fingerprint is a wrong kit and is
/// always refused; a key that differs from a live key needs `replace`.
pub(crate) fn restore_keys(
    keyring: &dyn Keyring,
    keys: &[KitKey],
    recorded: Option<&BTreeMap<String, String>>,
    replace: bool,
) -> Result<RestoreReport> {
    let mut to_write: Vec<&KitKey> = Vec::new();
    let mut unchanged = Vec::new();
    for k in keys {
        let spec = spec_for(&k.account).ok_or(RecoveryError::UnknownKey)?;
        if k.key.len() != spec.len {
            return Err(RecoveryError::NotARecoveryKit);
        }
        if let Some(rec) = recorded.and_then(|r| r.get(&k.account)) {
            if *rec != fingerprint(&k.account, &k.key) {
                return Err(RecoveryError::WrongRecovery);
            }
        }
        let live = keyring
            .get(&k.account)
            .map_err(|_| RecoveryError::KeyringUnavailable)?
            .map(Zeroizing::new);
        match live {
            Some(cur) if cur.as_slice() == k.key.as_slice() => unchanged.push(k.account.clone()),
            Some(_) if !replace => {
                return Err(RecoveryError::WouldReplaceLiveKey(k.account.clone()))
            }
            _ => to_write.push(k),
        }
    }
    let mut restored = Vec::new();
    for k in to_write {
        keyring
            .set(&k.account, &k.key)
            .map_err(|_| RecoveryError::KeyringUnavailable)?;
        restored.push(k.account.clone());
    }
    Ok(RestoreReport {
        restored,
        unchanged,
    })
}

// --------------------------------------------------------------------------
// Status
// --------------------------------------------------------------------------

/// One covered key, without its bytes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KeyStatus {
    pub account: String,
    pub label: String,
    pub covers: String,
    pub present: bool,
    /// Fingerprint of the live key, when present.
    pub fingerprint: Option<String>,
    /// Fingerprint recorded when this install last made a kit.
    #[serde(rename = "recordedFingerprint")]
    pub recorded_fingerprint: Option<String>,
}

/// What the recovery panel shows. Never carries key bytes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RecoveryStatus {
    #[serde(rename = "keyringReachable")]
    pub keyring_reachable: bool,
    pub keys: Vec<KeyStatus>,
}

pub(crate) fn status_of(keyring: &dyn Keyring, data_dir: &Path) -> RecoveryStatus {
    let recorded = read_recorded(data_dir).unwrap_or_default();
    let mut reachable = true;
    let keys = DEVICE_KEYS
        .iter()
        .map(|spec| {
            let live = match keyring.get(spec.account) {
                Ok(v) => v.map(Zeroizing::new),
                Err(_) => {
                    reachable = false;
                    None
                }
            };
            KeyStatus {
                account: spec.account.to_string(),
                label: spec.label.to_string(),
                covers: spec.covers.to_string(),
                present: live.is_some(),
                fingerprint: live.as_ref().map(|k| fingerprint(spec.account, k)),
                recorded_fingerprint: recorded.get(spec.account).cloned(),
            }
        })
        .collect();
    RecoveryStatus {
        keyring_reachable: reachable,
        keys,
    }
}

// --------------------------------------------------------------------------
// Commands
// --------------------------------------------------------------------------

fn data_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path().app_data_dir().map_err(|e| e.to_string())
}

fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// **recovery_kit_status** — which device keys exist, their fingerprints, and what this
/// install recorded. No key bytes.
#[tauri::command]
pub async fn recovery_kit_status<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> std::result::Result<RecoveryStatus, String> {
    crate::blocking::off_main(move || {
        let dir = data_dir(&app)?;
        Ok(status_of(&crate::custody::OsKeyring::legacy(), &dir))
    })
    .await
}

/// **recovery_kit_save_phrase** — write the phrase sheet to the member-chosen path. The
/// words are written by Rust and never returned. Returns the number of keys covered.
#[tauri::command]
pub async fn recovery_kit_save_phrase<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
) -> std::result::Result<usize, String> {
    crate::blocking::off_main(move || {
        let dir = data_dir(&app)?;
        let keys = collect_keys(&crate::custody::OsKeyring::legacy()).map_err(|e| e.to_string())?;
        write_sheet(Path::new(&path), &keys, &today()).map_err(|e| e.to_string())?;
        record_fingerprints(&dir, &keys).map_err(|e| e.to_string())?;
        Ok(keys.len())
    })
    .await
}

/// **recovery_kit_save_file** — seal the kit under the member's passphrase and write it to
/// the chosen path. Returns the number of keys covered.
#[tauri::command]
pub async fn recovery_kit_save_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
    passphrase: String,
) -> std::result::Result<usize, String> {
    crate::blocking::off_main(move || {
        let passphrase = Zeroizing::new(passphrase);
        let dir = data_dir(&app)?;
        let keys = collect_keys(&crate::custody::OsKeyring::legacy()).map_err(|e| e.to_string())?;
        write_sealed(Path::new(&path), &passphrase, &keys, &today()).map_err(|e| e.to_string())?;
        record_fingerprints(&dir, &keys).map_err(|e| e.to_string())?;
        Ok(keys.len())
    })
    .await
}

/// **recovery_kit_restore_phrase** — restore from the member's typed or pasted sheet.
#[tauri::command]
pub async fn recovery_kit_restore_phrase<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    sheet: String,
    replace: bool,
) -> std::result::Result<RestoreReport, String> {
    crate::blocking::off_main(move || {
        let sheet = Zeroizing::new(sheet);
        let dir = data_dir(&app)?;
        let keys = parse_sheet(&sheet).map_err(|e| e.to_string())?;
        let recorded = read_recorded(&dir);
        restore_keys(
            &crate::custody::OsKeyring::legacy(),
            &keys,
            recorded.as_ref(),
            replace,
        )
        .map_err(|e| e.to_string())
    })
    .await
}

/// **recovery_kit_restore_file** — restore from a sealed recovery file.
#[tauri::command]
pub async fn recovery_kit_restore_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
    passphrase: String,
    replace: bool,
) -> std::result::Result<RestoreReport, String> {
    crate::blocking::off_main(move || {
        let passphrase = Zeroizing::new(passphrase);
        let dir = data_dir(&app)?;
        let keys = read_sealed(Path::new(&path), &passphrase).map_err(|e| e.to_string())?;
        let recorded = read_recorded(&dir);
        restore_keys(
            &crate::custody::OsKeyring::legacy(),
            &keys,
            recorded.as_ref(),
            replace,
        )
        .map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("recovery_kit_tests.rs");
}
