//! Training rounds (SETL-S3) — a T1 money surface (D-23). REAL 40204 reads: the desktop reads a
//! group's current round + phase from PatronageLedger, and the member's cumulative weight +
//! claimable SALT, all via public `eth_call` (Rule 1 — never a fabricated round). The WRITE paths
//! are honestly gated: recordContribution is SETTLER-only (contributions meter through the
//! coordinator), and real member SALT crediting is @rule8-gated (gateSec not yet passed), so
//! contribute/claim report that plainly rather than pretend. Signing (a future claimDividend) routes
//! through the SignatureCeremony — Rule 3, nothing here signs.
use serde::Serialize;
use sha3::{Digest, Keccak256};

use crate::rpc::RpcClient;
use crate::staking::decode_uint256_word;

// Selectors confirmed by the chain team (SETL-S3 reply). Pinned, not recomputed — the exact
// argument types (e.g. roundMergedHash(bytes32)) are the ledger's, and these are the source of truth.
const SEL_ROUND_MERGED_HASH: [u8; 4] = [0xa4, 0x40, 0x2c, 0xa3]; // roundMergedHash(bytes32) -> bytes32
const SEL_UNITS: [u8; 4] = [0x00, 0xeb, 0xa3, 0x50]; // units(address) -> uint256 (cumulative weight)
const SEL_PENDING_DIVIDEND_OF: [u8; 4] = [0x8c, 0x7b, 0xfd, 0x2c]; // pendingDividendOf(address) -> uint256 wei

/// Bound the round scan so a group with no rounds is a few cheap calls, not an unbounded loop.
const MAX_ROUND_SCAN: u64 = 128;

/// The group's on-chain round key: `keccak256(abi.encode(bytes32 groupId, uint256 n))`, n 1-based.
fn round_key(group_id: &[u8; 32], n: u64) -> [u8; 32] {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(group_id);
    buf[56..64].copy_from_slice(&n.to_be_bytes()); // uint256 big-endian, low 8 bytes
    let out = Keccak256::digest(buf);
    let mut key = [0u8; 32];
    key.copy_from_slice(&out);
    key
}

/// Map the app's group id to the on-chain `bytes32 groupId`. A 0x+64-hex id is used verbatim (the
/// comms daemon's ids are already bytes32); anything else is `keccak256(id)` so it still resolves
/// deterministically.
///
/// VERIFIED end-to-end against 40204 (SETL-S3): the comms member-daemon serializes a group id as
/// `hex::encode(gid.0)` — bare 64-hex, no `0x` (comms-member-daemon/src/ipc.rs) — which this decodes
/// verbatim. `roundMergedHash(0xa4402ca3 ++ round_key(groupId, 4))` for the golden group returned a
/// non-zero merged hash on rpc.citrate.ai, confirming both the mapping and the round-key encoding are
/// correct. Do not "fix" this to hash the hex string — that would break the verified read path.
fn group_bytes32(group: &str) -> [u8; 32] {
    let stripped = group.strip_prefix("0x").unwrap_or(group);
    if stripped.len() == 64 {
        if let Ok(bytes) = hex::decode(stripped) {
            let mut b = [0u8; 32];
            b.copy_from_slice(&bytes);
            return b;
        }
    }
    let out = Keccak256::digest(group.as_bytes());
    let mut b = [0u8; 32];
    b.copy_from_slice(&out);
    b
}

fn left_pad_address(addr: &str) -> [u8; 32] {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    let mut word = [0u8; 32];
    if let Ok(bytes) = hex::decode(stripped) {
        if bytes.len() == 20 {
            word[12..32].copy_from_slice(&bytes);
        }
    }
    word
}

/// One `eth_call` returning a 32-byte word, or None on a short/empty return (no code / not set).
fn call_word<T: crate::rpc::RpcTransport>(rpc: &RpcClient<T>, to: &str, calldata: &[u8]) -> Option<[u8; 32]> {
    let call = serde_json::json!({ "to": to, "data": format!("0x{}", hex::encode(calldata)) });
    let ret = rpc.eth_call(call).ok()?;
    if ret.len() < 32 {
        return None;
    }
    let mut word = [0u8; 32];
    word.copy_from_slice(&ret[..32]);
    Some(word)
}

fn is_zero(word: &[u8; 32]) -> bool {
    word.iter().all(|&b| b == 0)
}

// ── DTOs (mirror the frozen TS TrainingDomain: RoundStatus / RewardInfo) ──

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundStatus {
    pub group_id: String,
    pub round: u64,
    /// "idle" (no committed round) | "committed" (a round's merged hash is set). The ledger commits
    /// atomically, so open/aggregating/settled never appear on-chain.
    pub phase: String,
    pub participants: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardInfo {
    pub round: u64,
    /// Cumulative patronage weight (a count, from `units`).
    pub weight: String,
    /// Claimable dividend in whole SALT (from `pendingDividendOf`, wei / 1e18).
    pub salt: String,
}

fn patronage_ledger() -> Result<&'static str, String> {
    let a = crate::addresses::patronage_ledger();
    if a.is_empty() || a == "0x0000000000000000000000000000000000000000" {
        return Err("the settlement ledger address is not confirmed on 40204 yet".to_string());
    }
    Ok(a)
}

