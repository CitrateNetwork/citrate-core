// created: 2026-10-18 | branch: beta/k1a-gate0-harness | author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
// status: active (SCL-S1.3; grows with every SCL sprint, becomes the required check in SCL-S13.1)
//
// The lifecycle fault-injection suite. Each fixture starts real OS processes through the public
// kit API, records every pid it creates, and cleans every one of them up even when its property
// fails (the `OwnedFixture` guard runs from `Drop`). A fixture that cannot acquire what it needs
// panics with `INCONCLUSIVE`, which fails the run: a missing observation is never a pass.
//
// Every test here is listed in `lifecycle_faults.manifest`; `manifest_lists_every_fixture`
// keeps the two in step.
//
// Unix only for now (macOS and Linux). The Windows lane joins in SCL-S13.1.

#![cfg(unix)]

mod lifecycle;

use std::time::Duration;

use citrate_core_kit::supervisor::{
    BackoffPolicy, ChildWatch, CommitRefused, ControlResult, ControlToken, DeadlinePublisher,
    DeadlineState, Desired, HealthCheck, Phase, PublicationRefused, Receipt, SidecarSpec,
    Supervisor, SupervisorConfig, SupervisorState, DEADLINE_RECORDS,
};
use lifecycle::fixture::{
    pgrep, pid_alive, pid_reaped, sh_bin, sleep_bin, OwnedFixture, SUITE_PREFIX,
};
use lifecycle::timing::{hold_for, sample_over, wait_for, Deadline};

/// How long a fixture waits to acquire what it needs before it gives up as INCONCLUSIVE. Long,
/// because hosts running many suites at once can take seconds to schedule a fork and exec.
const ACQUIRE: Duration = Duration::from_secs(15);

fn fast_backoff(max_retries: u32) -> BackoffPolicy {
    BackoffPolicy {
        base_delay: Duration::from_millis(20),
        multiplier: 2,
        max_delay: Duration::from_millis(80),
        max_retries,
    }
}

/// A receipt read that rides out a briefly contended cell: `Unknown` (the bounded read found
/// the cell busy) is retried for up to a second, so assertions see the real receipt.
/// An activation step with nothing to do (the tests that use it are about ticket currency).
fn activate_nothing() -> Result<(), std::convert::Infallible> {
    Ok(())
}

fn receipt_of(sup: &Supervisor, token: ControlToken) -> Receipt {
    let deadline = Deadline::after(Duration::from_secs(1));
    loop {
        match sup.receipt(token) {
            Receipt::Unknown { .. } if !deadline.expired() => {
                std::thread::sleep(Duration::from_millis(2))
            }
            r => return r,
        }
    }
}

/// Wait until the service is Running and its child has really started executing (it wrote its
/// pid to the fixture's pid file). A control raced against a child that has not run its first
/// line yet would observe nothing about it, so every fixture waits for both.
fn running_pid(sup: &Supervisor, fx: &OwnedFixture) -> u32 {
    let st = sup.wait_until(|s| *s == SupervisorState::Running, ACQUIRE);
    let pid = match (st.state, st.pid) {
        (SupervisorState::Running, Some(pid)) => pid,
        (state, pid) => panic!("INCONCLUSIVE: service not Running with a pid: {state:?} {pid:?}"),
    };
    let recorded = wait_for(ACQUIRE, Duration::from_millis(5), || {
        fx.pidfile_pids().contains(&pid).then_some(())
    });
    assert!(
        recorded.is_ok(),
        "INCONCLUSIVE: child {pid} never recorded itself (pid file {:?})",
        fx.pidfile_pids()
    );
    pid
}

// ---------------------------------------------------------------------------
// Harness positive controls: the guard and the timing helpers do what they claim.
// ---------------------------------------------------------------------------

/// The guard kills and reaps every recorded pid when the test body panics, including
/// descendants that were reparented after their parent was killed first.
#[test]
fn harness_guard_cleans_up_after_a_panicking_body() {
    let mut recorded = Vec::new();
    let mut marker = String::new();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let fx = OwnedFixture::new("panic");
        marker = fx.marker().to_string();
        let parent = fx.spawn_direct(fx.descendants_args(2));
        let leaf = fx.spawn_direct(fx.leaf_args());
        let pids = fx.expect_pids(4, ACQUIRE);
        recorded = pids.clone();
        assert!(pids.contains(&parent) && pids.contains(&leaf));
        // Kill the parent first so its two descendants are reparented away from us.
        lifecycle::fixture::kill(parent, libc::SIGKILL);
        panic!("deliberate failure inside the fixture body");
    }));
    assert!(outcome.is_err(), "the body panicked as intended");
    assert_eq!(
        recorded.len(),
        4,
        "parent, two descendants and the leaf were recorded"
    );
    let gone = wait_for(Duration::from_secs(5), Duration::from_millis(20), || {
        recorded.iter().all(|p| !pid_alive(*p)).then_some(())
    });
    assert!(
        gone.is_ok(),
        "every recorded pid is gone after the guard ran: {recorded:?}"
    );
    assert!(
        pgrep(&marker).is_empty(),
        "no process carries the marker any more"
    );
}

/// The guard never signals a recorded pid whose command line does not carry its marker (a pid
/// that the OS reused for an unrelated process).
#[test]
fn harness_guard_never_signals_an_unmarked_pid() {
    // An unmarked process, started and owned by this test, not by the fixture.
    let mut foreign = std::process::Command::new(sleep_bin())
        .arg("30")
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: cannot spawn the foreign process: {e}"));
    let foreign_pid = foreign.id();
    let report = {
        let fx = OwnedFixture::new("foreign");
        fx.spawn_direct(fx.leaf_args());
        fx.expect_pids(1, ACQUIRE);
        fx.record(foreign_pid);
        fx.assert_clean()
    };
    let still_alive = pid_alive(foreign_pid);
    let _ = foreign.kill();
    let _ = foreign.wait();
    assert!(still_alive, "the unmarked process was left alone");
    assert_eq!(
        report.foreign,
        vec![foreign_pid],
        "and reported as foreign: {report:?}"
    );
    assert!(!report.signalled.contains(&foreign_pid));
}

/// A marked process that never wrote the pid file is still found and killed by the sweep.
#[test]
fn harness_sweep_finds_marked_processes_missing_from_the_record() {
    let fx = OwnedFixture::new("sweep");
    // A marked process that never records itself and is not the guard's own child: its parent
    // starts it in the background and exits, so it is reparented away from this test.
    let launcher = fx.spawn_direct(vec![
        "-c".into(),
        format!(
            "{} -f \"$1\" >/dev/null 2>&1 & exit 0",
            lifecycle::fixture::tail_bin().display()
        ),
        fx.marker().to_string(),
        fx.path("anchor").display().to_string(),
    ]);
    // The launcher shell carries the marker too; wait for the background process it left.
    let seen = wait_for(ACQUIRE, Duration::from_millis(10), || {
        fx.live_marked().into_iter().find(|p| *p != launcher)
    });
    let unrecorded = match seen {
        Ok((pid, _)) => pid,
        Err(_) => panic!("INCONCLUSIVE: the unrecorded marked process never appeared"),
    };
    assert!(fx.recorded_pids().is_empty(), "nothing was recorded");
    let report = fx.assert_clean();
    assert!(report.swept.contains(&unrecorded), "{report:?}");
    assert!(!pid_alive(unrecorded));
}

/// The harness never signals pid 0 (this test's own process group) or a pid that would wrap
/// negative, whatever a pid file or a record contains.
#[test]
fn harness_never_signals_pid_zero_or_a_wrapping_pid() {
    use lifecycle::fixture::{kill, positive_pid};
    assert_eq!(positive_pid(0), None);
    assert_eq!(positive_pid(u32::MAX), None);
    assert_eq!(positive_pid(i32::MAX as u32 + 1), None);
    assert_eq!(
        positive_pid(std::process::id()),
        i32::try_from(std::process::id()).ok()
    );
    // Signal 0 only probes, so a broken guard here could not hurt the test runner.
    assert!(!kill(0, 0));
    assert!(!kill(u32::MAX, 0));
    assert!(!pid_alive(0) && !pid_alive(u32::MAX));
    // A recorded 0 is skipped by cleanup, never signalled.
    let fx = OwnedFixture::new("pidzero");
    fx.record(0);
    fx.record(u32::MAX);
    let report = fx.assert_clean();
    assert!(
        report.signalled.is_empty() && report.foreign.is_empty(),
        "{report:?}"
    );
}

/// An exited child that nobody has waited for is not reaped. The check the supervisor fixtures
/// use for "Stop, Drop or a crash reaped the child" must fail on it, and cleanup must report it
/// as left behind; once it is waited for, both pass.
#[test]
fn harness_unreaped_child_fails_the_reaped_check() {
    let fx = OwnedFixture::new("zombie");
    let mut child = std::process::Command::new(sh_bin())
        .args(["-c", "exit 0", fx.marker()])
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: cannot spawn the child: {e}"));
    let pid = child.id();
    // It exits at once; until we wait for it, it stays a zombie of this process.
    let exited = wait_for(ACQUIRE, Duration::from_millis(5), || {
        (!pid_alive(pid)).then_some(())
    });
    let reaped_while_unwaited = pid_reaped(pid);
    fx.record(pid);
    let report = fx.cleanup();
    let _ = child.wait();
    let reaped_after_wait = pid_reaped(pid);
    assert!(exited.is_ok(), "INCONCLUSIVE: the child never exited");
    assert!(
        !reaped_while_unwaited,
        "an exited, unwaited child is not reported as reaped"
    );
    assert!(
        report.leftover.contains(&pid),
        "cleanup reports the unreaped child as left behind: {report:?}"
    );
    assert!(reaped_after_wait, "once waited for, the child is reaped");
}

/// Cleanup kills every recorded marked pid on its own. With the sweep turned off, a parent's
/// background descendants (recorded through the pid file, and not children of this test) are
/// still signalled and gone, and nothing is left over. Without this control the final sweep
/// would hide a guard that records pids but never kills them.
#[test]
fn harness_recorded_pids_are_killed_without_the_sweep() {
    let fx = OwnedFixture::without_sweep("nosweep");
    let parent = fx.spawn_direct(fx.descendants_args(2));
    let pids = fx.expect_pids(3, ACQUIRE);
    let descendants: Vec<u32> = pids.iter().copied().filter(|p| *p != parent).collect();
    if descendants.len() != 2 {
        panic!("INCONCLUSIVE: expected two recorded descendants, got {pids:?}");
    }
    let report = fx.cleanup();
    // Nothing may outlive the test, whatever the result: kill any survivor before asserting.
    let survivors = fx.live_marked();
    for &p in &survivors {
        lifecycle::fixture::kill(p, libc::SIGKILL);
    }
    assert!(
        survivors.is_empty(),
        "recorded descendants survived a cleanup without the sweep: {survivors:?} ({report:?})"
    );
    for d in &descendants {
        assert!(
            report.signalled.contains(d),
            "{d} was signalled: {report:?}"
        );
        assert!(!pid_alive(*d), "{d} is gone");
    }
    assert!(
        report.swept.is_empty() && report.leftover.is_empty(),
        "{report:?}"
    );
}

