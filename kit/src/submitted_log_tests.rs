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
        self.0.join(RECORD_FILE)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The genesis the record is opened on in the tests that don't switch networks: r1005, so a file
/// in an earlier format is on its own network there and loads exactly as before.
const G: &str = LEGACY_GENESIS;

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
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(log.record(&h(7)));
    drop(log);
    let again = SubmittedTxLog::open(t.file(), G);
    assert!(!again.record(&h(7)), "a re-send after a restart is not a first submission");
    assert!(again.record(&h(8)));
}

// Hashes recorded without a sender/nonce: an eviction can't be bounded, so the history is uncertain.
#[test]
fn the_record_keeps_the_last_256_and_then_treats_absent_hashes_as_unknown() {
    let t = Tmp::new("bound");
    let log = SubmittedTxLog::open(t.file(), G);
    for n in 0..300 {
        log.record(&h(n));
    }
    assert_eq!(log.len(), CAPACITY);
    assert_eq!(CAPACITY, 256);
    assert!(log.is_uncertain(), "history was truncated");
    drop(log);
    let again = SubmittedTxLog::open(t.file(), G);
    assert_eq!(again.len(), 256);
    assert!(again.is_uncertain(), "the truncation survives a restart");
    assert!(!again.record(&h(299)), "the newest are kept");
    assert!(!again.record(&h(0)), "an evicted hash is not a first send");
    assert!(!again.record(&h(1000)), "after truncation no absent hash is a first send");
}

#[test]
fn the_write_is_atomic_and_leaves_no_temp_file() {
    let t = Tmp::new("atomic");
    let log = SubmittedTxLog::open(t.file(), G);
    log.record(&h(1));
    let names: Vec<String> = std::fs::read_dir(&t.0)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec![RECORD_FILE.to_string()]);
    let body: serde_json::Value = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(body["current"]["entries"], serde_json::json!([{ "hash": h(1) }]));
    assert_eq!(body["current"]["uncertain"], false);
}

#[test]
fn an_unreadable_record_treats_every_send_as_a_resubmission() {
    // A corrupt file means the app can't prove these bytes were never sent: never "not sent".
    let t = Tmp::new("corrupt");
    std::fs::write(t.file(), b"{not json").expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(log.is_uncertain());
    assert!(!log.record(&h(1)));
    assert!(!log.record(&h(2)));
}

#[test]
fn a_missing_file_is_an_empty_record() {
    let t = Tmp::new("missing");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(!log.is_uncertain());
    assert!(log.record(&h(1)));
}

// ---- STD audit parity: concurrency, crash windows, fail-closed ----

#[test]
fn concurrent_records_of_the_same_hash_yield_exactly_one_first() {
    let t = Tmp::new("race");
    let log = std::sync::Arc::new(SubmittedTxLog::open(t.file(), G));
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
    assert!(!SubmittedTxLog::open(t.file(), G).record(&h(42)));
}

#[test]
fn concurrent_records_of_different_hashes_are_all_kept() {
    let t = Tmp::new("race-many");
    let log = std::sync::Arc::new(SubmittedTxLog::open(t.file(), G));
    let joins: Vec<_> = (0..32)
        .map(|n| {
            let l = log.clone();
            std::thread::spawn(move || l.record(&h(n)))
        })
        .collect();
    assert!(joins.into_iter().all(|j| j.join().expect("thread")));
    drop(log);
    let again = SubmittedTxLog::open(t.file(), G);
    assert_eq!(again.len(), 32, "every write landed whole; none lost to interleaving");
}

#[test]
fn a_temp_file_left_by_a_crash_does_not_affect_the_record() {
    let t = Tmp::new("crash-tmp");
    let log = SubmittedTxLog::open(t.file(), G);
    log.record(&h(1));
    drop(log);
    // A crash between writing the temp file and the rename leaves the old file intact plus a temp.
    std::fs::write(t.0.join(".submitted-tx-networks.json.tmp-99999"), b"[\"0xpartial").expect("tmp");
    let again = SubmittedTxLog::open(t.file(), G);
    assert!(!again.is_uncertain());
    assert!(!again.record(&h(1)));
}

#[test]
fn a_truncated_record_is_degraded_and_never_proves_a_first_send() {
    let t = Tmp::new("truncated");
    std::fs::write(t.file(), format!("[\"{}\",\"0x", h(1))).expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(log.is_uncertain());
    assert!(!log.record(&h(9)));
}

#[test]
fn an_oversized_list_on_disk_is_trimmed_and_marked_uncertain() {
    let t = Tmp::new("oversized");
    let list: Vec<String> = (0..400).map(h).collect();
    std::fs::write(t.legacy(), serde_json::to_vec(&list).expect("json")).expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
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
    std::fs::write(t.legacy(), serde_json::to_vec(&list).expect("json")).expect("write");
    assert!(SubmittedTxLog::open(t.file(), G).is_uncertain());
}

#[test]
fn a_legacy_short_list_loads_as_complete_history() {
    let t = Tmp::new("legacy-short");
    std::fs::write(t.legacy(), serde_json::to_vec(&vec![h(1)]).expect("json")).expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(!log.is_uncertain());
    assert!(!log.record(&h(1)));
    assert!(log.record(&h(2)));
}

#[test]
fn a_corrupt_record_stays_uncertain_after_it_is_rewritten_and_reopened() {
    let t = Tmp::new("corrupt-reopen");
    std::fs::write(t.file(), b"{not json").expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(!log.record(&h(1)));
    drop(log);
    let again = SubmittedTxLog::open(t.file(), G);
    assert!(again.is_uncertain(), "history lost to corruption is never treated as complete");
    assert!(!again.record(&h(2)));
}

#[test]
fn a_record_at_an_unreadable_location_fails_closed() {
    // The path's parent is a regular file: the record can't be read, so no send is a first.
    let t = Tmp::new("unreadable");
    let blocker = t.0.join("blocker");
    std::fs::write(&blocker, b"x").expect("blocker");
    let log = SubmittedTxLog::open(blocker.join(RECORD_FILE), G);
    assert!(log.is_uncertain());
    assert!(!log.record(&h(5)));
}

#[test]
fn a_record_that_cannot_be_made_durable_is_refused_and_not_kept() {
    let t = Tmp::new("rewrite-fails");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record(&h(1)), Ok(true));
    drop(log);
    // The rewrite's temp path is taken by a directory, so the next write fails.
    let block = t.0.join(format!(".submitted-tx-networks.json.tmp-{}", std::process::id()));
    std::fs::create_dir(&block).expect("block tmp");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(log.try_record(&h(2)).is_err(), "no durable record, so the caller must not send");
    std::fs::remove_dir(&block).expect("unblock");
    // Nothing was sent, so the refused hash is not history.
    assert_eq!(log.try_record(&h(2)), Ok(true));
    let on_disk: serde_json::Value = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(on_disk["current"]["entries"], serde_json::json!([{ "hash": h(1) }, { "hash": h(2) }]));
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
    let log = SubmittedTxLog::open(t.file(), G);
    evict_a_nonce_0(&log);
    drop(log);
    let again = SubmittedTxLog::open(t.file(), G);
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

#[test]
fn a_record_in_the_previous_format_loads_and_keeps_its_flag() {
    let t = Tmp::new("v2");
    std::fs::write(t.legacy(), serde_json::to_vec(&serde_json::json!({ "hashes": [h(1)], "uncertain": false })).expect("json")).expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(!log.is_uncertain());
    assert!(!log.record(&h(1)));
    std::fs::write(t.legacy(), serde_json::to_vec(&serde_json::json!({ "hashes": [h(1)], "uncertain": true })).expect("json")).expect("write");
    assert!(SubmittedTxLog::open(t.file(), G).is_uncertain());
}

