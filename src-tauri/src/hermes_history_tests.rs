// 0.5.4 — the sealed Hermes chat history (ML-KEM-768 + HKDF-SHA256 + AES-256-GCM envelope, key in
// the OS keyring). Pure: temp dirs and an in-memory keyring; nothing reaches the OS keychain.
use super::*;
use crate::custody::CustodyError;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "citrate-hhist-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        TestDir(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct MemKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
}

impl MemKeyring {
    fn raw(&self) -> Option<Vec<u8>> {
        self.store.lock().unwrap().get(KEY_ACCOUNT).cloned()
    }
    fn put_raw(&self, v: Vec<u8>) {
        self.store
            .lock()
            .unwrap()
            .insert(KEY_ACCOUNT.to_string(), v);
    }
}

impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

struct DeadKeyring;

impl Keyring for DeadKeyring {
    fn get(&self, _a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
}

const SECRET: &str = "my validator bond unlocks at block 991234, tell nobody";

fn doc(id: &str, updated_at: u64, text: &str) -> ThreadDoc {
    ThreadDoc {
        id: id.to_string(),
        title: format!("title of {id}: {text}"),
        created_at: updated_at,
        updated_at,
        msgs: serde_json::json!([
            {"id": "m1", "who": "You", "text": text, "chips": [], "streaming": false},
            {"id": "m2", "who": "Agent", "text": format!("reply to {text}"), "chips": [], "streaming": false}
        ]),
    }
}

fn all_bytes(dir: &std::path::Path) -> Vec<u8> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.filter_map(Result::ok) {
            if let Ok(b) = std::fs::read(e.path()) {
                out.extend_from_slice(&b);
            }
        }
    }
    out
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn sealed_path(dir: &std::path::Path, id: &str) -> std::path::PathBuf {
    dir.join(format!("t-{id}.sealed"))
}

fn ids(l: &HistoryList) -> Vec<String> {
    l.threads.iter().map(|t| t.id.clone()).collect()
}

#[test]
fn a_conversation_round_trips_through_the_sealed_store() {
    let d = TestDir::new("rt");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    assert!(s.list_threads(0).unwrap().threads.is_empty());
    assert!(kr.raw().is_none(), "listing an empty history mints no key");
    s.save_thread(doc("a1", 10, SECRET)).unwrap();
    s.save_thread(doc("b2", 20, "second")).unwrap();
    let l = s.list_threads(30).unwrap();
    assert_eq!(ids(&l), vec!["b2", "a1"], "most recently updated first");
    assert_eq!(l.threads[1], doc("a1", 10, SECRET));
    assert!(l.unreadable.is_empty());
    // Saving again replaces the conversation.
    s.save_thread(doc("a1", 40, "edited")).unwrap();
    let l = s.list_threads(50).unwrap();
    assert_eq!(ids(&l), vec!["a1", "b2"]);
    assert_eq!(l.threads[0], doc("a1", 40, "edited"));
}

#[test]
fn the_key_is_an_ml_kem_seed_in_the_keyring_and_never_on_disk() {
    let d = TestDir::new("keyshape");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    let raw = kr.raw().expect("key minted on first save");
    assert_eq!(raw.len(), KEY_ONE_LEN);
    assert_eq!(&raw[..4], KEY_MAGIC);
    let seed = &raw[8..8 + SEED_LEN];
    assert!(!contains(&all_bytes(d.path()), seed), "the seed never touches the folder");
    // The file carries an ML-KEM-768 ciphertext (1088 bytes) as its first envelope part.
    let bytes = std::fs::read(sealed_path(d.path(), "a1")).unwrap();
    let mut i = HEADER_LEN;
    assert_eq!(take_part(&bytes, &mut i).unwrap().len(), 1088);
}

#[test]
fn nothing_the_member_wrote_is_on_disk_in_plaintext() {
    let d = TestDir::new("plain");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.set_retention_days(Some(30), false).unwrap();
    let disk = all_bytes(d.path());
    assert!(!disk.is_empty());
    for needle in [SECRET, "validator bond", "title of a1", "reply to", "\"who\"", "retentionDays"] {
        assert!(!contains(&disk, needle.as_bytes()), "{needle:?} is on disk in plaintext");
    }
}

