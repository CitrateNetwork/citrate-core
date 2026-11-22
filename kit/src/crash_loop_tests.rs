// Startup crash-loop detection (0.5.7 fd-limit lane). Pure: no processes, injected times.
use super::*;

/// The line the 0.5.7 node printed on every start on the owner's Mac (path shortened).
const EMFILE: &str = "Error: IO error: While open a file for random read: /data/node/003500.sst: Too many open files";

#[test]
fn classify_finds_the_emfile_line_among_others() {
    let tail = format!("INFO starting\n{EMFILE}\nINFO shutting down\n");
    let e = classify(&tail, "exit code: 1");
    assert_eq!(e.kind, CrashKind::TooManyOpenFiles);
    assert_eq!(e.line, EMFILE);
    assert_eq!(e.class, "too-many-open-files");
    // The raw errno form counts too.
    assert_eq!(
        classify("open failed: (os error 24)", "exit code: 1").kind,
        CrashKind::TooManyOpenFiles
    );
}

#[test]
fn classify_reads_the_latest_report_only() {
    // anyhow's report shape: the EMFILE cause sits under the headline.
    let chained = "Error: failed to open the chain database\nCaused by:\n    IO error: While open a file: 7.sst: Too many open files";
    let e = classify(chained, "exit code: 1");
    assert_eq!(e.kind, CrashKind::TooManyOpenFiles);
    assert!(e.line.contains("Too many open files"));
    // An earlier incarnation's EMFILE line, still in the ring, does not decide a later crash.
    let later = format!("{EMFILE}\nINFO restarting\nError: genesis mismatch");
    let e = classify(&later, "exit code: 1");
    assert_eq!(e.kind, CrashKind::Other);
    assert_eq!(e.line, "Error: genesis mismatch");
}

#[test]
fn classify_other_errors_fold_digits_and_fall_back_to_the_exit() {
    let a = classify("INFO hi\nError: bind 127.0.0.1:8545 failed\nINFO bye", "exit code: 1");
    let b = classify("Error: bind 127.0.0.1:9545 failed", "exit code: 1");
    assert_eq!(a.kind, CrashKind::Other);
    assert_eq!(a.line, "Error: bind 127.0.0.1:8545 failed");
    assert_eq!(a.class, b.class, "a port number does not split the class");
    // No stderr at all: the exit status is the line.
    let c = classify("", "signal: 9");
    assert_eq!(c.line, "signal: 9");
    // A long line is bounded on a char boundary.
    let long = "é".repeat(CRASH_LINE_MAX_BYTES);
    assert!(classify(&long, "").line.len() <= CRASH_LINE_MAX_BYTES);
}

#[test]
fn three_same_crashes_in_a_row_are_a_loop_two_are_not() {
    let mut h = CrashHistory::default();
    h.record(1_000, EMFILE, "exit code: 1");
    h.record(31_000, EMFILE, "exit code: 1");
    assert_eq!(h.crash_loop(), None);
    h.record(61_000, EMFILE, "exit code: 1");
    let l = h.crash_loop().expect("three EMFILE exits are a loop");
    assert_eq!(l.kind, CrashKind::TooManyOpenFiles);
    assert_eq!(l.line, EMFILE);
    assert_eq!(l.count, 3);
    assert_eq!(l.first_at_unix_ms, 1_000);
    assert_eq!(l.last_at_unix_ms, 61_000);
}

#[test]
fn another_error_or_a_long_gap_breaks_the_run() {
    let mut h = CrashHistory::default();
    h.record(1_000, EMFILE, "exit code: 1");
    h.record(2_000, EMFILE, "exit code: 1");
    h.record(3_000, "Error: something else", "exit code: 1");
    h.record(4_000, EMFILE, "exit code: 1");
    assert_eq!(h.crash_loop(), None, "the run is broken by another class");

    let mut g = CrashHistory::default();
    g.record(0, EMFILE, "exit code: 1");
    g.record(1, EMFILE, "exit code: 1");
    g.record(2 + CRASH_LOOP_MIN_GAP_MS, EMFILE, "exit code: 1");
    assert_eq!(g.crash_loop(), None, "a gap over the limit breaks it");
}