/// The harness's pgrep matches the marker as a fixed string, not as the regular expression
/// `pgrep -f` takes: a `.` in a marker matches only a `.`, so a process whose command line
/// differs only there is never counted (or signalled) as the fixture's.
#[test]
fn harness_pgrep_matches_the_marker_as_a_fixed_string() {
    let fx = OwnedFixture::new("dot.ok");
    let decoy = fx.marker().replace("dot.ok", "dotXok");
    assert_ne!(decoy, fx.marker());
    let leaf = fx.spawn_direct(fx.leaf_args());
    fx.expect_pids(1, ACQUIRE);
    // The decoy is one process, the guard's own child (killed and reaped at cleanup): the shell
    // execs `tail -f` on a file whose path carries the decoy string but not the marker.
    let decoy_anchor = std::env::temp_dir().join(format!("{decoy}-anchor"));
    if let Err(e) = std::fs::write(&decoy_anchor, b"") {
        panic!("INCONCLUSIVE: cannot create the decoy anchor: {e}");
    }
    let decoy_pid = fx.spawn_direct(vec![
        "-c".into(),
        format!(
            "exec {} -f \"$1\"",
            lifecycle::fixture::tail_bin().display()
        ),
        decoy.clone(),
        decoy_anchor.display().to_string(),
    ]);
    let seen = wait_for(ACQUIRE, Duration::from_millis(10), || {
        pgrep(&decoy).contains(&decoy_pid).then_some(())
    });
    assert!(seen.is_ok(), "INCONCLUSIVE: the decoy never appeared");
    // Everything the decoy started, once it has settled into its long-lived process (it either
    // exec'd that process itself or forked it).
    let tail = lifecycle::fixture::tail_bin().display().to_string();
    let settled = wait_for(ACQUIRE, Duration::from_millis(10), || {
        let kids = lifecycle::fixture::children_of(decoy_pid);
        let execd = lifecycle::fixture::command_line(decoy_pid).is_some_and(|c| c.contains(&tail));
        (!kids.is_empty() || execd).then_some(kids)
    });
    let forked = match settled {
        Ok((kids, _)) => kids,
        Err(_) => panic!("INCONCLUSIVE: the decoy never settled"),
    };
    let matched = pgrep(fx.marker());
    assert!(
        matched.contains(&leaf),
        "the marked leaf matches: {matched:?}"
    );
    assert!(
        !matched.contains(&decoy_pid),
        "a `.` in the marker matched another character: {decoy_pid} in {matched:?}"
    );
    assert!(!fx.is_marked_alive(decoy_pid));
    fx.assert_clean();
    // Nothing the decoy started outlives the test.
    let gone = wait_for(Duration::from_secs(5), Duration::from_millis(20), || {
        forked.iter().all(|p| !pid_alive(*p)).then_some(())
    });
    let leaked: Vec<u32> = forked.iter().copied().filter(|p| pid_alive(*p)).collect();
    for &p in &leaked {
        lifecycle::fixture::kill(p, libc::SIGKILL);
    }
    let _ = std::fs::remove_file(&decoy_anchor);
    assert!(gone.is_ok(), "the decoy left {leaked:?} behind");
    assert!(pid_reaped(decoy_pid), "the decoy itself was reaped");
    assert!(
        pgrep(&decoy).is_empty(),
        "no process carries the decoy string"
    );
}

/// Waits and observation windows are measured on the monotonic clock and never end early.
#[test]
fn harness_timing_windows_are_monotonic_and_never_short() {
    let held = hold_for(Duration::from_millis(120));
    assert!(held >= Duration::from_millis(120), "held {held:?}");

    let timed_out = wait_for(
        Duration::from_millis(150),
        Duration::from_millis(40),
        || None::<()>,
    );
    match timed_out {
        Err(e) => assert!(
            e.waited >= Duration::from_millis(150),
            "waited {:?}",
            e.waited
        ),
        Ok(_) => panic!("a condition that never holds cannot be met"),
    }

    let mut n = 0;
    let met = wait_for(Duration::from_secs(5), Duration::from_millis(5), || {
        n += 1;
        (n >= 3).then_some(n)
    });
    assert!(matches!(met, Ok((3, d)) if d < Duration::from_secs(5)));

    let samples = sample_over(Duration::from_millis(200), Duration::from_millis(30), || ());
    let last = samples.last().map(|(t, _)| *t).unwrap_or_default();
    assert!(
        last >= Duration::from_millis(200),
        "the last sample is at the end of the window"
    );
    assert!(
        samples.windows(2).all(|w| w[0].0 <= w[1].0),
        "offsets are monotonic"
    );

    let d = Deadline::after(Duration::from_millis(50));
    assert!(!d.expired() && d.remaining() <= Duration::from_millis(50));
}

// ---------------------------------------------------------------------------
// Supervisor positive controls (green on the current supervisor). Each is the positive half of
// a planned negative fixture (SPEC-001 "Acceptance and empirical evidence").
// ---------------------------------------------------------------------------

/// A fresh Start creates exactly one incarnation, and no other appears over a 2 s window.
/// (Positive control for bug (a), "Stop idempotence / no respawn".)
#[test]
fn pc_fresh_start_creates_exactly_one_incarnation() {
    let fx = OwnedFixture::new("one-start");
    let sup = Supervisor::start(SupervisorConfig::new(
        fx.leaf_spec("lcf-one-start"),
        fx.path("crash.jsonl"),
    ))
    .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = running_pid(&sup, &fx);
    fx.expect_pids(1, ACQUIRE);
    let samples = sample_over(Duration::from_secs(2), Duration::from_millis(100), || {
        fx.pidfile_pids().len()
    });
    assert!(
        samples.iter().all(|(_, n)| *n == 1),
        "one incarnation only: {samples:?}"
    );
    assert_eq!(fx.pidfile_pids(), vec![pid]);
    assert_eq!(sup.status().restarts, 0);
    drop(sup);
    assert!(
        pid_reaped(pid),
        "dropping the supervisor ends and reaps the child"
    );
    fx.assert_clean();
}

/// One Stop ends the child, reaps it, and leaves the service Off with no new child for 2 s.
#[test]
fn pc_single_stop_ends_the_child_and_stays_off() {
    let fx = OwnedFixture::new("one-stop");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-one-stop"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = running_pid(&sup, &fx);
    sup.stop();
    let st = sup.status();
    assert_eq!(st.state, SupervisorState::Off);
    assert_eq!(st.pid, None);
    assert!(pid_reaped(pid), "the child is reaped when Stop returns");
    let samples = sample_over(Duration::from_secs(2), Duration::from_millis(100), || {
        (fx.pidfile_pids().len(), sup.status().state)
    });
    assert!(
        samples
            .iter()
            .all(|(_, (n, s))| *n == 1 && *s == SupervisorState::Off),
        "no respawn after one Stop: {samples:?}"
    );
    drop(sup);
    fx.assert_clean();
}

/// An external kill of the child is a crash: a new, distinct incarnation appears and the old
/// one is gone. (Positive control: the supervisor's ordinary repair path still works.)
#[test]
fn pc_crash_respawns_a_distinct_incarnation() {
    let fx = OwnedFixture::new("respawn");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-respawn"), fx.path("crash.jsonl"));
    cfg.backoff = fast_backoff(5);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let first = running_pid(&sup, &fx);
    fx.expect_pids(1, ACQUIRE);
    lifecycle::fixture::kill(first, libc::SIGKILL);
    let pids = fx.expect_pids(2, ACQUIRE);
    assert_ne!(pids[0], pids[1], "a distinct incarnation");
    let second = wait_for(ACQUIRE, Duration::from_millis(10), || {
        let st = sup.status();
        (st.state == SupervisorState::Running && st.pid == Some(pids[1])).then_some(())
    });
    assert!(
        second.is_ok(),
        "the new incarnation is the one reported Running"
    );
    assert!(pid_reaped(first), "the crashed incarnation was reaped");
    assert!(sup.status().restarts >= 1);
    drop(sup);
    fx.assert_clean();
}

/// A crash loop is bounded: initial run plus `max_retries` respawns, then Failed, and no
/// further child for 1 s. (Positive control for the fork-bomb bound the SC2 work must keep.)
#[test]
fn pc_crash_loop_reaches_failed_after_the_retry_budget() {
    let fx = OwnedFixture::new("crash-loop");
    let mut cfg = SupervisorConfig::new(fx.crash_spec("lcf-crash-loop", 3), fx.path("crash.jsonl"));
    cfg.backoff = fast_backoff(2);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let st = sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(10));
    assert_eq!(st.state, SupervisorState::Failed);
    hold_for(Duration::from_secs(1));
    assert_eq!(
        fx.pidfile_pids().len(),
        3,
        "first run plus two retries, then nothing"
    );
    drop(sup);
    fx.assert_clean();
}

/// Every process the suite starts carries the suite prefix the CI job's pgrep check uses.
#[test]
fn pc_fixture_processes_carry_the_suite_prefix() {
    let fx = OwnedFixture::new("prefix");
    assert!(fx.marker().starts_with(SUITE_PREFIX));
    let pid = fx.spawn_direct(fx.leaf_args());
    fx.expect_pids(1, ACQUIRE);
    let found = wait_for(ACQUIRE, Duration::from_millis(10), || {
        pgrep(SUITE_PREFIX).contains(&pid).then_some(())
    });
    assert!(
        found.is_ok(),
        "pgrep -f {SUITE_PREFIX} sees the fixture process"
    );
    fx.assert_clean();
}

// ---------------------------------------------------------------------------
// SCL-S2 (SC1): acknowledged controls. Bug (a) of 06_BUG_TRIAGE: a repeated Stop respawned the
// child. These failed on the supervisor at `b521d98` and pass with the SC1 control cell.
// ---------------------------------------------------------------------------

/// Bug (a): a second Stop on a stopped service does not respawn it (BDD E1, `@bug-a`).
#[test]
fn stop_in_off_never_respawns() {
    let fx = OwnedFixture::new("stop-off");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-stop-off"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = running_pid(&sup, &fx);
    sup.stop();
    assert_eq!(sup.status().state, SupervisorState::Off);
    sup.stop();
    let samples = sample_over(Duration::from_secs(2), Duration::from_millis(50), || {
        let st = sup.status();
        (fx.pidfile_pids().len(), st.state, st.pid)
    });
    let respawned: Vec<_> = samples
        .iter()
        .filter(|(_, (n, s, p))| *n != 1 || *s != SupervisorState::Off || p.is_some())
        .collect();
    drop(sup);
    let report = fx.assert_clean();
    assert!(
        respawned.is_empty(),
        "a second Stop respawned the child (first pid {pid}): {respawned:?}; cleanup {report:?}"
    );
}

/// A Stop while the service sits in Backoff cancels the retry, and a second Stop changes
/// nothing: no new child over 2 s.
#[test]
fn stop_in_backoff_never_respawns() {
    let fx = OwnedFixture::new("stop-backoff");
    let mut cfg =
        SupervisorConfig::new(fx.crash_spec("lcf-stop-backoff", 3), fx.path("crash.jsonl"));
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(600),
        multiplier: 1,
        max_delay: Duration::from_millis(600),
        max_retries: 5,
    };
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Backoff { .. }), ACQUIRE);
    assert!(
        matches!(st.state, SupervisorState::Backoff { .. }),
        "INCONCLUSIVE: never reached Backoff: {st:?}"
    );
    let before = fx.expect_pids(1, ACQUIRE).len();
    sup.stop();
    sup.stop();
    let samples = sample_over(Duration::from_secs(2), Duration::from_millis(50), || {
        (fx.pidfile_pids().len(), sup.status().state)
    });
    drop(sup);
    fx.assert_clean();
    assert!(
        samples
            .iter()
            .all(|(_, (n, s))| *n == before && *s == SupervisorState::Off),
        "no retry after Stop in Backoff: {samples:?}"
    );
}

/// Many Stops racing one teardown (the child ignores SIGTERM, so the teardown lasts the whole
/// grace) end in one stopped service: no Stop respawns the child afterwards.
#[test]
fn concurrent_stops_during_teardown_never_respawn() {
    let fx = OwnedFixture::new("stop-race");
    let mut cfg = SupervisorConfig::new(
        fx.stubborn_leaf_spec("lcf-stop-race"),
        fx.path("crash.jsonl"),
    );
    cfg.stop_grace = Duration::from_millis(400);
    let sup = std::sync::Arc::new(
        Supervisor::start(cfg)
            .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}")),
    );
    let pid = running_pid(&sup, &fx);
    let stoppers: Vec<_> = (0..8)
        .map(|_| {
            let s = sup.clone();
            std::thread::spawn(move || s.stop())
        })
        .collect();
    for t in stoppers {
        let _ = t.join();
    }
    assert!(
        pid_reaped(pid),
        "the child is reaped after every Stop returned"
    );
    let samples = sample_over(Duration::from_secs(2), Duration::from_millis(50), || {
        let st = sup.status();
        (fx.pidfile_pids().len(), st.state, st.pid)
    });
    drop(sup);
    let report = fx.assert_clean();
    let respawned: Vec<_> = samples
        .iter()
        .filter(|(_, (n, s, p))| *n != 1 || *s != SupervisorState::Off || p.is_some())
        .collect();
    assert!(
        respawned.is_empty(),
        "a queued Stop respawned the child: {respawned:?}; cleanup {report:?}"
    );
}

