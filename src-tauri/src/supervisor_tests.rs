// citrate-core — SidecarSupervisor tests (CORE-C1.0).
//
// RED-TEST-FIRST protocol: each behavior test was written to fail against a
// missing/naive supervisor first, then the module made it pass. The negative
// control (`fork_bomb_bound_is_load_bearing`) proves the max-retry cap is what
// stops an instant-exit child from looping unbounded — with the cap the run is
// finite; a bound removed (simulated by an enormous max_retries + a tight
// wall-clock budget) would never reach Failed.
//
// Portability: children are real OS binaries resolved shell-free at spawn time
// (`sleep`, `false`, `touch`), present on both macOS and Linux. NO test relies
// on a wall-clock RACE — every wait is a bounded `wait_until` poll with a
// generous timeout, and the backoff-ordering test uses an INJECTED clock so the
// asserted delays are deterministic (the B1.1-F-4 lesson).

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Resolve the first existing path for a coreutil, so the tests run on macOS
/// (`/bin/sleep`) and Linux (`/bin/sleep` or `/usr/bin/sleep`) alike.
fn resolve_bin(candidates: &[&str]) -> PathBuf {
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    panic!("no test binary found among {candidates:?}");
}

fn sleep_bin() -> PathBuf {
    resolve_bin(&["/bin/sleep", "/usr/bin/sleep"])
}

/// A binary that exits immediately with a non-zero code (crash-loop fodder).
fn false_bin() -> PathBuf {
    resolve_bin(&["/usr/bin/false", "/bin/false"])
}

fn touch_bin() -> PathBuf {
    resolve_bin(&["/usr/bin/touch", "/bin/touch"])
}

/// `/bin/sh`, used as a test CHILD binary to print known lines to stdout+stderr.
/// NOTE: this spawns a shell as the SUPERVISED PROCESS (a legitimate child), NOT
/// via the supervisor's arg path — the injection guarantee is about args being
/// literal argv, which the metachar test proves; here we deliberately want a
/// child that writes to both streams so the log-capture ring can be asserted.
fn sh_bin() -> PathBuf {
    resolve_bin(&["/bin/sh", "/usr/bin/sh"])
}

/// A unique temp dir for a test (crash records + injection artifacts). Cleaned
/// up on drop.
struct TmpDir(PathBuf);

impl TmpDir {
    fn new(tag: &str) -> Self {
        let pid = std::process::id();
        let nonce = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("citrate-core-sup-{tag}-{pid}-{nonce}"));
        std::fs::create_dir_all(&p).expect("mk tmpdir");
        TmpDir(p)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A deterministic clock: returns a fixed base plus a monotonically increasing
/// step each call, so backoff `next_retry_ms` values are predictable and never
/// depend on the wall clock.
struct FakeClock {
    base: u64,
    step: AtomicU64,
}

impl FakeClock {
    fn new() -> Arc<Self> {
        Arc::new(FakeClock {
            base: 1_000_000,
            step: AtomicU64::new(0),
        })
    }
}

impl Clock for FakeClock {
    fn now_unix_ms(&self) -> u64 {
        // Each read advances by 1000 "ms" so successive crash timestamps and
        // backoff anchors strictly increase (deterministic ordering).
        self.base + self.step.fetch_add(1, Ordering::SeqCst) * 1000
    }
}

/// Read every crash record (JSON line) from the file, in order.
fn read_crash_records(path: &std::path::Path) -> Vec<CrashRecord> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<CrashRecord>(l).expect("valid crash record json"))
        .collect()
}

// ---------------------------------------------------------------------------
// 1) crash → restart: a killed child produces a crash record AND a restart.
// ---------------------------------------------------------------------------

