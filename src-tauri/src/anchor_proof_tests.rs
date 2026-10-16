//! HUP-S7.3 (US-7.2 AC3): core's own check of an anchored decision's proof.
//!
//! The fixture was captured from the real sidecar (citrate-agent-runtime `hup/n5-anchor-rest`):
//! five decision records of one closed day written through `citrate-agent-records`, planned and
//! proven through `/anchor/plan` and `/anchor/proof`. Selectors were checked with `cast sig`
//! (foundry 1.5.1, 2026-10-01).

use super::*;
use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::VecDeque;

const FIXTURE: &str = include_str!("../tests/fixtures/anchor/sidecar-proofs-5.json");
const REGISTRY: &str = "0x00000000000000000000000000000000000000a1";
const ME: &str = "0x1111111111111111111111111111111111111111";
const OTHER: &str = "0x2222222222222222222222222222222222222222";

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture json")
}

fn proofs() -> Vec<SidecarProof> {
    fixture()["proofs"]
        .as_array()
        .expect("proofs")
        .iter()
        .map(|p| serde_json::from_value(p.clone()).expect("proof"))
        .collect()
}

fn planned_commitment() -> [u8; 32] {
    hex32(fixture()["plan"]["commitment"].as_str().expect("c")).expect("hex")
}

// ------------------------------------------------------------------ selectors + decoding

#[test]
fn registry_selectors_match_cast() {
    assert_eq!(
        hex::encode(&get_anchor_calldata(&[0u8; 32])[..4]),
        "7feb51d9"
    );
    assert_eq!(
        hex::encode(&get_anchor_by_calldata(&[0u8; 20], &[0u8; 32])[..4]),
        "a7e0e3e0"
    );
    assert_eq!(hex::encode(anchor_not_found_selector()), "46716752");
}

#[test]
fn get_anchor_by_puts_the_committer_then_the_root() {
    let me = [0x11u8; 20];
    let root = [0x22u8; 32];
    let cd = get_anchor_by_calldata(&me, &root);
    assert_eq!(cd.len(), 68);
    assert!(cd[4..16].iter().all(|b| *b == 0));
    assert_eq!(&cd[16..36], &me);
    assert_eq!(&cd[36..], &root);
}

fn anchor_ret(kind: u8, root: &[u8; 32], committer: &str, block: u64, ts: u64) -> Vec<u8> {
    let mut v = Vec::new();
    let mut w = [0u8; 32];
    w[31] = kind;
    v.extend_from_slice(&w);
    v.extend_from_slice(root);
    let mut a = [0u8; 32];
    a[12..].copy_from_slice(&hex::decode(committer.trim_start_matches("0x")).expect("hex"));
    v.extend_from_slice(&a);
    let mut b = [0u8; 32];
    b[24..].copy_from_slice(&block.to_be_bytes());
    v.extend_from_slice(&b);
    let mut t = [0u8; 32];
    t[24..].copy_from_slice(&ts.to_be_bytes());
    v.extend_from_slice(&t);
    v
}

#[test]
fn decodes_the_anchor_struct() {
    let root = [7u8; 32];
    let a = decode_anchor(&anchor_ret(2, &root, ME, 42, 1_790_000_000)).expect("decodes");
    assert_eq!(a.kind, 2);
    assert_eq!(a.root, format!("0x{}", hex::encode(root)));
    assert_eq!(a.committer, ME);
    assert_eq!(a.block_number, 42);
    assert_eq!(a.timestamp, 1_790_000_000);
}

#[test]
fn a_short_or_dirty_answer_is_not_an_anchor() {
    assert!(decode_anchor(&[0u8; 64]).is_err());
    let mut bad = anchor_ret(2, &[7u8; 32], ME, 42, 1);
    bad[64] = 1; // high bytes of the address word
    assert!(decode_anchor(&bad).is_err());
    let mut big = anchor_ret(2, &[7u8; 32], ME, 42, 1);
    big[96] = 1; // a block number past u64
    assert!(decode_anchor(&big).is_err());
}

