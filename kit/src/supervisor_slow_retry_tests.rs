// citrate-core — CORE-D3: the slow retry after `Failed` (planset r1007-v060 §CORE-D).
//
// Without a slow retry a service that spent its retry budget stays `Failed` until a member Start
// (SPEC-001). With one configured, the monitor tries it again once per period: exactly one attempt,
// under a spent budget, so a further failure returns it to `Failed` at once and the bound on
// attempts stays one per period. A Stop or shutdown cancels the pending retry. Platform-neutral
// except the recovery case, which needs a real binary.

fn wait_lifecycle(
    sup: &Supervisor,
    limit: Duration,
    what: &str,
    pred: impl Fn(&LifecycleSnapshot) -> bool,
) -> LifecycleSnapshot {
    let deadline = std::time::Instant::now() + limit;
    loop {
        let snap = sup.lifecycle();
        if let Some(s) = snap.as_ref().filter(|s| pred(s)) {
            return s.clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "INCONCLUSIVE: {what}: {snap:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn without_a_slow_retry_a_failed_service_stays_failed() {
    let sup = Supervisor::start(missing_bin_config("slow-off", 0)).expect("start");
    let failed = wait_failed(&sup);
    std::thread::sleep(Duration::from_millis(300));
    let later = sup.lifecycle().expect("lifecycle");
    assert_eq!(later.phase, Phase::Failed);
    assert_eq!(
        later.counters.attempts, failed.counters.attempts,
        "no slow retry configured: no further attempt"
    );
    let obs = sup.observe().expect("observe");
    assert_eq!(obs.slow_retry, None, "nothing is scheduled");
}

#[test]
fn a_failed_service_is_retried_once_per_slow_retry_period() {
    let mut cfg = missing_bin_config("slow-on", 1);
    cfg.slow_retry = Some(Duration::from_millis(300));
    let sup = Supervisor::start(cfg).expect("start");
    let failed = wait_failed(&sup);
    assert_eq!(failed.counters.attempts, 2, "first attempt plus one retry");

    let obs = sup.observe().expect("observe");
    let slow = obs
        .slow_retry
        .expect("a Failed service with a slow retry says when it is tried next");
    assert_eq!(slow.every_ms, 300);
    assert_eq!(slow.attempts, 0, "no slow retry has run yet");
    assert!(slow.next_at_unix_ms > 0);

    // Not before the period.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        sup.lifecycle().expect("lifecycle").counters.attempts,
        2,
        "the slow retry waits out its period"
    );

    // Exactly one attempt when it comes, then Failed again (the budget stays spent).
    let retried = wait_lifecycle(&sup, Duration::from_secs(3), "one slow retry", |s| {
        s.counters.attempts >= 3 && s.phase == Phase::Failed
    });
    assert_eq!(
        retried.counters.attempts, 3,
        "one attempt per period, never a fresh budget of retries"
    );
    let obs = sup.observe().expect("observe");
    let slow = obs.slow_retry.expect("the next slow retry is scheduled");
    assert_eq!(slow.attempts, 1);
}

#[test]
fn a_stop_cancels_a_pending_slow_retry() {
    let mut cfg = missing_bin_config("slow-stop", 0);
    cfg.slow_retry = Some(Duration::from_millis(200));
    let sup = Supervisor::start(cfg).expect("start");
    let failed = wait_failed(&sup);
    let token = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop on a Failed service: {other:?}"),
    };
    assert!(matches!(wait_receipt(&sup, token), Receipt::Complete { .. }));
    std::thread::sleep(Duration::from_millis(500));
    let after = sup.lifecycle().expect("lifecycle");
    assert_eq!(after.phase, Phase::Stopped, "a Stop wins over the slow retry");
    assert_eq!(
        after.counters.attempts, failed.counters.attempts,
        "no attempt after the Stop"
    );
    assert_eq!(sup.observe().expect("observe").slow_retry, None);
}