#[test]
fn files_are_owner_only() {
    let d = TestDir::new("modes");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, "x")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = std::fs::metadata(sealed_path(d.path(), "a1")).unwrap().permissions().mode() & 0o777;
        let dir = std::fs::metadata(d.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(f, 0o600);
        assert_eq!(dir, 0o700);
    }
}

#[test]
fn a_changed_byte_anywhere_in_a_file_fails_closed() {
    let d = TestDir::new("tamper");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    let path = sealed_path(d.path(), "a1");
    let good = std::fs::read(&path).unwrap();
    // The header, the KEM ciphertext, the wrapped key and the payload: each byte position class.
    let len = good.len();
    for at in [9, HEADER_LEN + 4, HEADER_LEN + 600, len - 200, len - 1] {
        let mut bad = good.clone();
        bad[at] ^= 0x01;
        std::fs::write(&path, &bad).unwrap();
        let l = s.list_threads(2).unwrap();
        assert!(l.threads.is_empty(), "byte {at}: a tampered file must not open");
        assert_eq!(l.unreadable, vec!["a1"], "byte {at}");
        // And it is never silently overwritten.
        assert!(s.save_thread(doc("a1", 3, "overwrite")).is_err(), "byte {at}");
        assert_eq!(std::fs::read(&path).unwrap(), bad);
    }
    // A truncated file fails closed too.
    std::fs::write(&path, &good[..len / 2]).unwrap();
    assert_eq!(s.list_threads(2).unwrap().unreadable, vec!["a1"]);
    std::fs::write(&path, &good).unwrap();
    assert_eq!(ids(&s.list_threads(2).unwrap()), vec!["a1"]);
}

#[test]
fn a_file_copied_to_another_conversations_name_does_not_open() {
    let d = TestDir::new("moved");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.save_thread(doc("b2", 2, "other")).unwrap();
    std::fs::copy(sealed_path(d.path(), "a1"), sealed_path(d.path(), "b2")).unwrap();
    let l = s.list_threads(3).unwrap();
    assert_eq!(ids(&l), vec!["a1"]);
    assert_eq!(l.unreadable, vec!["b2"]);
}

#[test]
fn the_wrong_key_cannot_open_the_history() {
    let d = TestDir::new("wrongkey");
    let kr = MemKeyring::default();
    HistoryStore::new(d.path(), &kr).save_thread(doc("a1", 1, SECRET)).unwrap();
    // Another device's key (same generation number, different seed).
    let other = MemKeyring::default();
    other.put_raw(
        KeySet {
            cur: KeyGen::mint(1),
            prev: None,
        }
        .encode()
        .to_vec(),
    );
    let l = HistoryStore::new(d.path(), &other).list_threads(2).unwrap();
    assert!(l.threads.is_empty());
    assert_eq!(l.unreadable, vec!["a1"]);
}

#[test]
fn a_missing_key_with_history_present_fails_closed_and_never_mints_over_it() {
    let d = TestDir::new("nokey");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    kr.delete(KEY_ACCOUNT).unwrap();
    let e = s.list_threads(2).unwrap_err();
    assert!(e.contains("key is not in the keychain"), "{e}");
    assert!(s.save_thread(doc("b2", 2, "new")).is_err());
    assert!(s.set_retention_days(Some(5), false).is_err());
    assert!(kr.raw().is_none(), "no new key was minted over the old history");
    assert!(sealed_path(d.path(), "a1").exists(), "nothing was removed");
    // Delete all is the way out, and the history starts over cleanly.
    s.erase_all().unwrap();
    assert!(s.list_threads(3).unwrap().threads.is_empty());
    s.save_thread(doc("c3", 3, "fresh")).unwrap();
    assert_eq!(ids(&s.list_threads(4).unwrap()), vec!["c3"]);
}

#[test]
fn an_unreachable_or_malformed_keyring_refuses_every_operation() {
    let d = TestDir::new("deadkr");
    let s = HistoryStore::new(d.path(), &DeadKeyring);
    assert!(s.list_threads(1).is_err());
    assert!(s.save_thread(doc("a1", 1, "x")).is_err());
    assert!(s.set_retention_days(None, false).is_err());
    assert!(!sealed_path(d.path(), "a1").exists());

    let kr = MemKeyring::default();
    kr.put_raw(vec![1, 2, 3]);
    let s = HistoryStore::new(d.path(), &kr);
    assert!(s.list_threads(1).unwrap_err().contains("malformed"));
    assert!(s.save_thread(doc("a1", 1, "x")).is_err());
}

