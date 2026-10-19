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
    /// Hashes `eth_getBlockByHash` no longer serves (the block left the executed chain).
    unknown_hashes: RefCell<std::collections::HashSet<String>>,
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
            unknown_hashes: RefCell::new(Default::default()),
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
            Some("eth_getBlockByHash") => {
                let h = body["params"][0].as_str().unwrap_or_default().to_ascii_lowercase();
                if self.unknown_hashes.borrow().contains(&h) {
                    return Ok(ok(JsonValue::Null));
                }
                let n = self
                    .reorged
                    .borrow()
                    .iter()
                    .find(|(_, v)| v.eq_ignore_ascii_case(&h))
                    .map(|(n, _)| *n)
                    .or_else(|| u64::from_str_radix(h.trim_start_matches("0x"), 16).ok());
                match n {
                    Some(n) if n <= self.head => Ok(ok(serde_json::json!({ "number": format!("0x{n:x}"), "hash": h }))),
                    _ => Ok(ok(JsonValue::Null)),
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
        // The paging stops H2_RESERVE before the deadline, so give it a little more than that.
        let deadline = std::time::Instant::now() + H2_RESERVE + std::time::Duration::from_millis(50);
        match read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(deadline)) {
            Ok(p) => {
                done = Some(p);
                break;
            }
            Err(StakingError::Incomplete { .. }) => continue,
            Err(e) => panic!("{e}"),
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
        claimed: [4u128].into_iter().collect(),
        anchor: Some((40_000, stub_hash(40_000))),
        generation: 2,
        ..Default::default()
    };
    save_scan(&file, "k", &scan).expect("save");
    assert_eq!(load_scan(&file, "k"), scan);
    assert_eq!(load_scan(&file, "other"), WithdrawalScan::default());
    std::fs::write(&file, "not json").expect("write");
    assert_eq!(load_scan(&file, "k"), WithdrawalScan::default(), "a corrupt file starts over");
    let _ = std::fs::remove_dir_all(&dir);
}

// #371 review: eth_blockNumber (stored height) can be ahead of what eth_getLogs serves (applied
// tip). A position committed past the applied tip, then saved, would skip those blocks forever.
// The committed position stays SAFE_DEPTH_BLOCKS behind the head, so after a relaunch the trailing
// blocks are read again and the withdrawal is found.
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
    assert!(scan.next_block <= 5_001 - SAFE_DEPTH_BLOCKS, "{}", scan.next_block);
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

// Codex P2: a claimed id is a tombstone. Merging an older scan that still lists it, and reading
// again, never brings it back, and it is not read from the chain again.
#[test]
fn a_claimed_id_is_never_resurrected_by_a_merge() {
    let rpc = RpcClient::with_transport(ChainStub::new(3_000, vec![(30, 1_000, true), (31, 1_100, false)]));
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert!(scan.claimed.contains(&30) && !scan.ids.contains(&30));
    let older = WithdrawalScan { next_block: 500, ids: [30u128, 31].into_iter().collect(), ..Default::default() };
    let merged = merge_scans(&older, &scan);
    assert!(!merged.ids.contains(&30), "claimed id resurrected");
    assert!(merged.ids.contains(&31));
    let mut merged = merged;
    let calls_before = rpc.transport().requests.borrow().len();
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut merged, None).expect("read");
    assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["31"]);
    let reqs = rpc.transport().requests.borrow()[calls_before..].to_vec();
    let asked_30 = reqs.iter().filter(|r| r.is_array()).flat_map(|r| r.as_array().cloned().unwrap_or_default())
        .any(|c| c["params"][0]["data"].as_str().is_some_and(|d| d.ends_with(&format!("{:032x}", 30))));
    assert!(!asked_30, "a claimed id is not read again");
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

// A node without citrate_executedHead: the conservative depth, and never past it.
#[test]
fn without_executed_head_the_conservative_depth_holds() {
    let rpc = RpcClient::with_transport(ChainStub::new(8_000, vec![]));
    // An older node that reports its applied tip (it is behind by nothing).
    *rpc.transport().syncing.borrow_mut() = Some(serde_json::json!({ "startingBlock": "0x1", "currentBlock": "0x1f40", "highestBlock": "0x1f41" }));
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(scan.next_block, 8_001 - SAFE_DEPTH_BLOCKS);
    let asked = rpc.transport().requests.borrow().iter().any(|r| r["method"] == "citrate_executedHead");
    assert!(asked, "the executed head is asked for first");
}