/// The slow retry is what brings a service back once the cause went away: here the binary was
/// missing and then appears.
#[cfg(unix)]
#[test]
fn a_slow_retry_recovers_a_service_whose_cause_went_away() {
    let dir = std::env::temp_dir().join(format!(
        "citrate-core-sup-slow-recover-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create dir");
    let bin = dir.join("sleeper");
    let spec = SidecarSpec::new("slow-recover", bin.clone(), vec!["30".into()]);
    let mut cfg = SupervisorConfig::new(spec, dir.join("crash.jsonl"));
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 1,
        max_delay: Duration::from_millis(5),
        max_retries: 0,
    };
    cfg.slow_retry = Some(Duration::from_millis(200));
    let mut sup = Supervisor::start(cfg).expect("start");
    wait_failed(&sup);

    let sleep = ["/bin/sleep", "/usr/bin/sleep"]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.exists())
        .expect("a sleep binary");
    std::os::unix::fs::symlink(&sleep, &bin).expect("link the binary into place");

    let alive = wait_lifecycle(&sup, Duration::from_secs(5), "recovered to Alive", |s| {
        s.phase == Phase::Alive
    });
    assert!(alive.pid.is_some());
    assert_eq!(sup.observe().expect("observe").slow_retry, None, "nothing scheduled while Alive");
    sup.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// INV-17 as amended 2026-11-04 (owner decision, core#600) and INV-17a, over every observation of
/// a never-healthy service across several slow retries, for several budgets. With no member Start
/// in between:
/// - while `Failed`, the failures recorded since the member Start are exactly
///   `1 + max_retries + slow_retries` (the amended bound, reached with equality: each slow retry
///   adds one failure and no more), and the budget is spent (`max_retries + 1` counted);
/// - after the first `Failed`, no observation is ever `Backoff`, and the budget never drops below
///   `max_retries` counted: a slow retry is one attempt, never a fresh budget;
/// - at every observation the failures stay within the bound, counting the one slow retry that
///   may be in flight (including the instant between arming it and admitting its spawn, when the
///   phase still reads `Failed` with nothing scheduled).
#[test]
fn slow_retries_never_give_a_fresh_budget() {
    for max_retries in [0_u32, 1, 2] {
        let mut cfg = missing_bin_config(&format!("slow-bound-{max_retries}"), max_retries);
        cfg.slow_retry = Some(Duration::from_millis(40));
        let sup = Supervisor::start(cfg).expect("start");
        let bound = |slow: u64| 1 + u64::from(max_retries) + slow;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen_failed = false;
        let mut slow_seen: u64 = 0;
        let mut failed_observations = 0_u32;
        while slow_seen < 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "INCONCLUSIVE: max_retries {max_retries}: {slow_seen} slow retries observed"
            );
            let obs = sup.observe().expect("observe");
            let snap = &obs.lifecycle;
            let failures = u64::from(snap.restarts);
            let counted = snap.counters.consecutive_failures;
            if snap.phase == Phase::Failed && obs.slow_retry.is_none() {
                // A slow retry is armed (`arm_slow_retry`) but its spawn is not admitted yet: the
                // phase is still Failed for that instant. One attempt is in flight.
                assert_eq!(
                    counted, max_retries,
                    "max_retries {max_retries}: an armed slow retry leaves the budget spent"
                );
                assert!(
                    failures <= bound(slow_seen + 1),
                    "{failures} failures, armed"
                );
            } else if snap.phase == Phase::Failed {
                let slow = obs.slow_retry.expect("checked above");
                assert!(
                    slow.attempts >= slow_seen,
                    "the slow-retry count never goes back"
                );
                slow_seen = slow.attempts;
                seen_failed = true;
                failed_observations += 1;
                assert_eq!(
                    failures,
                    bound(slow_seen),
                    "max_retries {max_retries}: while Failed, 1 + max_retries + slow retries \
                     failures since the member Start (INV-17 amended, INV-17a)"
                );
                assert_eq!(
                    counted,
                    max_retries + 1,
                    "max_retries {max_retries}: Failed means the budget is spent"
                );
            } else if seen_failed {
                assert_ne!(
                    snap.phase,
                    Phase::Backoff,
                    "max_retries {max_retries}: a slow retry never backs off under a fresh budget"
                );
                assert!(
                    counted >= max_retries,
                    "max_retries {max_retries}: a slow retry left {counted} counted failures"
                );
                assert!(
                    failures <= bound(slow_seen + 1),
                    "max_retries {max_retries}: {failures} failures with at most {} slow retries",
                    slow_seen + 1
                );
            } else {
                assert!(
                    failures <= bound(0),
                    "before any slow retry: {failures} failures"
                );
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            failed_observations > 3,
            "INCONCLUSIVE: Failed seen {failed_observations} times"
        );
    }
}

