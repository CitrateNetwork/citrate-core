// Tests for the desktop mining gate. Included into `mining_gate::tests`.

use super::*;
use crate::rpc::RpcError;
use std::collections::HashMap;

const ME: &str = "0x88fd000000000000000000000000000000000001";
const FLEET_A: &str = "0x6f24c86bea62e4676217f1cec762c8ebaeff2880";
const FLEET_B: &str = "0x47fb000000000000000000000000000000000002";
const NOW: u64 = 1_800_000_000;

fn tip(number: u64, miner: &str, age: u64) -> BlockRef {
    BlockRef { number, hash: format!("0xh{number}"), miner: miner.into(), timestamp: NOW - age }
}

/// Evidence where every condition holds.
fn healthy() -> MiningEvidence {
    MiningEvidence {
        coinbase: Some(ME.into()),
        local_head: Some(1_000),
        local_peers: Some(4),
        network_tip: Some(tip(1_002, FLEET_A, 3)),
        branch_check: Some((992, Some("0xabc".into()), Some("0xABC".into()))),
        recent_miners: vec![FLEET_A.into(), FLEET_B.into(), FLEET_A.into()],
        now_secs: NOW,
    }
}

#[test]
fn arms_only_when_every_condition_holds() {
    assert_eq!(evaluate(&healthy()), Ok(()));
}

#[test]
fn no_coinbase_holds() {
    let ev = MiningEvidence { coinbase: None, ..healthy() };
    assert_eq!(evaluate(&ev), Err(MiningHold::NoCoinbase));
}

#[test]
fn unreachable_network_holds() {
    let ev = MiningEvidence { network_tip: None, ..healthy() };
    assert_eq!(evaluate(&ev), Err(MiningHold::NetworkUnreachable));
}

/// The 2026-10-14 shape: the public RPC froze while a desktop node kept sealing alone. A
/// stale network tip must hold production even though the local node looks caught up to it.
#[test]
fn stalled_network_tip_holds() {
    let ev = MiningEvidence { network_tip: Some(tip(1_002, FLEET_A, 3_600)), ..healthy() };
    assert_eq!(evaluate(&ev), Err(MiningHold::NetworkStalled { age_secs: 3_600 }));
}

#[test]
fn behind_holds() {
    let ev = MiningEvidence { local_head: Some(500), ..healthy() };
    assert_eq!(evaluate(&ev), Err(MiningHold::Behind { local: 500, tip: 1_002 }));
}

/// Same height, different branch: the height check alone would arm here.
#[test]
fn another_branch_holds() {
    let ev = MiningEvidence {
        branch_check: Some((992, Some("0xaaa".into()), Some("0xbbb".into()))),
        ..healthy()
    };
    assert_eq!(evaluate(&ev), Err(MiningHold::OnAnotherBranch { height: 992 }));
}

#[test]
fn missing_branch_reads_hold() {
    let local_missing = MiningEvidence { branch_check: Some((992, None, Some("0x1".into()))), ..healthy() };
    assert_eq!(evaluate(&local_missing), Err(MiningHold::NodeRpcDown));
    let public_missing = MiningEvidence { branch_check: Some((992, Some("0x1".into()), None)), ..healthy() };
    assert_eq!(evaluate(&public_missing), Err(MiningHold::NetworkUnreachable));
    let none = MiningEvidence { branch_check: None, ..healthy() };
    assert_eq!(evaluate(&none), Err(MiningHold::NetworkUnreachable));
}

#[test]
fn too_few_peers_holds() {
    let ev = MiningEvidence { local_peers: Some(1), ..healthy() };
    assert_eq!(evaluate(&ev), Err(MiningHold::TooFewPeers { peers: 1 }));
    let unknown = MiningEvidence { local_peers: None, ..healthy() };
    assert_eq!(evaluate(&unknown), Err(MiningHold::TooFewPeers { peers: 0 }));
}

/// One fleet producer (today's network) holds desktop production.
#[test]
fn a_single_other_producer_holds() {
    let ev = MiningEvidence { recent_miners: vec![FLEET_A.into(); 24], ..healthy() };
    assert_eq!(evaluate(&ev), Err(MiningHold::TooFewProducers { others: 1 }));
}

/// This node's own blocks never count toward the producer minimum (case-insensitive).
#[test]
fn own_blocks_do_not_count_as_other_producers() {
    let ev = MiningEvidence {
        recent_miners: vec![FLEET_A.into(), ME.to_ascii_uppercase().replace("0X", "0x"), ME.into()],
        ..healthy()
    };
    assert_eq!(evaluate(&ev), Err(MiningHold::TooFewProducers { others: 1 }));
}

#[test]
fn producer_addresses_are_compared_case_insensitively() {
    let ev = MiningEvidence {
        recent_miners: vec![FLEET_A.into(), FLEET_A.to_ascii_uppercase().replace("0X", "0x")],
        ..healthy()
    };
    assert_eq!(evaluate(&ev), Err(MiningHold::TooFewProducers { others: 1 }));
}

#[test]
fn every_hold_names_its_reason() {
    for (hold, needle) in [
        (MiningHold::NetworkStalled { age_secs: 90 }, "90s old"),
        (MiningHold::Behind { local: 5, tip: 9 }, "block 5 of 9"),
        (MiningHold::OnAnotherBranch { height: 7 }, "block 7"),
        (MiningHold::TooFewPeers { peers: 1 }, "1 peer(s)"),
        (MiningHold::TooFewProducers { others: 1 }, "1 other producer(s)"),
    ] {
        let text = hold.to_string();
        assert!(text.starts_with("Block production is off"), "{text}");
        assert!(text.contains(needle), "{text} should contain {needle}");
    }
}

