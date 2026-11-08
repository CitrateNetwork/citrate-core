//! "Remove old network data" (CORE-N3 follow-up, 0.5.3): the member deletes the previous network's
//! chain data that a genesis switch set aside in `<node data dir>/chain.<tag>.bak[-n]/`
//! ([`crate::node_genesis::set_aside_chain_db`]).
//!
//! Data source: the node data dir on this machine. Nothing else is read or written.
//!
//! What can be removed, and only that: a real directory (not a symlink) directly in the node data
//! dir, whose name is a backup name ([`crate::node_genesis::is_chain_aside_name`]), that holds only
//! regular chain database files (RocksDB names, or the `<name>.dup-<n>` copies the set-aside
//! makes), whose total size is still the size the member confirmed, while no genesis switch is
//! unfinished. The wallet (outside the node dir), the node keys, `node.toml`, the marker and the
//! current chain (all top-level files of the node dir) can never match. Files are removed one by
//! one, then the empty directory: nothing is removed recursively.

use std::path::{Path, PathBuf};

/// One set-aside chain database, as listed for the member.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainBackup {
    /// The directory name, `chain.<tag>.bak[-n]`.
    pub name: String,
    /// Total size of its files, in bytes.
    pub bytes: u64,
    /// How many files it holds.
    pub files: u64,
}

/// Why a removal was refused. Nothing was removed in any of these cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupError {
    /// The name is not a chain backup directly in the node data dir (or is a symlink).
    NotABackup(String),
    /// No backup of that name.
    Absent(String),
    /// The backup holds something that is not a chain database file.
    Unexpected(String),
    /// Its size is not the size the member confirmed (it changed since it was shown).
    Changed { confirmed: u64, now: u64 },
    /// A genesis switch is not finished; the backup may still be being written.
    SwitchInProgress,
    /// A filesystem step failed.
    Io(String),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupError::NotABackup(n) => write!(f, "{n:?} is not old network data in the node folder"),
            BackupError::Absent(n) => write!(f, "there is no old network data named {n:?}"),
            BackupError::Unexpected(why) => write!(
                f,
                "the folder holds something that is not chain data ({why}), so it was not removed"
            ),
            BackupError::Changed { confirmed, now } => write!(
                f,
                "the folder changed since it was shown ({confirmed} bytes then, {now} now); read it again"
            ),
            BackupError::SwitchInProgress => write!(
                f,
                "a network switch is not finished; start the node once, then try again"
            ),
            BackupError::Io(m) => write!(f, "removing the old network data failed: {m}"),
        }
    }
}

impl std::error::Error for BackupError {}

/// A file name a chain backup may hold: a RocksDB file, or a `<rocksdb name>.dup-<n>` copy.
fn is_backup_file_name(name: &str) -> bool {
    if crate::node_genesis::is_chain_db_entry(name) {
        return true;
    }
    match name.rsplit_once(".dup-") {
        Some((base, n)) => {
            !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && crate::node_genesis::is_chain_db_entry(base)
        }
        None => false,
    }
}

/// The files of the backup at `dir` and their total size; refuses anything else inside it.
fn backup_contents(dir: &Path) -> Result<(Vec<PathBuf>, u64), BackupError> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| BackupError::Io(format!("read {}: {e}", dir.display())))?;
    let mut files = Vec::new();
    let mut bytes = 0u64;
    for entry in rd {
        let entry = entry.map_err(|e| BackupError::Io(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = std::fs::symlink_metadata(entry.path())
            .map_err(|e| BackupError::Io(format!("{name}: {e}")))?;
        if meta.file_type().is_symlink() {
            return Err(BackupError::Unexpected(format!("{name} is a link")));
        }
        if !meta.is_file() {
            return Err(BackupError::Unexpected(format!("{name} is not a file")));
        }
        if !is_backup_file_name(&name) {
            return Err(BackupError::Unexpected(format!(
                "{name} is not a chain database file"
            )));
        }
        bytes = bytes.saturating_add(meta.len());
        files.push(entry.path());
    }
    Ok((files, bytes))
}

/// The backup directory `name` in `data_dir`, checked to be a real directory directly in it.
fn backup_dir(data_dir: &Path, name: &str) -> Result<PathBuf, BackupError> {
    if !crate::node_genesis::is_chain_aside_name(name) {
        return Err(BackupError::NotABackup(name.to_string()));
    }
    let dir = data_dir.join(name);
    let meta = match std::fs::symlink_metadata(&dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(BackupError::Absent(name.to_string()))
        }
        Err(e) => return Err(BackupError::Io(format!("{name}: {e}"))),
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(BackupError::NotABackup(name.to_string()));
    }
    // Belt and braces: the resolved directory's parent is the resolved node data dir.
    let real = dir
        .canonicalize()
        .map_err(|e| BackupError::Io(format!("{name}: {e}")))?;
    let base = data_dir
        .canonicalize()
        .map_err(|e| BackupError::Io(format!("{}: {e}", data_dir.display())))?;
    if real.parent() != Some(base.as_path()) {
        return Err(BackupError::NotABackup(name.to_string()));
    }
    Ok(dir)
}

/// Every chain backup in `data_dir`, by name, with its size. A backup holding anything unexpected
/// is still listed (its size counts only what can be read); its removal is refused.
pub fn list_chain_backups(data_dir: &Path) -> Result<Vec<ChainBackup>, String> {
    let rd = match std::fs::read_dir(data_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read {}: {e}", data_dir.display())),
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(dir) = backup_dir(data_dir, &name) else {
            continue;
        };
        let (files, bytes) = match backup_contents(&dir) {
            Ok((f, b)) => (f.len() as u64, b),
            Err(_) => {
                let mut n = 0u64;
                let mut b = 0u64;
                for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    n += 1;
                    b = b.saturating_add(
                        std::fs::symlink_metadata(e.path())
                            .map(|m| m.len())
                            .unwrap_or(0),
                    );
                }
                (n, b)
            }
        };
        out.push(ChainBackup { name, bytes, files });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Remove the backup `name` from `data_dir`, if it is still `confirmed_bytes` in size. Returns the
/// bytes freed. Every refusal removes nothing.
pub fn remove_chain_backup(
    data_dir: &Path,
    name: &str,
    confirmed_bytes: u64,
) -> Result<u64, BackupError> {
    let dir = backup_dir(data_dir, name)?;
    if std::fs::symlink_metadata(data_dir.join(crate::node_genesis::MIGRATION_INTENT_FILE)).is_ok()
    {
        return Err(BackupError::SwitchInProgress);
    }
    let (files, bytes) = backup_contents(&dir)?;
    if bytes != confirmed_bytes {
        return Err(BackupError::Changed {
            confirmed: confirmed_bytes,
            now: bytes,
        });
    }
    for f in &files {
        std::fs::remove_file(f).map_err(|e| BackupError::Io(format!("{}: {e}", f.display())))?;
    }
    std::fs::remove_dir(&dir).map_err(|e| BackupError::Io(format!("{name}: {e}")))?;
    Ok(bytes)
}
