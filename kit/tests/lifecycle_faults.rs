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

use citrate_core_kit::supervisor::{BackoffPolicy, Supervisor, SupervisorConfig, SupervisorState};
use lifecycle::fixture::{pgrep, pid_alive, sleep_bin, OwnedFixture, SUITE_PREFIX};
use lifecycle::timing::{hold_for, sample_over, wait_for, Deadline};

const ACQUIRE: Duration = Duration::from_secs(5);

fn fast_backoff(max_retries: u32) -> BackoffPolicy {
    BackoffPolicy {
        base_delay: Duration::from_millis(20),
        multiplier: 2,
        max_delay: Duration::from_millis(80),
        max_retries,
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
    fx.spawn_direct(vec![
        "-c".into(),
        format!(
            "{} -f \"$1\" >/dev/null 2>&1 & exit 0",
            lifecycle::fixture::tail_bin().display()
        ),
        fx.marker().to_string(),
        fx.path("anchor").display().to_string(),
    ]);
    let seen = wait_for(ACQUIRE, Duration::from_millis(10), || {
        fx.live_marked().first().copied()
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
    assert!(!pid_alive(pid), "dropping the supervisor ends the child");
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
    assert!(!pid_alive(pid), "the child is gone after Stop returns");
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
    assert!(!pid_alive(first));
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
