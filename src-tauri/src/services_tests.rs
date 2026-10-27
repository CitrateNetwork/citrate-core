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
