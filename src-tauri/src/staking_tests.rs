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
/// would send staked SALT to the wrong contract (@rule8). Re-earned 2026-10-04: the
/// module literal had drifted from the book (`0xfd27…685e`, while the canonical book and
/// the live chain carry `LiquidStakingPool` elsewhere), so the pin is now the generated
/// book itself, which `scripts/sync-addresses.py --rpc` verifies has code on 40204 and
/// `generated_book_is_not_hand_edited` keeps machine-written.
#[test]
fn pool_address_is_the_canonical_40204_value() {
    let a = liquid_staking_pool();
    assert_eq!(a, crate::addresses::liquid_staking_pool());
    assert!(a.len() == 42 && a.starts_with("0x"), "{a}");
    assert_ne!(a, "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e", "the drifted literal");
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
    let body = &rpc.transport().requests()[0];
    let params = &body["params"][0];
    assert_eq!(
        params["to"].as_str().unwrap().to_ascii_lowercase(),
        liquid_staking_pool(),
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
        rpc.transport().requests().is_empty(),
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
    let pool_bytes = hex::decode(&liquid_staking_pool()[2..]).unwrap();
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

    let pool_bytes = hex::decode(&liquid_staking_pool()[2..]).unwrap();
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

    let pool_bytes = hex::decode(&liquid_staking_pool()[2..]).unwrap();
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
        "address": liquid_staking_pool(),
        "topics": [
            WITHDRAWAL_REQUESTED_TOPIC0,
            format!("0x{}", hex::encode(id_word)),
            format!("0x{}", hex::encode(staker_word)),
        ],
        "data": "0x",
        "blockNumber": format!("0x{block:x}"),
    })
}

// ---------------------------------------------------------------------------
// C1-W — a chain stub for the paged scan. It serves eth_blockNumber, eth_getLogs over the
// requested range, and withdrawals(id). `clamp_to` mimics today's node, which silently cuts a range
// to its first 1000 blocks; `max_range` mimics citrate-chain#334, which answers -32005 when
// toBlock - fromBlock > max_range. `fail_from` makes any window touching that block fail.
// ---------------------------------------------------------------------------
struct ChainStub {
    head: u64,
    /// (id, request block, claimed)
    withdrawals: Vec<(u128, u64, bool)>,
    clamp_to: Option<u64>,
    max_range: Option<u64>,
    fail_from: RefCell<Option<u64>>,
    /// How many eth_getLogs calls answer -32002 ("retry": the node's cache is filling) first.
    retry_first: RefCell<u32>,
    /// citrate-chain#334: eth_blockNumber is the STORED height, but eth_getLogs omits blocks above
    /// the APPLIED tip without an error. `applied_tip` = that tip (None = all applied).
    applied_tip: RefCell<Option<u64>>,
    /// What eth_syncing answers: `false`, `{currentBlock}` (the applied tip), or None = an error.
    syncing: RefCell<Option<JsonValue>>,
    /// `citrate_executedHead` (DGX, stacked on citrate-chain#334): Some(number) = served as
    /// {number, hash}; None = an older node without the method (-32601).
    executed_head: RefCell<Option<u64>>,
    /// citrate-chain#337: `citrate_executedHead` answers this JSON-RPC error code instead (e.g.
    /// -32603 when the node has no persisted applied tip).
    executed_head_error: RefCell<Option<i64>>,
    /// Answers for the next `citrate_executedHead` calls, in order (Ok(number) or Err(code)), before
    /// the fields above apply: h1 and h2 of one read.
    executed_next: RefCell<std::collections::VecDeque<Result<u64, i64>>>,
    /// A raw `citrate_executedHead` response body (e.g. a malformed result), used when set.
    executed_head_raw: RefCell<Option<JsonValue>>,
    /// `citrate_executedHead` fails at the transport (a timeout) when set.
    executed_head_timeout: RefCell<bool>,
    /// Block hashes that differ from the default `stub_hash(n)` (a reorg). The replaced block is
    /// ABANDONED: like the chain, it is still stored, but `citrate_isExecuted` says false.
    reorged: RefCell<std::collections::HashMap<u64, String>>,
    /// `citrate_isExecuted(hash)` (chain#337): None = served from the stub's chain; Some(Err(code))
    /// = that JSON-RPC error (-32601 absent, -32603 no tip, -32602 malformed hash); Some(Ok(raw)) =
    /// that raw result (e.g. malformed).
    is_executed: RefCell<Option<Result<JsonValue, i64>>>,
    /// `citrate_isExecuted` times out when set.
    is_executed_timeout: RefCell<bool>,
    /// Ids whose claim was reorged out: `withdrawals(id)` reads `claimed = false`.
    unclaim: RefCell<std::collections::HashSet<u128>>,
    /// Latency of each eth_call batch and of each eth_getLogs call.
    batch_delay: std::cell::Cell<std::time::Duration>,
    logs_delay: std::cell::Cell<std::time::Duration>,
    /// Hashes for the next `citrate_executedHead` answers (h1 and h2 at one height, two hashes).
    executed_next_hashes: RefCell<std::collections::VecDeque<String>>,
    requests: RefCell<Vec<JsonValue>>,
}
fn stub_hash(n: u64) -> String {
    format!("0x{n:064x}")
}
impl ChainStub {
    fn new(head: u64, withdrawals: Vec<(u128, u64, bool)>) -> Self {
        ChainStub {
            head,
            withdrawals,
            clamp_to: None,
            max_range: None,
            fail_from: RefCell::new(None),
            retry_first: RefCell::new(0),
            applied_tip: RefCell::new(None),
            syncing: RefCell::new(Some(JsonValue::Bool(false))),
            executed_head: RefCell::new(None),
            executed_head_error: RefCell::new(None),
            executed_next: RefCell::new(Default::default()),
            executed_head_raw: RefCell::new(None),
            executed_head_timeout: RefCell::new(false),
            reorged: RefCell::new(Default::default()),
            is_executed: RefCell::new(None),
            is_executed_timeout: RefCell::new(false),
            unclaim: RefCell::new(Default::default()),
            batch_delay: std::cell::Cell::new(std::time::Duration::ZERO),
            logs_delay: std::cell::Cell::new(std::time::Duration::ZERO),
            executed_next_hashes: RefCell::new(Default::default()),
            requests: RefCell::new(Vec::new()),
        }
    }
    fn log_ranges(&self) -> Vec<(u64, u64)> {
        self.requests
            .borrow()
            .iter()
            .filter(|r| r["method"] == "eth_getLogs")
            .map(|r| (hex_q(&r["params"][0]["fromBlock"]), hex_q(&r["params"][0]["toBlock"])))
            .collect()
    }
}
fn hex_q(v: &JsonValue) -> u64 {
    u64::from_str_radix(v.as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
}
impl RpcTransport for ChainStub {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.requests.borrow_mut().push(body.clone());
        // A JSON-RPC batch: answer each request, keeping its id.
        if let Some(batch) = body.as_array() {
            std::thread::sleep(self.batch_delay.get());
            let mut out = Vec::new();
            for b in batch {
                let mut r = self.one(b.clone())?;
                r["id"] = b["id"].clone();
                out.push(r);
            }
            return Ok(JsonValue::Array(out));
        }
        self.one(body)
    }
}
impl ChainStub {
    fn hash_at(&self, n: u64) -> String {
        self.reorged.borrow().get(&n).cloned().unwrap_or_else(|| stub_hash(n))
    }
    fn batches(&self) -> usize {
        self.requests.borrow().iter().filter(|r| r.is_array()).count()
    }
    /// The withdrawal ids read with `withdrawals(id)` in requests[from..].
    fn asked_ids(&self, from: usize) -> std::collections::BTreeSet<u128> {
        self.requests.borrow()[from..]
            .iter()
            .filter_map(|r| r.as_array().cloned())
            .flatten()
            .filter_map(|c| c["params"][0]["data"].as_str().map(|d| u128::from_str_radix(&d[d.len() - 32..], 16).unwrap_or(0)))
            .collect()
    }
    fn tip(&self) -> u64 {
        self.executed_head.borrow().unwrap_or(self.head)
    }
    fn one(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        match body["method"].as_str() {
            Some("citrate_executedHead") if *self.executed_head_timeout.borrow() => {
                Err(RpcError::Transport("rpc.citrate.ai timed out".into()))
            }
            Some("citrate_executedHead") if self.executed_head_raw.borrow().is_some() => {
                Ok(self.executed_head_raw.borrow().clone().unwrap_or_default())
            }
            Some("citrate_isExecuted") if *self.is_executed_timeout.borrow() => {
                Err(RpcError::Transport("rpc.citrate.ai timed out".into()))
            }
            Some("citrate_isExecuted") => match self.is_executed.borrow().clone() {
                Some(Err(code)) => Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1,
                    "error": { "code": code, "message": "citrate_isExecuted: refused" } })),
                Some(Ok(raw)) => Ok(ok(raw)),
                None => {
                    let h = body["params"][0].as_str().unwrap_or_default().to_ascii_lowercase();
                    let n = u64::from_str_radix(h.trim_start_matches("0x"), 16).ok();
                    let on_chain = self.reorged.borrow().iter().find(|(_, v)| v.eq_ignore_ascii_case(&h)).map(|(n, _)| *n);
                    let executed = match (on_chain, n) {
                        (Some(m), _) => Some(m),
                        (None, Some(m)) if !self.reorged.borrow().contains_key(&m) => Some(m),
                        _ => None,
                    }
                    .filter(|m| *m <= self.tip());
                    Ok(ok(match executed {
                        Some(m) => serde_json::json!({ "executed": true, "number": format!("0x{m:x}") }),
                        None => serde_json::json!({ "executed": false, "number": null }),
                    }))
                }
            },
            Some("citrate_executedHead") if !self.executed_next.borrow().is_empty() => {
                let forced = self.executed_next_hashes.borrow_mut().pop_front();
                match self.executed_next.borrow_mut().pop_front() {
                    Some(Ok(n)) => Ok(ok(serde_json::json!({ "number": format!("0x{n:x}"), "hash": forced.unwrap_or_else(|| self.hash_at(n)) }))),
                    Some(Err(code)) => Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1,
                        "error": { "code": code, "message": "citrate_executedHead: read failed" } })),
                    None => Err(RpcError::Transport("unreachable".into())),
                }
            }
            Some("citrate_executedHead") if self.executed_head_error.borrow().is_some() => {
                let code = self.executed_head_error.borrow().unwrap_or(-32603);
                Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1,
                    "error": { "code": code, "message": "citrate_executedHead: no applied tip" } }))
            }
            Some("citrate_executedHead") => match *self.executed_head.borrow() {
                Some(n) => Ok(ok(serde_json::json!({ "number": format!("0x{n:x}"), "hash": self.hash_at(n) }))),
                None => Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1,
                    "error": { "code": -32601, "message": "Method not found" } })),
            },
            Some("eth_syncing") => match self.syncing.borrow().clone() {
                Some(v) => Ok(ok(v)),
                None => Err(RpcError::Transport("eth_syncing failed".into())),
            },
            Some("eth_blockNumber") => Ok(ok(JsonValue::String(format!("0x{:x}", self.head)))),
            Some("eth_getLogs") => {
                std::thread::sleep(self.logs_delay.get());
                let f = &body["params"][0];
                let from = hex_q(&f["fromBlock"]);
                let mut to = hex_q(&f["toBlock"]);
                if let Some(max) = self.max_range {
                    if to - from > max {
                        return Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "error": {
                            "code": -32005, "message": "block range too large",
                            "data": { "maxBlockRange": max } } }));
                    }
                }
                if *self.retry_first.borrow() > 0 {
                    *self.retry_first.borrow_mut() -= 1;
                    return Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "error": {
                        "code": -32002, "message": "log index warming, retry",
                        "data": { "retryable": true } } }));
                }
                if let Some(at) = *self.fail_from.borrow() {
                    if from <= at && at <= to {
                        return Err(RpcError::Transport("rpc.citrate.ai timed out".into()));
                    }
                }
                if let Some(c) = self.clamp_to {
                    to = to.min(from + c);
                }
                if let Some(t) = *self.applied_tip.borrow() {
                    to = to.min(t);
                }
                let logs: Vec<JsonValue> = self
                    .withdrawals
                    .iter()
                    .filter(|(_, b, _)| from <= *b && *b <= to)
                    .map(|(id, b, _)| withdrawal_log(*id, *b))
                    .collect();
                Ok(ok(JsonValue::Array(logs)))
            }
            Some("eth_call") => {
                let data = body["params"][0]["data"].as_str().unwrap();
                let id = u128::from_str_radix(&data[data.len() - 32..], 16).unwrap();
                let (_, b, claimed) = self
                    .withdrawals
                    .iter()
                    .find(|(i, _, _)| *i == id)
                    .copied()
                    .unwrap();
                let claimed = claimed && !self.unclaim.borrow().contains(&id);
                Ok(ok(JsonValue::String(format!(
                    "0x{}",
                    hex::encode(withdrawal_tuple_hex(STAKER_ADDR, 1, 2_000_000_000_000_000_000, b, claimed))
                ))))
            }
            _ => Err(RpcError::Transport("unexpected method".into())),
        }
    }
}

