// @rule8 — LiquidStakingPool: the self-stake read + the deposit intent builder.
//
// Two load-bearing properties, both grounded (Rule 1 — no fabricated numbers):
//   * The `deposit()` intent's `raw` JSON DECODES (via the same txdecode B1.4 uses)
//     to the exact deposit call — pool `to`, exact staked `value` (no truncation),
//     the real `deposit()` selector as calldata, explicit gas. If this drifts the
//     ceremony would sign a different tx than the human approved.
//   * `read_self_stake` reads `balanceOf(addr)` on the pool and decodes the real
//     uint256 (a fresh wallet honestly reads 0) — never a sim.
//
// Selectors are PINNED constants proven here by an INDEPENDENT Keccak-256
// derivation (dev-only `sha3`, same as the wallet/earnings tests) — a drift in a
// constant or a signature string fails loudly (Rule 11 tripwire).
use super::*;
use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// A scripted mock RPC transport (mirrors earnings_tests / rpc_tests).
// ---------------------------------------------------------------------------
struct MockRpc {
    requests: RefCell<Vec<JsonValue>>,
    responses: RefCell<VecDeque<JsonValue>>,
}
impl MockRpc {
    fn new(responses: Vec<JsonValue>) -> Self {
        MockRpc {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }
    fn requests(&self) -> Vec<JsonValue> {
        self.requests.borrow().clone()
    }
}
impl RpcTransport for MockRpc {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.requests.borrow_mut().push(body.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}
fn ok(result: JsonValue) -> JsonValue {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

/// A 32-byte `uint256` ABI word for `n`, as a `0x`-hex string (the shape an
/// `eth_call` balanceOf read returns).
fn uint256_hex(n: u128) -> String {
    let mut word = [0u8; 32];
    word[16..32].copy_from_slice(&n.to_be_bytes());
    format!("0x{}", hex::encode(word))
}

/// Independent Keccak-256 selector derivation (dev-only `sha3`) — proves the pinned
/// production constants are correct.
fn derive_selector(sig: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

const STAKER_ADDR: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";

// ===========================================================================
// Canonical address + selector pins (Rule 11 tripwires)
// ===========================================================================

/// The pool address MUST be the canonical 40204.json value. A paste error here
/// would send staked SALT to the wrong contract (@rule8) — pin it exactly.
#[test]
fn pool_address_is_the_canonical_40204_value() {
    assert_eq!(
        LIQUID_STAKING_POOL,
        "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e"
    );
}

/// The pinned `deposit()` selector is the REAL keccak of the signature AND the
/// canonical WETH-style value 0xd0e30db0.
#[test]
fn deposit_selector_is_keccak_of_signature() {
    assert_eq!(deposit_selector(), derive_selector("deposit()"));
    assert_eq!(format!("0x{}", hex::encode(deposit_selector())), "0xd0e30db0");
}

/// The pinned `balanceOf(address)` selector is the REAL keccak of the signature AND
/// the canonical ERC-20/721 value 0x70a08231.
#[test]
fn balance_of_selector_is_keccak_of_signature() {
    assert_eq!(balance_of_selector(), derive_selector("balanceOf(address)"));
    assert_eq!(
        format!("0x{}", hex::encode(balance_of_selector())),
        "0x70a08231"
    );
}

// ===========================================================================
// Address validation + the balanceOf read
// ===========================================================================

#[test]
fn validate_address_canonicalizes_and_rejects_malformed() {
    assert_eq!(
        validate_address("0x9858EFFD232B4033E47D90003D41EC34ECAEDA94").unwrap(),
        STAKER_ADDR,
        "canonical lowercased 0x form"
    );
    assert!(validate_address("0x1234").is_err(), "too short");
    assert!(validate_address("notanaddr").is_err(), "no 0x / not hex");
    assert!(
        validate_address("0xZZ58effd232b4033e47d90003d41ec34ecaeda94").is_err(),
        "non-hex digits"
    );
}

#[test]
fn balance_of_calldata_is_selector_plus_left_padded_address() {
    let calldata = encode_balance_of_calldata(STAKER_ADDR);
    assert_eq!(calldata.len(), 36, "4-byte selector + one 32-byte word");
    assert_eq!(&calldata[0..4], &balance_of_selector(), "selector prefix");
    assert!(
        calldata[4..16].iter().all(|&b| b == 0),
        "address is left-padded (12 leading zero bytes)"
    );
    let addr_bytes = hex::decode(&STAKER_ADDR[2..]).unwrap();
    assert_eq!(&calldata[16..36], &addr_bytes[..], "address in the low 20 bytes");
}

#[test]
fn decode_uint256_word_valid_short_and_overflow() {
    // Valid.
    let ret = hex::decode(&uint256_hex(4_200_000_000_000_000_000)[2..]).unwrap();
    assert_eq!(decode_uint256_word(&ret).unwrap(), 4_200_000_000_000_000_000);
    // Short return → decode error (never silently zero).
    assert!(decode_uint256_word(&[0u8; 16]).is_err(), "short word rejected");
    // A value in the high 16 bytes → refuse to truncate (Rule 1).
    let mut overflow = [0u8; 32];
    overflow[0] = 1;
    assert!(
        decode_uint256_word(&overflow).is_err(),
        "value beyond u128 refused, not truncated"
    );
}

#[test]
fn read_self_stake_reads_pool_balance_over_mock_rpc() {
    let staked: u128 = 5_000_000_000_000_000_000; // 5 SALT self-staked
    let rpc = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(
        uint256_hex(staked),
    ))]));
    let got = read_self_stake(&rpc, STAKER_ADDR).expect("self-stake read");
    assert_eq!(got, staked, "the real balanceOf value, not a sim");