/// A monitor seam the test arms: unarmed it lets the monitor through; armed, it holds the next
/// monitor turn that reaches it until released (or until the guard drops, so a failing assertion
/// never leaves the monitor parked).
#[derive(Default)]
struct SlowRetryGate {
    state: std::sync::Mutex<(bool, bool, bool)>, // (armed, held, released)
    cv: std::sync::Condvar,
}

impl SlowRetryGate {
    fn hook(gate: &Arc<SlowRetryGate>) -> Arc<dyn Fn() + Send + Sync> {
        let g = gate.clone();
        Arc::new(move || {
            let mut s = g.state.lock().unwrap_or_else(|p| p.into_inner());
            if !s.0 || s.2 {
                return;
            }
            s.1 = true;
            g.cv.notify_all();
            while !s.2 {
                s = g.cv.wait(s).unwrap_or_else(|p| p.into_inner());
            }
        })
    }

    fn arm(&self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).0 = true;
    }

    fn wait_held(&self, limit: Duration) -> bool {
        let deadline = std::time::Instant::now() + limit;
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        while !s.1 {
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            s = self
                .cv
                .wait_timeout(s, deadline - now)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
        true
    }

    fn release(&self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).2 = true;
        self.cv.notify_all();
    }
}

struct ReleaseOnDrop(Arc<SlowRetryGate>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// A due slow retry is published in one step: no read ever sees the service `Failed` with no
/// slow retry scheduled. The monitor is held at the budget seam, after the slow retry is armed and
/// before its spawn is admitted (the instant core#600 left as `Failed` with nothing scheduled);
/// there the service already reads `Starting`, with the budget left spent but for this attempt.
#[test]
fn an_armed_slow_retry_never_reads_failed_with_nothing_scheduled() {
    let gate = Arc::new(SlowRetryGate::default());
    let mut cfg = missing_bin_config("slow-armed", 1);
    cfg.slow_retry = Some(Duration::from_millis(50));
    cfg.budget_barrier = Some(SlowRetryGate::hook(&gate));
    let sup = Supervisor::start(cfg).expect("start");
    let _release = ReleaseOnDrop(gate.clone());
    let failed = wait_failed(&sup);
    assert_eq!(
        failed.counters.consecutive_failures, 2,
        "the budget is spent"
    );

    gate.arm();
    assert!(
        gate.wait_held(Duration::from_secs(5)),
        "INCONCLUSIVE: the slow retry never reached the budget seam"
    );
    let obs = sup.observe().expect("observe");
    assert!(
        !(obs.lifecycle.phase == Phase::Failed && obs.slow_retry.is_none()),
        "a Failed service with a slow retry configured reads as having none scheduled: {:?}",
        obs.lifecycle
    );
    assert_eq!(
        obs.lifecycle.phase,
        Phase::Starting,
        "the slow retry is published as the attempt it is"
    );
    assert_eq!(
        obs.lifecycle.counters.consecutive_failures, 1,
        "one attempt under the spent budget"
    );
    assert_eq!(sup.status().state, SupervisorState::Starting);

    gate.release();
    let again = wait_lifecycle(&sup, Duration::from_secs(5), "Failed again", |s| {
        s.phase == Phase::Failed && s.restarts == 3
    });
    assert_eq!(again.counters.consecutive_failures, 2);
    let slow = sup.observe().expect("observe").slow_retry;
    assert!(
        slow.is_some_and(|s| s.attempts == 1),
        "Failed again with the next slow retry scheduled: {slow:?}"
    );
}
