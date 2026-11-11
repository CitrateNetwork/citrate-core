// citrate-core — CORE-D6: while an update is being installed, the spawn gate holds every new start.
// Platform-neutral: each test gives its supervisors their own gate (never the app-wide one), with
// a binary that does not exist, so nothing here depends on a real child.

fn gated_config(tag: &str, gate: &Arc<SpawnGate>) -> SupervisorConfig {
    let mut cfg = missing_bin_config(&format!("gate-{tag}"), 0);
    cfg.spawn_gate = gate.clone();
    cfg
}

#[test]
fn a_held_gate_refuses_a_new_supervisor_with_its_reason() {
    let gate = Arc::new(SpawnGate::default());
    gate.hold("an update is being installed");
    let err = Supervisor::start(gated_config("start", &gate)).err().expect("refused");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    assert!(err.to_string().contains("an update is being installed"), "{err}");
    gate.release();
    let sup = Supervisor::start(gated_config("start", &gate)).expect("starts once released");
    sup.stop();
}

#[test]
fn a_held_gate_refuses_member_and_automated_starts_and_commits_nothing() {
    let gate = Arc::new(SpawnGate::default());
    let sup = Supervisor::start(gated_config("intent", &gate)).expect("start");
    sup.request_stop();
    let before = sup.lifecycle().map(|l| l.intent_sequence);
    gate.hold("an update is being installed");
    for r in [sup.request_start(), sup.request_auto_start(), sup.request_auto_restart()] {
        assert_eq!(r, ControlResult::Held("an update is being installed".to_string()));
    }
    assert_eq!(sup.lifecycle().map(|l| l.intent_sequence), before, "no intent was committed");
    gate.release();
    assert!(!matches!(sup.request_start(), ControlResult::Held(_)), "released: a Start is admitted again");
    sup.stop();
}

#[test]
fn a_stop_is_never_held() {
    let gate = Arc::new(SpawnGate::default());
    let sup = Supervisor::start(gated_config("stop", &gate)).expect("start");
    gate.hold("an update is being installed");
    assert!(!matches!(sup.request_stop(), ControlResult::Held(_)), "the drain must be able to stop it");
    sup.stop();
    gate.release();
}

#[test]
fn the_default_config_uses_the_app_wide_gate() {
    let cfg = missing_bin_config("gate-default", 0);
    assert!(Arc::ptr_eq(&cfg.spawn_gate, &SpawnGate::global()));
    assert!(SpawnGate::default().held().is_none(), "a new gate is open");
}

// #614 review (P2): an empty owner slot is not proof that the child exited. The gate also tracks
// every supervisor started under it and reports those whose child is still running, so the
// pre-update drain can wait for real exits.

#[test]
fn a_supervisor_with_no_child_is_never_reported_as_running() {
    let gate = Arc::new(SpawnGate::default());
    let sup = Supervisor::start(gated_config("nochild", &gate)).expect("start");
    // The binary does not exist: no spawn ever succeeds, so no pid is ever published.
    let _ = sup.wait_until(|s| matches!(s, SupervisorState::Failed | SupervisorState::Backoff { .. }), Duration::from_secs(5));
    assert!(gate.live_children().is_empty(), "{:?}", gate.live_children());
    assert!(gate.wait_until_no_children(Duration::from_millis(50)).is_empty());
    sup.stop();
}

#[cfg(unix)]
#[test]
fn a_running_child_is_reported_until_it_has_really_exited() {
    let gate = Arc::new(SpawnGate::default());
    let mut cfg = SupervisorConfig::new(
        SidecarSpec::new("gate-live", sh_bin(), vec!["-c".into(), "exec sleep 60".into()]),
        std::env::temp_dir().join(format!("citrate-gate-live-{}.jsonl", std::process::id())),
    );
    cfg.spawn_gate = gate.clone();
    let sup = Supervisor::start(cfg).expect("start");
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    assert!(st.pid.is_some());
    assert_eq!(gate.live_children(), vec!["gate-live".to_string()]);
    // A Stop in flight on another thread (as when the member pressed Stop just before the drain,
    // and the owner already took the supervisor out of its slot).
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        sup.stop();
        drop(sup);
    });
    assert_eq!(
        gate.wait_until_no_children(Duration::from_millis(50)),
        vec!["gate-live".to_string()],
        "still running while the stop is in flight"
    );
    assert!(gate.wait_until_no_children(Duration::from_secs(10)).is_empty(), "gone once it exited");
    let _ = stopper.join();
}