// Durability of the rename (Codex review of #347): the record must be durable BEFORE the caller
// broadcasts. The temp file is fsynced, renamed over the record, and then the directory holding it
// is fsynced, so a power loss after the broadcast can't bring back an older record (or none) and
// turn a resend of a mined tx into a "first" submission. Only then does try_record return.

thread_local! {
    /// What each directory sync saw: (dir, the record file's text, temp files left in the dir).
    static SYNC_SEEN: std::cell::RefCell<Vec<(std::path::PathBuf, String, usize)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn spy_sync(dir: &std::path::Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(dir.join(RECORD_FILE)).unwrap_or_default();
    let temps = std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
                .count()
        })
        .unwrap_or(usize::MAX);
    SYNC_SEEN.with(|s| s.borrow_mut().push((dir.to_path_buf(), text, temps)));
    Ok(())
}

fn failing_sync(_dir: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::other("directory sync failed"))
}

#[test]
fn the_directory_is_synced_after_the_rename_before_the_record_is_used() {
    SYNC_SEEN.with(|s| s.borrow_mut().clear());
    let t = Tmp::new("dirsync-order");
    let log = SubmittedTxLog::open_with(t.file(), G, spy_sync);
    // Opening a new profile writes its empty record once; count only the record write below.
    SYNC_SEEN.with(|s| s.borrow_mut().clear());
    assert_eq!(log.try_record_for(&h(41), A, 0), Ok(true));
    let seen = SYNC_SEEN.with(|s| s.borrow().clone());
    assert_eq!(seen.len(), 1, "one directory sync per record write");
    let (dir, text, temps) = &seen[0];
    assert_eq!(dir, &t.0, "the directory that holds the record is synced");
    assert!(text.contains(&h(41)), "the sync runs after the rename put the new record in place");
    assert_eq!(*temps, 0, "the temp file was already renamed when the directory was synced");
}

#[test]
fn a_failed_directory_sync_refuses_the_record_and_keeps_nothing() {
    let t = Tmp::new("dirsync-fail");
    let log = SubmittedTxLog::open_with(t.file(), G, failing_sync);
    let r = log.try_record_for(&h(42), A, 0);
    assert!(r.is_err(), "a record that isn't durable must stop the send, got {r:?}");
    assert_eq!(log.len(), 0, "the refused hash is not kept");
    assert!(!log.record(&h(43)), "record() is never a first send when the write isn't durable");
}

#[test]
fn every_record_write_syncs_the_directory_including_on_eviction() {
    SYNC_SEEN.with(|s| s.borrow_mut().clear());
    let t = Tmp::new("dirsync-every");
    let log = SubmittedTxLog::open_with(t.file(), G, spy_sync);
    SYNC_SEEN.with(|s| s.borrow_mut().clear());
    for n in 0..(CAPACITY as u32 + 2) {
        log.try_record_for(&h(50_000 + n), A, u64::from(n)).expect("record");
    }
    assert_eq!(SYNC_SEEN.with(|s| s.borrow().len()), CAPACITY + 2);
}

#[test]
fn the_platform_directory_sync_works_on_a_real_directory() {
    let t = Tmp::new("dirsync-real");
    assert!(sync_dir(&t.0).is_ok());
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_for(&h(44), A, 0), Ok(true));
}

#[test]
fn a_directory_sync_on_a_missing_directory_fails_closed() {
    let t = Tmp::new("dirsync-missing");
    assert!(sync_dir(&t.0.join("not-there")).is_err());
}

// #347 review (Codex round 2): when the app data dir can't be resolved, the record can't survive a
// restart, so it must never prove a first submission. Every send is then a re-submission, and a
// definitive-looking node error is reported as "outcome unknown", never "not sent".
#[test]
fn an_unpersisted_record_never_proves_a_first_send() {
    let log = SubmittedTxLog::unpersisted();
    assert!(log.is_uncertain(), "a record that can't survive a restart is uncertain from the start");
    assert_eq!(log.try_record_for(&h(60_001), A, 0), Ok(false));
    assert_eq!(log.try_record(&h(60_002)), Ok(false));
    assert!(!log.record(&h(60_003)));
    assert_eq!(log.try_record_for(&h(60_001), A, 0), Ok(false), "a repeat is still a re-submission");
}

#[test]
fn an_unpersisted_record_stays_uncertain_through_eviction() {
    let log = SubmittedTxLog::unpersisted();
    for n in 0..(CAPACITY as u32 + 3) {
        assert_eq!(log.try_record_for(&h(70_000 + n), A, 1_000 + u64::from(n)), Ok(false));
    }
    assert!(log.is_uncertain());
}

/// Tripwire (C1-V6): outside tests, a SubmittedTxLog is built only by `open(path)` (persisted) or
/// `unpersisted()` (uncertain). No production path can hold a certain in-memory record.
#[test]
fn production_code_builds_the_record_only_persisted_or_uncertain() {
    let roots = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/src"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../src-tauri/src"),
    ];
    let mut offenders = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = roots.iter().map(std::path::PathBuf::from).collect();
    while let Some(p) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&p) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") || name == "submitted_log.rs" {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap_or_default();
            for (i, line) in src.lines().enumerate() {
                if let Some(at) = line.find("SubmittedTxLog::") {
                    let rest = &line[at + "SubmittedTxLog::".len()..];
                    if !(rest.starts_with("open(") || rest.starts_with("unpersisted(")) {
                        offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(offenders.is_empty(), "a certain in-memory record outside tests:\n{}", offenders.join("\n"));
}

// Join-8: the Activate step finds its own send again after a restart, and Forget is durable.
fn activate_call() -> CallMeta {
    CallMeta { to: "0x2222222222222222222222222222222222222222".into(), selector: "0x4bac89ff".into() }
}
const MEMBER: &str = "0x1111111111111111111111111111111111111111";

#[test]
fn join8_the_latest_recorded_call_survives_a_restart() {
    let t = Tmp::new("latest-call");
    {
        let log = SubmittedTxLog::open(t.file(), G);
        assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
        assert_eq!(log.try_record_call(&h(2), MEMBER, 6, NonceSource::Fetched, None), Ok(true)); // another tx
        assert_eq!(log.try_record_call(&h(3), MEMBER, 7, NonceSource::Fetched, Some(&activate_call())), Ok(true));
    }
    let log = SubmittedTxLog::open(t.file(), G);
    let found = log.latest_call(MEMBER, &activate_call()).expect("found after a restart");
    assert_eq!(found.hash, h(3));
    assert_eq!(found.nonce, Some(7));
    assert!(found.at_ms.is_some_and(|ms| ms > 1_600_000_000_000), "recorded time is kept");
    // Another sender, or another call, finds nothing.
    assert_eq!(log.latest_call("0x9999999999999999999999999999999999999999", &activate_call()), None);
    let other = CallMeta { to: activate_call().to, selector: "0xdeadbeef".into() };
    assert_eq!(log.latest_call(MEMBER, &other), None);
}

#[test]
fn join8_forget_is_durable_and_never_makes_a_hash_first_again() {
    let t = Tmp::new("forget");
    {
        let log = SubmittedTxLog::open(t.file(), G);
        assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
        assert_eq!(log.forget(&h(1)), Ok(true));
        assert_eq!(log.latest_call(MEMBER, &activate_call()), None);
    }
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.latest_call(MEMBER, &activate_call()), None, "forgotten across a restart");
    // The same signed bytes (a re-sign at the same nonce) are still a re-submission.
    assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(false));
    assert_eq!(log.forget(&h(42)), Ok(false), "an unknown hash is not forgotten");
}

