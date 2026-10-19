//! K4 — verified export, backup and restore of the member's storage: Files (the objects this node
//! keeps in its local IPFS store) and Memory (the mem-mcp graph store).
//!
//! ## What a backup is
//! The member picks a folder. Export creates a NEW subfolder in it,
//! `citrate-backup-YYYYMMDD-HHMMSS` (UTC; `-2`, `-3`, … if that name exists), and never writes
//! anywhere else:
//!
//! ```text
//! citrate-backup-20261019-120000/
//!   citrate-backup.json            the manifest
//!   files/<cid>                    one file per object pinned on this node (bytes from kubo)
//!   memory/<relative path>         the memory store dir, copied with the daemon stopped
//! ```
//!
//! The manifest lists every file with its size, SHA-256 and Unix mode (and, for Files, the CID),
//! sorted by path; the memory store's directories with their modes (`dirs`); the parts it holds
//! (`parts`, never a part that was skipped) and the parts the export skipped with the reason
//! (`skippedParts`); and `entriesSha256`, a plain SHA-256 over all of that. The digest catches a
//! damaged manifest or an edit that did not recompute it; it is not a signature, so a deliberate
//! edit that recomputes it (with matching files) is not detected. Modes read from the manifest
//! are therefore masked on restore (never group- or world-writable). A requested part with nothing to back up (no memory store, an empty one, no pinned
//! files) is skipped, never written as an empty part.
//!
//! ## Verification and refusal
//! [`verify_folder`] checks the manifest format and digest, that every listed path is a safe
//! relative path under `files/` or `memory/`, that every listed file exists as a regular file
//! (never a symlink) with the listed size and SHA-256, and that the folder holds nothing the
//! manifest does not list. Any problem is a [`BackupError::Mismatch`] naming each file.
//! [`restore`] runs that check FIRST and refuses on any mismatch before it stops a daemon or
//! touches a store. Export ends by running the same check on what it wrote.
//!
//! ## Memory
//! The mem-mcp daemon holds the store's RocksDB lock, so a consistent copy needs it stopped.
//! Export and restore stop a running daemon, do the copy, and start it again; a stopped daemon
//! stays stopped. From before the stop until the copy or swap is done they hold the memory store
//! lock ([`crate::memory::MemoryStoreLock`]): `MemoryManager::start` refuses while it is held, so
//! no start path (the Start control, a seed, a knowledge-import restart) opens the store
//! mid-restore. Restore re-applies each file's and directory's recorded mode and checks it took.
//! The restore report lists only the parts it restored and names each skipped part and why. They hold the knowledge importer's store slot ([`ImportGuard`]) for the whole
//! operation, so no import writes the store meanwhile. Restore copies into a staging dir beside
//! the store, re-checks every hash there, then swaps it in. The replaced store is KEPT beside it
//! as `memory.before-restore-<time>`, never deleted. The live socket (`memdag.sock`) and the
//! crash log are not member data and are never copied.
//!
//! A backup is NOT encrypted. The memory store is not encrypted at rest by core (see `memory.rs`),
//! and the Memory part is a plain copy of the store as it is on disk; Files are plain copies too.
//! So the backup folder is created owner-only (0700 on Unix) and the surface tells
//! the member to keep it somewhere private. Every dir and file this module creates (the backup
//! folder, its contents, and the restore staging dir and everything in it) is owner-only from
//! creation (0700 dirs via `DirBuilder` mode, 0600 files via `OpenOptions` mode), never created at
//! the umask default and narrowed afterwards. A restored file then gets its recorded mode.
//!
//! App quit: a running daemon is started again only if app quit has not begun (the import
//! registry's gate, which `shutdown_all_sidecars` closes before it stops memory).
//!
//! ## Files
//! Only objects PINNED on this node are exported (an index row the daemon no longer holds is
//! listed in the report as skipped, with the reason). Restore re-adds each verified file to the
//! local kubo store and pins it locally with [`crate::storage::LOCAL_PIN_MARKER`]: no bond, no transaction. If
//! kubo answers a different CID (a different CID version), the report lists it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::knowledge_import::ImportGuard;
use crate::storage::{PinState, StorageManager};

/// The manifest file name at the root of a backup folder.
pub const MANIFEST_NAME: &str = "citrate-backup.json";
/// The manifest format this build writes and accepts.
pub const FORMAT: &str = "citrate-core.storage-backup.v1";
/// The Tauri event carrying [`Progress`] while an export or restore runs.
pub const PROGRESS_EVENT: &str = "storage://backup-progress";

const FILES_DIR: &str = "files";
const MEMORY_DIR: &str = "memory";
/// Memory-dir entries that are never backed up: the live socket and the crash log.
const MEMORY_EXCLUDE: &[&str] = &["memdag.sock", "crash-records.jsonl"];
/// The largest manifest read (a manifest is a few hundred bytes per file).
const MAX_MANIFEST_BYTES: u64 = 64 << 20;
/// How many problems the error text names before it summarizes the rest.
const MAX_NAMED_MISMATCHES: usize = 8;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A part of the member's storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    Files,
    Memory,
}

impl Part {
    /// Parse the renderer's part names; anything else is refused.
    pub fn parse_list(names: &[String]) -> Result<Vec<Part>, BackupError> {
        let mut out = Vec::new();
        for n in names {
            let p = match n.as_str() {
                "files" => Part::Files,
                "memory" => Part::Memory,
                other => {
                    return Err(BackupError::InvalidFolder(format!(
                        "unknown storage part {other:?}"
                    )))
                }
            };
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out.sort();
        Ok(out)
    }
}

/// One file in a backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// `/`-separated path relative to the backup folder (`files/<cid>` or `memory/<…>`).
    pub path: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file's bytes.
    pub sha256: String,
    /// The CID, for `files/` entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    /// Unix permission bits (`& 0o777`) of the original file, re-applied on restore. `None` on
    /// platforms without Unix modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

/// A directory of the memory store and its Unix permission bits (`memory` is the store root).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestDir {
    pub path: String,
    pub mode: u32,
}

/// A part the export was asked for but did not write, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedPart {
    pub part: Part,
    pub reason: String,
}

/// The backup manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    /// Unix seconds when the export ran.
    pub created_at: u64,
    pub app_version: String,
    /// The parts this backup actually holds (never a part that was skipped).
    pub parts: Vec<Part>,
    /// Sorted by `path`.
    pub entries: Vec<ManifestEntry>,
    /// [`manifest_digest`] over `entries`, `dirs` and `skipped_parts`.
    pub entries_sha256: String,
    /// The memory store's directories with their modes, sorted by `path` (parents first).
    #[serde(default)]
    pub dirs: Vec<ManifestDir>,
    /// Parts the export was asked for but did not write.
    #[serde(default)]
    pub skipped_parts: Vec<SkippedPart>,
}

