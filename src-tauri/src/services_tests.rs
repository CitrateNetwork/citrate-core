// K1a-2 — the services registry and its snapshot. Each row reports what the service's own kit
// supervisor says (real supervisors here, never a stand-in), and nothing when it can't read one.
use super::*;
use crate::supervisor::{
    BackoffPolicy, ControlResult, Receipt, SidecarSpec, SupervisorConfig, SupervisorState,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A clock the test sets, so transition and crash times are known exactly.
struct SetClock(AtomicU64);

impl Clock for SetClock {
    fn now_unix_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let n: u64 = rand::random();
    std::env::temp_dir().join(format!("citrate-core-services-{tag}-{n:016x}"))
}

fn config(tag: &str, bin: std::path::PathBuf, args: Vec<String>, clock: Arc<SetClock>) -> SupervisorConfig {
    let dir = scratch(tag);
    let mut cfg = SupervisorConfig::new(
        SidecarSpec::new(format!("svc-{tag}"), bin, args),
        dir.join("crash.jsonl"),
    );
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 1,
        max_delay: Duration::from_millis(5),
        max_retries: 0,
    };
    cfg.stop_grace = Duration::from_millis(500);
    cfg.clock = clock;
    cfg
}

/// A service whose binary does not exist: every spawn fails at once, on every OS.
fn failing_service(tag: &str, clock: Arc<SetClock>) -> Supervisor {
    let bin = scratch(tag).join("no-such-binary");
    let sup = Supervisor::start(config(tag, bin, vec![], clock)).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    sup
}

fn desc(id: ServiceId) -> &'static ServiceDescriptor {
    REGISTRY
        .iter()
        .find(|d| d.id == id)
        .unwrap_or_else(|| panic!("{id:?} is not in the registry"))
}

fn wait_receipt(sup: &Supervisor, token: crate::supervisor::ControlToken) -> Receipt {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let r = sup.receipt(token);
        if !matches!(r, Receipt::Pending | Receipt::Unknown { .. })
            || std::time::Instant::now() >= deadline
        {
            return r;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_registry_lists_the_nine_supervised_services_in_order() {
    let ids: Vec<ServiceId> = REGISTRY.iter().map(|d| d.id).collect();
    assert_eq!(
        ids,
        [
            ServiceId::Node,
            ServiceId::NodeAgent,
            ServiceId::Memory,
            ServiceId::LlamaChat,
            ServiceId::LlamaEmbed,
            ServiceId::Ipfs,
            ServiceId::Hermes,
            ServiceId::Comms,
            ServiceId::Cluster,
        ]
    );
    for d in &REGISTRY {
        assert!(!d.prerequisites.is_empty(), "{:?} names its prerequisites", d.id);
        assert!(!d.probe.is_empty() && !d.ready_signal.is_empty(), "{:?}", d.id);
    }
}

/// Tripwire: the registry is the list of what core supervises. Every non-test source file that
/// starts a kit `Supervisor` is the owner of exactly one registry row and exposes its slot, and
/// every row's owner really starts one. A new supervised service can't ship missing from
/// Node › Services.
#[test]
fn every_supervisor_start_in_core_is_a_registry_owner() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut starters: Vec<String> = std::fs::read_dir(&dir)
        .expect("read src")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".rs") && !name.ends_with("_tests.rs")
        })
        .filter(|p| {
            std::fs::read_to_string(p)
                .map(|s| s.contains("Supervisor::start(config)"))
                .unwrap_or(false)
        })
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(|n| format!("src-tauri/src/{n}")))
        .collect();
    starters.sort();
    let mut owners: Vec<String> = REGISTRY.iter().map(|d| d.owner.to_string()).collect();
    owners.sort();
    assert_eq!(starters, owners, "files that start a supervisor vs registry owners");
    for owner in &owners {
        let file = dir.join(owner.trim_start_matches("src-tauri/src/"));
        let src = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            src.contains("pub(crate) fn supervisor_slot(&self) -> &Mutex<Option<Supervisor>>"),
            "{owner} exposes its supervisor slot to the registry"
        );
    }
}

