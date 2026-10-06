//! SCL-S0.3: the managed browser does not outlive Hermes.
//!
//! The Hermes sidecar starts its managed browser itself, so the browser is a child of the sidecar
//! and core never spawns it. When the sidecar ends without stopping its browser (core kills the
//! sidecar after its stop grace, the sidecar crashes, or the app itself was killed while the
//! sidecar ran), the browser would be left running with its temporary profile. This module lets
//! core clean that up from facts core observed itself:
//!
//! 1. **Observe** ([`watch`]): while core's supervisor holds the sidecar as its own unreaped child
//!    (so the pid names that child, not a reused id), core lists the sidecar's direct children and
//!    keeps a record of the ones that are a managed browser: pid, start time, boot identity,
//!    executable path, process group and profile folder. The record is written by core, into
//!    core's own app data, and only from what the OS reports about the sidecar's children; nothing
//!    the sidecar says is used.
//! 2. **Clean** ([`apply`]): after the sidecar has ended (every path, including the hard kill),
//!    before the next sidecar start, and at app launch, core stops every recorded browser that is
//!    still the same process (pid, start time, boot identity, executable path all equal, and its
//!    command line still names the recorded profile with a debugging port), then removes the
//!    recorded profile folder.
//!
//! A record is a claim to re-check, not authority: [`apply`] signals only a process whose live
//! identity equals the record and whose command line still has the managed-browser shape, and
//! removes only a folder named like a managed-browser profile directly inside this user's temporary
//! folder. Residual: a browser started and orphaned within one observe interval of a sidecar crash
//! (not a stop or kill by core, which observe right before signalling) is not recorded.
//!
//! Unix only (macOS, Linux). On Windows [`watch`] returns `None` and [`apply`] does nothing; the
//! Windows containment work (SCL S10, Job objects) covers it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::supervisor::ChildWatch;
use serde::{Deserialize, Serialize};

/// The record file name inside the Hermes data folder.
pub const RECORD_FILE: &str = "browser-owned.json";

/// How often the running sidecar's children are listed.
pub const OBSERVE_INTERVAL: Duration = Duration::from_secs(2);

/// The profile folder prefix the runtime's managed browser uses (`citrate-browser-…`).
const PROFILE_PREFIX: &str = "citrate-browser-";

/// The command-line flag every managed browser is started with.
const DEBUG_PORT_FLAG: &str = "--remote-debugging-port=";
const PROFILE_FLAG: &str = "--user-data-dir=";

/// One managed browser core saw as a child of its sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserRecord {
    pub pid: u32,
    /// The OS start time of the process (macOS: µs since the epoch; Linux: clock ticks since boot).
    pub start: u64,
    /// The boot this start time belongs to (Linux `boot_id`; macOS boot time).
    pub boot: String,
    pub exe: PathBuf,
    /// The process group the browser leads (equal to `pid` when it leads its own group).
    pub pgid: u32,
    pub profile: PathBuf,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecordFile {
    browsers: Vec<BrowserRecord>,
}

/// What [`apply`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Recorded browsers that were still running and were stopped.
    pub stopped: usize,
    /// Recorded profile folders that were removed.
    pub profiles_removed: usize,
    /// Recorded browsers that are still running after the stop attempt (kept in the record).
    pub still_running: usize,
}

/// The supervisor hooks for the Hermes sidecar: observe its managed-browser children while it
/// runs, clean them up after it ends. `None` where this is not supported (Windows).
pub fn watch(record_path: PathBuf) -> Option<ChildWatch> {
    if !cfg!(unix) {
        return None;
    }
    let observe_path = record_path.clone();
    Some(ChildWatch {
        interval: OBSERVE_INTERVAL,
        observe: Arc::new(move |pid| observe(&observe_path, pid)),
        ended: Arc::new(move || {
            let _ = apply(&record_path);
        }),
    })
}

