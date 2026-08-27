// CX-S2.1 — kubo storage seam tests. Included into `storage::tests` (private access).
//
// A fake in-memory kubo (blobs + a pin set) drives the manager through add → pin → list →
// unpin → retrieve, so the orchestration + the index/live-pin reconciliation are proven with no
// real daemon. Parser tests pin the two kubo response shapes the production transport depends on.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

/// An in-memory kubo: content-addressed blobs + a recursive pin set. `fail` flips every op to a
/// transport error (the daemon-down path).
struct FakeKubo {
    blobs: Mutex<BTreeMap<String, Vec<u8>>>,
    pins: Mutex<BTreeSet<String>>,
    fail: bool,
}
impl FakeKubo {
    fn new() -> Arc<Self> {
        Arc::new(FakeKubo {
            blobs: Mutex::new(BTreeMap::new()),
            pins: Mutex::new(BTreeSet::new()),
            fail: false,
        })
    }
    fn failing() -> Arc<Self> {
        Arc::new(FakeKubo {
            blobs: Mutex::new(BTreeMap::new()),
            pins: Mutex::new(BTreeSet::new()),
            fail: true,
        })
    }
    /// A deterministic fake CID (content-ish, collision-free for the distinct test inputs).
    fn cid_for(bytes: &[u8]) -> String {
        format!("bafyfake{}x{}", bytes.len(), bytes.first().copied().unwrap_or(0))
    }
}
/// Share the fake with the manager (which takes a `Box<dyn KuboTransport>`) while keeping a handle
/// for direct manipulation — mirrors connections_tests' SharedHttp.
struct SharedKubo(Arc<FakeKubo>);
impl KuboTransport for SharedKubo {
    fn add(&self, _filename: &str, bytes: &[u8]) -> Result<AddOutcome> {
        if self.0.fail {
            return Err(StorageError::Transport("kubo down".into()));
        }
        let cid = FakeKubo::cid_for(bytes);
        self.0.blobs.lock().unwrap().insert(cid.clone(), bytes.to_vec());
        Ok(AddOutcome {
            cid,
            size_bytes: bytes.len() as u64,
        })
    }
    fn pin_add(&self, cid: &str) -> Result<()> {
        if self.0.fail {
            return Err(StorageError::Transport("kubo down".into()));
        }
        self.0.pins.lock().unwrap().insert(cid.to_string());
        Ok(())
    }
    fn pin_rm(&self, cid: &str) -> Result<()> {
        self.0.pins.lock().unwrap().remove(cid);
        Ok(())
    }
    fn pin_ls(&self) -> Result<Vec<String>> {
        if self.0.fail {
            return Err(StorageError::Transport("kubo down".into()));
        }
        Ok(self.0.pins.lock().unwrap().iter().cloned().collect())
    }
    fn cat(&self, cid: &str) -> Result<Vec<u8>> {
        self.0
            .blobs
            .lock()
            .unwrap()
            .get(cid)
            .cloned()
            .ok_or_else(|| StorageError::Transport(format!("no such object: {cid}")))
    }
}

fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("citrate-storage-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn manager(tag: &str, fake: Arc<FakeKubo>) -> (StorageManager, PathBuf) {
    let dir = tmp_dir(tag);
    let mgr = StorageManager::new(Box::new(SharedKubo(fake)), dir.clone()).with_clock(|| 1000);
    (mgr, dir)
}

/// Write a temp file to add.
fn temp_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn add_records_the_file_but_does_not_pin_it() {
    let fake = FakeKubo::new();
    let (mgr, dir) = manager("add", fake);
    let file = temp_file(&dir, "notes.txt", b"hello commons");
    let out = mgr.add_file(&file).expect("add");
    assert_eq!(out.size_bytes, 13);

    let rows = mgr.list().expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cid, out.cid);
    assert_eq!(rows[0].size_bytes, 13);
    assert_eq!(rows[0].bond_salt, ""); // no bond until pinned
    assert_eq!(rows[0].added_at, 1000); // the injected clock
    // Added is NOT pinning — a bare add leaves the object unpinned until the bonded step.
    assert_eq!(rows[0].pin_state, PinState::Unpinned);
}

