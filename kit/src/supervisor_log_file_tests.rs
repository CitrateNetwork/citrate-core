// citrate-core — CORE-D5: a supervised sidecar's output is kept on disk as well as in the ring.
// Unix only: the children are /bin/sh scripts (helpers from supervisor_tests.rs).

fn wait_for_file(path: &std::path::Path, pred: impl Fn(&str) -> bool, timeout: Duration) -> String {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if pred(&text) || std::time::Instant::now() >= deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn log_config(tmp: &TmpDir, filter: Option<crate::sidecar_log::LineFilter>) -> crate::sidecar_log::LogFileConfig {
    crate::sidecar_log::LogFileConfig {
        path: tmp.path("logs").join("printer.log"),
        max_bytes: 1 << 20,
        keep: 2,
        filter,
        queue: 4096,
    }
}

#[test]
fn captured_lines_reach_the_log_file_with_their_stream() {
    let tmp = TmpDir::new("logfile-capture");
    let script = "echo OUT_ON_DISK; echo ERR_ON_DISK 1>&2; exec sleep 60".to_string();
    let spec = SidecarSpec::new("printer", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    cfg.log_file = Some(log_config(&tmp, None));
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let path = tmp.path("logs").join("printer.log");
    let text = wait_for_file(&path, |t| t.contains("OUT_ON_DISK") && t.contains("ERR_ON_DISK"), Duration::from_secs(5));
    assert!(text.lines().any(|l| l.ends_with(" out OUT_ON_DISK")), "the file text is not as expected");
    assert!(text.lines().any(|l| l.ends_with(" err ERR_ON_DISK")), "the file text is not as expected");
    // The ring still has them too.
    let ring = sup.logs();
    assert!(ring.iter().any(|l| l.line == "OUT_ON_DISK"));
    let st = sup.log_file_status().expect("a log file is configured");
    assert_eq!(st.write_failures, 0);
    assert!(st.path.ends_with("printer.log"));
    sup.stop();
}

#[test]
fn the_log_file_survives_a_crash_and_keeps_both_runs() {
    let tmp = TmpDir::new("logfile-restart");
    let script = "echo RUN_$$; exec sleep 60".to_string();
    let spec = SidecarSpec::new("printer", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(20),
        multiplier: 2,
        max_delay: Duration::from_millis(50),
        max_retries: 5,
    };
    cfg.log_file = Some(log_config(&tmp, None));
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let path = tmp.path("logs").join("printer.log");
    let first = wait_for_file(&path, |t| t.contains("RUN_"), Duration::from_secs(5));
    assert!(first.contains("RUN_"), "{first}");
    let st = sup.wait_until(|s| matches!(s, SupervisorState::Running), Duration::from_secs(5));
    signal_pid(st.pid.expect("pid"), libc::SIGKILL);
    let both = wait_for_file(&path, |t| t.matches("RUN_").count() >= 2, Duration::from_secs(6));
    assert!(both.matches("RUN_").count() >= 2, "the second run appends to the same file: {both}");
    sup.stop();
    // After the supervisor is gone the file is still there for the diagnostics bundle.
    assert!(std::fs::read_to_string(&path).unwrap().contains("RUN_"));
}

#[test]
fn the_configured_filter_keeps_a_key_off_the_disk_but_not_out_of_the_ring() {
    let tmp = TmpDir::new("logfile-filter");
    // A vendor-key shape, built so no scanner reads the source as a credential.
    let shape = ["sk-", "THISISAPLANTED", "SHAPE0123"].concat();
    let script = format!("echo note={shape}; exec sleep 60");
    let spec = SidecarSpec::new("printer", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    let filter: crate::sidecar_log::LineFilter = {
        let shape = shape.clone();
        Arc::new(move |l: &str| Some(l.replace(&shape, "<redacted>")))
    };
    cfg.log_file = Some(log_config(&tmp, Some(filter)));
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let path = tmp.path("logs").join("printer.log");
    let text = wait_for_file(&path, |t| t.contains("note="), Duration::from_secs(5));
    assert!(text.contains("note=<redacted>"), "the file text is not as expected");
    assert!(!text.contains("PLANTED"), "the file text is not as expected");
    sup.stop();
}

#[test]
fn without_a_log_file_nothing_is_written() {
    let tmp = TmpDir::new("logfile-none");
    let script = "echo ONLY_IN_RING; exec sleep 60".to_string();
    let spec = SidecarSpec::new("printer", sh_bin(), vec!["-c".into(), script]);
    let cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    assert!(cfg.log_file.is_none(), "the default keeps output in the ring only");
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let logs = wait_for_logs(&sup, |l| l.iter().any(|x| x.line == "ONLY_IN_RING"), Duration::from_secs(5));
    assert!(logs.iter().any(|x| x.line == "ONLY_IN_RING"));
    assert!(sup.log_file_status().is_none());
    assert!(!tmp.path("logs").exists());
    sup.stop();
}

/// #613 review (P2): the disk write runs off the output readers. A log writer that stalls (here a
/// filter that blocks until released, standing in for a stalled disk) never stops capture into the
/// ring, so the child never blocks on a full pipe; lines past the queue are counted as dropped.
#[test]
fn a_stalled_log_writer_never_blocks_capture() {
    let tmp = TmpDir::new("logfile-stall");
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let g = gate.clone();
    let filter: crate::sidecar_log::LineFilter = Arc::new(move |l: &str| {
        let (m, cv) = &*g;
        let mut open = m.lock().unwrap_or_else(|e| e.into_inner());
        while !*open {
            open = cv.wait(open).unwrap_or_else(|e| e.into_inner());
        }
        Some(l.to_string())
    });
    let script = "i=0; while [ $i -lt 300 ]; do echo LINE_$i; i=$((i+1)); done; exec sleep 60".to_string();
    let spec = SidecarSpec::new("printer", sh_bin(), vec!["-c".into(), script]);
    let mut cfg = SupervisorConfig::new(spec, tmp.path("crashes.jsonl"));
    let mut lf = log_config(&tmp, Some(filter));
    lf.queue = 4;
    cfg.log_file = Some(lf);
    let sup = Supervisor::start(cfg).expect("supervisor starts");
    let logs = wait_for_logs(&sup, |l| l.iter().any(|x| x.line == "LINE_299"), Duration::from_secs(10));
    assert!(logs.iter().any(|x| x.line == "LINE_299"), "capture went on while the writer was stalled");
    let st = sup.log_file_status().expect("a log file is configured");
    assert!(st.lines_dropped > 0, "lines past the queue are counted: {st:?}");
    {
        let (m, cv) = &*gate;
        *m.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_all();
    }
    let path = tmp.path("logs").join("printer.log");
    let text = wait_for_file(&path, |t| t.contains("LINE_0"), Duration::from_secs(5));
    assert!(text.contains("LINE_0"), "the queued lines land once the writer moves again");
    sup.stop();
}
