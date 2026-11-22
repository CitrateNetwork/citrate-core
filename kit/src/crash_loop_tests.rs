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
    let mut h = CrashHistory::new();
    h.record(1_000, EMFILE, "exit code: 1");
    h.record(31_000, EMFILE, "exit code: 1");
    assert_eq!(h.crash_loop(31_500, false), None);
    h.record(61_000, EMFILE, "exit code: 1");
    let l = h.crash_loop(61_500, false).expect("three EMFILE exits are a loop");
    assert_eq!(l.kind, CrashKind::TooManyOpenFiles);
    assert_eq!(l.line, EMFILE);
    assert_eq!(l.count, 3);
    assert_eq!(l.first_at_unix_ms, 1_000);
    assert_eq!(l.last_at_unix_ms, 61_000);
}

#[test]
fn another_error_or_a_long_gap_breaks_the_run() {
    let mut h = CrashHistory::new();
    h.record(1_000, EMFILE, "exit code: 1");
    h.record(2_000, EMFILE, "exit code: 1");
    h.record(3_000, "Error: something else", "exit code: 1");
    h.record(4_000, EMFILE, "exit code: 1");
    assert_eq!(h.crash_loop(4_000, false), None, "the run is broken by another class");

    let mut g = CrashHistory::new();
    g.record(0, EMFILE, "exit code: 1");
    g.record(1, EMFILE, "exit code: 1");
    g.record(2 + CRASH_LOOP_GAP_MS, EMFILE, "exit code: 1");
    assert_eq!(g.crash_loop(3 + CRASH_LOOP_GAP_MS, false), None, "a gap over the limit breaks it");
}

#[test]
fn a_child_up_past_the_gap_ends_the_loop_but_failed_keeps_it() {
    let mut h = CrashHistory::new();
    for t in [0, 30_000, 60_000] {
        h.record(t, EMFILE, "exit code: 1");
    }
    let later = 60_000 + CRASH_LOOP_GAP_MS + 1;
    assert!(h.crash_loop(later, false).is_some(), "no child running: still the loop");
    assert_eq!(h.crash_loop(later, true), None, "a child up past the gap ended it");
    assert!(h.crash_loop(61_000, true).is_some(), "a child up for a second has not");
}

#[test]
fn the_history_is_bounded() {
    let mut h = CrashHistory::new();
    for t in 0..(CRASH_HISTORY_CAPACITY as u64 * 3) {
        h.record(t * 1_000, EMFILE, "exit code: 1");
    }
    let l = h.crash_loop(u64::MAX / 2, false).expect("loop");
    assert_eq!(l.count as usize, CRASH_HISTORY_CAPACITY);
}
