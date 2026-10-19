// citrate-core — SidecarSupervisor lifecycle cell tests (SCL-S2, SC1). Platform-neutral: they
// run on Unix and Windows. The process fault fixtures for SC1 live in the integration suite
// `kit/tests/lifecycle_faults.rs`.

/// A service whose binary does not exist: every spawn fails at once, on every OS.
fn missing_bin_config(tag: &str, max_retries: u32) -> SupervisorConfig {
    let dir = std::env::temp_dir().join(format!(
        "citrate-core-sup-lc-{tag}-{}",
        std::process::id()
    ));
    let spec = SidecarSpec::new(
        format!("lc-{tag}"),
        dir.join("no-such-binary"),
        vec!["--flag".into()],
    );
    let mut cfg = SupervisorConfig::new(spec, dir.join("crash.jsonl"));
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 1,
        max_delay: Duration::from_millis(5),
        max_retries,
    };
    cfg
}

fn wait_receipt(sup: &Supervisor, token: ControlToken) -> Receipt {
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
fn lifecycle_snapshot_counters_cross_as_decimal_strings() {
    let snap = LifecycleSnapshot {
        name: "svc".into(),
        phase: Phase::Stopping,
        readiness: Readiness::NotAssessed,
        desired: Desired::Stopped,
        app_epoch: u64::MAX,
        intent_sequence: 9_007_199_254_740_993, // 2^53 + 1: not exact as a JS number
        generation: 1,
        incarnation: 2,
        revision: 10,
        restarts: 3,
        pid: Some(42),
        quarantine: None,
        open_tickets: 2,
    };
    let v = serde_json::to_value(&snap).unwrap_or_default();
    assert_eq!(v["appEpoch"], "18446744073709551615");
    assert_eq!(v["intentSequence"], "9007199254740993");
    assert_eq!(v["generation"], "1");
    assert_eq!(v["incarnation"], "2");
    assert_eq!(v["revision"], "10");
    assert_eq!(v["restarts"], "3");
    assert_eq!(v["pid"], 42);
    assert_eq!(v["openTickets"], 2);
    assert_eq!(v["phase"], "stopping");
    assert_eq!(v["readiness"], "notAssessed");
    assert_eq!(v["desired"], "stopped");
}

#[test]
fn phase_vocabulary_adds_stopping_and_quarantined_and_readiness_is_separate() {
    let phases: Vec<String> = [
        Phase::Stopped,
        Phase::Starting,
        Phase::Alive,
        Phase::Backoff,
        Phase::Failed,
        Phase::Stopping,
        Phase::Quarantined,
    ]
    .iter()
    .map(|p| serde_json::to_value(p).unwrap_or_default().to_string())
    .collect();
    assert_eq!(
        phases,
        [
            "\"stopped\"",
            "\"starting\"",
            "\"alive\"",
            "\"backoff\"",
            "\"failed\"",
            "\"stopping\"",
            "\"quarantined\""
        ]
    );
    let readiness: Vec<String> = [
        Readiness::NotAssessed,
        Readiness::Awaiting,
        Readiness::Ready,
        Readiness::Stale,
        Readiness::Unavailable,
    ]
    .iter()
    .map(|r| serde_json::to_value(r).unwrap_or_default().to_string())
    .collect();
    assert_eq!(
        readiness,
        [
            "\"notAssessed\"",
            "\"awaiting\"",
            "\"ready\"",
            "\"stale\"",
            "\"unavailable\""
        ]
    );
}

#[test]
fn receipts_serialize_with_a_kind_tag() {
    let started = serde_json::to_value(Receipt::Started {
        incarnation: u64::MAX,
    })
    .unwrap_or_default();
    assert_eq!(started["kind"], "started");
    assert_eq!(started["incarnation"], "18446744073709551615");
    let incomplete = serde_json::to_value(Receipt::Incomplete {
        remaining: vec!["reader:stdout".into()],
        reason: "held".into(),
    })
    .unwrap_or_default();
    assert_eq!(incomplete["kind"], "incomplete");
    assert_eq!(incomplete["remaining"][0], "reader:stdout");
    let pending = serde_json::to_value(Receipt::Pending).unwrap_or_default();
    assert_eq!(pending["kind"], "pending");
}

#[test]
fn descriptor_bytes_count_every_field() {
    let mut spec = SidecarSpec::new("abc", "/x/y", vec!["12".into(), "345".into()]);
    spec.env = vec![("K".into(), "vv".into())];
    spec.workdir = Some(PathBuf::from("/w"));
    assert_eq!(spec.descriptor_bytes(), 3 + 4 + 5 + 3 + 2);
}

#[test]
fn an_oversized_descriptor_is_rejected_before_anything_runs() {
    let mut cfg = missing_bin_config("oversized", 0);
    cfg.spec.args = vec!["x".repeat(MAX_DESCRIPTOR_BYTES)];
    match Supervisor::start(cfg) {
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput),
        Ok(_) => panic!("a descriptor over {MAX_DESCRIPTOR_BYTES} bytes was accepted"),
    }
}

#[test]
fn start_receipt_reports_failed_and_foreign_tokens_are_expired() {
    let sup = Supervisor::start(missing_bin_config("failed", 0)).expect("start");
    let st = sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    assert_eq!(st.state, SupervisorState::Failed);
    let snap = sup.lifecycle().expect("snapshot");
    assert_eq!(snap.phase, Phase::Failed);
    assert_eq!(snap.intent_sequence, 1);
    let first = ControlToken {
        app_epoch: snap.app_epoch,
        intent_sequence: 1,
    };
    assert!(
        matches!(sup.receipt(first), Receipt::Failed { .. }),
        "{:?}",
        sup.receipt(first)
    );
    let other_run = ControlToken {
        app_epoch: snap.app_epoch.wrapping_add(1),
        intent_sequence: 1,
    };
    assert_eq!(sup.receipt(other_run), Receipt::Expired);
    let never_issued = ControlToken {
        app_epoch: snap.app_epoch,
        intent_sequence: 99,
    };
    assert_eq!(sup.receipt(never_issued), Receipt::Expired);
}

