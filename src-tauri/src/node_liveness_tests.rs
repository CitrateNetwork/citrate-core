// CORE-D2 — the node liveness probe (planset r1007-v060 §CORE-D). Pure policy tests on a set
// clock: a wedged-but-running node is restarted only after a sustained stall while it has peers
// and the network is ahead of it, never during initial sync, with a growing gap between restarts.

use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

const MIN: u64 = 60_000;

fn policy() -> LivenessPolicy {
    LivenessPolicy {
        enabled: true,
        check_every: Duration::from_secs(60),
        stall_after: Duration::from_secs(10 * 60),
        first_gap: Duration::from_secs(20 * 60),
        max_gap: Duration::from_secs(4 * 60 * 60),
        tip_margin: 32,
    }
}

fn sample(run: u64, head: u64, peers: u64, tip: u64) -> Option<Sample> {
    Some(Sample {
        run: (1, run),
        head: Some(head),
        peers,
        tip: Some(tip),
    })
}

/// Run a caught-up incarnation `run` up to `at` so it has been seen at the tip once.
fn caught_up(w: &mut LivenessWatch, run: u64, at: u64, head: u64) {
    assert!(!matches!(
        w.observe(at, sample(run, head, 5, head)),
        Verdict::Restart { .. }
    ));
}

#[test]
fn an_advancing_head_is_never_restarted() {
    let mut w = LivenessWatch::new(policy());
    for i in 0..200u64 {
        let v = w.observe(i * MIN, sample(1, 1_000 + i * 30, 5, 1_000 + i * 30 + 5));
        assert_eq!(v, Verdict::Healthy, "minute {i}");
    }
}

#[test]
fn a_node_in_initial_sync_is_never_restarted() {
    // Never within the margin of the tip: initial sync, however long the head stands still.
    let mut w = LivenessWatch::new(policy());
    for i in 0..300u64 {
        let v = w.observe(i * MIN, sample(1, 5_000, 8, 90_000));
        assert_eq!(v, Verdict::Hold(Hold::InitialSync), "minute {i}");
    }
}

#[test]
fn a_sustained_stall_with_peers_while_the_network_moves_restarts_with_a_reason() {
    let mut w = LivenessWatch::new(policy());
    caught_up(&mut w, 1, 0, 10_000);
    // Head stuck at 10_000; the network moves on.
    let mut restart = None;
    for i in 1..=12u64 {
        let v = w.observe(i * MIN, sample(1, 10_000, 4, 10_100 + i * 30));
        if i < 11 {
            assert!(
                matches!(v, Verdict::Stalling { .. }),
                "minute {i}: a stall under stall_after is only watched: {v:?}"
            );
        } else if let Verdict::Restart { reason } = v {
            restart = Some((i, reason));
            break;
        }
    }
    let (at, reason) = restart.expect("a 10 min stall with peers while the network moves restarts");
    assert_eq!(at, 11, "the stall began at the first unchanged sample (minute 1)");
    assert!(reason.contains("10000"), "reason names the stuck head: {reason}");
    assert!(reason.contains("4 peers"), "reason names the peers: {reason}");
    assert!(reason.contains("10430"), "reason names the network tip: {reason}");
}

#[test]
fn no_peers_is_not_a_wedge() {
    let mut w = LivenessWatch::new(policy());
    caught_up(&mut w, 1, 0, 10_000);
    for i in 1..=60u64 {
        let v = w.observe(i * MIN, sample(1, 10_000, 0, 10_100 + i * 30));
        assert_eq!(v, Verdict::Hold(Hold::NoPeers), "minute {i}");
    }
}

#[test]
fn a_halted_network_is_not_a_wedge_and_restarts_the_stall_clock() {
    let mut w = LivenessWatch::new(policy());
    caught_up(&mut w, 1, 0, 10_000);
    // The whole network halts at our head for an hour.
    for i in 1..=60u64 {
        let v = w.observe(i * MIN, sample(1, 10_000, 4, 10_010));
        assert_eq!(v, Verdict::Hold(Hold::NetworkNotAhead), "minute {i}");
    }
    // It resumes; our head does not. The stall is measured from the resumption.
    for i in 61..=70u64 {
        let v = w.observe(i * MIN, sample(1, 10_000, 4, 10_000 + (i - 60) * 40));
        assert!(matches!(v, Verdict::Stalling { .. }), "minute {i}: {v:?}");
    }
    assert!(matches!(
        w.observe(71 * MIN, sample(1, 10_000, 4, 10_500)),
        Verdict::Restart { .. }
    ));
}