/// The group's current round + phase, read from PatronageLedger. Scans roundMergedHash for the
/// highest committed round; honest "idle / round 0" when none.
#[tauri::command]
pub fn training_status(app: tauri::AppHandle, group: String) -> Result<RoundStatus, String> {
    let _ = &app;
    let ledger = patronage_ledger()?;
    let rpc = RpcClient::citrate();
    let gid = group_bytes32(&group);

    let mut current = 0u64;
    for n in 1..=MAX_ROUND_SCAN {
        let mut calldata = SEL_ROUND_MERGED_HASH.to_vec();
        calldata.extend_from_slice(&round_key(&gid, n));
        match call_word(&rpc, ledger, &calldata) {
            Some(word) if !is_zero(&word) => current = n,
            _ => break, // first unset round → no further rounds
        }
    }
    let phase = if current > 0 { "committed" } else { "idle" };
    // Participant count needs PatronageRecorded event indexing (follow-up); report 0 = "uncounted"
    // rather than a fabricated number. The UI shows "—" for 0.
    Ok(RoundStatus {
        group_id: group,
        round: current,
        phase: phase.to_string(),
        participants: 0,
    })
}

/// The member's reward for a group's current round: cumulative weight + claimable SALT, read from
/// PatronageLedger by the wallet address. Reads only; signs nothing.
#[tauri::command]
pub fn training_reward(
    app: tauri::AppHandle,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    group: String,
) -> Result<RewardInfo, String> {
    let ledger = patronage_ledger()?;
    let rpc = RpcClient::citrate();
    let member = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?.address;
    let word = left_pad_address(&member);

    let mut units_call = SEL_UNITS.to_vec();
    units_call.extend_from_slice(&word);
    let weight = call_word(&rpc, ledger, &units_call)
        .and_then(|w| decode_uint256_word(&w).ok())
        .unwrap_or(0);

    let mut pending_call = SEL_PENDING_DIVIDEND_OF.to_vec();
    pending_call.extend_from_slice(&word);
    let pending_wei = call_word(&rpc, ledger, &pending_call)
        .and_then(|w| decode_uint256_word(&w).ok())
        .unwrap_or(0);
    let salt_whole = pending_wei / 1_000_000_000_000_000_000u128; // wei → whole SALT

    // Round is informational here; the surface pairs this with training_status.
    let round = training_status(app, group).map(|s| s.round).unwrap_or(0);
    Ok(RewardInfo {
        round,
        weight: weight.to_string(),
        salt: salt_whole.to_string(),
    })
}

/// Opening a round is SETTLER-only (the coordinator commits it). Honest, not a sim.
#[tauri::command]
pub fn training_start(_group: String) -> Result<(), String> {
    Err("opening a training round is done by the settlement coordinator, not the member".to_string())
}

/// Contributions meter through the coordinator; recordContribution is SETTLER-only, and real SALT
/// crediting is @rule8-gated (gateSec not yet passed). Say so plainly rather than fake a submit.
#[tauri::command]
pub fn training_contribute(_group: String) -> Result<(), String> {
    Err("contributions meter through the settlement coordinator, and member SALT crediting is not yet authorized on 40204 (pending @rule8 / gateSec)".to_string())
}

/// Claim is member-callable + ceremony-gated (ModelCooperative.claimDividend). Until revenue is
/// credited (gateSec), pendingDividend is 0 and there is nothing to claim — reported honestly. When
/// dividends flow, this builds a claimDividend intent that stops at the SignatureCeremony (Rule 3).
#[tauri::command]
pub fn training_claim(
    custody: tauri::State<'_, crate::custody::CustodyState>,
    group: String,
) -> Result<(), String> {
    let ledger = patronage_ledger()?;
    let rpc = RpcClient::citrate();
    let member = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?.address;
    let mut pending_call = SEL_PENDING_DIVIDEND_OF.to_vec();
    pending_call.extend_from_slice(&left_pad_address(&member));
    let pending = call_word(&rpc, ledger, &pending_call)
        .and_then(|w| decode_uint256_word(&w).ok())
        .unwrap_or(0);
    let _ = group;
    if pending == 0 {
        return Err("no dividend to claim yet — member SALT crediting activates once revenue is credited (pending @rule8 / gateSec)".to_string());
    }
    // A real dividend exists: the claimDividend ceremony wiring lands with the first credited round.
    Err("a claimable dividend exists — the ceremony-gated claimDividend path lands with the first credited round".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_key_is_keccak_of_group_and_index() {
        let gid = [0x11u8; 32];
        let k1 = round_key(&gid, 1);
        let k2 = round_key(&gid, 2);
        assert_ne!(k1, k2, "distinct rounds → distinct keys");
        // deterministic
        assert_eq!(k1, round_key(&gid, 1));
    }

    #[test]
    fn group_bytes32_uses_hex_verbatim_else_keccak() {
        let hex_id = "0x".to_string() + &"ab".repeat(32);
        let b = group_bytes32(&hex_id);
        assert_eq!(b, [0xabu8; 32]);
        // a non-hex id hashes (deterministic, 32 bytes)
        let h = group_bytes32("grp_dana");
        assert_eq!(h, group_bytes32("grp_dana"));
        assert_ne!(h, [0u8; 32]);
    }

    #[test]
    fn left_pad_address_right_aligns_20_bytes() {
        let w = left_pad_address("0x00000000000000000000000000000000000000ff");
        assert_eq!(w[31], 0xff);
        assert!(w[..12].iter().all(|&b| b == 0));
    }
}
