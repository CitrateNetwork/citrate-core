// 0.5.7 fd-limit lane: a sidecar that exits on start with the same error is named as a crash
// loop, carrying the child's own error line, while the supervisor keeps retrying with backoff.
// The child is a real `/bin/sh` that prints the node's 0.5.7 EMFILE line to stderr and exits 1.

/// The line the 0.5.7 node printed on every start (path shortened).
const EMFILE_LINE: &str =
    "Error: IO error: While open a file for random read: /data/node/003500.sst: Too many open files";

fn emfile_spec() -> SidecarSpec {
    let script = format!("echo 'INFO opening the chain database'; echo '{EMFILE_LINE}' 1>&2; exit 1");
    SidecarSpec::new("citrate-node", sh_bin(), vec!["-c".into(), script])
}

fn wait_for_loop(sup: &Supervisor, within: Duration) -> Option<crate::crash_loop::CrashLoop> {
    let deadline = std::time::Instant::now() + within;
    loop {
        if let Some(l) = sup.crash_loop() {
            return Some(l);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn repeated_emfile_exits_become_a_crash_loop_carrying_the_error_and_retries_go_on() {
    let tmp = TmpDir::new("crashloop-emfile");
    let mut cfg = SupervisorConfig::new(emfile_spec(), tmp.path("crashes.jsonl"));
    cfg.clock = FakeClock::new();
    // A budget far above the loop threshold: the loop is named long before Failed.
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 50,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let l = wait_for_loop(&sup, Duration::from_secs(15)).expect("the repeated EMFILE exits are named a crash loop");
    assert_eq!(l.kind, crate::crash_loop::CrashKind::TooManyOpenFiles);
    assert_eq!(l.line, EMFILE_LINE, "the loop carries the child's own error line");
    assert!(l.count >= crate::crash_loop::CRASH_LOOP_THRESHOLD, "count {}", l.count);
    assert!(l.last_at_unix_ms >= l.first_at_unix_ms);

    // Naming the loop does not give up: the supervisor is still restarting with backoff.
    let st = sup.status();
    assert_ne!(st.state, SupervisorState::Failed, "the loop was named, not given up on: {st:?}");
    let before = st.restarts;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while sup.status().restarts <= before && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(sup.status().restarts > before, "the supervisor stopped retrying");
    assert!(sup.crash_loop().is_some(), "the loop stays named while it goes on");

    // A member Stop is not a crash loop.
    let _ = sup.request_stop();
    let _ = sup.wait_until(|s| matches!(s, SupervisorState::Off), Duration::from_secs(10));
    assert_eq!(sup.crash_loop(), None, "a stopped service shows no crash loop");
}

#[test]
fn a_spent_budget_keeps_the_crash_loop_named() {
    let tmp = TmpDir::new("crashloop-failed");
    let mut cfg = SupervisorConfig::new(emfile_spec(), tmp.path("crashes.jsonl"));
    cfg.clock = FakeClock::new();
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 3,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Failed), Duration::from_secs(10));
    assert_eq!(st.state, SupervisorState::Failed);
    let l = sup.crash_loop().expect("Failed after the loop still names it");
    assert_eq!(l.kind, crate::crash_loop::CrashKind::TooManyOpenFiles);
    assert_eq!(l.count, 4, "initial run + 3 retries, all the same error");
}

#[test]
fn different_errors_each_time_are_not_a_crash_loop() {
    let tmp = TmpDir::new("crashloop-varied");
    let marker = tmp.path("n");
    // Each run prints a different error word (the run count from a file), so no class repeats.
    let script = format!(
        "n=$(cat '{m}' 2>/dev/null || echo 0); n=$((n+1)); echo $n > '{m}'; \
         case $n in 1) w=alpha;; 2) w=bravo;; 3) w=charlie;; *) w=delta$n;; esac; \
         echo \"Error: $w\" 1>&2; exit 1",
        m = marker.display()
    );
    let spec = SidecarSpec::new("varied", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    cfg.clock = FakeClock::new();
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 2,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Failed), Duration::from_secs(10));
    assert_eq!(st.state, SupervisorState::Failed);
    assert_eq!(sup.crash_loop(), None, "three different errors are not one loop");
}

/// A wall clock the test can move forward (review P1-1: "up for 9 minutes").
struct OffsetClock {
    offset_ms: AtomicU64,
}

impl Clock for OffsetClock {
    fn now_unix_ms(&self) -> u64 {
        SystemClock.now_unix_ms() + self.offset_ms.load(Ordering::SeqCst)
    }
}