#[test]
fn read_pending_withdrawals_lists_unclaimed_and_gates_claimable() {
    // id 1 matured, id 2 not yet, id 3 CLAIMED (excluded). Head past id-1's gate only.
    let head: u64 = 500 + WITHDRAWAL_DELAY + 10;
    let rpc = RpcClient::with_transport(ChainStub::new(
        head,
        vec![(1, 500, false), (2, head - 5, false), (3, 600, true)],
    ));
    let pending = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("pending");
    assert_eq!(pending.len(), 2, "the two UNCLAIMED withdrawals (claimed one excluded)");
    let p1 = pending.iter().find(|p| p.id == "1").expect("id 1 present");
    assert_eq!(p1.salt_wei, "2000000000000000000");
    assert_eq!(p1.request_block, 500);
    assert_eq!(p1.claimable_at_block, 500 + WITHDRAWAL_DELAY);
    assert!(p1.claimable, "id-1 matured -> claimable now");
    let p2 = pending.iter().find(|p| p.id == "2").expect("id 2 present");
    assert!(!p2.claimable, "id-2 not yet matured -> not claimable");

    // Every getLogs filter targets the pool + WithdrawalRequested topic0 + the staker.
    let reqs = rpc.transport().requests.borrow().clone();
    let body = reqs.iter().find(|r| r["method"] == "eth_getLogs").expect("a getLogs call");
    let filter = &body["params"][0];
    assert_eq!(filter["address"], liquid_staking_pool());
    assert_eq!(filter["topics"][0], WITHDRAWAL_REQUESTED_TOPIC0);
    assert!(
        filter["topics"][2].as_str().unwrap().to_ascii_lowercase().contains(&STAKER_ADDR[2..]),
        "topic2 = the staker (indexed address)"
    );
}

#[test]
fn read_pending_withdrawals_empty_for_a_fresh_wallet() {
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![]));
    let pending = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("empty pending");
    assert!(pending.is_empty(), "fresh wallet -> empty, never a fabricated entry");
}

// C1-W: today's node clamps any range to its first 1000 blocks without saying so. A scan of
// earliest..latest only ever saw blocks 0-1000. Paged in <=1000-block windows from the pool's
// deployment block, a withdrawal far past block 1000 is found.
#[test]
fn a_withdrawal_after_block_1000_is_found_on_a_node_that_clamps_ranges() {
    let mut stub = ChainStub::new(120_000, vec![(7, 5_000, false), (8, 119_999, false), (9, 900, false)]);
    stub.clamp_to = Some(1000);
    let rpc = RpcClient::with_transport(stub);
    let pending = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("pending");
    let mut ids: Vec<&str> = pending.iter().map(|p| p.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["7", "8", "9"]);
    let ranges = rpc.transport().log_ranges();
    assert_eq!(ranges.first().map(|r| r.0), Some(LIQUID_STAKING_POOL_DEPLOY_BLOCK));
    assert_eq!(ranges.last().map(|r| r.1), Some(120_000));
    for (a, b) in &ranges {
        assert!(b >= a && b - a < LOG_WINDOW_BLOCKS, "window {a}..={b} spans <= {LOG_WINDOW_BLOCKS} blocks");
    }
    // Contiguous: no block is skipped between windows.
    for w in ranges.windows(2) {
        assert_eq!(w[1].0, w[0].1 + 1, "windows are contiguous");
    }
}

// citrate-chain#334: a range over the limit answers -32005. The scan halves its window and goes on.
#[test]
fn a_limit_exceeded_answer_halves_the_window() {
    let mut stub = ChainStub::new(10_000, vec![(4, 3_333, false), (5, 9_999, false)]);
    stub.max_range = Some(300);
    let rpc = RpcClient::with_transport(stub);
    let pending = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("pending");
    assert_eq!(pending.len(), 2);
    let ranges = rpc.transport().log_ranges();
    let served: Vec<_> = ranges.iter().filter(|(a, b)| b - a <= 300).collect();
    assert!(!served.is_empty());
    assert_eq!(served.last().map(|r| r.1), Some(10_000));
    for w in served.windows(2) {
        assert_eq!(w[1].0, w[0].1 + 1, "served windows are contiguous");
    }
}

// The same withdrawal is never listed twice (a re-scanned tail, or overlapping windows).
#[test]
fn results_are_deduplicated_across_windows_and_refreshes() {
    let rpc = RpcClient::with_transport(ChainStub::new(3_000, vec![(11, 2_990, false)]));
    let mut scan = WithdrawalScan::default();
    let first = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("first");
    let second = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("second");
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1, "no duplicate after the tail re-scan");
}

// The committed position is kept, so a refresh only reads the new blocks plus the trailing margin.
#[test]
fn a_refresh_is_incremental() {
    let rpc = RpcClient::with_transport(ChainStub::new(50_000, vec![(1, 10_000, false)]));
    *rpc.transport().executed_head.borrow_mut() = Some(50_000);
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("first");
    // The committed position stays the reorg margin below the executed head (chain#337 protocol).
    assert_eq!(scan.next_block, 50_001 - EXECUTED_HEAD_MARGIN_BLOCKS);
    let before = rpc.transport().log_ranges().len();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("refresh");
    let after = rpc.transport().log_ranges();
    assert_eq!(after.len() - before, 1, "one window: the trailing margin");
    let (a, b) = after[after.len() - 1];
    assert_eq!(b, 50_000);
    assert_eq!(a, 50_001 - EXECUTED_HEAD_MARGIN_BLOCKS, "only the trailing margin is re-read");
}

// A failed window is an error, never an empty (or partial) list; the position does not move.
#[test]
fn a_failed_window_is_an_error_not_an_empty_list() {
    let rpc = RpcClient::with_transport(ChainStub::new(20_000, vec![(3, 2_000, false)]));
    *rpc.transport().fail_from.borrow_mut() = Some(8_500);
    let mut scan = WithdrawalScan::default();
    let err = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect_err("must fail");
    let msg = err.to_string();
    assert!(msg.contains("8") && msg.contains("timed out"), "{msg}");
    // #337 protocol: an error commits nothing; the ids seen are kept (each is confirmed on chain).
    assert_eq!(scan.next_block, 0, "an error never advances the position");
    assert!(scan.ids.contains(&3), "the withdrawal found before the failure is kept");
    *rpc.transport().fail_from.borrow_mut() = None;
    let pending = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("resumed");
    assert_eq!(pending.len(), 1);
}

// The first scan of a long chain is spread over refreshes by a time budget: an unfinished scan is
// "still reading", never a list.
#[test]
fn an_unfinished_scan_is_reported_as_still_reading() {
    let rpc = RpcClient::with_transport(ChainStub::new(30_000, vec![(2, 25_000, false)]));
    *rpc.transport().executed_head.borrow_mut() = Some(30_000);
    let mut scan = WithdrawalScan::default();
    let past = std::time::Instant::now();
    let err = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(past)).expect_err("budget spent");
    assert!(matches!(err, StakingError::OutOfBudget), "{err:?}");
    assert!(err.to_string().contains("still reading chain history"), "{err}");
    let mut done = None;
    for _ in 0..100 {
        let deadline = std::time::Instant::now() + H2_RESERVE + std::time::Duration::from_millis(50);
        let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(deadline)).expect("read");
        if scan.last_read_complete {
            done = Some(rows);
            break;
        }
    }
    assert_eq!(done.expect("finishes").len(), 1);
}

// citrate-chain#334 review: a deep historical range can answer -32002 ("retry") while the node's
// cache fills. The same window is retried after a bounded back-off, and the scan goes on.
#[test]
fn a_retry_answer_retries_the_same_window_and_then_reads_it() {
    let rpc = RpcClient::with_transport(ChainStub::new(2_000, vec![(6, 800, false)]));
    *rpc.transport().retry_first.borrow_mut() = 2;
    let pending = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("pending after retries");
    assert_eq!(pending.len(), 1);
    let ranges = rpc.transport().log_ranges();
    // Three calls for the first window (two -32002, then data), all the SAME window (not halved).
    assert_eq!(ranges[0], ranges[1]);
    assert_eq!(ranges[1], ranges[2]);
    assert_eq!(ranges[0], (LIQUID_STAKING_POOL_DEPLOY_BLOCK, LIQUID_STAKING_POOL_DEPLOY_BLOCK + 999));
}

// A -32002 that does not say `retryable: true` is a window error after ONE call.
#[test]
fn a_retry_code_without_the_retryable_flag_is_not_retried() {
    struct Busy(RefCell<u32>);
    impl RpcTransport for Busy {
        fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
            match body["method"].as_str() {
                Some("eth_blockNumber") => Ok(ok(JsonValue::String("0x7d0".into()))),
                Some("eth_syncing") => Ok(ok(JsonValue::Bool(false))),
                Some("citrate_executedHead") => Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1,
                    "error": { "code": -32601, "message": "Method not found" } })),
                _ => {
                    *self.0.borrow_mut() += 1;
                    Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1,
                        "error": { "code": -32002, "message": "busy" } }))
                }
            }
        }
    }
    let rpc = RpcClient::with_transport(Busy(RefCell::new(0)));
    let err = read_pending_withdrawals(&rpc, STAKER_ADDR).expect_err("an error");
    assert!(matches!(err, StakingError::Rpc(_)), "{err:?}");
    assert_eq!(*rpc.transport().0.borrow(), 1, "no retry");
}

#[test]
fn the_retry_back_off_is_bounded() {
    let waits: Vec<u64> = (0..LOGS_RETRY_ATTEMPTS).map(|a| logs_retry_backoff(a).as_millis() as u64).collect();
    assert_eq!(waits.first(), Some(&250));
    assert!(waits.windows(2).all(|w| w[1] >= w[0]));
    assert!(waits.iter().all(|w| *w <= 2_000), "{waits:?}");
}

// After the bounded retries the read is "still indexing chain history", never an empty list, and
// the blocks read before stay in the scan.
#[test]
fn a_window_that_keeps_asking_for_a_retry_is_still_indexing() {
    let rpc = RpcClient::with_transport(ChainStub::new(1_200, vec![]));
    *rpc.transport().retry_first.borrow_mut() = 1_000;
    let mut scan = WithdrawalScan::default();
    let err = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect_err("indexing");
    assert!(matches!(err, StakingError::Indexing(_)), "{err:?}");
    assert!(err.to_string().contains("still indexing chain history"), "{err}");
    assert_eq!(rpc.transport().log_ranges().len() as u32, LOGS_RETRY_ATTEMPTS);
}

