// citrate-core — SidecarSupervisor lifecycle cell tests (SCL-S2, SC1). Platform-neutral: they
// run on Unix and Windows. The process fault fixtures for SC1 live in the integration suite
// `kit/tests/lifecycle_faults.rs`.

/// A service whose binary does not exist: every spawn fails at once, on every OS.
fn missing_bin_config(tag: &str, max_retries: u32) -> SupervisorConfig {
    let dir =
        std::env::temp_dir().join(format!("citrate-core-sup-lc-{tag}-{}", std::process::id()));
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
        open_tickets: 2,
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
    assert_eq!(
        sup.status().restarts,
        2,
        "one more attempt under a fresh budget"
    );

    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop on a Failed service: {other:?}"),
    };
    assert_eq!(stop.intent_sequence, 3);
    match wait_receipt(&sup, stop) {
        Receipt::Complete { scope } => {
            assert!(
                !scope.contains(&"child".to_string()),
                "there was no child: {scope:?}"
            )
        }
        other => panic!("{other:?}"),
    }
    let snap = sup.lifecycle().expect("snapshot");
    assert_eq!(snap.phase, Phase::Stopped);
    assert_eq!(snap.desired, Desired::Stopped);
    assert_eq!(sup.status().state, SupervisorState::Off);
    assert!(
        snap.revision > rev_before,
        "every change bumps the revision"
    );
    assert!(matches!(
        sup.request_stop(),
        ControlResult::AlreadySatisfied(_)
    ));
    assert_eq!(
        sup.status().restarts,
        2,
        "Stop never consumes failure budget"
    );
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
    assert!(
        !p.arm(7, 1, 99, Duration::from_secs(60)),
        "no record beyond the overlay"
    );
    p.release(7, 1, 3);
    assert_eq!(p.armed_records(), DEADLINE_RECORDS - 1);
    assert!(
        p.arm(7, 1, 99, Duration::from_secs(60)),
        "a freed record is reused"
    );
    assert_eq!(
        p.expired(7, 1, 3),
        Some(false),
        "a released control has no record"
    );
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
        instance: 1,
        intent_sequence: 1,
    };
    assert_eq!(
        p.record_state(1, token),
        Some(DeadlineState::Armed),
        "never marked"
    );
    assert_eq!(
        p.expired(1, 1, 1),
        Some(true),
        "but the read sees the deadline passed"
    );
}

#[test]
fn a_stale_expiry_mark_never_applies_to_a_rewritten_record() {
    let p = idle_publisher();
    assert!(p.arm(1, 1, 1, Duration::ZERO));
    p.observe(); // marks the expired record
    let a = ControlToken {
        app_epoch: 1,
        instance: 1,
        intent_sequence: 1,
    };
    assert_eq!(p.record_state(1, a), Some(DeadlineState::Expired));
    p.release(1, 1, 1);
    assert!(
        p.arm(2, 1, 5, Duration::from_secs(60)),
        "the same record, rewritten"
    );
    let b = ControlToken {
        app_epoch: 1,
        instance: 2,
        intent_sequence: 5,
    };
    assert_eq!(p.record_state(2, b), Some(DeadlineState::Armed));
    assert_eq!(
        p.expired(2, 1, 5),
        Some(false),
        "A's mark does not carry over to B"
    );
}