#[test]
fn no_owner_or_an_empty_slot_is_not_supervised_and_claims_no_phase() {
    let e = entry(desc(ServiceId::Comms), SlotRead::NoOwner);
    assert_eq!(e.observation, ObservationKind::NotSupervised);
    assert_eq!((e.phase, e.readiness, e.restarts), (None, None, None));
    assert!(e.last_transition.is_none() && e.last_crash.is_none());

    let empty: Mutex<Option<Supervisor>> = Mutex::new(None);
    let e = entry(desc(ServiceId::Ipfs), SlotRead::Slot(&empty));
    assert_eq!(e.observation, ObservationKind::NotSupervised);
    assert_eq!(e.phase, None);
}

#[test]
fn a_busy_owner_or_a_held_slot_is_busy_never_a_guess() {
    let e = entry(desc(ServiceId::Cluster), SlotRead::OwnerBusy);
    assert_eq!(e.observation, ObservationKind::Busy);
    assert_eq!(e.phase, None);

    // The owner holds its slot (a start in progress): the read gives up within its budget.
    let clock = Arc::new(SetClock(AtomicU64::new(1)));
    let slot = Arc::new(Mutex::new(Some(failing_service("held", clock))));
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let holder = {
        let slot = slot.clone();
        std::thread::spawn(move || {
            let _g = slot.lock().unwrap_or_else(|e| e.into_inner());
            let _ = held_tx.send(());
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        })
    };
    held_rx.recv_timeout(Duration::from_secs(5)).expect("holder took the slot");
    let started = std::time::Instant::now();
    let e = entry(desc(ServiceId::Node), SlotRead::Slot(&slot));
    assert!(started.elapsed() < Duration::from_secs(1), "the read is bounded");
    let _ = done_tx.send(());
    let _ = holder.join();
    assert_eq!(e.observation, ObservationKind::Busy);
    assert_eq!(e.phase, None);
}

#[test]
fn a_failed_service_reports_phase_transition_restarts_and_last_crash_from_its_supervisor() {
    let clock = Arc::new(SetClock(AtomicU64::new(4_200_000)));
    let slot = Mutex::new(Some(failing_service("failed", clock.clone())));
    clock.0.store(5_000_000, Ordering::SeqCst);
    let e = entry(desc(ServiceId::Memory), SlotRead::Slot(&slot));
    assert_eq!(e.observation, ObservationKind::Observed);
    assert_eq!(e.phase, Some(Phase::Failed));
    assert_eq!(e.readiness, Some(Readiness::NotAssessed));
    let t = e.last_transition.expect("a transition");
    assert_eq!(t.to, Phase::Failed);
    assert_eq!(t.at_unix_ms, 4_200_000, "when it happened, not when it was read");
    assert_eq!(e.restarts, Some(1));
    let crash = e.last_crash.clone().expect("last crash");
    assert_eq!(crash.at_unix_ms, 4_200_000);
    assert!(!crash.exit.is_empty());
    let json = serde_json::to_value(&e).unwrap_or_default();
    assert_eq!(json["phase"], "failed");
    assert_eq!(json["readiness"], "notAssessed");
    assert_eq!(json["lastTransition"]["to"], "failed");
    assert!(
        json["lastCrash"].get("stderrTail").is_none(),
        "the stderr tail never crosses to the view"
    );
}