#[test]
fn sample_heights_span_the_window_newest_first() {
    let hs = sample_heights(10_000);
    assert_eq!(hs.len() as u64, PRODUCER_SAMPLES);
    assert_eq!(hs.first(), Some(&10_000));
    assert!(hs.windows(2).all(|w| w[0] > w[1]), "strictly descending: {hs:?}");
    assert!(*hs.last().unwrap_or(&0) > 10_000 - PRODUCER_WINDOW, "{hs:?}");
    // Near genesis: never height 0.
    assert_eq!(sample_heights(5), vec![5]);
}

/// A chain served over JSON-RPC: height → (hash, miner, timestamp), plus a peer count.
struct FakeChain {
    blocks: HashMap<u64, (String, String, u64)>,
    peers: u64,
}

impl FakeChain {
    fn new(head: u64, miner_of: impl Fn(u64) -> String, peers: u64) -> Self {
        let blocks = (1..=head).map(|n| (n, (format!("0x{n:064x}"), miner_of(n), NOW - (head - n) * 2))).collect();
        FakeChain { blocks, peers }
    }
    fn head(&self) -> u64 {
        self.blocks.keys().copied().max().unwrap_or(0)
    }
}

impl RpcTransport for FakeChain {
    fn call(&self, body: Value) -> std::result::Result<Value, RpcError> {
        let method = body.get("method").and_then(Value::as_str).unwrap_or("");
        let params = body.get("params").cloned().unwrap_or(json!([]));
        let result = match method {
            "eth_blockNumber" => json!(format!("0x{:x}", self.head())),
            "net_peerCount" => json!(format!("0x{:x}", self.peers)),
            "eth_getBlockByNumber" => {
                let n = match params.get(0).and_then(Value::as_str) {
                    Some("latest") => self.head(),
                    Some(h) => u64::from_str_radix(h.trim_start_matches("0x"), 16)
                        .map_err(|e| RpcError::Node(e.to_string()))?,
                    None => return Err(RpcError::Node("no block param".into())),
                };
                match self.blocks.get(&n) {
                    Some((hash, miner, ts)) => json!({
                        "number": format!("0x{n:x}"),
                        "hash": hash,
                        "miner": miner,
                        "timestamp": format!("0x{ts:x}"),
                    }),
                    None => Value::Null,
                }
            }
            other => return Err(RpcError::Node(format!("unexpected method {other}"))),
        };
        Ok(json!({ "jsonrpc": "2.0", "id": 1, "result": result }))
    }
}

struct Down;
impl RpcTransport for Down {
    fn call(&self, _: Value) -> std::result::Result<Value, RpcError> {
        Err(RpcError::Transport("connection refused".into()))
    }
}

fn rotating(n: u64) -> String {
    [FLEET_A, FLEET_B][(n % 2) as usize].to_string()
}

#[test]
fn gather_reads_a_healthy_multi_producer_network_as_ready() {
    let public = FakeChain::new(2_000, rotating, 0);
    let local = FakeChain::new(1_995, rotating, 3);
    let ev = gather(&local, &public, Some(ME.into()), NOW);
    assert_eq!(ev.local_head, Some(1_995));
    assert_eq!(ev.network_tip.as_ref().map(|t| t.number), Some(2_000));
    assert_eq!(ev.branch_check.as_ref().map(|b| b.0), Some(1_995 - BRANCH_CHECK_DEPTH));
    assert_eq!(ev.recent_miners.len() as u64, PRODUCER_SAMPLES);
    assert_eq!(evaluate(&ev), Ok(()));
}

/// The 2026-10-14 incident shape from the desktop side: one fleet producer.
/// Producers taking turns every block are both seen (sampling must not alias with them).
#[test]
fn sampling_sees_producers_in_short_rotations() {
    for period in 2..=6u64 {
        let miners: Vec<String> = sample_heights(10_000)
            .into_iter()
            .map(|h| format!("p{}", h % period))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(miners.len() as u64, period, "rotation of {period} fully seen");
    }
}

#[test]
fn gather_holds_next_to_a_single_fleet_producer() {
    let public = FakeChain::new(2_000, |_| FLEET_A.to_string(), 0);
    let local = FakeChain::new(2_000, |_| FLEET_A.to_string(), 3);
    let ev = gather(&local, &public, Some(ME.into()), NOW);
    assert_eq!(evaluate(&ev), Err(MiningHold::TooFewProducers { others: 1 }));
}

#[test]
fn gather_detects_a_local_node_on_another_branch() {
    let public = FakeChain::new(2_000, rotating, 0);
    let mut local = FakeChain::new(2_000, rotating, 3);
    for (n, b) in local.blocks.iter_mut() {
        if *n > 1_900 {
            b.0 = format!("0xfork{n}");
        }
    }
    let ev = gather(&local, &public, Some(ME.into()), NOW);
    assert_eq!(evaluate(&ev), Err(MiningHold::OnAnotherBranch { height: 2_000 - BRANCH_CHECK_DEPTH }));
}

#[test]
fn gather_holds_when_either_rpc_is_down() {
    let public = FakeChain::new(2_000, rotating, 0);
    let local = FakeChain::new(2_000, rotating, 3);
    assert_eq!(evaluate(&gather(&Down, &public, Some(ME.into()), NOW)), Err(MiningHold::NodeRpcDown));
    assert_eq!(evaluate(&gather(&local, &Down, Some(ME.into()), NOW)), Err(MiningHold::NetworkUnreachable));
}

#[test]
fn gather_without_a_coinbase_reads_nothing() {
    let ev = gather(&Down, &Down, None, NOW);
    assert_eq!(evaluate(&ev), Err(MiningHold::NoCoinbase));
}