#[test]
fn delete_rotates_the_key_so_an_old_copy_of_the_conversation_never_opens() {
    let d = TestDir::new("erase");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.save_thread(doc("b2", 2, "keep me")).unwrap();
    // The bytes of a1 "survive" (a snapshot, a backup, blocks the SSD kept).
    let survived = std::fs::read(sealed_path(d.path(), "a1")).unwrap();
    let old_key = kr.raw().unwrap();
    s.erase_thread("a1").unwrap();
    assert!(!sealed_path(d.path(), "a1").exists());
    let new_key = kr.raw().unwrap();
    assert_ne!(new_key, old_key, "the key was rotated");
    assert_eq!(new_key.len(), KEY_ONE_LEN, "the previous key is gone from the keyring");
    assert_eq!(u32::from_be_bytes(new_key[4..8].try_into().unwrap()), 2);
    // The kept conversation was resealed and still opens.
    assert_eq!(ids(&s.list_threads(3).unwrap()), vec!["b2"]);
    // Put the surviving copy back: no key on the device opens it.
    std::fs::write(sealed_path(d.path(), "a1"), &survived).unwrap();
    let l = s.list_threads(3).unwrap();
    assert_eq!(ids(&l), vec!["b2"]);
    assert_eq!(l.unreadable, vec!["a1"]);
    // Deleting something that is not there changes nothing.
    let before = kr.raw();
    s.erase_thread("zz").unwrap();
    assert_eq!(kr.raw(), before);
    assert!(s.erase_thread("../etc").is_err());
}

#[test]
fn an_erase_interrupted_at_each_step_is_finished_at_the_next_open() {
    // After step 2 (files removed, key not rotated yet): the pending marker drives a rotation.
    let d = TestDir::new("crash2");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.save_thread(doc("b2", 2, "keep")).unwrap();
    let survived = std::fs::read(sealed_path(d.path(), "a1")).unwrap();
    let keys = KeySet::decode(&kr.raw().unwrap()).unwrap();
    s.write_erase_marker(&keys, &["t-a1".to_string()]).unwrap();
    std::fs::remove_file(sealed_path(d.path(), "a1")).unwrap();
    let old_key = kr.raw().unwrap();
    assert_eq!(ids(&s.list_threads(3).unwrap()), vec!["b2"]);
    assert_ne!(kr.raw().unwrap(), old_key);
    assert!(!d.path().join(PENDING_FILE).exists());
    std::fs::write(sealed_path(d.path(), "a1"), &survived).unwrap();
    assert_eq!(s.list_threads(3).unwrap().unreadable, vec!["a1"]);

    // After step 3 (keyring holds {new, prev}, files still under the old key).
    let d = TestDir::new("crash3");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("b2", 2, "keep")).unwrap();
    let old = KeySet::decode(&kr.raw().unwrap()).unwrap();
    let old_gen = old.cur.gen;
    kr.put_raw(
        KeySet {
            cur: KeyGen::mint(old.cur.gen + 1),
            prev: Some(old.cur),
        }
        .encode()
        .to_vec(),
    );
    assert_eq!(kr.raw().unwrap().len(), KEY_TWO_LEN);
    assert_eq!(ids(&s.list_threads(3).unwrap()), vec!["b2"], "resealed under the new key");
    assert_eq!(kr.raw().unwrap().len(), KEY_ONE_LEN, "the previous key was dropped");
    let bytes = std::fs::read(sealed_path(d.path(), "b2")).unwrap();
    let current = KeySet::decode(&kr.raw().unwrap()).unwrap().cur.gen;
    assert_eq!(current, old_gen + 1);
    assert_eq!(file_gen(&bytes), Some(current));
}

#[test]
fn delete_all_removes_every_file_and_the_key() {
    let d = TestDir::new("all");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.save_thread(doc("b2", 2, "x")).unwrap();
    s.set_retention_days(Some(7), false).unwrap();
    s.erase_all().unwrap();
    assert!(kr.raw().is_none());
    assert!(all_bytes(d.path()).is_empty());
    assert!(s.list_threads(3).unwrap().threads.is_empty());
}