#[test]
fn an_unknown_tip_or_a_silent_rpc_never_restarts() {
    let mut w = LivenessWatch::new(policy());
    caught_up(&mut w, 1, 0, 10_000);
    for i in 1..=30u64 {
        let v = w.observe(
            i * MIN,
            Some(Sample {
                run: (1, 1),
                head: Some(10_000),
                peers: 4,
                tip: None,
            }),
        );
        assert_eq!(v, Verdict::Hold(Hold::TipUnknown), "minute {i}");
    }
    for i in 31..=60u64 {
        let v = w.observe(
            i * MIN,
            Some(Sample {
                run: (1, 1),
                head: None,
                peers: 0,
                tip: Some(20_000),
            }),
        );
        assert_eq!(v, Verdict::Hold(Hold::RpcUnavailable), "minute {i}");
    }
    assert_eq!(w.observe(61 * MIN, None), Verdict::Hold(Hold::NotRunning));
}

#[test]
fn restarts_back_off_and_a_new_incarnation_starts_in_initial_sync() {
    let mut w = LivenessWatch::new(policy());
    caught_up(&mut w, 1, 0, 10_000);
    let mut t = MIN;
    loop {
        if let Verdict::Restart { .. } = w.observe(t, sample(1, 10_000, 4, 10_000 + t / 1_000)) {
            break;
        }
        t += MIN;
        assert!(t < 60 * MIN);
    }
    w.restarted(t);
    assert_eq!(w.restarts(), 1);
    // The new incarnation is in initial sync until it reaches the tip.
    let v = w.observe(t + MIN, sample(2, 10_000, 4, 11_000));
    assert_eq!(v, Verdict::Hold(Hold::InitialSync));
    // It catches up, then wedges again at once.
    let t2 = t + 2 * MIN;
    caught_up(&mut w, 2, t2, 11_100);
    let mut held = false;
    let mut second = None;
    let mut now = t2 + MIN;
    while now < t2 + 60 * MIN {
        match w.observe(now, sample(2, 11_100, 4, 11_100 + now / 1_000)) {
            Verdict::Hold(Hold::Backoff { until_ms }) => {
                held = true;
                assert_eq!(until_ms, t + 20 * MIN, "first gap after the first restart");
            }
            Verdict::Restart { .. } => {
                second = Some(now);
                break;
            }
            _ => {}
        }
        now += MIN;
    }
    assert!(held, "a second stall inside the first gap is held");
    let second = second.expect("the second restart comes after the gap");
    assert!(second >= t + 20 * MIN);
    w.restarted(second);
    assert_eq!(w.restarts(), 2);
    assert_eq!(
        w.next_allowed_ms(),
        Some(second + 40 * MIN),
        "the gap doubles after each restart"
    );
}

#[test]
fn the_gap_is_capped_and_resets_after_a_long_healthy_run() {
    let mut p = policy();
    p.first_gap = Duration::from_secs(3 * 60 * 60);
    let mut w = LivenessWatch::new(p);
    w.restarted(0);
    w.restarted(1);
    w.restarted(2);
    assert_eq!(w.next_allowed_ms(), Some(2 + 4 * 60 * MIN), "capped at max_gap");
    // Caught up and advancing well past max_gap after the last restart: the streak ends.
    caught_up(&mut w, 9, 5 * 60 * MIN, 50_000);
    assert_eq!(w.restarts(), 0);
    assert_eq!(w.next_allowed_ms(), None);
}