#[test]
fn join8_a_forget_that_cannot_be_written_is_an_error_and_changes_nothing() {
    let t = Tmp::new("forget-fail");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
    std::fs::create_dir(t.0.join(format!(".submitted-tx-networks.json.tmp-{}", std::process::id()))).expect("block tmp");
    assert!(log.forget(&h(1)).is_err());
    assert_eq!(log.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)));
}

#[test]
fn join8_a_file_from_before_call_metadata_still_loads() {
    let t = Tmp::new("old-format");
    std::fs::write(
        t.legacy(),
        format!(r#"{{"entries":[{{"hash":"{}","sender":"{}","nonce":5}}],"evicted":{{}},"uncertain":false}}"#, h(1), MEMBER),
    )
    .expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(!log.is_uncertain());
    assert_eq!(log.latest_call(MEMBER, &activate_call()), None);
    assert_eq!(log.try_record_for(&h(1), MEMBER, 5), Ok(false));
}

/// Tripwire (#347 DGX review): the temp file is fsynced BEFORE it is renamed over the record. Only
/// the directory sync has a runtime seam, so this pins the file sync in `write_atomic`'s source: a
/// change that drops it, ignores its error, or moves it after the rename fails here. (#354 reuses
/// this writer.)
#[test]
fn write_atomic_fsyncs_the_temp_file_before_the_rename() {
    let src = include_str!("submitted_log.rs");
    let start = src.find("fn write_atomic(").expect("write_atomic exists");
    let body = &src[start..start + src[start..].find("\n}\n").expect("end of write_atomic")];
    let sync = body.find(".sync_all()?").expect("the temp file is fsynced and its error propagated");
    let rename = body.find("std::fs::rename(").expect("the temp file is renamed over the record");
    let create = body.find("create_secret_file(&tmp)").expect("the temp file is created");
    assert!(create < sync && sync < rename, "order must be create temp -> fsync temp -> rename");
    assert!(body[rename..].contains("sync_dir(&dir)"), "the directory is synced after the rename");
}

// #347 Codex P1 (upgrade): builds before this record never wrote it. A profile that already holds
// a wallet (the custody envelope sits in the same app data dir) may have submitted transactions
// with an unknown outcome, so a missing record there is NOT a certain empty history.
#[test]
fn a_missing_record_next_to_an_existing_wallet_is_uncertain() {
    let t = Tmp::new("upgrade-wallet");
    std::fs::write(t.0.join(crate::custody::ENVELOPE_FILE), b"sealed").expect("vault");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(log.is_uncertain(), "an existing wallet with no record may have sent before");
    assert_eq!(log.try_record_for(&h(80_001), A, 7), Ok(false), "never a provable first send");
    drop(log);
    assert!(SubmittedTxLog::open(t.file(), G).is_uncertain(), "the uncertainty is persisted");
}

// A provably new profile (no wallet yet) starts certain, and the empty record is written at once,
// so a wallet created later in this run does not make the next launch look like an upgrade.
#[test]
fn a_fresh_profile_starts_certain_and_writes_its_record_at_once() {
    let t = Tmp::new("fresh-profile");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(!log.is_uncertain());
    assert!(t.file().exists(), "the empty record is written when the profile is new");
    drop(log);
    std::fs::write(t.0.join(crate::custody::ENVELOPE_FILE), b"sealed").expect("vault");
    assert!(!SubmittedTxLog::open(t.file(), G).is_uncertain(), "a wallet created after the record is not an upgrade");
}

// ---- #511: the record is keyed to the genesis it was sent on ----
//
// A 0.5.2 build on r1005 records its sends; the same install then starts on a new genesis with
// the same chain id. The r1005 entries are kept as history ("sent on the previous network") and
// take no part in the new network's first-submission logic: no nonce mark, no uncertainty.

/// Test-only genesis for the new network. Not a real block hash.
const NEW: &str = "0x2026120100000000000000000000000000000000000000000000000000000c52";

fn disk(t: &Tmp) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json")
}

fn dir_names(t: &Tmp) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(&t.0)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn every_entry_is_written_with_the_genesis_it_was_sent_on() {
    let t = Tmp::new("genesis-entry");
    let log = SubmittedTxLog::open(t.file(), G);
    log.try_record_for(&h(1), A, 0).expect("record");
    log.record(&h(2));
    let d = disk(&t);
    assert_eq!(d["current"]["genesis"], G, "the entries are in the section of their genesis");
    assert_eq!(
        d["current"]["entries"],
        serde_json::json!([
            { "hash": h(1), "sender": A, "nonce": 0 },
            { "hash": h(2) }
        ])
    );
}

#[test]
fn entries_sent_on_another_genesis_are_archived_when_the_record_opens() {
    let t = Tmp::new("switch-archive");
    let old = SubmittedTxLog::open(t.file(), G);
    old.try_record_for(&h(1), A, 0).expect("record");
    old.try_record_for(&h(2), A, 1).expect("record");
    drop(old);
    let log = SubmittedTxLog::open(t.file(), NEW);
    assert_eq!(log.len(), 0, "no entry of the previous network is live on the new one");
    assert!(!log.is_uncertain());
    // The archive is on disk at once, in one atomic write, before anything is sent.
    let d = disk(&t);
    assert_eq!(d["current"]["genesis"], NEW);
    assert_eq!(d["current"]["entries"], serde_json::json!([]));
    assert_eq!(
        d["previous_networks"],
        serde_json::json!([{
            "genesis": G,
            "entries": [
                { "hash": h(1), "sender": A, "nonce": 0 },
                { "hash": h(2), "sender": A, "nonce": 1 }
            ],
            "evicted": {},
            "uncertain": false
        }])
    );
    assert_eq!(dir_names(&t), vec![RECORD_FILE.to_string()], "no temp file left");
}

#[test]
fn an_uncertain_history_on_the_previous_genesis_does_not_make_the_new_one_uncertain() {
    // A 0.5.2 build on r1005 opened next to a 0.5.1 wallet (no record): uncertain on r1005.
    let t = Tmp::new("switch-uncertain");
    std::fs::write(t.0.join(crate::custody::ENVELOPE_FILE), b"sealed").expect("vault");
    let old = SubmittedTxLog::open(t.file(), G);
    assert!(old.is_uncertain());
    old.try_record_for(&h(3), A, 0).expect("record");
    drop(old);
    let log = SubmittedTxLog::open(t.file(), NEW);
    assert!(!log.is_uncertain(), "the r1005 history's uncertainty is not the new network's");
    assert_eq!(log.try_record_for(&h(4), A, 0), Ok(true), "a new-chain send can be proved first");
    drop(log);
    let again = SubmittedTxLog::open(t.file(), NEW);
    assert!(!again.is_uncertain(), "and stays so across a restart");
    let r1005 = section_of(&disk(&t), G).expect("r1005 section");
    assert_eq!(r1005["upgrade_bound"], serde_json::json!({}), "kept with the history: {r1005}");
}

