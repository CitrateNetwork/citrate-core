//! Startup cleanup of this installation's own leftover sidecars (SCL-S0.1).
//!
//! A sidecar left running by a previous run that crashed can still hold its data-dir lock or
//! socket, so a fresh launch cannot open its own. At startup, before any sidecar of ours is
//! spawned, this module stops those leftovers.
//!
//! It is the one-release legacy path of the sidecar-lifecycle planset (O-17): 0.4.x wrote no
//! ownership records, so 0.5.0 identifies its leftovers by executable path alone, and only
//! narrowly:
//!
//! - **Exact owned paths only.** A process is a candidate only when the kernel's record of its
//!   executable equals one of this installation's sidecar binaries, as resolved by the same
//!   resolvers the app uses to launch them. A process's command line, arguments, `argv[0]` or
//!   bare name never make it a candidate, so a process that merely mentions one of our paths or
//!   shares a sidecar's name is never signalled.
//! - **Already orphaned.** A candidate whose parent is a live instance of this installation's
//!   main executable belongs to that instance and is left alone.
//! - **Same user only,** and never this process or its own children.
//! - **Re-checked immediately before the signal,** so a pid that was reused in the meantime by
//!   another program is not signalled.
//!
//! Anything else (for example a sidecar of another installation) is out of scope here; the
//! startup checks of SCL-S0.6 name such a process in the UI instead of signalling it.
//!
//! Supported on macOS and Linux. Elsewhere the process list is unavailable and the cleanup does
//! nothing.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// One running process as the kernel reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcEntry {
    pub pid: u32,
    pub ppid: u32,
    /// The kernel's record of the executable the process is running.
    pub exe: PathBuf,
}

/// How long [`cleanup`] waits for the signalled processes to be gone.
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// Both spellings of a path worth comparing: as given, and canonical when it resolves.
fn spellings(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![p.to_path_buf()];
    if let Ok(c) = std::fs::canonicalize(p) {
        if c != p {
            v.push(c);
        }
    }
    v
}

/// True when `exe` names the same file as one of `targets` (exact path comparison, after
/// canonicalising both sides where the path resolves).
fn same_path(exe: &Path, targets: &[PathBuf]) -> bool {
    let exe_forms = spellings(exe);
    targets.iter().any(|t| exe_forms.iter().any(|e| e == t))
}

/// Expand a list of paths into every spelling worth comparing against.
fn expand(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    for p in paths {
        for s in spellings(p) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
    }
    v
}

