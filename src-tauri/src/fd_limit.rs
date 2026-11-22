//! Raise this process's soft open-file limit at start-up, before any sidecar is spawned.
//!
//! 0.5.7: macOS gives an app launched from Finder or the Dock a soft `RLIMIT_NOFILE` of 256. The
//! bundled node inherits it, and a chain database with more SST files than that exits on start
//! with "Too many open files" (EMFILE), then the supervisor restarts it into the same wall. Rlimits
//! are inherited across fork and exec, so raising the app's own soft limit once, first, raises it
//! for every child the app spawns (node, IPFS, llama-server, the rest).
//!
//! The soft limit goes to `min(hard, TARGET_NOFILE)`, and on macOS also to at most the kernel's
//! per-process cap (`kern.maxfilesperproc`, else `OPEN_MAX`), because `setrlimit` there refuses a
//! soft limit above it even when the hard limit reads unlimited. It is never lowered. Any failure
//! is logged and start-up goes on: the node raises its own limit too (the chain-side fix), this is
//! the second guard.
// `macos_cap` runs only on macOS and the rest only on Unix; the tests cover every item everywhere.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// The soft limit asked for. RocksDB keeps an fd per open SST; 65536 covers a large database with
/// room for sockets.
pub const TARGET_NOFILE: u64 = 65536;
/// macOS `<sys/syslimits.h>` `OPEN_MAX`: the documented `setrlimit(RLIMIT_NOFILE)` ceiling when
/// `kern.maxfilesperproc` can't be read.
pub const MACOS_OPEN_MAX: u64 = 10240;

/// The soft limit to set: `want`, capped by the hard limit and by the OS per-process cap when
/// there is one, and never below the current soft limit.
pub fn target_soft(soft: u64, hard: u64, want: u64, os_cap: Option<u64>) -> u64 {
    let mut t = want.min(hard);
    if let Some(cap) = os_cap.filter(|c| *c > 0) {
        t = t.min(cap);
    }
    t.max(soft)
}

/// The macOS per-process cap: `kern.maxfilesperproc` when it reads as a positive number, else
/// [`MACOS_OPEN_MAX`].
pub fn macos_cap(maxfilesperproc: Option<i64>) -> u64 {
    match maxfilesperproc {
        Some(n) if n > 0 => n as u64,
        _ => MACOS_OPEN_MAX,
    }
}

/// What start-up did to the limit, for the one log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FdLimitReport {
    pub before_soft: u64,
    pub hard: u64,
    pub after_soft: u64,
    /// Why the limit was not raised as asked, when it was not.
    pub error: Option<String>,
}

impl std::fmt::Display for FdLimitReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let show = |v: u64| {
            if v == u64::MAX {
                "unlimited".to_string()
            } else {
                v.to_string()
            }
        };
        write!(
            f,
            "[fd-limit] open-file limit: soft {} -> {} (hard {})",
            show(self.before_soft),
            show(self.after_soft),
            show(self.hard)
        )?;
        if let Some(e) = &self.error {
            write!(f, "; {e}")?;
        }
        Ok(())
    }
}

// `rlim_t` is u64 on the shipped targets (Linux and macOS, 64-bit) but not on every Unix, so the
// casts stay and the lint that calls them unnecessary here is allowed.
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
fn rlim_to_u64(v: libc::rlim_t) -> u64 {
    if v == libc::RLIM_INFINITY {
        u64::MAX
    } else {
        v as u64
    }
}

#[cfg(unix)]
fn get_nofile() -> std::io::Result<(u64, u64)> {
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `rl` is a valid, writable rlimit for the duration of the call.
    let rc = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((rlim_to_u64(rl.rlim_cur), rlim_to_u64(rl.rlim_max)))
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
fn set_soft_nofile(soft: u64, hard: u64) -> std::io::Result<()> {
    let to = |v: u64| {
        if v == u64::MAX {
            libc::RLIM_INFINITY
        } else {
            v as libc::rlim_t
        }
    };
    let rl = libc::rlimit {
        rlim_cur: to(soft),
        rlim_max: to(hard),
    };
    // SAFETY: `rl` is a valid rlimit; setrlimit only reads it.
    let rc = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rl) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn os_cap() -> Option<u64> {
    let name = c"kern.maxfilesperproc";
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>();
    // SAFETY: `name` is NUL-terminated; `value`/`len` are valid for writes of a c_int, and no new
    // value is passed (null, 0).
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut libc::c_int).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    Some(macos_cap((rc == 0).then_some(i64::from(value))))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_cap() -> Option<u64> {
    None
}

/// Raise this process's soft `RLIMIT_NOFILE` toward `want` (see the module doc). Never fails:
/// what happened is in the report.
#[cfg(unix)]
pub fn raise_nofile(want: u64) -> FdLimitReport {
    let (soft, hard) = match get_nofile() {
        Ok(v) => v,
        Err(e) => {
            return FdLimitReport {
                before_soft: 0,
                hard: 0,
                after_soft: 0,
                error: Some(format!("getrlimit failed: {e}")),
            }
        }
    };
    let target = target_soft(soft, hard, want, os_cap());
    let mut error = None;
    if target > soft {
        if let Err(e) = set_soft_nofile(target, hard) {
            // A kernel that still refuses (an older macOS): the documented OPEN_MAX ceiling.
            let fallback = target_soft(soft, hard, want, Some(MACOS_OPEN_MAX));
            let retry = (fallback > soft && fallback < target)
                .then(|| set_soft_nofile(fallback, hard))
                .transpose();
            error = Some(match retry {
                Ok(Some(())) => format!("setrlimit to {target} refused ({e}); used {fallback}"),
                _ => format!("setrlimit to {target} refused ({e}); left unchanged"),
            });
        }
    }
    let after_soft = get_nofile().map(|(s, _)| s).unwrap_or(soft);
    FdLimitReport {
        before_soft: soft,
        hard,
        after_soft,
        error,
    }
}

/// Start-up entry: raise to [`TARGET_NOFILE`] and log one line. Unix only (Windows has no
/// `RLIMIT_NOFILE`; its handle limit is not the problem this fixes).
pub fn apply_at_startup() {
    #[cfg(unix)]
    eprintln!("{}", raise_nofile(TARGET_NOFILE));
}

#[cfg(test)]
mod tests {
    include!("fd_limit_tests.rs");
}