/// Repeated Stop joins the first Stop's receipt; `Accepted` is not `Complete`; a Stop on a
/// stopped service is `AlreadySatisfied` (US-1.1 AC2, US-1.2 AC1 and AC2).
#[test]
fn repeated_stop_joins_receipt() {
    let fx = OwnedFixture::new("stop-join");
    let mut cfg = SupervisorConfig::new(
        fx.stubborn_leaf_spec("lcf-stop-join"),
        fx.path("crash.jsonl"),
    );
    cfg.stop_grace = Duration::from_millis(600);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = running_pid(&sup, &fx);
    let first = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("first Stop: {other:?}"),
    };
    // The child ignores SIGTERM, so the teardown lasts the whole grace: accepted, not done.
    assert_eq!(
        receipt_of(&sup, first),
        Receipt::Pending,
        "Accepted never means Complete"
    );
    let during = sup.lifecycle().map(|l| l.phase);
    assert_eq!(during, Some(Phase::Stopping));
    match sup.request_stop() {
        ControlResult::Accepted(t) => assert_eq!(t, first, "a repeated Stop joins the first"),
        other => panic!("repeated Stop: {other:?}"),
    }
    let done = wait_for(ACQUIRE, Duration::from_millis(10), || {
        match receipt_of(&sup, first) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    let receipt = match done {
        Ok((r, _)) => r,
        Err(e) => panic!("INCONCLUSIVE: the Stop never settled within {:?}", e.waited),
    };
    assert!(
        matches!(&receipt, Receipt::Complete { scope } if scope.iter().any(|s| s == "child")),
        "{receipt:?}"
    );
    assert!(pid_reaped(pid), "a Complete Stop has reaped the child");
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(snap.phase, Phase::Stopped);
    assert_eq!(
        snap.intent_sequence, first.intent_sequence,
        "joined Stops add no intent"
    );
    assert!(matches!(
        sup.request_stop(),
        ControlResult::AlreadySatisfied(_)
    ));
    let samples = sample_over(Duration::from_secs(2), Duration::from_millis(50), || {
        fx.pidfile_pids().len()
    });
    drop(sup);
    fx.assert_clean();
    assert!(
        samples.iter().all(|(_, n)| *n == 1),
        "no respawn: {samples:?}"
    );
}

/// After a completed Stop, one explicit Start creates exactly one new incarnation (the positive
/// control of bug (a), SPEC-001 "Stop idempotence / no respawn").
#[test]
fn explicit_start_after_stop_creates_exactly_one_incarnation() {
    let fx = OwnedFixture::new("restart");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-restart"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let first_pid = running_pid(&sup, &fx);
    let first_incarnation = sup.lifecycle().map(|l| l.incarnation);
    sup.stop();
    // On a heavily loaded host the old readers can take longer than the stop window to reach end
    // of file; the Stop then settles Incomplete and the service is briefly Quarantined. This test
    // is about a Start after a completed Stop, so wait until ownership is reconciled.
    let reconciled = wait_for(ACQUIRE, Duration::from_millis(10), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(
        reconciled.is_ok(),
        "INCONCLUSIVE: the Stop never reconciled"
    );
    let token = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start after Stop: {other:?}"),
    };
    let pids = fx.expect_pids(2, ACQUIRE);
    let second_pid = running_pid(&sup, &fx);
    assert_eq!(pids[1], second_pid);
    assert_ne!(first_pid, second_pid);
    let started = wait_for(ACQUIRE, Duration::from_millis(10), || {
        matches!(receipt_of(&sup, token), Receipt::Started { .. }).then_some(())
    });
    assert!(
        started.is_ok(),
        "the Start receipt reports the spawn: {:?}",
        receipt_of(&sup, token)
    );
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(Some(snap.incarnation), first_incarnation.map(|i| i + 1));
    assert_eq!(snap.phase, Phase::Alive);
    assert!(matches!(
        sup.request_start(),
        ControlResult::AlreadySatisfied(_)
    ));
    let samples = sample_over(Duration::from_secs(2), Duration::from_millis(50), || {
        fx.pidfile_pids().len()
    });
    drop(sup);
    fx.assert_clean();
    assert!(
        samples.iter().all(|(_, n)| *n == 2),
        "exactly one new incarnation: {samples:?}"
    );
}

/// A Stop whose log reader cannot finish (a descendant holds the child's output pipe open) is
/// `Incomplete` and names the reader; the service is Quarantined, not Stopped, and a Start is
/// refused until the reader really ends (BDD E1 "Stop reports incomplete cleanup honestly").
#[test]
fn stop_with_a_held_reader_is_incomplete_and_quarantined() {
    let fx = OwnedFixture::new("held-reader");
    let mut cfg = SupervisorConfig::new(
        fx.descendants_spec("lcf-held-reader", 1),
        fx.path("crash.jsonl"),
    );
    cfg.stop_grace = Duration::from_millis(300);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let parent = running_pid(&sup, &fx);
    let pids = fx.expect_pids(2, ACQUIRE);
    let descendant = pids[1];
    let token = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    let settled = wait_for(ACQUIRE, Duration::from_millis(10), || {
        match receipt_of(&sup, token) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    let receipt = match settled {
        Ok((r, _)) => r,
        Err(e) => panic!("INCONCLUSIVE: the Stop never settled within {:?}", e.waited),
    };
    assert!(pid_reaped(parent), "the direct child is reaped");
    assert!(
        pid_alive(descendant),
        "INCONCLUSIVE: the descendant should still hold the pipe"
    );
    match &receipt {
        Receipt::Incomplete { remaining, reason } => {
            assert!(
                remaining.iter().any(|r| r.starts_with("reader")),
                "{remaining:?}"
            );
            assert!(!reason.is_empty());
        }
        other => panic!("a held reader cannot be Complete: {other:?}"),
    }
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(snap.phase, Phase::Quarantined);
    assert!(matches!(sup.request_start(), ControlResult::Quarantined(_)));
    // A repeated Stop while quarantined is not "already satisfied": owned work is still running.
    match sup.request_stop() {
        ControlResult::Quarantined(reason) => assert!(!reason.is_empty()),
        other => panic!("a repeated Stop while quarantined: {other:?}"),
    }
    assert_eq!(fx.pidfile_pids().len(), 2, "no new child while quarantined");
    // The descendant ends, the pipe closes, the reader finishes: ownership is reconciled.
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    let released = wait_for(ACQUIRE, Duration::from_millis(20), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(released.is_ok(), "late reader completion leaves quarantine");
    assert!(
        matches!(receipt_of(&sup, token), Receipt::Incomplete { .. }),
        "the receipt keeps what was observed at the time"
    );
    assert!(
        matches!(sup.request_stop(), ControlResult::AlreadySatisfied(_)),
        "once ownership is reconciled, a Stop is already satisfied"
    );
    assert!(matches!(sup.request_start(), ControlResult::Accepted(_)));
    fx.expect_pids(3, ACQUIRE);
    drop(sup);
    fx.assert_clean();
}

/// A Start accepted while a Stop's teardown is in progress (superseding it) must not spawn a new
/// incarnation while the old incarnation's reader is still held open by a descendant: the old
/// resources are reconciled first, then exactly one new incarnation starts.
#[test]
fn start_during_held_reader_teardown_waits_for_cleanup() {
    let fx = OwnedFixture::new("start-held");
    let mut cfg = SupervisorConfig::new(
        fx.descendants_spec("lcf-start-held", 1),
        fx.path("crash.jsonl"),
    );
    cfg.stop_grace = Duration::from_millis(300);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let parent = running_pid(&sup, &fx);
    let pids = fx.expect_pids(2, ACQUIRE);
    let descendant = pids[1];
    assert!(matches!(sup.request_stop(), ControlResult::Accepted(_)));
    let start = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start during teardown: {other:?}"),
    };
    let samples = sample_over(
        Duration::from_millis(1500),
        Duration::from_millis(50),
        || (fx.pidfile_pids().len(), sup.lifecycle().map(|l| l.phase)),
    );
    let torn_down = wait_for(ACQUIRE, Duration::from_millis(20), || {
        pid_reaped(parent).then_some(())
    });
    assert!(
        torn_down.is_ok(),
        "the old direct child was torn down and reaped"
    );
    assert!(
        pid_alive(descendant),
        "INCONCLUSIVE: the descendant should still hold the pipe"
    );
    let spawned: Vec<_> = samples
        .iter()
        .filter(|(_, (n, phase))| *n != 2 || *phase == Some(Phase::Alive))
        .collect();
    assert!(
        spawned.is_empty(),
        "a new incarnation started while the old reader was held: {spawned:?}"
    );
    assert_eq!(
        receipt_of(&sup, start),
        Receipt::Pending,
        "the Start waits for the cleanup"
    );
    // The descendant ends, the old reader finishes: the Start proceeds with one new incarnation.
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    let started = wait_for(ACQUIRE, Duration::from_millis(10), || {
        matches!(receipt_of(&sup, start), Receipt::Started { .. }).then_some(())
    });
    assert!(
        started.is_ok(),
        "the Start proceeds after cleanup: {:?}",
        receipt_of(&sup, start)
    );
    let after = fx.expect_pids(4, ACQUIRE);
    assert_eq!(
        after.len(),
        4,
        "exactly one new incarnation (parent and its descendant)"
    );
    drop(sup);
    fx.assert_clean();
}

/// When the service monitor is gone, a Stop is still committed (the snapshot shows it), and the
/// caller is told the outcome cannot be observed, never that it succeeded.
#[test]
fn stop_without_a_monitor_is_observation_unknown() {
    let fx = OwnedFixture::new("no-monitor");
    let mut spec = fx.leaf_spec("lcf-no-monitor");
    // A child-watch hook that panics on the monitor thread ends the monitor. It waits until the
    // child has recorded its pid, so the fixture knows which child the ended monitor owned.
    let pidfile = fx.path("pids");
    spec.child_watch = Some(ChildWatch {
        interval: Duration::from_millis(20),
        observe: std::sync::Arc::new(move |_| {
            let recorded = std::fs::read_to_string(&pidfile).is_ok_and(|s| !s.trim().is_empty());
            if recorded {
                panic!("deliberate: end the monitor thread");
            }
        }),
        ended: std::sync::Arc::new(|| {}),
        prepare: None,
    });
    let sup = Supervisor::start(SupervisorConfig::new(spec, fx.path("crash.jsonl")))
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = fx.expect_pids(1, ACQUIRE)[0];
    let gone = wait_for(ACQUIRE, Duration::from_millis(10), || {
        (!sup.monitor_running()).then_some(())
    });
    assert!(gone.is_ok(), "INCONCLUSIVE: the monitor did not end");
    let before = sup.lifecycle().map(|l| l.intent_sequence);
    let stop = match sup.request_stop() {
        ControlResult::ObservationUnknown(token, reason) => {
            assert!(!reason.is_empty());
            let snap = sup
                .lifecycle()
                .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
            assert_eq!(Some(token.intent_sequence), before.map(|s| s + 1));
            assert_eq!(
                snap.desired,
                Desired::Stopped,
                "the intent is committed and visible"
            );
            token
        }
        other => panic!("expected ObservationUnknown, got {other:?}"),
    };
    // Joining the pending Stop stays ObservationUnknown: never Accepted for a token nobody can
    // settle.
    match sup.request_stop() {
        ControlResult::ObservationUnknown(token, _) => assert_eq!(token, stop),
        other => panic!("a repeated Stop without a monitor: {other:?}"),
    }
    // The legacy stop() returns at once instead of waiting out its bound.
    let t = Deadline::after(Duration::from_secs(10));
    sup.stop();
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "stop() took {:?}",
        t.elapsed()
    );
    // A Start, and a Start joining it, are ObservationUnknown too.
    let start = match sup.request_start() {
        ControlResult::ObservationUnknown(token, _) => token,
        other => panic!("a Start without a monitor: {other:?}"),
    };
    match sup.request_start() {
        ControlResult::ObservationUnknown(token, _) => assert_eq!(token, start),
        other => panic!("a repeated Start without a monitor: {other:?}"),
    }
    // The monitor that ended still ended its child: nothing outlives the supervisor.
    let reaped = wait_for(ACQUIRE, Duration::from_millis(10), || {
        pid_reaped(pid).then_some(())
    });
    drop(sup);
    assert!(
        reaped.is_ok(),
        "a monitor that ended terminates and reaps its child"
    );
    fx.assert_clean();
}

/// After the monitor ends (a `ChildWatch` hook panicked while the child was running), a Start
/// called directly, with no Stop first, is never `AlreadySatisfied`: nothing supervises the
/// service any more, so the cell stops reporting Alive and the Start's outcome cannot be
/// observed.
#[test]
fn start_after_the_monitor_ends_is_never_already_satisfied() {
    let fx = OwnedFixture::new("start-no-monitor");
    let mut spec = fx.leaf_spec("lcf-start-no-monitor");
    // Panic only once the child has recorded its pid, so the monitor ends while it owns a live,
    // published child.
    let pidfile = fx.path("pids");
    spec.child_watch = Some(ChildWatch {
        interval: Duration::from_millis(20),
        observe: std::sync::Arc::new(move |_| {
            let recorded = std::fs::read_to_string(&pidfile).is_ok_and(|s| !s.trim().is_empty());
            if recorded {
                panic!("deliberate: end the monitor thread");
            }
        }),
        ended: std::sync::Arc::new(|| {}),
        prepare: None,
    });
    let sup = Supervisor::start(SupervisorConfig::new(spec, fx.path("crash.jsonl")))
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = fx.expect_pids(1, ACQUIRE)[0];
    let gone = wait_for(ACQUIRE, Duration::from_millis(5), || {
        (!sup.monitor_running()).then_some(())
    });
    assert!(gone.is_ok(), "INCONCLUSIVE: the monitor did not end");
    start_without_a_monitor_is_not_satisfied(&sup);
    assert!(pid_reaped(pid), "the ended monitor reaped its child");
    drop(sup);
    fx.assert_clean();
}

/// The same after the child already exited on its own: the monitor reaped it, then the
/// `ended` hook panicked before the crash was recorded. The cell must not keep the old pid
/// Alive, and a Start is never `AlreadySatisfied`.
#[test]
fn start_after_the_monitor_ends_past_an_exited_child_is_never_already_satisfied() {
    let fx = OwnedFixture::new("start-no-monitor-exited");
    let mut spec = fx.crash_spec("lcf-start-no-monitor-exited", 3);
    spec.child_watch = Some(ChildWatch {
        interval: Duration::from_millis(20),
        observe: std::sync::Arc::new(|_| {}),
        ended: std::sync::Arc::new(|| panic!("deliberate: end the monitor after the child exited")),
        prepare: None,
    });
    let mut cfg = SupervisorConfig::new(spec, fx.path("crash.jsonl"));
    cfg.backoff = fast_backoff(3);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = fx.expect_pids(1, ACQUIRE)[0];
    let gone = wait_for(ACQUIRE, Duration::from_millis(5), || {
        (!sup.monitor_running()).then_some(())
    });
    assert!(gone.is_ok(), "INCONCLUSIVE: the monitor did not end");
    assert!(
        pid_reaped(pid),
        "INCONCLUSIVE: the child exited and was reaped"
    );
    start_without_a_monitor_is_not_satisfied(&sup);
    hold_for(Duration::from_millis(300));
    assert_eq!(
        fx.pidfile_pids().len(),
        1,
        "nothing respawns without a monitor"
    );
    drop(sup);
    fx.assert_clean();
}

/// Shared tail of the two fixtures above: the monitor has ended.
fn start_without_a_monitor_is_not_satisfied(sup: &Supervisor) {
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_ne!(
        snap.phase,
        Phase::Alive,
        "an ended monitor leaves no supervised child: {snap:?}"
    );
    assert_eq!(
        snap.pid, None,
        "no pid is claimed without a monitor: {snap:?}"
    );
    assert_ne!(
        sup.status().state,
        SupervisorState::Running,
        "the legacy view stops reporting Running"
    );
    let start = match sup.request_start() {
        ControlResult::ObservationUnknown(token, reason) => {
            assert!(!reason.is_empty());
            token
        }
        other => panic!("a Start after the monitor ended: {other:?}"),
    };
    // A second Start joins the first and is still not a success.
    match sup.request_start() {
        ControlResult::ObservationUnknown(token, _) => assert_eq!(token, start),
        other => panic!("a repeated Start after the monitor ended: {other:?}"),
    }
}

/// A child that crashes leaving a descendant holding its output open: after the retry budget
/// is spent, an explicit Start spawns nothing until the old readers are reconciled.
#[test]
fn start_after_a_crash_with_a_held_reader_waits_for_cleanup() {
    let fx = OwnedFixture::new("crash-held");
    let mut cfg = SupervisorConfig::new(
        fx.crash_with_descendant_spec("lcf-crash-held"),
        fx.path("crash.jsonl"),
    );
    cfg.backoff = fast_backoff(0);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let failed = sup.wait_until(|s| *s == SupervisorState::Failed, ACQUIRE);
    assert_eq!(
        failed.state,
        SupervisorState::Failed,
        "INCONCLUSIVE: {failed:?}"
    );
    held_reader_blocks_the_next_start(&sup, &fx);
    drop(sup);
    fx.assert_clean();
}

/// A child whose health check keeps failing, leaving a descendant holding its output open:
/// after the retry budget is spent, an explicit Start spawns nothing until the old readers are
/// reconciled.
#[test]
fn start_after_health_failures_with_a_held_reader_waits_for_cleanup() {
    let fx = OwnedFixture::new("health-held");
    let mut spec = fx.descendants_spec("lcf-health-held", 1);
    // Healthy until the first incarnation has recorded itself and its descendant, then failing:
    // under load an always-false probe could end the child before it wrote its pids.
    let pidfile = fx.path("pids");
    spec.health_check = Some(HealthCheck {
        interval: Duration::from_millis(50),
        grace: Duration::ZERO,
        probe: std::sync::Arc::new(move || {
            std::fs::read_to_string(&pidfile)
                .map(|s| s.lines().count() < 2)
                .unwrap_or(true)
        }),
    });
    let mut cfg = SupervisorConfig::new(spec, fx.path("crash.jsonl"));
    cfg.backoff = fast_backoff(0);
    cfg.stop_grace = Duration::from_millis(300);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let failed = sup.wait_until(|s| *s == SupervisorState::Failed, ACQUIRE);
    assert_eq!(
        failed.state,
        SupervisorState::Failed,
        "INCONCLUSIVE: {failed:?}"
    );
    held_reader_blocks_the_next_start(&sup, &fx);
    drop(sup);
    fx.assert_clean();
}

/// Shared tail of the two fixtures above: the first incarnation (a parent and one descendant)
/// has ended and been given up on; its descendant still holds the output pipe.
fn held_reader_blocks_the_next_start(sup: &Supervisor, fx: &OwnedFixture) {
    let pids = fx.expect_pids(2, ACQUIRE);
    let descendant = pids[1];
    assert!(
        pid_alive(descendant),
        "INCONCLUSIVE: the descendant should hold the pipe"
    );
    let start = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start after Failed: {other:?}"),
    };
    let samples = sample_over(
        Duration::from_millis(1500),
        Duration::from_millis(50),
        || (fx.pidfile_pids().len(), sup.lifecycle().map(|l| l.phase)),
    );
    let spawned: Vec<_> = samples
        .iter()
        .filter(|(_, (n, phase))| *n != 2 || *phase == Some(Phase::Alive))
        .collect();
    assert!(
        spawned.is_empty(),
        "a new incarnation started while the old reader was held: {spawned:?}"
    );
    assert_eq!(receipt_of(sup, start), Receipt::Pending);
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    // The receipt, not the pid file: the new incarnation may be ended by its own fixture (crash,
    // failing probe) before its shell has written its pid.
    let started = wait_for(ACQUIRE, Duration::from_millis(10), || {
        matches!(receipt_of(sup, start), Receipt::Started { .. }).then_some(())
    });
    assert!(
        started.is_ok(),
        "the Start proceeds after cleanup: {:?}",
        receipt_of(sup, start)
    );
}

/// core#397: a crash that leaves a descendant holding the child's output, with retry budget left.
/// The automatic retry waits for the held readers exactly like an explicit Start: no new
/// incarnation while the pipe is held, the service Quarantined meanwhile, and the retry proceeds
/// once the descendant ends.
#[test]
fn crash_retry_with_a_held_reader_waits_for_cleanup() {
    let fx = OwnedFixture::new("retry-crash-held");
    let mut cfg = SupervisorConfig::new(
        fx.crash_with_descendant_spec("lcf-retry-crash-held"),
        fx.path("crash.jsonl"),
    );
    cfg.backoff = fast_backoff(1);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let descendant = retry_waits_for_the_held_reader(&sup, &fx);
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    retry_proceeds_after_cleanup(&sup);
    drop(sup);
    fx.assert_clean();
}

/// core#397: the same for a health-check failure with retry budget left.
#[test]
fn health_retry_with_a_held_reader_waits_for_cleanup() {
    let fx = OwnedFixture::new("retry-health-held");
    let mut spec = fx.descendants_spec("lcf-retry-health-held", 1);
    // Healthy until the first incarnation has recorded itself and its descendant, then failing.
    let pidfile = fx.path("pids");
    spec.health_check = Some(HealthCheck {
        interval: Duration::from_millis(50),
        grace: Duration::ZERO,
        probe: std::sync::Arc::new(move || {
            std::fs::read_to_string(&pidfile)
                .map(|s| s.lines().count() < 2)
                .unwrap_or(true)
        }),
    });
    let mut cfg = SupervisorConfig::new(spec, fx.path("crash.jsonl"));
    cfg.backoff = fast_backoff(1);
    cfg.stop_grace = Duration::from_millis(300);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let descendant = retry_waits_for_the_held_reader(&sup, &fx);
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    retry_proceeds_after_cleanup(&sup);
    drop(sup);
    fx.assert_clean();
}

/// core#397: a Stop that arrives while an automatic retry waits for held readers wins: it never
/// lets the retry spawn, and once the descendant ends the service is Stopped with one incarnation.
#[test]
fn stop_during_a_held_retry_spawns_nothing() {
    let fx = OwnedFixture::new("retry-stop-held");
    let mut cfg = SupervisorConfig::new(
        fx.crash_with_descendant_spec("lcf-retry-stop-held"),
        fx.path("crash.jsonl"),
    );
    cfg.backoff = fast_backoff(3);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let descendant = retry_waits_for_the_held_reader(&sup, &fx);
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop during a held retry: {other:?}"),
    };
    let settled = wait_for(ACQUIRE, Duration::from_millis(10), || {
        match receipt_of(&sup, stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    match settled {
        Ok((Receipt::Incomplete { remaining, .. }, _)) => assert!(
            remaining.iter().any(|r| r.starts_with("reader")),
            "{remaining:?}"
        ),
        Ok((other, _)) => panic!("a held reader cannot make the Stop Complete: {other:?}"),
        Err(e) => panic!("INCONCLUSIVE: the Stop never settled within {:?}", e.waited),
    }
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    let stopped = wait_for(ACQUIRE, Duration::from_millis(20), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(
        stopped.is_ok(),
        "the Stop leaves quarantine once the reader ends"
    );
    hold_for(Duration::from_millis(300));
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(
        snap.incarnation, 1,
        "the held retry never spawned: {snap:?}"
    );
    assert_eq!(fx.pidfile_pids().len(), 2, "no new child after the Stop");
    drop(sup);
    fx.assert_clean();
}

/// Shared head of the three fixtures above: the first incarnation (a parent and one descendant)
/// ended with retry budget left, and its descendant still holds the output pipe. Asserts that no
/// second incarnation starts while it does, and returns the descendant's pid.
fn retry_waits_for_the_held_reader(sup: &Supervisor, fx: &OwnedFixture) -> u32 {
    let pids = fx.expect_pids(2, ACQUIRE);
    let descendant = pids[1];
    let crashed = wait_for(ACQUIRE, Duration::from_millis(5), || {
        (sup.status().restarts >= 1).then_some(())
    });
    assert!(
        crashed.is_ok(),
        "INCONCLUSIVE: the first incarnation never ended"
    );
    assert!(
        pid_alive(descendant),
        "INCONCLUSIVE: the descendant should hold the pipe"
    );
    // Counters, not timings: the incarnation counter is bumped by every spawn.
    let samples = sample_over(
        Duration::from_millis(1500),
        Duration::from_millis(50),
        || {
            let snap = sup.lifecycle();
            (
                snap.as_ref().map(|l| l.incarnation),
                fx.pidfile_pids().len(),
                snap.map(|l| l.phase),
            )
        },
    );
    let spawned: Vec<_> = samples
        .iter()
        .filter(|(_, (inc, n, phase))| {
            inc.is_some_and(|i| i != 1) || *n != 2 || *phase == Some(Phase::Alive)
        })
        .collect();
    assert!(
        spawned.is_empty(),
        "an automatic retry started a new incarnation while the old reader was held: {spawned:?}"
    );
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(
        snap.phase,
        Phase::Quarantined,
        "the held retry is reported, not hidden: {snap:?}"
    );
    assert!(snap.quarantine.is_some_and(|q| !q.is_empty()));
    assert_eq!(snap.pid, None);
    descendant
}

/// Shared tail: once the old reader ended, the retry spawns the second incarnation, which uses
/// up the budget (max_retries = 1), so the service ends Failed after exactly two incarnations.
fn retry_proceeds_after_cleanup(sup: &Supervisor) {
    let failed = wait_for(ACQUIRE, Duration::from_millis(10), || {
        sup.lifecycle()
            .filter(|l| l.phase == Phase::Failed)
            .map(|l| l.incarnation)
    });
    match failed {
        Ok((incarnation, _)) => assert_eq!(incarnation, 2, "exactly one retry ran"),
        Err(e) => panic!(
            "the retry did not proceed after cleanup within {:?}: {:?}",
            e.waited,
            sup.lifecycle()
        ),
    }
}

/// After shutdown begins, a Start is refused as `RejectedClosing`.
#[test]
fn start_after_shutdown_is_rejected_closing() {
    let fx = OwnedFixture::new("closing");
    let mut sup = Supervisor::start(SupervisorConfig::new(
        fx.leaf_spec("lcf-closing"),
        fx.path("crash.jsonl"),
    ))
    .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    let pid = running_pid(&sup, &fx);
    sup.shutdown();
    assert!(pid_reaped(pid), "shutdown reaps the child");
    assert_eq!(sup.request_start(), ControlResult::RejectedClosing);
    hold_for(Duration::from_millis(300));
    assert_eq!(fx.pidfile_pids().len(), 1, "no child after Closing");
    drop(sup);
    fx.assert_clean();
}

// ---------------------------------------------------------------------------
// SCL-S2.3: spawn and publication tickets. A spawn admitted before a Stop or shutdown is still
// owned: its late child goes straight to teardown and is never published, and the Stop is not
// settled until it has. A credential publication admitted before a Stop cannot become the
// active reference afterwards.
// ---------------------------------------------------------------------------

/// A gate the monitor waits at between spawn admission and the OS spawn call, so a fixture can
/// hold a spawn "in flight" while it commits a Stop.
#[derive(Default)]
struct SpawnGate {
    state: std::sync::Mutex<(u32, bool)>, // (times entered, released)
    cv: std::sync::Condvar,
}

impl SpawnGate {
    fn hook(gate: &std::sync::Arc<SpawnGate>) -> std::sync::Arc<dyn Fn() + Send + Sync> {
        let g = gate.clone();
        std::sync::Arc::new(move || {
            let mut s = g.state.lock().unwrap_or_else(|p| p.into_inner());
            s.0 += 1;
            g.cv.notify_all();
            while !s.1 {
                s = g.cv.wait(s).unwrap_or_else(|p| p.into_inner());
            }
        })
    }

    fn wait_entered(&self, n: u32) {
        let deadline = Deadline::after(ACQUIRE);
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        while s.0 < n {
            if deadline.expired() {
                panic!("INCONCLUSIVE: the monitor never reached the spawn gate");
            }
            s = self
                .cv
                .wait_timeout(s, deadline.remaining())
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    fn release(&self) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        s.1 = true;
        self.cv.notify_all();
    }

    /// How many times the monitor has reached the gate so far.
    fn entered(&self) -> u32 {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).0
    }
}

impl Drop for SpawnGate {
    fn drop(&mut self) {
        self.release();
    }
}

/// Record every legacy state the service publishes, sampled every millisecond.
fn record_states(
    sup: &std::sync::Arc<Supervisor>,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<Vec<SupervisorState>> {
    let s = sup.clone();
    let stop = stop.clone();
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        while !stop.load(std::sync::atomic::Ordering::SeqCst) {
            let st = s.status().state;
            if seen.last() != Some(&st) {
                seen.push(st);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        seen
    })
}

/// A spawn admitted before Stop returns after it: the child goes straight to teardown, is never
/// published Running, and the Stop stays Pending until that ticket is resolved (INV-12).
#[test]
fn late_spawn_after_stop_goes_straight_to_teardown() {
    let fx = OwnedFixture::new("late-spawn");
    let gate = std::sync::Arc::new(SpawnGate::default());
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-late-spawn"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    cfg.spawn_barrier = Some(SpawnGate::hook(&gate));
    let sup = std::sync::Arc::new(
        Supervisor::start(cfg)
            .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}")),
    );
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let states = record_states(&sup, &done);
    gate.wait_entered(1);
    let held = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(held.open_tickets, 1, "the admitted spawn is an open ticket");
    let start = ControlToken {
        app_epoch: held.app_epoch,
        instance: held.instance,
        intent_sequence: 1,
    };
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    hold_for(Duration::from_millis(300));
    assert_eq!(
        receipt_of(&sup, stop),
        Receipt::Pending,
        "Complete waits for the ticket"
    );
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Stopping));
    gate.release();
    let settled = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    let receipt = match settled {
        Ok((r, _)) => r,
        Err(e) => panic!("INCONCLUSIVE: the Stop never settled within {:?}", e.waited),
    };
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let seen = states.join().unwrap_or_default();
    match &receipt {
        Receipt::Complete { scope } => {
            assert!(scope.iter().any(|s| s == "spawnTicket"), "{scope:?}");
            assert!(scope.iter().any(|s| s == "child"), "{scope:?}");
        }
        // On a heavily loaded host the late child's readers can miss the stop window; the Stop
        // is then honestly Incomplete on the readers alone, never Complete without the ticket.
        Receipt::Incomplete { remaining, .. }
            if remaining.iter().all(|r| r.starts_with("reader")) => {}
        other => panic!("{other:?}"),
    }
    // Either way, ownership reconciles once the readers end.
    let reconciled = wait_for(ACQUIRE, Duration::from_millis(10), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(reconciled.is_ok(), "INCONCLUSIVE: never reconciled");
    assert!(
        !seen.contains(&SupervisorState::Running),
        "the late child was never published: {seen:?}"
    );
    assert_eq!(
        receipt_of(&sup, start),
        Receipt::Expired,
        "the Start was superseded, not Started"
    );
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(snap.open_tickets, 0);
    assert_eq!(snap.incarnation, 1, "the late spawn is counted");
    assert_eq!(snap.phase, Phase::Stopped);
    for pid in fx.pidfile_pids() {
        assert!(!pid_alive(pid), "the late child {pid} is gone");
    }
    hold_for(Duration::from_millis(500));
    assert!(
        fx.pidfile_pids().len() <= 1,
        "nothing was spawned after the Stop"
    );
    drop(sup);
    fx.assert_clean();
}

/// A Start that lands while the monitor marks the service Failed never joins the dying intent:
/// it is a fresh intent (new token), and the dying intent's receipt is Failed. The service's
/// binary cannot be started, so the first Start's receipt is still pending when it fails.
#[test]
fn a_start_racing_the_failed_transition_is_a_fresh_intent() {
    let fx = OwnedFixture::new("failed-race");
    let gate = std::sync::Arc::new(SpawnGate::default());
    let spec = SidecarSpec::new("lcf-failed-race", fx.path("no-such-binary"), vec![]);
    let mut cfg = SupervisorConfig::new(spec, fx.path("crash.jsonl"));
    cfg.backoff = fast_backoff(0);
    cfg.failed_barrier = Some(SpawnGate::hook(&gate));
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    // The monitor is inside the Failed transition, held at the barrier.
    gate.wait_entered(1);
    let (epoch, instance) = sup
        .lifecycle()
        .map(|l| (l.app_epoch, l.instance))
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    let dying = ControlToken {
        app_epoch: epoch,
        instance,
        intent_sequence: 1,
    };
    let fresh = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start during the Failed transition: {other:?}"),
    };
    gate.release();
    assert_ne!(
        fresh, dying,
        "the Start must not join the intent that is failing"
    );
    let settled = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, dying) {
            Receipt::Pending => None,
            r => Some(r),
        }
    });
    assert!(
        matches!(settled, Ok((Receipt::Failed { .. }, _))),
        "the failing intent reads Failed: {settled:?}"
    );
    // The fresh Start is its own attempt: it runs and settles on its own receipt.
    let own = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, fresh) {
            Receipt::Pending => None,
            r => Some(r),
        }
    });
    assert!(matches!(own, Ok((Receipt::Failed { .. }, _))), "{own:?}");
    drop(sup);
    fx.assert_clean();
}

