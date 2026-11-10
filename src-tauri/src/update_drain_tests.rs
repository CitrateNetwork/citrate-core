// CORE-D6 — the pre-update drain. Pure: fixture snapshots and a private spawn gate, except the last
// test, which drains a real supervisor through the same function.
use super::*;
use crate::services::{entry, SlotRead, REGISTRY};

fn snapshot(f: impl Fn(ServiceId) -> (ObservationKind, Option<Phase>)) -> ServicesSnapshot {
    ServicesSnapshot {
        taken_at_unix_ms: 1,
        services: REGISTRY
            .iter()
            .map(|d| {
                let mut e = entry(d, SlotRead::NoOwner);
                let (o, p) = f(d.id);
                e.observation = o;
                e.phase = p;
                e
            })
            .collect(),
    }
}

fn running() -> ServicesSnapshot {
    snapshot(|id| match id {
        ServiceId::Node | ServiceId::Memory | ServiceId::Hermes => (ObservationKind::Observed, Some(Phase::Alive)),
        _ => (ObservationKind::NotSupervised, None),
    })
}

fn drained() -> ServicesSnapshot {
    snapshot(|_| (ObservationKind::NotSupervised, None))
}

#[test]
fn the_gate_is_held_before_anything_is_stopped_and_the_report_says_before_and_after() {
    let gate = SpawnGate::default();
    let reads = std::cell::Cell::new(0);
    let stopped_with_gate_held = std::cell::Cell::new(false);
    let report = drain_with(
        &gate,
        || {
            reads.set(reads.get() + 1);
            if reads.get() == 1 { running() } else { drained() }
        },
        || stopped_with_gate_held.set(gate.held().is_some()),
        Vec::new,
        || 42,
    );
    assert!(stopped_with_gate_held.get(), "no sidecar can restart while the drain stops them");
    assert_eq!(reads.get(), 2);
    assert!(report.all_stopped);
    assert_eq!(report.took_ms, 42);
    assert_eq!(report.services.len(), REGISTRY.len());
    let node = report.services.iter().find(|s| s.id == ServiceId::Node).unwrap();
    assert_eq!((node.before.as_str(), node.after.as_str(), node.stopped), ("alive", "notSupervised", true));
    let ipfs = report.services.iter().find(|s| s.id == ServiceId::Ipfs).unwrap();
    assert_eq!((ipfs.before.as_str(), ipfs.after.as_str()), ("notSupervised", "notSupervised"));
    assert_eq!(gate.held().as_deref(), Some(HOLD_REASON), "the gate stays held until the relaunch or a cancel");
    let v = serde_json::to_value(&report).unwrap();
    assert_eq!(v["allStopped"], true);
    assert_eq!(v["services"][0]["after"], "notSupervised");
}

#[test]
fn a_sidecar_still_alive_or_unread_after_the_drain_means_not_all_stopped() {
    for (o, p) in [
        (ObservationKind::Observed, Some(Phase::Alive)),
        (ObservationKind::Observed, Some(Phase::Stopping)),
        (ObservationKind::Observed, Some(Phase::Backoff)),
        (ObservationKind::Busy, None),
    ] {
        let gate = SpawnGate::default();
        let after = snapshot(|id| if id == ServiceId::Memory { (o, p) } else { (ObservationKind::NotSupervised, None) });
        let reads = std::cell::Cell::new(0);
        let report = drain_with(
            &gate,
            || {
                reads.set(reads.get() + 1);
                if reads.get() == 1 { running() } else { after.clone() }
            },
            || {},
            Vec::new,
            || 0,
        );
        assert!(!report.all_stopped, "{o:?} {p:?}");
        let m = report.services.iter().find(|s| s.id == ServiceId::Memory).unwrap();
        assert!(!m.stopped);
    }
}

#[test]
fn stopped_and_failed_phases_hold_no_process() {
    let gate = SpawnGate::default();
    let after = snapshot(|id| match id {
        ServiceId::Node => (ObservationKind::Observed, Some(Phase::Stopped)),
        ServiceId::Comms => (ObservationKind::Observed, Some(Phase::Failed)),
        _ => (ObservationKind::NotSupervised, None),
    });
    let report = drain_with(&gate, || after.clone(), || {}, Vec::new, || 0);
    assert!(report.all_stopped);
    assert_eq!(report.services.iter().find(|s| s.id == ServiceId::Comms).unwrap().after, "failed");
}

