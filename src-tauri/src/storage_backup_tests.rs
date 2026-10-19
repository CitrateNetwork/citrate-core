// K4 — verified export, backup and restore of Files and Memory. Included into
// `storage_backup::tests` (private access).
//
// Files come from an in-memory kubo (the same shape as storage_tests' fake); Memory is a real
// directory on disk standing in for the mem-mcp store; the daemon is a fake that records stop and
// start. Every test works in its own temp dir.

use super::*;
use crate::storage::{AddOutcome, KuboTransport, StorageError, StorageManager, LOCAL_PIN_MARKER};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------- fakes

/// An in-memory kubo: content-addressed blobs + a recursive pin set.
struct FakeKubo {
    blobs: Mutex<BTreeMap<String, Vec<u8>>>,
    pins: Mutex<BTreeSet<String>>,
    /// When set, `add` answers this CID instead of the content CID (a CID-version change).
    force_cid: Mutex<Option<String>>,
}
impl FakeKubo {
    fn new() -> Arc<Self> {
        Arc::new(FakeKubo {
            blobs: Mutex::new(BTreeMap::new()),
            pins: Mutex::new(BTreeSet::new()),
            force_cid: Mutex::new(None),
        })
    }
    fn cid_for(bytes: &[u8]) -> String {
        let h = sha256_hex(bytes);
        format!("bafyfake{}", &h[..24])
    }
    /// Put an object in the store and pin it (what `ipfs add` does by default).
    fn put_pinned(&self, bytes: &[u8]) -> String {
        let cid = Self::cid_for(bytes);
        self.blobs.lock().unwrap().insert(cid.clone(), bytes.to_vec());
        self.pins.lock().unwrap().insert(cid.clone());
        cid
    }
}
struct SharedKubo(Arc<FakeKubo>);
impl KuboTransport for SharedKubo {
    fn add(&self, _filename: &str, bytes: &[u8]) -> std::result::Result<AddOutcome, StorageError> {
        let cid = self
            .0
            .force_cid
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| FakeKubo::cid_for(bytes));
        self.0.blobs.lock().unwrap().insert(cid.clone(), bytes.to_vec());
        Ok(AddOutcome {
            cid,
            size_bytes: bytes.len() as u64,
        })
    }
    fn pin_add(&self, cid: &str) -> std::result::Result<(), StorageError> {
        self.0.pins.lock().unwrap().insert(cid.to_string());
        Ok(())
    }
    fn pin_rm(&self, cid: &str) -> std::result::Result<(), StorageError> {
        self.0.pins.lock().unwrap().remove(cid);
        Ok(())
    }
    fn pin_ls(&self) -> std::result::Result<Vec<String>, StorageError> {
        Ok(self.0.pins.lock().unwrap().iter().cloned().collect())
    }
    fn cat(&self, cid: &str) -> std::result::Result<Vec<u8>, StorageError> {
        self.0
            .blobs
            .lock()
            .unwrap()
            .get(cid)
            .cloned()
            .ok_or_else(|| StorageError::Transport(format!("no such object: {cid}")))
    }
}

/// A fake memory daemon: records stops and starts.
struct FakeDaemon {
    running: Mutex<bool>,
    stops: AtomicUsize,
    starts: AtomicUsize,
    /// While the daemon "runs", it must not see the store change under it.
    store_dir: PathBuf,
    /// Set when a write to the store happened while the daemon was running.
    touched_while_running: Mutex<bool>,
}
impl FakeDaemon {
    fn new(running: bool, store_dir: &Path) -> Self {
        FakeDaemon {
            running: Mutex::new(running),
            stops: AtomicUsize::new(0),
            starts: AtomicUsize::new(0),
            store_dir: store_dir.to_path_buf(),
            touched_while_running: Mutex::new(false),
        }
    }
}
impl DaemonControl for FakeDaemon {
    fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }
    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        *self.running.lock().unwrap() = false;
    }
    fn start(&self) -> std::result::Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        *self.running.lock().unwrap() = true;
        Ok(())
    }
}
impl FakeDaemon {
    fn note_access(&self) {
        if *self.running.lock().unwrap() && self.store_dir.exists() {
            *self.touched_while_running.lock().unwrap() = true;
        }
    }
}