#[test]
fn the_store_keeps_fifty_conversations_and_erases_the_oldest() {
    let d = TestDir::new("cap");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    for i in 0..MAX_THREADS {
        assert!(s.save_thread(doc(&format!("t{i}"), i as u64 + 1, "x")).unwrap().evicted.is_empty());
    }
    let before = kr.raw().unwrap();
    let out = s.save_thread(doc("newest", 1000, "x")).unwrap();
    assert_eq!(out.evicted, vec!["t0"]);
    assert!(!sealed_path(d.path(), "t0").exists());
    assert_ne!(kr.raw().unwrap(), before, "eviction is an erase too");
    let l = s.list_threads(2000).unwrap();
    assert_eq!(l.threads.len(), MAX_THREADS);
    assert_eq!(l.threads[0].id, "newest");
}

#[test]
fn retention_erases_only_conversations_past_the_setting() {
    let d = TestDir::new("ret");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    let day = MS_PER_DAY;
    let now = 100 * day;
    s.save_thread(doc("old", now - 10 * day, SECRET)).unwrap();
    s.save_thread(doc("new", now - day, "recent")).unwrap();
    // No setting: kept until deleted.
    let l = s.list_threads(now).unwrap();
    assert_eq!(l.retention_days, None);
    assert_eq!(l.threads.len(), 2);
    s.set_retention_days(Some(7), false).unwrap();
    let l = s.list_threads(now).unwrap();
    assert_eq!(l.retention_days, Some(7));
    assert_eq!(l.purged, 1);
    assert_eq!(ids(&l), vec!["new"]);
    assert!(!sealed_path(d.path(), "old").exists());
    assert!(s.set_retention_days(Some(0), false).is_err());
    assert!(s.set_retention_days(Some(MAX_RETENTION_DAYS + 1), false).is_err());
    s.set_retention_days(None, false).unwrap();
    assert_eq!(s.list_threads(now).unwrap().retention_days, None);
}

#[test]
fn bad_input_is_refused_before_anything_is_written() {
    let d = TestDir::new("input");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    for id in ["", "../x", "a/b", "a.b", &"x".repeat(65)] {
        assert!(s.save_thread(doc(id, 1, "x")).is_err(), "{id:?}");
    }
    let mut d2 = doc("a1", 1, "x");
    d2.msgs = serde_json::json!({"not": "a list"});
    assert!(s.save_thread(d2).is_err());
    let mut big = doc("a1", 1, "x");
    big.msgs = serde_json::json!([{"text": "y".repeat(MAX_THREAD_BYTES)}]);
    assert!(s.save_thread(big).is_err());
    assert!(kr.raw().is_none());
    assert!(all_bytes(d.path()).is_empty());
    // Unknown fields are refused by the command's type.
    let v = serde_json::json!({"id": "a", "title": "", "createdAt": 1, "updatedAt": 1, "msgs": [], "apiKey": "x"});
    assert!(serde_json::from_value::<ThreadDoc>(v).is_err());
}

#[test]
fn the_keyring_record_round_trips_and_rejects_garbage() {
    let ks = KeySet {
        cur: KeyGen::mint(7),
        prev: Some(KeyGen::mint(6)),
    };
    let enc = ks.encode();
    let back = KeySet::decode(&enc).unwrap();
    assert_eq!(back.cur.gen, 7);
    assert_eq!(&back.cur.seed[..], &ks.cur.seed[..]);
    assert_eq!(back.prev.as_ref().unwrap().gen, 6);
    let mut bad = enc.to_vec();
    bad[0] ^= 1;
    assert!(KeySet::decode(&bad).is_err());
    assert!(KeySet::decode(&enc[..KEY_ONE_LEN - 1]).is_err());
    // The same seed always yields the same key pair (FIPS 203 seed form), so a seal opens.
    let k = KeyGen::mint(1);
    let set = KeySet { cur: KeyGen { gen: 1, seed: k.seed.clone() }, prev: None };
    let sealed = seal_file(&k, "t-x", b"hello").unwrap();
    assert_eq!(&open_sealed(&set, "t-x", &sealed).unwrap()[..], b"hello");
    assert!(open_sealed(&set, "t-y", &sealed).is_none(), "the name is bound");
}