// On resume the hash at the committed position is checked. A reorg there rewinds one window, and a
// withdrawal that the new branch carries in that window is found.
#[test]
fn a_reorg_at_the_committed_position_rescans_the_window() {
    let rpc = RpcClient::with_transport(ChainStub::new(9_000, vec![(40, 5_990, false)]));
    *rpc.transport().executed_head.borrow_mut() = Some(9_000);
    let mut scan = WithdrawalScan {
        next_block: 6_001,
        anchor: Some((6_000, stub_hash(6_000))),
        ..Default::default()
    };
    rpc.transport().reorged.borrow_mut().insert(6_000, format!("0x{}", "ee".repeat(32)));
    // The anchored block left the executed chain: eth_getBlockByHash no longer serves it.
    rpc.transport().unknown_hashes.borrow_mut().insert(stub_hash(6_000));
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["40"]);
    let first = rpc.transport().log_ranges()[0];
    assert_eq!(first.0, 6_001 - LOG_WINDOW_BLOCKS, "rewound one window");
    assert_eq!(scan.generation, 1);
    // Unchanged hash: no rewind.
    let rpc2 = RpcClient::with_transport(ChainStub::new(9_000, vec![]));
    *rpc2.transport().executed_head.borrow_mut() = Some(9_000);
    let mut same = WithdrawalScan { next_block: 6_001, anchor: Some((6_000, stub_hash(6_000))), ..Default::default() };
    read_pending_withdrawals_with(&rpc2, STAKER_ADDR, &mut same, None).expect("read");
    assert_eq!(rpc2.transport().log_ranges()[0].0, 6_001);
    assert_eq!(same.generation, 0);
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

// A reorg is detected by HASH (eth_getBlockByHash of the anchored executed head), never by
// eth_getBlockByNumber, which can name a sibling at that height.
#[test]
fn a_reorg_is_detected_by_hash_not_by_number() {
    let rpc = RpcClient::with_transport(ChainStub::new(9_000, vec![(41, 5_500, false)]));
    *rpc.transport().executed_head.borrow_mut() = Some(9_000);
    let gone = stub_hash(6_000);
    // The height index still names the same hash at 6000, but the block left the executed chain.
    rpc.transport().unknown_hashes.borrow_mut().insert(gone.clone());
    let mut scan = WithdrawalScan { next_block: 6_001, anchor: Some((6_000, gone)), ..Default::default() };
    let got = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("read");
    assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["41"], "rewound and re-read");
    assert_eq!(scan.generation, 1);
    let by_number = rpc.transport().requests.borrow().iter().any(|r| r["method"] == "eth_getBlockByNumber");
    assert!(!by_number, "the reorg check never uses eth_getBlockByNumber");
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

// DGX probe B: fall back to eth_syncing ONLY on -32601 (method not found). A timeout, -32603, any
// other error code or a malformed result is no evidence, even when eth_syncing would report a tip.
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
    // -32601 (an older node) with an eth_syncing applied tip IS evidence.
    let stub = ChainStub::new(9_000, vec![]);
    *stub.syncing.borrow_mut() = Some(behind);
    let rpc = RpcClient::with_transport(stub);
    let mut scan = WithdrawalScan::default();
    read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, None).expect("fallback");
    assert!(scan.next_block > 0 && scan.next_block <= 8_001 - EXECUTED_HEAD_MARGIN_BLOCKS, "{}", scan.next_block);
    assert!(scan.last_read_confirmed);
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