#[test]
fn the_publisher_marks_expiry_within_its_cadence() {
    let p = DeadlinePublisher::start(Duration::from_millis(10));
    assert!(p.is_running() && p.init_error().is_none());
    assert!(p.arm(3, 1, 1, Duration::from_millis(30)));
    let token = ControlToken {
        app_epoch: 1,
        instance: 3,
        intent_sequence: 1,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while p.record_state(3, token) != Some(DeadlineState::Expired) {
        assert!(
            std::time::Instant::now() < deadline,
            "the publisher never marked it"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn joining_a_deadline_only_ever_tightens_it() {
    let p = idle_publisher();
    assert!(
        p.arm_or_tighten(4, 1, 1, Duration::from_secs(60)),
        "arms when absent"
    );
    assert!(
        p.arm_or_tighten(4, 1, 1, Duration::from_secs(3600)),
        "a looser join is accepted"
    );
    assert_eq!(p.expired(4, 1, 1), Some(false));
    assert!(
        p.arm_or_tighten(4, 1, 1, Duration::ZERO),
        "a tighter join moves it earlier"
    );
    assert_eq!(p.expired(4, 1, 1), Some(true));
    assert!(p.arm_or_tighten(4, 1, 1, Duration::from_secs(3600)));
    assert_eq!(p.expired(4, 1, 1), Some(true), "never moved later again");
    assert_eq!(p.armed_records(), 1);
}

/// A hook that reports when a writer reached its pause point and holds it there until released.
fn pause_hook() -> (
    Box<dyn FnOnce() + Send>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let (at_tx, at_rx) = std::sync::mpsc::channel::<()>();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let hook = Box::new(move || {
        let _ = at_tx.send(());
        let _ = go_rx.recv();
    });
    (hook, at_rx, go_tx)
}

/// One owner's release is suspended right after it frees a record; another owner claims and
/// fully arms that record meanwhile. The new record must read as armed (with its own deadline)
/// while the first release is still returning, never as torn.
#[test]
fn a_record_rearmed_while_its_release_is_still_returning_reads_armed() {
    let p = idle_publisher();
    assert!(p.arm(1, 1, 1, Duration::from_secs(60)));
    let (hook, at, go) = pause_hook();
    p.pause_once(PausePoint::ReleaseFreed, hook);
    let releaser = {
        let p = p.clone();
        std::thread::spawn(move || p.release(1, 1, 1))
    };
    if at.recv_timeout(Duration::from_secs(10)).is_err() {
        panic!("INCONCLUSIVE: the release never reached its pause point");
    }
    // The only free record is the one being released: owner 2 claims and arms it.
    let armed = p.arm(2, 1, 7, Duration::from_secs(60));
    let b = ControlToken {
        app_epoch: 1,
        instance: 2,
        intent_sequence: 7,
    };
    let state = p.record_state(2, b);
    let expired = p.expired(2, 1, 7);
    let _ = go.send(());
    let _ = releaser.join();
    assert!(armed, "the freed record was claimed");
    assert_eq!(
        state,
        Some(DeadlineState::Armed),
        "a fully armed record reads as armed while the previous release returns"
    );
    assert_eq!(expired, Some(false), "and its expiry is determinate");
    assert_eq!(p.record_state(2, b), Some(DeadlineState::Armed));
    assert_eq!(p.armed_records(), 1);
}

/// A settlement whose expiry read stays indeterminate (a record mid-write through every bounded
/// retry) counts the control as expired: a late completion is never stored as Complete. The cell
/// asks this only for a control that armed its own record (see
/// `a_plain_control_never_reads_another_owners_torn_record_as_expiry`).
#[test]
fn an_indeterminate_expiry_at_settlement_counts_as_expired() {
    let p = idle_publisher();
    let (hook, at, go) = pause_hook();
    p.pause_once(PausePoint::ArmWriting, hook);
    let armer = {
        let p = p.clone();
        std::thread::spawn(move || p.arm(3, 1, 1, Duration::from_secs(60)))
    };
    if at.recv_timeout(Duration::from_secs(10)).is_err() {
        panic!("INCONCLUSIVE: the arm never reached its pause point");
    }
    let read = p.expired(4, 1, 2);
    let at_settle = p.expired_at_settle(4, 1, 2);
    let _ = go.send(());
    let _ = armer.join();
    assert_eq!(
        read, None,
        "INCONCLUSIVE: the paused write did not make the read torn"
    );
    assert!(
        at_settle,
        "an indeterminate expiry at settlement counts as expired"
    );
    assert!(
        !p.expired_at_settle(4, 1, 2),
        "once readable, a control without a record has not expired"
    );
}

/// Controls that never armed a deadline settle on what was observed, even while another owner's
/// record stays mid-write in the shared publisher for the whole settlement. Only a control with
/// a record of its own can be `DeadlineExpired`; an unrelated torn record must not fabricate one,
/// neither at settlement (the Stop) nor at supersession (the Start the Stop replaced).
#[test]
fn a_plain_control_never_reads_another_owners_torn_record_as_expiry() {
    let publisher = idle_publisher();
    let mut cfg = missing_bin_config("torn-other", 0);
    cfg.deadlines = Some(publisher.clone());
    let sup = Supervisor::start(cfg).expect("start");
    sup.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));

    // Another supervisor (owner 900 001) is descheduled in the middle of arming its record.
    let (hook, at, go) = pause_hook();
    publisher.pause_once(PausePoint::ArmWriting, hook);
    let armer = {
        let p = publisher.clone();
        std::thread::spawn(move || p.arm(900_001, 1, 1, Duration::from_secs(60)))
    };
    if at.recv_timeout(Duration::from_secs(10)).is_err() {
        panic!("INCONCLUSIVE: the other owner's arm never reached its pause point");
    }
    assert_eq!(
        publisher.expired(900_002, 1, 1),
        None,
        "INCONCLUSIVE: the paused write did not make reads torn"
    );

    let start = match sup.request_start() {
        ControlResult::Accepted(t) => t,
        other => panic!("Start after Failed: {other:?}"),
    };
    let stop = match sup.request_stop() {
        ControlResult::Accepted(t) => t,
        other => panic!("Stop: {other:?}"),
    };
    // Answered while the other owner's record is still mid-write.
    let start_receipt = wait_receipt(&sup, start);
    let stop_receipt = wait_receipt(&sup, stop);
    let _ = go.send(());
    let _ = armer.join();
    // What was stored at settlement, read once that write has finished.
    let stored_start = sup.receipt(start);
    let stored_stop = sup.receipt(stop);

    assert!(
        matches!(stored_stop, Receipt::Complete { .. }),
        "a plain Stop is stored as what was observed, not as another owner's torn record: \
         {stored_stop:?}"
    );
    assert!(
        matches!(
            stored_start,
            Receipt::Superseded | Receipt::Failed { .. } | Receipt::Expired
        ),
        "a Start without a deadline is never stored DeadlineExpired: {stored_start:?}"
    );
    assert!(
        matches!(stop_receipt, Receipt::Complete { .. }),
        "the receipt of a control without a record answers while another owner writes: \
         {stop_receipt:?}"
    );
    assert!(
        matches!(
            start_receipt,
            Receipt::Superseded | Receipt::Failed { .. } | Receipt::Expired
        ),
        "{start_receipt:?}"
    );
}

/// Tripwire: the cell asks the deadline overlay whether a control expired only for the control
/// that armed its own record. Fails on an `expired_at_settle` call in the supervisor that is not
/// guarded by `had_deadline` in the same expression.
#[test]
fn settlement_reads_expiry_only_for_a_control_that_armed_a_deadline() {
    let source = include_str!("supervisor.rs");
    let mut calls = 0;
    for (at, _) in source.match_indices(".expired_at_settle(") {
        calls += 1;
        let stmt_start = source[..at].rfind(';').map_or(0, |i| i + 1);
        assert!(
            source[stmt_start..at].contains("had_deadline("),
            "an expiry read at settlement without the had_deadline guard: {}",
            &source[stmt_start..(at + 40).min(source.len())]
        );
    }
    assert!(calls >= 2, "settle and commit both read expiry");
}

/// Tripwire: a deadline record is published free only after its writer has finished its
/// version update, so the next owner's `arm` is always the record's only writer. Fails on any
/// `REC_FREE` store that is followed by a version bump in the same block.
#[test]
fn deadline_records_are_published_free_only_after_their_last_version_bump() {
    let source = include_str!("supervisor_deadline.rs");
    let mut stores = 0;
    for (at, _) in source.match_indices("state.store(REC_FREE") {
        stores += 1;
        let rest = &source[at..];
        let block_end = rest.find("\n            }").unwrap_or(rest.len());
        assert!(
            !rest[..block_end].contains("version.fetch_add"),
            "a record is published free before its version update finished: {}",
            &rest[..block_end.min(200)]
        );
    }
    assert!(stores >= 1, "the release path publishes REC_FREE");
}

/// Tripwire: a publication ticket is resolved only by an explicit call (`commit`, `withdraw`,
/// `cleaned`). A `Drop` that releases one would let an unwinding caller report cleanup that never
/// happened, so neither ticket type may implement `Drop`.
#[test]
fn publication_tickets_never_release_on_drop() {
    let source = include_str!("supervisor.rs");
    for ty in ["PublicationTicket", "StaleTicket"] {
        assert!(
            !source.contains(&format!("impl Drop for {ty}")),
            "{ty} must not release its publication on Drop"
        );
    }
}

/// Tripwire: `PublicationTicket::commit` runs the caller's activation step inside the same
/// critical section as the currency check. The step must run before the lock is dropped and
/// before the publication is resolved; a commit that returns first and leaves activation to the
/// caller reopens the window in which a Stop settles while a file becomes active.
#[test]
fn publication_commit_activates_inside_the_critical_section() {
    let source = include_str!("supervisor.rs");
    let start = source
        .find("    pub fn commit<T, E>(")
        .expect("PublicationTicket::commit takes an activation step");
    let body = &source[start..];
    let end = body.find("\n    }\n").expect("commit body ends");
    let body = &body[..end];
    let lock = body
        .find("self.shared.lock()")
        .expect("commit takes the cell lock");
    let activate = body
        .find("activate()")
        .expect("commit runs the activation step");
    let resolve = body
        .find("resolve_publication(")
        .expect("commit resolves the publication");
    assert!(lock < activate, "activation runs under the cell lock");
    assert!(
        activate < resolve,
        "the publication resolves only after activation"
    );
    let between = &body[lock..activate];
    assert!(
        !between.contains("return Ok"),
        "commit must not return success before activating"
    );
    for (at, _) in between.match_indices("drop(c)") {
        let after = &between[at..];
        let next = after.find(';').map_or(after, |i| &after[i + 1..]);
        assert!(
            next.trim_start()
                .starts_with("return Err(CommitRefused::Stale"),
            "the lock is dropped before activation only on the stale path"
        );
    }
}

#[test]
fn releasing_an_owner_frees_only_its_records() {
    let p = idle_publisher();
    assert!(p.arm(5, 1, 1, Duration::from_secs(60)));
    assert!(p.arm(5, 1, 2, Duration::from_secs(60)));
    assert!(p.arm(6, 1, 1, Duration::from_secs(60)));
    p.release_owner(5);
    assert_eq!(p.armed_records(), 1);
    let other = ControlToken {
        app_epoch: 1,
        instance: 6,
        intent_sequence: 1,
    };
    assert_eq!(p.record_state(6, other), Some(DeadlineState::Armed));
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
    assert_eq!(
        b.receipt(ta),
        Receipt::Expired,
        "B never answers for A's token"
    );
    assert_eq!(
        a.receipt(tb),
        Receipt::Expired,
        "A never answers for B's token"
    );
    drop(a);
    let a2 = Supervisor::start(missing_bin_config("token-a", 0)).expect("start a2");
    a2.wait_until(|s| *s == SupervisorState::Failed, Duration::from_secs(5));
    let _ = a2.request_stop();
    assert_eq!(
        a2.receipt(ta),
        Receipt::Expired,
        "a replacement never answers for the old one"
    );
}

/// Tripwire (core#397): every spawn, automatic retries included, is preceded by the full reader
/// reconcile. `run_monitor` has exactly one spawn admission, and the reconcile
/// (`drain_readers(&mut readers, READER_DRAIN)`) runs at loop level on the path to it, not
/// inside a branch that only some spawns take (a "first spawn for a new intent" condition was
/// the defect).
#[test]
fn every_spawn_reconciles_held_readers_first_source_check() {
    let src = include_str!("supervisor.rs");
    let start = src
        .find("fn run_monitor(")
        .unwrap_or_else(|| panic!("tripwire: run_monitor not found"));
    let body = &src[start..];
    let end = body
        .find("\n}\n")
        .unwrap_or_else(|| panic!("tripwire: run_monitor has no end"));
    let body = &body[..end];
    assert_eq!(
        body.matches("admit_spawn(").count(),
        1,
        "run_monitor must have exactly one spawn admission"
    );
    assert_eq!(
        src.matches("spawn_child(&").count(),
        1,
        "the supervisor must have exactly one spawn call"
    );
    let admit = body
        .find("admit_spawn(")
        .unwrap_or_else(|| panic!("tripwire: no spawn admission"));
    let running = body
        .find("// desired == Running")
        .unwrap_or_else(|| panic!("tripwire: no Running branch marker"));
    let path = &body[running..admit];
    let reconcile = path
        .lines()
        .find(|l| l.contains("drain_readers(&mut readers, READER_DRAIN)"))
        .unwrap_or_else(|| panic!("tripwire: no reader reconcile before the spawn admission"));
    let indent = reconcile.len() - reconcile.trim_start().len();
    assert_eq!(
        indent, 8,
        "the reader reconcile must run at loop level, for every spawn: {reconcile:?}"
    );
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
    assert!(
        found > 0,
        "tripwire: request_start has no AlreadySatisfied answer to check"
    );
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
