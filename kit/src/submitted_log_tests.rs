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
