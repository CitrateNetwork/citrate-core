//! STO-2 (v0.6.0) — the encrypted backup file.
//!
//! An encrypted backup is ONE file, `citrate-backup-YYYYMMDD-HHMMSS.citrate-backup`, in the folder
//! the member picks. It holds a K4 backup folder (manifest, `files/`, `memory/`) sealed under the
//! member's passphrase with the `recovery_kit` construction: the custody vault's Argon2id
//! derivation (D-A2-1 parameters, recorded in the header and refused on any other value) and
//! AES-256-GCM, with its own magic and a versioned header. Because a backup can be large, the
//! AEAD runs as the STREAM construction (`aead::stream`, big-endian 32-bit counter + last-chunk
//! flag), so it is written and read in 1 MiB chunks without holding the backup in memory, and a
//! truncated, reordered or spliced file fails to open.
//!
//! ```text
//! header (52 bytes, the associated data of EVERY chunk)
//!   magic "CITRBAK\0" (8) | version 1 (1) | kdf 1 = Argon2id v1.3 (1)
//!   m_cost KiB (4 BE) | t_cost (4 BE) | p_cost (4 BE) | salt_len 16 (1) | salt (16)
//!   aead 1 = AES-256-GCM STREAM-BE32 (1) | nonce_prefix_len 7 (1) | nonce_prefix (7)
//!   chunk_len (4 BE, plaintext bytes per chunk)
//! body: chunks of chunk_len + 16 bytes; the last one is shorter and sealed as the last chunk
//! ```
//!
//! The sealed plaintext is a record stream: `b'D' | path_len (2 BE) | path` for a directory,
//! `b'F' | path_len (2 BE) | path | size (8 BE) | bytes` for a file, and a single `b'E'` at the
//! end. The manifest comes first. Paths are the backup's own `/`-separated relative paths and
//! are checked with the same rule as the manifest ([`super::safe_rel_path`]) before anything is
//! written.
//!
//! Nothing readable reaches the chosen folder: export builds the plain folder in a private
//! staging dir inside the app data dir (where the memory store already lives unencrypted), seals
//! it into the destination, and removes the staging copy. Restore and verify open the file into a
//! staging dir the same way, run the unchanged K4 check and restore on it, and remove it. A wrong
//! passphrase, a damaged file or a file of another kind is refused before any store is touched.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use aes_gcm::aead::stream::{DecryptorBE32, EncryptorBE32};
use aes_gcm::aead::{KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key};
use rand::RngCore;
use zeroize::Zeroizing;

use super::{
    create_private_dir, create_private_dir_all, create_private_file, io_err, safe_rel_path,
    slash_path, walk, BackupError, MANIFEST_NAME,
};

/// File magic. Distinct from the recovery kit (`CITRKEY\0`) and the journal export, so one can
/// never be opened as another.
pub const MAGIC: &[u8; 8] = b"CITRBAK\0";
/// File extension of an encrypted backup.
pub const FILE_EXT: &str = "citrate-backup";
const VERSION: u8 = 1;
const KDF_ARGON2ID_V13: u8 = 1;
const AEAD_AES256GCM_STREAM_BE32: u8 = 1;
const SALT_LEN: usize = citrate_core_kit::custody::PASSPHRASE_SALT_LEN;
/// STREAM-BE32 over a 96-bit nonce: 7-byte prefix, 4-byte counter, 1-byte last flag.
const NONCE_PREFIX_LEN: usize = 7;
/// Plaintext bytes per chunk.
pub const CHUNK_LEN: usize = 1 << 20;
const TAG_LEN: usize = 16;
/// Header length in bytes.
pub const HEADER_LEN: usize = 8 + 1 + 1 + 12 + 1 + SALT_LEN + 1 + 1 + NONCE_PREFIX_LEN + 4;
/// Minimum passphrase length, in characters (the recovery kit's rule).
pub const MIN_PASSPHRASE_CHARS: usize = crate::recovery_kit::MIN_PASSPHRASE_CHARS;
/// The longest record path accepted.
const MAX_PATH_LEN: usize = 4096;

const REC_DIR: u8 = b'D';
const REC_FILE: u8 = b'F';
const REC_END: u8 = b'E';

/// How the member asked an export to be written.
pub enum Protection {
    /// Sealed under this passphrase (the default).
    Passphrase(Zeroizing<String>),
    /// The plain K4 folder, chosen explicitly after the warning.
    Plaintext,
}

/// Debug never prints the passphrase.
impl std::fmt::Debug for Protection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protection::Passphrase(_) => f.write_str("Passphrase(<redacted>)"),
            Protection::Plaintext => f.write_str("Plaintext"),
        }
    }
}