#[test]
fn the_history_key_is_device_sealed_not_in_the_recovery_kit_and_listed_for_local_delete() {
    let kit = include_str!("recovery_kit.rs");
    assert!(!kit.contains(KEY_ACCOUNT), "the history key must not follow a recovery phrase");
    let local = include_str!("local_data.rs");
    assert!(local.contains(KEY_ACCOUNT), "delete my local data must list the history key");
}

/// Turns the crash injection off again even when the test fails.
struct CrashAfterMarker;
impl CrashAfterMarker {
    fn on() -> Self {
        CRASH_AFTER_MARKER.with(|c| c.set(true));
        CrashAfterMarker
    }
}
impl Drop for CrashAfterMarker {
    fn drop(&mut self) {
        CRASH_AFTER_MARKER.with(|c| c.set(false));
    }
}

#[test]
fn a_crash_after_the_marker_but_before_the_file_is_removed_never_brings_the_conversation_back() {
    // BerryManifold P2 on #679: step 1 done, step 2 not. Recovery must remove the named file
    // before resealing the rest, or the deleted conversation comes back under the new key.
    let d = TestDir::new("crash1");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.save_thread(doc("b2", 2, "keep")).unwrap();
    let survived = std::fs::read(sealed_path(d.path(), "a1")).unwrap();
    let old_key = kr.raw().unwrap();
    {
        let _crash = CrashAfterMarker::on();
        assert!(s.erase_thread("a1").unwrap_err().contains("injected crash"));
    }
    // The crash left exactly that state: the marker, the file still there, the old key.
    assert!(d.path().join(PENDING_FILE).exists());
    assert!(sealed_path(d.path(), "a1").exists());
    assert_eq!(kr.raw().unwrap(), old_key);

    // The next open finishes the erase: a1 stays gone, b2 is resealed, the key is rotated.
    let l = s.list_threads(3).unwrap();
    assert_eq!(ids(&l), vec!["b2"]);
    assert!(l.unreadable.is_empty());
    assert!(!sealed_path(d.path(), "a1").exists());
    assert!(!d.path().join(PENDING_FILE).exists());
    assert_ne!(kr.raw().unwrap(), old_key);
    // And the copy that survived elsewhere opens under no key on the device.
    std::fs::write(sealed_path(d.path(), "a1"), &survived).unwrap();
    let l = s.list_threads(3).unwrap();
    assert_eq!(ids(&l), vec!["b2"]);
    assert_eq!(l.unreadable, vec!["a1"]);
}