/// One way a backup folder disagrees with its manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Mismatch {
    /// Listed, but not in the folder.
    Missing { path: String },
    /// Listed with a different size.
    #[serde(rename_all = "camelCase")]
    SizeDiffers {
        path: String,
        expected: u64,
        actual: u64,
    },
    /// Same size, different content.
    HashDiffers { path: String },
    /// In the folder, not in the manifest.
    Unlisted { path: String },
    /// A manifest path that is absolute, climbs out, is outside `files/`/`memory/`, or repeats.
    UnsafePath { path: String },
    /// A symlink or other non-regular file where a file is listed.
    NotAFile { path: String },
    /// A manifest mode outside the permission bits (0o777): setuid, setgid, sticky or garbage.
    BadMode { path: String },
    /// A restored file or dir did not end up with the manifest's mode.
    #[serde(rename_all = "camelCase")]
    ModeDiffers {
        path: String,
        expected: u32,
        actual: u32,
    },
}

impl Mismatch {
    fn describe(&self) -> String {
        match self {
            Mismatch::Missing { path } => format!("{path}: missing"),
            Mismatch::SizeDiffers {
                path,
                expected,
                actual,
            } => format!("{path}: {actual} bytes, manifest says {expected}"),
            Mismatch::HashDiffers { path } => format!("{path}: content differs"),
            Mismatch::Unlisted { path } => format!("{path}: not in the manifest"),
            Mismatch::UnsafePath { path } => format!("{path:?}: not an allowed backup path"),
            Mismatch::NotAFile { path } => format!("{path}: not a regular file"),
            Mismatch::BadMode { path } => format!("{path}: not an allowed file mode"),
            Mismatch::ModeDiffers {
                path,
                expected,
                actual,
            } => format!("{path}: mode {actual:o}, manifest says {expected:o}"),
        }
    }
}

/// A backup/restore error. Every `Display` is safe to show the member.
#[derive(Debug)]
pub enum BackupError {
    /// The chosen folder or the requested parts are not usable.
    InvalidFolder(String),
    /// The manifest is missing, unreadable, of another format, or edited.
    Manifest(String),
    /// The folder does not match its manifest (restore refuses).
    Mismatch(Vec<Mismatch>),
    /// The memory store is busy (a knowledge import is writing it).
    Busy(String),
    /// A filesystem error.
    Io(String),
    /// The local IPFS (kubo) store failed.
    Kubo(String),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupError::InvalidFolder(m) => write!(f, "{m}"),
            BackupError::Manifest(m) => write!(f, "the backup manifest can't be used: {m}"),
            BackupError::Mismatch(list) => {
                let named: Vec<String> = list
                    .iter()
                    .take(MAX_NAMED_MISMATCHES)
                    .map(Mismatch::describe)
                    .collect();
                let more = list.len().saturating_sub(MAX_NAMED_MISMATCHES);
                write!(
                    f,
                    "the backup does not match its manifest ({} problem{}): {}{}",
                    list.len(),
                    if list.len() == 1 { "" } else { "s" },
                    named.join("; "),
                    if more > 0 {
                        format!("; and {more} more")
                    } else {
                        String::new()
                    }
                )
            }
            BackupError::Busy(m) => write!(f, "{m}"),
            BackupError::Io(m) => write!(f, "storage backup file error: {m}"),
            BackupError::Kubo(m) => write!(f, "the local file store failed: {m}"),
        }
    }
}
impl std::error::Error for BackupError {}

fn io_err(context: &str, e: impl std::fmt::Display) -> BackupError {
    BackupError::Io(format!("{context}: {e}"))
}

/// One progress step, emitted as [`PROGRESS_EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// "export" | "restore".
    pub op: &'static str,
    /// "files" | "memory" | "verify" | "daemon" | "done".
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    /// The file (backup-relative path) or step just handled.
    pub item: String,
}

/// An item export left out, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub item: String,
    pub reason: String,
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    /// The new backup folder.
    pub folder: String,
    pub created_at: u64,
    /// The parts actually written (a requested part that was not written is in `skipped`).
    pub parts: Vec<Part>,
    /// Files (IPFS objects) written.
    pub files: u64,
    /// Memory store files written.
    pub memory_files: u64,
    pub total_bytes: u64,
    pub entries_sha256: String,
    pub skipped: Vec<Skipped>,
    /// The memory daemon was running, stopped for the copy, and started again.
    pub daemon_restarted: bool,
    /// Anything the member should know that did not stop the export.
    pub notes: Vec<String>,
}

/// A Files object that came back under a different CID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CidChange {
    pub expected: String,
    pub actual: String,
}

/// What a restore did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub folder: String,
    /// The parts actually restored (a requested part that was not restored is in `skipped`).
    pub parts: Vec<Part>,
    /// Requested parts that were not restored, and why.
    pub skipped: Vec<Skipped>,
    pub files_restored: u64,
    pub cids_changed: Vec<CidChange>,
    pub memory_files_restored: u64,
    /// Where the replaced memory store was kept (None when there was none).
    pub previous_memory_kept_at: Option<String>,
    pub daemon_restarted: bool,
    pub notes: Vec<String>,
}

/// The verify-only answer for the surface (never an `Err` for a mismatch: `ok` is false and the
/// problems are listed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub folder: String,
    pub ok: bool,
    pub created_at: Option<u64>,
    pub parts: Vec<Part>,
    pub file_count: u64,
    pub total_bytes: u64,
    pub mismatches: Vec<Mismatch>,
    /// A problem with the manifest itself (unreadable, other format, edited).
    pub problem: Option<String>,
}

/// Stop/start control over the memory daemon (production: [`crate::memory::MemoryManager`]).
pub trait DaemonControl {
    fn is_running(&self) -> bool;
    fn stop(&self);
    fn start(&self) -> Result<(), String>;
}

impl DaemonControl for crate::memory::MemoryManager {
    fn is_running(&self) -> bool {
        self.is_running_now()
    }
    fn stop(&self) {
        crate::memory::MemoryManager::stop(self)
    }
    fn start(&self) -> Result<(), String> {
        crate::memory::MemoryManager::start(self).map_err(|e| e.to_string())
    }
}

/// The memory store to back up or restore into.
pub struct MemorySource<'a> {
    /// The memory dir (`<app data>/memory`): everything under it except [`MEMORY_EXCLUDE`].
    pub dir: PathBuf,
    /// The RocksDB store dir inside it (the importer's lock key).
    pub store_path: PathBuf,
    pub daemon: &'a dyn DaemonControl,
    /// The knowledge-import registry, closed by app quit (`shutdown_all_sidecars`). Production
    /// passes the app-wide one; a closed registry means the app is quitting.
    pub registry: std::sync::Arc<crate::knowledge_import::ImportRegistry>,
}

// ---------------------------------------------------------------------------
// Hashing and paths
// ---------------------------------------------------------------------------

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The digest over a manifest's entries only (a manifest with no dirs and no skipped parts).
/// Tests build hand-made manifests with it.
#[cfg(test)]
pub fn entries_digest(entries: &[ManifestEntry]) -> String {
    digest_records(entries, &[], &[])
}

