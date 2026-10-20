// Tests for the persisted record of submitted signed transactions (UI truth contract v1.1 §1).

use super::*;

struct Tmp(std::path::PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "citrate-submitted-log-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).expect("tmp");
        Tmp(p)
    }
    fn file(&self) -> std::path::PathBuf {
        self.0.join("submitted-tx.json")
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn h(n: u32) -> String {
    format!("0x{:064x}", n)
}

#[test]
fn the_first_record_of_a_hash_is_first_and_a_second_is_not() {
    let log = SubmittedTxLog::in_memory();
    assert!(log.record(&h(1)));
    assert!(!log.record(&h(1)));
    assert!(log.record(&h(2)));
}

#[test]
fn hashes_are_compared_case_insensitively() {
    let log = SubmittedTxLog::in_memory();
    assert!(log.record(&format!("0x{}", "AB".repeat(32))));
    assert!(!log.record(&format!("0x{}", "ab".repeat(32))));
}

#[test]
fn the_record_survives_a_restart() {
    let t = Tmp::new("restart");
    let log = SubmittedTxLog::open(t.file());
    assert!(log.record(&h(7)));
    drop(log);
    let again = SubmittedTxLog::open(t.file());
    assert!(!again.record(&h(7)), "a re-send after a restart is not a first submission");
    assert!(again.record(&h(8)));
}

// Hashes recorded without a sender/nonce: an eviction can't be bounded, so the history is uncertain.
#[test]
fn the_record_keeps_the_last_256_and_then_treats_absent_hashes_as_unknown() {
    let t = Tmp::new("bound");
    let log = SubmittedTxLog::open(t.file());
    for n in 0..300 {
        log.record(&h(n));
    }
    assert_eq!(log.len(), CAPACITY);
    assert_eq!(CAPACITY, 256);
    assert!(log.is_uncertain(), "history was truncated");
    drop(log);
    let again = SubmittedTxLog::open(t.file());
    assert_eq!(again.len(), 256);
    assert!(again.is_uncertain(), "the truncation survives a restart");
    assert!(!again.record(&h(299)), "the newest are kept");
    assert!(!again.record(&h(0)), "an evicted hash is not a first send");
    assert!(!again.record(&h(1000)), "after truncation no absent hash is a first send");
}

#[test]
fn the_write_is_atomic_and_leaves_no_temp_file() {
    let t = Tmp::new("atomic");
    let log = SubmittedTxLog::open(t.file());
    log.record(&h(1));
    let names: Vec<String> = std::fs::read_dir(&t.0)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["submitted-tx.json".to_string()]);
    let body: serde_json::Value = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(body["hashes"], serde_json::json!([h(1)]));
    assert_eq!(body["uncertain"], false);
}

#[test]
fn an_unreadable_record_treats_every_send_as_a_resubmission() {
    // A corrupt file means the app can't prove these bytes were never sent: never "not sent".
    let t = Tmp::new("corrupt");
    std::fs::write(t.file(), b"{not json").expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert!(log.is_uncertain());
    assert!(!log.record(&h(1)));
    assert!(!log.record(&h(2)));
}

#[test]
fn a_missing_file_is_an_empty_record() {
    let t = Tmp::new("missing");
    let log = SubmittedTxLog::open(t.file());
    assert!(!log.is_uncertain());
    assert!(log.record(&h(1)));
}

// ---- STD audit parity: concurrency, crash windows, fail-closed ----

