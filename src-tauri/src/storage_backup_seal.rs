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

/// Parsed, checked header fields.
struct Header {
    raw: Vec<u8>,
    salt: [u8; SALT_LEN],
    nonce_prefix: [u8; NONCE_PREFIX_LEN],
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_be_bytes(s.try_into().ok()?))
}

/// Check every header field before any key derivation: an unknown version, KDF, KDF cost,
/// AEAD or chunk size is refused rather than guessed at.
fn parse_header(raw: &[u8]) -> Result<Header, BackupError> {
    if raw.len() < HEADER_LEN || &raw[..8] != MAGIC {
        return Err(not_a_backup());
    }
    if raw[8] != VERSION {
        return Err(BackupError::Manifest(format!(
            "this encrypted backup is format version {}, which this app does not read",
            raw[8]
        )));
    }
    let (m, t, p) = citrate_core_kit::custody::PASSPHRASE_KDF_PARAMS;
    let salt_at = 23;
    let aead_at = salt_at + SALT_LEN;
    let prefix_at = aead_at + 2;
    let chunk_at = prefix_at + NONCE_PREFIX_LEN;
    if raw[9] != KDF_ARGON2ID_V13
        || (be_u32(raw, 10), be_u32(raw, 14), be_u32(raw, 18)) != (Some(m), Some(t), Some(p))
        || raw[22] as usize != SALT_LEN
        || raw[aead_at] != AEAD_AES256GCM_STREAM_BE32
        || raw[aead_at + 1] as usize != NONCE_PREFIX_LEN
        || be_u32(raw, chunk_at) != Some(CHUNK_LEN as u32)
    {
        return Err(not_a_backup());
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&raw[salt_at..aead_at]);
    let mut nonce_prefix = [0u8; NONCE_PREFIX_LEN];
    nonce_prefix.copy_from_slice(&raw[prefix_at..chunk_at]);
    Ok(Header {
        raw: raw[..HEADER_LEN].to_vec(),
        salt,
        nonce_prefix,
    })
}

fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, BackupError> {
    citrate_core_kit::custody::derive_passphrase_key(passphrase, salt)
        .map_err(|_| wrong_passphrase())
}

// ---------------------------------------------------------------------------
// Chunked AEAD writer / reader
// ---------------------------------------------------------------------------

/// Buffers plaintext and writes full chunks; [`SealWriter::finish`] writes the last chunk.
pub(crate) struct SealWriter<W: Write> {
    out: W,
    enc: Option<EncryptorBE32<Aes256Gcm>>,
    aad: Vec<u8>,
    buf: Zeroizing<Vec<u8>>,
}

impl<W: Write> SealWriter<W> {
    /// Write the header to `out` and start the stream. `salt` and `nonce_prefix` must be fresh
    /// random values for every file (only tests pass fixed ones, for the known-answer vector).
    pub(crate) fn start(
        mut out: W,
        passphrase: &[u8],
        salt: [u8; SALT_LEN],
        nonce_prefix: [u8; NONCE_PREFIX_LEN],
    ) -> Result<Self, BackupError> {
        let head = header(&salt, &nonce_prefix);
        let key = derive_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
        let enc = EncryptorBE32::from_aead(cipher, nonce_prefix.as_slice().into());
        out.write_all(&head)
            .map_err(|e| io_err("write the backup", e))?;
        Ok(SealWriter {
            out,
            enc: Some(enc),
            aad: head,
            buf: Zeroizing::new(Vec::with_capacity(CHUNK_LEN)),
        })
    }