#[test]
fn a_start_after_failed_gets_a_fresh_budget_and_a_stop_settles_complete() {
    let sup = Supervisor::start(missing_bin_config("fresh", 0)).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    assert_eq!(sup.status().restarts, 1);
    let rev_before = sup.lifecycle().expect("snapshot").revision;

    let start = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start after Failed: {other:?}"),
    };
    assert_eq!(start.intent_sequence, 2);
    assert!(matches!(wait_receipt(&sup, start), Receipt::Failed { .. }));
    assert_eq!(sup.status().restarts, 2, "one more attempt under a fresh budget");

    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop on a Failed service: {other:?}"),
    };
    assert_eq!(stop.intent_sequence, 3);
    match wait_receipt(&sup, stop) {
        Receipt::Complete { scope } => {
            assert!(!scope.contains(&"child".to_string()), "there was no child: {scope:?}")
        }
        other => panic!("{other:?}"),
    }
    let snap = sup.lifecycle().expect("snapshot");
    assert_eq!(snap.phase, Phase::Stopped);
    assert_eq!(snap.desired, Desired::Stopped);
    assert_eq!(sup.status().state, SupervisorState::Off);
    assert!(snap.revision > rev_before, "every change bumps the revision");
    assert!(matches!(
        sup.request_stop(),
        ControlResult::AlreadySatisfied(_)
    ));
    assert_eq!(sup.status().restarts, 2, "Stop never consumes failure budget");
}

#[test]
fn a_superseded_start_receipt_says_so() {
    let sup = Supervisor::start(missing_bin_config("superseded", 0)).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    let start = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("{other:?}"),
    };
    // Commit a Stop before the Start can settle or right after: either way the Start's receipt
    // is no longer Pending and is never invented as a success.
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("{other:?}"),
    };
    assert!(matches!(
        sup.receipt(start),
        Receipt::Superseded | Receipt::Failed { .. } | Receipt::Expired
    ));
    assert!(matches!(wait_receipt(&sup, stop), Receipt::Complete { .. }));
}

/// A publisher whose thread is alive but never runs its body (it never observes).
fn idle_publisher() -> Arc<DeadlinePublisher> {
    DeadlinePublisher::start_with_spawner(Duration::from_millis(20), |body| {
        std::thread::Builder::new().spawn(move || {
            let _never_run = body;
            loop {
                std::thread::park();
            }
        })
    })
}

#[test]
fn deadline_overlay_arms_up_to_its_fixed_size_and_frees_on_release() {
    let p = idle_publisher();
    for seq in 0..DEADLINE_RECORDS as u64 {
        assert!(p.arm(7, 1, seq, Duration::from_secs(60)));
    }
    assert_eq!(p.armed_records(), DEADLINE_RECORDS);
    assert!(!p.arm(7, 1, 99, Duration::from_secs(60)), "no record beyond the overlay");
    p.release(7, 1, 3);
    assert_eq!(p.armed_records(), DEADLINE_RECORDS - 1);
    assert!(p.arm(7, 1, 99, Duration::from_secs(60)), "a freed record is reused");
    assert_eq!(p.expired(7, 1, 3), Some(false), "a released control has no record");
}

#[test]
fn a_deadline_read_compares_the_monotonic_clock_without_the_publisher() {
    let p = idle_publisher();
    assert!(p.is_running());
    assert!(p.arm(1, 1, 1, Duration::from_millis(20)));
    assert_eq!(p.expired(1, 1, 1), Some(false));
    std::thread::sleep(Duration::from_millis(40));
    let token = ControlToken {
        app_epoch: 1,
        intent_sequence: 1,
    };
    assert_eq!(p.record_state(1, token), Some(DeadlineState::Armed), "never marked");
    assert_eq!(p.expired(1, 1, 1), Some(true), "but the read sees the deadline passed");
}

#[test]
fn a_stale_expiry_mark_never_applies_to_a_rewritten_record() {
    let p = idle_publisher();
    assert!(p.arm(1, 1, 1, Duration::ZERO));
    p.observe(); // marks the expired record
    let a = ControlToken {
        app_epoch: 1,
        intent_sequence: 1,
    };
    assert_eq!(p.record_state(1, a), Some(DeadlineState::Expired));
    p.release(1, 1, 1);
    assert!(p.arm(2, 1, 5, Duration::from_secs(60)), "the same record, rewritten");
    let b = ControlToken {
        app_epoch: 1,
        intent_sequence: 5,
    };
    assert_eq!(p.record_state(2, b), Some(DeadlineState::Armed));
    assert_eq!(p.expired(2, 1, 5), Some(false), "A's mark does not carry over to B");
}

#[test]
fn the_publisher_marks_expiry_within_its_cadence() {
    let p = DeadlinePublisher::start(Duration::from_millis(10));
    assert!(p.is_running() && p.init_error().is_none());
    assert!(p.arm(3, 1, 1, Duration::from_millis(30)));
    let token = ControlToken {
        app_epoch: 1,
        intent_sequence: 1,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while p.record_state(3, token) != Some(DeadlineState::Expired) {
        assert!(std::time::Instant::now() < deadline, "the publisher never marked it");
        std::thread::sleep(Duration::from_millis(5));
    }
}