/// A script whose first `n` runs print the EMFILE line and exit 1; later runs stay up.
fn emfile_then_up_script(marker: &std::path::Path, n: u32) -> String {
    format!(
        "c=$(cat '{m}' 2>/dev/null || echo 0); c=$((c+1)); echo $c > '{m}'; \
         if [ $c -le {n} ]; then echo '{EMFILE_LINE}' 1>&2; exit 1; fi; exec sleep 60",
        m = marker.display()
    )
}

/// Review P1-1: a node that recovered is not shown as crash-looping. The loop ends on the
/// supervisor's health credit, not on uptime: after the credit, even 9 minutes later on the
/// supervisor's clock (under the old 10-minute uptime rule it still read as a loop), there is none.
#[test]
fn a_healthy_child_after_a_loop_is_not_a_loop() {
    let tmp = TmpDir::new("crashloop-recovered");
    let spec = SidecarSpec::new(
        "citrate-node",
        sh_bin(),
        vec!["-c".into(), emfile_then_up_script(&tmp.path("n"), 3)],
    );
    let mut cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    let clock = Arc::new(OffsetClock { offset_ms: AtomicU64::new(0) });
    cfg.clock = clock.clone();
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 50,
    };
    cfg.healthy_after = Duration::from_millis(1500);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let l = wait_for_loop(&sup, Duration::from_secs(15)).expect("three EMFILE exits are a loop");
    assert_eq!(l.count, 3);
    // The fourth run stays up. Until it earns credit the loop may still show; after, never.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while sup.lifecycle().map_or(0, |l| l.counters.health_credits) == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(sup.lifecycle().is_some_and(|l| l.counters.health_credits >= 1), "the child earned credit");
    assert_eq!(sup.status().state, SupervisorState::Running);
    assert_eq!(sup.crash_loop(), None, "a credited child is not a loop");
    clock.offset_ms.store(9 * 60_000, Ordering::SeqCst);
    assert_eq!(sup.status().state, SupervisorState::Running);
    assert_eq!(sup.crash_loop(), None, "up 9 minutes after the loop: still not a loop");
    let _ = sup.request_stop();
}

/// Review P1-2: once the budget is spent the slow retries keep exiting the same way, and the
/// loop stays named across them (the gap is derived from the slow retry).
#[test]
fn slow_retry_crashes_keep_the_loop_named() {
    let tmp = TmpDir::new("crashloop-slowretry");
    let mut cfg = SupervisorConfig::new(emfile_spec(), tmp.path("crashes.jsonl"));
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 2,
    };
    cfg.slow_retry = Some(Duration::from_millis(200));
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Failed), Duration::from_secs(10));
    assert_eq!(st.state, SupervisorState::Failed);
    let before = sup.crash_loop().expect("the backoff crashes are a loop").count;
    assert_eq!(before, 3);
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while sup.crash_loop().map_or(0, |l| l.count) < before + 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let l = sup.crash_loop().expect("still one loop after the slow retries");
    assert!(l.count >= before + 2, "two slow-retry crashes joined the loop: {l:?}");
    assert_eq!(l.kind, crate::crash_loop::CrashKind::TooManyOpenFiles);
    let _ = sup.request_stop();
}

/// Review P3: a crash is classified from its own run's stderr only. Two runs print the EMFILE
/// line; then three runs are killed by a signal without printing anything. Those are named by
/// their exit ("signal: 9"), never by the earlier EMFILE line still in the ring.
#[test]
fn a_crash_is_classified_from_its_own_run_only() {
    let tmp = TmpDir::new("crashloop-ownrun");
    let script = format!(
        "c=$(cat '{m}' 2>/dev/null || echo 0); c=$((c+1)); echo $c > '{m}'; \
         if [ $c -le 2 ]; then echo '{EMFILE_LINE}' 1>&2; exit 1; fi; kill -9 $$",
        m = tmp.path("n").display()
    );
    let spec = SidecarSpec::new("citrate-node", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    cfg.clock = FakeClock::new();
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 4,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Failed), Duration::from_secs(10));
    assert_eq!(st.state, SupervisorState::Failed);
    let l = sup.crash_loop().expect("three signal kills are a loop");
    assert_eq!(l.kind, crate::crash_loop::CrashKind::Other);
    assert_eq!(l.line, "signal: 9");
    assert_eq!(l.count, 3);
}