// With no evidence, a budget stop cannot keep progress, so it says "can't confirm" instead of
// "still reading" for ever (DGX: until chain#334/#336/#337 are on the public RPC).
#[test]
fn a_budget_stop_without_evidence_is_unconfirmed() {
    let rpc = RpcClient::with_transport(ChainStub::new(50_000_000, vec![]));
    let mut scan = WithdrawalScan::default();
    let deadline = std::time::Instant::now() + H2_RESERVE + std::time::Duration::from_millis(30);
    let err = read_pending_withdrawals_with(&rpc, STAKER_ADDR, &mut scan, Some(deadline)).expect_err("unconfirmed");
    assert!(matches!(err, StakingError::Unconfirmed), "{err:?}");
    assert!(err.to_string().contains("can't confirm"), "{err}");
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

// A claim settles once the committed position (the margin below the executed head) has passed the
// head it was seen at; until then it is read again every time. A rewind unsettles it.
#[test]
fn a_claim_is_reread_until_settled_and_again_after_a_rewind() {
    let at = |head: u64| {
        let rpc = RpcClient::with_transport(ChainStub::new(head, vec![(7, 1_000, true)]));
        *rpc.transport().executed_head.borrow_mut() = Some(head);
        rpc
    };
    let mut scan = WithdrawalScan::default();
    let r = at(5_000);
    read_pending_withdrawals_with(&r, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    let n = r.transport().requests.borrow().len();
    read_pending_withdrawals_with(&r, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert!(r.transport().asked_ids(n).contains(&7), "an unsettled claim is read again");
    // The executed head moves 300 blocks on: the claim seen at 5000 is now margin-deep.
    let r2 = at(5_300);
    read_pending_withdrawals_with(&r2, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    let n2 = r2.transport().requests.borrow().len();
    read_pending_withdrawals_with(&r2, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert!(!r2.transport().asked_ids(n2).contains(&7), "a settled claim is not read again");
    // A reorg at the anchor (citrate_isExecuted says false) rewinds and unsettles it.
    let r3 = at(5_300);
    let anchor = scan.anchor.clone().map(|a| a.0).unwrap_or(5_200);
    r3.transport().reorged.borrow_mut().insert(anchor, format!("0x{}", "ee".repeat(32)));
    read_pending_withdrawals_with(&r3, STAKER_ADDR, &mut scan, None).unwrap_or_default();
    assert!(r3.transport().asked_ids(0).contains(&7), "a rewind makes the claim unsettled again");
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
    let fresh = RpcClient::with_transport(ChainStub::new(30_000, vec![]));
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
    assert_eq!(SAFE_DEPTH_BLOCKS, 1000, "the chain's settled depth");
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
                let ok = stmt.contains("commit_cap") || stmt.contains("saturating_sub(LOG_WINDOW_BLOCKS)");
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
            let (exec_arm, fallback) = def.split_once("Ok(None) =>").unwrap_or((def, ""));
            if exec_arm.contains("SAFE_DEPTH_BLOCKS") {
                bad.push("the executed-head arm falls back to a fixed depth".into());
            }
            if !exec_arm.contains("EXECUTED_HEAD_MARGIN_BLOCKS") {
                bad.push("the executed-head arm commits without the reorg margin".into());
            }
            if !fallback.contains("syncing") {
                bad.push("the fallback is not bounded by the eth_syncing applied tip".into());
            }
            if !fallback.contains("Err(_) => None") {
                bad.push("an executed-head error advances the position".into());
            }
            // DGX probe A: eth_syncing `false` is not evidence.
            if !fallback.contains("Ok(None) | Err(_) => None") {
                bad.push("eth_syncing false is taken as evidence".into());
            }
        }
    }
    if prod.matches("rpc.executed_head()").count() < 2 {
        bad.push("citrate_executedHead is not read twice (h1 before the logs, h2 after)".into());
    }
    // The reorg check is by hash: a block-by-number read can name a sibling.
    if prod.contains(".block_hash(") || prod.contains("eth_getBlockByNumber") {
        bad.push("a reorg or anchor check reads a block by number".into());
    }
    // getLogs never reads past the executed head: the window end is `scan_end`.
    if !prod.contains(".min(scan_end)") {
        bad.push("the log windows are not capped at the executed head".into());
    }
    let uses = prod.matches("SAFE_DEPTH_BLOCKS").count();
    let defined = prod.matches("pub const SAFE_DEPTH_BLOCKS").count();
    if uses - defined != 1 {
        bad.push(format!("SAFE_DEPTH_BLOCKS used {} times outside its definition", uses - defined));
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
    // Negative control: the shape of the c38974a bug fails the tripwire.
    let old = "let x = rpc.executed_head();\n\
               let a = rpc.block_hash(h);\n\
               let commit_cap = match e { Ok(Some(n)) => n.min(head - SAFE_DEPTH_BLOCKS), Ok(None) => match rpc.syncing() { _ => 0 } };\n\
               scan.next_block = (head + 1).saturating_sub(SAFE_DEPTH_BLOCKS);\n";
    let bad = scan_position_violations(old);
    assert!(bad.iter().any(|b| b.contains("unbounded scan-position write")), "{bad:?}");
    assert!(bad.iter().any(|b| b.contains("fixed depth")), "{bad:?}");
    assert!(bad.iter().any(|b| b.contains("reorg margin")), "{bad:?}");
    assert!(bad.iter().any(|b| b.contains("error advances")), "{bad:?}");
    assert!(bad.iter().any(|b| b.contains("syncing false")), "{bad:?}");
    assert!(bad.iter().any(|b| b.contains("not capped at the executed head")), "{bad:?}");
    assert!(bad.iter().any(|b| b.contains("read twice")), "{bad:?}");
    assert!(bad.iter().any(|b| b.contains("by number")), "{bad:?}");
}