#[cfg(unix)]
#[test]
fn a_real_supervisor_is_stopped_and_cannot_come_back_until_the_gate_is_released() {
    use citrate_core_kit::supervisor::{SidecarSpec, Supervisor, SupervisorConfig};
    let dir = std::env::temp_dir().join(format!("citrate-d6-drain-{}", std::process::id()));
    let gate = std::sync::Arc::new(SpawnGate::default());
    let config = || {
        let mut c = SupervisorConfig::new(
            SidecarSpec::new("d6-node", "/bin/sh", vec!["-c".into(), "exec sleep 60".into()]),
            dir.join("crash.jsonl"),
        );
        c.spawn_gate = gate.clone();
        c
    };
    let sup = Supervisor::start(config()).expect("start");
    let st = sup.wait_until(|s| matches!(s, citrate_core_kit::supervisor::SupervisorState::Running), std::time::Duration::from_secs(5));
    let pid = st.pid.expect("a child is running");
    let slot = std::sync::Mutex::new(Some(sup));
    let desc = REGISTRY.iter().find(|d| d.id == ServiceId::Node).unwrap();
    let read = || ServicesSnapshot { taken_at_unix_ms: 0, services: vec![entry(desc, SlotRead::Slot(&slot))] };
    let report = drain_with(
        &gate,
        read,
        || {
            let held = slot.lock().unwrap().take();
            if let Some(s) = held {
                s.stop();
            }
        },
        || gate.wait_until_no_children(std::time::Duration::from_secs(10)),
        || 0,
    );
    assert!(report.all_stopped, "{report:?}");
    assert_eq!(report.services[0].before, "alive");
    // SAFETY: kill(pid, 0) only probes existence.
    assert!(unsafe { libc::kill(pid as libc::pid_t, 0) } != 0, "the child is gone");
    let refused = Supervisor::start(config()).err().expect("held: nothing starts");
    assert!(refused.to_string().contains(HOLD_REASON), "{refused}");
    gate.release();
    let again = Supervisor::start(config()).expect("released: it starts again");
    again.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_drain_commands_name_their_data_sources_and_are_registered() {
    let src = include_str!("update_drain.rs");
    for cmd in ["update_prepare", "update_prepare_cancel"] {
        let at = src.find(&format!("pub async fn {cmd}(")).unwrap_or_else(|| panic!("{cmd}"));
        let doc = &src[src[..at].rfind(&format!("/// **{cmd}**")).unwrap_or_else(|| panic!("{cmd} doc"))..at];
        assert!(doc.contains("Data source"), "{cmd} names its data source (Rule 7)");
        assert!(include_str!("lib.rs").contains(&format!("update_drain::{cmd},")), "{cmd} registered");
        assert!(include_str!("../permissions/main-window.toml").contains(&format!("\"{cmd}\"")), "{cmd} in the ACL");
    }
}

/// Every registry owner's supervisor is built with `SupervisorConfig::new`, so every sidecar passes
/// the app-wide gate (none sets its own).
#[test]
fn no_registry_owner_replaces_the_app_wide_gate() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for d in &REGISTRY {
        let src = std::fs::read_to_string(dir.join(d.owner.trim_start_matches("src-tauri/src/"))).unwrap_or_default();
        assert!(src.contains("SupervisorConfig::new("), "{}", d.owner);
        assert!(!src.contains(".spawn_gate ="), "{} must not replace the spawn gate", d.owner);
    }
}

/// #614 review (P2): a child still exiting after the owners' stops (an empty slot is not an exit)
/// means not all stopped, and the report names it.
#[test]
fn a_child_still_exiting_after_the_stops_means_not_all_stopped() {
    let gate = SpawnGate::default();
    let report = drain_with(&gate, drained, || {}, || vec!["citrate-node".to_string()], || 0);
    assert!(!report.all_stopped);
    assert_eq!(report.still_exiting, vec!["citrate-node".to_string()]);
    let v = serde_json::to_value(&report).unwrap();
    assert_eq!(v["stillExiting"][0], "citrate-node");
    let ok = drain_with(&gate, drained, || {}, Vec::new, || 0);
    assert!(ok.all_stopped && ok.still_exiting.is_empty());
}

/// #614 review (P2): cancelling releases the gate and reopens knowledge imports (the drain closed
/// them), so the memory daemon and imports can start again without an app restart.
#[test]
fn cancelling_reopens_imports_and_releases_the_gate() {
    let gate = SpawnGate::default();
    let imports = crate::knowledge_import::ImportRegistry::default();
    gate.hold(HOLD_REASON);
    imports.shutdown(std::time::Duration::from_millis(10));
    assert!(imports.is_closed());
    cancel_with(&gate, &imports);
    assert!(gate.held().is_none());
    assert!(!imports.is_closed());
    assert_eq!(imports.unless_closed(|| 7), Some(7));
}

#[test]
fn the_production_drain_waits_for_real_exits_and_cancel_reopens_imports() {
    let src = include_str!("update_drain.rs");
    assert!(src.contains("SpawnGate::global().wait_until_no_children("), "the command waits for real exits");
    assert!(src.contains("cancel_with(&SpawnGate::global(), &crate::knowledge_import::ImportRegistry::global())"));
    // #614 review (P2): no claim that the node flushes on SIGTERM (it handles SIGINT only).
    assert!(!src.contains("so the node flushes"), "the doc claims a flush the node does not do");
}