/// The Start receipt of a spawn that a Stop overtook reads Superseded while it is retained.
#[test]
fn a_start_overtaken_during_its_spawn_reads_superseded() {
    let fx = OwnedFixture::new("superseded-spawn");
    let gate = std::sync::Arc::new(SpawnGate::default());
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-superseded"), fx.path("crash.jsonl"));
    cfg.spawn_barrier = Some(SpawnGate::hook(&gate));
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    gate.wait_entered(1);
    let (epoch, instance) = sup
        .lifecycle()
        .map(|l| (l.app_epoch, l.instance))
        .unwrap_or_default();
    let start = ControlToken {
        app_epoch: epoch,
        instance,
        intent_sequence: 1,
    };
    assert!(matches!(sup.request_stop(), ControlResult::Accepted(_)));
    assert_eq!(
        receipt_of(&sup, start),
        Receipt::Superseded,
        "never an invented Started"
    );
    gate.release();
    drop(sup);
    fx.assert_clean();
}

/// A spawn admitted before shutdown returns after it: the child is torn down, shutdown returns,
/// nothing is left running, and no later Start is admitted.
#[test]
fn late_spawn_after_shutdown_is_torn_down() {
    let fx = OwnedFixture::new("late-close");
    let gate = std::sync::Arc::new(SpawnGate::default());
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-late-close"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    cfg.join_timeout = Duration::from_secs(10);
    cfg.spawn_barrier = Some(SpawnGate::hook(&gate));
    let mut sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    gate.wait_entered(1);
    let releaser = {
        let g = gate.clone();
        std::thread::spawn(move || {
            hold_for(Duration::from_millis(200));
            g.release();
        })
    };
    sup.shutdown();
    let _ = releaser.join();
    assert!(!sup.monitor_running(), "shutdown joined the monitor");
    for pid in fx.pidfile_pids() {
        assert!(!pid_alive(pid), "the late child {pid} is gone");
    }
    assert_eq!(sup.request_start(), ControlResult::RejectedClosing);
    assert_eq!(sup.status().state, SupervisorState::Off);
    drop(sup);
    fx.assert_clean();
}

