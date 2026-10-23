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
        instance: 7,
        intent_sequence: 9_007_199_254_740_993, // 2^53 + 1: not exact as a JS number
        generation: 1,
        incarnation: 2,
        revision: 10,
        restarts: 3,
        pid: Some(42),
        quarantine: None,
    };
    let v = serde_json::to_value(&snap).unwrap_or_default();
    assert_eq!(v["appEpoch"], "18446744073709551615");
    assert_eq!(v["instance"], "7");
    assert_eq!(v["intentSequence"], "9007199254740993");
    assert_eq!(v["generation"], "1");
    assert_eq!(v["incarnation"], "2");
    assert_eq!(v["revision"], "10");
    assert_eq!(v["restarts"], "3");
    assert_eq!(v["pid"], 42);
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
        instance: snap.instance,
        intent_sequence: 1,
    };
    assert!(
        matches!(sup.receipt(first), Receipt::Failed { .. }),
        "{:?}",
        sup.receipt(first)
    );
    let other_run = ControlToken {
        app_epoch: snap.app_epoch.wrapping_add(1),
        instance: snap.instance,
        intent_sequence: 1,
    };
    assert_eq!(sup.receipt(other_run), Receipt::Expired);
    let never_issued = ControlToken {
        app_epoch: snap.app_epoch,
        instance: snap.instance,
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

/// Identical concurrent Starts after Failed join one intent: none supersedes another.
#[test]
fn concurrent_starts_after_failed_join_one_intent() {
    for round in 0..20 {
        let sup = Arc::new(
            Supervisor::start(missing_bin_config(&format!("join-{round}"), 0)).expect("start"),
        );
        sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
        let starters: Vec<_> = (0..4)
            .map(|_| {
                let s = sup.clone();
                std::thread::spawn(move || s.request_start())
            })
            .collect();
        let tokens: Vec<ControlToken> = starters
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter_map(|r| match r {
                ControlResult::Accepted(t) => Some(t),
                _ => None,
            })
            .collect();
        for t in &tokens {
            assert_ne!(
                wait_receipt(&sup, *t),
                Receipt::Superseded,
                "round {round}: a concurrent identical Start superseded another: {tokens:?}"
            );
        }
    }
}

/// A token is bound to the supervisor that issued it: another supervisor (another service, or
/// a replacement for the same one) never answers for it.
#[test]
fn a_token_is_only_answered_by_the_supervisor_that_issued_it() {
    let a = Supervisor::start(missing_bin_config("token-a", 0)).expect("start a");
    let b = Supervisor::start(missing_bin_config("token-b", 0)).expect("start b");
    a.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    b.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    let ta = match a.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("{other:?}"),
    };
    let tb = match b.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("{other:?}"),
    };
    assert!(matches!(wait_receipt(&a, ta), Receipt::Complete { .. }));
    assert!(matches!(wait_receipt(&b, tb), Receipt::Complete { .. }));
    assert_eq!(b.receipt(ta), Receipt::Expired, "B never answers for A's token");
    assert_eq!(a.receipt(tb), Receipt::Expired, "A never answers for B's token");
    drop(a);
    let a2 = Supervisor::start(missing_bin_config("token-a", 0)).expect("start a2");
    a2.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    let _ = a2.request_stop();
    assert_eq!(a2.receipt(ta), Receipt::Expired, "a replacement never answers for the old one");
}

/// The source text of the function whose signature starts with `sig`, up to its closing brace
/// at `indent` (a structural tripwire helper; the module source is `include_str!`-ed).
fn fn_source<'a>(src: &'a str, sig: &str, indent: &str) -> &'a str {
    let start = src
        .find(sig)
        .unwrap_or_else(|| panic!("tripwire: `{sig}` not found in supervisor.rs"));
    let close = format!("\n{indent}}}\n");
    let len = src[start..]
        .find(&close)
        .unwrap_or_else(|| panic!("tripwire: no closing brace for `{sig}`"));
    &src[start..start + len]
}

/// Tripwire (core#398): a running service is only "already satisfied" while a monitor
/// supervises it, and the monitor marks the cell when it ends abnormally. Every
/// `AlreadySatisfied` that `request_start` returns sits behind a `monitor_running()` check, and
/// `run_monitor` installs its exit guard before its loop, so a new early return or a new
/// satisfied answer cannot quietly drop either.
#[test]
fn already_satisfied_start_requires_a_running_monitor_source_check() {
    let src = include_str!("supervisor.rs");
    let start = fn_source(src, "pub fn request_start(&self)", "    ");
    let lines: Vec<&str> = start.lines().collect();
    let mut found = 0;
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("ControlResult::AlreadySatisfied") {
            continue;
        }
        found += 1;
        // The guarding `if` is the nearest preceding line that opens a condition.
        let guard = lines[..i]
            .iter()
            .rev()
            .find(|l| l.trim_start().starts_with("if ") || l.trim_start().starts_with("&&"))
            .unwrap_or_else(|| panic!("tripwire: AlreadySatisfied at line {i} has no guard"));
        let cond: String = lines[..i]
            .iter()
            .skip_while(|l| *l != guard)
            .copied()
            .collect();
        assert!(
            cond.contains("self.monitor_running()"),
            "request_start answers AlreadySatisfied without checking the monitor: {cond}"
        );
    }
    assert!(found > 0, "tripwire: request_start has no AlreadySatisfied answer to check");
    let monitor = fn_source(src, "fn run_monitor(", "");
    let guard_at = monitor
        .find("MonitorExit(")
        .unwrap_or_else(|| panic!("tripwire: run_monitor installs no MonitorExit guard"));
    let loop_at = monitor
        .find("\n    loop {")
        .unwrap_or_else(|| panic!("tripwire: run_monitor has no main loop"));
    assert!(
        guard_at < loop_at,
        "run_monitor must install its exit guard before the loop"
    );
}