#[test]
fn a_disabled_policy_only_holds() {
    let mut p = policy();
    p.enabled = false;
    let mut w = LivenessWatch::new(p);
    for i in 0..30u64 {
        assert_eq!(
            w.observe(i * MIN, sample(1, 10_000, 4, 20_000)),
            Verdict::Hold(Hold::Disabled)
        );
    }
}

#[test]
fn the_policy_reads_its_overrides_and_refuses_churning_values() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    };
    let d = policy_from_env(env(&[]));
    assert!(d.enabled);
    assert_eq!(d.stall_after, DEFAULT_STALL_AFTER);
    assert!(!policy_from_env(env(&[(LIVENESS_ENV, "off")])).enabled);
    assert!(!policy_from_env(env(&[(LIVENESS_ENV, "0")])).enabled);
    assert_eq!(
        policy_from_env(env(&[(STALL_SECS_ENV, "1200")])).stall_after,
        Duration::from_secs(1200)
    );
    // Below the floor, or not a number: the default stands.
    assert_eq!(
        policy_from_env(env(&[(STALL_SECS_ENV, "5")])).stall_after,
        DEFAULT_STALL_AFTER
    );
    assert_eq!(
        policy_from_env(env(&[(STALL_SECS_ENV, "soon")])).stall_after,
        DEFAULT_STALL_AFTER
    );
}

// ---- the driver, on a set clock and a recording probe ----

struct SetClock(AtomicU64);

impl Clock for SetClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A recording probe: serves the next sample and records every restart and note.
struct FakeProbe {
    sample: RefCell<Option<Sample>>,
    restart_ok: bool,
    restarts: RefCell<Vec<String>>,
    notes: RefCell<Vec<String>>,
}

impl LivenessProbe for FakeProbe {
    fn sample(&self) -> Option<Sample> {
        self.sample.borrow().clone()
    }
    fn restart(&self, reason: &str) -> std::result::Result<(), String> {
        self.restarts.borrow_mut().push(reason.to_string());
        if self.restart_ok {
            Ok(())
        } else {
            Err("the service is not meant to be running".to_string())
        }
    }
    fn note(&self, text: String) {
        self.notes.borrow_mut().push(text);
    }
}

fn probe(restart_ok: bool) -> FakeProbe {
    FakeProbe {
        sample: RefCell::new(None),
        restart_ok,
        restarts: RefCell::new(Vec::new()),
        notes: RefCell::new(Vec::new()),
    }
}

fn drive_to_stall(w: &mut LivenessWatch, clock: &SetClock, p: &FakeProbe) -> Verdict {
    *p.sample.borrow_mut() = sample(1, 10_000, 4, 10_000);
    tick(w, clock, p);
    let mut last = Verdict::Healthy;
    for i in 1..=11u64 {
        clock.0.store(i * MIN, Ordering::SeqCst);
        *p.sample.borrow_mut() = sample(1, 10_000, 4, 10_100 + i * 30);
        last = tick(w, clock, p);
    }
    last
}

#[test]
fn the_driver_restarts_once_logs_the_reason_and_counts_it() {
    let clock = SetClock(AtomicU64::new(0));
    let p = probe(true);
    let mut w = LivenessWatch::new(policy());
    let v = drive_to_stall(&mut w, &clock, &p);
    assert!(matches!(v, Verdict::Restart { .. }), "{v:?}");
    assert_eq!(p.restarts.borrow().len(), 1, "one restart request");
    assert_eq!(w.restarts(), 1);
    let notes = p.notes.borrow();
    let last = notes.last().expect("the restart is noted");
    assert!(last.contains("restart"), "{last}");
    assert!(last.contains("10000"), "{last}");
}

#[test]
fn a_refused_restart_is_noted_and_not_counted() {
    let clock = SetClock(AtomicU64::new(0));
    let p = probe(false);
    let mut w = LivenessWatch::new(policy());
    drive_to_stall(&mut w, &clock, &p);
    assert_eq!(p.restarts.borrow().len(), 1);
    assert_eq!(w.restarts(), 0, "a refused restart is not a restart");
    let notes = p.notes.borrow();
    assert!(
        notes.iter().any(|n| n.contains("refused")),
        "the refusal is noted: {notes:?}"
    );
}