#[test]
fn nonce_marks_of_the_previous_genesis_do_not_apply_on_the_new_one() {
    let t = Tmp::new("switch-nonce");
    let old = SubmittedTxLog::open(t.file(), G);
    // CAPACITY + 10 sends from A evict A's nonces 0..=9 on r1005.
    for n in 0..(CAPACITY as u32 + 10) {
        old.try_record_for(&h(n), A, u64::from(n)).expect("record");
    }
    assert_eq!(old.try_record_for(&h(90_000), A, 9), Ok(false), "on r1005 nonce 9 is unknown");
    drop(old);
    let log = SubmittedTxLog::open(t.file(), NEW);
    for n in 0..10u32 {
        assert_eq!(
            log.try_record_for(&h(91_000 + n), A, u64::from(n)),
            Ok(true),
            "new-chain nonce {n} is not bounded by r1005's evicted nonces"
        );
    }
    // The probe at nonce 9 evicted one more r1005 entry (nonce 10).
    assert_eq!(section_of(&disk(&t), G).expect("r1005")["evicted"][A], 10);
}

// The chain id is the same on both networks, so bytes broadcast on r1005 are valid bytes on the new
// network too. A send of byte-identical bytes is never provably first: a refusal can't say "not sent".
#[test]
fn byte_identical_bytes_of_an_archived_send_are_never_a_first_submission() {
    let t = Tmp::new("switch-identical");
    let old = SubmittedTxLog::open(t.file(), G);
    old.try_record_for(&h(1), A, 0).expect("record");
    drop(old);
    let log = SubmittedTxLog::open(t.file(), NEW);
    assert_eq!(log.try_record_for(&h(1), A, 0), Ok(false));
    assert_eq!(log.try_record_for(&h(2), A, 0), Ok(true), "other bytes at the same nonce are first");
}

#[test]
fn a_record_written_before_the_genesis_was_kept_belongs_to_r1005() {
    let t = Tmp::new("legacy-r1005");
    let legacy = serde_json::to_vec(&serde_json::json!({
        "entries": [{ "hash": h(1), "sender": A, "nonce": 5 }],
        "evicted": { A: 300 },
        "uncertain": true
    }))
    .expect("json");
    // On r1005 it behaves exactly as before.
    std::fs::write(t.legacy(), &legacy).expect("write");
    let same = SubmittedTxLog::open(t.file(), LEGACY_GENESIS);
    assert!(same.is_uncertain());
    assert_eq!(same.len(), 1);
    assert_eq!(same.try_record_for(&h(1), A, 5), Ok(false));
    drop(same);
    // On a new genesis it is r1005 history.
    std::fs::write(t.legacy(), &legacy).expect("write");
    let log = SubmittedTxLog::open(t.file(), NEW);
    assert!(!log.is_uncertain());
    assert_eq!(log.len(), 0);
    assert_eq!(log.try_record_for(&h(9), A, 0), Ok(true));
    let d = disk(&t);
    assert_eq!(d["previous_networks"][0]["genesis"], LEGACY_GENESIS);
    assert_eq!(d["previous_networks"][0]["evicted"][A], 300);
    assert_eq!(d["previous_networks"][0]["uncertain"], true);
}

#[test]
fn returning_to_an_archived_genesis_brings_its_history_back() {
    let t = Tmp::new("switch-back");
    std::fs::write(t.0.join(crate::custody::ENVELOPE_FILE), b"sealed").expect("vault");
    let old = SubmittedTxLog::open(t.file(), G);
    old.try_record_for(&h(1), A, 0).expect("record");
    drop(old);
    let new = SubmittedTxLog::open(t.file(), NEW);
    assert_eq!(new.try_record_for(&h(2), A, 0), Ok(true));
    drop(new);
    let back = SubmittedTxLog::open(t.file(), G);
    assert!(back.is_uncertain(), "r1005's uncertainty is r1005's again");
    assert_eq!(back.len(), 1, "only r1005's entries are live on r1005");
    assert_eq!(back.try_record_for(&h(1), A, 0), Ok(false));
    let d = disk(&t);
    assert_eq!(d["previous_networks"][0]["genesis"], NEW);
    assert_eq!(d["previous_networks"][0]["entries"][0]["hash"], h(2));
}

#[test]
fn an_archive_that_could_not_be_written_at_open_is_written_by_the_next_record() {
    let t = Tmp::new("switch-retry");
    let old = SubmittedTxLog::open(t.file(), G);
    old.try_record_for(&h(1), A, 0).expect("record");
    drop(old);
    let block = t.0.join(format!(".submitted-tx-networks.json.tmp-{}", std::process::id()));
    std::fs::create_dir(&block).expect("block tmp");
    let log = SubmittedTxLog::open(t.file(), NEW);
    assert_eq!(disk(&t)["current"]["genesis"], G, "the write at open failed");
    std::fs::remove_dir(&block).expect("unblock");
    assert_eq!(log.try_record_for(&h(2), A, 0), Ok(true));
    let d = disk(&t);
    assert_eq!(d["current"]["genesis"], NEW);
    assert_eq!(d["current"]["entries"][0]["hash"], h(2));
    assert_eq!(d["previous_networks"][0]["entries"][0]["hash"], h(1));
}

#[test]
fn concurrent_first_sends_after_a_switch_keep_the_archive() {
    let t = Tmp::new("switch-race");
    let old = SubmittedTxLog::open(t.file(), G);
    old.try_record_for(&h(1), A, 0).expect("record");
    drop(old);
    let log = std::sync::Arc::new(SubmittedTxLog::open(t.file(), NEW));
    let joins: Vec<_> = (0..16u32)
        .map(|n| {
            let l = log.clone();
            std::thread::spawn(move || l.try_record_for(&h(100 + n), B, u64::from(n)))
        })
        .collect();
    assert!(joins.into_iter().all(|j| j.join().expect("thread") == Ok(true)));
    let d = disk(&t);
    assert_eq!(d["current"]["entries"].as_array().map(Vec::len), Some(16));
    assert_eq!(d["previous_networks"][0]["entries"][0]["hash"], h(1));
}

/// Tripwire (#511): every entry the record writes is in a section that names its genesis, for every
/// file a previous build may have left (each legacy shape, and a record of this format), on the
/// same genesis and after a switch. The current section is the one opened on; no genesis has two
/// sections.
#[test]
fn every_written_entry_carries_a_genesis() {
    let shapes: Vec<(&str, std::path::PathBuf, Option<Vec<u8>>)> = vec![
        ("missing", LEGACY_FILE.into(), None),
        ("bare-list", LEGACY_FILE.into(), Some(serde_json::to_vec(&vec![h(1), h(2)]).expect("json"))),
        ("hashes", LEGACY_FILE.into(), Some(serde_json::to_vec(&serde_json::json!({ "hashes": [h(1)], "uncertain": false })).expect("json"))),
        (
            "no-genesis",
            LEGACY_FILE.into(),
            Some(serde_json::to_vec(&serde_json::json!({
                "entries": [{ "hash": h(1), "sender": A, "nonce": 1 }], "evicted": {}, "uncertain": false
            })).expect("json")),
        ),
        (
            "sectioned",
            RECORD_FILE.into(),
            Some(serde_json::to_vec(&serde_json::json!({
                "format": 3,
                "current": { "genesis": NEW, "entries": [{ "hash": h(3) }], "evicted": {}, "uncertain": false },
                "previous_networks": [{ "genesis": G, "entries": [{ "hash": h(1) }], "evicted": {}, "uncertain": false }]
            })).expect("json")),
        ),
    ];
    let is_genesis = |v: &serde_json::Value| {
        v.as_str().is_some_and(|s| s.len() == 66 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit()))
    };
    for (name, file, bytes) in &shapes {
        for active in [G, NEW] {
            let t = Tmp::new(&format!("tripwire-{name}"));
            if let Some(b) = bytes {
                std::fs::write(t.0.join(file), b).expect("write");
            }
            let log = SubmittedTxLog::open(t.file(), active);
            log.try_record_for(&h(500), B, 1_000).expect("record");
            log.record(&h(501));
            let d = disk(&t);
            assert_eq!(d["current"]["genesis"], active, "{name} on {active}");
            let mut seen = vec![active.to_string()];
            for p in d["previous_networks"].as_array().cloned().unwrap_or_default() {
                assert!(is_genesis(&p["genesis"]), "{name} on {active}: section {p}");
                let g = p["genesis"].as_str().unwrap_or_default().to_string();
                assert!(!seen.contains(&g), "{name} on {active}: two sections for {g}");
                seen.push(g);
                assert!(p["entries"].is_array(), "{name} on {active}: {p}");
            }
            let live: Vec<&str> = d["current"]["entries"].as_array().expect("entries").iter().filter_map(|e| e["hash"].as_str()).collect();
            assert!(live.contains(&h(500).as_str()) && live.contains(&h(501).as_str()), "{name} on {active}: {d}");
        }
    }
}

