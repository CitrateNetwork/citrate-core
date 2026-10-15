//! When a desktop node may produce blocks.
//!
//! Being caught up to the public RPC's height is not enough. A node can sit at the right height
//! on another branch, the public RPC can itself be stalled, and a desktop producer next to a
//! single fleet producer is the case the network does not yet handle well. So the producer is
//! armed only when ALL of these hold, and disarmed again by the watchdog when one stops holding:
//!
//! 1. the network's newest block is recent (the public RPC is not stalled);
//! 2. this node is within [`crate::node::is_caught_up`] of the network tip;
//! 3. this node's own chain (synced from its peers) has the SAME block hash as the public RPC at
//!    a common height, so two sources agree on the branch, not just the height;
//! 4. this node has at least [`MIN_PEERS`] peers;
//! 5. at least [`MIN_OTHER_PRODUCERS`] producers other than this node sealed blocks recently.
//!
//! Every input is read from a real RPC (Rule 1); any read that fails holds production.

use serde_json::{json, Value};

use crate::rpc::RpcTransport;

/// The public RPC's newest block must be at most this old (seconds). 40204 seals every ~2s;
/// a minute of silence means the network (or the public RPC) is not progressing.
pub const MAX_TIP_AGE_SECS: u64 = 60;

/// The minimum live peer count of this node.
pub const MIN_PEERS: u64 = 2;

/// How many producers other than this node must have sealed blocks in the recent window.
pub const MIN_OTHER_PRODUCERS: usize = 2;

/// How many recent public blocks the producer count reads.
pub const PRODUCER_SAMPLES: u64 = 24;

/// The spacing of those reads, in blocks. Prime, so producers that take turns in a short
/// rotation (every 2nd or 3rd block, …) are not all missed by sampling in step with them.
pub const PRODUCER_STEP: u64 = 13;

/// The recent window the producer count spans (~10 minutes at 2s blocks).
pub const PRODUCER_WINDOW: u64 = PRODUCER_SAMPLES * PRODUCER_STEP;

/// How far below the lower of the two heads the branch check compares hashes, so both sides
/// have the block and a fresh tip that has not propagated yet is not a false mismatch.
pub const BRANCH_CHECK_DEPTH: u64 = 8;

/// One block as the gate needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRef {
    pub number: u64,
    pub hash: String,
    pub miner: String,
    pub timestamp: u64,
}

/// What the gate reads. `None` means the read failed.
#[derive(Debug, Clone, Default)]
pub struct MiningEvidence {
    /// This node's coinbase (its own blocks are not counted as other producers).
    pub coinbase: Option<String>,
    pub local_head: Option<u64>,
    pub local_peers: Option<u64>,
    pub network_tip: Option<BlockRef>,
    /// `(height, local hash, public hash)` at the branch-check height.
    pub branch_check: Option<(u64, Option<String>, Option<String>)>,
    /// Miners of the sampled recent public blocks (empty when the reads failed).
    pub recent_miners: Vec<String>,
    /// Wall clock, unix seconds.
    pub now_secs: u64,
}

/// Why production is held. The text is shown to the member as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiningHold {
    NoCoinbase,
    NodeRpcDown,
    NetworkUnreachable,
    NetworkStalled { age_secs: u64 },
    Behind { local: u64, tip: u64 },
    OnAnotherBranch { height: u64 },
    TooFewPeers { peers: u64 },
    TooFewProducers { others: usize },
}

impl std::fmt::Display for MiningHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MiningHold::NoCoinbase => write!(f, "Block production is off: no wallet address yet."),
            MiningHold::NodeRpcDown => write!(f, "Block production is off: the node is not answering yet."),
            MiningHold::NetworkUnreachable => {
                write!(f, "Block production is off: the network could not be reached to confirm the chain head.")
            }
            MiningHold::NetworkStalled { age_secs } => write!(
                f,
                "Block production is off: the network's newest block is {age_secs}s old, so the chain is not progressing."
            ),
            MiningHold::Behind { local, tip } => write!(
                f,
                "Block production is off until this node catches up (block {local} of {tip})."
            ),
            MiningHold::OnAnotherBranch { height } => write!(
                f,
                "Block production is off: at block {height} this node's chain differs from the network's."
            ),
            MiningHold::TooFewPeers { peers } => write!(
                f,
                "Block production is off: this node has {peers} peer(s), it needs at least {MIN_PEERS}."
            ),
            MiningHold::TooFewProducers { others } => write!(
                f,
                "Block production is off: {others} other producer(s) are live, at least {MIN_OTHER_PRODUCERS} are needed first."
            ),
        }
    }
}