/// A publication ticket committed while its intent is current becomes active; one admitted
/// before a Stop and released inside the stop window is named in the Complete scope.
#[test]
fn publication_ticket_released_before_stop_settles_is_in_the_complete_scope() {
    let fx = OwnedFixture::new("pub-ok");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-ok"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let current = sup
        .admit_publication("seed")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    assert!(
        current.commit(activate_nothing).is_ok(),
        "a current ticket becomes active"
    );
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    assert_eq!(sup.lifecycle().map(|l| l.open_tickets), Some(1));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    let stale = match ticket.commit(activate_nothing) {
        Err(CommitRefused::Stale(stale)) => stale,
        other => panic!("a ticket admitted before Stop must not become active after it: {other:?}"),
    };
    stale.cleaned();
    let settled = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    match settled {
        Ok((Receipt::Complete { scope }, _)) => {
            assert!(scope.iter().any(|s| s == "publication:bearer"), "{scope:?}")
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        sup.admit_publication("late"),
        Err(PublicationRefused::NotCurrent)
    ));
    drop(sup);
    fx.assert_clean();
}

/// A publication ticket released partway through the stop window, after every log reader is
/// done, is waited for: the Stop is Complete and names it. The test releases the ticket at the
/// monitor's settle gate (readers done, ticket open), not on a timer, so a window that stopped
/// waiting for publications once the readers were done would settle before the gate is reached.
#[test]
fn publication_released_after_the_readers_end_is_in_the_complete_scope() {
    let fx = OwnedFixture::new("pub-late");
    let gate = std::sync::Arc::new(SpawnGate::default());
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-late"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    cfg.settle_barrier = Some(SpawnGate::hook(&gate));
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    // Either the monitor reaches the gate, or the Stop settles without waiting for the ticket.
    let reached = wait_for(ACQUIRE, Duration::from_millis(2), || {
        if gate.entered() > 0 {
            return Some(None);
        }
        match sup.receipt(stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(Some(r)),
        }
    });
    match reached {
        Ok((None, _)) => {}
        Ok((Some(Receipt::Incomplete { remaining, .. }), _))
            if remaining.iter().any(|r| r != "publication:bearer") =>
        {
            panic!(
                "INCONCLUSIVE: the log readers did not end inside the stop window: {remaining:?}"
            )
        }
        Ok((Some(r), _)) => {
            panic!("the Stop settled while its publication ticket was still open: {r:?}")
        }
        Err(_) => panic!("INCONCLUSIVE: the monitor never reached the settle gate"),
    }
    assert_eq!(
        receipt_of(&sup, stop),
        Receipt::Pending,
        "the Stop waits at the gate"
    );
    let stale = match ticket.commit() {
        Err(stale) => stale,
        Ok(()) => panic!("a ticket admitted before Stop must not become active after it"),
    };
    stale.cleaned();
    gate.release();
    let settled = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    match settled {
        Ok((Receipt::Complete { scope }, _)) => {
            assert!(scope.iter().any(|s| s == "publication:bearer"), "{scope:?}");
            assert!(scope.iter().any(|s| s == "readers"), "{scope:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Stopped));
    drop(sup);
    fx.assert_clean();
}

/// A publication ticket still held when the stop window ends makes the Stop Incomplete and the
/// service Quarantined; releasing it reconciles ownership.
#[test]
fn held_publication_ticket_makes_stop_incomplete_until_released() {
    let fx = OwnedFixture::new("pub-held");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-held"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    let settled = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    match settled {
        Ok((Receipt::Incomplete { remaining, .. }, _)) => {
            // Under heavy load the readers may also still be draining; the ticket is always named.
            assert!(
                remaining.iter().any(|r| r == "publication:bearer"),
                "{remaining:?}"
            )
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Quarantined));
    assert!(matches!(sup.request_start(), ControlResult::Quarantined(_)));
    ticket.withdraw();
    let released = wait_for(ACQUIRE, Duration::from_millis(10), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(released.is_ok(), "releasing the ticket leaves quarantine");
    drop(sup);
    fx.assert_clean();
}

/// A stale ticket dropped without `cleaned()` (an ignored error, or an unwind) keeps its
/// publication owned: what it wrote may still be on disk, so the Stop is never Complete and the
/// service stays Quarantined.
#[test]
fn a_dropped_stale_ticket_keeps_the_stop_open() {
    let fx = OwnedFixture::new("pub-dropped");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-dropped"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    let file = fx.path("bearer");
    if let Err(e) = std::fs::write(&file, b"credential") {
        panic!("INCONCLUSIVE: cannot write the credential file: {e}");
    }
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    let stale = match ticket.commit(activate_nothing) {
        Err(CommitRefused::Stale(stale)) => stale,
        other => panic!("a ticket admitted before Stop must not become active after it: {other:?}"),
    };
    drop(stale); // never cleaned: the file is still there
    let receipt = settle(&sup, stop, ACQUIRE);
    assert!(file.exists(), "the stale file was left behind");
    match &receipt {
        Receipt::Incomplete { remaining, .. } => assert!(
            remaining.iter().any(|r| r == "publication:bearer"),
            "{remaining:?}"
        ),
        other => panic!("a Stop with an uncleaned stale publication is not Complete: {other:?}"),
    }
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Quarantined));
    assert!(matches!(sup.request_start(), ControlResult::Quarantined(_)));
    drop(sup);
    fx.assert_clean();
}

/// A caller that unwinds while it holds an uncommitted ticket (here, after writing the file and
/// before `commit`) drops the ticket without saying that nothing it wrote remains. The ticket
/// stays owned: the Stop is never Complete while the file may be on disk, and the service stays
/// Quarantined.
#[test]
fn a_ticket_dropped_by_an_unwinding_caller_keeps_the_stop_open() {
    let fx = OwnedFixture::new("pub-unwind");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-unwind"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let file = fx.path("bearer");
    let mut stop = None;
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ticket = sup
            .admit_publication("bearer")
            .unwrap_or_else(|e| panic!("INCONCLUSIVE: admission while running: {e:?}"));
        if let Err(e) = std::fs::write(&file, b"credential") {
            panic!("INCONCLUSIVE: cannot write the credential file: {e}");
        }
        stop = match sup.request_stop() {
            ControlResult::Accepted(t) => Some(t),
            other => panic!("INCONCLUSIVE: Stop: {other:?}"),
        };
        let _owned = ticket;
        panic!("deliberate: the publishing caller fails before commit");
    }));
    assert!(unwound.is_err(), "the caller unwound as intended");
    let stop = stop.unwrap_or_else(|| panic!("INCONCLUSIVE: no Stop was committed"));
    let receipt = settle(&sup, stop, ACQUIRE);
    assert!(file.exists(), "the written file is still on disk");
    match &receipt {
        Receipt::Incomplete { remaining, .. } => assert!(
            remaining.iter().any(|r| r == "publication:bearer"),
            "{remaining:?}"
        ),
        other => panic!("a Stop with an unresolved publication is not Complete: {other:?}"),
    }
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Quarantined));
    assert!(matches!(sup.request_start(), ControlResult::Quarantined(_)));
    drop(sup);
    fx.assert_clean();
}

