//! Shared filesystem helpers for writing secret material with restrictive
//! permissions FROM THE CREATING SYSCALL (CORE-B-001, CORE-B-006).
//!
//! `std::fs::write` creates a file at the process umask (typically world-readable
//! `0644`) and leaves it that way until a later `chmod` narrows it — a window in
//! which another local user (or any process holding a descriptor to the dir) can
//! open key / seed / token material before it is hardened. A crash inside that
//! window also leaves the file at the loose mode.
//!
//! These helpers set mode `0600` in the `open(2)` call itself (via
//! `OpenOptionsExt::mode`) so a NEWLY created secret file is never observable at
//! looser permissions, and additionally re-assert `0600` after writing so a
//! PRE-EXISTING file that was created loosely (the mode arg is ignored when the
//! file already exists) is narrowed too. One shared implementation replaces the
//! four byte-identical `persist_secret_0600` copies the audit flagged.

use std::io;
use std::path::Path;

/// Create `path` for writing, truncating any existing file, with mode `0600`
/// applied by the creating `open(2)` on unix. Returns the open handle so the
/// caller can write + fsync (see [`crate::custody`]'s atomic envelope write).
///
/// NOTE: the `mode` argument only takes effect when `open(2)` CREATES the file;
/// for an already-existing path it is ignored, so callers that must guarantee
/// `0600` on a possibly-preexisting file use [`write_secret_file`] (which
/// re-asserts the mode after writing).
#[cfg(unix)]
pub fn create_secret_file(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

/// Non-unix fallback: no POSIX mode. (This app ships on macOS/Linux; the guard is
/// a no-op on platforms without unix permissions.)
#[cfg(not(unix))]
pub fn create_secret_file(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

/// Ensure `dir` exists and is owner-only (`0700`) on unix.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dir)?.permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(dir, perms)?;
    }
    Ok(())
}

/// Write `bytes` to `path` as owner-only secret material: the parent dir (if any)
/// is created `0700`, the file is created `0600` by the `open(2)` itself (never a
/// world-readable window), and its mode is re-asserted `0600` after the write
/// (covering a pre-existing loosely-created file). Fails closed on any I/O error.
pub fn write_secret_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut f = create_secret_file(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = f.metadata()?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("citrate-fsutil-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    /// CORE-B-001 tripwire. The CREATING syscall must yield `0600` — asserted
    /// BEFORE any subsequent `chmod` runs, so it fails on the old
    /// `fs::write`-then-`set_mode` sequence (which is `0644` in the window between
    /// the two calls).
    #[test]
    fn create_secret_file_is_0600_from_the_open_call() {
        let dir = tmp("create");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("member.seed");
        let f = create_secret_file(&p).unwrap();
        // No chmod between create and this stat: the mode is whatever open(2) set.
        let mode = f.metadata().unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "a secret file must be 0600 AT CREATION, not only after a later chmod"
        );
        drop(f);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// CORE-B-001 / CORE-B-006. The full writer round-trips the bytes and leaves
    /// no group/other bits on the file OR its parent dir.
    #[test]
    fn write_secret_file_is_owner_only() {
        let dir = tmp("write");
        let p = dir.join("nested").join("pending.json");
        write_secret_file(&p, b"secret-priv-key").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"secret-priv-key");
        let fmode = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(fmode & 0o077, 0, "no group/other access to secret material");
        let dmode = std::fs::metadata(p.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dmode & 0o077, 0, "the secret's parent dir must be 0700");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
