// citrate-core — the services registry read (K1a-2). `Supervisor::observe` is what core's
// `services_snapshot` command reports for each supervised service: the lifecycle cell, the last
// phase transition with the time it happened, and the last crash. Platform-neutral: every case
// uses a service whose binary does not exist, so it runs on every OS lane.

/// A clock the test sets by hand, so a transition's time is known exactly.
struct SetClock(std::sync::atomic::AtomicU64);

impl SetClock {
    fn at(ms: u64) -> Arc<Self> {
        Arc::new(SetClock(std::sync::atomic::AtomicU64::new(ms)))
    }
    fn set(&self, ms: u64) {
        self.0.store(ms, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Clock for SetClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn registry_config(tag: &str, clock: Arc<SetClock>) -> SupervisorConfig {
    let mut cfg = missing_bin_config(&format!("registry-{tag}"), 0);
    cfg.clock = clock;
    cfg
}

#[test]
fn observe_reports_the_last_transition_at_the_clock_time_it_happened() {
    let clock = SetClock::at(7_000_000);
    let sup = Supervisor::start(registry_config("failed", clock.clone())).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    // Reading later never moves the transition's time.
    clock.set(8_000_000);
    let obs = sup.observe().expect("observe");
    assert_eq!(obs.lifecycle.phase, Phase::Failed);
    assert_eq!(obs.last_transition.to, Phase::Failed);
    assert!(
        matches!(
            obs.last_transition.from,
            Some(Phase::Starting) | Some(Phase::Backoff)
        ),
        "Failed is reached from Starting or Backoff: {:?}",
        obs.last_transition
    );
    assert_eq!(
        obs.last_transition.at_unix_ms, 7_000_000,
        "the time the phase changed, from the supervisor's clock"
    );
}

#[test]
fn observe_follows_a_stop_through_stopping_to_stopped() {
    let clock = SetClock::at(1_000_000);
    let sup = Supervisor::start(registry_config("stop", clock.clone())).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    clock.set(9_000_000);
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop on a Failed service: {other:?}"),
    };
    assert!(matches!(wait_receipt(&sup, stop), Receipt::Complete { .. }));
    let obs = sup.observe().expect("observe");
    assert_eq!(
        obs.last_transition,
        PhaseTransition {
            from: Some(Phase::Stopping),
            to: Phase::Stopped,
            at_unix_ms: 9_000_000,
        }
    );
    // A repeated Stop changes nothing, so the transition stays where it was.
    clock.set(9_500_000);
    assert!(matches!(
        sup.request_stop(),
        ControlResult::AlreadySatisfied(_)
    ));
    assert_eq!(
        sup.observe().expect("observe").last_transition.at_unix_ms,
        9_000_000
    );
}

#[test]
fn observe_carries_the_restart_count_and_the_last_crash() {
    let sup = Supervisor::start(registry_config("crash", SetClock::at(3_000_000))).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    let obs = sup.observe().expect("observe");
    assert_eq!(obs.lifecycle.restarts, 1);
    let crash = obs.last_crash.expect("the failed spawn left a crash record");
    assert_eq!(crash.name, "lc-registry-crash");
    assert_eq!(crash.at_unix_ms, 3_000_000);
}

#[test]
fn a_contended_cell_is_not_observed() {
    let sup = Supervisor::start(registry_config("busy", SetClock::at(1))).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    let held = sup.shared.lock();
    assert_eq!(sup.observe(), None, "a contended read claims nothing");
    drop(held);
    assert!(sup.observe().is_some());
}

/// Tripwire (K1a-2): the last transition is only true if every phase change records it. Every
/// write of the phase goes through `Cell::set_phase`; a direct `.phase =` assignment anywhere else
/// would change the phase without recording when.
#[test]
fn every_phase_write_goes_through_set_phase_source_check() {
    let src = include_str!("supervisor.rs");
    let setter = src
        .find("fn set_phase(")
        .unwrap_or_else(|| panic!("tripwire: Cell::set_phase not found"));
    let setter_end = setter
        + src[setter..]
            .find("\n    }\n")
            .unwrap_or_else(|| panic!("tripwire: set_phase has no end"));
    let direct: Vec<usize> = src
        .match_indices(".phase = ")
        .map(|(i, _)| i)
        .filter(|i| !(setter..setter_end).contains(i))
        .collect();
    let lines: Vec<usize> = direct
        .iter()
        .map(|i| src[..*i].matches('\n').count() + 1)
        .collect();
    assert!(
        lines.is_empty(),
        "direct phase writes outside Cell::set_phase at supervisor.rs lines {lines:?}"
    );
}