// Reviewer CR at d737f33 (money path): a re-send of forgotten bytes must bring the send back into
// recovery, durably, or a restart offers a second activation while the re-sent one is in flight.
#[test]
fn join8_a_resend_of_forgotten_bytes_is_found_again_after_a_restart() {
    let t = Tmp::new("resend-forgotten");
    {
        let log = SubmittedTxLog::open(t.file(), G);
        assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
        assert_eq!(log.forget(&h(1)), Ok(true));
        assert_eq!(log.latest_call(MEMBER, &activate_call()), None);
        // The member re-signs at the same nonce (same bytes) and sends again.
        assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(false), "still not a first submission");
        assert_eq!(log.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)), "recovered in this run");
    }
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)), "recovered after a restart");
}

#[test]
fn join8_a_resend_of_forgotten_bytes_is_the_latest_call_even_after_a_newer_forgotten_one() {
    let t = Tmp::new("resend-order");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
    assert_eq!(log.forget(&h(1)), Ok(true));
    assert_eq!(log.try_record_call(&h(2), MEMBER, 6, NonceSource::Fetched, Some(&activate_call())), Ok(true));
    assert_eq!(log.forget(&h(2)), Ok(true));
    assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(false));
    assert_eq!(log.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)));
}

#[test]
fn join8_a_resend_of_forgotten_bytes_that_cannot_be_written_is_refused() {
    let t = Tmp::new("resend-fail");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
    assert_eq!(log.forget(&h(1)), Ok(true));
    std::fs::create_dir(t.0.join(format!(".submitted-tx-networks.json.tmp-{}", std::process::id()))).expect("block tmp");
    assert!(log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())).is_err(), "no durable record, so the caller must not send");
    assert_eq!(log.latest_call(MEMBER, &activate_call()), None, "unchanged");
}

// #443: an upgraded profile (a wallet, no record) is uncertain only up to each sender's first
// post-upgrade send at a nonce the app read from the node. A later app-nonce send signs fresh
// bytes no earlier run can have sent, so its refusal is definitive again.
fn upgraded(tag: &str) -> Tmp {
    let t = Tmp::new(tag);
    std::fs::write(t.0.join(crate::custody::ENVELOPE_FILE), b"sealed").expect("vault");
    t
}

#[test]
fn upgrade_a_fetched_nonce_send_above_the_first_post_upgrade_send_is_a_first_send() {
    let t = upgraded("upgrade-bound");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), A, 7, NonceSource::Fetched, None), Ok(false), "the first send after the upgrade stays unknown");
    assert_eq!(log.try_record_call(&h(2), A, 7, NonceSource::Fetched, None), Ok(false), "so does any send at or below that nonce");
    assert_eq!(log.try_record_call(&h(3), A, 8, NonceSource::Fetched, None), Ok(true), "a higher fetched nonce is a first send");
    drop(log);
    let again = SubmittedTxLog::open(t.file(), G);
    assert_eq!(again.try_record_call(&h(4), A, 9, NonceSource::Fetched, None), Ok(true), "the bound survives a restart");
    assert_eq!(again.try_record_call(&h(5), A, 6, NonceSource::Fetched, None), Ok(false), "below the bound stays unknown");
}

#[test]
fn upgrade_a_supplied_nonce_stays_unknown() {
    let t = upgraded("upgrade-supplied");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), A, 7, NonceSource::Fetched, None), Ok(false));
    assert_eq!(log.try_record_call(&h(2), A, 50, NonceSource::Supplied, None), Ok(false), "a requester's nonce is never bounded");
    assert_eq!(log.try_record_for(&h(3), A, 51), Ok(false), "nor is a nonce of unknown origin");
    assert_eq!(log.try_record_call(&h(4), A, 8, NonceSource::Fetched, None), Ok(true));
}

#[test]
fn upgrade_the_bound_is_per_sender() {
    let t = upgraded("upgrade-per-sender");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), A, 7, NonceSource::Fetched, None), Ok(false));
    assert_eq!(log.try_record_call(&h(2), B, 9, NonceSource::Fetched, None), Ok(false), "B's first send after the upgrade is unknown");
    assert_eq!(log.try_record_call(&h(3), B, 10, NonceSource::Fetched, None), Ok(true));
}

#[test]
fn upgrade_bounded_record_reads_uncertain_to_a_build_that_does_not_know_the_bound() {
    // A build without the bound reads only `uncertain`: it must stay fail closed.
    let t = upgraded("upgrade-ondisk");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), A, 7, NonceSource::Fetched, None), Ok(false));
    // The record is a file such a build neither opens nor parses (design 3), and the bound is
    // its section's.
    let bytes = std::fs::read(t.file()).expect("read");
    assert!(serde_json::from_slice::<OldReader>(&bytes).is_err());
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(v["current"]["upgrade_bound"][A], serde_json::json!(7), "{v}");
}

#[test]
fn upgrade_a_corrupt_record_is_never_bounded() {
    let t = upgraded("upgrade-corrupt");
    std::fs::write(t.file(), b"{not json").expect("corrupt");
    let log = SubmittedTxLog::open(t.file(), G);
    assert_eq!(log.try_record_call(&h(1), A, 7, NonceSource::Fetched, None), Ok(false));
    assert_eq!(log.try_record_call(&h(2), A, 8, NonceSource::Fetched, None), Ok(false), "a damaged history stays unknown for good");
    drop(log);
    let again = SubmittedTxLog::open(t.file(), G);
    assert_eq!(again.try_record_call(&h(3), A, 9, NonceSource::Fetched, None), Ok(false));
}

// ---- #511 lane design (MAC lead, 2026-10-27): one section per network ----
//
// 1. Each network's section holds its entries AND its #507 upgrade bound; a network switch
//    archives the whole section, read-only.
// 2. latest_call, forget and every current-network decision read only the current section.
// 3. A new file (RECORD_FILE, format 3). The legacy file's sends (r1005) are migrated into r1005's
//    section, and the legacy file is set aside, never deleted and never read as empty. A build
//    that reads only the legacy file can't misread the new one.
// 4. #507's semantics are unchanged within a section.

impl Tmp {
    fn record(&self) -> std::path::PathBuf {
        self.0.join(RECORD_FILE)
    }
    fn legacy(&self) -> std::path::PathBuf {
        self.0.join(LEGACY_FILE)
    }
}

/// The section of `genesis` in a RECORD_FILE body (current or archived), if any.
fn section_of(d: &serde_json::Value, genesis: &str) -> Option<serde_json::Value> {
    if d["current"]["genesis"] == genesis {
        return Some(d["current"].clone());
    }
    d["previous_networks"]
        .as_array()?
        .iter()
        .find(|s| s["genesis"] == genesis)
        .cloned()
}

