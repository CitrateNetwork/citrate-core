// Tests for the withdrawn-branch recovery. Included into `node_withdrawn::tests`.

use super::*;
use crate::rpc::RpcError;

const CHECK: ForkCheck = FORK_CHECKS[0];
const WITHDRAWN_HASH: &str = "0x8a0d748900000000000000000000000000000000000000000000000000000001";

/// A node RPC that answers `eth_blockNumber` with `head` and `eth_getBlockByNumber` with `hash`
/// (each `None` = the call fails).
struct FakeNode {
    head: Option<u64>,
    hash: Option<&'static str>,
}

impl RpcTransport for FakeNode {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        let fail = || RpcError::Transport("down".into());
        match body.get("method").and_then(Value::as_str) {
            Some("eth_blockNumber") => {
                let h = self.head.ok_or_else(fail)?;
                Ok(json!({ "jsonrpc": "2.0", "id": 1, "result": format!("0x{h:x}") }))
            }
            Some("eth_getBlockByNumber") => {
                let h = self.hash.ok_or_else(fail)?;
                Ok(json!({ "jsonrpc": "2.0", "id": 1, "result": { "hash": h } }))
            }
            _ => Err(fail()),
        }
    }
}

#[test]
fn the_compiled_in_check_is_the_2026_10_15_rollback_on_the_book_genesis() {
    assert_eq!(CHECK.height, 365_785);
    assert_eq!(
        CHECK.canonical,
        "0x51482ff9724a2f0cbf4fe678f3ce556f6222334d5b2a3f5eba8a2b52af0cf3d2"
    );
    assert_eq!(checks_for(crate::addresses::genesis_hash()), vec![CHECK]);
}

#[test]
fn another_genesis_gets_no_checks() {
    assert!(checks_for("0xabcdef").is_empty());
}

#[test]
fn a_healthy_node_on_the_canonical_block_is_left_alone() {
    let node = FakeNode { head: Some(400_000), hash: Some(CHECK.canonical) };
    assert_eq!(assess(&node, &CHECK), Verdict::Canonical);
    // Case does not matter.
    let upper = CHECK.canonical.to_ascii_uppercase().replacen("0X", "0x", 1);
    assert_eq!(verdict(&CHECK, Some(400_000), Some(&upper)), Verdict::Canonical);
}

#[test]
fn a_node_on_another_block_at_the_check_height_is_withdrawn() {
    let node = FakeNode { head: Some(386_060), hash: Some(WITHDRAWN_HASH) };
    assert_eq!(
        assess(&node, &CHECK),
        Verdict::Withdrawn { local: WITHDRAWN_HASH.into() }
    );
}

#[test]
fn a_node_below_the_check_height_is_checked_again_later() {
    let node = FakeNode { head: Some(365_784), hash: Some(WITHDRAWN_HASH) };
    assert_eq!(assess(&node, &CHECK), Verdict::NotReached);
}

#[test]
fn a_failed_read_never_moves_anything() {
    assert_eq!(assess(&FakeNode { head: None, hash: None }, &CHECK), Verdict::Unknown);
    assert_eq!(
        assess(&FakeNode { head: Some(400_000), hash: None }, &CHECK),
        Verdict::Unknown
    );
}

/// A node RPC whose `eth_getBlockByNumber` answers with a raw JSON-RPC response body.
struct RawNode {
    head: u64,
    block_response: Value,
}

impl RpcTransport for RawNode {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        match body.get("method").and_then(Value::as_str) {
            Some("eth_blockNumber") => {
                Ok(json!({ "jsonrpc": "2.0", "id": 1, "result": format!("0x{:x}", self.head) }))
            }
            Some("eth_getBlockByNumber") => Ok(self.block_response.clone()),
            _ => Err(RpcError::Transport("down".into())),
        }
    }
}

// UI truth contract v1.1 §6: compare only once the node has applied the checkpoint height AND
// returns that block. A null, an error, or anything that is not that block is "not yet", never a
// mismatch.
#[test]
fn a_null_block_at_or_above_the_height_is_not_yet() {
    let node = RawNode { head: 400_000, block_response: json!({ "jsonrpc": "2.0", "id": 1, "result": null }) };
    assert_eq!(assess(&node, &CHECK), Verdict::Unknown);
}

#[test]
fn an_error_response_for_the_block_is_not_yet() {
    let node = RawNode {
        head: 400_000,
        block_response: json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "header not found" } }),
    };
    assert_eq!(assess(&node, &CHECK), Verdict::Unknown);
}

#[test]
fn a_block_for_another_height_is_not_that_block() {
    let node = RawNode {
        head: 400_000,
        block_response: json!({ "jsonrpc": "2.0", "id": 1, "result": { "number": "0x61a80", "hash": WITHDRAWN_HASH } }),
    };
    assert_eq!(assess(&node, &CHECK), Verdict::Unknown);
}

