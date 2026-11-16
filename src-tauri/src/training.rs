//! Training rounds (SETL-S3) — a T1 money surface (D-23). REAL 40204 reads: the desktop reads a
//! group's current round + phase from PatronageLedger, and the member's cumulative weight +
//! claimable SALT, all via public `eth_call` (Rule 1 — never a fabricated round). The WRITE paths
//! are honestly gated: recordContribution is SETTLER-only (contributions meter through the
//! coordinator), and real member SALT crediting is @rule8-gated (gateSec not yet passed), so
//! contribute/claim report that plainly rather than pretend. Signing (a future claimDividend) routes
//! through the SignatureCeremony — Rule 3, nothing here signs.
use serde::Serialize;
use sha3::{Digest, Keccak256};

use crate::agent_sbt::decode_u128;
use crate::rpc::RpcClient;

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

/// One `eth_call` returning a 32-byte word, telling a failed read from an empty one: Err when the
/// RPC call itself failed (unknown, never zero), Ok(None) on a short or empty return.
fn read_word<T: crate::rpc::RpcTransport>(
    rpc: &RpcClient<T>,
    to: &str,
    calldata: &[u8],
) -> Result<Option<[u8; 32]>, String> {
    let call = serde_json::json!({ "to": to, "data": format!("0x{}", hex::encode(calldata)) });
    let ret = rpc.eth_call(call).map_err(|e| e.to_string())?;
    if ret.len() < 32 {
        return Ok(None);
    }
    let mut word = [0u8; 32];
    word.copy_from_slice(&ret[..32]);
    Ok(Some(word))
}

/// A uint word the ledger must return. A failed read or a missing value is an error with its
/// reason, never 0 (0.5.4: Hermes reports patronage from this, and unknown is not zero).
fn read_uint<T: crate::rpc::RpcTransport>(
    rpc: &RpcClient<T>,
    to: &str,
    calldata: &[u8],
    what: &str,
) -> Result<u128, String> {
    match read_word(rpc, to, calldata) {
        Err(e) => Err(format!("PatronageLedger {what} could not be read: {e}")),
        Ok(None) => Err(format!(
            "PatronageLedger {what} returned no value (no contract code at {to}?)"
        )),
        Ok(Some(w)) => {
            decode_u128(&w).map_err(|e| format!("PatronageLedger {what} is not a number: {e}"))
        }
    }
}

/// The member's cumulative patronage units and pending dividend (wei) on PatronageLedger.
pub fn read_patronage<T: crate::rpc::RpcTransport>(
    rpc: &RpcClient<T>,
    ledger: &str,
    member: &str,
) -> Result<(u128, u128), String> {
    let word = left_pad_address(member);
    let mut units_call = SEL_UNITS.to_vec();
    units_call.extend_from_slice(&word);
    let weight = read_uint(rpc, ledger, &units_call, "units()")?;
    let mut pending_call = SEL_PENDING_DIVIDEND_OF.to_vec();
    pending_call.extend_from_slice(&word);
    let pending_wei = read_uint(rpc, ledger, &pending_call, "pendingDividendOf()")?;
    Ok((weight, pending_wei))
}

/// The highest committed round of a group on PatronageLedger (0 when none). A failed read is an
/// error, not "no rounds".
pub fn scan_rounds<T: crate::rpc::RpcTransport>(
    rpc: &RpcClient<T>,
    ledger: &str,
    gid: &[u8; 32],
) -> Result<u64, String> {
    let mut current = 0u64;
    for n in 1..=MAX_ROUND_SCAN {
        let mut calldata = SEL_ROUND_MERGED_HASH.to_vec();
        calldata.extend_from_slice(&round_key(gid, n));
        match read_word(rpc, ledger, &calldata)
            .map_err(|e| format!("PatronageLedger roundMergedHash() could not be read: {e}"))?
        {
            Some(word) if !is_zero(&word) => current = n,
            _ => break, // first unset round → no further rounds
        }
    }
    Ok(current)
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
    /// The same dividend exactly, in wei (decimal string).
    pub pending_wei: String,
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
pub async fn training_status(
    app_h: tauri::AppHandle,
    group: String,
) -> Result<RoundStatus, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || training_status_sync(app_h.clone(), group)).await
}

/// Blocking body of [`training_status`]; reached only through [`crate::blocking::off_main`].
pub fn training_status_sync(app: tauri::AppHandle, group: String) -> Result<RoundStatus, String> {
    let _ = &app;
    let ledger = patronage_ledger()?;
    let rpc = RpcClient::citrate();
    let gid = group_bytes32(&group);
    let current = scan_rounds(&rpc, ledger, &gid)?;
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
pub async fn training_reward(app_h: tauri::AppHandle, group: String) -> Result<RewardInfo, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st1 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        training_reward_sync(app_h.clone(), st1, group)
    })
    .await
}

