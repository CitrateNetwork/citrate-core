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

#[test]
fn the_record_keeps_the_last_256() {
    let t = Tmp::new("bound");
    let log = SubmittedTxLog::open(t.file());
    for n in 0..300 {
        assert!(log.record(&h(n)));
    }
    assert_eq!(log.len(), CAPACITY);
    assert_eq!(CAPACITY, 256);
    drop(log);
    let again = SubmittedTxLog::open(t.file());
    assert_eq!(again.len(), 256);
    assert!(!again.record(&h(299)), "the newest are kept");
    assert!(again.record(&h(0)), "the oldest were dropped");
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
    let body: Vec<String> = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(body, vec![h(1)]);
}

#[test]
fn an_unreadable_record_treats_every_send_as_a_resubmission() {
    // A corrupt file means the app can't prove these bytes were never sent: never "not sent".
    let t = Tmp::new("corrupt");
    std::fs::write(t.file(), b"{not json").expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert!(log.is_degraded());
    assert!(!log.record(&h(1)));
    assert!(!log.record(&h(2)));
}

#[test]
fn a_missing_file_is_an_empty_record() {
    let t = Tmp::new("missing");
    let log = SubmittedTxLog::open(t.file());
    assert!(!log.is_degraded());
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
    assert!(!again.is_degraded());
    assert!(!again.record(&h(1)));
}

#[test]
fn a_truncated_record_is_degraded_and_never_proves_a_first_send() {
    let t = Tmp::new("truncated");
    std::fs::write(t.file(), format!("[\"{}\",\"0x", h(1))).expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert!(log.is_degraded());
    assert!(!log.record(&h(9)));
}

#[test]
fn an_oversized_list_on_disk_is_trimmed_to_the_newest_256() {
    let t = Tmp::new("oversized");
    let list: Vec<String> = (0..400).map(h).collect();
    std::fs::write(t.file(), serde_json::to_vec(&list).expect("json")).expect("write");
    let log = SubmittedTxLog::open(t.file());
    assert_eq!(log.len(), 256);
    assert!(!log.record(&h(399)));
    assert!(log.record(&h(0)));
}

#[test]
fn a_record_at_an_unreadable_location_fails_closed() {
    // The path's parent is a regular file: the record can't be read, so no send is a first.
    let t = Tmp::new("unreadable");
    let blocker = t.0.join("blocker");
    std::fs::write(&blocker, b"x").expect("blocker");
    let log = SubmittedTxLog::open(blocker.join("submitted-tx.json"));
    assert!(log.is_degraded());
    assert!(!log.record(&h(5)));
}

#[test]
fn a_record_whose_file_cannot_be_rewritten_still_holds_in_memory() {
    let t = Tmp::new("rewrite-fails");
    let log = SubmittedTxLog::open(t.file());
    assert!(log.record(&h(1)));
    drop(log);
    // The rewrite's temp path is taken by a directory, so the next write fails.
    std::fs::create_dir(t.0.join(format!(".submitted-tx.json.tmp-{}", std::process::id()))).expect("block tmp");
    let log = SubmittedTxLog::open(t.file());
    assert!(!log.is_degraded());
    assert!(log.record(&h(2)), "a first send is still a first send");
    assert!(!log.record(&h(2)), "and the in-memory record holds it");
    let on_disk: Vec<String> = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(on_disk, vec![h(1)], "the failed rewrite left the previous file whole");
}