#[test]
fn a_block_without_a_well_formed_hash_is_not_yet() {
    for hash in ["", "0x", "0x1234", "not-a-hash"] {
        let node = RawNode {
            head: 400_000,
            block_response: json!({ "jsonrpc": "2.0", "id": 1, "result": { "number": format!("0x{:x}", CHECK.height), "hash": hash } }),
        };
        assert_eq!(assess(&node, &CHECK), Verdict::Unknown, "hash {hash:?}");
    }
}

#[test]
fn the_returned_check_block_with_another_hash_is_withdrawn() {
    let node = RawNode {
        head: 400_000,
        block_response: json!({ "jsonrpc": "2.0", "id": 1, "result": { "number": format!("0x{:x}", CHECK.height), "hash": WITHDRAWN_HASH } }),
    };
    assert_eq!(assess(&node, &CHECK), Verdict::Withdrawn { local: WITHDRAWN_HASH.into() });
}

/// A fresh scratch folder (removed at the end of each test).
struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "citrate-core-withdrawn-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&p).expect("mkdir");
        Tmp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A node data dir as the app leaves it: chain DB files next to the node's keys and config.
fn data_dir_with_db(root: &Path) -> PathBuf {
    let dir = root.join("node");
    std::fs::create_dir_all(&dir).expect("mkdir");
    for (name, body) in [
        ("CURRENT", "MANIFEST-000005\n"),
        ("MANIFEST-000005", "m"),
        ("OPTIONS-000007", "o"),
        ("000013.sst", "s"),
        ("000181.log", "l"),
        ("LOG", "log"),
        ("IDENTITY", "id"),
        ("LOCK", ""),
        ("proposer.key", "secret-proposer"),
        ("noise.key", "secret-noise"),
        ("node.toml", "data_dir = \"x\""),
        (crate::node_genesis::GENESIS_MARKER_FILE, "0x1dcf"),
        ("crash-records.jsonl", "{}"),
    ] {
        std::fs::write(dir.join(name), body).expect("write");
    }
    dir
}

#[test]
fn moving_aside_takes_only_the_chain_db_and_keeps_keys_and_config() {
    let tmp = Tmp::new("keeps");
    let dir = data_dir_with_db(tmp.path());
    let (dest, moved) = move_chain_db_aside(&dir, "2026-10-17")
        .expect("move")
        .expect("something moved");
    assert_eq!(dest, tmp.path().join("node.withdrawn-2026-10-17"));
    for kept in ["proposer.key", "noise.key", "node.toml", "chain-genesis", "crash-records.jsonl"] {
        assert!(dir.join(kept).exists(), "{kept} must stay in the data dir");
        assert!(!dest.join(kept).exists(), "{kept} must not be moved");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("proposer.key")).expect("read"),
        "secret-proposer"
    );
    for gone in ["CURRENT", "MANIFEST-000005", "000013.sst", "000181.log", "IDENTITY", "LOCK"] {
        assert!(!dir.join(gone).exists(), "{gone} must leave the data dir");
        assert!(dest.join(gone).exists(), "{gone} must be in {}", dest.display());
    }
    assert!(moved.contains(&"000013.sst".to_string()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&dest).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[test]
fn moving_aside_twice_is_a_no_op_the_second_time() {
    let tmp = Tmp::new("twice");
    let dir = data_dir_with_db(tmp.path());
    assert!(move_chain_db_aside(&dir, "2026-10-17").expect("first").is_some());
    assert_eq!(move_chain_db_aside(&dir, "2026-10-17").expect("second"), None);
    // No second folder was created.
    assert!(!tmp.path().join("node.withdrawn-2026-10-17-2").exists());
}

#[test]
fn an_existing_aside_folder_is_never_overwritten() {
    let tmp = Tmp::new("existing");
    let dir = data_dir_with_db(tmp.path());
    // The owner's manual move from 2026-10-15 used the same naming.
    let earlier = tmp.path().join("node.withdrawn-2026-10-15");
    std::fs::create_dir_all(&earlier).expect("mkdir");
    std::fs::write(earlier.join("000013.sst"), "earlier").expect("write");
    let (dest, _) = move_chain_db_aside(&dir, "2026-10-15")
        .expect("move")
        .expect("moved");
    assert_eq!(dest, tmp.path().join("node.withdrawn-2026-10-15-2"));
    assert_eq!(
        std::fs::read_to_string(earlier.join("000013.sst")).expect("read"),
        "earlier"
    );
}

#[test]
fn civil_dates() {
    assert_eq!(civil_date(0), "1970-01-01");
    assert_eq!(civil_date(20_741), "2026-10-15");
    assert_eq!(civil_date(11_016), "2000-02-29");
}