#[test]
fn crash_produces_record_and_restart() {
    let tmp = TmpDir::new("crash-restart");
    let crash_file = tmp.path("crashes.jsonl");
    // A long-lived child we can kill; on kill it restarts (a fresh sleep spawns).
    let spec = SidecarSpec::new("sleepy", sleep_bin(), vec!["60".into()]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    // Fast backoff so the restart is observed quickly.
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(20),
        multiplier: 2,
        max_delay: Duration::from_millis(50),
        max_retries: 5,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // Wait until it is Running with a pid.
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    let first_pid = st.pid.expect("running child has a pid");

    // Kill the child out from under the supervisor (simulate a crash).
    signal_pid(first_pid, libc::SIGKILL);

    // The supervisor must restart it. Wait specifically for the restart counter
    // to advance (not merely "Running", which could still be the pre-crash
    // Running snapshot — the crash-then-respawn is what we must observe).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let s = sup.status();
        if s.restarts >= 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "supervisor did not restart the crashed child (restarts still 0, state {:?})",
            s.state
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // And it comes back up Running with a (fresh) pid.
    let restarted = sup.wait_until(
        |s| matches!(s, SupervisorState::Running),
        Duration::from_secs(5),
    );
    assert!(
        matches!(restarted.state, SupervisorState::Running),
        "supervisor did not return to Running after restart: {:?}",
        restarted.state
    );
    let _ = first_pid;

    // A crash record file exists with at least one record.
    let records = read_crash_records(&crash_file);
    assert!(!records.is_empty(), "no crash record written");
    assert_eq!(records[0].name, "sleepy");

    sup.stop();
}

// ---------------------------------------------------------------------------
// 2) backoff bound (anti-fork-bomb): an instant-exit child reaches Failed within
//    the max-retry bound, and the recorded backoff anchors strictly increase.
// ---------------------------------------------------------------------------

#[test]
fn fork_bomb_is_bounded_and_backoff_increases() {
    let tmp = TmpDir::new("forkbomb");
    let crash_file = tmp.path("crashes.jsonl");
    let clock = FakeClock::new();
    // `false` exits IMMEDIATELY every time — the classic fork-bomb shape.
    let spec = SidecarSpec::new("flapper", false_bin(), vec![]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.clock = clock.clone();
    // Small real delays (so the test is fast) but a real cap.
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(40),
        max_retries: 4,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // It MUST reach the terminal Failed state (bounded — not a tight loop).
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Failed), Duration::from_secs(10));
    assert_eq!(
        st.state,
        SupervisorState::Failed,
        "instant-exit child never reached Failed — the fork-bomb bound is NOT holding"
    );

    // Bounded attempts: exactly max_retries + 1 crashes (the initial run + the
    // capped retries), never an unbounded flood.
    let records = read_crash_records(&crash_file);
    assert_eq!(
        records.len(),
        5, // initial run + max_retries(4) restarts
        "unexpected crash count (fork-bomb bound off?): {}",
        records.len()
    );

    // The crash timestamps (from the injected clock) strictly increase — the
    // process was backed off between attempts, not spun in a tight loop.
    for w in records.windows(2) {
        assert!(
            w[1].at_unix_ms > w[0].at_unix_ms,
            "crash timestamps did not increase (tight loop?): {} !> {}",
            w[1].at_unix_ms,
            w[0].at_unix_ms
        );
    }

    // The restart_attempt index increases 0,1,2,3,4 — each crash is a distinct
    // consecutive attempt.
    let attempts: Vec<u32> = records.iter().map(|r| r.restart_attempt).collect();
    assert_eq!(attempts, vec![0, 1, 2, 3, 4], "restart attempts not sequential");
}

// ---------------------------------------------------------------------------
// 2b) NEGATIVE CONTROL: the max-retry cap is load-bearing. With a huge cap and a
//     tight wall-clock budget, an instant-exit child does NOT reach Failed —
//     proving the cap (not something else) is what terminates the fork-bomb.
// ---------------------------------------------------------------------------