fn record_json(t: &Tmp) -> serde_json::Value {
    std::fs::read(t.record())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// A legacy record as the last builds before the per-network record wrote it: Join-8 fields, an
/// eviction mark, and #507's upgrade bound (written with `uncertain: true` as its marker).
fn legacy_body() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "entries": [
            { "hash": h(1), "sender": MEMBER, "nonce": 5, "to": activate_call().to, "selector": activate_call().selector, "at_ms": 1_700_000_000_000u64 },
            { "hash": h(2), "sender": A, "nonce": 400 }
        ],
        "evicted": { A: 300 },
        "uncertain": true,
        "upgrade_bound": { MEMBER: 5 }
    }))
    .expect("json")
}

#[test]
fn d1_the_upgrade_bound_belongs_to_its_network_and_is_archived_with_it() {
    let t = upgraded("d1-bound-archived");
    let r1005 = SubmittedTxLog::open(t.record(), G);
    assert_eq!(r1005.try_record_call(&h(1), A, 7, NonceSource::Fetched, None), Ok(false), "r1005: the first send after the upgrade is unknown");
    drop(r1005);
    let new = SubmittedTxLog::open(t.record(), NEW);
    assert!(!new.is_uncertain(), "r1005's bound is not the new network's");
    assert_eq!(new.try_record_call(&h(2), A, 0, NonceSource::Fetched, None), Ok(true), "new-network nonce 0 is not under r1005's bound 7");
    drop(new);
    let archived = section_of(&record_json(&t), G).expect("r1005's section is archived");
    assert_eq!(archived["upgrade_bound"][A], 7, "the bound moved to the archive with its section: {archived}");
    let back = SubmittedTxLog::open(t.record(), G);
    assert_eq!(back.try_record_call(&h(3), A, 6, NonceSource::Fetched, None), Ok(false), "back on r1005 its bound applies again");
    assert_eq!(back.try_record_call(&h(4), A, 8, NonceSource::Fetched, None), Ok(true));
}

#[test]
fn d1_a_profile_upgraded_on_a_new_network_keeps_r1005_bounded_too() {
    // 0.5.1 (no record) -> 0.5.2 starting on the new genesis: 0.5.1's unknown sends were r1005's.
    let t = upgraded("d1-new-then-r1005");
    let new = SubmittedTxLog::open(t.record(), NEW);
    assert!(new.is_uncertain(), "the first launch after 0.5.1 is upgrade-bounded");
    assert_eq!(new.try_record_call(&h(1), A, 0, NonceSource::Fetched, None), Ok(false));
    assert_eq!(new.try_record_call(&h(2), A, 1, NonceSource::Fetched, None), Ok(true));
    drop(new);
    let r1005 = SubmittedTxLog::open(t.record(), G);
    assert!(r1005.is_uncertain(), "r1005 holds 0.5.1's unrecorded sends");
    assert_eq!(r1005.try_record_call(&h(3), A, 5, NonceSource::Fetched, None), Ok(false), "r1005's own bound is set by its own first send");
    assert_eq!(r1005.try_record_call(&h(4), A, 6, NonceSource::Fetched, None), Ok(true));
}

#[test]
fn d2_latest_call_and_forget_never_reach_a_previous_network() {
    let t = Tmp::new("d2-current-only");
    let r1005 = SubmittedTxLog::open(t.record(), G);
    assert_eq!(r1005.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
    drop(r1005);
    let new = SubmittedTxLog::open(t.record(), NEW);
    assert_eq!(new.latest_call(MEMBER, &activate_call()), None, "an activation sent on r1005 is not outstanding here");
    assert_eq!(new.forget(&h(1)), Ok(false), "a previous network's send can't be forgotten from this one");
    drop(new);
    let back = SubmittedTxLog::open(t.record(), G);
    assert_eq!(back.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)), "r1005's section is unchanged");
}

#[test]
fn d2_a_resend_of_bytes_sent_on_a_previous_network_is_recorded_on_this_one() {
    let t = Tmp::new("d2-resend-archived");
    let r1005 = SubmittedTxLog::open(t.record(), G);
    assert_eq!(r1005.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(true));
    drop(r1005);
    let new = SubmittedTxLog::open(t.record(), NEW);
    assert_eq!(new.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())), Ok(false), "identical bytes are never provably first (same chain id)");
    assert_eq!(new.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)), "sent on this network now");
    assert_eq!(new.forget(&h(1)), Ok(true), "and forgotten on this network");
    drop(new);
    let cur = section_of(&record_json(&t), NEW).expect("current section");
    assert_eq!(cur["entries"][0]["hash"], h(1), "{cur}");
    let back = SubmittedTxLog::open(t.record(), G);
    assert_eq!(back.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)), "forgetting on the new network leaves r1005's send alone");
}

/// Tripwire (design 2): an archived section never changes an answer about the current network.
/// The same current-network history, with and without a rich archive (an outstanding activation,
/// eviction marks, an uncertain flag and an upgrade bound, all on r1005), gives the same answers to
/// every query, except that byte-identical bytes are never first.
#[test]
fn d2_an_archived_section_never_changes_a_current_network_answer() {
    let with = upgraded("d2-with-archive");
    {
        let r1005 = SubmittedTxLog::open(with.record(), G);
        r1005.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())).expect("record");
        for n in 0..=(CAPACITY as u32) {
            r1005.try_record_call(&h(1_000 + n), A, u64::from(n), NonceSource::Fetched, None).expect("record");
        }
        r1005.try_record(&h(9)).expect("record");
    }
    let without = Tmp::new("d2-without-archive");
    let a = SubmittedTxLog::open(with.record(), NEW);
    let b = SubmittedTxLog::open(without.record(), NEW);
    assert_eq!(a.is_uncertain(), b.is_uncertain());
    assert_eq!(a.len(), b.len());
    assert_eq!(a.latest_call(MEMBER, &activate_call()), b.latest_call(MEMBER, &activate_call()));
    assert_eq!(a.forget(&h(1)), b.forget(&h(1)));
    for (i, (who, nonce, src)) in [
        (A, 0u64, NonceSource::Fetched),
        (A, 3, NonceSource::Supplied),
        (MEMBER, 5, NonceSource::Fetched),
        (B, 0, NonceSource::Fetched),
    ]
    .into_iter()
    .enumerate()
    {
        let hash = h(50_000 + i as u32);
        assert_eq!(a.try_record_call(&hash, who, nonce, src, None), b.try_record_call(&hash, who, nonce, src, None), "{who} at {nonce}");
    }
    assert_eq!(a.try_record(&h(50_100)), b.try_record(&h(50_100)));
    assert_eq!(a.latest_call(MEMBER, &activate_call()), b.latest_call(MEMBER, &activate_call()));
}