/// The digest stored as `entriesSha256`: every entry, dir and skipped part.
pub fn manifest_digest(m: &Manifest) -> String {
    digest_records(&m.entries, &m.dirs, &m.skipped_parts)
}

/// One `path\nsize\nsha256\ncid\nmode\n` record per entry, then `dir\npath\nmode\n` per dir and
/// `skip\npart\nreason\n` per skipped part.
fn digest_records(
    entries: &[ManifestEntry],
    dirs: &[ManifestDir],
    skipped: &[SkippedPart],
) -> String {
    let mut h = Sha256::new();
    for e in entries {
        h.update(e.path.as_bytes());
        h.update(b"\n");
        h.update(e.size.to_string().as_bytes());
        h.update(b"\n");
        h.update(e.sha256.as_bytes());
        h.update(b"\n");
        h.update(e.cid.as_deref().unwrap_or("").as_bytes());
        h.update(b"\n");
        h.update(e.mode.map(|m| m.to_string()).unwrap_or_default().as_bytes());
        h.update(b"\n");
    }
    for d in dirs {
        h.update(b"dir\n");
        h.update(d.path.as_bytes());
        h.update(b"\n");
        h.update(d.mode.to_string().as_bytes());
        h.update(b"\n");
    }
    for sp in skipped {
        h.update(b"skip\n");
        h.update(part_name(sp.part).as_bytes());
        h.update(b"\n");
        h.update(sp.reason.as_bytes());
        h.update(b"\n");
    }
    hex::encode(h.finalize())
}

fn part_name(p: Part) -> &'static str {
    match p {
        Part::Files => "files",
        Part::Memory => "memory",
    }
}

fn part_label(p: Part) -> &'static str {
    match p {
        Part::Files => "Files",
        Part::Memory => "Memory",
    }
}

/// The Unix permission bits (`& 0o777`) of `path` (not following symlinks).
#[cfg(unix)]
fn mode_of(path: &Path) -> Result<Option<u32>, BackupError> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::symlink_metadata(path).map_err(|e| io_err("read file mode", e))?;
    Ok(Some(meta.permissions().mode() & 0o777))
}

#[cfg(not(unix))]
fn mode_of(_path: &Path) -> Result<Option<u32>, BackupError> {
    Ok(None)
}

/// Set the Unix permission bits of `path` (a no-op without Unix modes).
#[cfg(unix)]
fn apply_mode(path: &Path, mode: u32) -> Result<(), BackupError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o777))
        .map_err(|e| io_err("set file mode", e))
}

#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: u32) -> Result<(), BackupError> {
    Ok(())
}

/// Apply a mode RECORDED IN A MANIFEST to a restored file or dir, and check it took (a filesystem
/// that drops modes is a mismatch, not a silent widening). The manifest is not signed, so the
/// recorded mode is masked: permission bits only, and never group- or world-writable. This is the
/// only way the restore path sets a mode (`restore_path_applies_modes_only_through_the_mask`).
fn apply_restored_mode(path: &Path, rel: &str, recorded: u32) -> Result<(), BackupError> {
    let mode = recorded & 0o777 & !0o022;
    apply_mode(path, mode)?;
    if let Some(actual) = mode_of(path)? {
        if actual != mode {
            return Err(BackupError::Mismatch(vec![Mismatch::ModeDiffers {
                path: rel.to_string(),
                expected: mode,
                actual,
            }]));
        }
    }
    Ok(())
}

/// Open a NEW file (never overwrites) that is owner-only (0600) from the moment it exists.
fn create_private_file(to: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(to)
}

/// Create `dir` (and missing parents) owner-only (0700) at creation, never at the umask default.
fn create_private_dir_all(dir: &Path) -> Result<(), BackupError> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir).map_err(|e| io_err("create a folder", e))
}

/// Create one NEW owner-only (0700) dir; fails if it exists.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
}