#[test]
fn concurrent_records_of_the_same_hash_yield_exactly_one_first() {
    let t = Tmp::new("race");
    let log = std::sync::Arc::new(SubmittedTxLog::open(t.file()));
    let firsts: usize = (0..16)
        .map(|_| {
            let l = log.clone();
            std::thread::spawn(move || l.record(&h(42)))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|j| usize::from(j.join().expect("thread")))
        .sum();
    assert_eq!(firsts, 1, "two concurrent sends of the same bytes: only one is a first submission");
    drop(log);
    assert!(!SubmittedTxLog::open(t.file()).record(&h(42)));
}

#[test]
fn concurrent_records_of_different_hashes_are_all_kept() {
    let t = Tmp::new("race-many");
    let log = std::sync::Arc::new(SubmittedTxLog::open(t.file()));
    let joins: Vec<_> = (0..32)
        .map(|n| {
            let l = log.clone();
            std::thread::spawn(move || l.record(&h(n)))
        })
        .collect();
    assert!(joins.into_iter().all(|j| j.join().expect("thread")));
    drop(log);
    let again = SubmittedTxLog::open(t.file());
    assert_eq!(again.len(), 32, "every write landed whole; none lost to interleaving");
}

#[test]
fn a_temp_file_left_by_a_crash_does_not_affect_the_record() {
    let t = Tmp::new("crash-tmp");
    let log = SubmittedTxLog::open(t.file());
    log.record(&h(1));
    drop(log);
    // A crash between writing the temp file and the rename leaves the old file intact plus a temp.
    std::fs::write(t.0.join(".submitted-tx.json.tmp-99999"), b"[\"0xpartial").expect("tmp");
    let again = SubmittedTxLog::open(t.file());
    assert!(!again.is_uncertain());
    assert!(!again.record(&h(1)));
}

#[test]
fn a_truncated_record_is_degraded_and_never_proves_a_first_send() {
    let t = Tmp::new("truncated");
    std::fs::write(t.file(), format!("[\"{}\",\"0x", h(1))).expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert!(log.is_uncertain());
    assert!(!log.record(&h(9)));
}

#[test]
fn an_oversized_list_on_disk_is_trimmed_and_marked_uncertain() {
    let t = Tmp::new("oversized");
    let list: Vec<String> = (0..400).map(h).collect();
    std::fs::write(t.file(), serde_json::to_vec(&list).expect("json")).expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert_eq!(log.len(), 256);
    assert!(log.is_uncertain());
    assert!(!log.record(&h(399)));
    assert!(!log.record(&h(0)), "trimmed history can't prove a first send");
}

#[test]
fn a_legacy_full_list_is_treated_as_possibly_truncated() {
    // Before the uncertain flag, a list at capacity may already have evicted hashes.
    let t = Tmp::new("legacy-full");
    let list: Vec<String> = (0..256).map(h).collect();
    std::fs::write(t.file(), serde_json::to_vec(&list).expect("json")).expect("write");
    assert!(SubmittedTxLog::open(t.file()).is_uncertain());
}

#[test]
fn a_legacy_short_list_loads_as_complete_history() {
    let t = Tmp::new("legacy-short");
    std::fs::write(t.file(), serde_json::to_vec(&vec![h(1)]).expect("json")).expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert!(!log.is_uncertain());
    assert!(!log.record(&h(1)));
    assert!(log.record(&h(2)));
}

#[test]
fn a_corrupt_record_stays_uncertain_after_it_is_rewritten_and_reopened() {
    let t = Tmp::new("corrupt-reopen");
    std::fs::write(t.file(), b"{not json").expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert!(!log.record(&h(1)));
    drop(log);
    let again = SubmittedTxLog::open(t.file());
    assert!(again.is_uncertain(), "history lost to corruption is never treated as complete");
    assert!(!again.record(&h(2)));
}

#[test]
fn a_record_at_an_unreadable_location_fails_closed() {
    // The path's parent is a regular file: the record can't be read, so no send is a first.
    let t = Tmp::new("unreadable");
    let blocker = t.0.join("blocker");
    std::fs::write(&blocker, b"x").expect("blocker");
    let log = SubmittedTxLog::open(blocker.join("submitted-tx.json"));
    assert!(log.is_uncertain());
    assert!(!log.record(&h(5)));
}

#[test]
fn a_record_that_cannot_be_made_durable_is_refused_and_not_kept() {
    let t = Tmp::new("rewrite-fails");
    let log = SubmittedTxLog::open(t.file());
    assert_eq!(log.try_record(&h(1)), Ok(true));
    drop(log);
    // The rewrite's temp path is taken by a directory, so the next write fails.
    let block = t.0.join(format!(".submitted-tx.json.tmp-{}", std::process::id()));
    std::fs::create_dir(&block).expect("block tmp");
    let log = SubmittedTxLog::open(t.file());
    assert!(log.try_record(&h(2)).is_err(), "no durable record, so the caller must not send");
    std::fs::remove_dir(&block).expect("unblock");
    // Nothing was sent, so the refused hash is not history.
    assert_eq!(log.try_record(&h(2)), Ok(true));
    let on_disk: serde_json::Value = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(on_disk["hashes"], serde_json::json!([h(1), h(2)]));
}

// ---- Eviction keeps "not sent" possible: a per-sender nonce high-water mark ----

const A: &str = "0x00000000000000000000000000000000000000aa";
const B: &str = "0x00000000000000000000000000000000000000bb";

/// Fill the record with CAPACITY + 1 submissions from A at nonces 0..=CAPACITY, evicting A's nonce 0.
fn evict_a_nonce_0(log: &SubmittedTxLog) {
    for n in 0..=(CAPACITY as u32) {
        assert_eq!(log.try_record_for(&h(n), A, u64::from(n)), Ok(true));
    }
}

#[test]
fn after_eviction_a_resend_at_an_evicted_nonce_is_unknown() {
    let log = SubmittedTxLog::in_memory();
    evict_a_nonce_0(&log);
    // Different bytes at A's evicted nonce 0 (e.g. a re-signed copy): can't prove it was never sent.
    assert_eq!(log.try_record_for(&h(10_000), A, 0), Ok(false));
}

#[test]
fn after_eviction_a_new_tx_at_a_higher_nonce_from_the_same_sender_is_a_first_send() {
    let log = SubmittedTxLog::in_memory();
    evict_a_nonce_0(&log);
    assert_eq!(log.try_record_for(&h(10_001), A, 1), Ok(true), "nonce 1 is above everything evicted");
    assert_eq!(log.try_record_for(&h(10_002), A, 5_000), Ok(true));
}

#[test]
fn after_eviction_another_sender_is_unaffected() {
    let log = SubmittedTxLog::in_memory();
    evict_a_nonce_0(&log);
    assert_eq!(log.try_record_for(&h(10_003), B, 0), Ok(true));
}

#[test]
fn the_nonce_high_water_mark_survives_a_restart() {
    let t = Tmp::new("hwm-restart");
    let log = SubmittedTxLog::open(t.file());
    evict_a_nonce_0(&log);
    drop(log);
    let again = SubmittedTxLog::open(t.file());
    assert!(!again.is_uncertain(), "eviction with known nonces keeps the history usable");
    assert_eq!(again.try_record_for(&h(20_000), A, 0), Ok(false));
    assert_eq!(again.try_record_for(&h(20_001), A, 300), Ok(true));
    assert_eq!(again.try_record_for(&h(20_002), B, 0), Ok(true));
}

#[test]
fn evicting_an_entry_with_no_sender_or_nonce_leaves_the_whole_history_uncertain() {
    let log = SubmittedTxLog::in_memory();
    assert_eq!(log.try_record(&h(1)), Ok(true));
    for n in 0..(CAPACITY as u32) {
        log.try_record_for(&h(100 + n), A, u64::from(n)).expect("record");
    }
    assert!(log.is_uncertain(), "an evicted entry without a nonce can't be bounded");
    assert_eq!(log.try_record_for(&h(9_999), B, 0), Ok(false));
}