#[cfg(unix)]
#[test]
fn a_live_child_is_alive_with_readiness_not_assessed_then_stopped_after_a_stop() {
    let clock = Arc::new(SetClock(AtomicU64::new(10_000)));
    let sup = Supervisor::start(config(
        "live",
        std::path::PathBuf::from("/bin/sleep"),
        vec!["30".into()],
        clock.clone(),
    ))
    .expect("start");
    sup.wait_until(|s| *s == SupervisorState::Running, Duration::from_secs(5));
    let slot = Mutex::new(Some(sup));
    let e = entry(desc(ServiceId::Node), SlotRead::Slot(&slot));
    assert_eq!(e.observation, ObservationKind::Observed);
    assert_eq!(e.phase, Some(Phase::Alive));
    assert_eq!(
        e.readiness,
        Some(Readiness::NotAssessed),
        "alive is not ready: readiness is its own axis"
    );
    assert_eq!(
        e.last_transition,
        Some(PhaseTransition {
            from: Some(Phase::Starting),
            to: Phase::Alive,
            at_unix_ms: 10_000,
        })
    );
    assert_eq!(e.restarts, Some(0));
    assert!(e.last_crash.is_none());

    clock.0.store(20_000, Ordering::SeqCst);
    let token = {
        let g = slot.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_ref().map(|s| s.request_stop()) {
            Some(ControlResult::Accepted(t)) => t,
            other => panic!("stop: {other:?}"),
        }
    };
    let receipt = {
        let g = slot.lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref().map(|s| wait_receipt(s, token))
    };
    assert!(matches!(receipt, Some(Receipt::Complete { .. })), "{receipt:?}");
    let e = entry(desc(ServiceId::Node), SlotRead::Slot(&slot));
    assert_eq!(e.phase, Some(Phase::Stopped));
    assert_eq!(
        e.last_transition,
        Some(PhaseTransition {
            from: Some(Phase::Stopping),
            to: Phase::Stopped,
            at_unix_ms: 20_000,
        })
    );
    assert!(e.last_crash.is_none(), "an intentional stop is not a crash");
}

/// The JSON contract Node › Services reads, and the N-8 tripwire: only the node has a log read.
#[test]
fn the_snapshot_serializes_the_contract_the_tab_reads() {
    let clock = SetClock(AtomicU64::new(1_790_000_000_000));
    let snap = snapshot_with(&clock, |_| SlotRead::NoOwner);
    let v = serde_json::to_value(&snap).unwrap_or_default();
    assert_eq!(v["takenAtUnixMs"], 1_790_000_000_000u64);
    let rows = v["services"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 9);
    let node = &rows[0];
    assert_eq!(node["id"], "node");
    assert_eq!(node["observation"], "notSupervised");
    assert!(node["phase"].is_null() && node["readiness"].is_null());
    assert_eq!(node["logBudgetLines"], 500);
    assert_eq!(node["controls"], true);
    let ids: Vec<&str> = rows.iter().filter_map(|r| r["id"].as_str()).collect();
    assert_eq!(
        ids,
        ["node", "nodeAgent", "memory", "llamaChat", "llamaEmbed", "ipfs", "hermes", "comms", "cluster"]
    );
    let with_logs: Vec<&str> = rows
        .iter()
        .filter(|r| !r["logsCommand"].is_null())
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert_eq!(with_logs, ["node"], "logs are not available yet for the other eight (N-8)");
    assert_eq!(rows[0]["logsCommand"], "node_logs");
}

// ---------------------------------------------------------------------------
// CORE-D4 — `sidecars_health`: one row per sidecar with state, restart counters, last crash, the
// slow retry (CORE-D3) and the last N log lines, all from that sidecar's supervisor.
// ---------------------------------------------------------------------------

const HOME: &str = "/Users/member";

#[test]
fn a_health_row_reports_counters_last_crash_and_the_slow_retry_from_the_supervisor() {
    let clock = Arc::new(SetClock(AtomicU64::new(4_200_000)));
    let bin = scratch("health-failed").join("no-such-binary");
    let mut cfg = config("health-failed", bin, vec![], clock);
    cfg.slow_retry = Some(Duration::from_secs(600));
    let sup = Supervisor::start(cfg).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    let slot = Mutex::new(Some(sup));
    let h = health_entry(desc(ServiceId::Memory), SlotRead::Slot(&slot), 20, HOME);
    assert_eq!(h.service.observation, ObservationKind::Observed);
    assert_eq!(h.service.phase, Some(Phase::Failed));
    let counters = h.counters.expect("counters from the lifecycle cell");
    assert_eq!(counters.attempts, 1);
    assert_eq!(counters.consecutive_failures, 1);
    assert!(h.service.last_crash.is_some(), "the last crash is on the row");
    let slow = h.slow_retry.expect("a Failed sidecar with a slow retry says when it is next tried");
    assert_eq!(slow.every_ms, 600_000);
    assert_eq!(slow.next_at_unix_ms, 4_200_000 + 600_000);
    let v = serde_json::to_value(&h).unwrap_or_default();
    assert_eq!(v["id"], "memory", "the registry row is flattened into the health row");
    assert_eq!(v["phase"], "failed");
    assert_eq!(v["counters"]["attempts"], "1");
    assert_eq!(v["slowRetry"]["everyMs"], "600000");
    assert!(v["logTail"].is_array());
    assert!(v["lastCrash"].get("stderrTail").is_none());
}