#[test]
fn d3_a_legacy_record_is_migrated_into_r1005_and_set_aside() {
    let t = Tmp::new("d3-migrate");
    std::fs::write(t.legacy(), legacy_body()).expect("legacy");
    let new = SubmittedTxLog::open(t.record(), NEW);
    assert_eq!(new.len(), 0);
    assert!(!new.is_uncertain(), "r1005's history is not the new network's");
    drop(new);
    assert!(!t.legacy().exists(), "the legacy file is not read again as the record");
    let aside = t.0.join(format!("{LEGACY_FILE}.migrated"));
    assert_eq!(std::fs::read(&aside).ok(), Some(legacy_body()), "set aside byte for byte, never deleted");
    let d = record_json(&t);
    assert_eq!(d["format"], 3, "{d}");
    assert_eq!(d["current"]["genesis"], NEW);
    let r = section_of(&d, LEGACY_GENESIS).expect("r1005 section");
    assert_eq!(r["entries"].as_array().map(Vec::len), Some(2), "{r}");
    assert_eq!(r["entries"][0]["selector"], activate_call().selector, "Join-8 fields kept: {r}");
    assert_eq!(r["evicted"][A], 300);
    assert_eq!(r["upgrade_bound"][MEMBER], 5, "#507's bound migrated with its section: {r}");
    assert_eq!(r["uncertain"], false, "the bound, not a damaged history");
    // Back on r1005 the migrated section is the current one, as it was.
    let back = SubmittedTxLog::open(t.record(), LEGACY_GENESIS);
    assert_eq!(back.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)));
    assert_eq!(back.try_record_for(&h(2), A, 400), Ok(false));
    assert_eq!(back.try_record_call(&h(3), MEMBER, 6, NonceSource::Fetched, None), Ok(true), "above the migrated bound");
    assert_eq!(back.try_record_call(&h(4), A, 300, NonceSource::Supplied, None), Ok(false), "at the migrated eviction mark");
}

#[test]
fn d3_on_r1005_the_legacy_record_is_the_current_section() {
    let t = Tmp::new("d3-same");
    std::fs::write(t.legacy(), legacy_body()).expect("legacy");
    let log = SubmittedTxLog::open(t.record(), LEGACY_GENESIS);
    assert_eq!(log.len(), 2);
    assert!(log.is_uncertain(), "upgrade-bounded");
    assert_eq!(log.latest_call(MEMBER, &activate_call()).map(|c| c.hash), Some(h(1)));
    assert!(t.0.join(format!("{LEGACY_FILE}.migrated")).exists());
    assert_ne!(t.record(), t.legacy());
}

#[test]
fn d3_a_legacy_file_written_again_by_an_older_build_is_merged_again() {
    let t = Tmp::new("d3-remerge");
    std::fs::write(t.legacy(), legacy_body()).expect("legacy");
    drop(SubmittedTxLog::open(t.record(), NEW));
    // An older build ran in between: it found no legacy file and wrote a new one with its send.
    let older = serde_json::json!({ "entries": [{ "hash": h(9), "sender": A, "nonce": 401 }], "evicted": {}, "uncertain": true });
    std::fs::write(t.legacy(), serde_json::to_vec(&older).expect("json")).expect("legacy again");
    let new = SubmittedTxLog::open(t.record(), NEW);
    assert_eq!(new.len(), 0);
    drop(new);
    assert!(t.0.join(format!("{LEGACY_FILE}.migrated-2")).exists(), "the first set-aside is kept too");
    let r = section_of(&record_json(&t), LEGACY_GENESIS).expect("r1005 section");
    let hashes: Vec<&str> = r["entries"].as_array().expect("entries").iter().filter_map(|e| e["hash"].as_str()).collect();
    assert_eq!(hashes, vec![h(1).as_str(), h(2).as_str(), h(9).as_str()]);
    assert_eq!(r["uncertain"], true, "the older build's uncertain record keeps r1005 uncertain");
}

#[test]
fn d3_the_legacy_file_is_set_aside_only_after_the_record_is_durable() {
    let t = Tmp::new("d3-durable-first");
    std::fs::write(t.legacy(), legacy_body()).expect("legacy");
    let block = t.0.join(format!(".{RECORD_FILE}.tmp-{}", std::process::id()));
    std::fs::create_dir(&block).expect("block tmp");
    let log = SubmittedTxLog::open(t.record(), LEGACY_GENESIS);
    assert_eq!(log.len(), 2, "the legacy history is used in memory");
    drop(log);
    assert_eq!(std::fs::read(t.legacy()).ok(), Some(legacy_body()), "not set aside: the record could not be written");
    assert!(!t.0.join(format!("{LEGACY_FILE}.migrated")).exists());
    std::fs::remove_dir(&block).expect("unblock");
    let log = SubmittedTxLog::open(t.record(), LEGACY_GENESIS);
    assert_eq!(log.len(), 2);
    assert!(!t.legacy().exists() && t.0.join(format!("{LEGACY_FILE}.migrated")).exists());
}

#[test]
fn d3_an_unparsable_legacy_file_is_never_read_as_empty() {
    let t = Tmp::new("d3-legacy-corrupt");
    std::fs::write(t.legacy(), b"{not json").expect("legacy");
    let log = SubmittedTxLog::open(t.record(), LEGACY_GENESIS);
    assert!(log.is_uncertain());
    assert_eq!(log.try_record_call(&h(1), A, 9, NonceSource::Fetched, None), Ok(false));
    assert_eq!(log.try_record_call(&h(2), A, 10, NonceSource::Fetched, None), Ok(false), "a damaged history is never bounded");
    assert_eq!(std::fs::read(t.0.join(format!("{LEGACY_FILE}.migrated"))).ok(), Some(b"{not json".to_vec()));
}

/// The readers of the last legacy builds, copied as they were (c7db777e), plus the two older shapes.
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct OldReader {
    entries: Vec<serde_json::Value>,
    #[serde(default)]
    evicted: std::collections::BTreeMap<String, u64>,
    uncertain: bool,
    #[serde(default)]
    upgrade_bound: Option<std::collections::BTreeMap<String, u64>>,
}
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct OldReaderV2 {
    hashes: Vec<String>,
    uncertain: bool,
}

#[test]
fn d3_a_build_that_reads_only_the_legacy_record_can_not_misread_the_new_one() {
    assert_ne!(RECORD_FILE, LEGACY_FILE, "older builds never open the new file by name");
    let t = upgraded("d3-old-reader");
    let log = SubmittedTxLog::open(t.record(), G);
    log.try_record_call(&h(1), A, 7, NonceSource::Fetched, Some(&activate_call())).expect("record");
    drop(log);
    drop(SubmittedTxLog::open(t.record(), NEW));
    let bytes = std::fs::read(t.record()).expect("record");
    assert!(serde_json::from_slice::<OldReader>(&bytes).is_err(), "an older reader must fail (closed), not read an empty or partial history");
    assert!(serde_json::from_slice::<OldReaderV2>(&bytes).is_err());
    assert!(serde_json::from_slice::<Vec<String>>(&bytes).is_err());
}

#[test]
fn d3_a_record_in_an_unknown_format_is_set_aside_and_every_network_is_uncertain() {
    let t = Tmp::new("d3-future");
    let future = serde_json::to_vec(&serde_json::json!({ "format": 4, "current": { "genesis": NEW, "entries": [], "uncertain": false } })).expect("json");
    std::fs::write(t.record(), &future).expect("future");
    let log = SubmittedTxLog::open(t.record(), NEW);
    assert!(log.is_uncertain(), "a format this build can't read is never read as empty");
    drop(log);
    assert_eq!(std::fs::read(t.0.join(format!("{RECORD_FILE}.unreadable"))).ok(), Some(future), "kept, not overwritten");
    let other = SubmittedTxLog::open(t.record(), G);
    assert!(other.is_uncertain(), "the lost record may have held this network's sends too");
}

