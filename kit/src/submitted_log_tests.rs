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
    assert_eq!(names, vec!["submitted-tx.json".to_string()]);
    let body: serde_json::Value = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(body["entries"], serde_json::json!([{ "hash": h(1), "genesis": G }]));
    assert_eq!(body["uncertain"], false);
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
    std::fs::write(t.0.join(".submitted-tx.json.tmp-99999"), b"[\"0xpartial").expect("tmp");
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
    std::fs::write(t.file(), serde_json::to_vec(&list).expect("json")).expect("write");
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
    std::fs::write(t.file(), serde_json::to_vec(&list).expect("json")).expect("write");
    assert!(SubmittedTxLog::open(t.file(), G).is_uncertain());
}

#[test]
fn a_legacy_short_list_loads_as_complete_history() {
    let t = Tmp::new("legacy-short");
    std::fs::write(t.file(), serde_json::to_vec(&vec![h(1)]).expect("json")).expect("write");
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
    let log = SubmittedTxLog::open(blocker.join("submitted-tx.json"), G);
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
    let block = t.0.join(format!(".submitted-tx.json.tmp-{}", std::process::id()));
    std::fs::create_dir(&block).expect("block tmp");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(log.try_record(&h(2)).is_err(), "no durable record, so the caller must not send");
    std::fs::remove_dir(&block).expect("unblock");
    // Nothing was sent, so the refused hash is not history.
    assert_eq!(log.try_record(&h(2)), Ok(true));
    let on_disk: serde_json::Value = serde_json::from_slice(&std::fs::read(t.file()).expect("read")).expect("json");
    assert_eq!(on_disk["entries"], serde_json::json!([{ "hash": h(1), "genesis": G }, { "hash": h(2), "genesis": G }]));
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
    std::fs::write(t.file(), serde_json::to_vec(&serde_json::json!({ "hashes": [h(1)], "uncertain": false })).expect("json")).expect("write");
    let log = SubmittedTxLog::open(t.file(), G);
    assert!(!log.is_uncertain());
    assert!(!log.record(&h(1)));
    std::fs::write(t.file(), serde_json::to_vec(&serde_json::json!({ "hashes": [h(1)], "uncertain": true })).expect("json")).expect("write");
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
    let text = std::fs::read_to_string(dir.join("submitted-tx.json")).unwrap_or_default();
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
    assert_eq!(d["genesis"], G);
    assert_eq!(
        d["entries"],
        serde_json::json!([
            { "hash": h(1), "sender": A, "nonce": 0, "genesis": G },
            { "hash": h(2), "genesis": G }
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
    assert_eq!(d["genesis"], NEW);
    assert_eq!(d["entries"], serde_json::json!([]));
    assert_eq!(
        d["sent_on_previous_networks"],
        serde_json::json!([{
            "genesis": G,
            "entries": [
                { "hash": h(1), "sender": A, "nonce": 0, "genesis": G },
                { "hash": h(2), "sender": A, "nonce": 1, "genesis": G }
            ],
            "evicted": {},
            "uncertain": false
        }])
    );
    assert_eq!(dir_names(&t), vec!["submitted-tx.json".to_string()], "no temp file left");
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
    assert_eq!(disk(&t)["sent_on_previous_networks"][0]["uncertain"], true, "kept with the history");
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
    assert_eq!(disk(&t)["sent_on_previous_networks"][0]["evicted"][A], 9);
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
    std::fs::write(t.file(), &legacy).expect("write");
    let same = SubmittedTxLog::open(t.file(), LEGACY_GENESIS);
    assert!(same.is_uncertain());
    assert_eq!(same.len(), 1);
    assert_eq!(same.try_record_for(&h(1), A, 5), Ok(false));
    drop(same);
    // On a new genesis it is r1005 history.
    std::fs::write(t.file(), &legacy).expect("write");
    let log = SubmittedTxLog::open(t.file(), NEW);
    assert!(!log.is_uncertain());
    assert_eq!(log.len(), 0);
    assert_eq!(log.try_record_for(&h(9), A, 0), Ok(true));
    let d = disk(&t);
    assert_eq!(d["sent_on_previous_networks"][0]["genesis"], LEGACY_GENESIS);
    assert_eq!(d["sent_on_previous_networks"][0]["evicted"][A], 300);
    assert_eq!(d["sent_on_previous_networks"][0]["uncertain"], true);
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
    assert_eq!(d["sent_on_previous_networks"][0]["genesis"], NEW);
    assert_eq!(d["sent_on_previous_networks"][0]["entries"][0]["hash"], h(2));
}

#[test]
fn an_archive_that_could_not_be_written_at_open_is_written_by_the_next_record() {
    let t = Tmp::new("switch-retry");
    let old = SubmittedTxLog::open(t.file(), G);
    old.try_record_for(&h(1), A, 0).expect("record");
    drop(old);
    let block = t.0.join(format!(".submitted-tx.json.tmp-{}", std::process::id()));
    std::fs::create_dir(&block).expect("block tmp");
    let log = SubmittedTxLog::open(t.file(), NEW);
    assert!(disk(&t).get("sent_on_previous_networks").is_none(), "the write at open failed");
    std::fs::remove_dir(&block).expect("unblock");
    assert_eq!(log.try_record_for(&h(2), A, 0), Ok(true));
    let d = disk(&t);
    assert_eq!(d["genesis"], NEW);
    assert_eq!(d["entries"][0]["hash"], h(2));
    assert_eq!(d["sent_on_previous_networks"][0]["entries"][0]["hash"], h(1));
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
    assert_eq!(d["entries"].as_array().map(Vec::len), Some(16));
    assert_eq!(d["sent_on_previous_networks"][0]["entries"][0]["hash"], h(1));
}

/// Tripwire (#511): every entry the record writes carries the genesis it was sent on, for every
/// file shape a previous build may have left, on the same genesis and after a switch. Live entries
/// carry the record's genesis; archived ones the genesis of their section.
#[test]
fn every_written_entry_carries_a_genesis() {
    let shapes: Vec<(&str, Option<Vec<u8>>)> = vec![
        ("missing", None),
        ("bare-list", Some(serde_json::to_vec(&vec![h(1), h(2)]).expect("json"))),
        ("hashes", Some(serde_json::to_vec(&serde_json::json!({ "hashes": [h(1)], "uncertain": false })).expect("json"))),
        (
            "no-genesis",
            Some(serde_json::to_vec(&serde_json::json!({
                "entries": [{ "hash": h(1), "sender": A, "nonce": 1 }], "evicted": {}, "uncertain": false
            })).expect("json")),
        ),
        (
            "keyed",
            Some(serde_json::to_vec(&serde_json::json!({
                "genesis": NEW,
                "entries": [{ "hash": h(3), "genesis": NEW }],
                "evicted": {},
                "uncertain": false,
                "sent_on_previous_networks": [{
                    "genesis": G, "entries": [{ "hash": h(1) }], "evicted": {}, "uncertain": false
                }]
            })).expect("json")),
        ),
    ];
    let is_genesis = |v: &serde_json::Value| {
        v.as_str().is_some_and(|s| s.len() == 66 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit()))
    };
    for (name, bytes) in &shapes {
        for active in [G, NEW] {
            let t = Tmp::new(&format!("tripwire-{name}"));
            if let Some(b) = bytes {
                std::fs::write(t.file(), b).expect("write");
            }
            let log = SubmittedTxLog::open(t.file(), active);
            log.try_record_for(&h(500), B, 1_000).expect("record");
            log.record(&h(501));
            let d = disk(&t);
            assert_eq!(d["genesis"], active, "{name} on {active}");
            for e in d["entries"].as_array().expect("entries") {
                assert_eq!(e["genesis"], active, "{name} on {active}: live entry {e}");
            }
            for p in d["sent_on_previous_networks"].as_array().cloned().unwrap_or_default() {
                assert!(is_genesis(&p["genesis"]), "{name} on {active}: section {p}");
                assert_ne!(p["genesis"], active, "{name} on {active}: the live genesis is not archived");
                for e in p["entries"].as_array().expect("entries") {
                    assert_eq!(e["genesis"], p["genesis"], "{name} on {active}: archived entry {e}");
                }
            }
        }
    }
}
