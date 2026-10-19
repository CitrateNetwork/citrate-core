// created: 2026-10-18 | branch: beta/k1a-gate0-harness | author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
// status: active (SCL-S1.3 fault-injection harness)
//
// Owned-process fixtures with marker-checked guards.
//
// Every process a fixture starts carries the fixture's marker in its own argv: a shell gets it
// as `$0`, and the long-lived leaf (`tail -f <dir>/anchor`) has it in the anchor path, because
// the anchor file lives in a directory named after the marker. Every process also appends its
// own pid to the fixture's pid file before it does anything else, so the guard knows every pid
// the fixture created, including descendants that were reparented after their parent died.
//
// The guard ([`OwnedFixture`]) kills and reaps on every path, including a panicking test body
// (it runs from `Drop`). It signals a pid only after checking that the pid's current command
// line still carries the marker, so a pid that was reused by an unrelated process is reported
// as foreign and left alone. After the recorded pids it sweeps once more for any process that
// carries the marker (pgrep), so nothing the fixture started can outlive the test.

#![allow(dead_code)]

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use citrate_core_kit::supervisor::SidecarSpec;

use super::timing::{wait_for, Deadline};

/// Every marker starts with this. The CI job checks `pgrep -f citrate-lcf-` after the suite:
/// any match is a process the suite left behind.
pub const SUITE_PREFIX: &str = "citrate-lcf-";

static NONCE: AtomicU64 = AtomicU64::new(0);

/// Resolve the first existing path among `candidates` (macOS and Linux keep coreutils in
/// different places). A missing tool makes the fixture inconclusive, never a pass.
pub fn resolve_bin(candidates: &[&str]) -> PathBuf {
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    panic!("INCONCLUSIVE: no fixture binary among {candidates:?}");
}

pub fn sh_bin() -> PathBuf {
    resolve_bin(&["/bin/sh", "/usr/bin/sh"])
}

pub fn tail_bin() -> PathBuf {
    resolve_bin(&["/usr/bin/tail", "/bin/tail"])
}

pub fn sleep_bin() -> PathBuf {
    resolve_bin(&["/bin/sleep", "/usr/bin/sleep"])
}

/// What the guard did when it cleaned up.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    /// Every pid the fixture recorded (pid file plus explicit records), deduplicated.
    pub recorded: Vec<u32>,
    /// Recorded pids that were alive and still carried the marker, and were sent SIGKILL.
    pub signalled: Vec<u32>,
    /// Recorded pids that were alive but no longer carried the marker. Never signalled.
    pub foreign: Vec<u32>,
    /// Marked processes found by the final sweep that were not in the record. Killed.
    pub swept: Vec<u32>,
    /// Marked processes still alive after cleanup (should always be empty).
    pub leftover: Vec<u32>,
}

/// An owned fixture directory plus the guard for every process started under it.
pub struct OwnedFixture {
    dir: PathBuf,
    marker: String,
    pidfile: PathBuf,
    anchor: PathBuf,
    extra: Mutex<Vec<u32>>,
    children: Mutex<Vec<Child>>,
    cleaned: Mutex<Option<CleanupReport>>,
}