/// Record the managed browsers that are direct children of `parent` (core's own live sidecar).
/// Entries already recorded are kept until [`apply`] has dealt with them.
pub fn observe(record_path: &Path, parent: u32) {
    let found: Vec<BrowserRecord> = os::children(parent)
        .into_iter()
        .filter_map(browser_record)
        .collect();
    let mut file = read(record_path);
    let before = file.browsers.len();
    // Drop entries that are gone for good (no process with that identity, no profile left),
    // e.g. a browser the sidecar itself stopped and cleaned up.
    file.browsers
        .retain(|r| still_same_process(r) || r.profile.exists());
    let mut changed = file.browsers.len() != before;
    for r in found {
        if !file.browsers.contains(&r) {
            file.browsers.push(r);
            changed = true;
        }
    }
    if !changed {
        return;
    }
    if file.browsers.is_empty() {
        let _ = std::fs::remove_file(record_path);
    } else {
        let _ = write(record_path, &file);
    }
}

/// Stop every recorded browser that is still the same process and remove its profile folder.
/// The record keeps only browsers that could not be confirmed stopped.
pub fn apply(record_path: &Path) -> ApplyReport {
    let file = read(record_path);
    let mut report = ApplyReport::default();
    if file.browsers.is_empty() {
        let _ = std::fs::remove_file(record_path);
        return report;
    }
    let mut keep = Vec::new();
    for r in file.browsers {
        if still_same_process(&r) {
            os::stop(&r);
            if wait_gone(&r, Duration::from_secs(3)) {
                report.stopped += 1;
            } else {
                report.still_running += 1;
                keep.push(r);
                continue;
            }
        } else if profile_in_use(&r) {
            // Not the recorded process, but whatever runs under that pid uses this profile:
            // leave both alone.
            continue;
        }
        if remove_profile(&r.profile) {
            report.profiles_removed += 1;
        }
    }
    if keep.is_empty() {
        let _ = std::fs::remove_file(record_path);
    } else {
        let _ = write(record_path, &RecordFile { browsers: keep });
    }
    report
}

/// A record for `pid` when it is a managed browser: its command line has a debugging port and a
/// profile folder that [`profile_ok`] accepts.
fn browser_record(pid: u32) -> Option<BrowserRecord> {
    let info = os::info(pid)?;
    let profile = managed_profile(&info.argv)?;
    Some(BrowserRecord {
        pid,
        start: info.start,
        boot: os::boot_id()?,
        exe: info.exe,
        pgid: info.pgid,
        profile,
    })
}

/// The profile folder named by a managed browser's command line, if it has that shape.
fn managed_profile(argv: &[String]) -> Option<PathBuf> {
    if !argv.iter().any(|a| a.starts_with(DEBUG_PORT_FLAG)) {
        return None;
    }
    let profile = argv
        .iter()
        .find_map(|a| a.strip_prefix(PROFILE_FLAG))
        .map(PathBuf::from)?;
    profile_ok(&profile).then_some(profile)
}

/// Whether `p` is shaped like a managed-browser profile: an absolute path whose last component
/// starts with `citrate-browser-`, directly inside this user's temporary folder.
pub fn profile_ok(p: &Path) -> bool {
    if !p.is_absolute() {
        return false;
    }
    let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !name.starts_with(PROFILE_PREFIX) || name.len() == PROFILE_PREFIX.len() {
        return false;
    }
    let Some(parent) = p.parent() else {
        return false;
    };
    let tmp = std::env::temp_dir();
    let canon = |q: &Path| std::fs::canonicalize(q).unwrap_or_else(|_| q.to_path_buf());
    canon(parent) == canon(&tmp)
}

/// Whether the recorded process is still running as the same process with the same shape.
fn still_same_process(r: &BrowserRecord) -> bool {
    let Some(info) = os::info(r.pid) else {
        return false;
    };
    let Some(boot) = os::boot_id() else {
        return false;
    };
    info.start == r.start
        && boot == r.boot
        && info.exe == r.exe
        && managed_profile(&info.argv).as_deref() == Some(r.profile.as_path())
}

