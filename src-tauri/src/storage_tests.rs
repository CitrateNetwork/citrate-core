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

// CX-S2.2 — the bond commitments must match citrate-chain's frozen CommD vectors BYTE-FOR-BYTE.
// A drift here would register a slashable bond, so this is a hard money-path gate, not a nicety.
// Source: citrate-chain crates/citrate-commd/tests/commd_frozen_v1.rs.
#[test]
fn bond_commitments_match_the_chain_frozen_vectors() {
    let cases: &[(&str, Vec<u8>, &str, &str)] = &[
        (
            "empty",
            vec![],
            "0000000000000000000000000000000000000000000000000000000000000000",
            "294d5514bcdc323b146ef99c58c637945d2d3b43a5cb5efc0b0c057c36f28d3a",
        ),
        (
            "one_byte_0x01",
            vec![1],
            "0000000000000000000000000000000000000000000000000000000000000001",
            "0403682fea89ee0ec92726d683e023ebd8c5b78f6ab3098175e81455620f48b7",
        ),
        (
            "hello_pin",
            b"hello pin".to_vec(),
            "00000000000000000000000000000000000000000000006e6970206f6c6c6568",
            "2b0894c438660404477b872ced1b10a5ebc3d83263238cb38eae09355c202d7f",
        ),
        (
            "100_incrementing",
            (0..100u32).map(|i| i as u8).collect(),
            "02765621f5f7e5c569458c89aff1f46adf4d6bf58a135d66eb2dc9c3a6e64290",
            "03f2fcdf67adea13e3d76eed02265bc432473b09c5bd89d26b7d6a275ffe1216",
        ),
    ];
    for (label, data, commd, dc) in cases {
        let c = bond_commitments(data);
        assert_eq!(hex::encode(c.comm_d), *commd, "commD drift at {label}");
        assert_eq!(hex::encode(c.data_commit), *dc, "dataCommit drift at {label}");
        // dataHash is plain keccak256(data).
        assert_eq!(c.data_hash, keccak256(data));
    }
}

#[test]
fn register_model_calldata_is_abi_well_formed() {
    let c = bond_commitments(b"hello pin");
    let data_uri = "ipfs://bafytestcid";
    let cd = register_model_calldata(bond_cid(data_uri), &c, data_uri);

    // selector = keccak256("registerModel(bytes32,bytes32,bytes32,bytes32,string)")[..4]
    assert_eq!(
        &cd[0..4],
        &keccak256(b"registerModel(bytes32,bytes32,bytes32,bytes32,string)")[..4]
    );
    // cid, commD, dataHash, dataCommit are words 0..4 (after the selector).
    assert_eq!(&cd[4..36], &bond_cid(data_uri));
    assert_eq!(&cd[36..68], &c.comm_d);
    assert_eq!(&cd[68..100], &c.data_hash);
    assert_eq!(&cd[100..132], &c.data_commit);
    // word 4 = string offset = 0xa0 (5 head words).
    assert_eq!(cd[132 + 31], 0xa0);
    // tail: length word then the right-padded uri bytes.
    let uri = data_uri.as_bytes();
    assert_eq!(cd[164 + 31], uri.len() as u8);
    assert_eq!(&cd[196..196 + uri.len()], uri);
    // The args (everything after the 4-byte selector) are 32-byte aligned.
    assert_eq!((cd.len() - 4) % 32, 0);
}