impl OwnedFixture {
    /// A fresh fixture with a unique marker `citrate-lcf-<test pid>-<nonce>-<tag>`.
    pub fn new(tag: &str) -> Self {
        let nonce = NONCE.fetch_add(1, Ordering::SeqCst);
        let marker = format!("{SUITE_PREFIX}{}-{nonce}-{tag}", std::process::id());
        let dir = std::env::temp_dir().join(&marker);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            panic!(
                "INCONCLUSIVE: cannot create fixture dir {}: {e}",
                dir.display()
            );
        }
        let pidfile = dir.join("pids");
        let anchor = dir.join("anchor");
        for p in [&pidfile, &anchor] {
            if let Err(e) = std::fs::write(p, b"") {
                panic!("INCONCLUSIVE: cannot create {}: {e}", p.display());
            }
        }
        OwnedFixture {
            dir,
            marker,
            pidfile,
            anchor,
            extra: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            cleaned: Mutex::new(None),
        }
    }

    pub fn marker(&self) -> &str {
        &self.marker
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A path inside the fixture directory (crash records and the like).
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn sh_args(&self, script: &str) -> Vec<String> {
        vec![
            "-c".into(),
            script.into(),
            self.marker.clone(),
            self.pidfile.display().to_string(),
            self.anchor.display().to_string(),
        ]
    }

    /// argv for a long-lived leaf: records its pid, then becomes `tail -f <anchor>` (same pid,
    /// marker in the anchor path). Exits only when signalled.
    pub fn leaf_args(&self) -> Vec<String> {
        self.sh_args(&format!(
            "echo $$ >>\"$1\"; exec {} -f \"$2\"",
            tail_bin().display()
        ))
    }

    /// argv for a long-lived leaf that ignores SIGTERM (the ignore survives `exec`, and it is set
    /// before the pid is recorded, so a recorded child always ignores it), so a stop
    /// has to wait out its grace and escalate to SIGKILL. Keeps a teardown in progress long
    /// enough to race controls against it.
    pub fn stubborn_leaf_args(&self) -> Vec<String> {
        self.sh_args(&format!(
            "trap '' TERM; echo $$ >>\"$1\"; exec {} -f \"$2\"",
            tail_bin().display()
        ))
    }

    /// A supervised long-lived leaf that ignores SIGTERM.
    pub fn stubborn_leaf_spec(&self, name: &str) -> SidecarSpec {
        SidecarSpec::new(name, sh_bin(), self.stubborn_leaf_args())
    }

    /// A supervised long-lived leaf.
    pub fn leaf_spec(&self, name: &str) -> SidecarSpec {
        SidecarSpec::new(name, sh_bin(), self.leaf_args())
    }

    /// A supervised child that records its pid and exits at once with `code` (crash-loop fodder).
    pub fn crash_spec(&self, name: &str, code: u8) -> SidecarSpec {
        SidecarSpec::new(
            name,
            sh_bin(),
            self.sh_args(&format!("echo $$ >>\"$1\"; exit {code}")),
        )
    }

    /// A supervised child that records its pid, stays up for `secs` (a decimal like "0.15"),
    /// then exits with code 1. Its timer writes nothing to the output pipes, so a teardown in
    /// the middle leaves no reader held.
    pub fn crash_after_spec(&self, name: &str, secs: &str) -> SidecarSpec {
        SidecarSpec::new(
            name,
            sh_bin(),
            self.sh_args(&format!(
                "echo $$ >>\"$1\"; {} {secs} </dev/null >/dev/null 2>&1; exit 1",
                sleep_bin().display()
            )),
        )
    }

    /// A supervised child that starts one long-lived descendant (which inherits its output
    /// pipes), records both pids, then exits with code 3: a crash that leaves its output open.
    pub fn crash_with_descendant_spec(&self, name: &str) -> SidecarSpec {
        SidecarSpec::new(
            name,
            sh_bin(),
            self.sh_args(&format!(
                "echo $$ >>\"$1\"; {} -f \"$2\" & echo $! >>\"$1\"; exit 3",
                tail_bin().display()
            )),
        )
    }

    /// argv for a parent shell that starts `n` long-lived leaf descendants in the background,
    /// records every pid, then waits. The bug (e) reproduction shape.
    pub fn descendants_args(&self, n: u32) -> Vec<String> {
        self.sh_args(&format!(
            "echo $$ >>\"$1\"; i=0; while [ $i -lt {n} ]; do {} -f \"$2\" & echo $! >>\"$1\"; \
             i=$((i+1)); done; wait",
            tail_bin().display()
        ))
    }

    /// A supervised parent with `n` background descendants.
    pub fn descendants_spec(&self, name: &str, n: u32) -> SidecarSpec {
        SidecarSpec::new(name, sh_bin(), self.descendants_args(n))
    }

    /// Spawn a marked process directly (not under a supervisor). The guard owns the `Child` and
    /// reaps it on cleanup.
    pub fn spawn_direct(&self, args: Vec<String>) -> u32 {
        let child = Command::new(sh_bin())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("INCONCLUSIVE: fixture spawn failed: {e}"));
        let pid = child.id();
        self.children
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(child);
        pid
    }

    /// Record a pid the fixture did not learn through its pid file.
    pub fn record(&self, pid: u32) {
        self.extra
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(pid);
    }

    /// Every pid in the pid file, in the order the processes wrote them.
    pub fn pidfile_pids(&self) -> Vec<u32> {
        std::fs::read_to_string(&self.pidfile)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse::<u32>().ok())
            .collect()
    }

    /// Every recorded pid (pid file plus explicit records), deduplicated, in first-seen order.
    pub fn recorded_pids(&self) -> Vec<u32> {
        let mut out = self.pidfile_pids();
        out.extend(
            self.extra
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .copied(),
        );
        let mut seen = std::collections::HashSet::new();
        out.retain(|p| seen.insert(*p));
        out
    }

    /// Wait until the pid file holds at least `n` pids. Not reaching `n` is inconclusive.
    pub fn expect_pids(&self, n: usize, timeout: Duration) -> Vec<u32> {
        match wait_for(timeout, Duration::from_millis(10), || {
            let pids = self.pidfile_pids();
            (pids.len() >= n).then_some(pids)
        }) {
            Ok((pids, _)) => pids,
            Err(e) => panic!(
                "INCONCLUSIVE: fixture recorded {} of {n} pids within {:?}",
                self.pidfile_pids().len(),
                e.waited
            ),
        }
    }

    /// Whether `pid` is alive now and its command line carries this fixture's marker.
    pub fn is_marked_alive(&self, pid: u32) -> bool {
        pid_alive(pid) && command_line(pid).is_some_and(|c| c.contains(&self.marker))
    }

    /// Recorded pids that are alive and still carry the marker.
    pub fn live_recorded(&self) -> Vec<u32> {
        self.recorded_pids()
            .into_iter()
            .filter(|p| self.is_marked_alive(*p))
            .collect()
    }

    /// Every live process whose command line carries the marker (recorded or not).
    pub fn live_marked(&self) -> Vec<u32> {
        pgrep(&self.marker)
            .into_iter()
            .filter(|p| pid_alive(*p))
            .collect()
    }

    /// Kill and reap everything this fixture started. Idempotent: the first call does the work
    /// and later calls return the same report. Runs from `Drop` too, so a failing test cleans up.
    pub fn cleanup(&self) -> CleanupReport {
        let mut done = self.cleaned.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(r) = done.as_ref() {
            return r.clone();
        }
        let mut report = CleanupReport {
            recorded: self.recorded_pids(),
            ..CleanupReport::default()
        };
        for &pid in &report.recorded {
            if !pid_alive(pid) {
                continue;
            }
            if command_line(pid).is_some_and(|c| c.contains(&self.marker)) {
                kill(pid, libc::SIGKILL);
                report.signalled.push(pid);
            } else {
                report.foreign.push(pid);
            }
        }
        // Reap the children this guard spawned directly.
        for child in self
            .children
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter_mut()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Final sweep: anything still carrying the marker (a recorded pid that raced the pid
        // file, or a process that never got to write it). Repeat until none or the deadline.
        // A forked shell can be mid-exec (briefly invisible to pgrep) when the sweep looks, so
        // the sweep ends only after the marker has been absent for several looks in a row.
        let deadline = Deadline::after(Duration::from_secs(5));
        let mut quiet = 0;
        loop {
            let live: Vec<u32> = pgrep(&self.marker)
                .into_iter()
                .filter(|p| pid_alive(*p))
                .collect();
            if live.is_empty() {
                quiet += 1;
                if quiet >= 10 || deadline.expired() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            quiet = 0;
            if deadline.expired() {
                break;
            }
            for pid in live {
                if !report.recorded.contains(&pid) && !report.swept.contains(&pid) {
                    report.swept.push(pid);
                }
                kill(pid, libc::SIGKILL);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Wait for every signalled or swept pid to be gone (reaped by its parent, by init, or
        // by the supervisor that owns it).
        let targets: Vec<u32> = report
            .signalled
            .iter()
            .chain(report.swept.iter())
            .copied()
            .collect();
        let _ = wait_for(Duration::from_secs(5), Duration::from_millis(20), || {
            targets.iter().all(|p| !pid_alive(*p)).then_some(())
        });
        // A last sweep for anything that became visible only now (a shell that was mid-exec
        // during the sweep above); it is killed and counted as swept, not left over.
        let late: Vec<u32> = self.live_marked();
        for &pid in &late {
            if !report.recorded.contains(&pid) && !report.swept.contains(&pid) {
                report.swept.push(pid);
            }
            kill(pid, libc::SIGKILL);
        }
        let all: Vec<u32> = targets.into_iter().chain(late).collect();
        let _ = wait_for(Duration::from_secs(5), Duration::from_millis(20), || {
            all.iter().all(|p| !pid_alive(*p)).then_some(())
        });
        report.leftover = all.into_iter().filter(|p| pid_alive(*p)).collect();
        for p in self.live_marked() {
            if !report.leftover.contains(&p) {
                report.leftover.push(p);
            }
        }
        *done = Some(report.clone());
        report
    }

    /// Clean up and fail the test if any marked process is left.
    pub fn assert_clean(&self) -> CleanupReport {
        let r = self.cleanup();
        assert!(
            r.leftover.is_empty(),
            "fixture {} left processes behind: {r:?}",
            self.marker
        );
        r
    }
}

impl Drop for OwnedFixture {
    fn drop(&mut self) {
        let r = self.cleanup();
        if !r.leftover.is_empty() {
            // Never panic in Drop (a panic while unwinding aborts the run). Say it loudly; the
            // CI job's pgrep check fails the run.
            eprintln!(
                "lifecycle-faults: fixture {} left processes behind: {r:?}",
                self.marker
            );
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Whether `pid` names a live process. A zombie (exited, not yet reaped) is not alive.
pub fn pid_alive(pid: u32) -> bool {
    let Some(target) = positive_pid(pid) else {
        return false;
    };
    // SAFETY: kill(target, 0) on a positive pid checks existence and permission without
    // delivering a signal.
    let exists = unsafe { libc::kill(target, 0) } == 0
        || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    exists && !is_zombie(pid)
}

/// `pid` as a positive `pid_t`, or `None` for 0 (the caller's process group) and values above
/// `i32::MAX` (which would wrap to a negative pid: a group, or every process for -1).
pub fn positive_pid(pid: u32) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok().filter(|p| *p > 0)
}

fn is_zombie(pid: u32) -> bool {
    ps_field(pid, "stat=").is_some_and(|s| s.trim_start().starts_with('Z'))
}

/// The command line of `pid` as `ps` reports it, if the process exists.
pub fn command_line(pid: u32) -> Option<String> {
    ps_field(pid, "command=")
}

fn ps_field(pid: u32, field: &str) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", field, "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Pids whose full command line contains `pattern` (fixed string), excluding this process.
pub fn pgrep(pattern: &str) -> Vec<u32> {
    let out = match Command::new("pgrep")
        .args(["-f", pattern])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(e) => panic!("INCONCLUSIVE: pgrep unavailable: {e}"),
    };
    let me = std::process::id();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .filter(|p| *p != me)
        .collect()
}

pub fn kill(pid: u32, signal: i32) -> bool {
    let Some(target) = positive_pid(pid) else {
        return false;
    };
    // SAFETY: kill(2) with a positive pid and a signal number has no memory effects. Callers
    // check the marker first, so only fixture processes are signalled.
    unsafe { libc::kill(target, signal) == 0 }
}