/// Blocking body of [`training_reward`]; reached only through [`crate::blocking::off_main`].
pub fn training_reward_sync(
    app: tauri::AppHandle,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    group: String,
) -> Result<RewardInfo, String> {
    let ledger = patronage_ledger()?;
    let rpc = RpcClient::citrate();
    let member = crate::wallet::address(&custody.0)
        .map_err(|e| e.to_string())?
        .address;
    let (weight, pending_wei) = read_patronage(&rpc, ledger, &member)?;
    let salt_whole = pending_wei / 1_000_000_000_000_000_000u128; // wei → whole SALT

    // Round is informational here; the surface pairs this with training_status.
    let round = training_status_sync(app, group)?.round;
    Ok(RewardInfo {
        round,
        weight: weight.to_string(),
        salt: salt_whole.to_string(),
        pending_wei: pending_wei.to_string(),
    })
}

/// Opening a round is SETTLER-only (the coordinator commits it). Honest, not a sim.
#[tauri::command]
pub fn training_start(_group: String) -> Result<(), String> {
    Err(
        "opening a training round is done by the settlement coordinator, not the member"
            .to_string(),
    )
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
pub async fn training_claim(app_h: tauri::AppHandle, group: String) -> Result<(), String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        training_claim_sync(st0, group)
    })
    .await
}

/// Blocking body of [`training_claim`]; reached only through [`crate::blocking::off_main`].
pub fn training_claim_sync(
    custody: tauri::State<'_, crate::custody::CustodyState>,
    group: String,
) -> Result<(), String> {
    let ledger = patronage_ledger()?;
    let rpc = RpcClient::citrate();
    let member = crate::wallet::address(&custody.0)
        .map_err(|e| e.to_string())?
        .address;
    let mut pending_call = SEL_PENDING_DIVIDEND_OF.to_vec();
    pending_call.extend_from_slice(&left_pad_address(&member));
    let pending = read_uint(&rpc, ledger, &pending_call, "pendingDividendOf()")?;
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

    /// A scripted eth_call: each answer in turn, Err for a transport failure.
    struct Answers(std::cell::RefCell<Vec<Result<serde_json::Value, String>>>);

    impl crate::rpc::RpcTransport for Answers {
        fn call(
            &self,
            _body: serde_json::Value,
        ) -> Result<serde_json::Value, crate::rpc::RpcError> {
            match self.0.borrow_mut().remove(0) {
                Ok(v) => Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": v })),
                Err(e) => Err(crate::rpc::RpcError::Transport(e)),
            }
        }
    }

    fn rpc(answers: Vec<Result<serde_json::Value, String>>) -> RpcClient<Answers> {
        RpcClient::with_transport(Answers(std::cell::RefCell::new(answers)))
    }

    fn uint(n: u128) -> serde_json::Value {
        serde_json::json!(format!("0x{n:064x}"))
    }

    const LEDGER: &str = "0x41f68b9e710585b064459a0ec706148fbf503659";
    const ME: &str = "0x00000000000000000000000000000000000000ff";

    #[test]
    fn patronage_reads_units_and_pending_wei_exactly() {
        let r = rpc(vec![Ok(uint(7)), Ok(uint(1_500_000_000_000_000_000))]);
        assert_eq!(
            read_patronage(&r, LEDGER, ME).unwrap(),
            (7, 1_500_000_000_000_000_000)
        );
    }

    #[test]
    fn a_failed_patronage_read_is_an_error_never_zero() {
        let r = rpc(vec![Err("connection refused".into())]);
        let e = read_patronage(&r, LEDGER, ME).unwrap_err();
        assert!(e.contains("units() could not be read"), "{e}");
        let r = rpc(vec![Ok(uint(7)), Ok(serde_json::json!("0x"))]);
        let e = read_patronage(&r, LEDGER, ME).unwrap_err();
        assert!(e.contains("pendingDividendOf() returned no value"), "{e}");
    }

    #[test]
    fn round_scan_stops_at_the_first_unset_round_and_fails_on_a_read_error() {
        let gid = [0x11u8; 32];
        let r = rpc(vec![Ok(uint(1)), Ok(uint(9)), Ok(uint(0))]);
        assert_eq!(scan_rounds(&r, LEDGER, &gid).unwrap(), 2);
        let r = rpc(vec![Ok(uint(1)), Err("timeout".into())]);
        assert!(scan_rounds(&r, LEDGER, &gid)
            .unwrap_err()
            .contains("roundMergedHash() could not be read"));
    }

    #[test]
    fn left_pad_address_right_aligns_20_bytes() {
        let w = left_pad_address("0x00000000000000000000000000000000000000ff");
        assert_eq!(w[31], 0xff);
        assert!(w[..12].iter().all(|&b| b == 0));
    }
}