// ---------------------------------------------------------------- fixtures

fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "citrate-k4-backup-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A memory dir shaped like the real one: a RocksDB-like store dir, sidecar markers, the live
/// socket (never backed up) and the crash log (never backed up).
fn seed_memory_dir(root: &Path) -> PathBuf {
    let mem = root.join("memory");
    let store = mem.join("store.bge.memdag");
    std::fs::create_dir_all(store.join("sub")).unwrap();
    std::fs::write(store.join("000123.sst"), vec![7u8; 4096]).unwrap();
    std::fs::write(store.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
    std::fs::write(store.join("LOCK"), b"").unwrap();
    std::fs::write(store.join("sub").join("OPTIONS-000007"), b"[Version]\n").unwrap();
    std::fs::write(mem.join("docs-corpus.seeded"), b"abc\ndef\n").unwrap();
    std::fs::write(mem.join("memdag.sock"), b"").unwrap();
    std::fs::write(mem.join("crash-records.jsonl"), b"{}\n").unwrap();
    mem
}

fn files_manager(root: &Path, kubo: Arc<FakeKubo>) -> StorageManager {
    StorageManager::new(Box::new(SharedKubo(kubo)), root.join("ipfs"))
}

fn no_progress() -> impl FnMut(&Progress) {
    |_p: &Progress| {}
}

/// Export both parts from a seeded root into `dest`; returns (report, kubo, mem dir, daemon).
fn export_both(tag: &str) -> (ExportReport, Arc<FakeKubo>, PathBuf, PathBuf) {
    let root = tmp_dir(tag);
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let kubo = FakeKubo::new();
    kubo.put_pinned(b"a first file");
    kubo.put_pinned(b"a second, longer file with more bytes");
    let mem = seed_memory_dir(&root);
    let mgr = files_manager(&root, kubo.clone());
    let daemon = FakeDaemon::new(true, &mem.join("store.bge.memdag"));
    let src = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    let report = export(
        &dest,
        &[Part::Files, Part::Memory],
        Some(&mgr),
        Some(&src),
        1_760_000_000,
        &mut no_progress(),
    )
    .expect("export");
    (report, kubo, mem, root)
}

// ---------------------------------------------------------------- export

#[test]
fn export_writes_a_manifest_with_sha256_and_size_for_every_file() {
    let (report, _kubo, _mem, _root) = export_both("export-manifest");
    let folder = PathBuf::from(&report.folder);
    let m = read_manifest(&folder).expect("manifest");
    assert_eq!(m.format, FORMAT);
    assert_eq!(m.parts, vec![Part::Files, Part::Memory]);
    assert_eq!(report.files, 2);
    // store: 000123.sst, CURRENT, LOCK, sub/OPTIONS; plus docs-corpus.seeded.
    assert_eq!(report.memory_files, 5);
    assert_eq!(m.entries.len(), 7);
    for e in &m.entries {
        let bytes = std::fs::read(folder.join(&e.path)).unwrap();
        assert_eq!(e.size, bytes.len() as u64, "{}", e.path);
        assert_eq!(e.sha256, sha256_hex(&bytes), "{}", e.path);
    }
    // Files carry their CID; the file name under files/ is the CID.
    for e in m.entries.iter().filter(|e| e.path.starts_with("files/")) {
        let cid = e.cid.as_deref().expect("file entry has a cid");
        assert_eq!(e.path, format!("files/{cid}"));
    }
    // Entries are sorted, so the same data always gives the same manifest.
    let mut sorted = m.entries.clone();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(sorted, m.entries);
    assert_eq!(m.entries_sha256, entries_digest(&m.entries));
    assert_eq!(report.total_bytes, m.entries.iter().map(|e| e.size).sum::<u64>());
}

#[test]
fn export_never_copies_the_live_socket_or_the_crash_log() {
    let (report, _kubo, _mem, _root) = export_both("export-exclude");
    let m = read_manifest(Path::new(&report.folder)).unwrap();
    assert!(m.entries.iter().all(|e| !e.path.ends_with("memdag.sock")));
    assert!(m.entries.iter().all(|e| !e.path.ends_with("crash-records.jsonl")));
    assert!(m
        .entries
        .iter()
        .any(|e| e.path == "memory/store.bge.memdag/sub/OPTIONS-000007"));
}

#[test]
fn export_stops_a_running_memory_daemon_and_restarts_it() {
    let root = tmp_dir("export-daemon");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let mem = seed_memory_dir(&root);
    let daemon = FakeDaemon::new(true, &mem);
    let src = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    let mut seen_running_during_copy = false;
    let mut on = |p: &Progress| {
        if p.phase == "memory" {
            if daemon.is_running() {
                seen_running_during_copy = true;
            }
            daemon.note_access();
        }
    };
    let report = export(&dest, &[Part::Memory], None, Some(&src), 1, &mut on).unwrap();
    assert!(!seen_running_during_copy, "the store is copied with the daemon stopped");
    assert!(!*daemon.touched_while_running.lock().unwrap());
    assert_eq!(daemon.stops.load(Ordering::SeqCst), 1);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), 1);
    assert!(daemon.is_running(), "a daemon that was running is running again");
    assert!(report.daemon_restarted);
}