impl Protection {
    /// The command's two inputs into one choice. An export is encrypted unless the member
    /// chose plaintext, and choosing plaintext with a passphrase, or neither, is refused.
    pub fn choose(passphrase: Option<&str>, plaintext: bool) -> Result<Protection, BackupError> {
        match (passphrase, plaintext) {
            (Some(_), true) => Err(BackupError::InvalidFolder(
                "choose a passphrase or an unencrypted backup, not both".to_string(),
            )),
            (None, true) => Ok(Protection::Plaintext),
            (None, false) => Err(BackupError::InvalidFolder(
                "set a passphrase to encrypt the backup, or choose an unencrypted backup"
                    .to_string(),
            )),
            (Some(p), false) => {
                check_passphrase(p)?;
                Ok(Protection::Passphrase(Zeroizing::new(p.to_string())))
            }
        }
    }
}

/// The passphrase policy (same minimum as the recovery file).
pub fn check_passphrase(passphrase: &str) -> Result<(), BackupError> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(BackupError::InvalidFolder(format!(
            "use a passphrase of at least {MIN_PASSPHRASE_CHARS} characters"
        )));
    }
    Ok(())
}

fn wrong_passphrase() -> BackupError {
    BackupError::Manifest(
        "that passphrase does not open this backup, or the file was changed or damaged".to_string(),
    )
}

fn not_a_backup() -> BackupError {
    BackupError::Manifest("this is not a Citrate encrypted backup".to_string())
}

/// Build the header for `salt` and `nonce_prefix`.
pub(crate) fn header(salt: &[u8; SALT_LEN], nonce_prefix: &[u8; NONCE_PREFIX_LEN]) -> Vec<u8> {
    let (m, t, p) = citrate_core_kit::custody::PASSPHRASE_KDF_PARAMS;
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.push(VERSION);
    h.push(KDF_ARGON2ID_V13);
    h.extend_from_slice(&m.to_be_bytes());
    h.extend_from_slice(&t.to_be_bytes());
    h.extend_from_slice(&p.to_be_bytes());
    h.push(SALT_LEN as u8);
    h.extend_from_slice(salt);
    h.push(AEAD_AES256GCM_STREAM_BE32);
    h.push(NONCE_PREFIX_LEN as u8);
    h.extend_from_slice(nonce_prefix);
    h.extend_from_slice(&(CHUNK_LEN as u32).to_be_bytes());
    h
}

fn red() -> BackupError {
    io_err("seal", "RED: the sealed format is not implemented yet")
}

pub(crate) struct SealWriter<W: Write> {
    out: W,
}

impl<W: Write> SealWriter<W> {
    pub(crate) fn start(
        out: W,
        _passphrase: &[u8],
        _salt: [u8; SALT_LEN],
        _nonce_prefix: [u8; NONCE_PREFIX_LEN],
    ) -> Result<Self, BackupError> {
        let _ = out;
        Err(red())
    }
    pub(crate) fn feed(&mut self, _data: &[u8]) -> Result<(), BackupError> {
        Err(red())
    }
    pub(crate) fn finish(self) -> Result<W, BackupError> {
        let _ = self.out;
        Err(red())
    }
}

pub(crate) struct OpenReader<R: Read> {
    src: R,
}

impl<R: Read> OpenReader<R> {
    pub(crate) fn start(src: R, _passphrase: &[u8]) -> Result<Self, BackupError> {
        let _ = src;
        Err(red())
    }
    pub(crate) fn take_exact(&mut self, _buf: &mut [u8]) -> Result<(), BackupError> {
        let _ = &self.src;
        Err(red())
    }
    pub(crate) fn at_end(&self) -> bool {
        false
    }
}

pub(crate) fn seal_folder(
    _folder: &Path,
    _file: &Path,
    _passphrase: &[u8],
) -> Result<u64, BackupError> {
    Err(red())
}

pub fn is_sealed_file(_path: &Path) -> bool {
    false
}

pub(crate) fn open_into(_file: &Path, _passphrase: &[u8], _into: &Path) -> Result<(), BackupError> {
    Err(red())
}

pub fn sealed_file_name(now: u64) -> String {
    format!("{}.{FILE_EXT}", super::backup_folder_name(now))
}

pub(crate) fn staging_dir(staging_root: &Path, tag: &str) -> Result<PathBuf, BackupError> {
    create_private_dir_all(staging_root)?;
    let mut rnd = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut rnd);
    Ok(staging_root.join(format!("{tag}-{}", hex::encode(rnd))))
}

pub(crate) struct StagingGuard(pub PathBuf);

impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