#[test]
fn an_unobserved_health_row_claims_nothing() {
    let h = health_entry(desc(ServiceId::Comms), SlotRead::NoOwner, 20, HOME);
    assert_eq!(h.service.observation, ObservationKind::NotSupervised);
    assert!(h.counters.is_none() && h.slow_retry.is_none());
    assert!(h.log_tail.is_empty());
    let clock = SetClock(AtomicU64::new(9));
    let all = health_with(&clock, 20, HOME, |_| SlotRead::NoOwner, |_| None);
    assert_eq!(all.services.len(), 9, "one health row per registry row");
    assert_eq!(all.taken_at_unix_ms, 9);
}

#[cfg(unix)]
#[test]
fn the_log_tail_is_the_newest_lines_bounded_and_scrubbed() {
    let clock = Arc::new(SetClock(AtomicU64::new(1)));
    let script = "i=0; while [ $i -lt 30 ]; do echo line-$i; i=$((i+1)); done; \
                  echo key sk-abcdefghijklmnopqrstuvwx at /Users/member/Library/x; \
                  echo from 0x1111111111111111111111111111111111111111; sleep 30";
    let sup = Supervisor::start(config(
        "health-logs",
        std::path::PathBuf::from("/bin/sh"),
        vec!["-c".into(), script.into()],
        clock,
    ))
    .expect("start");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while sup.logs().len() < 32 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let slot = Mutex::new(Some(sup));
    let h = health_entry(desc(ServiceId::Ipfs), SlotRead::Slot(&slot), 5, HOME);
    assert_eq!(h.log_tail.len(), 5, "bounded to the asked number of lines");
    assert_eq!(h.log_lines_kept, Some(32), "the ring holds every line so far");
    let text: Vec<&str> = h.log_tail.iter().map(|l| l.line.as_str()).collect();
    assert_eq!(text[0], "line-27", "the newest lines, oldest first: {text:?}");
    let joined = text.join("\n");
    assert!(!joined.contains("sk-abcdefghijklmnopqrstuvwx"), "a key shape never crosses: {joined}");
    assert!(!joined.contains("/Users/member"), "the home path is folded: {joined}");
    assert!(!joined.contains("0x1111111111111111111111111111111111111111"), "{joined}");
    assert!(joined.contains("<redacted>"));
    let taken = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(sup) = taken {
        sup.stop();
    }
}

#[test]
fn the_tail_asked_for_is_clamped() {
    assert_eq!(clamp_tail(None), HEALTH_LOG_LINES);
    assert_eq!(clamp_tail(Some(0)), 0);
    assert_eq!(clamp_tail(Some(10)), 10);
    assert_eq!(clamp_tail(Some(1_000_000)), MAX_HEALTH_LOG_LINES);
}

#[test]
fn the_node_row_carries_the_liveness_note() {
    let clock = SetClock(AtomicU64::new(9));
    let all = health_with(&clock, 0, HOME, |_| SlotRead::NoOwner, |id| {
        (id == ServiceId::Node).then(|| "liveness restart #1: head stuck".to_string())
    });
    assert_eq!(all.services[0].note.as_deref(), Some("liveness restart #1: head stuck"));
    assert!(all.services[1..].iter().all(|h| h.note.is_none()));
}