/// Whether the process now running under the recorded pid names the recorded profile.
fn profile_in_use(r: &BrowserRecord) -> bool {
    os::info(r.pid)
        .is_some_and(|i| managed_profile(&i.argv).as_deref() == Some(r.profile.as_path()))
}

fn wait_gone(r: &BrowserRecord, within: Duration) -> bool {
    let deadline = std::time::Instant::now() + within;
    loop {
        if !still_same_process(r) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Remove a recorded profile folder when it still has the managed-profile shape and is a real
/// folder (not a link). A few tries: helpers may hold it for a moment after the browser ends.
fn remove_profile(p: &Path) -> bool {
    if !profile_ok(p) {
        return false;
    }
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {}
        _ => return false,
    }
    for _ in 0..20 {
        if std::fs::remove_dir_all(p).is_ok() || !p.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn read(path: &Path) -> RecordFile {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Write the record owner-only, through a temporary file and a rename.
fn write(path: &Path, file: &RecordFile) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        use std::io::Write;
        let mut f = opts.open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// What the OS reports about one process.
#[derive(Debug, Clone)]
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
struct ProcInfo {
    start: u64,
    exe: PathBuf,
    pgid: u32,
    argv: Vec<String>,
}

#[cfg(target_os = "macos")]
mod os {
    use super::{BrowserRecord, ProcInfo};
    use std::path::PathBuf;

    pub fn children(parent: u32) -> Vec<u32> {
        let Ok(ppid) = libc::pid_t::try_from(parent) else {
            return Vec::new();
        };
        let mut buf: Vec<libc::pid_t> = vec![0; 256];
        loop {
            let bytes = (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
            // SAFETY: the buffer holds `bytes` bytes of pid_t; the call writes at most that many
            // and returns how many pids it wrote.
            let n = unsafe { libc::proc_listchildpids(ppid, buf.as_mut_ptr().cast(), bytes) };
            if n <= 0 {
                return Vec::new();
            }
            let n = n as usize;
            if n < buf.len() {
                return buf[..n]
                    .iter()
                    .filter(|p| **p > 0)
                    .map(|p| *p as u32)
                    .collect();
            }
            if buf.len() >= 65536 {
                return Vec::new();
            }
            let len = buf.len() * 2;
            buf.resize(len, 0);
        }
    }

    pub fn info(pid: u32) -> Option<ProcInfo> {
        let p = libc::c_int::try_from(pid).ok().filter(|p| *p > 0)?;
        // SAFETY: zeroed is a valid proc_bsdinfo (plain integers and byte arrays).
        let mut bsd: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: the buffer is one proc_bsdinfo of `size` bytes.
        let got = unsafe {
            libc::proc_pidinfo(
                p,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut bsd as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if got != size || bsd.pbi_pid != pid {
            return None;
        }
        let start = bsd
            .pbi_start_tvsec
            .checked_mul(1_000_000)?
            .checked_add(bsd.pbi_start_tvusec)?;
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer holds `path.len()` bytes.
        let n = unsafe { libc::proc_pidpath(p, path.as_mut_ptr().cast(), path.len() as u32) };
        if n <= 0 {
            return None;
        }
        path.truncate(n as usize);
        let exe = PathBuf::from(String::from_utf8(path).ok()?);
        Some(ProcInfo {
            start,
            exe,
            pgid: bsd.pbi_pgid,
            argv: argv(p)?,
        })
    }

    /// The process's arguments from `KERN_PROCARGS2`: argc, the exec path, padding, then argv.
    fn argv(pid: libc::c_int) -> Option<Vec<String>> {
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX, 0];
        let mut argmax: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        // SAFETY: reads one c_int into `argmax`.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut argmax as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || argmax <= 0 {
            return None;
        }
        let mut buf = vec![0u8; argmax as usize];
        let mut size = buf.len();
        mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        // SAFETY: the buffer holds `size` bytes; the kernel writes at most that many.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || size < std::mem::size_of::<libc::c_int>() {
            return None;
        }
        buf.truncate(size);
        let argc = i32::from_ne_bytes(buf[..4].try_into().ok()?);
        let mut rest = &buf[4..];
        // Skip the exec path and the NUL padding after it.
        let end = rest.iter().position(|b| *b == 0)?;
        rest = &rest[end..];
        let start = rest.iter().position(|b| *b != 0)?;
        rest = &rest[start..];
        let mut out = Vec::new();
        for part in rest.split(|b| *b == 0).take(usize::try_from(argc).ok()?) {
            out.push(String::from_utf8_lossy(part).into_owned());
        }
        Some(out)
    }

    pub fn boot_id() -> Option<String> {
        let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        // SAFETY: zeroed is a valid timeval.
        let mut tv: libc::timeval = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<libc::timeval>();
        // SAFETY: reads one timeval into `tv`.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut tv as *mut libc::timeval).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (rc == 0).then(|| format!("{}.{}", tv.tv_sec, tv.tv_usec))
    }

    pub fn stop(r: &BrowserRecord) {
        super::unix_stop(r)
    }
}

#[cfg(target_os = "linux")]
mod os {
    use super::{BrowserRecord, ProcInfo};

    /// `ppid`, `pgrp` and `starttime` from `/proc/<pid>/stat` (fields 4, 5 and 22; the command
    /// name in field 2 may contain spaces and parentheses, so fields are counted after the last `)`).
    fn stat(pid: u32) -> Option<(u32, u32, u64)> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after = &s[s.rfind(')')? + 1..];
        let f: Vec<&str> = after.split_whitespace().collect();
        // f[0] = state (field 3), so field n is f[n - 3].
        let ppid = f.get(1)?.parse().ok()?;
        let pgrp = f.get(2)?.parse().ok()?;
        let start = f.get(19)?.parse().ok()?;
        Some((ppid, pgrp, start))
    }

    pub fn children(parent: u32) -> Vec<u32> {
        let Ok(rd) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        rd.flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()))
            .filter(|pid| stat(*pid).is_some_and(|(ppid, _, _)| ppid == parent))
            .collect()
    }

    pub fn info(pid: u32) -> Option<ProcInfo> {
        if pid == 0 {
            return None;
        }
        let (_, pgid, start) = stat(pid)?;
        let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let argv = raw
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        // Re-read the start time: the pid must not have changed hands while we read.
        let (_, _, again) = stat(pid)?;
        (again == start).then_some(ProcInfo {
            start,
            exe,
            pgid,
            argv,
        })
    }

    pub fn boot_id() -> Option<String> {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn stop(r: &BrowserRecord) {
        super::unix_stop(r)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod os {
    use super::{BrowserRecord, ProcInfo};

    pub fn children(_parent: u32) -> Vec<u32> {
        Vec::new()
    }
    pub fn info(_pid: u32) -> Option<ProcInfo> {
        None
    }
    pub fn boot_id() -> Option<String> {
        None
    }
    pub fn stop(_r: &BrowserRecord) {}
}

/// SIGKILL the recorded browser: its whole process group when it leads one (the runtime starts it
/// in a group of its own, so its helpers go with it), else the process alone. Called only right
/// after [`still_same_process`] confirmed the identity.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn unix_stop(r: &BrowserRecord) {
    let Ok(pid) = libc::pid_t::try_from(r.pid) else {
        return;
    };
    if pid <= 1 || r.pid == std::process::id() {
        return;
    }
    // SAFETY: getpgid/kill are plain syscalls on integer ids.
    unsafe {
        if r.pgid == r.pid && libc::getpgid(pid) == pid {
            libc::kill(-pid, libc::SIGKILL);
        } else {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    include!("owned_browser_tests.rs");
}