// ------------------------------------------------------------------ links 1 and 2

#[test]
fn every_fixture_proof_holds_and_leads_to_the_planned_commitment() {
    let c = planned_commitment();
    let ps = proofs();
    assert_eq!(ps.len(), 5);
    for p in &ps {
        assert_eq!(check_inclusion(&p.proof), Ok(c), "seq {}", p.seq);
        assert_eq!(record_bound(p), Some(true), "seq {}", p.seq);
    }
}

#[test]
fn the_commitment_formula_matches_the_runtime() {
    let p = &proofs()[0];
    let root = hex32(&p.proof.header.tree_root).expect("root");
    assert_eq!(commitment(&p.proof.header, &root), planned_commitment());
}

#[test]
fn a_changed_path_hash_fails() {
    let mut p = proofs()[1].proof.clone();
    let mut h = hex32(&p.path[0]).expect("hex");
    h[0] ^= 1;
    p.path[0] = hex::encode(h);
    assert!(check_inclusion(&p).is_err());
}

#[test]
fn a_dropped_or_extra_path_hash_fails() {
    let mut p = proofs()[1].proof.clone();
    p.path.pop();
    assert!(check_inclusion(&p).is_err());
    let mut p = proofs()[4].proof.clone();
    p.path.push(hex::encode([9u8; 32]));
    assert!(check_inclusion(&p).is_err());
}

#[test]
fn a_proof_cannot_be_moved_to_another_record() {
    // Same leaf and path, another seq: the leaf position no longer matches.
    let mut p = proofs()[2].proof.clone();
    p.seq = 3;
    assert!(check_inclusion(&p).is_err());
    // Another leaf index with the same path: the path no longer leads to the root.
    let mut p = proofs()[2].proof.clone();
    p.leaf_index = 3;
    p.seq = 3;
    assert!(check_inclusion(&p).is_err());
}

#[test]
fn a_changed_header_changes_or_breaks_the_commitment() {
    let c = planned_commitment();
    let mut p = proofs()[0].proof.clone();
    p.header.day += 1;
    assert_ne!(check_inclusion(&p), Ok(c), "another day is another value");
    let mut p = proofs()[0].proof.clone();
    p.header.count = 6;
    p.header.last_seq = 5;
    assert!(check_inclusion(&p) != Ok(c));
    let mut p = proofs()[0].proof.clone();
    p.header.count = 4; // no longer contiguous with last_seq 4
    assert!(check_inclusion(&p).is_err());
    let mut p = proofs()[0].proof.clone();
    p.header.v = 2;
    assert!(check_inclusion(&p).is_err());
}

#[test]
fn an_edited_record_is_not_bound_to_its_leaf() {
    let mut p = proofs()[4].clone();
    let canonical = p.record_canonical.clone().expect("bytes");
    p.record_canonical = Some(canonical.replace("deploy HelloMint", "deploy HelloMinT"));
    assert_eq!(record_bound(&p), Some(false));
    p.record_canonical = None;
    assert_eq!(record_bound(&p), None);
}

#[test]
fn leaf_and_node_hashes_are_domain_separated() {
    let x = [3u8; 32];
    assert_ne!(leaf_hash(&x), node_hash(&x, &x));
    // A single-leaf tree: the root is the leaf hash, with an empty path.
    assert!(verify_path(&x, 0, 1, &[], &leaf_hash(&x)));
    assert!(!verify_path(&x, 1, 1, &[], &leaf_hash(&x)));
}

// ------------------------------------------------------------------ link 3 (scripted chain)