#[test]
fn export_leaves_a_stopped_daemon_stopped() {
    let root = tmp_dir("export-daemon-off");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let mem = seed_memory_dir(&root);
    let daemon = FakeDaemon::new(false, &mem);
    let src = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    let report = export(&dest, &[Part::Memory], None, Some(&src), 1, &mut no_progress()).unwrap();
    assert_eq!(daemon.starts.load(Ordering::SeqCst), 0);
    assert!(!daemon.is_running());
    assert!(!report.daemon_restarted);
}

#[test]
fn export_reports_progress_for_each_file_in_order() {
    let root = tmp_dir("export-progress");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let kubo = FakeKubo::new();
    kubo.put_pinned(b"one");
    kubo.put_pinned(b"two!");
    let mgr = files_manager(&root, kubo);
    let mut events: Vec<Progress> = Vec::new();
    let mut on = |p: &Progress| events.push(p.clone());
    export(&dest, &[Part::Files], Some(&mgr), None, 1, &mut on).unwrap();
    let files: Vec<&Progress> = events.iter().filter(|p| p.phase == "files").collect();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].done, 1);
    assert_eq!(files[1].done, 2);
    assert!(files.iter().all(|p| p.total == 2 && p.op == "export"));
    // The export ends by verifying what it wrote.
    assert!(events.iter().any(|p| p.phase == "verify"));
    assert_eq!(events.last().map(|p| p.phase), Some("done"));
}

#[test]
fn export_skips_unpinned_index_rows_and_says_why() {
    let root = tmp_dir("export-skip");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let kubo = FakeKubo::new();
    let mgr = files_manager(&root, kubo.clone());
    // Added then unpinned: in the index, not held by this node.
    let f = root.join("gone.txt");
    std::fs::write(&f, b"no longer pinned").unwrap();
    let out = mgr.add_file(&f).unwrap();
    kubo.pins.lock().unwrap().remove(&out.cid);
    kubo.put_pinned(b"still here");
    let report = export(&dest, &[Part::Files], Some(&mgr), None, 1, &mut no_progress()).unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].item, out.cid);
    assert!(report.skipped[0].reason.contains("not pinned"));
}

#[test]
fn export_never_overwrites_and_refuses_a_bad_destination() {
    let root = tmp_dir("export-dest");
    let mem = seed_memory_dir(&root);
    let daemon = FakeDaemon::new(false, &mem);
    let src = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    // Relative path.
    let e = export(Path::new("relative/dir"), &[Part::Memory], None, Some(&src), 1, &mut no_progress());
    assert!(matches!(e, Err(BackupError::InvalidFolder(_))), "{e:?}");
    // Missing folder.
    let e = export(&root.join("missing"), &[Part::Memory], None, Some(&src), 1, &mut no_progress());
    assert!(matches!(e, Err(BackupError::InvalidFolder(_))), "{e:?}");
    // Inside the memory store itself (would copy into what it reads).
    let e = export(&mem, &[Part::Memory], None, Some(&src), 1, &mut no_progress());
    assert!(matches!(e, Err(BackupError::InvalidFolder(_))), "{e:?}");
    // No part chosen.
    let e = export(&root, &[], None, Some(&src), 1, &mut no_progress());
    assert!(matches!(e, Err(BackupError::InvalidFolder(_))), "{e:?}");
    // Two exports in the same second never share a folder.
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let a = export(&dest, &[Part::Memory], None, Some(&src), 7, &mut no_progress()).unwrap();
    let b = export(&dest, &[Part::Memory], None, Some(&src), 7, &mut no_progress()).unwrap();
    assert_ne!(a.folder, b.folder);
}