#[test]
fn d4_within_a_section_the_bound_keeps_507s_semantics_after_migration_and_a_switch() {
    let t = Tmp::new("d4-507");
    std::fs::write(t.legacy(), legacy_body()).expect("legacy");
    drop(SubmittedTxLog::open(t.record(), NEW));
    let log = SubmittedTxLog::open(t.record(), LEGACY_GENESIS);
    // MEMBER's bound is 5: at or below unknown, above first; supplied never bounded.
    assert_eq!(log.try_record_call(&h(10), MEMBER, 5, NonceSource::Fetched, None), Ok(false));
    assert_eq!(log.try_record_call(&h(11), MEMBER, 4, NonceSource::Fetched, None), Ok(false));
    assert_eq!(log.try_record_call(&h(12), MEMBER, 60, NonceSource::Supplied, None), Ok(false));
    assert_eq!(log.try_record_call(&h(13), MEMBER, 6, NonceSource::Fetched, None), Ok(true));
    // B has no bound yet: its first fetched send sets it (unknown), the next above it is first.
    assert_eq!(log.try_record_call(&h(14), B, 2, NonceSource::Fetched, None), Ok(false));
    assert_eq!(log.try_record_call(&h(15), B, 3, NonceSource::Fetched, None), Ok(true));
    drop(log);
    let r = section_of(&record_json(&t), LEGACY_GENESIS).expect("r1005 section");
    assert_eq!(r["upgrade_bound"], serde_json::json!({ MEMBER: 5, B: 2 }), "{r}");
}

/// Tripwire (design 2): outside tests, the archive (`Inner::previous`) is touched only to load,
/// write and migrate the record and by `sent_on_a_previous_network`, which may only take
/// first-ness away (`&& !...`). A new read of an archived section in a decision path fails here.
#[test]
fn tripwire_no_decision_path_reads_an_archived_section() {
    let src = include_str!("submitted_log.rs");
    let prod = &src[..src.find("#[cfg(test)]\nmod tests").expect("tests module")];
    let allowed = ["new", "sent_on_a_previous_network", "to_disk", "read_record", "open_with"];
    let mut offenders = Vec::new();
    for (at, _) in prod.match_indices(".previous") {
        let before = &prod[..at];
        let name = before
            .rfind("fn ")
            .map(|f| before[f + 3..].split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or(""))
            .unwrap_or("");
        if !allowed.contains(&name) {
            offenders.push(format!("fn {name} reads the archive"));
        }
    }
    let calls: Vec<_> = prod.match_indices("sent_on_a_previous_network(").map(|(i, _)| i).collect();
    for i in calls {
        let line_start = prod[..i].rfind('\n').map_or(0, |p| p + 1);
        let line = &prod[line_start..i];
        let is_def = line.trim_start().starts_with("fn ");
        let narrows = line.trim_end().ends_with("&& !g.");
        if !(is_def || narrows) {
            offenders.push(format!("sent_on_a_previous_network used outside `&& !`: {}", line.trim()));
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

// ---- Review of 0cffe02 (lead decision (i)): a new network's section starts bounded when any
// archived section is uncertain, bounded or has eviction marks. Decided once, when the section is
// created; no decision path reads the archive.

#[test]
fn p1_an_evicted_send_of_a_previous_network_replayed_here_is_never_first() {
    let t = Tmp::new("p1-evicted-replay");
    let r1005 = SubmittedTxLog::open(t.record(), G);
    // CAPACITY + 1 sends from A on r1005: h(0) at nonce 0 is evicted from the section.
    evict_a_nonce_0(&r1005);
    drop(r1005);
    let new = SubmittedTxLog::open(t.record(), NEW);
    assert!(new.is_uncertain(), "an archive with eviction marks leaves the new network bounded");
    assert_eq!(new.try_record_call(&h(0), A, 0, NonceSource::Fetched, None), Ok(false), "the evicted r1005 bytes, sent again here, are never a first send");
    assert_eq!(new.try_record_call(&h(70_000), A, 0, NonceSource::Supplied, None), Ok(false));
    // After A's first fetched-nonce send here (the bound), a fetched send above it is first again.
    assert_eq!(new.try_record_call(&h(70_001), A, 1, NonceSource::Fetched, None), Ok(true));
}

#[test]
fn both_upgrade_paths_from_0_5_1_bound_the_new_network_alike() {
    // Direct: 0.5.1 (wallet, no record) opens on the new genesis.
    let direct = upgraded("p2-direct");
    // Via a #507-era build: its legacy record carries an upgrade bound on r1005.
    let via = upgraded("p2-via-507");
    std::fs::write(
        via.legacy(),
        serde_json::to_vec(&serde_json::json!({ "entries": [{ "hash": h(1), "sender": A, "nonce": 3 }], "evicted": {}, "uncertain": true, "upgrade_bound": { A: 3 } })).expect("json"),
    )
    .expect("legacy");
    for t in [&direct, &via] {
        let log = SubmittedTxLog::open(t.record(), NEW);
        assert!(log.is_uncertain(), "{}", t.0.display());
        assert_eq!(log.try_record_call(&h(10), B, 9, NonceSource::Supplied, None), Ok(false), "{}", t.0.display());
        assert_eq!(log.try_record_call(&h(11), B, 4, NonceSource::Fetched, None), Ok(false), "{}", t.0.display());
        assert_eq!(log.try_record_call(&h(12), B, 5, NonceSource::Fetched, None), Ok(true), "{}", t.0.display());
        drop(log);
        let r1005 = section_of(&record_json(t), G).expect("r1005 section");
        assert!(r1005["upgrade_bound"].is_object(), "r1005 bounded on both paths: {r1005}");
    }
}

#[cfg(unix)]
fn chmod(p: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// Item 3: a record that can't be READ (an I/O error) is set aside like an unparsable one, never
/// overwritten by the next write.
#[cfg(unix)]
#[test]
fn an_unreadable_record_is_set_aside_never_overwritten() {
    let t = Tmp::new("unreadable-aside");
    let log = SubmittedTxLog::open(t.record(), G);
    log.try_record_call(&h(1), MEMBER, 5, NonceSource::Fetched, Some(&activate_call())).expect("record");
    drop(log);
    let original = std::fs::read(t.record()).expect("record");
    chmod(&t.record(), 0o000);
    if std::fs::read(t.record()).is_ok() {
        return; // running as a user that reads through permissions: nothing to test
    }
    let log = SubmittedTxLog::open(t.record(), G);
    assert!(log.is_uncertain());
    assert_eq!(log.try_record(&h(2)), Ok(false));
    drop(log);
    let aside = t.0.join(format!("{RECORD_FILE}.unreadable"));
    let _ = std::fs::set_permissions(&aside, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    assert_eq!(std::fs::read(&aside).ok(), Some(original), "the unreadable record is kept byte for byte");
}

/// Item 3: if the unreadable record can't be set aside either, nothing overwrites it: every write
/// is refused, so the caller sends nothing.
#[cfg(unix)]
#[test]
fn an_unreadable_record_that_can_not_be_set_aside_refuses_every_write() {
    let t = Tmp::new("unreadable-stuck");
    let dir = t.0.join("app");
    std::fs::create_dir(&dir).expect("dir");
    let record = dir.join(RECORD_FILE);
    std::fs::write(&record, b"{\"format\":3}").expect("record");
    chmod(&record, 0o000);
    chmod(&dir, 0o500);
    let readable = std::fs::read(&record).is_ok();
    let log = SubmittedTxLog::open(record.clone(), G);
    let r = log.try_record(&h(3));
    chmod(&dir, 0o700);
    chmod(&record, 0o600);
    if readable {
        return;
    }
    assert!(r.as_ref().is_err_and(|e| e.contains("could not be read")), "no write while the record can't be read: {r:?}");
    assert_eq!(std::fs::read(&record).ok(), Some(b"{\"format\":3}".to_vec()));
}