#[test]
fn the_erase_marker_is_sealed_and_names_no_conversation_in_plaintext() {
    let d = TestDir::new("marker");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    let id = "distinctive-thread-id-0123456789";
    s.save_thread(doc(id, 1, SECRET)).unwrap();
    {
        let _crash = CrashAfterMarker::on();
        assert!(s.erase_thread(id).is_err());
    }
    let marker = std::fs::read(d.path().join(PENDING_FILE)).unwrap();
    assert_eq!(&marker[..8], MAGIC, "a sealed history file");
    assert!(!contains(&marker, id.as_bytes()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.path().join(PENDING_FILE)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert!(s.list_threads(2).unwrap().threads.is_empty());
}

#[test]
fn a_marker_that_does_not_open_fails_closed_and_reseals_nothing() {
    let d = TestDir::new("badmarker");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.save_thread(doc("b2", 2, "keep")).unwrap();
    let keys = KeySet::decode(&kr.raw().unwrap()).unwrap();
    // An empty marker (what an earlier build of this branch wrote), a flipped byte, and a marker
    // naming something that is not a conversation all refuse.
    let mut flipped = {
        s.write_erase_marker(&keys, &["t-a1".to_string()]).unwrap();
        std::fs::read(d.path().join(PENDING_FILE)).unwrap()
    };
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    let not_a_thread = {
        s.write_erase_marker(&keys, &["settings".to_string()]).unwrap();
        std::fs::read(d.path().join(PENDING_FILE)).unwrap()
    };
    let a1 = std::fs::read(sealed_path(d.path(), "a1")).unwrap();
    let b2 = std::fs::read(sealed_path(d.path(), "b2")).unwrap();
    let key = kr.raw().unwrap();
    for bad in [Vec::new(), flipped, not_a_thread] {
        std::fs::write(d.path().join(PENDING_FILE), &bad).unwrap();
        let e = s.list_threads(3).unwrap_err();
        assert!(e.contains("does not open"), "{e}");
        assert!(s.save_thread(doc("c3", 3, "x")).is_err());
        assert_eq!(std::fs::read(sealed_path(d.path(), "a1")).unwrap(), a1);
        assert_eq!(std::fs::read(sealed_path(d.path(), "b2")).unwrap(), b2);
        assert_eq!(kr.raw().unwrap(), key, "no rotation on a marker that does not open");
    }
    // Delete all still works.
    s.erase_all().unwrap();
    assert!(s.list_threads(4).unwrap().threads.is_empty());
}

#[test]
fn an_unreadable_retention_setting_fails_closed_and_is_not_overwritten_silently() {
    // Matr0xshka P2 on #679: a tampered settings file must not become "keep forever".
    let d = TestDir::new("badsettings");
    let kr = MemKeyring::default();
    let s = HistoryStore::new(d.path(), &kr);
    let day = MS_PER_DAY;
    let now = 100 * day;
    s.save_thread(doc("old", now - 10 * day, SECRET)).unwrap();
    s.set_retention_days(Some(7), false).unwrap();
    let path = d.path().join("settings.sealed");
    let mut bad = std::fs::read(&path).unwrap();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    std::fs::write(&path, &bad).unwrap();

    let l = s.list_threads(now).unwrap();
    assert!(l.retention_unreadable, "reported, not hidden");
    assert_eq!(l.retention_days, None);
    assert_eq!(l.purged, 0, "nothing is erased by an unknown setting");
    assert_eq!(ids(&l), vec!["old"]);
    // Neither a conversation save nor a plain settings write replaces it.
    s.save_thread(doc("new", now, "x")).unwrap();
    assert!(s.set_retention_days(None, false).unwrap_err().contains("does not open"));
    assert_eq!(std::fs::read(&path).unwrap(), bad);
    // The member chooses again: replaced, and it applies.
    s.set_retention_days(Some(7), true).unwrap();
    let l = s.list_threads(now).unwrap();
    assert!(!l.retention_unreadable);
    assert_eq!(l.retention_days, Some(7));
    assert_eq!(l.purged, 1);
    assert_eq!(ids(&l), vec!["new"]);
}

/// A keyring that records, when the key is deleted, whether history files were still present.
struct OrderKeyring {
    inner: MemKeyring,
    dir: std::path::PathBuf,
    files_at_key_delete: StdMutex<Option<usize>>,
}

impl Keyring for OrderKeyring {
    fn get(&self, a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        self.inner.get(a)
    }
    fn set(&self, a: &str, s: &[u8]) -> std::result::Result<(), CustodyError> {
        self.inner.set(a, s)
    }
    fn delete(&self, a: &str) -> std::result::Result<(), CustodyError> {
        let n = std::fs::read_dir(&self.dir).map(|e| e.count()).unwrap_or(0);
        *self.files_at_key_delete.lock().unwrap() = Some(n);
        self.inner.delete(a)
    }
}

#[test]
fn delete_all_drops_the_key_before_the_files() {
    let d = TestDir::new("allorder");
    let kr = OrderKeyring {
        inner: MemKeyring::default(),
        dir: d.path().to_path_buf(),
        files_at_key_delete: StdMutex::new(None),
    };
    let s = HistoryStore::new(d.path(), &kr);
    s.save_thread(doc("a1", 1, SECRET)).unwrap();
    s.save_thread(doc("b2", 2, "x")).unwrap();
    s.erase_all().unwrap();
    assert_eq!(*kr.files_at_key_delete.lock().unwrap(), Some(2), "the key went first");
    assert!(all_bytes(d.path()).is_empty());
    // An unreachable keyring refuses before any file is touched.
    let d2 = TestDir::new("alldead");
    let kr2 = MemKeyring::default();
    HistoryStore::new(d2.path(), &kr2).save_thread(doc("a1", 1, "x")).unwrap();
    assert!(HistoryStore::new(d2.path(), &DeadKeyring).erase_all().is_err());
    assert!(sealed_path(d2.path(), "a1").exists());
}