/// Positive control: a ticket withdrawn before the Stop (nothing it wrote remains) is resolved,
/// and the Stop settles Complete.
#[test]
fn a_withdrawn_ticket_lets_the_stop_complete() {
    let fx = OwnedFixture::new("pub-withdraw");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-withdraw"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: admission while running: {e:?}"));
    assert_eq!(sup.lifecycle().map(|l| l.open_tickets), Some(1));
    ticket.withdraw();
    assert_eq!(sup.lifecycle().map(|l| l.open_tickets), Some(0));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    assert!(
        matches!(settle(&sup, stop, ACQUIRE), Receipt::Complete { .. }),
        "a withdrawn ticket leaves nothing for the Stop to wait on"
    );
    drop(sup);
    fx.assert_clean();
}

/// A ticket admitted under an intent that a Stop and then a Start replaced never becomes the
/// active reference, although the service is meant to be running again.
#[test]
fn a_ticket_from_a_replaced_intent_never_becomes_active() {
    let fx = OwnedFixture::new("pub-replaced");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-replaced"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    assert!(matches!(sup.request_stop(), ControlResult::Accepted(_)));
    let start = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start after Stop: {other:?}"),
    };
    assert_eq!(
        sup.lifecycle().map(|l| l.desired),
        Some(Desired::Running),
        "the service is meant to run again"
    );
    match ticket.commit(activate_nothing) {
        Err(CommitRefused::Stale(stale)) => {
            assert_eq!(stale.name(), "bearer");
            stale.cleaned();
        }
        other => {
            panic!(
                "a ticket from a replaced intent must not become the active reference: {other:?}"
            )
        }
    }
    assert!(matches!(
        settle(&sup, start, ACQUIRE),
        Receipt::Started { .. }
    ));
    drop(sup);
    fx.assert_clean();
}

/// A Start that supersedes a Stop while a publication admitted under the old intent is still
/// held spawns nothing until that ticket is released.
#[test]
fn start_during_teardown_waits_for_an_old_publication_ticket() {
    let fx = OwnedFixture::new("start-pub");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-start-pub"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    assert!(matches!(sup.request_stop(), ControlResult::Accepted(_)));
    let start = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start during teardown: {other:?}"),
    };
    let samples = sample_over(
        Duration::from_millis(1000),
        Duration::from_millis(50),
        || (fx.pidfile_pids().len(), sup.lifecycle().map(|l| l.phase)),
    );
    assert!(
        samples
            .iter()
            .all(|(_, (n, phase))| *n == 1 && *phase != Some(Phase::Alive)),
        "no new incarnation while the old ticket is held: {samples:?}"
    );
    assert_eq!(receipt_of(&sup, start), Receipt::Pending);
    // A repeated Start joins the held one (Quarantined once the monitor has placed the hold,
    // or the same pending token before that); it never starts anything new.
    match sup.request_start() {
        ControlResult::Quarantined(_) => {}
        ControlResult::Accepted(t) => assert_eq!(t, start),
        other => panic!("a repeated Start while held: {other:?}"),
    }
    ticket.withdraw();
    fx.expect_pids(2, ACQUIRE);
    drop(sup);
    fx.assert_clean();
}

