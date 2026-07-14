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
    let sup = Supervisor::start(cfg);

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
    let sup = Supervisor::start(cfg);

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
    let sup = Supervisor::start(cfg);

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
    let sup = Supervisor::start(cfg);

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
    let sup = Supervisor::start(cfg);

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
    let sup = Supervisor::start(cfg);

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
        probe: Box::new(move || calls2.fetch_add(1, Ordering::SeqCst) == 0),
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
    let sup = Supervisor::start(cfg);

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