    // The eth_call targeted the POOL with the balanceOf calldata for the staker.
    let body = &rpc.transport.requests()[0];
    let params = &body["params"][0];
    assert_eq!(
        params["to"].as_str().unwrap().to_ascii_lowercase(),
        LIQUID_STAKING_POOL,
        "read targets the LiquidStakingPool"
    );
    let data = params["data"].as_str().unwrap();
    assert!(data.starts_with("0x70a08231"), "balanceOf selector: {data}");
    assert!(
        data.to_ascii_lowercase().contains(&STAKER_ADDR[2..]),
        "staker address in the calldata: {data}"
    );
}

#[test]
fn read_self_stake_rejects_a_malformed_address_before_any_rpc() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert!(read_self_stake(&rpc, "0xnope").is_err());
    assert!(
        rpc.transport.requests().is_empty(),
        "fail closed BEFORE calling the node with a bad address"
    );
}

// ===========================================================================
// The deposit intent — decodes to the exact stake tx (@rule8)
// ===========================================================================

#[test]
fn stake_json_decodes_to_the_expected_deposit_call() {
    let value: u128 = 3_000_000_000_000_000_000; // 3 SALT staked
    let json = encode_stake_json(STAKER_ADDR, value);
    let (parsed, display) =
        crate::txdecode::decode_transaction(&json).expect("deposit json must decode");

    // to = the pool (not a fabricated address).
    let pool_bytes = hex::decode(&LIQUID_STAKING_POOL[2..]).unwrap();
    let mut pool = [0u8; 20];
    pool.copy_from_slice(&pool_bytes);
    assert_eq!(parsed.to, Some(pool), "to = LiquidStakingPool");

    assert_eq!(parsed.value, value, "staked value round-trips exactly (no truncation)");
    assert_eq!(parsed.data, deposit_selector().to_vec(), "calldata = deposit() selector");
    assert_eq!(parsed.gas_limit, Some(200_000), "explicit deposit gas");
    assert_eq!(
        parsed.from.as_deref(),
        Some(STAKER_ADDR),
        "from = the staking vault wallet"
    );
    // The human-facing decode shows a calldata call carrying the staked value.
    assert!(
        display.action.to_ascii_lowercase().contains("calldata"),
        "decoded action names the calldata call: {}",
        display.action
    );
}