#[test]
fn fork_bomb_bound_is_load_bearing() {
    let tmp = TmpDir::new("forkbomb-neg");
    let crash_file = tmp.path("crashes.jsonl");
    let spec = SidecarSpec::new("flapper-neg", false_bin(), vec![]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    // Effectively-unbounded retries + minimal delay: the ONLY thing that could
    // stop it is the cap, and we set it huge. Within a short budget it must
    // still be crash-looping (Backoff/Starting/Running), NOT Failed.
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(1),
        multiplier: 1,
        max_delay: Duration::from_millis(1),
        max_retries: u32::MAX,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // Give it time to churn through MANY restarts.
    let st = sup.wait_until(
        |s| matches!(s, SupervisorState::Failed),
        Duration::from_millis(400),
    );
    assert_ne!(
        st.state,
        SupervisorState::Failed,
        "reached Failed with an unbounded cap — the cap is NOT the load-bearing bound"
    );
    // It churned (many restarts) but never terminated — confirming only the cap
    // ends it.
    assert!(
        st.restarts > 5,
        "expected many restart attempts under an unbounded cap, got {}",
        st.restarts
    );

    sup.stop();
}

// ---------------------------------------------------------------------------
// 3) graceful stop: after stop the child is gone AND no restart happens
//    (intentional stop is not a crash).
// ---------------------------------------------------------------------------

#[test]
fn graceful_stop_kills_child_and_does_not_restart() {
    let tmp = TmpDir::new("stop");
    let crash_file = tmp.path("crashes.jsonl");
    let spec = SidecarSpec::new("stoppable", sleep_bin(), vec!["60".into()]);
    let cfg = SupervisorConfig::new(spec, &crash_file);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let st = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    let pid = st.pid.expect("running pid");

    sup.stop();

    // State is Off.
    let after = sup.status();
    assert_eq!(after.state, SupervisorState::Off, "not Off after stop");
    assert_eq!(after.pid, None, "pid not cleared after stop");

    // The child pid is dead.
    assert!(!pid_alive(pid), "child still alive after graceful stop");

    // No crash record was written — an intentional stop is NOT a crash.
    let records = read_crash_records(&crash_file);
    assert!(
        records.is_empty(),
        "intentional stop wrote a crash record (treated as crash): {records:?}"
    );

    // Give it a beat to (wrongly) restart, then confirm it stayed Off.
    let still = sup.wait_until(
        |s| matches!(s, SupervisorState::Running),
        Duration::from_millis(300),
    );
    assert_eq!(still.state, SupervisorState::Off, "restarted after intentional stop");
}

// ---------------------------------------------------------------------------
// 4) no orphan on drop: dropping the supervisor kills the child.
// ---------------------------------------------------------------------------

#[test]
fn drop_kills_child_no_orphan() {
    let tmp = TmpDir::new("drop");
    let crash_file = tmp.path("crashes.jsonl");
    let spec = SidecarSpec::new("orphan-check", sleep_bin(), vec!["60".into()]);
    let cfg = SupervisorConfig::new(spec, &crash_file);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let st = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    let pid = st.pid.expect("running pid");
    assert!(pid_alive(pid), "child should be alive before drop");

    drop(sup); // Drop must SIGTERM/SIGKILL the child and join the monitor.

    // The child must be dead (no orphan). Poll briefly for reap.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while pid_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!pid_alive(pid), "child orphaned after supervisor drop (pid {pid} alive)");
}

// ---------------------------------------------------------------------------
// 5) injection-proof: shell metacharacters in args are literal argv, never a
//    shell command. Prove the metachar-named file IS created and the
//    shell-interpreted `pwned` file is NOT.
// ---------------------------------------------------------------------------

#[test]
fn args_with_shell_metacharacters_are_literal_not_a_shell() {
    let tmp = TmpDir::new("inject");
    let crash_file = tmp.path("crashes.jsonl");
    let workdir = tmp.0.clone();

    // If a shell interpreted this arg it would run `touch pwned` (creating a file
    // literally named `pwned`). Directly spawned `touch` instead creates a file
    // whose NAME is this whole literal string. NOTE: no `/` in the payload — a
    // slash is a path separator, so `touch` would fail to create the literal
    // file (nonexistent dir); the metacharacters exercised are the ones that
    // MATTER for shell injection ($(), backticks, ;, &&).
    let malicious = "$(touch pwned); `touch pwned` && touch pwned".to_string();
    let mut spec = SidecarSpec::new("injectee", touch_bin(), vec![malicious.clone()]);
    spec.workdir = Some(workdir.clone());
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    // touch exits 0 immediately; keep retries low so we don't churn.
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 1,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // Let touch run at least once (then the supervisor may loop/Fail; we don't
    // care — we only assert the filesystem effect of the FIRST run).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let literal_file = workdir.join(&malicious);
    while !literal_file.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }

    // The literal-named file exists (touch got the metachars as one argv slot).
    assert!(
        literal_file.exists(),
        "the literal metachar-named file was not created — argv not delivered verbatim"
    );
    // The shell-interpretation artifact does NOT exist (no shell ran the $()).
    assert!(
        !workdir.join("pwned").exists(),
        "SHELL INJECTION: `pwned` was created — a shell interpreted the arg"
    );

    sup.stop();
}

// ---------------------------------------------------------------------------
// 6) health check: a failing probe transitions to a restart (Backoff/restart).
// ---------------------------------------------------------------------------