/// Stop and an immediate shutdown while a publication ticket is open: the Stop is Incomplete and
/// names the ticket, and the service stays Quarantined with the ticket counted. Releasing the
/// ticket afterwards reconciles ownership.
#[test]
fn shutdown_with_an_open_publication_ticket_is_incomplete() {
    let fx = OwnedFixture::new("close-pub");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-close-pub"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let mut sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    sup.shutdown();
    match receipt_of(&sup, stop) {
        Receipt::Incomplete { remaining, .. } => assert!(
            remaining.iter().any(|r| r == "publication:bearer"),
            "{remaining:?}"
        ),
        other => panic!("a Stop with an open ticket cannot be Complete: {other:?}"),
    }
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(snap.phase, Phase::Quarantined);
    assert_eq!(snap.open_tickets, 1);
    assert!(snap.quarantine.is_some());
    ticket.withdraw();
    assert_eq!(sup.lifecycle().map(|l| l.open_tickets), Some(0));
    // The release re-checks the readers; one that ends a moment later is seen by the next read.
    let reconciled = wait_for(ACQUIRE, Duration::from_millis(20), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(
        reconciled.is_ok(),
        "the late release reconciles ownership: {:?}",
        sup.lifecycle()
    );
    drop(sup);
    fx.assert_clean();
}

/// Shutdown while a descendant still holds the output open and a ticket is open: the service
/// stays Quarantined. Once the descendant ends and the ticket is released, ownership reconciles
/// to Stopped without a monitor: the release path checks the readers itself.
#[test]
fn after_shutdown_quarantine_clears_once_readers_and_tickets_are_done() {
    let fx = OwnedFixture::new("close-both");
    let mut cfg = SupervisorConfig::new(
        fx.descendants_spec("lcf-close-both", 1),
        fx.path("crash.jsonl"),
    );
    cfg.stop_grace = Duration::from_millis(300);
    let mut sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let descendant = fx.expect_pids(2, ACQUIRE)[1];
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    sup.shutdown();
    match receipt_of(&sup, stop) {
        Receipt::Incomplete { remaining, .. } => {
            assert!(
                remaining.iter().any(|r| r.starts_with("reader")),
                "{remaining:?}"
            );
            assert!(
                remaining.iter().any(|r| r == "publication:bearer"),
                "{remaining:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Quarantined));
    // The descendant ends: the old readers reach end of file. Then the ticket is released.
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    hold_for(Duration::from_millis(300));
    ticket.withdraw();
    assert_eq!(sup.lifecycle().map(|l| l.open_tickets), Some(0));
    // The release re-checks the readers; a reader that reaches end of file a moment later is
    // observed by the next read. Either way the quarantine clears without a monitor.
    let cleared = wait_for(Duration::from_secs(15), Duration::from_millis(20), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(
        cleared.is_ok(),
        "readers and tickets are done: {:?}",
        sup.lifecycle()
    );
    drop(sup);
    fx.assert_clean();
}

/// Shutdown while a descendant holds the output open (no ticket): Quarantined, and once the
/// descendant ends, a snapshot read observes the readers finished and reports Stopped.
#[test]
fn after_shutdown_a_late_reader_end_clears_quarantine() {
    let fx = OwnedFixture::new("close-reader");
    let mut cfg = SupervisorConfig::new(
        fx.descendants_spec("lcf-close-reader", 1),
        fx.path("crash.jsonl"),
    );
    cfg.stop_grace = Duration::from_millis(300);
    let mut sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let descendant = fx.expect_pids(2, ACQUIRE)[1];
    sup.shutdown();
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Quarantined));
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    let cleared = wait_for(ACQUIRE, Duration::from_millis(20), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(
        cleared.is_ok(),
        "a late reader end is observed after shutdown"
    );
    drop(sup);
    fx.assert_clean();
}

/// A service already Quarantined on a held ticket stays Quarantined through shutdown.
#[test]
fn shutdown_keeps_a_held_ticket_quarantine() {
    let fx = OwnedFixture::new("close-held");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-close-held"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let mut sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("admission while running: {e:?}"));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    let settled = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    assert!(
        matches!(settled, Ok((Receipt::Incomplete { .. }, _))),
        "INCONCLUSIVE: {settled:?}"
    );
    assert_eq!(sup.lifecycle().map(|l| l.phase), Some(Phase::Quarantined));
    sup.shutdown();
    let snap = sup
        .lifecycle()
        .unwrap_or_else(|| panic!("INCONCLUSIVE: snapshot busy"));
    assert_eq!(
        snap.phase,
        Phase::Quarantined,
        "shutdown does not clear the quarantine"
    );
    assert_eq!(snap.open_tickets, 1);
    ticket.withdraw();
    drop(sup);
    fx.assert_clean();
}

// ---------------------------------------------------------------------------
// SCL-S2.4: the reserved deadline publisher. A deadline is armed in the control's own admission
// path; the publisher marks expiry without taking any lifecycle lock; a receipt read compares
// the monotonic deadline itself, so expiry is reported even if the publisher never ran; a
// publisher that cannot be created or that ends leaves deadline admission unavailable and is
// never replaced.
// ---------------------------------------------------------------------------

fn stubborn_config(fx: &OwnedFixture, name: &str, grace: Duration) -> SupervisorConfig {
    let mut cfg = SupervisorConfig::new(fx.stubborn_leaf_spec(name), fx.path("crash.jsonl"));
    cfg.stop_grace = grace;
    cfg
}

fn settle(sup: &Supervisor, token: ControlToken, timeout: Duration) -> Receipt {
    match wait_for(timeout, Duration::from_millis(5), || {
        match receipt_of(sup, token) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    }) {
        Ok((r, _)) => r,
        Err(e) => panic!("INCONCLUSIVE: no settled receipt within {:?}", e.waited),
    }
}

/// A Stop whose teardown outlasts its deadline reads DeadlineExpired, marked by the publisher;
/// the late completion updates the snapshot but never turns the receipt into Complete.
#[test]
fn stop_deadline_expires_while_teardown_is_held() {
    let fx = OwnedFixture::new("deadline");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let mut cfg = stubborn_config(&fx, "lcf-deadline", Duration::from_secs(4));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let token = match sup.request_stop_within(Duration::from_millis(200)) {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    assert_eq!(receipt_of(&sup, token), Receipt::Pending);
    assert_eq!(publisher.armed_records(), 1);
    let marked = wait_for(Duration::from_secs(3), Duration::from_millis(5), || {
        (publisher.record_state(sup.owner_id(), token) == Some(DeadlineState::Expired))
            .then_some(())
    });
    assert!(marked.is_ok(), "the publisher marked the expiry");
    assert_eq!(receipt_of(&sup, token), Receipt::DeadlineExpired);
    let stopped = wait_for(Duration::from_secs(10), Duration::from_millis(10), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(stopped.is_ok(), "the late teardown is still observed");
    assert_eq!(
        receipt_of(&sup, token),
        Receipt::DeadlineExpired,
        "never a late Complete"
    );
    assert_eq!(publisher.armed_records(), 0, "settling frees the record");
    drop(sup);
    fx.assert_clean();
}

/// A receipt read after the deadline reports expiry even when the publisher never observed it.
#[test]
fn receipt_reports_expiry_even_if_the_publisher_never_ran() {
    let fx = OwnedFixture::new("deadline-idle");
    // A publisher thread that is alive but never runs its body: it never observes anything.
    let publisher = DeadlinePublisher::start_with_spawner(Duration::from_millis(20), |body| {
        std::thread::Builder::new().spawn(move || {
            let _never_run = body;
            loop {
                std::thread::park();
            }
        })
    });
    assert!(publisher.is_running());
    let mut cfg = stubborn_config(&fx, "lcf-deadline-idle", Duration::from_millis(1200));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let token = match sup.request_stop_within(Duration::from_millis(150)) {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    hold_for(Duration::from_millis(250));
    assert_eq!(
        publisher.record_state(sup.owner_id(), token),
        Some(DeadlineState::Armed),
        "the publisher did not mark it"
    );
    assert_eq!(receipt_of(&sup, token), Receipt::DeadlineExpired);
    drop(sup);
    fx.assert_clean();
}

/// A Stop that settles before its deadline is Complete, and its record is freed.
#[test]
fn stop_settled_before_its_deadline_is_complete() {
    let fx = OwnedFixture::new("deadline-ok");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-deadline-ok"), fx.path("crash.jsonl"));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let token = match sup.request_stop_within(Duration::from_secs(5)) {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    assert!(matches!(
        settle(&sup, token, ACQUIRE),
        Receipt::Complete { .. }
    ));
    assert_eq!(publisher.armed_records(), 0);
    drop(sup);
    fx.assert_clean();
}

/// Concurrent deadline Stops arm one record and join one token; the first deadline is absolute
/// and a joining call does not restart it.
#[test]
fn concurrent_deadline_stops_arm_one_record() {
    let fx = OwnedFixture::new("deadline-race");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let mut cfg = stubborn_config(&fx, "lcf-deadline-race", Duration::from_secs(3));
    cfg.deadlines = Some(publisher.clone());
    let sup = std::sync::Arc::new(
        Supervisor::start(cfg)
            .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}")),
    );
    running_pid(&sup, &fx);
    let callers: Vec<_> = (0..8)
        .map(|_| {
            let s = sup.clone();
            std::thread::spawn(move || loop {
                match s.request_stop_within(Duration::from_millis(300)) {
                    ControlResult::Busy => std::thread::yield_now(),
                    r => return r,
                }
            })
        })
        .collect();
    let results: Vec<ControlResult> = callers.into_iter().filter_map(|t| t.join().ok()).collect();
    let tokens: std::collections::HashSet<ControlToken> = results
        .iter()
        .map(|r| match r {
            ControlResult::Accepted(t) => *t,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(tokens.len(), 1, "every caller joined one Stop");
    assert_eq!(publisher.armed_records(), 1, "one record, no queue");
    let token = tokens
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no token"));
    // A later joining call does not extend the deadline.
    hold_for(Duration::from_millis(200));
    assert_eq!(
        sup.request_stop_within(Duration::from_secs(60)),
        ControlResult::Accepted(token)
    );
    assert_eq!(
        settle(&sup, token, Duration::from_secs(10)),
        Receipt::DeadlineExpired
    );
    drop(sup);
    fx.assert_clean();
}

/// The overlay has a fixed number of records. Arming beyond it answers Busy and commits nothing.
#[test]
fn deadline_overlay_is_fixed_and_full_admission_commits_nothing() {
    let fx = OwnedFixture::new("deadline-full");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let sups: Vec<Supervisor> = (0..=DEADLINE_RECORDS)
        .map(|i| {
            let mut cfg =
                stubborn_config(&fx, &format!("lcf-full-{i}"), Duration::from_millis(1500));
            cfg.deadlines = Some(publisher.clone());
            Supervisor::start(cfg)
                .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"))
        })
        .collect();
    for s in &sups {
        running_pid(s, &fx);
    }
    let mut accepted = 0;
    for s in &sups[..DEADLINE_RECORDS] {
        if let ControlResult::Accepted(_) = s.request_stop_within(Duration::from_secs(30)) {
            accepted += 1;
        }
    }
    assert_eq!(accepted, DEADLINE_RECORDS);
    assert_eq!(publisher.armed_records(), DEADLINE_RECORDS);
    let last = &sups[DEADLINE_RECORDS];
    let before = last.lifecycle().map(|l| l.intent_sequence);
    assert_eq!(
        last.request_stop_within(Duration::from_secs(30)),
        ControlResult::Busy
    );
    assert_eq!(
        last.lifecycle().map(|l| l.intent_sequence),
        before,
        "nothing committed"
    );
    assert_eq!(
        last.status().state,
        SupervisorState::Running,
        "and nothing stopped"
    );
    drop(sups);
    fx.assert_clean();
}

/// A Stop whose deadline passed while its teardown is still held stays DeadlineExpired when a
/// Start supersedes it before it settles: DeadlineExpired is final.
#[test]
fn an_expired_stop_superseded_before_it_settles_stays_deadline_expired() {
    let fx = OwnedFixture::new("deadline-superseded");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let mut cfg = stubborn_config(&fx, "lcf-deadline-superseded", Duration::from_secs(4));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let a = match sup.request_stop_within(Duration::from_millis(100)) {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    let expired = wait_for(Duration::from_secs(3), Duration::from_millis(5), || {
        (receipt_of(&sup, a) == Receipt::DeadlineExpired).then_some(())
    });
    assert!(
        expired.is_ok(),
        "INCONCLUSIVE: the Stop never read DeadlineExpired"
    );
    if sup.lifecycle().map(|l| l.phase) != Some(Phase::Stopping) {
        panic!("INCONCLUSIVE: the teardown was not held long enough to supersede the Stop");
    }
    let b = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start during the held teardown: {other:?}"),
    };
    assert_eq!(
        receipt_of(&sup, a),
        Receipt::DeadlineExpired,
        "superseding an expired Stop keeps it DeadlineExpired"
    );
    assert!(matches!(
        settle(&sup, b, Duration::from_secs(15)),
        Receipt::Started { .. }
    ));
    assert!(matches!(
        receipt_of(&sup, a),
        Receipt::DeadlineExpired | Receipt::Expired
    ));
    drop(sup);
    fx.assert_clean();
}

/// An expired Stop followed by later controls never reads Complete: it stays DeadlineExpired
/// while retained, then Expired, inside the fixed receipt storage.
#[test]
fn expired_stop_followed_by_later_controls_never_reads_complete() {
    let fx = OwnedFixture::new("deadline-later");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let mut cfg = stubborn_config(&fx, "lcf-deadline-later", Duration::from_millis(600));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let a = match sup.request_stop_within(Duration::from_millis(100)) {
        ControlResult::Accepted(t) => t,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        settle(&sup, a, Duration::from_secs(15)),
        Receipt::DeadlineExpired
    );
    for _ in 0..3 {
        let start = match sup.request_start() {
            ControlResult::Accepted(t) => t,
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            settle(&sup, start, ACQUIRE),
            Receipt::Started { .. }
        ));
        running_pid(&sup, &fx);
        let stop = match sup.request_stop_within(Duration::from_secs(10)) {
            ControlResult::Accepted(t) => t,
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            settle(&sup, stop, Duration::from_secs(15)),
            Receipt::Complete { .. }
        ));
        let late = receipt_of(&sup, a);
        assert!(
            matches!(late, Receipt::Expired | Receipt::DeadlineExpired),
            "the expired Stop never reads as done: {late:?}"
        );
        assert!(publisher.armed_records() <= 1);
    }
    drop(sup);
    fx.assert_clean();
}

/// A deadline Stop that joins a pending plain Stop applies its deadline to that Stop; a later,
/// tighter deadline replaces a looser one, never the other way round.
#[test]
fn a_deadline_stop_joining_a_plain_stop_applies_the_tighter_deadline() {
    let fx = OwnedFixture::new("deadline-join");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let sups: Vec<Supervisor> = (0..2)
        .map(|i| {
            let mut cfg = stubborn_config(&fx, &format!("lcf-join-{i}"), Duration::from_secs(4));
            cfg.deadlines = Some(publisher.clone());
            Supervisor::start(cfg)
                .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"))
        })
        .collect();
    for s in &sups {
        running_pid(s, &fx);
    }
    // A plain Stop, then a deadline Stop joining it.
    let plain = match sups[0].request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        sups[0].request_stop_within(Duration::from_millis(200)),
        ControlResult::Accepted(plain)
    );
    // A loose deadline Stop, then a tighter one joining it.
    let loose = match sups[1].request_stop_within(Duration::from_secs(30)) {
        ControlResult::Accepted(t) => t,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        sups[1].request_stop_within(Duration::from_millis(200)),
        ControlResult::Accepted(loose)
    );
    assert_eq!(publisher.armed_records(), 2, "one record per Stop");
    assert_eq!(
        settle(&sups[0], plain, Duration::from_secs(10)),
        Receipt::DeadlineExpired
    );
    assert_eq!(
        settle(&sups[1], loose, Duration::from_secs(10)),
        Receipt::DeadlineExpired
    );
    drop(sups);
    fx.assert_clean();
}

/// With the monitor gone, a deadline Stop is committed and reported ObservationUnknown, and it
/// does not occupy a deadline record nobody would ever free.
#[test]
fn a_deadline_stop_without_a_monitor_holds_no_record() {
    let fx = OwnedFixture::new("deadline-nomon");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let mut spec = fx.leaf_spec("lcf-deadline-nomon");
    // The hook ends the monitor once the child has recorded itself (the ended monitor still
    // terminates and reaps its child, so a panic before that would leave nothing to observe).
    let pidfile = fx.path("pids");
    spec.child_watch = Some(ChildWatch {
        interval: Duration::from_millis(20),
        observe: std::sync::Arc::new(move |_| {
            let recorded = std::fs::read_to_string(&pidfile).is_ok_and(|s| !s.trim().is_empty());
            if recorded {
                panic!("deliberate: end the monitor thread");
            }
        }),
        ended: std::sync::Arc::new(|| {}),
        prepare: None,
    });
    let mut cfg = SupervisorConfig::new(spec, fx.path("crash.jsonl"));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    fx.expect_pids(1, ACQUIRE);
    let gone = wait_for(ACQUIRE, Duration::from_millis(10), || {
        (!sup.monitor_running()).then_some(())
    });
    assert!(gone.is_ok(), "INCONCLUSIVE: the monitor did not end");
    assert!(matches!(
        sup.request_stop_within(Duration::from_secs(30)),
        ControlResult::ObservationUnknown(..)
    ));
    assert_eq!(
        publisher.armed_records(),
        0,
        "no record is left for a dead monitor"
    );
    // Joining that Stop with a deadline is ObservationUnknown too, and arms nothing.
    assert!(matches!(
        sup.request_stop_within(Duration::from_millis(100)),
        ControlResult::ObservationUnknown(..)
    ));
    assert_eq!(publisher.armed_records(), 0);
    drop(sup);
    fx.assert_clean();
}

/// Dropping a supervisor frees every deadline record it still holds.
#[test]
fn dropping_a_supervisor_frees_its_deadline_records() {
    let fx = OwnedFixture::new("deadline-drop");
    let publisher = DeadlinePublisher::start(Duration::from_millis(20));
    let mut cfg = stubborn_config(&fx, "lcf-deadline-drop", Duration::from_millis(300));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    assert!(matches!(
        sup.request_stop_within(Duration::from_secs(30)),
        ControlResult::Accepted(_)
    ));
    assert_eq!(publisher.armed_records(), 1);
    drop(sup);
    assert_eq!(publisher.armed_records(), 0);
    fx.assert_clean();
}

/// A publisher thread that cannot be created reports an initialization error; deadline
/// admission is unavailable, commits nothing, and no replacement thread is ever attempted.
#[test]
fn publisher_creation_failure_leaves_deadline_admission_unavailable() {
    let fx = OwnedFixture::new("pub-fail");
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let a = attempts.clone();
    let publisher =
        DeadlinePublisher::start_with_spawner(Duration::from_millis(20), move |_body| {
            a.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(std::io::Error::other("refused by the fixture"))
        });
    assert!(publisher.init_error().is_some());
    assert!(!publisher.is_running());
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-fail"), fx.path("crash.jsonl"));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let before = sup.lifecycle().map(|l| l.intent_sequence);
    for _ in 0..5 {
        match sup.request_stop_within(Duration::from_secs(1)) {
            ControlResult::Unavailable(reason) => assert!(reason.contains("refused"), "{reason}"),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }
    assert_eq!(
        sup.lifecycle().map(|l| l.intent_sequence),
        before,
        "nothing committed"
    );
    assert_eq!(
        attempts.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "no replacement"
    );
    drop(sup);
    fx.assert_clean();
}

/// A publisher thread that ends unexpectedly leaves deadline admission unavailable, and no
/// replacement thread is created.
#[test]
fn publisher_exit_leaves_admission_unavailable_without_replacement() {
    let fx = OwnedFixture::new("pub-exit");
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let a = attempts.clone();
    let publisher =
        DeadlinePublisher::start_with_spawner(Duration::from_millis(20), move |_body| {
            a.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // The thread starts and panics at once, without ever running the publisher body.
            std::thread::Builder::new().spawn(|| panic!("deliberate: the publisher dies"))
        });
    let gone = wait_for(ACQUIRE, Duration::from_millis(5), || {
        (!publisher.is_running()).then_some(())
    });
    assert!(
        gone.is_ok(),
        "INCONCLUSIVE: the publisher thread did not end"
    );
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-exit"), fx.path("crash.jsonl"));
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    for _ in 0..3 {
        assert!(matches!(
            sup.request_stop_within(Duration::from_secs(1)),
            ControlResult::Unavailable(_)
        ));
    }
    assert_eq!(
        attempts.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "no replacement"
    );
    assert_eq!(sup.status().state, SupervisorState::Running);
    drop(sup);
    fx.assert_clean();
}

/// While a publication's activation step runs, no Stop can commit: the step and the currency
/// check are one critical section, so the file can never become active under an intent that a
/// Stop has already replaced.
#[test]
fn no_stop_commits_while_a_publication_activates() {
    let fx = OwnedFixture::new("pub-activate");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-activate"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: admission while running: {e:?}"));
    let during = ticket.commit(|| {
        // A Stop from another thread while the file is being made active.
        let r = std::thread::scope(|s| s.spawn(|| sup.request_stop()).join());
        Ok::<_, std::convert::Infallible>(r)
    });
    let during = match during {
        Ok(Ok(r)) => r,
        other => panic!("INCONCLUSIVE: the activation step did not complete: {other:?}"),
    };
    assert!(
        !matches!(during, ControlResult::Accepted(_)),
        "a Stop committed while the publication was being activated: {during:?}"
    );
    assert_eq!(sup.lifecycle().map(|l| l.open_tickets), Some(0));
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop after the activation: {other:?}"),
    };
    assert!(
        matches!(settle(&sup, stop, ACQUIRE), Receipt::Complete { .. }),
        "a committed ticket leaves nothing for the Stop to wait on"
    );
    drop(sup);
    fx.assert_clean();
}