#[test]
fn stake_json_handles_a_large_value_beyond_u64() {
    // 32,000 SALT ~ 3.2e22 wei — beyond u64; must survive the 0x-hex round-trip.
    let value: u128 = 32_000u128 * 1_000_000_000_000_000_000u128;
    let json = encode_stake_json(STAKER_ADDR, value);
    let (parsed, _display) =
        crate::txdecode::decode_transaction(&json).expect("large-value deposit json must decode");
    assert_eq!(parsed.value, value, "u128 stake value beyond u64 round-trips exactly");
}

// ===========================================================================
// WP2 — withdraw selector pins (Rule 11 drift tripwires)
// ===========================================================================
//
// Each PINNED withdraw selector MUST be the independent keccak of its signature.
// NOTE: these prove the KECCAK-correct values, which DIVERGE from the selector
// table in WITHDRAW_WP2_SCOPE.md for four of five entries (the scope table was
// wrong — see the const doc-comments + the evidence note). The drift test is the
// ground truth (@rule8: a wrong selector = a wrong/reverting money tx).

#[test]
fn request_withdrawal_selector_is_keccak_of_signature() {
    assert_eq!(
        request_withdrawal_selector(),
        derive_selector("requestWithdrawal(uint256)")
    );
    assert_eq!(
        format!("0x{}", hex::encode(request_withdrawal_selector())),
        "0x9ee679e8"
    );
}

#[test]
fn claim_withdrawal_selector_is_keccak_of_signature() {
    assert_eq!(
        claim_withdrawal_selector(),
        derive_selector("claimWithdrawal(uint256)")
    );
    assert_eq!(
        format!("0x{}", hex::encode(claim_withdrawal_selector())),
        "0xf8444436"
    );
}

#[test]
fn withdrawals_selector_is_keccak_of_signature() {
    assert_eq!(
        withdrawals_selector(),
        derive_selector("withdrawals(uint256)")
    );
    assert_eq!(
        format!("0x{}", hex::encode(withdrawals_selector())),
        "0x5cc07076"
    );
}

#[test]
fn shares_selector_is_keccak_of_signature() {
    assert_eq!(shares_selector(), derive_selector("shares(address)"));
    assert_eq!(format!("0x{}", hex::encode(shares_selector())), "0xce7c2ac2");
}

#[test]
fn get_share_price_selector_is_keccak_of_signature() {
    assert_eq!(
        get_share_price_selector(),
        derive_selector("getSharePrice()")
    );
    assert_eq!(
        format!("0x{}", hex::encode(get_share_price_selector())),
        "0x5b1dac60"
    );
}

/// The `WithdrawalRequested(...)` topic0 the pending-list filter uses MUST be the
/// full keccak of the canonical signature (Rule 11) — a drift would silently miss
/// every withdrawal (an empty list dressed as "no pending").
#[test]
fn withdrawal_requested_topic0_is_keccak_of_signature() {
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(WITHDRAWAL_REQUESTED_SIG.as_bytes());
    assert_eq!(
        WITHDRAWAL_REQUESTED_TOPIC0,
        format!("0x{}", hex::encode(h)),
        "topic0 must be the real keccak of the event signature"
    );
}

// ===========================================================================
// WP2 — SALT→shares conversion (live-read grounded, no fabricated numbers)
// ===========================================================================

#[test]
fn salt_to_shares_withdraw_all_burns_exact_shares_no_dust() {
    // Ask for the whole self-stake (or more) → burn ALL shares exactly.
    let self_shares: u128 = 4_000_000_000_000_000_000; // 4e18 shares
    let self_stake_salt: u128 = 5_000_000_000_000_000_000; // 5 SALT
    assert_eq!(
        salt_to_share_amount(self_stake_salt, self_stake_salt, self_shares).unwrap(),
        self_shares,
        "withdraw-all burns every share (no dust, no share-price drift)"
    );
    // Over-ask (amount > self-stake) also burns exactly all shares (clamped).
    assert_eq!(
        salt_to_share_amount(self_stake_salt * 10, self_stake_salt, self_shares).unwrap(),
        self_shares
    );
}