#[test]
fn pin_marks_pinned_and_records_the_bond_then_unpin_flips_back() {
    let fake = FakeKubo::new();
    let (mgr, dir) = manager("pin", fake);
    let file = temp_file(&dir, "data.bin", b"\x01\x02\x03\x04");
    let out = mgr.add_file(&file).unwrap();

    mgr.pin(&out.cid, "salt-bond-42").expect("pin");
    let row = &mgr.list().unwrap()[0];
    assert_eq!(row.pin_state, PinState::Pinned);
    assert_eq!(row.bond_salt, "salt-bond-42");

    mgr.unpin(&out.cid).expect("unpin");
    let row = &mgr.list().unwrap()[0];
    // The record survives; truth-from-daemon says it is no longer pinned.
    assert_eq!(row.pin_state, PinState::Unpinned);
    assert_eq!(row.bond_salt, "salt-bond-42"); // the bond record is retained
}

#[test]
fn retrieve_roundtrips_the_bytes_to_a_local_file() {
    let fake = FakeKubo::new();
    let (mgr, dir) = manager("retrieve", fake);
    let content = b"the quick brown fox".to_vec();
    let file = temp_file(&dir, "src.txt", &content);
    let out = mgr.add_file(&file).unwrap();

    let path = mgr.retrieve(&out.cid).expect("retrieve");
    assert!(path.exists());
    assert_eq!(std::fs::read(&path).unwrap(), content);
}

#[test]
fn list_surfaces_a_live_pin_the_app_never_tracked() {
    let fake = FakeKubo::new();
    // Pin a CID directly on the daemon (out-of-band), never through the manager's index.
    fake.pins.lock().unwrap().insert("bafyexternalpin".to_string());
    let (mgr, _dir) = manager("untracked", fake);

    let rows = mgr.list().expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cid, "bafyexternalpin");
    assert_eq!(rows[0].pin_state, PinState::Pinned);
    assert_eq!(rows[0].size_bytes, 0); // honest: unknown metadata for an untracked pin
    assert_eq!(rows[0].bond_salt, "");
}

#[test]
fn a_down_daemon_surfaces_a_transport_error() {
    let (mgr, dir) = manager("down", FakeKubo::failing());
    let file = temp_file(&dir, "x.txt", b"x");
    let r = mgr.add_file(&file);
    assert!(matches!(r, Err(StorageError::Transport(_))), "got {r:?}");
    assert!(matches!(mgr.list(), Err(StorageError::Transport(_))));
}

#[test]
fn parse_add_response_takes_the_last_object_and_string_size() {
    // kubo streams one JSON object per file; the LAST is the root add, and Size is a STRING.
    let body = "{\"Name\":\"a\",\"Hash\":\"bafychild\",\"Size\":\"10\"}\n{\"Name\":\"dir\",\"Hash\":\"bafyroot\",\"Size\":\"4590807392\"}";
    let out = parse_add_response(body).expect("parse");
    assert_eq!(out.cid, "bafyroot");
    assert_eq!(out.size_bytes, 4_590_807_392);
    assert!(parse_add_response("").is_err());
}

#[test]
fn parse_pin_ls_extracts_the_keys() {
    let body = r#"{"Keys":{"bafyA":{"Type":"recursive"},"bafyB":{"Type":"recursive"}}}"#;
    let mut cids = parse_pin_ls(body).expect("parse");
    cids.sort();
    assert_eq!(cids, vec!["bafyA".to_string(), "bafyB".to_string()]);
    // An empty pin set is valid, not an error.
    assert_eq!(parse_pin_ls(r#"{"Keys":{}}"#).unwrap(), Vec::<String>::new());
}