    /// Feed plaintext. A chunk is sealed only once more data is known to follow it, so the last
    /// chunk is always the one [`finish`](Self::finish) seals with the last-chunk flag.
    pub(crate) fn feed(&mut self, mut data: &[u8]) -> Result<(), BackupError> {
        while !data.is_empty() {
            if self.buf.len() == CHUNK_LEN {
                let enc = self
                    .enc
                    .as_mut()
                    .ok_or_else(|| io_err("seal", "finished"))?;
                let ct = enc
                    .encrypt_next(Payload {
                        msg: self.buf.as_slice(),
                        aad: &self.aad,
                    })
                    .map_err(|_| io_err("seal", "encryption failed"))?;
                self.out
                    .write_all(&ct)
                    .map_err(|e| io_err("write the backup", e))?;
                self.buf.clear();
            }
            let room = CHUNK_LEN - self.buf.len();
            let take = room.min(data.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        Ok(())
    }

    /// Seal the last chunk and return the writer.
    pub(crate) fn finish(mut self) -> Result<W, BackupError> {
        let enc = self.enc.take().ok_or_else(|| io_err("seal", "finished"))?;
        let ct = enc
            .encrypt_last(Payload {
                msg: self.buf.as_slice(),
                aad: &self.aad,
            })
            .map_err(|_| io_err("seal", "encryption failed"))?;
        self.out
            .write_all(&ct)
            .map_err(|e| io_err("write the backup", e))?;
        self.out
            .flush()
            .map_err(|e| io_err("write the backup", e))?;
        Ok(self.out)
    }
}

/// Reads the sealed body chunk by chunk and yields authenticated plaintext only.
pub(crate) struct OpenReader<R: Read> {
    src: R,
    dec: Option<DecryptorBE32<Aes256Gcm>>,
    aad: Vec<u8>,
    /// One sealed chunk read ahead, so the reader knows whether the current one is the last.
    ahead: Vec<u8>,
    plain: Zeroizing<Vec<u8>>,
    pos: usize,
    done: bool,
}

/// Read until `buf` is full or the source ends; returns the bytes read.
fn read_full<R: Read>(src: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match src.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

impl<R: Read> OpenReader<R> {
    /// Read and check the header from `src`, derive the key, and open the first chunk (so a
    /// wrong passphrase is reported here, before the caller writes anything).
    pub(crate) fn start(mut src: R, passphrase: &[u8]) -> Result<Self, BackupError> {
        let mut raw = vec![0u8; HEADER_LEN];
        let n = read_full(&mut src, &mut raw).map_err(|e| io_err("read the backup", e))?;
        if n < HEADER_LEN {
            return Err(not_a_backup());
        }
        let head = parse_header(&raw)?;
        let key = derive_key(passphrase, &head.salt)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
        let dec = DecryptorBE32::from_aead(cipher, head.nonce_prefix.as_slice().into());
        let mut r = OpenReader {
            src,
            dec: Some(dec),
            aad: head.raw,
            ahead: Vec::new(),
            plain: Zeroizing::new(Vec::new()),
            pos: 0,
            done: false,
        };
        r.ahead = r.read_sealed_chunk()?;
        if r.ahead.len() < TAG_LEN {
            return Err(wrong_passphrase());
        }
        r.next_chunk()?;
        Ok(r)
    }

    fn read_sealed_chunk(&mut self) -> Result<Vec<u8>, BackupError> {
        let mut c = vec![0u8; CHUNK_LEN + TAG_LEN];
        let n = read_full(&mut self.src, &mut c).map_err(|e| io_err("read the backup", e))?;
        c.truncate(n);
        Ok(c)
    }

    /// Decrypt the chunk read ahead. It is the last one exactly when nothing follows it.
    fn next_chunk(&mut self) -> Result<(), BackupError> {
        let current = std::mem::take(&mut self.ahead);
        let following = if current.len() == CHUNK_LEN + TAG_LEN {
            self.read_sealed_chunk()?
        } else {
            Vec::new()
        };
        let pt = if following.is_empty() {
            let dec = self.dec.take().ok_or_else(wrong_passphrase)?;
            self.done = true;
            dec.decrypt_last(Payload {
                msg: &current,
                aad: &self.aad,
            })
        } else {
            let dec = self.dec.as_mut().ok_or_else(wrong_passphrase)?;
            dec.decrypt_next(Payload {
                msg: &current,
                aad: &self.aad,
            })
        }
        .map_err(|_| wrong_passphrase())?;
        self.plain = Zeroizing::new(pt);
        self.pos = 0;
        self.ahead = following;
        Ok(())
    }

    /// Fill `buf` exactly from authenticated plaintext, or fail (a short stream is damage).
    pub(crate) fn take_exact(&mut self, buf: &mut [u8]) -> Result<(), BackupError> {
        let mut n = 0;
        while n < buf.len() {
            if self.pos == self.plain.len() {
                if self.done {
                    return Err(wrong_passphrase());
                }
                self.next_chunk()?;
                continue;
            }
            let k = (self.plain.len() - self.pos).min(buf.len() - n);
            buf[n..n + k].copy_from_slice(&self.plain[self.pos..self.pos + k]);
            self.pos += k;
            n += k;
        }
        Ok(())
    }

    /// True when every chunk has been opened and every plaintext byte consumed.
    pub(crate) fn at_end(&self) -> bool {
        self.done && self.pos == self.plain.len()
    }
}

// ---------------------------------------------------------------------------
// Folder <-> record stream
// ---------------------------------------------------------------------------

fn record_path(kind: u8, path: &str, out: &mut Vec<u8>) -> Result<(), BackupError> {
    if path.len() > MAX_PATH_LEN {
        return Err(io_err("seal", "a path in the backup is too long"));
    }
    out.push(kind);
    out.extend_from_slice(&(path.len() as u16).to_be_bytes());
    out.extend_from_slice(path.as_bytes());
    Ok(())
}

/// Seal the plain backup `folder` into a NEW owner-only file at `file`. The manifest goes first,
/// then every directory (parents first), then every file. A symlink or other odd entry refuses.
pub(crate) fn seal_folder(
    folder: &Path,
    file: &Path,
    passphrase: &[u8],
) -> Result<u64, BackupError> {
    let walked = walk(folder)?;
    if !walked.odd.is_empty() {
        return Err(io_err(
            "seal",
            "the staged backup holds an unexpected entry",
        ));
    }
    let mut salt = [0u8; SALT_LEN];
    let mut prefix = [0u8; NONCE_PREFIX_LEN];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut prefix);
    let out = create_private_file(file).map_err(|e| io_err("create the backup file", e))?;
    let w = SealWriter::start(std::io::BufWriter::new(out), passphrase, salt, prefix)?;
    let w = write_records(folder, &walked, w)?;
    let out = w.finish()?;
    let f = out
        .into_inner()
        .map_err(|e| io_err("write the backup", e.error()))?;
    f.sync_all().map_err(|e| io_err("write the backup", e))?;
    let len = f
        .metadata()
        .map_err(|e| io_err("write the backup", e))?
        .len();
    Ok(len)
}

fn write_records<W: Write>(
    folder: &Path,
    walked: &super::Walked,
    mut w: SealWriter<W>,
) -> Result<SealWriter<W>, BackupError> {
    let manifest = PathBuf::from(MANIFEST_NAME);
    let mut files: Vec<&PathBuf> = Vec::with_capacity(walked.files.len());
    if walked.files.contains(&manifest) {
        files.push(&manifest);
    } else {
        return Err(io_err("seal", "the staged backup has no manifest"));
    }
    files.extend(walked.files.iter().filter(|p| **p != manifest));
    let mut rec = Vec::new();
    for d in &walked.dirs {
        let s = slash_path(d).ok_or_else(|| io_err("seal", "a folder name is not UTF-8"))?;
        rec.clear();
        record_path(REC_DIR, &s, &mut rec)?;
        w.feed(&rec)?;
    }
    let mut chunk = Zeroizing::new(vec![0u8; 64 * 1024]);
    for f in files {
        let s = slash_path(f).ok_or_else(|| io_err("seal", "a file name is not UTF-8"))?;
        let mut src = std::fs::File::open(folder.join(f)).map_err(|e| io_err("open", e))?;
        let size = src.metadata().map_err(|e| io_err("open", e))?.len();
        rec.clear();
        record_path(REC_FILE, &s, &mut rec)?;
        rec.extend_from_slice(&size.to_be_bytes());
        w.feed(&rec)?;
        let mut left = size;
        while left > 0 {
            let want = (chunk.len() as u64).min(left) as usize;
            src.read_exact(&mut chunk[..want])
                .map_err(|e| io_err("read", e))?;
            w.feed(&chunk[..want])?;
            left -= want as u64;
        }
    }
    w.feed(&[REC_END])?;
    Ok(w)
}

/// Is `path` a file that starts with the encrypted-backup magic?
pub fn is_sealed_file(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    if !f.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return false;
    }
    let mut m = [0u8; 8];
    matches!(read_full(&mut f, &mut m), Ok(8)) && &m == MAGIC
}

/// A record path is the manifest itself or a path the manifest rule allows. For a directory
/// record, the bare `files` / `memory` roots are allowed too.
fn record_rel(path: &str, dir: bool) -> Option<PathBuf> {
    if path == MANIFEST_NAME && !dir {
        return Some(PathBuf::from(MANIFEST_NAME));
    }
    if dir && (path == super::FILES_DIR || path == super::MEMORY_DIR) {
        return Some(PathBuf::from(path));
    }
    safe_rel_path(path)
}

/// Open the sealed `file` into a NEW owner-only folder `into` (which must not exist). Fails
/// closed on a wrong passphrase, a damaged or truncated file, an unsafe or repeated path, or
/// anything after the end record; the caller removes `into` on error.
pub(crate) fn open_into(file: &Path, passphrase: &[u8], into: &Path) -> Result<(), BackupError> {
    let src = std::fs::File::open(file).map_err(|e| io_err("open the backup file", e))?;
    if !src
        .metadata()
        .map_err(|e| io_err("open the backup file", e))?
        .is_file()
    {
        return Err(not_a_backup());
    }
    let mut r = OpenReader::start(std::io::BufReader::new(src), passphrase)?;
    create_private_dir(into).map_err(|e| io_err("create the staging folder", e))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut first = true;
    let mut buf = Zeroizing::new(vec![0u8; 64 * 1024]);
    loop {
        let mut kind = [0u8; 1];
        r.take_exact(&mut kind)?;
        if kind[0] == REC_END {
            break;
        }
        if kind[0] != REC_DIR && kind[0] != REC_FILE {
            return Err(not_a_backup());
        }
        let mut len = [0u8; 2];
        r.take_exact(&mut len)?;
        let len = u16::from_be_bytes(len) as usize;
        if len == 0 || len > MAX_PATH_LEN {
            return Err(not_a_backup());
        }
        let mut p = vec![0u8; len];
        r.take_exact(&mut p)?;
        let path = String::from_utf8(p).map_err(|_| not_a_backup())?;
        let dir = kind[0] == REC_DIR;
        // The manifest is the first file record (directories may precede it).
        if !dir && first && path != MANIFEST_NAME {
            return Err(not_a_backup());
        }
        let rel = record_rel(&path, dir).ok_or_else(|| {
            BackupError::Manifest(format!("the backup names an unsafe path: {path:?}"))
        })?;
        if !seen.insert(path.clone()) {
            return Err(BackupError::Manifest(format!(
                "the backup lists {path:?} twice"
            )));
        }
        let target = into.join(&rel);
        if dir {
            create_private_dir_all(&target)?;
            continue;
        }
        first = false;
        let mut size = [0u8; 8];
        r.take_exact(&mut size)?;
        let mut left = u64::from_be_bytes(size);
        if let Some(parent) = target.parent() {
            create_private_dir_all(parent)?;
        }
        let mut out = create_private_file(&target).map_err(|e| io_err("create", e))?;
        while left > 0 {
            let want = (buf.len() as u64).min(left) as usize;
            r.take_exact(&mut buf[..want])?;
            out.write_all(&buf[..want])
                .map_err(|e| io_err("write", e))?;
            left -= want as u64;
        }
        out.sync_all().map_err(|e| io_err("write", e))?;
    }
    if first {
        return Err(not_a_backup());
    }
    if !r.at_end() {
        return Err(wrong_passphrase());
    }
    Ok(())
}

/// A new, unused staging folder path under `staging_root` (created owner-only).
pub(crate) fn staging_dir(staging_root: &Path, tag: &str) -> Result<PathBuf, BackupError> {
    create_private_dir_all(staging_root)?;
    let mut rnd = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut rnd);
    Ok(staging_root.join(format!("{tag}-{}", hex::encode(rnd))))
}

/// Removes a staging folder when dropped (success or error).
pub(crate) struct StagingGuard(pub PathBuf);

impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
