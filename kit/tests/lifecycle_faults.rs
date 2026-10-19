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
    BackoffPolicy, ChildWatch, ControlResult, Desired, Phase, Receipt, Supervisor,
    SupervisorConfig, SupervisorState,
};
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
        !pid_alive(pid),
        "the child is gone after every Stop returned"
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
        sup.receipt(first),
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
        match sup.receipt(first) {
            Receipt::Pending => None,
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
    assert!(!pid_alive(pid));
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
    let token = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start after Stop: {other:?}"),
    };
    let pids = fx.expect_pids(2, ACQUIRE);
    let second_pid = running_pid(&sup, &fx);
    assert_eq!(pids[1], second_pid);
    assert_ne!(first_pid, second_pid);
    let started = wait_for(ACQUIRE, Duration::from_millis(10), || {
        matches!(sup.receipt(token), Receipt::Started { .. }).then_some(())
    });
    assert!(
        started.is_ok(),
        "the Start receipt reports the spawn: {:?}",
        sup.receipt(token)
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
        match sup.receipt(token) {
            Receipt::Pending => None,
            r => Some(r),
        }
    });
    let receipt = match settled {
        Ok((r, _)) => r,
        Err(e) => panic!("INCONCLUSIVE: the Stop never settled within {:?}", e.waited),
    };
    assert!(!pid_alive(parent), "the direct child is gone");
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
    assert_eq!(fx.pidfile_pids().len(), 2, "no new child while quarantined");
    // The descendant ends, the pipe closes, the reader finishes: ownership is reconciled.
    lifecycle::fixture::kill(descendant, libc::SIGKILL);
    let released = wait_for(ACQUIRE, Duration::from_millis(20), || {
        (sup.lifecycle().map(|l| l.phase) == Some(Phase::Stopped)).then_some(())
    });
    assert!(released.is_ok(), "late reader completion leaves quarantine");
    assert!(
        matches!(sup.receipt(token), Receipt::Incomplete { .. }),
        "the receipt keeps what was observed at the time"
    );
    assert!(matches!(sup.request_start(), ControlResult::Accepted(_)));
    fx.expect_pids(3, ACQUIRE);
    drop(sup);
    fx.assert_clean();
}

/// When the service monitor is gone, a Stop is still committed (the snapshot shows it), and the
/// caller is told the outcome cannot be observed, never that it succeeded.
#[test]
fn stop_without_a_monitor_is_observation_unknown() {
    let fx = OwnedFixture::new("no-monitor");
    let mut spec = fx.leaf_spec("lcf-no-monitor");
    // A child-watch hook that panics on the monitor thread ends the monitor.
    spec.child_watch = Some(ChildWatch {
        interval: Duration::from_millis(20),
        observe: std::sync::Arc::new(|_| panic!("deliberate: end the monitor thread")),
        ended: std::sync::Arc::new(|| {}),
    });
    let sup = Supervisor::start(SupervisorConfig::new(spec, fx.path("crash.jsonl")))
        .unwrap_or_else(|e| panic!("INCONCLUSIVE: supervisor did not start: {e}"));
    fx.expect_pids(1, ACQUIRE);
    let gone = wait_for(ACQUIRE, Duration::from_millis(10), || {
        (!sup.monitor_running()).then_some(())
    });
    assert!(gone.is_ok(), "INCONCLUSIVE: the monitor did not end");
    let before = sup.lifecycle().map(|l| l.intent_sequence);
    match sup.request_stop() {
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
        }
        other => panic!("expected ObservationUnknown, got {other:?}"),
    }
    drop(sup);
    fx.assert_clean();
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
    assert!(!pid_alive(pid));
    assert_eq!(sup.request_start(), ControlResult::RejectedClosing);
    hold_for(Duration::from_millis(300));
    assert_eq!(fx.pidfile_pids().len(), 1, "no child after Closing");
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