#[test]
fn salt_to_shares_partial_is_proportional_floor_and_bounded() {
    let self_shares: u128 = 4_000_000_000_000_000_000; // 4e18
    let self_stake_salt: u128 = 5_000_000_000_000_000_000; // 5 SALT
    // Withdraw half the SALT → half the shares (2e18), exact here.
    let half = self_stake_salt / 2;
    let got = salt_to_share_amount(half, self_stake_salt, self_shares).unwrap();
    assert_eq!(got, 2_000_000_000_000_000_000, "proportional: half SALT → half shares");
    assert!(got < self_shares, "a partial withdraw is always < total shares");
}

#[test]
fn salt_to_shares_handles_values_beyond_u64_without_overflow() {
    // 40,000 SALT self-stake, 40,000e18 shares — the amount*shares product is
    // ~1.6e45, far beyond u128; the 256-bit mul_div must not overflow.
    let self_stake_salt: u128 = 40_000u128 * 1_000_000_000_000_000_000u128;
    let self_shares: u128 = self_stake_salt; // 1:1 for a clean assertion
    let amount: u128 = 12_345u128 * 1_000_000_000_000_000_000u128;
    let got = salt_to_share_amount(amount, self_stake_salt, self_shares).unwrap();
    assert_eq!(got, amount, "1:1 share price → shares == amount, exact, no overflow");
}

#[test]
fn salt_to_shares_rejects_empty_position_and_zero_amount() {
    assert!(salt_to_share_amount(0, 5, 5).is_err(), "zero amount");
    assert!(salt_to_share_amount(1, 0, 0).is_err(), "no self-stake (0 SALT / 0 shares)");
    // A dust amount that floors to zero shares → err (never a 0-share tx).
    // 1 wei against a huge stake/shares ratio flooring below one share.
    let self_stake_salt: u128 = 1_000_000_000_000_000_000_000; // 1000 SALT
    let self_shares: u128 = 1; // absurd 1-share pool → 1 wei floors to 0 shares
    assert!(salt_to_share_amount(1, self_stake_salt, self_shares).is_err(), "floors to 0 shares");
}

// ===========================================================================
// WP2 — the request/claim intents decode to the exact tx (@rule8)
// ===========================================================================

#[test]
fn request_withdrawal_json_decodes_to_the_expected_call() {
    let share_amount: u128 = 2_000_000_000_000_000_000; // 2e18 shares
    let json = encode_request_withdrawal_json(STAKER_ADDR, share_amount);
    let (parsed, display) =
        crate::txdecode::decode_transaction(&json).expect("requestWithdrawal json must decode");

    let pool_bytes = hex::decode(&LIQUID_STAKING_POOL[2..]).unwrap();
    let mut pool = [0u8; 20];
    pool.copy_from_slice(&pool_bytes);
    assert_eq!(parsed.to, Some(pool), "to = LiquidStakingPool");
    assert_eq!(parsed.value, 0, "requestWithdrawal moves no SALT via msg.value");
    assert_eq!(parsed.gas_limit, Some(150_000), "explicit requestWithdrawal gas");

    // Calldata = selector ++ 32-byte shareAmount.
    assert_eq!(&parsed.data[0..4], &request_withdrawal_selector(), "selector prefix");
    assert_eq!(parsed.data.len(), 36, "selector + one uint256 word");
    let mut arg = [0u8; 16];
    arg.copy_from_slice(&parsed.data[20..36]);
    assert_eq!(u128::from_be_bytes(arg), share_amount, "shareAmount arg round-trips");
    assert!(
        display.action.to_ascii_lowercase().contains("calldata"),
        "decoded as a calldata call: {}",
        display.action
    );
}