// The scan progress is small (next block + ids) and survives a relaunch: a saved scan resumes at
// its next block instead of rescanning from block 427.
#[test]
fn saved_scan_progress_survives_a_relaunch() {
    let dir = std::env::temp_dir().join(format!("c1w-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let scan = WithdrawalScan {
        next_block: 40_001,
        ids: [9u128].into_iter().collect(),
        anchor: Some((40_000, stub_hash(40_000))),
        generation: 2,
        ..Default::default()
    };
    save_scan(&file, "k", &scan).expect("save");
    assert_eq!(load_scan(&file, "k"), scan);
    assert_eq!(load_scan(&file, "other"), WithdrawalScan::default());
    // A file saved by an earlier build carries stored claims: they are ignored (every id is read
    // again), and the position, ids and anchor load.
    std::fs::write(&file, serde_json::json!({ "k": {
        "next_block": 40_001, "ids": [9], "claimed": { "4": 39_000 },
        "anchor": [40_000, stub_hash(40_000)], "generation": 2 } }).to_string()).expect("write");
    assert_eq!(load_scan(&file, "k"), scan, "an earlier file's claims are dropped");
    std::fs::write(&file, "not json").expect("write");
    assert_eq!(load_scan(&file, "k"), WithdrawalScan::default(), "a corrupt file starts over");
    let _ = std::fs::remove_dir_all(&dir);
}

// #371 review: eth_blockNumber (stored height) can be ahead of what eth_getLogs serves (applied
// tip). A position committed past the applied tip, then saved, would skip those blocks forever.
// With no executed-head evidence nothing is committed, so after a relaunch the blocks are read
// again and the withdrawal is found.
#[test]
fn a_head_ahead_of_the_logs_does_not_skip_a_withdrawal_after_a_relaunch() {
    let dir = std::env::temp_dir().join(format!("c1w-lag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![(12, 4_500, false)]));
    *rpc.transport().applied_tip.borrow_mut() = Some(4_200);
    // Launch 1: the node has stored 5,000 blocks but applied 4,200; block 4,500 is not served yet.
    let mut scan = load_scan(&file, "k");
    let first = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 1");
    assert!(first.is_empty());
    assert_eq!(scan.next_block, 0, "no evidence: nothing committed");
    save_scan(&file, "k", &scan).expect("save");
    // Launch 2: the node has applied everything.
    *rpc.transport().applied_tip.borrow_mut() = None;
    let mut scan = load_scan(&file, "k");
    let second = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 2");
    assert_eq!(second.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["12"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// #371 review: overlapping refreshes must not roll progress back. A slower read that started
// from older progress and saves last keeps the larger position and the union of ids.
#[test]
fn an_older_scan_saved_last_does_not_roll_progress_back() {
    let dir = std::env::temp_dir().join(format!("c1w-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let newer = WithdrawalScan { next_block: 90_000, ids: [1u128, 2].into_iter().collect(), ..Default::default() };
    let older = WithdrawalScan { next_block: 20_000, ids: [1u128, 3].into_iter().collect(), ..Default::default() };
    save_scan(&file, "k", &newer).expect("save newer");
    save_scan(&file, "k", &older).expect("save older");
    let got = load_scan(&file, "k");
    assert_eq!(got.next_block, 90_000);
    assert_eq!(got.ids, [1u128, 2, 3].into_iter().collect());
    // The in-memory store merges the same way.
    assert_eq!(merge_scans(&newer, &older), got);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_saves_leave_a_whole_file_with_the_furthest_progress() {
    let dir = std::env::temp_dir().join(format!("c1w-conc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let handles: Vec<_> = (1..=16u64)
        .map(|i| {
            let file = file.clone();
            std::thread::spawn(move || {
                let s = WithdrawalScan { next_block: i * 1_000, ids: [i as u128].into_iter().collect(), ..Default::default() };
                save_scan(&file, "k", &s).expect("save");
            })
        })
        .collect();
    for h in handles {
        h.join().expect("join");
    }
    let got = load_scan(&file, "k");
    assert_eq!(got.next_block, 16_000);
    assert_eq!(got.ids.len(), 16, "no save lost");
    // Written by temp file + rename: nothing but the scan file is left in the folder.
    let names: Vec<String> = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["withdrawal-scans.json".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

// Codex P1 (#371 at c38974a): a fixed overlap does not prove the logs were served. The node's
// applied tip (eth_syncing currentBlock) is the evidence: stored head 5000, applied 3000, a
// withdrawal at 3500. The committed position never passes the applied tip, so after a relaunch
// (the node caught up) the withdrawal is found.
#[test]
fn the_committed_position_never_passes_the_applied_tip() {
    let dir = std::env::temp_dir().join(format!("c1w-applied-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![(21, 3_500, false)]));
    *rpc.transport().applied_tip.borrow_mut() = Some(3_000);
    *rpc.transport().syncing.borrow_mut() = Some(serde_json::json!({
        "startingBlock": "0xbb8", "currentBlock": "0xbb8", "highestBlock": "0x1388" }));
    let mut scan = load_scan(&file, "k");
    let first = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 1");
    assert!(first.is_empty());
    assert!(scan.next_block <= 3_001, "committed {} past the applied tip", scan.next_block);
    save_scan(&file, "k", &scan).expect("save");
    *rpc.transport().applied_tip.borrow_mut() = None;
    *rpc.transport().syncing.borrow_mut() = Some(JsonValue::Bool(false));
    let mut scan = load_scan(&file, "k");
    let second = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 2");
    assert_eq!(second.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["21"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// With no evidence of the applied tip (eth_syncing fails), the list is still read in full but the
// committed position does not move.
#[test]
fn without_applied_tip_evidence_the_position_does_not_advance() {
    let rpc = RpcClient::with_transport(ChainStub::new(9_000, vec![(5, 6_000, false)]));
    *rpc.transport().syncing.borrow_mut() = None;
    let mut scan = WithdrawalScan::default();
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(got.len(), 1);
    assert!(scan.next_block <= LIQUID_STAKING_POOL_DEPLOY_BLOCK, "{}", scan.next_block);
}

// Codex P2: a claimed id is never listed while the chain says it is claimed, after a merge with an
// older scan that still lists it too.
#[test]
fn a_claimed_id_is_never_listed_while_claimed() {
    let rpc = RpcClient::with_transport(ChainStub::new(3_000, vec![(30, 1_000, true), (31, 1_100, false)]));
    let mut scan = WithdrawalScan::default();
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(ids_of(&got), vec!["31"], "claimed: not listed");
    let older = WithdrawalScan { next_block: 500, ids: [30u128, 31].into_iter().collect(), ..Default::default() };
    let mut merged = merge_scans(&older, &scan);
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut merged, None).expect("read");
    assert_eq!(ids_of(&got), vec!["31"], "still claimed: not listed");
}

// Codex P2: the withdrawal reads are batched (one JSON-RPC batch per 50 ids) and inside the budget.
#[test]
fn withdrawal_reads_are_batched() {
    let w: Vec<(u128, u64, bool)> = (1..=120u128).map(|i| (i, 500 + i as u64, false)).collect();
    let rpc = RpcClient::with_transport(ChainStub::new(2_000, w));
    let got = read_pending_withdrawals(&rpc, STAKER_ADDR).expect("read");
    assert_eq!(got.len(), 120);
    assert_eq!(rpc.transport().batches(), 3, "120 ids in batches of {WITHDRAWAL_READ_BATCH}");
}

#[test]
fn the_withdrawal_reads_stop_at_the_deadline_with_still_reading() {
    let w: Vec<(u128, u64, bool)> = (1..=120u128).map(|i| (i, 500 + i as u64, false)).collect();
    let rpc = RpcClient::with_transport(ChainStub::new(2_000, w));
    let mut scan = WithdrawalScan { next_block: 2_001, ids: (1..=120u128).collect(), ..Default::default() };
    let err = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(std::time::Instant::now()))
        .expect_err("budget spent");
    assert!(err.to_string().contains("still reading chain history"), "{err}");
    assert_eq!(rpc.transport().batches(), 0);
}

// citrate_executedHead {number, hash} is the applied tip eth_getLogs serves. When the node serves
// it, progress is committed up to exactly that block, anchored on its hash.
#[test]
fn an_executed_head_bounds_the_committed_position() {
    let dir = std::env::temp_dir().join(format!("c1w-exec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![(22, 3_500, false)]));
    *rpc.transport().applied_tip.borrow_mut() = Some(3_000);
    *rpc.transport().executed_head.borrow_mut() = Some(3_000);
    let mut scan = load_scan(&file, "k");
    assert!(read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 1").is_empty());
    assert_eq!(scan.next_block, 3_001 - EXECUTED_HEAD_MARGIN_BLOCKS, "up to the executed head, less the margin");
    assert_eq!(scan.anchor, Some((3_000, stub_hash(3_000))), "anchored on the executed head");
    save_scan(&file, "k", &scan).expect("save");
    *rpc.transport().applied_tip.borrow_mut() = None;
    *rpc.transport().executed_head.borrow_mut() = Some(5_000);
    let mut scan = load_scan(&file, "k");
    let second = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 2");
    assert_eq!(second.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["22"]);
    assert_eq!(scan.next_block, 5_001 - EXECUTED_HEAD_MARGIN_BLOCKS);
    let _ = std::fs::remove_dir_all(&dir);
}

// A rewound scan (newer generation) wins a merge over an older, further scan, so the rewind is
// never undone by the max-position rule.
#[test]
fn a_rewind_wins_a_merge_over_older_progress() {
    let stale = WithdrawalScan { next_block: 9_001, ids: [1u128].into_iter().collect(), ..Default::default() };
    let rewound = WithdrawalScan { next_block: 5_001, ids: [2u128].into_iter().collect(), generation: 1, ..Default::default() };
    let m = merge_scans(&stale, &rewound);
    assert_eq!(m.next_block, 5_001);
    assert_eq!(m.generation, 1);
    assert_eq!(m.ids, [1u128, 2].into_iter().collect(), "ids are re-validated on chain, so both are kept");
    assert_eq!(merge_scans(&rewound, &stale), m, "order does not matter");
}

// citrate-chain#337 scanner protocol: read the executed head, getLogs with toBlock <= head.number,
// persist at most head.number - margin, never advance on an error.
//
// -32603 ("no applied tip / the read failed") is NO evidence: the position does not move, and it is
// not mistaken for an older node without the method (which falls back to eth_syncing).
#[test]
fn an_executed_head_error_is_no_evidence_and_does_not_advance() {
    let rpc = RpcClient::with_transport(ChainStub::new(9_000, vec![(8, 4_000, false)]));
    *rpc.transport().executed_head_error.borrow_mut() = Some(-32603);
    let mut scan = WithdrawalScan::default();
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(got.len(), 1, "the list is still read");
    assert_eq!(scan.next_block, 0, "no evidence: the position does not move");
    let mut later = WithdrawalScan { next_block: 5_001, ..Default::default() };
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut later, None).expect("read");
    assert_eq!(later.next_block, 5_001, "an error never advances a resumed position");
}

#[test]
fn the_scan_never_asks_for_logs_past_the_executed_head() {
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![]));
    *rpc.transport().executed_head.borrow_mut() = Some(3_000);
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    let ranges = rpc.transport().log_ranges();
    assert!(ranges.iter().all(|(_, b)| *b <= 3_000), "toBlock past the executed head: {ranges:?}");
    assert_eq!(ranges.last().map(|r| r.1), Some(3_000));
}

#[test]
fn progress_is_persisted_a_margin_below_the_executed_head() {
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![]));
    *rpc.transport().executed_head.borrow_mut() = Some(3_000);
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(scan.next_block, 3_001 - 100, "head.number - the reorg margin");
    assert_eq!(EXECUTED_HEAD_MARGIN_BLOCKS, 100, "citrate-chain MAX_REORG_DEPTH");
}

// The refined #337 scanner protocol (RPC review): h1 = executedHead, getLogs up to min(target, h1),
// h2 = executedHead again, commit at most min(h1, h2) - margin. A rewind probe saw h1 = 5 while
// getLogs served only up to 4, so h1 alone is never trusted.
#[test]
fn progress_is_committed_below_the_lower_of_two_executed_head_reads() {
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![]));
    rpc.transport().executed_next.borrow_mut().extend([Ok(3_000), Ok(2_950)]);
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(scan.next_block, 2_951 - EXECUTED_HEAD_MARGIN_BLOCKS, "min(h1, h2) - margin");
    let heads = rpc.transport().requests.borrow().iter().filter(|r| r["method"] == "citrate_executedHead").count();
    assert_eq!(heads, 2, "h1 before the logs, h2 after");
}

#[test]
fn a_failed_second_executed_head_read_never_advances() {
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![(3, 2_000, false)]));
    rpc.transport().executed_next.borrow_mut().extend([Ok(3_000), Err(-32603)]);
    let mut scan = WithdrawalScan::default();
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(got.len(), 1, "the list is still read and confirmed from contract state");
    assert_eq!(scan.next_block, 0, "no h2: nothing committed");
}

#[test]
fn a_window_error_never_advances_the_position() {
    let rpc = RpcClient::with_transport(ChainStub::new(9_000, vec![(3, 2_000, false)]));
    *rpc.transport().executed_head.borrow_mut() = Some(9_000);
    *rpc.transport().fail_from.borrow_mut() = Some(5_000);
    let mut scan = WithdrawalScan::default();
    assert!(read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).is_err());
    assert_eq!(scan.next_block, 0, "an error commits nothing");
    assert!(scan.ids.contains(&3), "ids seen are kept (each is confirmed from contract state)");
}

// DGX probe A (#371 at eebb3ba): `eth_syncing == false` only means no peer showed a higher height.
// It is NOT evidence of the applied tip. Stored head 5000, applied 3000, no citrate_executedHead
// (an older node), a withdrawal at 3500: nothing is committed, and after a relaunch (the node
// caught up) the withdrawal is found.
#[test]
fn probe_a_syncing_false_is_not_evidence() {
    let dir = std::env::temp_dir().join(format!("c1w-probe-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![(50, 3_500, false)]));
    *rpc.transport().applied_tip.borrow_mut() = Some(3_000);
    *rpc.transport().syncing.borrow_mut() = Some(JsonValue::Bool(false));
    let mut scan = load_scan(&file, "k");
    assert!(read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 1").is_empty());
    assert_eq!(scan.next_block, 0, "syncing=false committed {}", scan.next_block);
    assert!(!scan.last_read_confirmed, "no evidence: the read is not confirmed");
    save_scan(&file, "k", &scan).expect("save");
    *rpc.transport().applied_tip.borrow_mut() = None;
    let mut scan = load_scan(&file, "k");
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("launch 2");
    assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["50"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// DGX probe B: a timeout, -32603, any other error code or a malformed result from
// citrate_executedHead is no evidence, even when eth_syncing would report a tip.
#[test]
fn probe_b_only_method_not_found_falls_back() {
    let behind = serde_json::json!({ "startingBlock": "0x1", "currentBlock": "0x1f40", "highestBlock": "0x2328" });
    type Setup = fn(&ChainStub);
    let cases: [(&str, Setup); 4] = [
        ("timeout", |s| *s.executed_head_timeout.borrow_mut() = true),
        ("-32603", |s| *s.executed_head_error.borrow_mut() = Some(-32603)),
        ("-32000", |s| *s.executed_head_error.borrow_mut() = Some(-32000)),
        ("malformed", |s| {
            *s.executed_head_raw.borrow_mut() = Some(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": { "hash": "0x01" } }))
        }),
    ];
    for (name, setup) in cases {
        let stub = ChainStub::new(9_000, vec![(4, 2_000, false)]);
        setup(&stub);
        *stub.syncing.borrow_mut() = Some(behind.clone());
        let rpc = RpcClient::with_transport(stub);
        let mut scan = WithdrawalScan::default();
        let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect(name);
        assert_eq!(got.len(), 1, "{name}: the list is still read");
        assert_eq!(scan.next_block, 0, "{name}: no evidence, no advance");
        assert!(!scan.last_read_confirmed, "{name}");
    }
    // -32601 (an older node): no eth_syncing fallback any more (owner decision, round 5), so this
    // is no evidence either: see without_executed_head_eth_syncing_is_never_evidence.
}

// The executed-head path with both reads served confirms the read.
#[test]
fn an_executed_head_read_is_confirmed() {
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![]));
    *rpc.transport().executed_head.borrow_mut() = Some(5_000);
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert!(scan.last_read_confirmed);
}

// The RPC calls outside the paging loop (eth_blockNumber, h1, eth_syncing, the anchor check, h2)
// honour the deadline too: with the budget spent, nothing is called.
#[test]
fn a_spent_budget_makes_no_rpc_call() {
    let rpc = RpcClient::with_transport(ChainStub::new(5_000, vec![]));
    *rpc.transport().executed_head.borrow_mut() = Some(5_000);
    let mut scan = WithdrawalScan { next_block: 4_001, anchor: Some((4_100, stub_hash(4_100))), ..Default::default() };
    let past = std::time::Instant::now() - std::time::Duration::from_millis(1);
    let err = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(past)).expect_err("spent");
    assert!(err.to_string().contains("still reading chain history"), "{err}");
    assert!(rpc.transport().requests.borrow().is_empty(), "{:?}", rpc.transport().requests.borrow());
    assert_eq!(scan.next_block, 4_001);
}

// With no evidence, a budget stop returns what it found (DGX B1): not complete, not confirmed,
// nothing committed, and the note says how far it read.
#[test]
fn a_budget_stop_without_evidence_returns_what_it_found() {
    let s = ChainStub::new(50_000_000, vec![(3, 600, false)]);
    s.logs_delay.set(std::time::Duration::from_millis(2));
    let rpc = RpcClient::with_transport(s);
    let mut scan = WithdrawalScan::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(30);
    let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(deadline)).expect("rows");
    assert_eq!(ids_of(&rows), vec!["3"]);
    assert!(!scan.last_read_complete && !scan.last_read_confirmed);
    assert!(scan.last_read_note.clone().unwrap_or_default().contains("reading chain history"));
    assert_eq!(scan.next_block, 0);
}

// ===========================================================================
// #371 round 5 (Codex at 198fd9b + DGX rev371d). Each test fails on an assertion against 198fd9b.
// ===========================================================================

fn ids_of(rows: &[PendingWithdrawal]) -> Vec<String> {
    let mut v: Vec<String> = rows.iter().map(|p| p.id.clone()).collect();
    v.sort();
    v
}

// Codex P1: a claim read at `latest` is not final. If it is reorged out, the withdrawal is
// claimable again and must be listed, after a relaunch and after a merge with a scan that still
// holds the old claim.
#[test]
fn a_claim_reorged_out_is_listed_again_after_persistence_and_merge() {
    let dir = std::env::temp_dir().join(format!("c1w-claim-reorg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let claimed = RpcClient::with_transport(ChainStub::new(5_000, vec![(7, 1_000, true)]));
    *claimed.transport().executed_head.borrow_mut() = Some(5_000);
    let mut scan = load_scan(&file, "k");
    let first = read_pending_withdrawals_with(&claimed, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert!(first.is_empty(), "claimed: not listed");
    save_scan(&file, "k", &scan).expect("save");
    // The claim was reorged out.
    let reopened = RpcClient::with_transport(ChainStub::new(5_000, vec![(7, 1_000, true)]));
    *reopened.transport().executed_head.borrow_mut() = Some(5_000);
    reopened.transport().unclaim.borrow_mut().insert(7);
    let mut relaunched = load_scan(&file, "k");
    let after = read_pending_withdrawals_with(&reopened, STAKER_ADDR, &mut relaunched, None).unwrap_or_default();
    assert_eq!(ids_of(&after), vec!["7"], "a reorged-out claim is listed again after a relaunch");
    let mut merged = merge_scans(&scan, &WithdrawalScan::default());
    let after_merge = read_pending_withdrawals_with(&reopened, STAKER_ADDR, &mut merged, None).unwrap_or_default();
    assert_eq!(ids_of(&after_merge), vec!["7"], "a merge never makes the old claim final");
    let _ = std::fs::remove_dir_all(&dir);
}

// A claim is never final: a claimed id is read from contract state on every pass, however far the
// committed position has moved past it, and after a rewind.
#[test]
fn a_claimed_id_is_read_on_every_pass() {
    let at = |head: u64| {
        let rpc = RpcClient::with_transport(ChainStub::new(head, vec![(7, 1_000, true)]));
        *rpc.transport().executed_head.borrow_mut() = Some(head);
        rpc
    };
    let mut scan = WithdrawalScan::default();
    for head in [5_000, 5_000, 5_300, 5_300, 9_000] {
        let r = at(head);
        let rows = read_pending_withdrawals_with(&r, STAKER_ADDR, &mut scan, None).unwrap_or_default();
        assert!(r.transport().asked_ids(0).contains(&7), "head {head}: the claimed id is read");
        assert!(rows.is_empty(), "head {head}: claimed, not listed");
    }
    assert!(scan.next_block > 5_000);
    // A reorg at the anchor (citrate_isExecuted says false) rewinds; the id is read again.
    let r = at(9_000);
    let anchor = scan.anchor.clone().map(|a| a.0).unwrap_or(9_000);
    r.transport().reorged.borrow_mut().insert(anchor, format!("0x{}", "ee".repeat(32)));
    read_pending_withdrawals_with(&r, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.generation, 1);
    assert!(r.transport().asked_ids(0).contains(&7), "after a rewind: read again");
}

// Codex P1 (#371 @5b6445a): a session cursor read on a branch that was replaced, or read with no
// evidence at all, must never be promoted into committed coverage. Call 1 reads branch A to 5000
// and the commit is refused (h1 and h2 name 5000 with two hashes). Branch B has a withdrawal at
// 4500. Call 2 on B must read from the committed position, find it, and only then commit.
#[test]
fn a_session_cursor_from_a_refused_commit_is_never_promoted() {
    let mut scan = WithdrawalScan::default();
    let a = ChainStub::new(5_000, vec![]);
    a.executed_next.borrow_mut().extend([Ok(5_000), Ok(5_000)]);
    a.executed_next_hashes.borrow_mut().extend([format!("0x{}", "aa".repeat(32)), stub_hash(5_000)]);
    read_pending_withdrawals_with(&RpcClient::with_transport(a), STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.next_block, 0, "the replacement commits nothing");
    let b = ChainStub::new(5_000, vec![(45, 4_500, false)]);
    *b.executed_head.borrow_mut() = Some(5_000);
    let rpc = RpcClient::with_transport(b);
    let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(rpc.transport().log_ranges()[0].0, LIQUID_STAKING_POOL_DEPLOY_BLOCK, "read again from the committed position");
    assert_eq!(ids_of(&rows), vec!["45"], "the withdrawal on the replacement branch is found");
    assert!(scan.last_read_confirmed);
}

// The same promotion from a read with no evidence (today's RPC, no citrate_executedHead), when
// evidence appears later in the session (the RPC is upgraded) or the anchor check fails.
#[test]
fn a_session_cursor_read_without_evidence_is_never_promoted() {
    type Setup = fn(&ChainStub);
    let no_evidence: [(&str, Setup); 3] = [
        ("no executedHead", |_| {}),
        ("executedHead error", |s| *s.executed_head_error.borrow_mut() = Some(-32603)),
        ("executedHead timeout", |s| *s.executed_head_timeout.borrow_mut() = true),
    ];
    for (name, setup) in no_evidence {
        let mut scan = WithdrawalScan::default();
        let first = ChainStub::new(5_000, vec![]);
        setup(&first);
        read_pending_withdrawals_with(&RpcClient::with_transport(first), STAKER_ADDR, &mut scan, None).unwrap_or_default();
        assert_eq!(scan.next_block, 0, "{name}: nothing committed");
        let later = ChainStub::new(5_000, vec![(45, 4_500, false)]);
        *later.executed_head.borrow_mut() = Some(5_000);
        let rpc = RpcClient::with_transport(later);
        let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
        assert_eq!(ids_of(&rows), vec!["45"], "{name}: found once evidence is read");
        assert_eq!(rpc.transport().log_ranges()[0].0, LIQUID_STAKING_POOL_DEPLOY_BLOCK, "{name}");
    }
    // An anchor check that fails is no evidence too: the session cursor it leaves is not promoted.
    let anchored = WithdrawalScan { next_block: 2_901, anchor: Some((3_000, stub_hash(3_000))), ..Default::default() };
    let mut scan = anchored;
    let s = ChainStub::new(5_000, vec![]);
    *s.executed_head.borrow_mut() = Some(5_000);
    *s.is_executed.borrow_mut() = Some(Err(-32603));
    read_pending_withdrawals_with(&RpcClient::with_transport(s), STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.next_block, 2_901, "no advance without evidence");
    let s = ChainStub::new(5_000, vec![(46, 3_500, false)]);
    *s.executed_head.borrow_mut() = Some(5_000);
    let rpc = RpcClient::with_transport(s);
    let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(rpc.transport().log_ranges()[0].0, 2_901, "read from the committed position");
    assert_eq!(ids_of(&rows), vec!["46"]);
}

// Codex P1 (#371 @5b6445a): an eth_call at `latest` may see a later block than the executed head
// read before it, and on another branch. A claim seen at h1 = h2 = 5000 (the claim tx in 5001) is
// replaced by a fork that keeps 5000 and grows to 5200. The anchor still checks out, the position
// moves past 5000, and the withdrawal must still be listed: in the same session, after a relaunch,
// and after a merge.
#[test]
fn a_claim_seen_past_the_executed_head_and_forked_out_is_listed_again() {
    let dir = std::env::temp_dir().join(format!("c1w-claim-fork-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let claimed = ChainStub::new(5_000, vec![(7, 1_000, true)]);
    *claimed.executed_head.borrow_mut() = Some(5_000);
    let mut scan = WithdrawalScan::default();
    let first = read_pending_withdrawals_with(&RpcClient::with_transport(claimed), STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert!(first.is_empty(), "claimed: not listed");
    save_scan(&file, "k", &scan).expect("save");
    let forked = || {
        let s = ChainStub::new(5_200, vec![(7, 1_000, true)]);
        *s.executed_head.borrow_mut() = Some(5_200);
        s.unclaim.borrow_mut().insert(7);
        RpcClient::with_transport(s)
    };
    let mut same_session = scan.clone();
    let rpc = forked();
    let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut same_session, None).unwrap_or_default();
    assert!(same_session.next_block > 5_000, "the position passed the head the claim was seen at");
    assert!(rpc.transport().asked_ids(0).contains(&7), "the claimed id is read again");
    assert_eq!(ids_of(&rows), vec!["7"], "same session: listed again");
    let mut relaunched = load_scan(&file, "k");
    let rows = read_pending_withdrawals_with(&forked(), STAKER_ADDR, &mut relaunched, None).unwrap_or_default();
    assert_eq!(ids_of(&rows), vec!["7"], "after a relaunch: listed again");
    let mut merged = merge_scans(&same_session, &load_scan(&file, "k"));
    let rows = read_pending_withdrawals_with(&forked(), STAKER_ADDR, &mut merged, None).unwrap_or_default();
    assert_eq!(ids_of(&rows), vec!["7"], "after a merge: listed again");
    let _ = std::fs::remove_dir_all(&dir);
}

// Codex P2: confirmed means the served history reached the requested head. Stored head 5000,
// executed tip 3000, a withdrawal at 3500: the read is complete as far as the node serves, but NOT
// confirmed, and the note says how far the node has served.
#[test]
fn a_node_behind_the_head_is_never_confirmed() {
    let stub = ChainStub::new(5_000, vec![(9, 3_500, false)]);
    *stub.executed_head.borrow_mut() = Some(3_000);
    *stub.applied_tip.borrow_mut() = Some(3_000);
    let rpc = RpcClient::with_transport(stub);
    let mut scan = WithdrawalScan::default();
    let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert!(rows.is_empty());
    assert!(!scan.last_read_confirmed, "served to 3000 of 5000 is not confirmed");
    assert!(scan.last_read_complete, "it read everything the node serves");
    let note = scan.last_read_note.clone().unwrap_or_default();
    assert!(note.contains("3000") && note.contains("5000"), "{note}");
}

// Owner decision (round 5): no eth_syncing fallback. On a node without chain#336,
// eth_syncing.currentBlock can pass receipts that were never stored. Without citrate_executedHead
// (-32601) there is no evidence, whatever eth_syncing says: nothing is committed.
#[test]
fn without_executed_head_eth_syncing_is_never_evidence() {
    let s = ChainStub::new(9_000, vec![(4, 2_000, false)]);
    *s.syncing.borrow_mut() = Some(serde_json::json!({ "startingBlock": "0x1", "currentBlock": "0x1f40", "highestBlock": "0x2328" }));
    let rpc = RpcClient::with_transport(s);
    let mut scan = WithdrawalScan::default();
    let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(ids_of(&rows), vec!["4"], "rows are still read");
    assert_eq!(scan.next_block, 0, "eth_syncing is not evidence");
    assert!(!scan.last_read_confirmed);
}

// Codex P2: detail reads make progress across calls. 251 ids, slow batches, a budget that fits
// about two batches: every id is read exactly once and the read completes in a bounded number of
// calls.
#[test]
fn withdrawal_detail_reads_resume_across_calls() {
    let w: Vec<(u128, u64, bool)> = (1..=251u128).map(|i| (i, 500 + i as u64, false)).collect();
    let stub = ChainStub::new(2_000, w);
    *stub.executed_head.borrow_mut() = Some(2_000);
    stub.batch_delay.set(std::time::Duration::from_millis(40));
    let rpc = RpcClient::with_transport(stub);
    let mut scan = WithdrawalScan::default();
    let mut rows = Vec::new();
    let mut calls = 0;
    for _ in 0..20 {
        calls += 1;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(90);
        rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(deadline)).unwrap_or_default();
        if scan.last_read_complete {
            break;
        }
    }
    assert!(scan.last_read_complete, "the detail reads finish");
    assert_eq!(rows.len(), 251);
    assert!(calls <= 6, "{calls} calls");
    assert_eq!(rpc.transport().batches(), 6, "each id read once: no restart from the first id");
}

// DGX probe 5 (B1): today's RPC (no executedHead, eth_syncing false), a history longer than one
// call's budget. Rows found are shown at once (confirmed=false); a session-only cursor (never
// saved) carries the scan forward across calls until it reaches the head.
#[test]
fn probe_5_a_session_cursor_reaches_the_head_and_found_rows_are_shown() {
    let dir = std::env::temp_dir().join(format!("c1w-probe5-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    let stub = ChainStub::new(30_000, vec![(1, 900, false), (2, 29_000, false)]);
    stub.logs_delay.set(std::time::Duration::from_millis(3));
    let rpc = RpcClient::with_transport(stub);
    let mut scan = WithdrawalScan::default();
    let budget = || Some(std::time::Instant::now() + std::time::Duration::from_millis(12));
    let first = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, budget()).unwrap_or_default();
    assert_eq!(ids_of(&first), vec!["1"], "the row found in the first call is shown");
    assert!(!scan.last_read_confirmed && !scan.last_read_complete);
    let mut starts = vec![rpc.transport().log_ranges()[0].0];
    let mut seen = rpc.transport().log_ranges().len();
    let mut rows = first;
    for _ in 0..60 {
        if scan.last_read_complete {
            break;
        }
        rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, budget()).unwrap_or_default();
        let ranges = rpc.transport().log_ranges();
        if ranges.len() > seen {
            starts.push(ranges[seen].0);
        }
        seen = ranges.len();
    }
    assert!(scan.last_read_complete, "the session cursor reaches the head");
    assert_eq!(ids_of(&rows), vec!["1", "2"]);
    assert!(starts.windows(2).all(|w| w[1] > w[0]), "each call starts further on: {starts:?}");
    assert_eq!(scan.next_block, 0, "nothing is committed without evidence");
    // The cursor is session-only: after a relaunch the scan starts again at the deployment block.
    save_scan(&file, "k", &scan).expect("save");
    let mut relaunched = load_scan(&file, "k");
    let fresh = RpcClient::with_transport(ChainStub::new(30_000, vec![(1, 900, false), (2, 29_000, false)]));
    read_pending_withdrawals_with(&fresh, STAKER_ADDR, &mut relaunched, budget()).unwrap_or_default();
    assert_eq!(fresh.transport().log_ranges()[0].0, LIQUID_STAKING_POOL_DEPLOY_BLOCK);
    let _ = std::fs::remove_dir_all(&dir);
}

// DGX probe 6 (B2): abandoned blocks stay served on Citrate, so a hash read can't see a reorg. An
// older node (no citrate_isExecuted): the executed head falling below the anchor rewinds the scan,
// and a withdrawal on the new branch below the old anchor is found after the chain grows back.
#[test]
fn probe_6_the_executed_head_below_the_anchor_rewinds() {
    let node = |head: u64, w: Vec<(u128, u64, bool)>| {
        let s = ChainStub::new(head, w);
        *s.executed_head.borrow_mut() = Some(head);
        *s.is_executed.borrow_mut() = Some(Err(-32601));
        RpcClient::with_transport(s)
    };
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&node(6_000, vec![]), STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.anchor.as_ref().map(|a| a.0), Some(6_000));
    // An operator rollback: the executed head is 4200, below the anchor.
    read_pending_withdrawals_with(&node(4_200, vec![]), STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.generation, 1, "the fall below the anchor is a reorg");
    // The chain grows back on the new branch, with a withdrawal at 4500.
    let rows = read_pending_withdrawals_with(&node(7_000, vec![(45, 4_500, false)]), STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(ids_of(&rows), vec!["45"], "the withdrawal at 4500 is found");
    // Same height, another hash: also a reorg.
    let mut same = WithdrawalScan { next_block: 5_901, anchor: Some((6_000, format!("0x{}", "ab".repeat(32)))), ..Default::default() };
    read_pending_withdrawals_with(&node(6_000, vec![]), STAKER_ADDR, &mut same, None).unwrap_or_default();
    assert_eq!(same.generation, 1);
}

// chain#337 citrate_isExecuted(hash) is the anchor check. executed:false rewinds; executed:true at
// the anchor height does not; -32601 falls back to the executed-head heuristic; -32603, -32602, a
// timeout or a malformed result is no evidence (no advance, no rewind).
#[test]
fn the_anchor_is_checked_with_is_executed() {
    let anchored = || WithdrawalScan { next_block: 5_901, anchor: Some((6_000, stub_hash(6_000))), ..Default::default() };
    // executed:false (the anchored block was abandoned; it is still stored).
    let s = ChainStub::new(7_000, vec![(46, 5_500, false)]);
    *s.executed_head.borrow_mut() = Some(7_000);
    s.reorged.borrow_mut().insert(6_000, format!("0x{}", "ee".repeat(32)));
    let rpc = RpcClient::with_transport(s);
    let mut scan = anchored();
    let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(ids_of(&rows), vec!["46"], "rewound and re-read");
    assert_eq!(scan.generation, 1);
    let asked = rpc.transport().requests.borrow().iter().find(|r| r["method"] == "citrate_isExecuted").cloned();
    assert_eq!(asked.map(|r| r["params"][0].clone()), Some(serde_json::json!(stub_hash(6_000))));
    // executed:true at the anchor height: no rewind.
    let s = ChainStub::new(7_000, vec![]);
    *s.executed_head.borrow_mut() = Some(7_000);
    let rpc = RpcClient::with_transport(s);
    let mut scan = anchored();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.generation, 0);
    assert_eq!(rpc.transport().log_ranges()[0].0, 5_901);
    // Errors and malformed answers: no evidence.
    type Setup = fn(&ChainStub);
    let cases: [(&str, Setup); 4] = [
        ("-32603", |s| *s.is_executed.borrow_mut() = Some(Err(-32603))),
        ("-32602", |s| *s.is_executed.borrow_mut() = Some(Err(-32602))),
        ("timeout", |s| *s.is_executed_timeout.borrow_mut() = true),
        ("malformed", |s| *s.is_executed.borrow_mut() = Some(Ok(serde_json::json!({ "executed": "yes" })))),
    ];
    for (name, setup) in cases {
        let s = ChainStub::new(7_000, vec![(47, 6_500, false)]);
        *s.executed_head.borrow_mut() = Some(7_000);
        setup(&s);
        let rpc = RpcClient::with_transport(s);
        let mut scan = anchored();
        let rows = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
        assert_eq!(ids_of(&rows), vec!["47"], "{name}: still read");
        assert_eq!(scan.next_block, 5_901, "{name}: no advance");
        assert_eq!(scan.generation, 0, "{name}: no rewind");
        assert!(!scan.last_read_confirmed, "{name}");
    }
    // -32601 (an older node): the heuristic; the head above the anchor, so no rewind, and progress.
    let s = ChainStub::new(7_000, vec![]);
    *s.executed_head.borrow_mut() = Some(7_000);
    *s.is_executed.borrow_mut() = Some(Err(-32601));
    let rpc = RpcClient::with_transport(s);
    let mut scan = anchored();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.generation, 0);
    assert_eq!(scan.next_block, 7_001 - EXECUTED_HEAD_MARGIN_BLOCKS);
}

// DGX non-blocking 2: h1 and h2 at one height with different hashes are a replacement in flight:
// nothing is committed this pass.
#[test]
fn h1_and_h2_at_one_height_with_two_hashes_commit_nothing() {
    let s = ChainStub::new(5_000, vec![]);
    s.executed_next.borrow_mut().extend([Ok(5_000), Ok(5_000)]);
    s.executed_next_hashes.borrow_mut().extend([stub_hash(5_000), format!("0x{}", "cc".repeat(32))]);
    let rpc = RpcClient::with_transport(s);
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert_eq!(scan.next_block, 0);
    assert!(!scan.last_read_confirmed);
}

#[test]
fn the_scan_starts_at_the_pool_deployment_block() {
    // contracts/addresses/40204.provenance.json: LiquidStakingPool created at block 427.
    assert_eq!(LIQUID_STAKING_POOL_DEPLOY_BLOCK, 427);
    assert_eq!(LOG_WINDOW_BLOCKS, 1000);
    assert_eq!(EXECUTED_HEAD_MARGIN_BLOCKS, 100);
}

#[test]
fn read_pending_withdrawals_rejects_bad_address_before_any_rpc() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert!(read_pending_withdrawals(&rpc, "0xnope").is_err());
    assert!(
        rpc.transport().requests().is_empty(),
        "fail closed BEFORE any node call on a bad address"
    );
}

#[test]
fn read_self_shares_reads_pool_shares_over_mock_rpc() {
    let shares: u128 = 4_000_000_000_000_000_000;
    let rpc = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(uint256_hex(shares)))]));
    let got = read_self_shares(&rpc, STAKER_ADDR).expect("shares read");
    assert_eq!(got, shares, "the real shares() value, not a sim");
    let body = &rpc.transport().requests()[0];
    let data = body["params"][0]["data"].as_str().unwrap();
    assert!(data.starts_with("0xce7c2ac2"), "shares(address) selector: {data}");
}

// ---------------------------------------------------------------------------
// STD tripwire (C1-W). The bug class: a withdrawal-scan position committed past what the node has
// shown it executed, e.g. a fixed depth below eth_blockNumber, which skips unserved blocks for
// good once saved. Structural checks over the production source:
//  1. every write to a scan position is bounded by `commit_cap` (executed evidence) or is a reorg
//     rewind (`saturating_sub(LOG_WINDOW_BLOCKS)`);
//  2. `commit_cap` asks `citrate_executedHead` first, and its executed-head arm uses no fixed depth;
//  3. the fixed depth (SAFE_DEPTH_BLOCKS) appears only in the no-executed-head fallback, next to
//     the eth_syncing applied tip.
// ---------------------------------------------------------------------------
fn scan_position_violations(prod: &str) -> Vec<String> {
    let mut bad = Vec::new();
    for stmt in prod.split(';') {
        let mut rest = stmt;
        while let Some(i) = rest.find("next_block =") {
            let after = &rest[i + "next_block =".len()..];
            if !after.starts_with('=') {
                // The session cursor is never saved; a rewind only moves the position back.
                let ok = stmt.contains("commit_cap")
                    || stmt.contains("session.next_block")
                    || after.trim() == "LIQUID_STAKING_POOL_DEPLOY_BLOCK";
                if !ok {
                    bad.push(format!("unbounded scan-position write: {}", stmt.trim()));
                }
            }
            rest = after;
        }
    }
    match prod.find("let commit_cap") {
        None => bad.push("no commit_cap".into()),
        Some(i) => {
            let def = &prod[i..i + prod[i..].find("};").unwrap_or(prod.len() - i)];
            if !def.contains("EXECUTED_HEAD_MARGIN_BLOCKS") {
                bad.push("the commit cap has no margin below the executed head".into());
            }
            if !def.contains("if evidence") || !def.contains("_ => None") {
                bad.push("the commit cap advances without evidence".into());
            }
        }
    }
    if prod.matches("rpc.executed_head()").count() < 2 {
        bad.push("citrate_executedHead is not read twice (h1 before the logs, h2 after)".into());
    }
    // Owner decision (round 5): eth_syncing is never scan evidence.
    if prod.contains(".syncing()") {
        bad.push("eth_syncing is used as scan evidence".into());
    }
    // The anchor is checked with citrate_isExecuted: a hash or number block read can't see a reorg
    // on Citrate (abandoned blocks stay stored).
    if !prod.contains("rpc.is_executed(") {
        bad.push("the anchor is not checked with citrate_isExecuted".into());
    }
    if prod.contains("block_number_by_hash") || prod.contains(".block_hash(") || prod.contains("eth_getBlockByNumber") {
        bad.push("a reorg or anchor check reads a block by hash or number".into());
    }
    // getLogs never reads past the executed head: the window end is `scan_end`.
    if !prod.contains(".min(scan_end)") {
        bad.push("the log windows are not capped at the executed head".into());
    }
    // #371 @5b6445a: with executed evidence the read starts at the committed position; the session
    // cursor (read without validated evidence) only moves the start when nothing can be committed.
    match prod.find("let start") {
        None => bad.push("no read start".into()),
        Some(i) => {
            let def = &prod[i..i + prod[i..].find(';').unwrap_or(prod.len() - i)];
            let compact: String = def.split_whitespace().collect();
            if def.contains("session.next_block") && !compact.contains("Some(_)=>scan.next_block,") {
                bad.push("the session cursor moves the read start under executed evidence".into());
            }
        }
    }
    // #371 @5b6445a: an eth_call at `latest` is not bound to a validated block, so a claim read
    // there never stops a withdrawal from being read again.
    if prod.contains("claim_settled") {
        bad.push("a withdrawal read is skipped on a stored claim".into());
    }
    if let Some(i) = prod.find("let wanted") {
        let def = &prod[i..i + prod[i..].find(';').unwrap_or(prod.len() - i)];
        if def.contains(".filter(") {
            bad.push("a withdrawal read is skipped on a stored claim".into());
        }
    }
    bad
}

#[test]
fn tripwire_no_scan_position_passes_executed_evidence() {
    let src = include_str!("staking.rs");
    let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap_or(src);
    // Doc comments may name the constants; only code counts.
    let code: String = prod
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let bad = scan_position_violations(&code);
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn tripwire_catches_a_fixed_depth_position() {
    // Negative control: the shapes of the earlier bugs fail the tripwire.
    let old = "let x = rpc.executed_head();\n\
               let a = rpc.block_number_by_hash(h);\n\
               let commit_cap = match e { Ok(Some(n)) => Some(n), Ok(None) => match rpc.syncing() { Ok(Some(t)) => Some(t), _ => None } };\n\
               scan.next_block = (head + 1).saturating_sub(SAFE_DEPTH_BLOCKS);\n";
    let bad = scan_position_violations(old);
    for want in ["unbounded scan-position write", "no margin", "without evidence", "read twice", "eth_syncing", "citrate_isExecuted", "by hash or number", "not capped at the executed head"] {
        assert!(bad.iter().any(|b| b.contains(want)), "{want}: {bad:?}");
    }
}

#[test]
fn tripwire_catches_a_promoted_session_cursor_and_a_settled_claim() {
    // Negative control: the 5b6445a shapes, and a filter that skips claimed ids under another name.
    let shapes = [
        ("let start = scan.next_block.max(scan.session.next_block.saturating_sub(M)).max(D);", "session cursor"),
        ("fn claim_settled(&self, id: u128) -> bool { true }", "skipped on a stored claim"),
        ("let wanted: Vec<u128> = scan.ids.iter().copied().filter(|id| !done(*id)).collect();", "skipped on a stored claim"),
    ];
    for (src, want) in shapes {
        let bad = scan_position_violations(src);
        assert!(bad.iter().any(|b| b.contains(want)), "{want}: {bad:?}");
    }
}

// ===========================================================================
// C1-W, aligned with citrate_getLogs (citrate-chain#337 @ 600d2984).
//
// `Node` answers the way that commit's RPC does. Every behaviour names the handler line or the
// chain test it mirrors (STD #6). Paths are in citrate-chain at 600d2984:
//   eth_rpc.rs = core/api/src/eth_rpc.rs, canonical_logs.rs = core/api/src/canonical_logs.rs,
//   rate_limit.rs = core/api/src/rate_limit.rs, executed_head.rs = core/api/tests/executed_head.rs.
// Model: one executed block per height on the applied tip's selected-parent chain
// (canonical_logs.rs:7-13). Blocks above `fork.0` are on branch `fork.1`, so a reorg replaces
// their hashes; the abandoned blocks stay stored and citrate_isExecuted says false for them
// (canonical_logs.rs:33-38, 491-525; executed_head.rs:522).
// ===========================================================================
use std::cell::Cell;

/// The hash of the block at height `n` on branch `tag` (branch 0 is `stub_hash(n)`).
fn branch_hash(tag: u8, n: u64) -> String {
    format!("0x{tag:02x}{n:062x}")
}

struct Node {
    /// `eth_blockNumber`: the stored-header height, which can be ahead of the executed tip
    /// (executed_head.rs:200 executed_head_is_below_block_number_when_headers_are_ahead).
    head: Cell<u64>,
    /// The persisted applied tip every executed-chain read starts from (canonical_logs.rs:706-712).
    tip: Cell<u64>,
    /// Blocks above `.0` are on branch `.1`; at and below it, branch 0.
    fork: Cell<Option<(u64, u8)>>,
    /// (id, request block, claimed, branch). A request exists on the chain only when its block is
    /// on its branch.
    withdrawals: RefCell<Vec<(u128, u64, bool, u8)>>,
    /// false = today's public RPC, which has no citrate_getLogs or citrate_isExecuted (-32601).
    citrate_methods: Cell<bool>,
    /// `toBlock - fromBlock` above this is -32005 with `data.maxBlockRange` (canonical_logs.rs:127-146);
    /// citrate_getLogs is never clamped (eth_rpc.rs:1994-1996).
    max_range: Cell<u64>,
    /// eth_getLogs on today's node: a wide range is cut to `fromBlock + 1000` without an error (the
    /// legacy clamp, canonical_logs.rs:97-101, 132-134).
    clamp: Cell<bool>,
    /// The next N log reads exceed the method budget: -32005 with no data, checked before anything
    /// else (rate_limit.rs:96-101; eth_rpc.rs:1958).
    budget_exceeded: Cell<u32>,
    /// The next N log reads answer -32002 `{retryable: true}`: the index warming
    /// (canonical_logs.rs:285-291, 417-424, 480-483).
    warming: Cell<u32>,
    /// (block, reads): that block's receipt is not stored yet for the next `reads` log reads that
    /// cover it. citrate_getLogs answers -32002 `{retryable, blockNumber}` (eth_rpc.rs:3335-3345,
    /// canonical_logs.rs:646-658; executed_head.rs:1198); eth_getLogs skips the transaction
    /// (eth_rpc.rs:1934-1940, ReceiptPolicy::SkipMissing).
    missing_receipt: Cell<Option<(u64, u32)>>,
    /// (block, low tip): the first log read whose range covers `block` is served from `low tip`, and
    /// the tip is back right after: a rewind and return within one call. The handler reads the tip
    /// once per call (canonical_logs.rs:554; executed_head.rs:1081
    /// cursor_scan_never_skips_logs_across_a_rewind, "Codex's sequence for one call").
    transient: Cell<Option<(u64, u64)>>,
    /// Raw answers for the next citrate_getLogs calls (None = served normally).
    getlogs_raw: RefCell<VecDeque<Option<JsonValue>>>,
    /// Raw answers for the next citrate_isExecuted calls (None = served normally).
    is_executed_raw: RefCell<VecDeque<Option<JsonValue>>>,
    /// A log read whose range covers this block fails at the transport (a timeout).
    fail_at: Cell<Option<u64>>,
    /// `withdrawals(id)` reads `claimed = false` for these (a claim that was reorged out).
    unclaim: RefCell<std::collections::HashSet<u128>>,
    logs_delay: Cell<std::time::Duration>,
    batch_delay: Cell<std::time::Duration>,
    requests: RefCell<Vec<JsonValue>>,
}

impl Node {
    /// A #337 node whose stored head and executed tip are both `tip`, with withdrawals on branch 0.
    fn new(tip: u64, w: Vec<(u128, u64, bool)>) -> Self {
        Node {
            head: Cell::new(tip),
            tip: Cell::new(tip),
            fork: Cell::new(None),
            withdrawals: RefCell::new(w.into_iter().map(|(i, b, c)| (i, b, c, 0)).collect()),
            citrate_methods: Cell::new(true),
            max_range: Cell::new(1_000),
            clamp: Cell::new(false),
            budget_exceeded: Cell::new(0),
            warming: Cell::new(0),
            missing_receipt: Cell::new(None),
            transient: Cell::new(None),
            getlogs_raw: RefCell::new(VecDeque::new()),
            is_executed_raw: RefCell::new(VecDeque::new()),
            fail_at: Cell::new(None),
            unclaim: RefCell::new(Default::default()),
            logs_delay: Cell::new(std::time::Duration::ZERO),
            batch_delay: Cell::new(std::time::Duration::ZERO),
            requests: RefCell::new(Vec::new()),
        }
    }
    /// Today's public RPC: no citrate_* methods; eth_getLogs clamps a wide range.
    fn today(tip: u64, w: Vec<(u128, u64, bool)>) -> Self {
        let n = Node::new(tip, w);
        n.citrate_methods.set(false);
        n.clamp.set(true);
        n
    }
    /// Both the stored head and the executed tip move to `n`.
    fn grow(&self, n: u64) {
        self.head.set(n);
        self.tip.set(n);
    }
    fn tag_at(&self, n: u64) -> u8 {
        match self.fork.get() {
            Some((f, t)) if n > f => t,
            _ => 0,
        }
    }
    fn hash_at(&self, n: u64) -> String {
        branch_hash(self.tag_at(n), n)
    }
    /// (method, fromBlock, toBlock, afterBlockHash) of every log read, in order.
    fn log_reads(&self) -> Vec<(String, u64, u64, Option<String>)> {
        self.requests
            .borrow()
            .iter()
            .filter(|r| r["method"] == "eth_getLogs" || r["method"] == "citrate_getLogs")
            .map(|r| {
                let f = &r["params"][0];
                (
                    r["method"].as_str().unwrap_or_default().to_string(),
                    hex_q(&f["fromBlock"]),
                    hex_q(&f["toBlock"]),
                    f["afterBlockHash"].as_str().map(str::to_string),
                )
            })
            .collect()
    }
    fn methods(&self) -> Vec<String> {
        self.requests
            .borrow()
            .iter()
            .filter_map(|r| r["method"].as_str().map(str::to_string))
            .collect()
    }
    /// The withdrawal ids read with `withdrawals(id)` in requests[from..].
    fn asked_ids(&self, from: usize) -> std::collections::BTreeSet<u128> {
        self.requests.borrow()[from..]
            .iter()
            .filter_map(|r| r.as_array().cloned())
            .flatten()
            .filter_map(|c| c["params"][0]["data"].as_str().map(|d| u128::from_str_radix(&d[d.len() - 32..], 16).unwrap_or(0)))
            .collect()
    }
    fn batches(&self) -> usize {
        self.requests.borrow().iter().filter(|r| r.is_array()).count()
    }
    /// The tip one log read is served from: read once per call (canonical_logs.rs:554).
    fn snapshot_tip(&self, from: u64, to: u64) -> u64 {
        match self.transient.get() {
            Some((b, low)) if from <= b && b <= to => {
                self.transient.set(None);
                low
            }
            _ => self.tip.get(),
        }
    }
    /// The withdrawal logs in `from..=to` on the current chain, ascending, with their block's hash
    /// (eth_rpc.rs:3346-3364: blockHash/blockNumber from the receipt of the executed block).
    fn logs_between(&self, from: u64, to: u64, skip_missing: bool) -> Vec<JsonValue> {
        let mut w: Vec<(u128, u64, bool, u8)> = self.withdrawals.borrow().clone();
        w.sort_by_key(|x| x.1);
        w.into_iter()
            .filter(|(_, b, _, t)| from <= *b && *b <= to && self.tag_at(*b) == *t)
            .filter(|(_, b, _, _)| !(skip_missing && self.receipt_missing(*b)))
            .map(|(id, b, _, _)| {
                let mut l = withdrawal_log(id, b);
                l["blockHash"] = JsonValue::String(self.hash_at(b));
                l
            })
            .collect()
    }
    /// Whether block `b`'s receipt is missing for this read (counts the read down).
    fn receipt_missing(&self, b: u64) -> bool {
        match self.missing_receipt.get() {
            Some((at, n)) if at == b && n > 0 => {
                self.missing_receipt.set(Some((at, n - 1)));
                true
            }
            _ => false,
        }
    }
    fn err(code: i64, message: &str, data: JsonValue) -> JsonValue {
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": code, "message": message, "data": data } })
    }
    /// citrate_getLogs (eth_rpc.rs:1952-2026 with ExecutedChainIndex::executed_after,
    /// canonical_logs.rs:547-574).
    fn citrate_get_logs(&self, f: &JsonValue) -> Result<JsonValue, RpcError> {
        if !self.citrate_methods.get() {
            return Ok(Node::err(-32601, "Method not found", JsonValue::Null));
        }
        // eth_rpc.rs:1958: the method budget comes first.
        if self.budget_exceeded.get() > 0 {
            self.budget_exceeded.set(self.budget_exceeded.get() - 1);
            return Ok(Node::err(-32005, "Method budget exceeded. Reduce call frequency for expensive methods.", JsonValue::Null));
        }
        if let Some(Some(raw)) = self.getlogs_raw.borrow_mut().pop_front() {
            return Ok(raw);
        }
        std::thread::sleep(self.logs_delay.get());
        let (from, to) = (hex_q(&f["fromBlock"]), hex_q(&f["toBlock"]));
        if let Some(at) = self.fail_at.get() {
            if from <= at && at <= to {
                return Err(RpcError::Transport("rpc.citrate.ai timed out".into()));
            }
        }
        let after = f["afterBlockHash"].as_str();
        // eth_rpc.rs:1979-1993: afterBlockHash needs fromBlock >= 1.
        if after.is_some() && from == 0 {
            return Ok(Node::err(-32602, "citrate_getLogs: afterBlockHash needs fromBlock >= 1", JsonValue::Null));
        }
        // eth_rpc.rs:1994-1996 + canonical_logs.rs:127-146: over the limit is -32005, never clamped.
        if to.saturating_sub(from) > self.max_range.get() {
            return Ok(Node::err(-32005, "eth_getLogs range too large", serde_json::json!({ "maxBlockRange": self.max_range.get() })));
        }
        // canonical_logs.rs:554: ONE read of the applied tip.
        let tip = self.snapshot_tip(from, to);
        // canonical_logs.rs:555-563: a tip below the cursor serves nothing and judges nothing.
        let lo = if after.is_some() { from - 1 } else { from };
        if lo > tip {
            return Ok(ok(serde_json::json!({ "logs": [], "through": null })));
        }
        if self.warming.get() > 0 {
            self.warming.set(self.warming.get() - 1);
            return Ok(Node::err(-32002, "eth_getLogs: log index is warming; retry the same request", serde_json::json!({ "retryable": true })));
        }
        // canonical_logs.rs:566-571 + 660-673: the cursor must be the executed block at fromBlock-1.
        if let Some(a) = after {
            if !self.hash_at(lo).eq_ignore_ascii_case(a) {
                return Ok(Node::err(-32001, "citrate_getLogs: afterBlockHash is not the executed block", serde_json::json!({ "reorged": true, "height": format!("0x{lo:x}") })));
            }
        }
        // canonical_logs.rs:378-381 + 572: heights above the tip have not executed.
        let last = to.min(tip);
        if from > last {
            return Ok(ok(serde_json::json!({ "logs": [], "through": null })));
        }
        // eth_rpc.rs:3335-3345 (ReceiptPolicy::RequireAll): a missing receipt fails the range.
        if let Some((b, _)) = self.missing_receipt.get() {
            if from <= b && b <= last && self.receipt_missing(b) {
                return Ok(Node::err(-32002, "citrate_getLogs: receipts are not stored; retry the same request", serde_json::json!({ "retryable": true, "blockNumber": format!("0x{b:x}") })));
            }
        }
        // eth_rpc.rs:2019-2024: through = the last executed block served.
        let logs = self.logs_between(from, last, false);
        Ok(ok(serde_json::json!({ "logs": logs, "through": { "number": format!("0x{last:x}"), "hash": self.hash_at(last) } })))
    }
    /// eth_getLogs (eth_rpc.rs:1888-1944): the executed chain up to the tip, receipts that are
    /// missing skipped; today's node clamps a wide range instead of -32005.
    fn eth_get_logs(&self, f: &JsonValue) -> Result<JsonValue, RpcError> {
        if self.budget_exceeded.get() > 0 {
            self.budget_exceeded.set(self.budget_exceeded.get() - 1);
            return Ok(Node::err(-32005, "Method budget exceeded. Reduce call frequency for expensive methods.", JsonValue::Null));
        }
        std::thread::sleep(self.logs_delay.get());
        let from = hex_q(&f["fromBlock"]);
        let mut to = hex_q(&f["toBlock"]);
        if let Some(at) = self.fail_at.get() {
            if from <= at && at <= to {
                return Err(RpcError::Transport("rpc.citrate.ai timed out".into()));
            }
        }
        if to.saturating_sub(from) > self.max_range.get() {
            if self.clamp.get() {
                to = from + self.max_range.get();
            } else {
                return Ok(Node::err(-32005, "eth_getLogs range too large", serde_json::json!({ "maxBlockRange": self.max_range.get() })));
            }
        }
        if self.warming.get() > 0 {
            self.warming.set(self.warming.get() - 1);
            return Ok(Node::err(-32002, "eth_getLogs: log index is warming; retry the same request", serde_json::json!({ "retryable": true })));
        }
        let tip = self.snapshot_tip(from, to);
        Ok(ok(JsonValue::Array(self.logs_between(from, to.min(tip), true))))
    }
    /// citrate_isExecuted (canonical_logs.rs:503-525 + 585-631): executed iff the block is the
    /// executed block at its height under the tip; a malformed hash is -32602.
    fn is_executed(&self, params: &JsonValue) -> JsonValue {
        if !self.citrate_methods.get() {
            return Node::err(-32601, "Method not found", JsonValue::Null);
        }
        if let Some(Some(raw)) = self.is_executed_raw.borrow_mut().pop_front() {
            return raw;
        }
        let h = params[0].as_str().unwrap_or_default().to_ascii_lowercase();
        let digits = h.strip_prefix("0x").unwrap_or_default();
        if digits.len() != 64 || hex::decode(digits).is_err() {
            return Node::err(-32602, "citrate_isExecuted: block hash must be 32 bytes", JsonValue::Null);
        }
        let n = u64::from_str_radix(&digits[2..], 16).unwrap_or(u64::MAX);
        if n <= self.tip.get() && self.hash_at(n) == h {
            ok(serde_json::json!({ "executed": true, "number": format!("0x{n:x}") }))
        } else {
            ok(serde_json::json!({ "executed": false, "number": null }))
        }
    }
    fn one(&self, body: JsonValue) -> Result<JsonValue, RpcError> {
        match body["method"].as_str() {
            Some("eth_blockNumber") => Ok(ok(JsonValue::String(format!("0x{:x}", self.head.get())))),
            Some("citrate_getLogs") => self.citrate_get_logs(&body["params"][0]),
            Some("eth_getLogs") => self.eth_get_logs(&body["params"][0]),
            Some("citrate_isExecuted") => Ok(self.is_executed(&body["params"])),
            // canonical_logs.rs:714-728 (served so the code before this change can run here).
            Some("citrate_executedHead") if self.citrate_methods.get() => Ok(ok(serde_json::json!({
                "number": format!("0x{:x}", self.tip.get()), "hash": self.hash_at(self.tip.get()) }))),
            Some("citrate_executedHead") => Ok(Node::err(-32601, "Method not found", JsonValue::Null)),
            Some("eth_call") => {
                // LiquidStakingPool.withdrawals(id) at `latest`: an id with no request on this chain
                // reads the zero tuple (an unset mapping entry).
                let data = body["params"][0]["data"].as_str().unwrap_or_default();
                let id = u128::from_str_radix(&data[data.len().saturating_sub(32)..], 16).unwrap_or(0);
                let found = self.withdrawals.borrow().iter().copied().find(|(i, b, _, t)| {
                    *i == id && *b <= self.tip.get() && self.tag_at(*b) == *t
                });
                let ret = match found {
                    Some((_, b, claimed, _)) => withdrawal_tuple_hex(
                        STAKER_ADDR,
                        1,
                        2_000_000_000_000_000_000,
                        b,
                        claimed && !self.unclaim.borrow().contains(&id),
                    ),
                    None => vec![0u8; 160],
                };
                Ok(ok(JsonValue::String(format!("0x{}", hex::encode(ret)))))
            }
            _ => Err(RpcError::Transport("unexpected method".into())),
        }
    }
}

impl RpcTransport for Node {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.requests.borrow_mut().push(body.clone());
        if let Some(batch) = body.as_array() {
            std::thread::sleep(self.batch_delay.get());
            let mut out = Vec::new();
            for b in batch {
                let mut r = self.one(b.clone())?;
                r["id"] = b["id"].clone();
                out.push(r);
            }
            return Ok(JsonValue::Array(out));
        }
        self.one(body)
    }
}

fn scan_file(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("c1w-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("withdrawal-scans.json");
    (dir, file)
}

fn read_all(rpc: &RpcClient<Node>, scan: &mut WithdrawalScan) -> Vec<PendingWithdrawal> {
    read_pending_withdrawals_with(rpc, STAKER_ADDR, scan, None).unwrap_or_default()
}

// executed_head.rs:1081 cursor_scan_never_skips_logs_across_a_rewind ("Codex's sequence for one
// call"): the tip is at 4480 while the window covering 4500 is served and back at 5000 right after.
// That answer is complete only through 4480, so the next call resumes at 4481 and finds 4500.
#[test]
fn a_rewind_and_return_within_one_call_never_skips_a_withdrawal() {
    let node = Node::new(5_000, vec![(1, 4_500, false)]);
    node.transient.set(Some((4_500, 4_480)));
    let rpc = RpcClient::with_transport(node);
    let mut scan = WithdrawalScan::default();
    assert!(read_all(&rpc, &mut scan).is_empty(), "4500 was not served in this call");
    assert_eq!(scan.cursor(), Some((4_480, stub_hash(4_480))), "the cursor is what the answer was complete through");
    assert!(!scan.last_read_confirmed, "served to 4480 of 5000");
    let n = rpc.transport().log_reads().len();
    let rows = read_all(&rpc, &mut scan);
    assert_eq!(ids_of(&rows), vec!["1"], "found on the next call");
    let reads = rpc.transport().log_reads();
    assert_eq!((reads[n].1, reads[n].3.clone()), (4_481, Some(stub_hash(4_480))), "resumes after the cursor, chained on it");
    assert_eq!(scan.cursor(), Some((5_000, stub_hash(5_000))));
    assert!(scan.last_read_confirmed);
}

// executed_head.rs:1125 cursor_scan_restarts_on_a_reorg_below_the_cursor: the chain the scan read
// is replaced above 3000. The next window is -32001; the scan drops the cursor and the rows found
// above the newest saved cursor citrate_isExecuted still confirms, and reads on from there.
#[test]
fn a_reorg_below_the_cursor_resumes_from_a_saved_cursor_citrate_is_executed_confirms() {
    let node = Node::new(6_000, vec![(1, 2_000, false), (2, 4_200, false)]);
    node.withdrawals.borrow_mut().push((3, 3_500, false, 0xB));
    let rpc = RpcClient::with_transport(node);
    let mut scan = WithdrawalScan::default();
    assert_eq!(ids_of(&read_all(&rpc, &mut scan)), vec!["1", "2"]);
    rpc.transport().fork.set(Some((3_000, 0xB)));
    rpc.transport().grow(7_000);
    let (n, n_req) = (rpc.transport().log_reads().len(), rpc.transport().requests.borrow().len());
    let rows = read_all(&rpc, &mut scan);
    assert_eq!(ids_of(&rows), vec!["1", "3"], "the new branch's withdrawal, not the abandoned one");
    let reads = rpc.transport().log_reads();
    assert_eq!((reads[n].1, reads[n].3.clone()), (6_001, Some(stub_hash(6_000))), "the window chained on the old cursor");
    let (resume, after) = (reads[n + 1].1, reads[n + 1].3.clone());
    assert!(resume > LIQUID_STAKING_POOL_DEPLOY_BLOCK + 1 && resume <= 3_001, "resumed at {resume}: a saved cursor below the fork");
    assert_eq!(after, Some(stub_hash(resume - 1)), "chained on that saved cursor");
    assert!(rpc.transport().methods()[n_req..].iter().any(|m| m == "citrate_isExecuted"), "the saved cursor was checked");
    assert!(!rpc.transport().asked_ids(n_req).contains(&2), "rows found above the saved cursor are dropped");
    assert_eq!(scan.cursor(), Some((7_000, branch_hash(0xB, 7_000))));
    assert_eq!(scan.generation, 1);
    assert!(scan.last_read_confirmed);
}

// executed_head.rs:1156 cursor_scan_resumes_across_a_node_restart: the saved cursor survives an
// app relaunch, and a node that comes back with its tip below the cursor serves nothing and judges
// nothing (canonical_logs.rs:555-563): the cursor is kept, and the scan resumes after it.
#[test]
fn a_restart_with_the_tip_below_the_cursor_keeps_the_cursor() {
    let (dir, file) = scan_file("restart");
    let rpc = RpcClient::with_transport(Node::new(5_000, vec![]));
    let mut scan = load_scan(&file, "k");
    read_all(&rpc, &mut scan);
    assert_eq!(scan.cursor(), Some((5_000, stub_hash(5_000))));
    save_scan(&file, "k", &scan).expect("save");
    // Replay after an unclean shutdown: the node is back at 4000 on the same chain.
    let node = Node::new(5_000, vec![(9, 5_500, false)]);
    node.tip.set(4_000);
    let rpc = RpcClient::with_transport(node);
    let mut scan = load_scan(&file, "k");
    assert!(read_all(&rpc, &mut scan).is_empty());
    assert_eq!(scan.cursor(), Some((5_000, stub_hash(5_000))), "nothing served, nothing judged: the cursor is kept");
    assert_eq!(scan.generation, 0, "no rewind");
    assert!(!scan.last_read_confirmed);
    save_scan(&file, "k", &scan).expect("save");
    rpc.transport().grow(6_000);
    let mut scan = load_scan(&file, "k");
    let n = rpc.transport().log_reads().len();
    let rows = read_all(&rpc, &mut scan);
    assert_eq!(rpc.transport().log_reads()[n].1, 5_001, "resumes after the saved cursor");
    assert_eq!(ids_of(&rows), vec!["9"]);
    assert!(scan.last_read_confirmed);
    let _ = std::fs::remove_dir_all(&dir);
}

// executed_head.rs:1198 get_logs_refuses_a_range_with_missing_receipts: the producer writes
// receipts just after it commits the tip, so a fresh block can be executed without its receipt.
// citrate_getLogs answers -32002 retryable; the same window is read again and never skipped.
#[test]
fn missing_receipts_are_retried_and_never_skipped() {
    let node = Node::new(5_000, vec![(4, 4_500, false)]);
    node.missing_receipt.set(Some((4_500, 2)));
    let rpc = RpcClient::with_transport(node);
    let mut scan = WithdrawalScan::default();
    let rows = read_all(&rpc, &mut scan);
    assert_eq!(ids_of(&rows), vec!["4"], "the withdrawal in the block without a receipt is not skipped");
    let covering: Vec<_> = rpc.transport().log_reads().into_iter().filter(|r| r.1 <= 4_500 && 4_500 <= r.2).collect();
    assert_eq!(covering.len(), 3, "two -32002 answers, then the data");
    assert!(covering.windows(2).all(|w| w[0] == w[1]), "the SAME window: {covering:?}");
    assert!(scan.last_read_confirmed);
}

// A receipt lost in a crash keeps its block at -32002 (citrate-chain core/api/README.md, "Remaining
// assumptions"): after the bounded retries the read is "still indexing", and the cursor stays
// below that block.
#[test]
fn a_receipt_that_never_arrives_stops_the_scan_below_it() {
    let node = Node::new(5_000, vec![(4, 4_500, false)]);
    node.missing_receipt.set(Some((4_500, u32::MAX)));
    let rpc = RpcClient::with_transport(node);
    let mut scan = WithdrawalScan::default();
    let err = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None);
    assert!(matches!(err, Err(StakingError::Indexing(_))), "{err:?}");
    assert_eq!(scan.cursor(), Some((4_426, stub_hash(4_426))), "the windows before it are kept, none past it");
}

// canonical_logs.rs:127-146: a range over the node's limit is -32005 with data.maxBlockRange; the
// window halves, and the windows stay contiguous and chained.
#[test]
fn a_range_limit_halves_the_window_and_keeps_windows_chained() {
    let node = Node::new(10_000, vec![(4, 3_333, false), (5, 9_999, false)]);
    node.max_range.set(300);
    let rpc = RpcClient::with_transport(node);
    let mut scan = WithdrawalScan::default();
    assert_eq!(ids_of(&read_all(&rpc, &mut scan)), vec!["4", "5"]);
    let reads = rpc.transport().log_reads();
    assert!(reads.iter().all(|r| r.0 == "citrate_getLogs"), "{reads:?}");
    let served: Vec<_> = reads.iter().filter(|r| r.2 - r.1 <= 300).collect();
    for w in served.windows(2) {
        assert_eq!(w[1].1, w[0].2 + 1, "contiguous");
        assert_eq!(w[1].3, Some(stub_hash(w[0].2)), "chained on the previous through");
    }
    assert_eq!(scan.cursor(), Some((10_000, stub_hash(10_000))));
}

// rate_limit.rs:96-101: the method budget also answers -32005, with no data. A smaller window does
// not help, so it is an error: stop, commit nothing.
#[test]
fn a_method_budget_answer_is_not_a_range_limit() {
    let node = Node::new(5_000, vec![(4, 600, false)]);
    node.budget_exceeded.set(1);
    let rpc = RpcClient::with_transport(node);
    let mut scan = WithdrawalScan::default();
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None);
    assert!(matches!(got, Err(StakingError::Rpc(ref m)) if m.contains("budget")), "{got:?}");
    assert_eq!(rpc.transport().log_reads().len(), 1, "no halving");
    assert_eq!(scan.cursor(), None);
}

// Any other answer stops the read and commits nothing of that window: an unknown error, a -32001
// that does not say reorged, a -32002 that does not say retryable, and every malformed answer
// (a through outside the window, a malformed hash, a log past the through, logs without one).
#[test]
fn an_unknown_error_or_a_malformed_answer_stops_and_commits_nothing() {
    let res = |r: JsonValue| ok(r);
    let through = |n: u64, h: String| serde_json::json!({ "number": format!("0x{n:x}"), "hash": h });
    let cases: Vec<(&str, JsonValue)> = vec![
        ("-32603", Node::err(-32603, "citrate_getLogs: no applied tip", JsonValue::Null)),
        ("-32602", Node::err(-32602, "invalid params", JsonValue::Null)),
        ("-32001 without data.reorged", Node::err(-32001, "not found", JsonValue::Null)),
        ("-32002 not retryable", Node::err(-32002, "busy", JsonValue::Null)),
        ("through past toBlock", res(serde_json::json!({ "logs": [], "through": through(2_427, stub_hash(2_427)) }))),
        ("through before fromBlock", res(serde_json::json!({ "logs": [], "through": through(1_426, stub_hash(1_426)) }))),
        ("malformed through hash", res(serde_json::json!({ "logs": [], "through": through(2_000, "0x12".into()) }))),
        ("a log past through", res(serde_json::json!({ "logs": [withdrawal_log(5, 2_100)], "through": through(2_000, stub_hash(2_000)) }))),
        ("a log before fromBlock", res(serde_json::json!({ "logs": [withdrawal_log(5, 1_000)], "through": through(2_000, stub_hash(2_000)) }))),
        ("logs without a through", res(serde_json::json!({ "logs": [withdrawal_log(5, 1_500)], "through": null }))),
        ("no through", res(serde_json::json!({ "logs": [] }))),
    ];
    for (name, raw) in cases {
        let node = Node::new(5_000, vec![(5, 2_100, false)]);
        node.getlogs_raw.borrow_mut().extend([None, Some(raw)]);
        let rpc = RpcClient::with_transport(node);
        let mut scan = WithdrawalScan::default();
        let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None);
        assert!(got.is_err(), "{name}: {got:?}");
        assert_eq!(scan.cursor(), Some((1_426, stub_hash(1_426))), "{name}: the window before is kept, nothing of this one");
        assert_eq!(rpc.transport().log_reads().len(), 2, "{name}: stopped");
    }
}

// Today's public RPC has no citrate_getLogs (-32601): the plain eth_getLogs read still shows the
// rows it finds, commits nothing, and says it can't confirm. It never reads citrate_executedHead,
// eth_syncing or citrate_isExecuted.
#[test]
fn without_citrate_get_logs_the_plain_read_commits_nothing_and_cannot_confirm() {
    let rpc = RpcClient::with_transport(Node::today(30_000, vec![(1, 900, false), (2, 29_000, false)]));
    let mut scan = WithdrawalScan::default();
    let rows = read_all(&rpc, &mut scan);
    assert_eq!(ids_of(&rows), vec!["1", "2"], "rows are shown");
    assert_eq!(scan.cursor(), None, "nothing committed");
    assert!(scan.last_read_complete && !scan.last_read_confirmed);
    let note = scan.last_read_note.clone().unwrap_or_default();
    assert!(note.contains("citrate_getLogs"), "{note}");
    let methods = rpc.transport().methods();
    assert_eq!(methods.get(1).map(String::as_str), Some("citrate_getLogs"), "{methods:?}");
    for m in ["citrate_executedHead", "eth_syncing", "citrate_isExecuted"] {
        assert!(!methods.iter().any(|x| x == m), "{m} read: {methods:?}");
    }
    let plain: Vec<_> = rpc.transport().log_reads().into_iter().filter(|r| r.0 == "eth_getLogs").collect();
    assert_eq!(plain.first().map(|r| r.1), Some(LIQUID_STAKING_POOL_DEPLOY_BLOCK));
    assert_eq!(plain.last().map(|r| r.2), Some(30_000));
    assert!(plain.windows(2).all(|w| w[1].1 == w[0].2 + 1), "contiguous");
}

// README "Log scanner protocol" rule 5: persist nothing derived from citrate_executedHead,
// eth_blockNumber or eth_syncing. Stored head 9000, executed tip 5000: the cursor is exactly the
// served through, and the read is not confirmed.
#[test]
fn the_cursor_is_the_served_through_never_a_head_read() {
    let node = Node::new(5_000, vec![]);
    node.head.set(9_000);
    let rpc = RpcClient::with_transport(node);
    let mut scan = WithdrawalScan::default();
    read_all(&rpc, &mut scan);
    assert_eq!(scan.cursor(), Some((5_000, stub_hash(5_000))));
    assert!(scan.last_read_complete && !scan.last_read_confirmed);
    let note = scan.last_read_note.clone().unwrap_or_default();
    assert!(note.contains("5000") && note.contains("9000"), "{note}");
    assert!(!rpc.transport().methods().iter().any(|m| m == "citrate_executedHead"));
}

// A scan file written by an earlier build of this PR holds a position from the withdrawn
// two-read protocol, which can skip logs across a rewind (citrate-chain#337 at cfba7b9). It still
// loads (and leaves other keys alone), but its position is not trusted: the scan starts at 427.
#[test]
fn a_scan_file_from_an_earlier_build_loads_and_starts_at_the_deployment_block() {
    let (dir, file) = scan_file("legacy");
    std::fs::create_dir_all(&dir).expect("dir");
    let legacy = serde_json::json!({ "next_block": 40_001, "ids": [9], "claimed": { "4": 39_000 },
        "anchor": [40_000, stub_hash(40_000)], "generation": 2 });
    std::fs::write(&file, serde_json::json!({ "k": legacy, "other": legacy }).to_string()).expect("write");
    let mut scan = load_scan(&file, "k");
    assert_eq!(scan.cursor(), None);
    let rpc = RpcClient::with_transport(Node::new(50_000, vec![(9, 45_000, false)]));
    let rows = read_all(&rpc, &mut scan);
    let first = rpc.transport().log_reads()[0].clone();
    assert_eq!((first.1, first.3), (LIQUID_STAKING_POOL_DEPLOY_BLOCK, None), "from the deployment block, unchained");
    assert_eq!(ids_of(&rows), vec!["9"]);
    save_scan(&file, "k", &scan).expect("save");
    assert_eq!(load_scan(&file, "k").cursor(), Some((50_000, stub_hash(50_000))));
    let all: serde_json::Map<String, JsonValue> = serde_json::from_slice(&std::fs::read(&file).expect("read")).expect("json");
    assert!(all.contains_key("other"), "other keys are kept");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// STD tripwire (C1-W, citrate-chain#337 @ 600d2984). The bug class: scan progress that is not a
// served `through`, e.g. a position from citrate_executedHead, eth_blockNumber or eth_syncing, or a
// fixed margin below one of them. Two head reads can't show how far one getLogs call served across
// a rewind (the cfba7b9 blocker on #337). Structural checks over the production source:
//  1. no head read or margin feeds the scan (executed_head, .syncing(), commit_cap, the margins);
//  2. the scan reads citrate_getLogs and chains windows with afterBlockHash;
//  3. every cursor write is a served `through`, a saved cursor citrate_isExecuted confirmed
//     (`verified`), or a drop, and never mentions a head;
//  4. the plain eth_getLogs read is one function that touches no saved state.
// ---------------------------------------------------------------------------
fn served_through_violations(prod: &str) -> Vec<String> {
    let mut bad = Vec::new();
    for banned in ["executed_head", ".syncing()", "commit_cap", "EXECUTED_HEAD_MARGIN_BLOCKS", "SAFE_DEPTH_BLOCKS"] {
        if prod.contains(banned) {
            bad.push(format!("scan progress from a head read or a margin: {banned}"));
        }
    }
    if !prod.contains("rpc.citrate_get_logs(") {
        bad.push("the scan does not read citrate_getLogs".into());
    }
    if !prod.contains("\"afterBlockHash\"") {
        bad.push("windows are not chained with afterBlockHash".into());
    }
    for stmt in prod.split(';') {
        let mut rest = stmt;
        while let Some(i) = rest.find("cursor =") {
            let after = &rest[i + "cursor =".len()..];
            if !after.starts_with('=') {
                let rhs = after.trim();
                let from_served = rhs.starts_with("Some(through") || rhs.starts_with("Some(verified") || rhs == "None";
                let names_a_head = ["current_block", "block_number", "head"].iter().any(|h| stmt.contains(h));
                if !from_served || names_a_head {
                    bad.push(format!("a cursor that is not a served through: {}", stmt.trim()));
                }
            }
            rest = after;
        }
    }
    match prod.find("fn read_unbounded") {
        None => bad.push("no separate unbounded (eth_getLogs) read".into()),
        Some(i) => {
            let body = &prod[i..];
            let end = body[1..].find("\nfn ").or_else(|| body[1..].find("\npub fn ")).map_or(body.len(), |e| e + 1);
            let body = &body[..end];
            if body.contains("scan.") {
                bad.push("the unbounded read touches saved scan state".into());
            }
        }
    }
    if prod.matches("get_logs_range(").count() != 1 {
        bad.push("eth_getLogs is read outside the unbounded fallback".into());
    }
    bad
}

fn production_source() -> String {
    let src = include_str!("staking.rs");
    let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap_or(src);
    // Doc comments may name the methods; only code counts.
    prod.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

#[test]
fn tripwire_scan_progress_is_only_a_served_through() {
    let bad = served_through_violations(&production_source());
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn tripwire_catches_progress_from_a_head_read() {
    // Negative control: the shapes of the withdrawn protocol and of a head-derived cursor.
    let shapes = [
        ("let (h2, hash2) = rpc.executed_head().ok().flatten()?;", "executed_head"),
        ("let commit_cap = Some(n.saturating_sub(EXECUTED_HEAD_MARGIN_BLOCKS));", "commit_cap"),
        ("scan.cursor = Some((current_block, hash));", "not a served through"),
        ("scan.cursor = Some(through_or(head));", "not a served through"),
        ("fn read_unbounded() { scan.rows.insert(1, 2); rpc.get_logs_range(f) }", "touches saved scan state"),
        ("let a = rpc.get_logs_range(f); let b = rpc.get_logs_range(g);", "outside the unbounded fallback"),
    ];
    for (src, want) in shapes {
        let bad = served_through_violations(src);
        assert!(bad.iter().any(|b| b.contains(want)), "{want}: {bad:?}");
    }
}