#[test]
fn backup_folder_name_is_dated_utc() {
    // 2026-10-19 00:00:00 UTC
    assert_eq!(backup_folder_name(1_792_368_000), "citrate-backup-20261019-000000");
    assert_eq!(backup_folder_name(0), "citrate-backup-19700101-000000");
    assert_eq!(backup_folder_name(951_782_400), "citrate-backup-20000229-000000");
}

// ---------------------------------------------------------------- verify

#[test]
fn verify_accepts_an_untouched_backup() {
    let (report, _kubo, _mem, _root) = export_both("verify-ok");
    let v = verify_folder(Path::new(&report.folder)).expect("verifies");
    assert_eq!(v.entries.len(), 7);
    let r = verify_report(Path::new(&report.folder));
    assert!(r.ok, "{r:?}");
    assert_eq!(r.file_count, 7);
    assert!(r.mismatches.is_empty());
}

#[test]
fn verify_refuses_a_changed_byte() {
    let (report, _kubo, _mem, _root) = export_both("verify-byte");
    let folder = PathBuf::from(&report.folder);
    let target = folder.join("memory/store.bge.memdag/000123.sst");
    let mut bytes = std::fs::read(&target).unwrap();
    bytes[100] ^= 0x01; // same size, different content
    std::fs::write(&target, &bytes).unwrap();
    match verify_folder(&folder) {
        Err(BackupError::Mismatch(list)) => {
            assert_eq!(
                list,
                vec![Mismatch::HashDiffers {
                    path: "memory/store.bge.memdag/000123.sst".into()
                }]
            );
        }
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

#[test]
fn verify_refuses_missing_resized_and_extra_files() {
    let (report, _kubo, _mem, _root) = export_both("verify-shape");
    let folder = PathBuf::from(&report.folder);
    std::fs::remove_file(folder.join("memory/docs-corpus.seeded")).unwrap();
    std::fs::write(folder.join("memory/store.bge.memdag/CURRENT"), b"x").unwrap();
    std::fs::write(folder.join("memory/planted.bin"), b"not in the manifest").unwrap();
    let Err(BackupError::Mismatch(list)) = verify_folder(&folder) else {
        panic!("expected a mismatch");
    };
    assert!(list.contains(&Mismatch::Missing {
        path: "memory/docs-corpus.seeded".into()
    }));
    assert!(list.contains(&Mismatch::SizeDiffers {
        path: "memory/store.bge.memdag/CURRENT".into(),
        expected: 16,
        actual: 1,
    }));
    assert!(list.contains(&Mismatch::Unlisted {
        path: "memory/planted.bin".into()
    }));
    let r = verify_report(&folder);
    assert!(!r.ok);
    assert_eq!(r.mismatches.len(), 3);
}

#[test]
fn verify_refuses_an_edited_manifest() {
    let (report, _kubo, _mem, _root) = export_both("verify-manifest");
    let folder = PathBuf::from(&report.folder);
    let mut m = read_manifest(&folder).unwrap();
    m.entries.pop(); // drop an entry without recomputing the digest
    std::fs::write(folder.join(MANIFEST_NAME), serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(matches!(verify_folder(&folder), Err(BackupError::Manifest(_))));
    // A wrong format is refused too.
    let mut m = read_manifest(&folder).unwrap();
    m.format = "something-else/9".into();
    std::fs::write(folder.join(MANIFEST_NAME), serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(matches!(verify_folder(&folder), Err(BackupError::Manifest(_))));
    // No manifest at all.
    std::fs::remove_file(folder.join(MANIFEST_NAME)).unwrap();
    assert!(matches!(verify_folder(&folder), Err(BackupError::Manifest(_))));
}

#[test]
fn verify_refuses_unsafe_manifest_paths() {
    let root = tmp_dir("verify-unsafe");
    for bad in [
        "../escape",
        "/etc/passwd",
        "memory/../../escape",
        "memory/./x",
        "memory//x",
        "memory\\x",
        "other/x",
        "files/not-a-cid!",
        "files/sub/dir",
        "",
    ] {
        let entries = vec![ManifestEntry {
            path: bad.to_string(),
            size: 0,
            sha256: sha256_hex(b""),
            cid: None,
        }];
        let m = Manifest {
            format: FORMAT.into(),
            created_at: 1,
            app_version: "test".into(),
            parts: vec![Part::Memory],
            entries_sha256: entries_digest(&entries),
            entries,
        };
        std::fs::write(root.join(MANIFEST_NAME), serde_json::to_vec(&m).unwrap()).unwrap();
        let Err(BackupError::Mismatch(list)) = verify_folder(&root) else {
            panic!("{bad:?} must be refused");
        };
        assert!(
            list.iter().any(|x| matches!(x, Mismatch::UnsafePath { .. })),
            "{bad:?}: {list:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn verify_refuses_a_symlink_in_place_of_a_file() {
    let (report, _kubo, _mem, root) = export_both("verify-symlink");
    let folder = PathBuf::from(&report.folder);
    let target = folder.join("memory/docs-corpus.seeded");
    let outside = root.join("outside.txt");
    std::fs::write(&outside, b"abc\ndef\n").unwrap(); // same bytes as the real file
    std::fs::remove_file(&target).unwrap();
    std::os::unix::fs::symlink(&outside, &target).unwrap();
    let Err(BackupError::Mismatch(list)) = verify_folder(&folder) else {
        panic!("a symlink must be refused");
    };
    assert!(list.contains(&Mismatch::NotAFile {
        path: "memory/docs-corpus.seeded".into()
    }));
}

// ---------------------------------------------------------------- restore

#[test]
fn restore_memory_replaces_the_store_and_keeps_the_previous_copy() {
    let (report, _kubo, mem, _root) = export_both("restore-mem");
    // The live store moves on after the backup.
    std::fs::write(mem.join("store.bge.memdag/000123.sst"), b"newer state").unwrap();
    std::fs::write(mem.join("store.bge.memdag/000999.sst"), b"newer file").unwrap();
    let daemon = FakeDaemon::new(true, &mem);
    let target = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    let r = restore(
        Path::new(&report.folder),
        &[Part::Memory],
        None,
        Some(&target),
        42,
        &mut no_progress(),
    )
    .expect("restore");
    assert_eq!(r.memory_files_restored, 5);
    assert_eq!(
        std::fs::read(mem.join("store.bge.memdag/000123.sst")).unwrap(),
        vec![7u8; 4096]
    );
    assert!(!mem.join("store.bge.memdag/000999.sst").exists());
    // The replaced store is kept, not deleted.
    let kept = PathBuf::from(r.previous_memory_kept_at.expect("previous copy kept"));
    assert_eq!(
        std::fs::read(kept.join("store.bge.memdag/000123.sst")).unwrap(),
        b"newer state"
    );
    // Daemon: stopped for the swap, running again after.
    assert_eq!(daemon.stops.load(Ordering::SeqCst), 1);
    assert!(daemon.is_running());
    assert!(r.daemon_restarted);
}

#[test]
fn restore_refuses_a_mismatch_and_touches_nothing() {
    let (report, kubo, mem, _root) = export_both("restore-refuse");
    let folder = PathBuf::from(&report.folder);
    // Tamper with one memory file and one Files file.
    std::fs::write(folder.join("memory/docs-corpus.seeded"), b"abc\nXYZ\n").unwrap();
    let before_mem = std::fs::read(mem.join("docs-corpus.seeded")).unwrap();
    std::fs::write(mem.join("store.bge.memdag/000123.sst"), b"live state").unwrap();
    let daemon = FakeDaemon::new(true, &mem);
    let target = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    let pins_before = kubo.pins.lock().unwrap().clone();
    let root2 = tmp_dir("restore-refuse-files");
    let mgr = files_manager(&root2, kubo.clone());
    let r = restore(
        &folder,
        &[Part::Files, Part::Memory],
        Some(&mgr),
        Some(&target),
        42,
        &mut no_progress(),
    );
    let Err(BackupError::Mismatch(list)) = r else {
        panic!("restore must refuse, got {r:?}");
    };
    assert_eq!(
        list,
        vec![Mismatch::HashDiffers {
            path: "memory/docs-corpus.seeded".into()
        }]
    );
    // Nothing touched: the live store, the daemon, and kubo are as they were.
    assert_eq!(std::fs::read(mem.join("docs-corpus.seeded")).unwrap(), before_mem);
    assert_eq!(
        std::fs::read(mem.join("store.bge.memdag/000123.sst")).unwrap(),
        b"live state"
    );
    assert_eq!(daemon.stops.load(Ordering::SeqCst), 0);
    assert!(daemon.is_running());
    assert_eq!(*kubo.pins.lock().unwrap(), pins_before);
    // No staging or kept dirs were left beside the store.
    let siblings: Vec<String> = std::fs::read_dir(mem.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("memory."))
        .collect();
    assert!(siblings.is_empty(), "{siblings:?}");
}

#[test]
fn restore_files_re_adds_and_pins_locally_with_no_bond() {
    let (report, _kubo, _mem, _root) = export_both("restore-files");
    // Restore into a fresh node with an empty kubo.
    let fresh_root = tmp_dir("restore-files-fresh");
    let fresh = FakeKubo::new();
    let mgr = files_manager(&fresh_root, fresh.clone());
    let r = restore(
        Path::new(&report.folder),
        &[Part::Files],
        Some(&mgr),
        None,
        42,
        &mut no_progress(),
    )
    .expect("restore files");
    assert_eq!(r.files_restored, 2);
    assert!(r.cids_changed.is_empty());
    let rows = mgr.list().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.pin_state, crate::storage::PinState::Pinned);
        assert_eq!(row.bond_salt, LOCAL_PIN_MARKER, "a restored pin is local, never bonded");
    }
}

#[test]
fn restore_files_reports_a_changed_cid() {
    let (report, _kubo, _mem, _root) = export_both("restore-cid");
    let fresh_root = tmp_dir("restore-cid-fresh");
    let fresh = FakeKubo::new();
    *fresh.force_cid.lock().unwrap() = Some("bafyotherversion".into());
    let mgr = files_manager(&fresh_root, fresh);
    let r = restore(
        Path::new(&report.folder),
        &[Part::Files],
        Some(&mgr),
        None,
        42,
        &mut no_progress(),
    )
    .expect("restore");
    assert_eq!(r.cids_changed.len(), 2);
    assert!(r.cids_changed.iter().all(|c| c.actual == "bafyotherversion"));
}

#[test]
fn restore_refuses_a_part_the_backup_does_not_have() {
    let root = tmp_dir("restore-part");
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let kubo = FakeKubo::new();
    kubo.put_pinned(b"only files here");
    let mgr = files_manager(&root, kubo);
    let report = export(&dest, &[Part::Files], Some(&mgr), None, 1, &mut no_progress()).unwrap();
    let mem = seed_memory_dir(&root);
    let daemon = FakeDaemon::new(false, &mem);
    let target = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    let r = restore(
        Path::new(&report.folder),
        &[Part::Memory],
        None,
        Some(&target),
        42,
        &mut no_progress(),
    );
    assert!(matches!(r, Err(BackupError::InvalidFolder(_))), "{r:?}");
}

#[test]
fn restore_reports_progress_and_ends_with_done() {
    let (report, _kubo, mem, _root) = export_both("restore-progress");
    let daemon = FakeDaemon::new(false, &mem);
    let target = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
    };
    let mut events: Vec<Progress> = Vec::new();
    let mut on = |p: &Progress| events.push(p.clone());
    restore(Path::new(&report.folder), &[Part::Memory], None, Some(&target), 42, &mut on).unwrap();
    assert_eq!(events.first().map(|p| p.phase), Some("verify"));
    assert!(events.iter().filter(|p| p.phase == "memory").count() == 5);
    assert!(events.iter().all(|p| p.op == "restore"));
    assert_eq!(events.last().map(|p| p.phase), Some("done"));
}

#[test]
fn backup_error_text_names_the_problem_without_paths_outside_the_backup() {
    let e = BackupError::Mismatch(vec![
        Mismatch::HashDiffers {
            path: "memory/a".into(),
        },
        Mismatch::Missing {
            path: "files/bafyx".into(),
        },
    ]);
    let s = e.to_string();
    assert!(s.contains("does not match its manifest"), "{s}");
    assert!(s.contains("memory/a"), "{s}");
    assert!(s.contains("files/bafyx"), "{s}");
}

// ---------------------------------------------------------------- wiring tripwires

#[test]
fn backup_commands_are_registered_and_allowed_for_the_main_window() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in [
        "storage_backup_export",
        "storage_backup_verify",
        "storage_backup_restore",
        "storage_pin_local",
        "storage_pin_params",
    ] {
        assert!(lib.contains(&format!("::{cmd},")), "{cmd} registered in generate_handler!");
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} in main-window.toml");
    }
}

/// K4 (4): every long-running storage process is started through the supervisor, stopped on app
/// quit, and covered by the launch-time orphan sweep. Source tripwire (no binaries in CI).
#[test]
fn storage_daemons_run_under_the_supervisor_and_stop_on_quit() {
    let ipfs = include_str!("ipfs.rs");
    let memory = include_str!("memory.rs");
    let lib = include_str!("lib.rs");
    for (name, src) in [("ipfs.rs", ipfs), ("memory.rs", memory)] {
        assert!(
            src.contains("Supervisor::start(config)"),
            "{name}: the daemon starts through the supervisor"
        );
    }
    // The kubo daemon itself is never spawned directly: the only direct Commands in ipfs.rs are the
    // one-shot `init` / `config` calls, both waited for with `.output()`.
    assert_eq!(ipfs.matches("Command::new").count(), 2, "ipfs.rs direct spawns");
    assert_eq!(ipfs.matches(".output()").count(), 2, "ipfs.rs one-shots are waited for");
    let quit = lib
        .split("pub(crate) fn shutdown_all_sidecars")
        .nth(1)
        .and_then(|s| s.split("\n}\n").next())
        .expect("shutdown_all_sidecars exists");
    assert!(quit.contains("ipfs::IpfsState"), "kubo stops on quit");
    assert!(quit.contains("memory::MemoryState"), "mem-mcp stops on quit");
    let owned = lib
        .split("const OWNED_SIDECARS")
        .nth(1)
        .and_then(|s| s.split("];").next())
        .expect("OWNED_SIDECARS exists");
    assert!(owned.contains("\"ipfs\"") && owned.contains("\"mem-mcp\""));
}

/// K4 follow-up (DGX review of #368): until S2.2 ships, no path can open a pin-bond signing
/// request. The bonded `storage_pin` command is neither registered nor allowed for any window,
/// and no command in storage.rs builds a bond transaction.
#[test]
fn the_bonded_storage_pin_command_is_not_invocable() {
    let lib = include_str!("lib.rs");
    let handler = lib
        .split("generate_handler![")
        .nth(1)
        .and_then(|s| s.split("])").next())
        .expect("generate_handler! exists");
    assert!(
        !handler.contains("storage::storage_pin,"),
        "storage_pin must not be registered"
    );
    let acl = include_str!("../permissions/main-window.toml");
    assert!(!acl.contains("\"storage_pin\""), "storage_pin must not be allowed");
    for cap in [
        include_str!("../capabilities/default.json"),
        include_str!("../capabilities/popout.json"),
        include_str!("../capabilities/popout-contract.json"),
    ] {
        assert!(!cap.contains("\"storage_pin\""), "no capability allows storage_pin");
    }
    let storage = include_str!("storage.rs");
    assert!(!storage.contains("pub async fn storage_pin("), "no bonded pin command");
    assert!(!storage.contains("ceremony.0.request("), "storage.rs opens no signing request");
    assert!(!storage.contains("IntentKind::Transaction"), "storage.rs builds no transaction");
    // The local pin is still there and still allowed.
    assert!(handler.contains("storage::storage_pin_local,"));
    assert!(acl.contains("\"storage_pin_local\""));
}