/// The decision. Pure: every input is in `ev`.
pub fn evaluate(ev: &MiningEvidence) -> Result<(), MiningHold> {
    let Some(coinbase) = ev.coinbase.as_deref() else {
        return Err(MiningHold::NoCoinbase);
    };
    let Some(local) = ev.local_head else {
        return Err(MiningHold::NodeRpcDown);
    };
    let Some(tip) = ev.network_tip.as_ref() else {
        return Err(MiningHold::NetworkUnreachable);
    };
    let age_secs = ev.now_secs.saturating_sub(tip.timestamp);
    if age_secs > MAX_TIP_AGE_SECS {
        return Err(MiningHold::NetworkStalled { age_secs });
    }
    if !crate::node::is_caught_up(local, tip.number) {
        return Err(MiningHold::Behind {
            local,
            tip: tip.number,
        });
    }
    match &ev.branch_check {
        Some((height, Some(l), Some(p))) => {
            if !l.eq_ignore_ascii_case(p) {
                return Err(MiningHold::OnAnotherBranch { height: *height });
            }
        }
        Some((_, None, _)) => return Err(MiningHold::NodeRpcDown),
        _ => return Err(MiningHold::NetworkUnreachable),
    }
    let peers = ev.local_peers.unwrap_or(0);
    if peers < MIN_PEERS {
        return Err(MiningHold::TooFewPeers { peers });
    }
    let mut others: Vec<String> = ev
        .recent_miners
        .iter()
        .map(|m| m.to_ascii_lowercase())
        .filter(|m| !m.eq_ignore_ascii_case(coinbase))
        .collect();
    others.sort();
    others.dedup();
    if others.len() < MIN_OTHER_PRODUCERS {
        return Err(MiningHold::TooFewProducers {
            others: others.len(),
        });
    }
    Ok(())
}

/// The heights the producer count reads: `PRODUCER_SAMPLES` heights `PRODUCER_STEP` apart,
/// ending at `tip`, newest first, never below 1.
pub fn sample_heights(tip: u64) -> Vec<u64> {
    (0..PRODUCER_SAMPLES)
        .map(|i| tip.saturating_sub(i * PRODUCER_STEP))
        .filter(|h| *h >= 1)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn rpc_result<T: RpcTransport>(t: &T, method: &str, params: Value) -> Option<Value> {
    let resp = t
        .call(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .ok()?;
    if resp.get("error").is_some() {
        return None;
    }
    resp.get("result").cloned()
}

fn hex_u64(v: &Value) -> Option<u64> {
    u64::from_str_radix(v.as_str()?.trim_start_matches("0x"), 16).ok()
}

fn block_number<T: RpcTransport>(t: &T) -> Option<u64> {
    hex_u64(&rpc_result(t, "eth_blockNumber", json!([]))?)
}

/// `eth_getBlockByNumber(n | "latest", false)` reduced to a [`BlockRef`].
pub fn block_ref<T: RpcTransport>(t: &T, number: Option<u64>) -> Option<BlockRef> {
    let tag = match number {
        Some(n) => json!(format!("0x{n:x}")),
        None => json!("latest"),
    };
    let b = rpc_result(t, "eth_getBlockByNumber", json!([tag, false]))?;
    Some(BlockRef {
        number: hex_u64(b.get("number")?)?,
        hash: b.get("hash")?.as_str()?.to_string(),
        miner: b.get("miner")?.as_str()?.to_string(),
        timestamp: hex_u64(b.get("timestamp")?)?,
    })
}

/// Read every input of [`evaluate`] from this node's RPC (`local`) and the public RPC
/// (`public`). Stops reading at the first failed prerequisite; [`evaluate`] then names it.
pub fn gather<L: RpcTransport, P: RpcTransport>(
    local: &L,
    public: &P,
    coinbase: Option<String>,
    now_secs: u64,
) -> MiningEvidence {
    let mut ev = MiningEvidence {
        coinbase,
        now_secs,
        ..MiningEvidence::default()
    };
    if ev.coinbase.is_none() {
        return ev;
    }
    ev.local_head = block_number(local);
    let Some(local_head) = ev.local_head else {
        return ev;
    };
    ev.network_tip = block_ref(public, None);
    let Some(tip) = ev.network_tip.clone() else {
        return ev;
    };
    let height = local_head
        .min(tip.number)
        .saturating_sub(BRANCH_CHECK_DEPTH);
    ev.branch_check = Some((
        height,
        block_ref(local, Some(height)).map(|b| b.hash),
        block_ref(public, Some(height)).map(|b| b.hash),
    ));
    ev.local_peers = rpc_result(local, "net_peerCount", json!([])).and_then(|v| hex_u64(&v));
    ev.recent_miners = sample_heights(tip.number)
        .into_iter()
        .filter_map(|h| block_ref(public, Some(h)).map(|b| b.miner))
        .collect();
    ev
}

/// Unix seconds now (0 if the clock is before 1970, which then reads as a stalled network).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    include!("mining_gate_tests.rs");
}