#[test]
fn claim_withdrawal_json_decodes_to_the_expected_call() {
    let request_id: u128 = 7;
    let json = encode_claim_withdrawal_json(STAKER_ADDR, request_id);
    let (parsed, _display) =
        crate::txdecode::decode_transaction(&json).expect("claimWithdrawal json must decode");

    let pool_bytes = hex::decode(&LIQUID_STAKING_POOL[2..]).unwrap();
    let mut pool = [0u8; 20];
    pool.copy_from_slice(&pool_bytes);
    assert_eq!(parsed.to, Some(pool), "to = LiquidStakingPool");
    assert_eq!(parsed.value, 0, "claimWithdrawal sends no SALT (the pool pays out)");
    assert_eq!(parsed.gas_limit, Some(90_000), "explicit claimWithdrawal gas");
    assert_eq!(&parsed.data[0..4], &claim_withdrawal_selector(), "selector prefix");
    let mut arg = [0u8; 16];
    arg.copy_from_slice(&parsed.data[20..36]);
    assert_eq!(u128::from_be_bytes(arg), request_id, "requestId arg round-trips");
}

// ===========================================================================
// WP2 — the 5-word withdrawals(id) tuple decode
// ===========================================================================

/// Build a 5-word ABI tuple return for `withdrawals(id)`:
/// (staker, shareAmount, saltAmount, requestBlock, claimed).
fn withdrawal_tuple_hex(
    staker: &str,
    share_amount: u128,
    salt_amount: u128,
    request_block: u64,
    claimed: bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    // word 0: staker (left-padded 20-byte addr)
    let mut w = [0u8; 32];
    let sb = hex::decode(staker.trim_start_matches("0x")).unwrap();
    w[12..32].copy_from_slice(&sb);
    out.extend_from_slice(&w);
    // word 1: shareAmount
    let mut w = [0u8; 32];
    w[16..32].copy_from_slice(&share_amount.to_be_bytes());
    out.extend_from_slice(&w);
    // word 2: saltAmount
    let mut w = [0u8; 32];
    w[16..32].copy_from_slice(&salt_amount.to_be_bytes());
    out.extend_from_slice(&w);
    // word 3: requestBlock
    let mut w = [0u8; 32];
    w[24..32].copy_from_slice(&request_block.to_be_bytes());
    out.extend_from_slice(&w);
    // word 4: claimed bool
    let mut w = [0u8; 32];
    w[31] = if claimed { 1 } else { 0 };
    out.extend_from_slice(&w);
    out
}

#[test]
fn decode_withdrawal_tuple_extracts_the_five_fields() {
    let ret = withdrawal_tuple_hex(STAKER_ADDR, 3, 42_000_000_000_000_000_000, 1000, false);
    let (staker, salt, block, claimed) = decode_withdrawal_tuple(&ret).unwrap();
    assert!(staker.eq_ignore_ascii_case(STAKER_ADDR), "staker addr decoded");
    assert_eq!(salt, 42_000_000_000_000_000_000, "saltAmount decoded (wei)");
    assert_eq!(block, 1000, "requestBlock decoded");
    assert!(!claimed, "claimed=false decoded");

    let ret_claimed = withdrawal_tuple_hex(STAKER_ADDR, 3, 1, 5, true);
    let (_s, _a, _b, claimed2) = decode_withdrawal_tuple(&ret_claimed).unwrap();
    assert!(claimed2, "claimed=true decoded");

    assert!(decode_withdrawal_tuple(&[0u8; 100]).is_err(), "short tuple rejected");
}

// ===========================================================================
// WP2 — read_pending_withdrawals over a mock RPC (Rule 1: TEST transport)
// ===========================================================================

/// A single WithdrawalRequested log for `id`, staker = STAKER_ADDR, at `block`.
fn withdrawal_log(id: u128, block: u64) -> JsonValue {
    let mut id_word = [0u8; 32];
    id_word[16..32].copy_from_slice(&id.to_be_bytes());
    let mut staker_word = [0u8; 32];
    staker_word[12..32].copy_from_slice(&hex::decode(&STAKER_ADDR[2..]).unwrap());
    serde_json::json!({
        "address": LIQUID_STAKING_POOL,
        "topics": [
            WITHDRAWAL_REQUESTED_TOPIC0,
            format!("0x{}", hex::encode(id_word)),
            format!("0x{}", hex::encode(staker_word)),
        ],
        "data": "0x",
        "blockNumber": format!("0x{block:x}"),
    })
}

