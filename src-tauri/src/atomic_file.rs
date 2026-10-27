//! Replace a small file through a temp file whose name is unique to this write.
//!
//! #420: a fixed temp name (`<file>.tmp`) is shared by every concurrent writer of the same file,
//! so one writer can rename another's half-written file or fail after the other renamed it away.
//! A temp name with the process id and a per-process counter is never shared, and the rename that
//! publishes it is atomic, so overlapping writers each land a whole file and the last rename wins.
//! The `fixed_temp_names` tripwire in the tests keeps new fixed temp names out of `src-tauri/src`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// `<dir>/<name>.tmp-<pid>-<n>`: a temp path next to `path`, unique within this process and,
/// through the process id, across processes.
fn unique_temp_path(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    path.with_file_name(format!("{name}.tmp-{}-{n}", std::process::id()))
}

/// Write `bytes` to a unique temp file beside `path`, fsync it, and rename it over `path`. On a
/// failure the temp file is removed and `path` keeps its previous contents.
pub fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = unique_temp_path(path);
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// [`replace`] for owner-only data: the temp file is created `0600` by its own `open(2)` (never
/// readable by another user, even for a moment), and the directory is fsynced after the rename so
/// the new name survives a power loss.
pub fn replace_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = unique_temp_path(path);
    let written = (|| {
        let mut f = citrate_core_kit::fsutil::create_secret_file(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        sync_parent(path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// fsync the directory holding `path` (unix), so a rename into it is durable.
fn sync_parent(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("atomic_file_tests.rs");
}