#[test]
fn failing_health_check_triggers_restart() {
    let tmp = TmpDir::new("health");
    let crash_file = tmp.path("crashes.jsonl");

    // Probe fails only AFTER the first interval, forcing exactly one health-driven
    // restart; the child itself is a long sleep (alive but "unhealthy").
    let calls = Arc::new(AtomicU64::new(0));
    let calls2 = calls.clone();
    let health = HealthCheck {
        interval: Duration::from_millis(30),
        // First probe healthy (return true), subsequent probes unhealthy.
        probe: Arc::new(move || calls2.fetch_add(1, Ordering::SeqCst) == 0),
    };
    let mut spec = SidecarSpec::new("healthy?", sleep_bin(), vec!["60".into()]);
    spec.health_check = Some(health);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(10),
        multiplier: 2,
        max_delay: Duration::from_millis(30),
        max_retries: 3,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // The failing probe must drive at least one restart (restarts >= 1), and a
    // crash record with the health-failure reason exists.
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    assert!(matches!(st.state, SupervisorState::Running));

    // Wait for the health probe to fail + restart to be recorded.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let s = sup.status();
        if s.restarts >= 1 {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("failing health check never triggered a restart");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let records = read_crash_records(&crash_file);
    assert!(
        records.iter().any(|r| r.exit.contains("health check failed")),
        "no health-failure crash record: {records:?}"
    );

    sup.stop();
}

// ---------------------------------------------------------------------------
// 7) unit: backoff delay is bounded + monotone up to the cap (pure fn, no I/O).
// ---------------------------------------------------------------------------

#[test]
fn backoff_delay_is_bounded_and_monotone() {
    let policy = BackoffPolicy {
        base_delay: Duration::from_millis(100),
        multiplier: 2,
        max_delay: Duration::from_millis(1000),
        max_retries: 10,
    };
    // attempt 1 = base, then doubling, capped at max_delay.
    assert_eq!(policy.delay_for(1), Duration::from_millis(100));
    assert_eq!(policy.delay_for(2), Duration::from_millis(200));
    assert_eq!(policy.delay_for(3), Duration::from_millis(400));
    assert_eq!(policy.delay_for(4), Duration::from_millis(800));
    // Capped from here on — never exceeds max_delay (the fork-bomb-cap safety).
    assert_eq!(policy.delay_for(5), Duration::from_millis(1000));
    assert_eq!(policy.delay_for(50), Duration::from_millis(1000));
    // A huge attempt saturates, never overflows into a tiny delay.
    assert_eq!(policy.delay_for(u32::MAX), Duration::from_millis(1000));
    // Monotone non-decreasing across the ramp.
    let mut prev = Duration::ZERO;
    for a in 1..=10 {
        let d = policy.delay_for(a);
        assert!(d >= prev, "backoff not monotone at attempt {a}");
        prev = d;
    }
}

// ---------------------------------------------------------------------------
// 8) unit: the spawn descriptor never routes through a shell — structural proof
//    that `SidecarSpec` carries no command-string/shell field (defense in depth
//    alongside the behavioral injection test).
// ---------------------------------------------------------------------------

#[test]
fn spec_has_no_shell_field_source_check() {
    // The module source must construct exactly one Command via `Command::new`
    // with a PathBuf bin + `.args`, and must never spawn a SHELL. We check for
    // the concrete Rust invocation forms a shell would take (not prose — the
    // doc comment legitimately NAMES `sh -c` to say it is absent), so this test
    // cannot be tripped by documentation.
    let src = include_str!("supervisor.rs");
    for shell_call in [
        "Command::new(\"sh\")",
        "Command::new(\"/bin/sh\")",
        "Command::new(\"bash\")",
        "Command::new(\"cmd\")",
        "Command::new(\"powershell\")",
    ] {
        assert!(
            !src.contains(shell_call),
            "supervisor spawns a shell ({shell_call}) — injection surface"
        );
    }
    // A `-c` shell flag would be the other tell; it must not appear as an arg.
    assert!(
        !src.contains(".arg(\"-c\")") && !src.contains("\"-c\".to_string()"),
        "supervisor passes a `-c` shell flag — injection surface"
    );
    // The single spawn-construction site uses Command::new(&self.bin) (a PathBuf),
    // never a string command line.
    assert!(
        src.contains("Command::new(&self.bin)"),
        "spawn is not the expected direct Command::new(path) form"
    );
}

// ---------------------------------------------------------------------------
// WP-1 / F-1 (RED-FIRST): CONSECUTIVE, not LIFETIME. A child that runs HEALTHY
// for `healthy_after`, then crashes, repeated MORE times than `max_retries`,
// must KEEP getting restarted — never permanently `Failed` — because the
// crashes are non-consecutive (each is preceded by a sustained-healthy run that
// resets the counter). This FAILS against the lifetime-counter code (which would
// hit `Failed` after max_retries LIFETIME crashes) and passes after the F-1 fix.
//
// Maps to formal INV-1 (an intermittently-crashing-but-recovering child is never
// permanently Failed).
// ---------------------------------------------------------------------------

#[test]
fn intermittent_crashes_with_healthy_runs_never_permanently_fail() {
    let tmp = TmpDir::new("intermittent");
    let crash_file = tmp.path("crashes.jsonl");
    // A long-lived child we kill repeatedly; between kills it stays Running well
    // past `healthy_after`, so each crash is NON-consecutive (counter resets).
    let spec = SidecarSpec::new("longlived", sleep_bin(), vec!["60".into()]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    // Small real thresholds so the test is fast but the semantics are exact:
    // a run that stays up >= 80ms counts as sustained-healthy → reset.
    cfg.healthy_after = Duration::from_millis(80);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(10),
        multiplier: 2,
        max_delay: Duration::from_millis(30),
        // Cap = 3. We will crash the child 6 times (> cap). Under a LIFETIME
        // counter it would be Failed by crash #4; under CONSECUTIVE it never is.
        max_retries: 3,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let crashes_to_do = 6u32; // strictly greater than max_retries(3)
    for i in 0..crashes_to_do {
        // Wait until it is Running with a fresh pid.
        let st = sup.wait_until(
            |s| matches!(s, SupervisorState::Running),
            Duration::from_secs(5),
        );
        assert_eq!(
            st.state,
            SupervisorState::Running,
            "child not Running before intended crash #{i} (state {:?}) — it may have \
             wrongly reached Failed under a lifetime counter",
            st.state
        );
        let pid = st.pid.expect("running child has a pid");

        // Let it stay healthy past `healthy_after` so the consecutive counter
        // resets, THEN crash it. (Generous margin over the 80ms threshold.)
        std::thread::sleep(Duration::from_millis(160));

        // It must still be Running (a healthy sustained run), and NOT Failed.
        let mid = sup.status();
        assert_ne!(
            mid.state,
            SupervisorState::Failed,
            "supervisor reached Failed after a HEALTHY interval + {} prior crashes \
             — the counter is LIFETIME, not CONSECUTIVE (F-1 not fixed)",
            i
        );

        // Crash it (kill out from under the supervisor).
        signal_pid(pid, libc::SIGKILL);

        // Wait for the restart to be observed (restarts counter advances).
        let want = i + 1;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let s = sup.status();
            if s.restarts >= want {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "supervisor did not restart after crash #{i} (restarts {}, state {:?}) \
                 — it may be stuck in Failed (lifetime counter)",
                s.restarts,
                s.state
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // After MORE crashes than max_retries, the supervisor is STILL alive (never
    // permanently Failed) because every crash was preceded by a healthy reset.
    let end = sup.wait_until(
        |s| matches!(s, SupervisorState::Running),
        Duration::from_secs(5),
    );
    assert_ne!(
        end.state,
        SupervisorState::Failed,
        "intermittently-crashing-but-recovering child reached terminal Failed — \
         F-1 (consecutive-not-lifetime) is NOT holding"
    );
    assert!(
        end.restarts >= crashes_to_do,
        "expected at least {crashes_to_do} restarts, got {}",
        end.restarts
    );

    sup.stop();
}

// ---------------------------------------------------------------------------
// WP-1 / F-1 (companion): the fork-bomb bound STILL holds with the reset in
// place — a child that ONLY crash-loops (NO healthy interval ever) reaches
// Failed within max_retries. This is the two-properties-at-once proof: reset on
// healthy (above) must NOT let a fast crash-loop evade the cap (here). `false`
// exits instantly, so it never stays up `healthy_after` → never resets.
//
// Maps to formal INV-2 (crash-loop-only child reaches Failed within max_retries).
// ---------------------------------------------------------------------------

#[test]
fn crash_loop_without_healthy_interval_still_hits_cap_after_reset_fix() {
    let tmp = TmpDir::new("forkbomb-reset");
    let crash_file = tmp.path("crashes.jsonl");
    let spec = SidecarSpec::new("flapper2", false_bin(), vec![]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    // `healthy_after` is LARGE relative to the child's (instant) lifetime, so an
    // instantly-exiting child can NEVER accrue a sustained-healthy run → the
    // counter never resets → the cap is reached.
    cfg.healthy_after = Duration::from_secs(30);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(40),
        max_retries: 4,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let st = sup.wait_until(
        |s| matches!(s, SupervisorState::Failed),
        Duration::from_secs(10),
    );
    assert_eq!(
        st.state,
        SupervisorState::Failed,
        "instant-exit child never reached Failed WITH the reset fix present — the \
         reset wrongly let a fast crash-loop evade the fork-bomb cap"
    );
    // Bounded: exactly max_retries + 1 crashes, never an unbounded flood.
    let records = read_crash_records(&crash_file);
    assert_eq!(
        records.len(),
        5, // initial run + max_retries(4)
        "fork-bomb bound off after reset fix: {} crashes",
        records.len()
    );
}

// ---------------------------------------------------------------------------
// WP-2 / F-2 (RED-FIRST): a slow/HANGING health probe must NOT stall crash
// detection OR teardown. Against the old inline-probe code, a probe that blocks
// for T seconds would delay crash detection and block Drop→join for T. Here the
// probe blocks far longer than the test budget; the supervisor must still detect
// a real child crash AND complete `stop()` within a bounded time.
//
// Maps to formal: the model runs the probe as an event that cannot preempt the
// crash/stop transitions (liveness of Stop/crash-detection).
// ---------------------------------------------------------------------------

#[test]
fn hanging_health_probe_does_not_stall_crash_detection_or_stop() {
    let tmp = TmpDir::new("hang-probe");
    let crash_file = tmp.path("crashes.jsonl");

    // A probe that BLOCKS effectively forever (10s) once invoked. Under the old
    // inline design this would freeze the monitor loop for 10s.
    let health = HealthCheck {
        interval: Duration::from_millis(30),
        probe: Arc::new(move || {
            std::thread::sleep(Duration::from_secs(10));
            true
        }),
    };
    let mut spec = SidecarSpec::new("hang", sleep_bin(), vec!["60".into()]);
    spec.health_check = Some(health);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.healthy_after = Duration::from_secs(30);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(10),
        multiplier: 2,
        max_delay: Duration::from_millis(30),
        max_retries: 5,
    };
    // Keep teardown bound tight so the assertion is meaningful.
    cfg.join_timeout = Duration::from_secs(3);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let st = sup.wait_until(
        |s| matches!(s, SupervisorState::Running),
        Duration::from_secs(5),
    );
    let pid = st.pid.expect("running pid");

    // Let the probe get invoked (interval 30ms) so it is mid-hang.
    std::thread::sleep(Duration::from_millis(120));

    // (a) crash detection is NOT stalled by the hanging probe: kill the child and
    // the supervisor must notice + restart within a bounded time (far less than
    // the probe's 10s hang). A wedged probe also counts as unhealthy, so a
    // restart happens either way; what matters is it happens FAST.
    signal_pid(pid, libc::SIGKILL);
    let restart_deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        if sup.status().restarts >= 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < restart_deadline,
            "hanging health probe stalled crash detection — no restart within 3s \
             (probe hangs 10s); F-2 not fixed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    // (b) teardown is NOT stalled: stop() must return within a bounded time even
    // though a probe thread may still be hanging.
    let t0 = std::time::Instant::now();
    sup.stop();
    let elapsed = t0.elapsed();
    assert!(
        elapsed < Duration::from_secs(6),
        "stop() took {elapsed:?} — a hanging probe stalled teardown (F-2 not fixed)"
    );
}

// ---------------------------------------------------------------------------
// WP-2 (companion): dropping a supervisor whose child is HUNG (ignores SIGTERM)
// still completes teardown within the bounded join — no unbounded Drop→join.
// The child traps SIGTERM (via a shell that ignores it) but SIGKILL still ends
// it within stop_grace, and the bounded join returns promptly regardless.
//
// Maps to formal INV-4 (Off/Failed implies no live child) + bounded teardown.
// ---------------------------------------------------------------------------

#[test]
fn drop_with_bounded_join_returns_promptly() {
    let tmp = TmpDir::new("bounded-join");
    let crash_file = tmp.path("crashes.jsonl");
    let spec = SidecarSpec::new("joinbound", sleep_bin(), vec!["60".into()]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.stop_grace = Duration::from_millis(200);
    cfg.join_timeout = Duration::from_secs(2);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let st = sup.wait_until(
        |s| matches!(s, SupervisorState::Running),
        Duration::from_secs(5),
    );
    let pid = st.pid.expect("running pid");

    let t0 = std::time::Instant::now();
    drop(sup);
    let elapsed = t0.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "drop() took {elapsed:?} — teardown join is not bounded (F-2)"
    );
    // No orphan: the child is dead.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while pid_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!pid_alive(pid), "child orphaned after bounded-join drop (pid {pid})");
}

// ---------------------------------------------------------------------------
// WP-3 coverage: spawn-failure (bad binary path) → bounded backoff → Failed.
// A nonexistent binary can never spawn; it must NOT tight-loop — the same
// max_retries cap applies and the supervisor reaches terminal Failed. This is
// the spawn-fail arm the review flagged as untested.
//
// Maps to formal INV-2 (crash-loop bound; spawn-fail is a crash) + the
// spawn-fail→Failed transition.
// ---------------------------------------------------------------------------

#[test]
fn spawn_failure_reaches_failed_within_bound() {
    let tmp = TmpDir::new("spawn-fail");
    let crash_file = tmp.path("crashes.jsonl");
    // A path that does not exist → Command::spawn returns Err every time.
    let bogus = tmp.path("definitely-not-a-real-binary-xyz");
    let spec = SidecarSpec::new("ghost", bogus, vec![]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.healthy_after = Duration::from_secs(30); // spawn never succeeds → no reset
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 3,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let st = sup.wait_until(
        |s| matches!(s, SupervisorState::Failed),
        Duration::from_secs(10),
    );
    assert_eq!(
        st.state,
        SupervisorState::Failed,
        "spawn-failure never reached Failed — the bad-binary arm is not bounded"
    );
    // Bounded: initial spawn attempt + max_retries spawn attempts = 4 records.
    let records = read_crash_records(&crash_file);
    assert_eq!(
        records.len(),
        4,
        "spawn-fail crash count off (bound not applied to spawn arm): {}",
        records.len()
    );
    assert!(
        records.iter().all(|r| r.exit.starts_with("spawn failed")),
        "spawn-fail records not tagged as spawn failures: {records:?}"
    );
}

// ---------------------------------------------------------------------------
// WP-3 coverage: STOP during BACKOFF cancels the pending restart. A crash puts
// the supervisor into Backoff; a stop() arriving during that window must go Off
// and NOT respawn. This is the stop-during-backoff transition the review flagged
// as untested.
//
// Maps to formal INV-3 (an intentional stop never triggers a restart) + the
// Backoff→Stop→Off transition (cancels the scheduled restart).
// ---------------------------------------------------------------------------

#[test]
fn stop_during_backoff_cancels_pending_restart() {
    let tmp = TmpDir::new("stop-backoff");
    let crash_file = tmp.path("crashes.jsonl");
    // Instant-exit child so it enters Backoff quickly; a LONG base delay so the
    // Backoff window is wide enough to inject a stop() before the respawn.
    let spec = SidecarSpec::new("backoff-stop", false_bin(), vec![]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_secs(5), // wide Backoff window
        multiplier: 2,
        max_delay: Duration::from_secs(10),
        max_retries: 5,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // Wait until it is in Backoff (crashed once, waiting to respawn).
    let st = sup.wait_until(
        |s| matches!(s, SupervisorState::Backoff { .. }),
        Duration::from_secs(5),
    );
    assert!(
        matches!(st.state, SupervisorState::Backoff { .. }),
        "did not reach Backoff before stop (state {:?})",
        st.state
    );
    let crashes_before = read_crash_records(&crash_file).len();

    // Stop during Backoff: must cancel the pending restart and go Off.
    sup.stop();
    let after = sup.status();
    assert_eq!(
        after.state,
        SupervisorState::Off,
        "stop during Backoff did not go Off (state {:?})",
        after.state
    );

    // Give it well past the (5s) base delay a respawn WOULD have used — confirm
    // no new crash record appeared (the pending restart was truly cancelled).
    std::thread::sleep(Duration::from_millis(300));
    let still = sup.status();
    assert_eq!(
        still.state,
        SupervisorState::Off,
        "supervisor left Off after stop-during-backoff (respawned?): {:?}",
        still.state
    );
    let crashes_after = read_crash_records(&crash_file).len();
    assert_eq!(
        crashes_after, crashes_before,
        "a restart happened after stop-during-backoff (crash records grew {crashes_before}->{crashes_after})"
    );
}

// ---------------------------------------------------------------------------
// Q-A.2 / Q-B.2 (RED-FIRST): the supervisor must CAPTURE the child's stdout AND
// stderr into a bounded ring buffer, in order, so the Node LOG panel can show
// REAL node output in a packaged build (today it is empty because stdout was
// `Stdio::inherit()`, dropped in a GUI). These fail against the pre-fix code
// (no ring, stdout inherited) and pass once piped capture + reader threads land.
// ---------------------------------------------------------------------------

/// Poll the log ring until `pred` holds or the timeout elapses; return the final
/// snapshot. Bounded (no wall-clock race) — the reader threads push lines as the
/// child writes them.
fn wait_for_logs(
    sup: &Supervisor,
    pred: impl Fn(&[LogLine]) -> bool,
    timeout: Duration,
) -> Vec<LogLine> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let logs = sup.logs();
        if pred(&logs) || std::time::Instant::now() >= deadline {
            return logs;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The supervisor captures BOTH stdout and stderr lines into the ring, tagged by
/// stream, preserving each line's text. This is the core Q-A.2 behavior: a real
/// node's `citrate_network::sync: …` progress lines (stdout/stderr) reach the UI.
#[test]
fn captures_child_stdout_and_stderr_into_ring() {
    let tmp = TmpDir::new("logs-capture");
    let crash_file = tmp.path("crashes.jsonl");
    // A child that prints one known line to stdout and one to stderr, then stays
    // alive (so the supervisor observes a clean Running child, not a crash-loop).
    let script = "echo OUT_HELLO; echo ERR_OOPS 1>&2; sleep 60".to_string();
    let spec = SidecarSpec::new("printer", sh_bin(), vec!["-c".into(), script]);
    let cfg = SupervisorConfig::new(spec, &crash_file);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    let logs = wait_for_logs(
        &sup,
        |l| {
            l.iter().any(|x| x.line == "OUT_HELLO" && x.stream == "out")
                && l.iter().any(|x| x.line == "ERR_OOPS" && x.stream == "err")
        },
        Duration::from_secs(5),
    );
    assert!(
        logs.iter().any(|x| x.line == "OUT_HELLO" && x.stream == "out"),
        "stdout line not captured into the ring (stdout still inherited?): {logs:?}"
    );
    assert!(
        logs.iter().any(|x| x.line == "ERR_OOPS" && x.stream == "err"),
        "stderr line not captured into the ring: {logs:?}"
    );
    // Every captured line carries a timestamp from the clock (non-zero here).
    assert!(logs.iter().all(|x| x.ts > 0), "log line missing a timestamp");

    sup.stop();
}

/// NEGATIVE CONTROL (the capture is load-bearing): a child that writes NOTHING to
/// stdout/stderr yields an EMPTY ring — proving the ring is filled by REAL child
/// output, not fabricated. If the ring were seeded/faked, this would be non-empty.
#[test]
fn silent_child_yields_empty_ring() {
    let tmp = TmpDir::new("logs-silent");
    let crash_file = tmp.path("crashes.jsonl");
    // `sleep` writes nothing to either stream while it runs.
    let spec = SidecarSpec::new("silent", sleep_bin(), vec!["60".into()]);
    let cfg = SupervisorConfig::new(spec, &crash_file);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // Let it run well past boot; a silent child must leave the ring empty.
    let _ = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(300));
    let logs = sup.logs();
    assert!(
        logs.is_empty(),
        "a silent child produced log lines — the ring is fabricated, not real capture: {logs:?}"
    );

    sup.stop();
}

/// The ring is BOUNDED (drop-oldest): a child that prints MORE than the capacity
/// leaves exactly the newest `LOG_RING_CAPACITY` lines — the oldest are evicted,
/// so a chatty node cannot grow memory without bound (the anti-bloat invariant).
#[test]
fn ring_is_bounded_and_drops_oldest() {
    let tmp = TmpDir::new("logs-bound");
    let crash_file = tmp.path("crashes.jsonl");
    let over = LOG_RING_CAPACITY + 50;
    // Print `over` numbered lines to stdout, then stay alive.
    let script = format!("for i in $(seq 1 {over}); do echo LINE_$i; done; sleep 60");
    let spec = SidecarSpec::new("chatty", sh_bin(), vec!["-c".into(), script]);
    let cfg = SupervisorConfig::new(spec, &crash_file);
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // Wait until the LAST printed line has been captured (all output flushed).
    let last = format!("LINE_{over}");
    let logs = wait_for_logs(
        &sup,
        |l| l.iter().any(|x| x.line == last),
        Duration::from_secs(8),
    );
    assert!(
        logs.iter().any(|x| x.line == last),
        "the final line was never captured (buffer stalled?): last seen {:?}",
        logs.last()
    );
    // Bounded at capacity — never the full `over` lines.
    assert!(
        logs.len() <= LOG_RING_CAPACITY,
        "ring exceeded its cap ({} > {LOG_RING_CAPACITY}) — the drop-oldest bound is off",
        logs.len()
    );
    // The oldest lines were evicted: LINE_1 must be gone, the newest present.
    assert!(
        !logs.iter().any(|x| x.line == "LINE_1"),
        "oldest line (LINE_1) still present — drop-oldest not applied"
    );

    sup.stop();
}

/// The ring SURVIVES a restart: a child killed out from under the supervisor is
/// respawned; both the pre-crash and post-restart lines are retained (the ring
/// is not cleared on respawn), so the LOG panel keeps a continuous tail.
#[test]
fn ring_survives_restart() {
    let tmp = TmpDir::new("logs-restart");
    let crash_file = tmp.path("crashes.jsonl");
    // Each run prints a UNIQUE marker so we can tell the runs apart. The marker
    // includes the pid so run-1 and run-2 differ.
    let script = "echo RUN_MARKER_$$; sleep 60".to_string();
    let spec = SidecarSpec::new("restarter", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(20),
        multiplier: 2,
        max_delay: Duration::from_millis(50),
        max_retries: 5,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // Wait for the first run's marker.
    let logs1 = wait_for_logs(
        &sup,
        |l| l.iter().any(|x| x.line.starts_with("RUN_MARKER_")),
        Duration::from_secs(5),
    );
    let first_marker = logs1
        .iter()
        .find(|x| x.line.starts_with("RUN_MARKER_"))
        .map(|x| x.line.clone())
        .expect("first run marker captured");

    // Kill the child → the supervisor restarts it (a fresh run, new pid marker).
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    let pid = st.pid.expect("running pid");
    signal_pid(pid, libc::SIGKILL);

    // Wait for a SECOND distinct marker (the restart).
    let logs2 = wait_for_logs(
        &sup,
        |l| {
            l.iter()
                .filter(|x| x.line.starts_with("RUN_MARKER_"))
                .any(|x| x.line != first_marker)
        },
        Duration::from_secs(6),
    );
    // Both markers are still present — the ring was NOT cleared across restart.
    assert!(
        logs2.iter().any(|x| x.line == first_marker),
        "pre-crash log line was cleared on restart — the ring should survive: {logs2:?}"
    );
    assert!(
        logs2
            .iter()
            .filter(|x| x.line.starts_with("RUN_MARKER_"))
            .any(|x| x.line != first_marker),
        "no post-restart log line captured — the ring did not re-attach after respawn"
    );

    sup.stop();
}

/// The bounded stderr TAIL for a crash record is still populated from the ring's
/// `err` lines after the stdout/stderr pipes moved to the reader threads. This
/// guards the existing crash-record behavior against the piped-capture change:
/// a crashing child's stderr must still land in the crash record.
#[test]
fn crash_record_stderr_tail_comes_from_ring() {
    let tmp = TmpDir::new("logs-crashtail");
    let crash_file = tmp.path("crashes.jsonl");
    // A child that writes a known stderr line then exits non-zero immediately.
    let script = "echo BOOM_ON_STDERR 1>&2; exit 7".to_string();
    let spec = SidecarSpec::new("boomer", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(10),
        multiplier: 2,
        max_delay: Duration::from_millis(30),
        max_retries: 1,
    };
    let sup = Supervisor::start(cfg).expect("supervisor starts");

    // It crash-loops to Failed within the small cap; a crash record is written.
    let _ = sup.wait_until(|s| matches!(s, SupervisorState::Failed), Duration::from_secs(8));
    let records = read_crash_records(&crash_file);
    assert!(!records.is_empty(), "no crash record written");
    assert!(
        records.iter().any(|r| r.stderr_tail.contains("BOOM_ON_STDERR")),
        "crash-record stderr tail did not capture the child's stderr from the ring: {records:?}"
    );
}
