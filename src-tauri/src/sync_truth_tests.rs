// CORE-N4 (core#589): what the node itself says about its sync, read from its local RPC and
// feature-detected, so a node without the reads (today's chain) gives "not read", never a guess.

use crate::sync_truth::{parse_peer_tips, parse_sync_state, sync_progress_for, NodeSyncState};
use serde_json::json;

#[test]
fn sync_state_is_read_from_node_identity_when_the_node_has_it() {
    // citrate-chain core/api/src/node_rpc.rs sync_state_json (r1007 nodes), wedged form.
    let id = json!({
        "syncState": {
            "state": "wedged", "reason": "refused a switch off a dead branch", "forkDepth": 5120,
            "appliedHeight": 509000, "highestSeen": 510032,
            "deadTip": {"hash": "0x01", "height": 509000}, "since": 1793000000u64
        },
        "chainId": "0x9d0c", "genesisHash": "0xab"
    });
    assert_eq!(
        parse_sync_state(&id),
        Some(NodeSyncState {
            state: "wedged".into(),
            reason: Some("refused a switch off a dead branch".into()),
            fork_depth: Some(5120),
            applied_height: Some(509_000),
            highest_seen: Some(510_032),
        })
    );
    let synced = json!({"syncState": {"state": "synced", "reason": null, "forkDepth": null,
        "appliedHeight": 12, "highestSeen": null}});
    assert_eq!(parse_sync_state(&synced).map(|s| s.state), Some("synced".to_string()));
    let catching = json!({"syncState": {"state": "catching_up", "reason": "x", "forkDepth": null,
        "appliedHeight": 12, "highestSeen": 99}});
    assert_eq!(parse_sync_state(&catching).and_then(|s| s.highest_seen), Some(99));
}

#[test]
fn a_node_without_sync_state_or_with_an_unknown_one_is_not_read() {
    // Today's node: citrate_nodeIdentity has no syncState.
    assert_eq!(parse_sync_state(&json!({"chainId": "0x9d0c"})), None);
    assert_eq!(parse_sync_state(&json!(null)), None);
    assert_eq!(parse_sync_state(&json!({"syncState": "wedged"})), None);
    assert_eq!(parse_sync_state(&json!({"syncState": {"state": "maybe"}})), None);
    assert_eq!(parse_sync_state(&json!({"syncState": {"state": 3}})), None);
}

#[test]
fn peer_tips_are_the_reported_head_heights_and_absent_when_no_peer_reports_one() {
    // Today's net_peerInfo: id, addr, direction, no height.
    let today = json!([{"id": "a", "addr": "1.2.3.4:30303", "direction": "outbound"}]);
    assert_eq!(parse_peer_tips(&today), None);
    assert_eq!(parse_peer_tips(&json!([])), None);
    assert_eq!(parse_peer_tips(&json!({"not": "a list"})), None);
    let with = json!([
        {"id": "a", "headHeight": 510032},
        {"id": "b", "headHeight": "0x7c84f"},
        {"id": "c"},
        {"id": "d", "headHeight": -1},
        {"id": "e", "headHeight": "tall"}
    ]);
    assert_eq!(parse_peer_tips(&with), Some(vec![510_032, 0x7c84f]));
}

#[test]
fn a_wedged_node_never_reports_sync_progress_as_caught_up() {
    let wedged = NodeSyncState {
        state: "wedged".into(),
        reason: None,
        fork_depth: Some(10),
        applied_height: Some(100),
        highest_seen: Some(100),
    };
    // At the tip by height, but wedged: progress is unknown, not 100.
    assert_eq!(sync_progress_for(100, Some(100), Some(&wedged)), -1.0);
    // Otherwise the height rule as before.
    assert_eq!(sync_progress_for(100, Some(100), None), 100.0);
    let synced = NodeSyncState { state: "synced".into(), ..wedged.clone() };
    assert_eq!(sync_progress_for(100, Some(100), Some(&synced)), 100.0);
    assert_eq!(sync_progress_for(100, None, None), -1.0);
}
