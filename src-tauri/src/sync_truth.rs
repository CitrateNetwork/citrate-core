//! What the node itself says about its sync (CORE-N4, core#589; ADR-2026-11-03 CV-7b).
//!
//! Two reads of the node's own local RPC, both feature-detected, both refreshed with
//! `node_status` every 2 s:
//!
//! - `citrate_nodeIdentity.syncState` (citrate-chain `core/api/src/node_rpc.rs`
//!   `sync_state_json`, r1007 nodes): `synced`, `catching_up`
//!   or `wedged` (the node's applied tip is held on a dead branch it cannot switch from, from its
//!   durable dead-branch marker), with the fork depth and the highest network height its sync
//!   driver has seen. A node without it (every node before r1007) gives `None`: not read.
//! - `net_peerInfo[].headHeight`: the head height each connected peer announced. Today's node
//!   lists peers without a height, so this is `None` until the node exposes it.
//!
//! The webview decides the node word from these and the public RPC head
//! (`src/surfaces/node/nodeWords.ts`): "Synced" only when this node is within N blocks of every
//! view it has, the peers' view being the height a strict majority of them are at or above.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `citrate_nodeIdentity.syncState`, as the node reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSyncState {
    /// `synced`, `catching_up` or `wedged`.
    pub state: String,
    /// The node's own reason, when it gives one.
    pub reason: Option<String>,
    /// How deep the dead branch is (wedged only).
    pub fork_depth: Option<u64>,
    /// The node's applied tip height.
    pub applied_height: Option<u64>,
    /// The highest network height the node's sync driver has seen (`None`: not wired).
    pub highest_seen: Option<u64>,
}

/// The states this build understands; anything else is "not read".
const STATES: [&str; 3] = ["synced", "catching_up", "wedged"];

/// A JSON number or `0x` hex quantity as u64.
fn quantity(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => {
            let hex = s.strip_prefix("0x")?;
            u64::from_str_radix(hex, 16).ok()
        }
        _ => None,
    }
}

/// `syncState` from a `citrate_nodeIdentity` result; `None` when the node does not report one or
/// reports a state this build does not know.
pub fn parse_sync_state(identity: &Value) -> Option<NodeSyncState> {
    let s = identity.get("syncState")?.as_object()?;
    let state = s.get("state")?.as_str()?;
    if !STATES.contains(&state) {
        return None;
    }
    Some(NodeSyncState {
        state: state.to_string(),
        reason: s.get("reason").and_then(Value::as_str).map(str::to_string),
        fork_depth: s.get("forkDepth").and_then(quantity),
        applied_height: s.get("appliedHeight").and_then(quantity),
        highest_seen: s.get("highestSeen").and_then(quantity),
    })
}

/// The head heights the node's peers announced (`net_peerInfo[].headHeight`); `None` when no
/// peer entry carries one (the node does not expose them).
pub fn parse_peer_tips(peer_info: &Value) -> Option<Vec<u64>> {
    let tips: Vec<u64> = peer_info
        .as_array()?
        .iter()
        .filter_map(|p| p.get("headHeight").and_then(quantity))
        .collect();
    (!tips.is_empty()).then_some(tips)
}

/// True when the node reports itself wedged.
pub fn is_wedged(s: Option<&NodeSyncState>) -> bool {
    s.is_some_and(|s| s.state == "wedged")
}

/// [`crate::node::sync_progress`], except that a node reporting itself wedged is never caught up:
/// its progress is unknown (-1.0), whatever its height says.
pub fn sync_progress_for(height: u64, tip: Option<u64>, state: Option<&NodeSyncState>) -> f64 {
    if is_wedged(state) {
        return -1.0;
    }
    crate::node::sync_progress(height, tip)
}