/// Copy `from` to `to` (created new and owner-only; never overwrites), hashing the bytes on the
/// way. A recorded original mode is applied after the bytes are in.
fn copy_hashing(from: &Path, to: &Path) -> Result<(u64, String), BackupError> {
    let mut src = std::fs::File::open(from).map_err(|e| io_err("open", e))?;
    let mut dst = create_private_file(to).map_err(|e| io_err("create", e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut n_total: u64 = 0;
    loop {
        let n = src.read(&mut buf).map_err(|e| io_err("read", e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        dst.write_all(&buf[..n]).map_err(|e| io_err("write", e))?;
        n_total += n as u64;
    }
    dst.sync_all().map_err(|e| io_err("sync", e))?;
    Ok((n_total, hex::encode(h.finalize())))
}

/// SHA-256 and size of a file, streamed.
fn hash_file(path: &Path) -> Result<(u64, String), BackupError> {
    let mut f = std::fs::File::open(path).map_err(|e| io_err("open", e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut n_total: u64 = 0;
    loop {
        let n = f.read(&mut buf).map_err(|e| io_err("read", e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        n_total += n as u64;
    }
    Ok((n_total, hex::encode(h.finalize())))
}

/// A bare CID: non-empty, bounded, ASCII alphanumeric (the same rule `storage.rs` applies before
/// a CID reaches kubo or the filesystem).
fn is_bare_cid(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Turn a manifest path into a relative path under the backup folder, or `None` when it is not
/// allowed: empty, absolute, `.`/`..`, empty segments, backslashes, NUL, outside `files/` or
/// `memory/`, or a `files/` path that is not exactly `files/<cid>`.
fn safe_rel_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains('\\') || path.contains('\0') || path.starts_with('/') {
        return None;
    }
    let segs: Vec<&str> = path.split('/').collect();
    if segs.len() < 2 || segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return None;
    }
    match segs[0] {
        FILES_DIR if segs.len() == 2 && is_bare_cid(segs[1]) => {}
        MEMORY_DIR => {}
        _ => return None,
    }
    let rel: PathBuf = segs.iter().collect();
    rel.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(rel)
}

/// The `/`-separated backup path of `rel` (relative to some root).
fn slash_path(rel: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_str()?.to_string()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// What [`walk`] found under a root, as paths relative to it.
struct Walked {
    /// Regular files.
    files: Vec<PathBuf>,
    /// Symlinks and other non-regular entries (never followed).
    odd: Vec<PathBuf>,
    /// Subdirectories (not the root itself), sorted so parents come first.
    dirs: Vec<PathBuf>,
}

/// Every entry under `root` (recursively), never following symlinks.
fn walk(root: &Path) -> Result<Walked, BackupError> {
    let mut files = Vec::new();
    let mut odd = Vec::new();
    let mut dirs = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let dir = root.join(&rel);
        let rd = std::fs::read_dir(&dir).map_err(|e| io_err("list folder", e))?;
        for entry in rd {
            let entry = entry.map_err(|e| io_err("list folder", e))?;
            let child = rel.join(entry.file_name());
            let ft = entry.file_type().map_err(|e| io_err("file type", e))?;
            if ft.is_symlink() {
                odd.push(child);
            } else if ft.is_dir() {
                dirs.push(child.clone());
                stack.push(child);
            } else if ft.is_file() {
                files.push(child);
            } else {
                odd.push(child);
            }
        }
    }
    files.sort();
    odd.sort();
    dirs.sort();
    Ok(Walked { files, odd, dirs })
}

// ---------------------------------------------------------------------------
// Folder naming
// ---------------------------------------------------------------------------

/// `(year, month, day)` of a count of days since 1970-01-01 (proleptic Gregorian, UTC).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days_from_civil inverse.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `citrate-backup-YYYYMMDD-HHMMSS` for unix seconds `now` (UTC).
pub fn backup_folder_name(now: u64) -> String {
    let secs = now as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "citrate-backup-{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        t / 3600,
        (t / 60) % 60,
        t % 60
    )
}

/// Create a new, empty, owner-only backup folder in `dest`. Never reuses an existing one.
fn create_backup_folder(dest: &Path, now: u64) -> Result<PathBuf, BackupError> {
    let base = backup_folder_name(now);
    for n in 1..=999u32 {
        let name = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        let p = dest.join(&name);
        match create_private_dir(&p) {
            Ok(()) => {
                restrict_to_owner(&p)?;
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_err("create the backup folder", e)),
        }
    }
    Err(BackupError::InvalidFolder(
        "too many backups with the same name in that folder".to_string(),
    ))
}

#[cfg(unix)]
fn restrict_to_owner(p: &Path) -> Result<(), BackupError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| io_err("make the backup folder private", e))
}

#[cfg(not(unix))]
fn restrict_to_owner(_p: &Path) -> Result<(), BackupError> {
    // Windows: the folder inherits the ACL of the member-chosen parent (documented in
    // docs/STORAGE_K4.md).
    Ok(())
}

/// The chosen destination must be an existing absolute directory, and not inside the memory dir
/// (the export would copy into what it reads).
fn check_destination(
    dest: &Path,
    memory: Option<&MemorySource<'_>>,
) -> Result<PathBuf, BackupError> {
    if !dest.is_absolute() {
        return Err(BackupError::InvalidFolder(
            "choose a folder (an absolute path)".to_string(),
        ));
    }
    let canon = std::fs::canonicalize(dest).map_err(|_| {
        BackupError::InvalidFolder("that folder doesn't exist or can't be opened".to_string())
    })?;
    if !canon.is_dir() {
        return Err(BackupError::InvalidFolder(
            "that path is not a folder".to_string(),
        ));
    }
    if let Some(m) = memory {
        if let Ok(mem) = std::fs::canonicalize(&m.dir) {
            if canon.starts_with(&mem) {
                return Err(BackupError::InvalidFolder(
                    "choose a folder outside the memory store".to_string(),
                ));
            }
        }
    }
    Ok(canon)
}

// ---------------------------------------------------------------------------
// Memory daemon pause
// ---------------------------------------------------------------------------

/// Run `f` with the memory store quiet: the importer slot held and a running daemon stopped, then
/// started again (always, also when `f` fails). Returns `f`'s result and whether a restart ran;
/// a failed restart is added to `notes`.
fn with_memory_paused<T>(
    m: &MemorySource<'_>,
    notes: &mut Vec<String>,
    on_progress: &mut dyn FnMut(&Progress),
    op: &'static str,
    f: impl FnOnce(&mut dyn FnMut(&Progress)) -> Result<T, BackupError>,
) -> Result<(T, bool), BackupError> {
    let Some(_slot) = ImportGuard::acquire(&m.store_path) else {
        return Err(BackupError::Busy(
            "the memory store is busy (a knowledge import is writing it); try again when it finishes"
                .to_string(),
        ));
    };
    if m.registry.is_closed() {
        return Err(BackupError::Busy("the app is quitting".to_string()));
    }
    // Held from before the daemon stops until the copy/swap is done: every memory start path
    // (`MemoryManager::start`) refuses while it is held, so nothing opens the store mid-restore.
    let Some(store_lock) = crate::memory::MemoryStoreLock::acquire(&m.store_path) else {
        return Err(BackupError::Busy(
            "another memory backup or restore is running; try again when it finishes".to_string(),
        ));
    };
    let was_running = m.daemon.is_running();
    if was_running {
        on_progress(&Progress {
            op,
            phase: "daemon",
            done: 0,
            total: 0,
            item: "stopping the memory daemon".to_string(),
        });
        m.daemon.stop();
    }
    let out = f(on_progress);
    // Release the store before our own restart (which would otherwise be refused).
    drop(store_lock);
    let mut restarted = false;
    if was_running {
        // Never restart once app quit began: shutdown closes the registry before it stops the
        // memory daemon, and `unless_closed` holds the gate, so a quit either waits for this
        // start (and then stops it) or has already closed and this start does not run.
        match m.registry.unless_closed(|| m.daemon.start()) {
            Some(Ok(())) => restarted = true,
            Some(Err(e)) => notes.push(format!("the memory daemon did not start again: {e}")),
            None => notes.push(
                "the app is quitting, so the memory daemon was not started again".to_string(),
            ),
        }
    }
    out.map(|v| (v, restarted))
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Export `parts` into a new backup folder inside `dest`. `files` / `memory` are the sources (a
/// requested part without its source is refused). `now` is unix seconds (the folder name and
/// `createdAt`). Ends by verifying the written folder; a failed export removes its own folder.
pub fn export(
    dest: &Path,
    parts: &[Part],
    files: Option<&StorageManager>,
    memory: Option<&MemorySource<'_>>,
    now: u64,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<ExportReport, BackupError> {
    if parts.is_empty() {
        return Err(BackupError::InvalidFolder(
            "choose Files, Memory, or both".to_string(),
        ));
    }
    let want_files = parts.contains(&Part::Files);
    let want_memory = parts.contains(&Part::Memory);
    if want_files && files.is_none() {
        return Err(BackupError::InvalidFolder(
            "the file store is not available".to_string(),
        ));
    }
    if want_memory && memory.is_none() {
        return Err(BackupError::InvalidFolder(
            "the memory store is not available".to_string(),
        ));
    }
    let dest = check_destination(dest, memory)?;
    let folder = create_backup_folder(&dest, now)?;
    let result = export_into(&folder, parts, files, memory, now, on_progress);
    if result.is_err() {
        // Only ever our own, freshly created folder.
        let _ = std::fs::remove_dir_all(&folder);
    }
    result
}

fn export_into(
    folder: &Path,
    parts: &[Part],
    files: Option<&StorageManager>,
    memory: Option<&MemorySource<'_>>,
    now: u64,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<ExportReport, BackupError> {
    let mut entries: Vec<ManifestEntry> = Vec::new();
    let mut skipped = Vec::new();
    let mut notes = Vec::new();
    let mut file_count = 0u64;
    let mut memory_count = 0u64;
    let mut daemon_restarted = false;
    let mut dirs: Vec<ManifestDir> = Vec::new();
    let mut written: Vec<Part> = Vec::new();
    let mut skipped_parts: Vec<SkippedPart> = Vec::new();

    if let (true, Some(mgr)) = (parts.contains(&Part::Files), files) {
        let rows = mgr.list().map_err(|e| BackupError::Kubo(e.to_string()))?;
        let files_dir = folder.join(FILES_DIR);
        create_private_dir(&files_dir).map_err(|e| io_err("create files/", e))?;
        let (pinned, unpinned): (Vec<_>, Vec<_>) = rows
            .into_iter()
            .partition(|r| r.pin_state == PinState::Pinned);
        for r in unpinned {
            skipped.push(Skipped {
                item: r.cid,
                reason: "not pinned on this node".to_string(),
            });
        }
        let total = pinned.len() as u64;
        for (i, r) in pinned.into_iter().enumerate() {
            let done = i as u64 + 1;
            if !is_bare_cid(&r.cid) {
                skipped.push(Skipped {
                    item: r.cid,
                    reason: "not a plain CID".to_string(),
                });
                continue;
            }
            let bytes = match mgr.cat(&r.cid) {
                Ok(b) => b,
                Err(e) => {
                    skipped.push(Skipped {
                        item: r.cid,
                        reason: e.to_string(),
                    });
                    continue;
                }
            };
            let rel = format!("{FILES_DIR}/{}", r.cid);
            let mut out = create_private_file(&files_dir.join(&r.cid))
                .map_err(|e| io_err("write a file", e))?;
            out.write_all(&bytes)
                .map_err(|e| io_err("write a file", e))?;
            out.sync_all().map_err(|e| io_err("write a file", e))?;
            drop(out);
            // Owner-only, recorded so a restore can check it.
            let file_path = files_dir.join(&r.cid);
            apply_mode(&file_path, 0o600)?;
            entries.push(ManifestEntry {
                path: rel.clone(),
                size: bytes.len() as u64,
                sha256: sha256_hex(&bytes),
                cid: Some(r.cid),
                mode: mode_of(&file_path)?,
            });
            file_count += 1;
            on_progress(&Progress {
                op: "export",
                phase: "files",
                done,
                total,
                item: rel,
            });
        }
        if file_count > 0 {
            written.push(Part::Files);
        } else {
            let _ = std::fs::remove_dir(&files_dir);
            skipped_parts.push(SkippedPart {
                part: Part::Files,
                reason: "no files are pinned on this node".to_string(),
            });
        }
    }

    if let (true, Some(m)) = (parts.contains(&Part::Memory), memory) {
        if !m.dir.is_dir() {
            skipped_parts.push(SkippedPart {
                part: Part::Memory,
                reason: "there is no memory store on this machine yet".to_string(),
            });
        } else {
            let mem_out = folder.join(MEMORY_DIR);
            let (copied, restarted) =
                with_memory_paused(m, &mut notes, on_progress, "export", |on| {
                    copy_memory_out(&m.dir, &mem_out, on)
                })?;
            daemon_restarted = restarted;
            if copied.entries.is_empty() {
                let _ = std::fs::remove_dir_all(&mem_out);
                skipped_parts.push(SkippedPart {
                    part: Part::Memory,
                    reason: "the memory store is empty".to_string(),
                });
            } else {
                memory_count = copied.entries.len() as u64;
                entries.extend(copied.entries);
                dirs = copied.dirs;
                written.push(Part::Memory);
            }
        }
    }

    for sp in &skipped_parts {
        skipped.push(Skipped {
            item: part_name(sp.part).to_string(),
            reason: sp.reason.clone(),
        });
    }
    if written.is_empty() {
        let why: Vec<String> = skipped_parts
            .iter()
            .map(|sp| format!("{}: {}", part_label(sp.part), sp.reason))
            .collect();
        return Err(BackupError::InvalidFolder(format!(
            "nothing to back up ({})",
            why.join("; ")
        )));
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut manifest = Manifest {
        format: FORMAT.to_string(),
        created_at: now,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        parts: written.clone(),
        entries_sha256: String::new(),
        entries,
        dirs,
        skipped_parts,
    };
    manifest.entries_sha256 = manifest_digest(&manifest);
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| io_err("write the manifest", e))?;
    let mut mf = create_private_file(&folder.join(MANIFEST_NAME))
        .map_err(|e| io_err("write the manifest", e))?;
    mf.write_all(&json)
        .map_err(|e| io_err("write the manifest", e))?;
    mf.sync_all().map_err(|e| io_err("write the manifest", e))?;

    // Prove what was written reads back exactly.
    on_progress(&Progress {
        op: "export",
        phase: "verify",
        done: 0,
        total: manifest.entries.len() as u64,
        item: MANIFEST_NAME.to_string(),
    });
    verify_folder(folder)?;
    on_progress(&Progress {
        op: "export",
        phase: "done",
        done: manifest.entries.len() as u64,
        total: manifest.entries.len() as u64,
        item: folder.to_string_lossy().to_string(),
    });

    Ok(ExportReport {
        folder: folder.to_string_lossy().to_string(),
        created_at: now,
        parts: written,
        files: file_count,
        memory_files: memory_count,
        total_bytes: manifest.entries.iter().map(|e| e.size).sum(),
        entries_sha256: manifest.entries_sha256,
        skipped,
        daemon_restarted,
        notes,
    })
}

/// What [`copy_memory_out`] wrote: the file entries and the directories (with modes).
struct CopiedMemory {
    entries: Vec<ManifestEntry>,
    dirs: Vec<ManifestDir>,
}

/// Copy the memory dir into `out` (skipping the socket, the crash log, symlinks and non-regular
/// entries). Each copy keeps the original's mode, and every mode is recorded for the manifest.
fn copy_memory_out(
    mem_dir: &Path,
    out: &Path,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<CopiedMemory, BackupError> {
    let walked = walk(mem_dir)?;
    let files: Vec<PathBuf> = walked
        .files
        .into_iter()
        .filter(|rel| {
            !(rel.components().count() == 1
                && rel
                    .to_str()
                    .map(|s| MEMORY_EXCLUDE.contains(&s))
                    .unwrap_or(false))
        })
        .collect();
    let utf8 = |rel: &Path| {
        slash_path(rel).ok_or_else(|| {
            BackupError::Io(format!(
                "a memory store file name is not valid UTF-8: {}",
                rel.display()
            ))
        })
    };
    // Directories: the root plus every subdirectory, created owner-writable while copying.
    let mut dirs = Vec::new();
    if let Some(mode) = mode_of(mem_dir)? {
        dirs.push(ManifestDir {
            path: MEMORY_DIR.to_string(),
            mode,
        });
    }
    create_private_dir_all(out)?;
    for rel in &walked.dirs {
        let slash = utf8(rel)?;
        create_private_dir_all(&out.join(rel))?;
        if let Some(mode) = mode_of(&mem_dir.join(rel))? {
            dirs.push(ManifestDir {
                path: format!("{MEMORY_DIR}/{slash}"),
                mode,
            });
        }
    }
    let total = files.len() as u64;
    let mut entries = Vec::with_capacity(files.len());
    for (i, rel) in files.iter().enumerate() {
        let slash = utf8(rel)?;
        let to = out.join(rel);
        if let Some(parent) = to.parent() {
            create_private_dir_all(parent)?;
        }
        let from = mem_dir.join(rel);
        let mode = mode_of(&from)?;
        let (size, sha) = copy_hashing(&from, &to)?;
        if let Some(m) = mode {
            apply_mode(&to, m)?;
        }
        let path = format!("{MEMORY_DIR}/{slash}");
        entries.push(ManifestEntry {
            path: path.clone(),
            size,
            sha256: sha,
            cid: None,
            mode,
        });
        on_progress(&Progress {
            op: "export",
            phase: "memory",
            done: i as u64 + 1,
            total,
            item: path,
        });
    }
    // Directory modes last, deepest first, so a read-only original dir did not block the copy.
    if !entries.is_empty() {
        for d in dirs.iter().rev() {
            let rel: PathBuf = d.path.split('/').skip(1).collect();
            apply_mode(&out.join(rel), d.mode)?;
        }
    }
    dirs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(CopiedMemory { entries, dirs })
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

/// Read and parse a folder's manifest (no checks beyond parsing).
pub fn read_manifest(folder: &Path) -> Result<Manifest, BackupError> {
    let p = folder.join(MANIFEST_NAME);
    let meta = std::fs::symlink_metadata(&p)
        .map_err(|_| BackupError::Manifest(format!("no {MANIFEST_NAME} in that folder")))?;
    if !meta.file_type().is_file() {
        return Err(BackupError::Manifest(format!(
            "{MANIFEST_NAME} is not a regular file"
        )));
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(BackupError::Manifest(
            "the manifest is too large".to_string(),
        ));
    }
    let bytes = std::fs::read(&p).map_err(|e| BackupError::Manifest(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| BackupError::Manifest(e.to_string()))
}

/// Check a backup folder against its manifest. `Ok` only when every listed file is present, a
/// regular file, of the listed size and SHA-256, and nothing unlisted is in the folder.
pub fn verify_folder(folder: &Path) -> Result<Manifest, BackupError> {
    let m = read_manifest(folder)?;
    if m.format != FORMAT {
        return Err(BackupError::Manifest(format!(
            "format {:?} is not {FORMAT:?}",
            m.format
        )));
    }
    if manifest_digest(&m) != m.entries_sha256 {
        return Err(BackupError::Manifest(
            "its entry digest doesn't match its entries (the manifest is damaged or was edited)"
                .to_string(),
        ));
    }
    let mut problems = Vec::new();
    let mut listed = std::collections::BTreeSet::new();
    for e in &m.entries {
        let Some(rel) = safe_rel_path(&e.path) else {
            problems.push(Mismatch::UnsafePath {
                path: e.path.clone(),
            });
            continue;
        };
        if e.path.starts_with("files/") && e.cid.as_deref() != e.path.strip_prefix("files/") {
            problems.push(Mismatch::UnsafePath {
                path: e.path.clone(),
            });
            continue;
        }
        if !listed.insert(e.path.clone()) {
            problems.push(Mismatch::UnsafePath {
                path: e.path.clone(),
            });
            continue;
        }
        if e.mode.is_some_and(|mode| mode > 0o777) {
            problems.push(Mismatch::BadMode {
                path: e.path.clone(),
            });
            continue;
        }
        let full = folder.join(&rel);
        let meta = match std::fs::symlink_metadata(&full) {
            Ok(meta) => meta,
            Err(_) => {
                problems.push(Mismatch::Missing {
                    path: e.path.clone(),
                });
                continue;
            }
        };
        if !meta.file_type().is_file() {
            problems.push(Mismatch::NotAFile {
                path: e.path.clone(),
            });
            continue;
        }
        if meta.len() != e.size {
            problems.push(Mismatch::SizeDiffers {
                path: e.path.clone(),
                expected: e.size,
                actual: meta.len(),
            });
            continue;
        }
        let (_, sha) = hash_file(&full)?;
        if sha != e.sha256 {
            problems.push(Mismatch::HashDiffers {
                path: e.path.clone(),
            });
        }
    }
    // Directory records: the memory root or a safe path under it, permission bits only.
    let mut seen_dirs = std::collections::BTreeSet::new();
    for d in &m.dirs {
        let safe = d.path == MEMORY_DIR
            || (d.path.starts_with("memory/") && safe_rel_path(&d.path).is_some());
        if !safe || !seen_dirs.insert(d.path.clone()) {
            problems.push(Mismatch::UnsafePath {
                path: d.path.clone(),
            });
        } else if d.mode > 0o777 {
            problems.push(Mismatch::BadMode {
                path: d.path.clone(),
            });
        }
    }
    // Anything in the folder the manifest does not list.
    let walked = walk(folder)?;
    let (found, odd) = (walked.files, walked.odd);
    for rel in found {
        let Some(slash) = slash_path(&rel) else {
            problems.push(Mismatch::Unlisted {
                path: rel.to_string_lossy().to_string(),
            });
            continue;
        };
        if slash != MANIFEST_NAME && !listed.contains(&slash) {
            problems.push(Mismatch::Unlisted { path: slash });
        }
    }
    for rel in odd {
        let p = slash_path(&rel).unwrap_or_else(|| rel.to_string_lossy().to_string());
        if !listed.contains(&p) {
            problems.push(Mismatch::NotAFile { path: p });
        }
    }
    if problems.is_empty() {
        Ok(m)
    } else {
        Err(BackupError::Mismatch(problems))
    }
}

/// [`verify_folder`] as a report for the surface.
pub fn verify_report(folder: &Path) -> VerifyReport {
    let base = VerifyReport {
        folder: folder.to_string_lossy().to_string(),
        ok: false,
        created_at: None,
        parts: Vec::new(),
        file_count: 0,
        total_bytes: 0,
        mismatches: Vec::new(),
        problem: None,
    };
    // The manifest's own facts, when it parses (shown even when the check fails).
    let parsed = read_manifest(folder).ok();
    let with_facts = |mut r: VerifyReport| {
        if let Some(m) = &parsed {
            r.created_at = Some(m.created_at);
            r.parts = m.parts.clone();
            r.file_count = m.entries.len() as u64;
            r.total_bytes = m.entries.iter().map(|e| e.size).sum();
        }
        r
    };
    match verify_folder(folder) {
        Ok(_) => with_facts(VerifyReport { ok: true, ..base }),
        Err(BackupError::Mismatch(list)) => with_facts(VerifyReport {
            mismatches: list,
            ..base
        }),
        Err(e) => with_facts(VerifyReport {
            problem: Some(e.to_string()),
            ..base
        }),
    }
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/// Restore `parts` from a backup folder. Verifies the whole folder FIRST and refuses on any
/// mismatch before touching a daemon or a store. Files are re-added and pinned locally (no bond);
/// the memory store is swapped in from a re-checked staging copy, keeping the replaced store.
pub fn restore(
    folder: &Path,
    parts: &[Part],
    files: Option<&StorageManager>,
    memory: Option<&MemorySource<'_>>,
    now: u64,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<RestoreReport, BackupError> {
    if parts.is_empty() {
        return Err(BackupError::InvalidFolder(
            "choose Files, Memory, or both".to_string(),
        ));
    }
    on_progress(&Progress {
        op: "restore",
        phase: "verify",
        done: 0,
        total: 0,
        item: MANIFEST_NAME.to_string(),
    });
    let m = verify_folder(folder)?;
    // Decide what is actually restored. A requested part the backup does not hold, or holds with
    // no files, is skipped with its reason (and never touches the live store or daemon).
    let mut to_restore: Vec<Part> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();
    for p in parts {
        let prefix = format!("{}/", part_name(*p));
        let has_files = m.entries.iter().any(|e| e.path.starts_with(&prefix));
        if m.parts.contains(p) && has_files {
            to_restore.push(*p);
            continue;
        }
        let reason = if let Some(sp) = m.skipped_parts.iter().find(|sp| sp.part == *p) {
            format!("not in this backup (the export skipped it: {})", sp.reason)
        } else if !m.parts.contains(p) {
            "not in this backup".to_string()
        } else {
            format!("this backup has no {} files", part_label(*p))
        };
        skipped.push(Skipped {
            item: part_name(*p).to_string(),
            reason,
        });
    }
    if to_restore.is_empty() {
        let why: Vec<String> = skipped
            .iter()
            .map(|s| format!("{}: {}", s.item, s.reason))
            .collect();
        return Err(BackupError::InvalidFolder(format!(
            "nothing to restore ({})",
            why.join("; ")
        )));
    }
    let parts = &to_restore[..];
    if parts.contains(&Part::Files) && files.is_none() {
        return Err(BackupError::InvalidFolder(
            "the file store is not available".to_string(),
        ));
    }
    if parts.contains(&Part::Memory) && memory.is_none() {
        return Err(BackupError::InvalidFolder(
            "the memory store is not available".to_string(),
        ));
    }

    let mut report = RestoreReport {
        folder: folder.to_string_lossy().to_string(),
        parts: parts.to_vec(),
        skipped,
        files_restored: 0,
        cids_changed: Vec::new(),
        memory_files_restored: 0,
        previous_memory_kept_at: None,
        daemon_restarted: false,
        notes: Vec::new(),
    };

    if let (true, Some(m_target)) = (parts.contains(&Part::Memory), memory) {
        let mem_entries: Vec<&ManifestEntry> = m
            .entries
            .iter()
            .filter(|e| e.path.starts_with("memory/"))
            .collect();
        let mut notes = Vec::new();
        let ((n, kept), restarted) =
            with_memory_paused(m_target, &mut notes, on_progress, "restore", |on| {
                swap_in_memory(folder, &mem_entries, &m.dirs, &m_target.dir, now, on)
            })?;
        report.notes.extend(notes);
        report.memory_files_restored = n;
        report.previous_memory_kept_at = kept.map(|p| p.to_string_lossy().to_string());
        report.daemon_restarted = restarted;
    }

    if let (true, Some(mgr)) = (parts.contains(&Part::Files), files) {
        let file_entries: Vec<&ManifestEntry> = m
            .entries
            .iter()
            .filter(|e| e.path.starts_with("files/"))
            .collect();
        let total = file_entries.len() as u64;
        for (i, e) in file_entries.iter().enumerate() {
            let rel = safe_rel_path(&e.path).ok_or_else(|| {
                BackupError::Mismatch(vec![Mismatch::UnsafePath {
                    path: e.path.clone(),
                }])
            })?;
            let bytes =
                std::fs::read(folder.join(rel)).map_err(|err| io_err("read a file", err))?;
            // The file could have changed since the folder check: hash the bytes we add.
            if bytes.len() as u64 != e.size || sha256_hex(&bytes) != e.sha256 {
                return Err(BackupError::Mismatch(vec![Mismatch::HashDiffers {
                    path: e.path.clone(),
                }]));
            }
            let expected = e.cid.clone().unwrap_or_default();
            let out = mgr
                .add_bytes(&expected, &bytes)
                .map_err(|err| BackupError::Kubo(err.to_string()))?;
            mgr.pin_local(&out.cid)
                .map_err(|err| BackupError::Kubo(err.to_string()))?;
            if out.cid != expected {
                report.cids_changed.push(CidChange {
                    expected,
                    actual: out.cid,
                });
            }
            report.files_restored += 1;
            on_progress(&Progress {
                op: "restore",
                phase: "files",
                done: i as u64 + 1,
                total,
                item: e.path.clone(),
            });
        }
    }

    on_progress(&Progress {
        op: "restore",
        phase: "done",
        done: report.files_restored + report.memory_files_restored,
        total: report.files_restored + report.memory_files_restored,
        item: report.folder.clone(),
    });
    Ok(report)
}

/// Stage the memory entries beside the live dir, re-check every hash there, then swap: the live
/// dir moves to `memory.before-restore-<time>` (kept) and the staging dir takes its place. Any
/// failure before the swap removes the staging dir and leaves the live store as it was.
fn swap_in_memory(
    folder: &Path,
    entries: &[&ManifestEntry],
    dirs: &[ManifestDir],
    mem_dir: &Path,
    now: u64,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<(u64, Option<PathBuf>), BackupError> {
    let parent = mem_dir
        .parent()
        .ok_or_else(|| BackupError::Io("the memory store has no parent folder".to_string()))?;
    let base = mem_dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| MEMORY_DIR.to_string());
    let stamp = backup_folder_name(now).replace("citrate-backup-", "");
    let staging = unique_sibling(parent, &format!("{base}.restore-staging-{stamp}"));
    // 0700 from creation, before any file lands (the parent may be readable by other users).
    create_private_dir(&staging).map_err(|e| io_err("create the staging folder", e))?;

    let staged = (|| -> Result<u64, BackupError> {
        // Recorded directories first (empty ones included), owner-writable while staging.
        for d in dirs {
            if d.path != MEMORY_DIR {
                let inner: PathBuf = d.path.split('/').skip(1).collect();
                create_private_dir_all(&staging.join(inner))?;
            }
        }
        let total = entries.len() as u64;
        for (i, e) in entries.iter().enumerate() {
            let rel = safe_rel_path(&e.path).ok_or_else(|| {
                BackupError::Mismatch(vec![Mismatch::UnsafePath {
                    path: e.path.clone(),
                }])
            })?;
            // Strip the leading `memory/`.
            let inner: PathBuf = rel.components().skip(1).collect();
            let to = staging.join(&inner);
            if let Some(p) = to.parent() {
                create_private_dir_all(p)?;
            }
            let (size, sha) = copy_hashing(&folder.join(&rel), &to)?;
            if size != e.size || sha != e.sha256 {
                return Err(BackupError::Mismatch(vec![Mismatch::HashDiffers {
                    path: e.path.clone(),
                }]));
            }
            // The original's mode (masked, see apply_restored_mode); owner-only when none recorded.
            apply_restored_mode(&to, &e.path, e.mode.unwrap_or(0o600))?;
            on_progress(&Progress {
                op: "restore",
                phase: "memory",
                done: i as u64 + 1,
                total,
                item: e.path.clone(),
            });
        }
        // Directory modes last, deepest first; the root and any unrecorded dir are owner-only.
        let walked = walk(&staging)?;
        for rel in walked.dirs.iter().rev() {
            let slash = slash_path(rel).unwrap_or_default();
            let path = format!("{MEMORY_DIR}/{slash}");
            let mode = dirs
                .iter()
                .find(|d| d.path == path)
                .map(|d| d.mode)
                .unwrap_or(0o700);
            apply_restored_mode(&staging.join(rel), &path, mode)?;
        }
        let root_mode = dirs
            .iter()
            .find(|d| d.path == MEMORY_DIR)
            .map(|d| d.mode)
            .unwrap_or(0o700);
        apply_restored_mode(&staging, MEMORY_DIR, root_mode)?;
        Ok(total)
    })();
    let n = match staged {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    let kept = if mem_dir.exists() {
        let kept = unique_sibling(parent, &format!("{base}.before-restore-{stamp}"));
        if let Err(e) = std::fs::rename(mem_dir, &kept) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(io_err("move the current memory store aside", e));
        }
        Some(kept)
    } else {
        None
    };
    if let Err(e) = std::fs::rename(&staging, mem_dir) {
        // Put the previous store back; the staging copy stays for inspection.
        if let Some(k) = &kept {
            let _ = std::fs::rename(k, mem_dir);
        }
        return Err(io_err("swap in the restored memory store", e));
    }
    Ok((n, kept))
}

/// `parent/name`, or `parent/name-2`, `-3`, … when it exists.
fn unique_sibling(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    (2..u32::MAX)
        .map(|n| parent.join(format!("{name}-{n}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn files_manager<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<StorageManager, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("ipfs");
    Ok(StorageManager::new(
        Box::new(crate::storage::UreqKuboTransport::from_env()),
        dir,
    ))
}

fn memory_source<'a>(mgr: &'a crate::memory::MemoryManager) -> Result<MemorySource<'a>, String> {
    let store_path = mgr.store_path().to_path_buf();
    let dir = store_path
        .parent()
        .ok_or_else(|| "the memory store has no parent folder".to_string())?
        .to_path_buf();
    Ok(MemorySource {
        dir,
        store_path,
        daemon: mgr,
        registry: crate::knowledge_import::ImportRegistry::global(),
    })
}

/// **Command — storage_backup_export.** Export Files and/or Memory into a new verified backup
/// folder inside the member-chosen `dest`. Emits [`PROGRESS_EVENT`] while it runs.
#[tauri::command]
pub async fn storage_backup_export(
    app_h: tauri::AppHandle,
    dest: String,
    parts: Vec<String>,
) -> Result<ExportReport, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || storage_backup_export_sync(app_h.clone(), dest, parts)).await
}

/// Blocking body of [`storage_backup_export`]; reached only through [`crate::blocking::off_main`].
pub fn storage_backup_export_sync(
    app: tauri::AppHandle,
    dest: String,
    parts: Vec<String>,
) -> Result<ExportReport, String> {
    use tauri::Emitter;
    let parts = Part::parse_list(&parts).map_err(|e| e.to_string())?;
    let files = files_manager(&app)?;
    let mem_state = tauri::Manager::try_state::<crate::memory::MemoryState>(&app);
    let mem_src = match &mem_state {
        Some(s) => Some(memory_source(&s.0)?),
        None => None,
    };
    let mut on = |p: &Progress| {
        let _ = app.emit(PROGRESS_EVENT, p);
    };
    export(
        Path::new(&dest),
        &parts,
        Some(&files),
        mem_src.as_ref(),
        unix_now(),
        &mut on,
    )
    .map_err(|e| e.to_string())
}

/// **Command — storage_backup_verify.** Check a backup folder against its manifest.
#[tauri::command]
pub async fn storage_backup_verify(folder: String) -> Result<VerifyReport, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || storage_backup_verify_sync(folder)).await
}

/// Blocking body of [`storage_backup_verify`]; reached only through [`crate::blocking::off_main`].
pub fn storage_backup_verify_sync(folder: String) -> Result<VerifyReport, String> {
    let p = Path::new(&folder);
    if !p.is_absolute() {
        return Err("choose a backup folder (an absolute path)".to_string());
    }
    Ok(verify_report(p))
}

/// **Command — storage_backup_restore.** Restore Files and/or Memory from a backup folder; refuses
/// on any manifest mismatch before anything is touched. Emits [`PROGRESS_EVENT`].
#[tauri::command]
pub async fn storage_backup_restore(
    app_h: tauri::AppHandle,
    folder: String,
    parts: Vec<String>,
) -> Result<RestoreReport, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || storage_backup_restore_sync(app_h.clone(), folder, parts))
        .await
}

/// Blocking body of [`storage_backup_restore`]; reached only through [`crate::blocking::off_main`].
pub fn storage_backup_restore_sync(
    app: tauri::AppHandle,
    folder: String,
    parts: Vec<String>,
) -> Result<RestoreReport, String> {
    use tauri::Emitter;
    let p = Path::new(&folder);
    if !p.is_absolute() {
        return Err("choose a backup folder (an absolute path)".to_string());
    }
    let parts = Part::parse_list(&parts).map_err(|e| e.to_string())?;
    let files = files_manager(&app)?;
    let mem_state = tauri::Manager::try_state::<crate::memory::MemoryState>(&app);
    let mem_src = match &mem_state {
        Some(s) => Some(memory_source(&s.0)?),
        None => None,
    };
    let mut on = |pr: &Progress| {
        let _ = app.emit(PROGRESS_EVENT, pr);
    };
    restore(
        p,
        &parts,
        Some(&files),
        mem_src.as_ref(),
        unix_now(),
        &mut on,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("storage_backup_tests.rs");
}