/// An activation step that fails leaves the ticket open: the Stop names the publication until
/// the caller removes what it wrote and says so.
#[test]
fn a_failed_activation_keeps_the_ticket_open_until_cleaned() {
    let fx = OwnedFixture::new("pub-actfail");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-actfail"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: admission while running: {e:?}"));
    let open = match ticket.commit(|| Err::<(), _>("rename failed")) {
        Err(CommitRefused::Failed { ticket, error }) => {
            assert_eq!(error, "rename failed");
            ticket
        }
        other => panic!("a failed activation must be reported with its ticket: {other:?}"),
    };
    assert_eq!(
        sup.lifecycle().map(|l| l.open_tickets),
        Some(1),
        "a failed activation must not release the publication"
    );
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    let settled = wait_for(ACQUIRE, Duration::from_millis(5), || {
        match receipt_of(&sup, stop) {
            Receipt::Pending | Receipt::Unknown { .. } => None,
            r => Some(r),
        }
    });
    match settled {
        Ok((Receipt::Incomplete { remaining, .. }, _)) => assert!(
            remaining.iter().any(|r| r == "publication:bearer"),
            "{remaining:?}"
        ),
        other => panic!("a Stop with an uncleaned publication is not Complete: {other:?}"),
    }
    open.cleaned();
    let released = wait_for(ACQUIRE, Duration::from_millis(10), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(released.is_ok(), "cleaning the ticket leaves quarantine");
    drop(sup);
    fx.assert_clean();
}

/// An activation step that unwinds drops the ticket unresolved: the publication stays owned.
#[test]
fn an_activation_that_unwinds_keeps_the_publication_owned() {
    let fx = OwnedFixture::new("pub-actpanic");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-actpanic"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: admission while running: {e:?}"));
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ticket.commit(|| -> Result<(), std::convert::Infallible> {
            panic!("deliberate: the activation step fails by unwinding")
        });
    }));
    assert!(unwound.is_err(), "the activation step unwound as intended");
    assert_eq!(
        sup.lifecycle().map(|l| l.open_tickets),
        Some(1),
        "an unwound activation must not release the publication"
    );
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop after the unwind: {other:?}"),
    };
    match &settle(&sup, stop, ACQUIRE) {
        Receipt::Incomplete { remaining, .. } => assert!(
            remaining.iter().any(|r| r == "publication:bearer"),
            "{remaining:?}"
        ),
        other => panic!("a Stop with an unresolved publication is not Complete: {other:?}"),
    }
    drop(sup);
    fx.assert_clean();
}

/// A stale ticket never runs its activation step.
#[test]
fn a_stale_ticket_never_runs_its_activation_step() {
    let fx = OwnedFixture::new("pub-actstale");
    let mut cfg = SupervisorConfig::new(fx.leaf_spec("lcf-pub-actstale"), fx.path("crash.jsonl"));
    cfg.stop_grace = Duration::from_millis(500);
    let sup = Supervisor::start(cfg)
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    running_pid(&sup, &fx);
    let ticket = sup
        .admit_publication("bearer")
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: admission while running: {e:?}"));
    assert!(matches!(sup.request_stop(), ControlResult::Accepted(_)));
    let mut ran = false;
    match ticket.commit(|| {
        ran = true;
        Ok::<(), std::convert::Infallible>(())
    }) {
        Err(CommitRefused::Stale(stale)) => stale.cleaned(),
        other => panic!("a ticket admitted before Stop must not become active: {other:?}"),
    }
    assert!(!ran, "the activation step ran for a stale ticket");
    drop(sup);
    fx.assert_clean();
}

// ---------------------------------------------------------------------------
// Suite manifest
// ---------------------------------------------------------------------------

/// Every `#[test]` in this file is listed in the manifest, and every manifest entry exists.
/// (The seed of the SCL-S13.2 tripwire.)
#[test]
fn manifest_lists_every_fixture() {
    let source = include_str!("lifecycle_faults.rs");
    let manifest = include_str!("lifecycle_faults.manifest");
    let mut in_source = Vec::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "#[test]" {
            // Skip attributes and doc comments until the fn line.
            for next in lines.by_ref() {
                let t = next.trim();
                if let Some(rest) = t.strip_prefix("fn ") {
                    if let Some(name) = rest.split('(').next() {
                        in_source.push(name.trim().to_string());
                    }
                    break;
                }
            }
        }
    }
    let in_manifest: Vec<String> = manifest
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split('|').next().map(|n| n.trim().to_string()))
        .collect();
    for name in &in_source {
        assert!(
            in_manifest.contains(name),
            "{name} is not listed in the manifest"
        );
    }
    for name in &in_manifest {
        assert!(
            in_source.contains(name),
            "manifest lists {name}, which is not in the suite"
        );
    }
    assert!(!in_source.is_empty());
}
