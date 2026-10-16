//! citrate-core — HUP-S10.4 journal encrypted export / import.
//!
//! The member's journal lives in the webview's local storage. This module seals a
//! journal bundle (JSON handed over by the webview) into a passphrase-protected
//! file the member saves wherever they like, and opens such a file again.
//!
//! Crypto is REUSED, not invented:
//! - key derivation: the custody vault's own D-A2-1 Argon2id derivation
//!   ([`citrate_core_kit::custody::derive_passphrase_key`], m=64 MiB, t=3, p=1);
//! - AEAD: AES-256-GCM (the `aes-gcm` construction the vault seals slots with),
//!   fresh random 16-byte salt and 12-byte nonce per export.
//!
//! File format v1 (all integers big-endian), header = AEAD associated data:
//! ```text
//! 0   8  magic "CITJRNL\0"
//! 8   1  format version (1)
//! 9   1  kdf id (1 = Argon2id v0x13)
//! 10  4  m_cost KiB | 14 4 t_cost | 18 4 p_cost
//! 22  1  salt length (16) | 23 16 salt
//! 39  1  aead id (1 = AES-256-GCM)
//! 40  1  nonce length (12) | 41 12 nonce
//! 53  .. ciphertext || 16-byte tag
//! ```
//! A reader accepts only the parameters this build writes, so a crafted file can
//! never make the app spend more memory or time than an ordinary unlock.
//!
//! Plaintext never touches disk: it is sealed in memory and only ciphertext is
//! written. The passphrase and plaintext are held in zeroizing buffers here. No
//! key, signature or wallet material is involved (Rule 3).

use std::io::{Read, Write};
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use zeroize::Zeroizing;

/// File magic.
pub(crate) const MAGIC: &[u8; 8] = b"CITJRNL\0";
/// Current file format version.
pub(crate) const FORMAT_VERSION: u8 = 1;
/// KDF identifier: Argon2id, version 0x13.
pub(crate) const KDF_ARGON2ID_V13: u8 = 1;
/// AEAD identifier: AES-256-GCM.
pub(crate) const AEAD_AES256GCM: u8 = 1;
/// Salt length, bytes (the vault's salt length).
pub(crate) const SALT_LEN: usize = citrate_core_kit::custody::PASSPHRASE_SALT_LEN;
/// GCM nonce length, bytes.
pub(crate) const NONCE_LEN: usize = 12;
/// Header length, bytes.
pub(crate) const HEADER_LEN: usize = 8 + 1 + 1 + 12 + 1 + SALT_LEN + 1 + 1 + NONCE_LEN;
/// GCM tag length, bytes.
const TAG_LEN: usize = 16;
/// Largest file the importer will read (a journal is text; 32 MiB is generous).
pub(crate) const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Minimum export passphrase length, in characters.
pub(crate) const MIN_PASSPHRASE_CHARS: usize = 12;
/// The file extension an export path must carry.
pub(crate) const EXTENSION: &str = "citrate-journal";

/// Why a journal export or import failed. Messages are written for the member and
/// carry no internals (no algorithm names, no byte offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JournalCryptoError {
    /// Authentication failed: wrong passphrase, or the file was altered or damaged.
    WrongPassphraseOrDamaged,
    /// Not a journal export (bad magic or too short).
    NotAJournalFile,
    /// A journal export from a newer, unsupported format version.
    UnsupportedVersion,
    /// The header names key-derivation or encryption settings this app does not use.
    UnsupportedParameters,
    /// The export passphrase is shorter than [`MIN_PASSPHRASE_CHARS`].
    PassphraseTooShort,
    /// The path is not absolute, lacks the journal extension, or is a symlink.
    UnsafePath,
    /// The file is larger than [`MAX_FILE_BYTES`].
    TooLarge,
    /// Reading or writing the file failed.
    Io,
}