/// Review P1-1: only the supervisor's health credit (or Ready) ends a loop, never raw uptime.
/// A child that is up but not yet credited still shows the loop; once credited it is gone.
#[test]
fn a_loop_ends_on_recovery_not_on_uptime() {
    let mut h = CrashHistory::default();
    for t in [0, 30_000, 60_000] {
        h.record(t, EMFILE, "exit code: 1");
    }
    assert!(h.crash_loop().is_some());
    h.recovered();
    assert_eq!(h.crash_loop(), None, "a credited child ended the loop");
    // The next crash starts a new run: one crash is not a loop.
    h.record(600_000, EMFILE, "exit code: 1");
    assert_eq!(h.crash_loop(), None);
}

/// Review P1-2: the gap is twice the slow retry (and never under the minimum), so a `Failed`
/// service's slow retries, timed from entering `Failed`, stay in the loop.
#[test]
fn the_gap_is_derived_from_the_slow_retry() {
    let ten_min = std::time::Duration::from_secs(600);
    assert_eq!(crash_loop_gap_ms(None), CRASH_LOOP_MIN_GAP_MS);
    assert_eq!(crash_loop_gap_ms(Some(std::time::Duration::from_secs(60))), CRASH_LOOP_MIN_GAP_MS);
    assert_eq!(crash_loop_gap_ms(Some(ten_min)), 2 * 600_000);
    assert!(crash_loop_gap_ms(Some(ten_min)) > 600_000 + 30_000, "clear of one period plus a start");
    assert_eq!(crash_loop_gap_ms(Some(std::time::Duration::MAX)), u64::MAX);
}

/// Review P1-2: 9 backoff crashes (the node's budget: 8 retries, 30 s cap), then the service is
/// Failed and slow-retried every 10 minutes; each slow retry exits with EMFILE again a few
/// seconds after it starts. The loop stays named through all of them.
#[test]
fn backoff_crashes_then_slow_retry_crashes_stay_one_loop() {
    let slow = std::time::Duration::from_secs(600);
    let mut h = CrashHistory::new(crash_loop_gap_ms(Some(slow)));
    let mut t = 0u64;
    for i in 0..9u64 {
        h.record(t, EMFILE, "exit code: 1");
        t += 500u64.saturating_mul(1 << i.min(6)).min(30_000) + 2_000;
    }
    assert_eq!(h.crash_loop().map(|l| l.count), Some(9));
    let failed_at = t;
    for k in 1..=3u64 {
        // The slow retry fires one period after entering Failed (or after the last slow crash),
        // and the node takes a few seconds to open the database and exit.
        h.record(failed_at + k * (600_000 + 5_000), EMFILE, "exit code: 1");
        let l = h.crash_loop().expect("still one loop after a slow retry");
        assert_eq!(l.count, 9 + k as u32);
        assert_eq!(l.kind, CrashKind::TooManyOpenFiles);
    }
}

/// Review P3: a run that printed no error line (killed by a signal) is named by its exit status,
/// never by an earlier run's line. The caller passes only the crashed run's own stderr.
#[test]
fn a_run_with_no_error_line_is_named_by_its_exit() {
    let mut h = CrashHistory::default();
    h.record(0, EMFILE, "exit code: 1");
    h.record(1_000, EMFILE, "exit code: 1");
    h.record(2_000, "", "signal: 9");
    assert_eq!(h.crash_loop(), None, "the signal kill is its own class");
    let e = classify("", "signal: 9");
    assert_eq!(e.kind, CrashKind::Other);
    assert_eq!(e.line, "signal: 9");
}

#[test]
fn the_history_is_bounded() {
    let mut h = CrashHistory::default();
    for t in 0..(CRASH_HISTORY_CAPACITY as u64 * 3) {
        h.record(t * 1_000, EMFILE, "exit code: 1");
    }
    let l = h.crash_loop().expect("loop");
    assert_eq!(l.count as usize, CRASH_HISTORY_CAPACITY);
}