// ---------------------------------------------------------------------------
// CORE-D3 — the alert the UI is sent when a sidecar fails, and when it comes back.
// ---------------------------------------------------------------------------

fn row(id: ServiceId, observation: ObservationKind, phase: Option<Phase>) -> SidecarHealth {
    let mut h = health_entry(desc(id), SlotRead::NoOwner, 0, HOME);
    h.service.observation = observation;
    h.service.phase = phase;
    h
}

#[test]
fn an_alert_is_raised_once_when_a_sidecar_fails_and_once_when_it_recovers() {
    let mut t = AlertTracker::default();
    let alive = [row(ServiceId::Hermes, ObservationKind::Observed, Some(Phase::Alive))];
    assert!(t.update(&alive, 1).is_empty(), "nothing to say about a live sidecar");

    let mut failed = row(ServiceId::Hermes, ObservationKind::Observed, Some(Phase::Failed));
    failed.slow_retry = Some(crate::supervisor::SlowRetry {
        every_ms: 600_000,
        next_at_unix_ms: 700_000,
        attempts: 0,
    });
    failed.service.last_crash = Some(CrashView {
        at_unix_ms: 5,
        exit: "exit status: 1".into(),
        restart_attempt: 8,
    });
    let alerts = t.update(std::slice::from_ref(&failed), 2);
    assert_eq!(alerts.len(), 1);
    let a = &alerts[0];
    assert_eq!(a.id, ServiceId::Hermes);
    assert_eq!(a.kind, AlertKind::Failed);
    assert_eq!(a.next_retry_at_unix_ms, Some(700_000));
    assert_eq!(a.reason.as_deref(), Some("exit status: 1"));
    let v = serde_json::to_value(a).unwrap_or_default();
    assert_eq!(v["kind"], "failed");
    assert_eq!(v["label"], "Hermes");

    assert!(t.update(std::slice::from_ref(&failed), 3).is_empty(), "no repeat while still Failed");
    // A read that didn't get the lock says nothing and forgets nothing.
    let busy = [row(ServiceId::Hermes, ObservationKind::Busy, None)];
    assert!(t.update(&busy, 4).is_empty());
    assert!(t.update(std::slice::from_ref(&failed), 5).is_empty());

    let back = t.update(&alive, 6);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].kind, AlertKind::Recovered);
    assert!(t.update(&alive, 7).is_empty());
}

#[test]
fn a_sidecar_already_failed_at_the_first_read_is_alerted() {
    let mut t = AlertTracker::default();
    let failed = [row(ServiceId::Node, ObservationKind::Observed, Some(Phase::Failed))];
    let alerts = t.update(&failed, 1);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].kind, AlertKind::Failed);
}

#[test]
fn the_slow_retry_period_reads_its_override_and_refuses_churning_values() {
    let env = |v: Option<&'static str>| move |k: &str| (k == SLOW_RETRY_SECS_ENV).then(|| v).flatten().map(str::to_string);
    assert_eq!(slow_retry_from(env(None)), Some(DEFAULT_SLOW_RETRY));
    assert_eq!(slow_retry_from(env(Some("0"))), None, "0 turns the slow retry off");
    assert_eq!(slow_retry_from(env(Some("off"))), None);
    assert_eq!(slow_retry_from(env(Some("900"))), Some(Duration::from_secs(900)));
    assert_eq!(
        slow_retry_from(env(Some("5"))),
        Some(DEFAULT_SLOW_RETRY),
        "under the floor: the default stands"
    );
    assert_eq!(slow_retry_from(env(Some("soon"))), Some(DEFAULT_SLOW_RETRY));
}

/// Tripwire (CORE-D3): every registry owner gives its supervisor the app's slow retry, so no
/// sidecar is left `Failed` for good while the others recover.
#[test]
fn every_registry_owner_configures_the_slow_retry() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for d in &REGISTRY {
        let file = dir.join(d.owner.trim_start_matches("src-tauri/src/"));
        let src = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            src.contains("config.slow_retry = crate::services::slow_retry_every();"),
            "{} must set its supervisor's slow retry",
            d.owner
        );
    }
}