impl std::fmt::Display for JournalCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = match self {
            Self::WrongPassphraseOrDamaged => {
                "That passphrase does not open this file, or the file was changed or damaged."
            }
            Self::NotAJournalFile => "This file is not a Citrate journal export.",
            Self::UnsupportedVersion => {
                "This journal export was made by a newer version of the app."
            }
            Self::UnsupportedParameters => {
                "This journal export uses settings this app does not accept."
            }
            Self::PassphraseTooShort => "Use a passphrase of at least 12 characters.",
            Self::UnsafePath => "Choose a file name ending in .citrate-journal in a normal folder.",
            Self::TooLarge => "This file is too large to be a journal export.",
            Self::Io => "The file could not be read or written.",
        };
        f.write_str(m)
    }
}

type Result<T> = std::result::Result<T, JournalCryptoError>;

fn header(salt: &[u8; SALT_LEN], nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
    let (m, t, p) = citrate_core_kit::custody::PASSPHRASE_KDF_PARAMS;
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.push(FORMAT_VERSION);
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
    // The derivation can only fail on a malformed salt, which the header checks
    // rule out; map it to the generic authentication failure (no oracle).
    citrate_core_kit::custody::derive_passphrase_key(passphrase, salt)
        .map_err(|_| JournalCryptoError::WrongPassphraseOrDamaged)
}

/// Seal `plaintext` under `passphrase` into a complete v1 file image.
pub(crate) fn seal_journal(passphrase: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce);
    let head = header(&salt, &nonce);
    let key = derive_file_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &head,
            },
        )
        .map_err(|_| JournalCryptoError::Io)?;
    let mut out = head;
    out.extend_from_slice(&ct);
    Ok(out)
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    let arr: [u8; 4] = s.try_into().ok()?;
    Some(u32::from_be_bytes(arr))
}

/// Open a v1 file image. Every header field is checked against what this build
/// writes BEFORE any key derivation; the whole header is the AEAD's associated
/// data, so any change to it or to the ciphertext fails authentication.
pub(crate) fn open_journal(passphrase: &[u8], file: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if file.len() < 9 || &file[..8] != MAGIC {
        return Err(JournalCryptoError::NotAJournalFile);
    }
    if file[8] != FORMAT_VERSION {
        return Err(JournalCryptoError::UnsupportedVersion);
    }
    if file.len() < HEADER_LEN + TAG_LEN {
        return Err(JournalCryptoError::NotAJournalFile);
    }
    let params = (be_u32(file, 10), be_u32(file, 14), be_u32(file, 18));
    let (m, t, p) = citrate_core_kit::custody::PASSPHRASE_KDF_PARAMS;
    if file[9] != KDF_ARGON2ID_V13
        || params != (Some(m), Some(t), Some(p))
        || file[22] as usize != SALT_LEN
        || file[23 + SALT_LEN] != AEAD_AES256GCM
        || file[24 + SALT_LEN] as usize != NONCE_LEN
    {
        return Err(JournalCryptoError::UnsupportedParameters);
    }
    let head = &file[..HEADER_LEN];
    let salt = &file[23..23 + SALT_LEN];
    let nonce = &file[25 + SALT_LEN..HEADER_LEN];
    let key = derive_file_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let pt = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: &file[HEADER_LEN..],
                aad: head,
            },
        )
        .map_err(|_| JournalCryptoError::WrongPassphraseOrDamaged)?;
    Ok(Zeroizing::new(pt))
}

/// Export passphrase policy: at least [`MIN_PASSPHRASE_CHARS`] characters.
pub(crate) fn check_export_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(JournalCryptoError::PassphraseTooShort);
    }
    Ok(())
}