#[test]
fn read_pending_withdrawals_lists_unclaimed_and_gates_claimable() {
    // Two logs: id 1 (matured), id 2 (not yet), plus id 3 which is CLAIMED (excluded).
    let logs = JsonValue::Array(vec![
        withdrawal_log(1, 100),
        withdrawal_log(2, 100_000),
        withdrawal_log(3, 100),
    ]);
    // Responses in call order: getLogs, block_number, then withdrawals(1),(2),(3).
    let current_block: u64 = 100 + WITHDRAWAL_DELAY + 10; // past id-1's gate, before id-2's
    let responses = vec![
        ok(logs),
        ok(JsonValue::String(format!("0x{current_block:x}"))),
        // withdrawals(1): matured, unclaimed, 10 SALT.
        ok(JsonValue::String(format!(
            "0x{}",
            hex::encode(withdrawal_tuple_hex(STAKER_ADDR, 5, 10_000_000_000_000_000_000, 100, false))
        ))),
        // withdrawals(2): far-future request block, unclaimed, 3 SALT.
        ok(JsonValue::String(format!(
            "0x{}",
            hex::encode(withdrawal_tuple_hex(STAKER_ADDR, 2, 3_000_000_000_000_000_000, 100_000, false))
        ))),
        // withdrawals(3): CLAIMED → excluded.
        ok(JsonValue::String(format!(
            "0x{}",
            hex::encode(withdrawal_tuple_hex(STAKER_ADDR, 1, 1_000_000_000_000_000_000, 100, true))
        ))),
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let pending = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("pending");

    assert_eq!(pending.len(), 2, "the two UNCLAIMED withdrawals (claimed one excluded)");
    let p1 = pending.iter().find(|p| p.id == "1").expect("id 1 present");
    assert_eq!(p1.salt_wei, "10000000000000000000", "id-1 saltAmount (wei)");
    assert_eq!(p1.request_block, 100);
    assert_eq!(p1.claimable_at_block, 100 + WITHDRAWAL_DELAY);
    assert!(p1.claimable, "id-1 matured → claimable now");

    let p2 = pending.iter().find(|p| p.id == "2").expect("id 2 present");
    assert!(!p2.claimable, "id-2 not yet matured → not claimable");

    // The getLogs filter targeted the pool + WithdrawalRequested topic0 + staker.
    let body = &rpc.transport.requests()[0];
    assert_eq!(body["method"], "eth_getLogs");
    let filter = &body["params"][0];
    assert_eq!(filter["address"], LIQUID_STAKING_POOL);
    assert_eq!(filter["topics"][0], WITHDRAWAL_REQUESTED_TOPIC0);
    assert!(
        filter["topics"][2].as_str().unwrap().to_ascii_lowercase().contains(&STAKER_ADDR[2..]),
        "topic2 = the staker (indexed address)"
    );
}

#[test]
fn read_pending_withdrawals_empty_for_a_fresh_wallet() {
    // No logs → an honest empty list (getLogs then block_number are still called).
    let responses = vec![
        ok(JsonValue::Array(vec![])),
        ok(JsonValue::String("0x64".into())),
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let pending = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("empty pending");
    assert!(pending.is_empty(), "fresh wallet → empty, never a fabricated entry");
}

#[test]
fn read_pending_withdrawals_rejects_bad_address_before_any_rpc() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert!(read_pending_withdrawals(&rpc, "0xnope").is_err());
    assert!(
        rpc.transport.requests().is_empty(),
        "fail closed BEFORE any node call on a bad address"
    );
}

#[test]
fn read_self_shares_reads_pool_shares_over_mock_rpc() {
    let shares: u128 = 4_000_000_000_000_000_000;
    let rpc = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(uint256_hex(shares)))]));
    let got = read_self_shares(&rpc, STAKER_ADDR).expect("shares read");
    assert_eq!(got, shares, "the real shares() value, not a sim");
    let body = &rpc.transport.requests()[0];
    let data = body["params"][0]["data"].as_str().unwrap();
    assert!(data.starts_with("0xce7c2ac2"), "shares(address) selector: {data}");
}
