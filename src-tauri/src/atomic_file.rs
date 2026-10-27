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

/// Stage `bytes` for an owner-only (`0600`) file: write them to a temp file beside `path` and
/// fsync it. [`commit_staged`] publishes it. On a failure the temp file is removed.
pub fn stage_secret(path: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    stage_secret_with(bytes, || unique_temp_path(path))
}

/// [`stage_secret`] with the temp names drawn from `next_name`.
fn stage_secret_with(
    bytes: &[u8],
    mut next_name: impl FnMut() -> PathBuf,
) -> std::io::Result<PathBuf> {
    let tmp = next_name();
    let staged = (|| {
        let mut f = citrate_core_kit::fsutil::create_secret_file(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    match staged {
        Ok(()) => Ok(tmp),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Rename a file staged by [`stage_secret`] over `path`, then fsync the directory so the rename
/// itself is durable. A failed rename removes the staged file; `path` keeps its previous contents.
pub fn commit_staged(staged: &Path, path: &Path) -> std::io::Result<()> {
    if let Err(e) = std::fs::rename(staged, path) {
        let _ = std::fs::remove_file(staged);
        return Err(e);
    }
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir).and_then(|d| d.sync_all())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("atomic_file_tests.rs");
}