struct ScriptRpc {
    requests: RefCell<Vec<Value>>,
    responses: RefCell<VecDeque<Result<Value, RpcError>>>,
}
impl ScriptRpc {
    fn new(r: Vec<Result<Value, RpcError>>) -> RpcClient<ScriptRpc> {
        RpcClient::with_transport(ScriptRpc {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(r.into_iter().collect()),
        })
    }
}
impl RpcTransport for ScriptRpc {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        self.requests.borrow_mut().push(body);
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(RpcError::Transport("script: no response".into())))
    }
}
fn ok(v: Value) -> Result<Value, RpcError> {
    Ok(json!({"jsonrpc": "2.0", "id": 1, "result": v}))
}
fn revert(data: Option<&str>) -> Result<Value, RpcError> {
    let mut e = json!({"code": 3, "message": "execution reverted"});
    if let Some(d) = data {
        e["data"] = json!(d);
    }
    Ok(json!({"jsonrpc": "2.0", "id": 1, "error": e}))
}
fn data(b: &[u8]) -> Result<Value, RpcError> {
    ok(json!(format!("0x{}", hex::encode(b))))
}
fn code() -> Result<Value, RpcError> {
    ok(json!("0x6080"))
}
fn methods(rpc: &RpcClient<ScriptRpc>) -> Vec<String> {
    rpc.transport()
        .requests
        .borrow()
        .iter()
        .map(|r| r["method"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn no_registry_in_the_book_makes_no_chain_call() {
    let rpc = ScriptRpc::new(vec![]);
    assert_eq!(
        check_chain(&rpc, None, &[1u8; 32], Some(ME)),
        ChainCheck::NotInBook
    );
    assert!(methods(&rpc).is_empty());
}

#[test]
fn an_address_without_code_is_said_so() {
    let rpc = ScriptRpc::new(vec![ok(json!("0x"))]);
    assert_eq!(
        check_chain(&rpc, Some(REGISTRY), &[1u8; 32], Some(ME)),
        ChainCheck::NoCode {
            registry: REGISTRY.into()
        }
    );
}

#[test]
fn the_deployed_registry_anchor_by_me_is_proven_as_mine() {
    let root = [5u8; 32];
    // getAnchorBy is not on the deployed version: it reverts with no data, then getAnchor answers.
    let rpc = ScriptRpc::new(vec![
        code(),
        revert(None),
        data(&anchor_ret(2, &root, ME, 77, 1)),
    ]);
    match check_chain(&rpc, Some(REGISTRY), &root, Some(ME)) {
        ChainCheck::Anchored { anchor, by_you, .. } => {
            assert!(by_you);
            assert_eq!(anchor.block_number, 77);
        }
        c => panic!("expected anchored, got {c:?}"),
    }
    assert_eq!(methods(&rpc), vec!["eth_getCode", "eth_call", "eth_call"]);
}

#[test]
fn someone_elses_anchor_of_the_same_value_is_not_mine() {
    let root = [5u8; 32];
    let rpc = ScriptRpc::new(vec![
        code(),
        revert(None),
        data(&anchor_ret(2, &root, OTHER, 70, 1)),
    ]);
    match check_chain(&rpc, Some(REGISTRY), &root, Some(ME)) {
        ChainCheck::Anchored { anchor, by_you, .. } => {
            assert!(!by_you);
            assert_eq!(anchor.committer, OTHER);
        }
        c => panic!("expected anchored by another, got {c:?}"),
    }
}

#[test]
fn the_next_registry_answers_for_my_own_key_first() {
    let root = [5u8; 32];
    // getAnchorBy(me, root) exists and holds my record; no second read is needed.
    let rpc = ScriptRpc::new(vec![code(), data(&anchor_ret(2, &root, ME, 81, 1))]);
    match check_chain(&rpc, Some(REGISTRY), &root, Some(ME)) {
        ChainCheck::Anchored { by_you, anchor, .. } => {
            assert!(by_you);
            assert_eq!(anchor.block_number, 81);
        }
        c => panic!("expected mine, got {c:?}"),
    }
    let reqs = rpc.transport().requests.borrow().clone();
    let sent = reqs[1]["params"][0]["data"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(sent.starts_with("0xa7e0e3e0"), "{sent}");
}

#[test]
fn on_the_next_registry_a_missing_own_record_falls_back_to_the_first_committer() {
    let root = [5u8; 32];
    let nf = format!("0x{}", hex::encode(anchor_not_found_selector()));
    let rpc = ScriptRpc::new(vec![
        code(),
        revert(Some(&nf)),
        data(&anchor_ret(2, &root, OTHER, 60, 1)),
    ]);
    match check_chain(&rpc, Some(REGISTRY), &root, Some(ME)) {
        ChainCheck::Anchored { by_you, .. } => assert!(!by_you),
        c => panic!("expected another's anchor, got {c:?}"),
    }
}

#[test]
fn not_found_is_not_anchored() {
    let nf = format!("0x{}", hex::encode(anchor_not_found_selector()));
    let rpc = ScriptRpc::new(vec![code(), revert(Some(&nf)), revert(Some(&nf))]);
    assert_eq!(
        check_chain(&rpc, Some(REGISTRY), &[5u8; 32], Some(ME)),
        ChainCheck::NotAnchored {
            registry: REGISTRY.into()
        }
    );
    // Without an anchor key only the first-committer read is made.
    let rpc = ScriptRpc::new(vec![code(), revert(Some(&nf))]);
    assert_eq!(
        check_chain(&rpc, Some(REGISTRY), &[5u8; 32], None),
        ChainCheck::NotAnchored {
            registry: REGISTRY.into()
        }
    );
    assert_eq!(methods(&rpc), vec!["eth_getCode", "eth_call"]);
}

#[test]
fn a_wrong_kind_or_root_is_a_mismatch_never_a_proof() {
    let root = [5u8; 32];
    let rpc = ScriptRpc::new(vec![code(), data(&anchor_ret(1, &root, ME, 1, 1))]);
    assert!(matches!(
        check_chain(&rpc, Some(REGISTRY), &root, Some(ME)),
        ChainCheck::Mismatch { .. }
    ));
    let rpc = ScriptRpc::new(vec![code(), data(&anchor_ret(2, &[6u8; 32], ME, 1, 1))]);
    assert!(matches!(
        check_chain(&rpc, Some(REGISTRY), &root, Some(ME)),
        ChainCheck::Mismatch { .. }
    ));
}

#[test]
fn an_unreachable_chain_is_unknown_not_unanchored() {
    let rpc = ScriptRpc::new(vec![Err(RpcError::Transport("timeout".into()))]);
    match check_chain(&rpc, Some(REGISTRY), &[5u8; 32], Some(ME)) {
        ChainCheck::Unreachable { detail, .. } => assert!(detail.contains("timeout")),
        c => panic!("expected unreachable, got {c:?}"),
    }
}

// ------------------------------------------------------------------ the verdict

fn anchored_by(me: bool) -> ChainCheck {
    ChainCheck::Anchored {
        registry: REGISTRY.into(),
        anchor: OnChainAnchor {
            kind: 2,
            root: format!("0x{}", hex::encode(planned_commitment())),
            committer: if me { ME.into() } else { OTHER.into() },
            block_number: 12,
            timestamp: 1,
        },
        by_you: me,
    }
}

#[test]
fn proven_only_with_all_three_links_and_my_key() {
    let p = &proofs()[3];
    let mut asked = None;
    let v = verdict(p, |c| {
        asked = Some(*c);
        anchored_by(true)
    });
    assert_eq!(
        asked,
        Some(planned_commitment()),
        "core's own commitment goes to the chain"
    );
    assert!(v.proven, "{v:?}");
    assert!(v.line.starts_with("Proven."), "{}", v.line);
    assert!(v.line.contains("block 12"));
    assert_eq!(v.date.as_deref(), Some("2026-10-01"));
    assert_eq!(v.record.as_ref().map(|r| r["seq"].clone()), Some(json!(3)));

    let v = verdict(p, |_| anchored_by(false));
    assert!(!v.proven);
    assert!(v.line.contains("not by your anchor key"), "{}", v.line);

    let v = verdict(p, |_| ChainCheck::NotAnchored {
        registry: REGISTRY.into(),
    });
    assert!(!v.proven);
    assert!(v.inclusion_ok);
    assert!(v.line.contains("not anchored on 40204 yet"));

    let v = verdict(p, |_| ChainCheck::NotInBook);
    assert!(!v.proven);
    assert!(v.line.contains("not in the 40204 address book"));
}

#[test]
fn a_broken_proof_never_reaches_the_chain() {
    let mut p = proofs()[1].clone();
    p.proof.path[0] = hex::encode([0u8; 32]);
    let v = verdict(&p, |_| panic!("the chain must not be asked"));
    assert!(!v.proven);
    assert!(!v.inclusion_ok);
    assert!(v.line.contains("does not hold"));
}

#[test]
fn a_stated_commitment_that_differs_from_the_proof_is_refused() {
    let mut p = proofs()[1].clone();
    p.commitment = format!("0x{}", hex::encode([1u8; 32]));
    let v = verdict(&p, |_| panic!("the chain must not be asked"));
    assert!(!v.proven);
    assert!(v.inclusion_error.is_some());
}

#[test]
fn a_proof_for_another_record_is_refused() {
    let mut p = proofs()[1].clone();
    p.seq = 2;
    let v = verdict(&p, |_| panic!("the chain must not be asked"));
    assert!(!v.proven);
}

#[test]
fn an_edited_record_is_never_proven_even_if_anchored() {
    let mut p = proofs()[4].clone();
    p.record_canonical = p
        .record_canonical
        .map(|c| c.replace("rejected on the card", "approved on the card"));
    let v = verdict(&p, |_| anchored_by(true));
    assert!(!v.proven);
    assert_eq!(v.record_bound, Some(false));
    assert!(v.line.contains("changed after it was written"));
    // An older sidecar that sends no bytes: not proven, said honestly.
    let mut p = proofs()[4].clone();
    p.record_canonical = None;
    let v = verdict(&p, |_| anchored_by(true));
    assert!(!v.proven);
    assert!(v.line.contains("did not send the record's bytes"));
}

// ------------------------------------------------------------------ n6: verifier against RFC 6962

/// The largest power of two strictly below `n` (RFC 6962 section 2.1, `k < n <= 2k`).
fn split(n: usize) -> usize {
    let mut k = 1;
    while k * 2 < n {
        k *= 2;
    }
    k
}

/// An independent RFC 6962 tree (section 2.1): MTH of a list and the audit path of leaf `m`.
fn mth(leaves: &[[u8; 32]]) -> [u8; 32] {
    match leaves.len() {
        1 => leaf_hash(&leaves[0]),
        n => {
            let k = split(n);
            node_hash(&mth(&leaves[..k]), &mth(&leaves[k..]))
        }
    }
}

fn audit_path(m: usize, leaves: &[[u8; 32]]) -> Vec<[u8; 32]> {
    let n = leaves.len();
    if n <= 1 {
        return Vec::new();
    }
    let k = split(n);
    if m < k {
        let mut p = audit_path(m, &leaves[..k]);
        p.push(mth(&leaves[k..]));
        p
    } else {
        let mut p = audit_path(m - k, &leaves[k..]);
        p.push(mth(&leaves[..k]));
        p
    }
}

#[test]
fn every_leaf_of_every_tree_up_to_33_verifies_and_only_at_its_own_index() {
    for n in 1u8..=33 {
        let leaves: Vec<[u8; 32]> = (0..n)
            .map(|i| [i.wrapping_mul(7).wrapping_add(1); 32])
            .collect();
        let root = mth(&leaves);
        for m in 0..n as usize {
            let path = audit_path(m, &leaves);
            assert!(
                verify_path(&leaves[m], m as u64, n as u64, &path, &root),
                "size {n}, leaf {m}"
            );
            if n > 1 {
                let other = (m + 1) % n as usize;
                assert!(
                    !verify_path(&leaves[m], other as u64, n as u64, &path, &root),
                    "size {n}, leaf {m} must not verify at {other}"
                );
            }
        }
    }
}

#[test]
fn the_last_leaf_of_an_unbalanced_day_is_proven() {
    // Five records: the fifth sits on the right edge of an unbalanced tree (path of one hash).
    let p = proofs()[4].clone();
    assert_eq!(p.proof.leaf_index, 4);
    assert_eq!(p.proof.path.len(), 1);
    assert_eq!(check_inclusion(&p.proof), Ok(planned_commitment()));
    let v = verdict(&p, |_| anchored_by(true));
    assert!(v.proven, "{}", v.line);
}

#[test]
fn record_bytes_for_another_number_or_day_are_not_bound_even_when_rehashed() {
    // A sidecar that swaps in other bytes and the matching leaf hash still fails link 1 when the
    // bytes name another record number or another day than the proof.
    let base = proofs()[2].clone();
    let canonical = base.record_canonical.clone().expect("bytes");
    let mut body: Value = serde_json::from_str(&canonical).expect("json");
    body["seq"] = serde_json::json!(base.seq + 1);
    let other = serde_json::to_string(&body).expect("encode");
    let mut p = base.clone();
    p.proof.record_hash = hex::encode(record_hash_of(&other));
    p.record_canonical = Some(other);
    assert_eq!(record_bound(&p), Some(false));

    let mut body: Value = serde_json::from_str(&canonical).expect("json");
    let ts = body["ts_ms"].as_u64().expect("ts");
    body["ts_ms"] = serde_json::json!(ts + 86_400_000);
    let other = serde_json::to_string(&body).expect("encode");
    let mut p = base.clone();
    p.proof.record_hash = hex::encode(record_hash_of(&other));
    p.record_canonical = Some(other);
    assert_eq!(record_bound(&p), Some(false));
    // The untouched bytes are bound.
    assert_eq!(record_bound(&base), Some(true));
}

/// The Citrate node on 40204 prints the revert data into the message instead of `data`
/// (read from rpc.citrate.ai, 2026-10-04: `getAnchor` of an unanchored value on AnchorRegistry).
const LIVE_NOT_FOUND: &str =
    "execution reverted: Execution reverted: Contract call reverted: 0x46716752 (gas used: 32425)";
const LIVE_UNKNOWN: &str =
    "execution reverted: Execution reverted: Contract call reverted: Unknown reason (gas used: 21518)";

#[test]
fn revert_data_printed_in_the_message_is_read() {
    assert_eq!(
        revert_data_in_message(LIVE_NOT_FOUND),
        Some(anchor_not_found_selector().to_vec())
    );
    assert_eq!(revert_data_in_message(LIVE_UNKNOWN), None);
    assert_eq!(revert_data_in_message("execution reverted"), None);
    // Hex before the word "reverted", or too short to be a selector, is not revert data.
    assert_eq!(
        revert_data_in_message("0x46716752 then reverted: 0x12"),
        None
    );
    assert_eq!(revert_data_in_message("no revert here 0x46716752"), None);
}

#[test]
fn on_40204_a_value_that_is_not_anchored_reads_as_not_anchored() {
    let root = planned_commitment();
    let msg_err = |m: &str| -> Result<Value, RpcError> {
        Ok(json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32000, "message": m}}))
    };
    // getAnchorBy is not on the deployed registry ("Unknown reason"); getAnchor says
    // AnchorNotFound in the message.
    let rpc = ScriptRpc::new(vec![code(), msg_err(LIVE_UNKNOWN), msg_err(LIVE_NOT_FOUND)]);
    let c = check_chain(&rpc, Some(REGISTRY), &root, Some(ME));
    assert_eq!(
        c,
        ChainCheck::NotAnchored {
            registry: REGISTRY.to_string()
        }
    );
}