/// An export path must be absolute and end in `.citrate-journal`, so the command
/// can only ever create or replace journal export files.
pub(crate) fn check_export_path(path: &Path) -> Result<()> {
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e == EXTENSION)
        .unwrap_or(false);
    if !path.is_absolute() || !ext_ok {
        return Err(JournalCryptoError::UnsafePath);
    }
    Ok(())
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Open the export target for writing. On unix the open itself refuses a symlink
/// (`O_NOFOLLOW`, so a swap after the pre-check cannot redirect the write) and
/// does not block on a FIFO (`O_NONBLOCK`); anything that is not a regular file
/// is refused, and the mode is set to `0600` on the open handle so a replaced,
/// loosely-permissioned older export ends up owner-only too.
#[cfg(unix)]
pub(crate) fn open_export_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !f.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.set_len(0)?;
    Ok(f)
}

/// Non-unix fallback: the kit's secret-file opener (no POSIX mode or flags).
#[cfg(not(unix))]
pub(crate) fn open_export_file(path: &Path) -> std::io::Result<std::fs::File> {
    citrate_core_kit::fsutil::create_secret_file(path)
}

/// Open an import source for reading without blocking on a FIFO or device, and
/// refuse anything that is not a regular file.
fn open_import_file(path: &Path) -> Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NONBLOCK);
    }
    let f = opts.open(path).map_err(|_| JournalCryptoError::Io)?;
    let meta = f.metadata().map_err(|_| JournalCryptoError::Io)?;
    if !meta.is_file() {
        return Err(JournalCryptoError::NotAJournalFile);
    }
    Ok(f)
}

/// Seal `bundle` and write ONLY the ciphertext to `path` (owner-only mode on
/// unix). Returns the number of bytes written.
pub(crate) fn export_to_path(path: &Path, passphrase: &str, bundle: &[u8]) -> Result<u64> {
    check_export_passphrase(passphrase)?;
    check_export_path(path)?;
    if is_symlink(path) {
        return Err(JournalCryptoError::UnsafePath);
    }
    let sealed = seal_journal(passphrase.as_bytes(), bundle)?;
    let mut f = open_export_file(path).map_err(|_| JournalCryptoError::Io)?;
    f.write_all(&sealed).map_err(|_| JournalCryptoError::Io)?;
    f.sync_all().map_err(|_| JournalCryptoError::Io)?;
    Ok(sealed.len() as u64)
}

/// Read and open the export at `path`, returning the bundle text.
pub(crate) fn import_from_path(path: &Path, passphrase: &str) -> Result<Zeroizing<String>> {
    let f = open_import_file(path)?;
    let meta = f.metadata().map_err(|_| JournalCryptoError::Io)?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(JournalCryptoError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    f.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| JournalCryptoError::Io)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(JournalCryptoError::TooLarge);
    }
    let pt = open_journal(passphrase.as_bytes(), &bytes)?;
    let text = String::from_utf8(pt.to_vec()).map_err(|_| JournalCryptoError::NotAJournalFile)?;
    Ok(Zeroizing::new(text))
}

/// **journal_export_encrypted** — seal the journal bundle the webview hands over
/// and write only the ciphertext to the member-chosen `path`. Returns bytes written.
#[tauri::command]
pub async fn journal_export_encrypted(
    path: String,
    passphrase: String,
    bundle: String,
) -> std::result::Result<u64, String> {
    crate::blocking::off_main(move || {
        let passphrase = Zeroizing::new(passphrase);
        let bundle = Zeroizing::new(bundle);
        export_to_path(Path::new(&path), &passphrase, bundle.as_bytes()).map_err(|e| e.to_string())
    })
    .await
}

/// **journal_import_encrypted** — open a journal export with its passphrase and
/// return the bundle text to the webview (which validates and merges it).
#[tauri::command]
pub async fn journal_import_encrypted(
    path: String,
    passphrase: String,
) -> std::result::Result<String, String> {
    crate::blocking::off_main(move || {
        let passphrase = Zeroizing::new(passphrase);
        import_from_path(Path::new(&path), &passphrase)
            .map(|t| t.to_string())
            .map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("journal_export_tests.rs");
}