/// The pids to stop: processes whose executable is exactly one of `owned`, that are not this
/// process or its children, and whose parent is not a live instance of `main_exe`.
pub(crate) fn select_orphans(
    procs: &[ProcEntry],
    owned: &[PathBuf],
    main_exe: Option<&Path>,
    self_pid: u32,
) -> Vec<u32> {
    if owned.is_empty() {
        return Vec::new();
    }
    let owned = expand(owned);
    let main = main_exe.map(|m| expand(&[m.to_path_buf()]));
    let mut out = Vec::new();
    for p in procs {
        if p.pid == self_pid || p.pid <= 1 || p.ppid == self_pid {
            continue;
        }
        if !same_path(&p.exe, &owned) {
            continue;
        }
        if let Some(main) = &main {
            let parent_is_live_instance = procs
                .iter()
                .any(|q| q.pid == p.ppid && same_path(&q.exe, main));
            if parent_is_live_instance {
                continue;
            }
        }
        out.push(p.pid);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Stop this installation's leftover sidecars. `owned` are the exact executable paths of our
/// sidecar binaries; `main_exe` is this installation's main executable. Returns the pids that
/// were signalled. Best effort: a process that cannot be listed or signalled is skipped.
pub(crate) fn cleanup(owned: &[PathBuf], main_exe: Option<&Path>) -> Vec<u32> {
    let Some(procs) = os::list_processes() else {
        return Vec::new();
    };
    let owned_forms = expand(owned);
    let mut signalled = Vec::new();
    for pid in select_orphans(&procs, owned, main_exe, std::process::id()) {
        // Re-check right before the signal: the pid must still run one of our binaries.
        let still_ours = os::exe_of(pid)
            .map(|exe| same_path(&exe, &owned_forms))
            .unwrap_or(false);
        if still_ours && os::kill_now(pid) {
            signalled.push(pid);
        }
    }
    // Wait (bounded) until each signalled process no longer runs our binary, so the locks it
    // held are released before our own sidecars start.
    let deadline = Instant::now() + EXIT_WAIT;
    while Instant::now() < deadline {
        let any_left = signalled.iter().any(|pid| {
            os::exe_of(*pid)
                .map(|exe| same_path(&exe, &owned_forms))
                .unwrap_or(false)
        });
        if !any_left {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    signalled
}

/// The executable path as Linux reports it for a process whose file was replaced or removed
/// since it started: the link text gains a ` (deleted)` suffix. The process is still running
/// the binary that was installed at that path, so the suffix is dropped for the comparison.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux_exe_link_path(link: &Path) -> PathBuf {
    const SUFFIX: &str = " (deleted)";
    match link.to_str() {
        Some(s) if s.ends_with(SUFFIX) => PathBuf::from(&s[..s.len() - SUFFIX.len()]),
        _ => link.to_path_buf(),
    }
}

/// Parent pid from the text of Linux `/proc/<pid>/stat`. The command name (field 2) is in
/// parentheses and may contain spaces or `)`, so parsing starts after the LAST `)`.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn linux_stat_ppid(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    // Fields after the name: state, ppid, ...
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(target_os = "linux")]
mod os {
    use super::{linux_exe_link_path, linux_stat_ppid, ProcEntry};
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;

    fn same_user(pid: u32) -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        std::fs::metadata(format!("/proc/{pid}"))
            .map(|m| m.uid() == me)
            .unwrap_or(false)
    }

    pub(super) fn exe_of(pid: u32) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| linux_exe_link_path(&p))
    }

    pub(super) fn list_processes() -> Option<Vec<ProcEntry>> {
        let dir = std::fs::read_dir("/proc").ok()?;
        let mut out = Vec::new();
        for ent in dir.flatten() {
            let Some(pid) = ent.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            if !same_user(pid) {
                continue;
            }
            let Some(exe) = exe_of(pid) else { continue };
            let Some(ppid) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|s| linux_stat_ppid(&s))
            else {
                continue;
            };
            out.push(ProcEntry { pid, ppid, exe });
        }
        Some(out)
    }

    pub(super) fn kill_now(pid: u32) -> bool {
        let Ok(p) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: kill(2) with a positive pid signals that one process; no memory is touched.
        unsafe { libc::kill(p, libc::SIGKILL) == 0 }
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::ProcEntry;
    use std::path::PathBuf;

    fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
        let p = libc::c_int::try_from(pid).ok()?;
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
        // SAFETY: the buffer is a properly sized and aligned proc_bsdinfo; the kernel writes at
        // most `size` bytes and returns how many it wrote.
        let n = unsafe {
            libc::proc_pidinfo(
                p,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast::<libc::c_void>(),
                size,
            )
        };
        if n != size {
            return None;
        }
        // SAFETY: the kernel filled the whole struct (n == size); it was zeroed before.
        Some(unsafe { info.assume_init() })
    }

    pub(super) fn exe_of(pid: u32) -> Option<PathBuf> {
        let p = libc::c_int::try_from(pid).ok()?;
        let cap = usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE).ok()?;
        let mut buf = vec![0u8; cap];
        let cap32 = u32::try_from(cap).ok()?;
        // SAFETY: `buf` is `cap` bytes long and the kernel writes at most `cap32` bytes.
        let n = unsafe { libc::proc_pidpath(p, buf.as_mut_ptr().cast::<libc::c_void>(), cap32) };
        let n = usize::try_from(n).ok().filter(|n| *n > 0 && *n <= cap)?;
        buf.truncate(n);
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_vec(buf)))
    }

    pub(super) fn list_processes() -> Option<Vec<ProcEntry>> {
        // SAFETY: a null buffer asks only for the current count.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let count = usize::try_from(count).ok().filter(|c| *c > 0)?;
        // Head room for processes started between the two calls.
        let mut pids = vec![0 as libc::c_int; count + 64];
        let bytes = libc::c_int::try_from(pids.len() * std::mem::size_of::<libc::c_int>()).ok()?;
        // SAFETY: `pids` holds `bytes` bytes of c_int slots; the kernel fills at most that many.
        let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast::<libc::c_void>(), bytes) };
        let n = usize::try_from(n).ok()?.min(pids.len());
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        let mut out = Vec::new();
        for &raw in &pids[..n] {
            let Ok(pid) = u32::try_from(raw) else {
                continue;
            };
            if pid == 0 {
                continue;
            }
            let Some(info) = bsd_info(pid) else { continue };
            if info.pbi_uid != me {
                continue;
            }
            let Some(exe) = exe_of(pid) else { continue };
            out.push(ProcEntry {
                pid,
                ppid: info.pbi_ppid,
                exe,
            });
        }
        Some(out)
    }

    pub(super) fn kill_now(pid: u32) -> bool {
        let Ok(p) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: kill(2) with a positive pid signals that one process; no memory is touched.
        unsafe { libc::kill(p, libc::SIGKILL) == 0 }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod os {
    use super::ProcEntry;
    use std::path::PathBuf;

    pub(super) fn exe_of(_pid: u32) -> Option<PathBuf> {
        None
    }

    pub(super) fn list_processes() -> Option<Vec<ProcEntry>> {
        None
    }

    pub(super) fn kill_now(_pid: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    include!("owned_cleanup_tests.rs");
}
