// BC-1.3 — the REAL on-chain grant-status read that S5 (grant + stake ceremony)
// settles from. @rule8 · T1 money-path surface (it decides whether the 32,000-SALT
// membership grant is genuinely on-chain).
//
// Load-bearing properties, all grounded (Rule 1 — no fabricated settlement):
//   * The pinned selectors are the REAL keccak of their signatures (Rule 11 drift
//     tripwires) — a drift would read the wrong function and mis-settle S5.
//   * `read_grant_status` reads attributedStake(member) + attributedShares(member)
//     on the MembershipStakeVault and balanceOf(member) on the CitrateMemberSBT,
//     decoding the real uint256s. A granted member reads >= the requirement + sbt=1;
//     a fresh/never-granted member honestly reads 0 / 0 / false.
//   * A value beyond u128 is REFUSED (never truncated).
//
// The mock RPC transport mirrors staking_tests / earnings_tests (a TEST transport,
// never the default — Rule 1).
use super::*;
use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// A scripted mock RPC transport (mirrors staking_tests).
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

/// A 32-byte `uint256` ABI word for `n`, as a `0x`-hex string.
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

const MEMBER_ADDR: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";

// 32,000 SALT in wei — the validator stake requirement a granted member must meet.
const REQUIREMENT_WEI: u128 = 32_000u128 * 1_000_000_000_000_000_000u128;

// ===========================================================================
// Canonical address + selector pins (Rule 11 tripwires)
// ===========================================================================

/// The vault address MUST be the canonical 40204.json value. A paste error here
/// would read grant state from the wrong contract (@rule8 — a mis-settled S5).
#[test]
fn membership_stake_vault_is_the_canonical_40204_value() {
    assert_eq!(
        MEMBERSHIP_STAKE_VAULT,
        "0x0aceb7b474ecc4abe12696ce48628f0cabe0267e"
    );
}

/// The SBT address MUST be the canonical 40204.json value.
#[test]
fn citrate_member_sbt_is_the_canonical_40204_value() {
    assert_eq!(
        CITRATE_MEMBER_SBT,
        "0x7be005aa8c45c1695b4c75468c6ca8b40238a7c4"
    );
}

/// The pinned `attributedStake(address)` selector is the REAL keccak of the
/// signature (Rule 11 drift tripwire).
#[test]
fn attributed_stake_selector_is_keccak_of_signature() {
    assert_eq!(
        attributed_stake_selector(),
        derive_selector("attributedStake(address)")
    );
}

/// The pinned `attributedShares(address)` selector is the REAL keccak of the
/// signature (Rule 11 drift tripwire).
#[test]
fn attributed_shares_selector_is_keccak_of_signature() {
    assert_eq!(
        attributed_shares_selector(),
        derive_selector("attributedShares(address)")
    );
}

/// The pinned `VALIDATOR_STAKE_REQUIREMENT()` selector is the REAL keccak of the
/// signature (Rule 11 drift tripwire).
#[test]
fn validator_stake_requirement_selector_is_keccak_of_signature() {
    assert_eq!(
        validator_stake_requirement_selector(),
        derive_selector("VALIDATOR_STAKE_REQUIREMENT()")
    );
}

/// The pinned `balanceOf(address)` selector (reused for the SBT read) is the REAL
/// keccak of the signature AND the canonical ERC value 0x70a08231.
#[test]
fn sbt_balance_of_selector_is_keccak_of_signature() {
    assert_eq!(
        sbt_balance_of_selector(),
        derive_selector("balanceOf(address)")
    );
    assert_eq!(
        format!("0x{}", hex::encode(sbt_balance_of_selector())),
        "0x70a08231"
    );
}

// ===========================================================================
// Address validation + calldata targeting
// ===========================================================================

#[test]
fn validate_address_canonicalizes_and_rejects_malformed() {
    assert_eq!(
        validate_address("0x9858EFFD232B4033E47D90003D41EC34ECAEDA94").unwrap(),
        MEMBER_ADDR,
        "canonical lowercased 0x form"
    );
    assert!(validate_address("0x1234").is_err(), "too short");
    assert!(validate_address("notanaddr").is_err(), "no 0x / not hex");
}

#[test]
fn attributed_stake_call_targets_the_vault_with_the_member_in_calldata() {
    let call = attributed_stake_call(MEMBER_ADDR);
    assert_eq!(
        call["to"].as_str().unwrap().to_ascii_lowercase(),
        MEMBERSHIP_STAKE_VAULT,
        "the attributedStake read targets the vault"
    );
    let data = call["data"].as_str().unwrap();
    assert!(
        data.to_ascii_lowercase().contains(&MEMBER_ADDR[2..]),
        "member address in the calldata: {data}"
    );
}

#[test]
fn sbt_balance_of_call_targets_the_sbt_contract() {
    let call = sbt_balance_of_call(MEMBER_ADDR);
    assert_eq!(
        call["to"].as_str().unwrap().to_ascii_lowercase(),
        CITRATE_MEMBER_SBT,
        "the balanceOf read targets the CitrateMemberSBT"
    );
    let data = call["data"].as_str().unwrap();
    assert!(data.starts_with("0x70a08231"), "balanceOf selector: {data}");
}

// ===========================================================================
// read_grant_status over a mock RPC (Rule 1: TEST transport)
// ===========================================================================

#[test]
fn read_grant_status_granted_member_reads_real_stake_and_sbt() {
    // Call order: attributedStake, attributedShares, balanceOf(SBT).
    let responses = vec![
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // attributedStake == 32000e18
        ok(JsonValue::String(uint256_hex(1))),               // attributedShares > 0
        ok(JsonValue::String(uint256_hex(1))),               // SBT balanceOf == 1
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert_eq!(
        status.attributed_stake_wei,
        REQUIREMENT_WEI.to_string(),
        "the real attributedStake, not a sim"
    );
    assert_eq!(status.attributed_shares_wei, "1", "the real attributedShares");
    assert!(status.has_sbt, "SBT balanceOf==1 → member holds the SBT");

    // The three eth_calls targeted the right contracts in order.
    let reqs = rpc.transport.requests();
    assert_eq!(reqs.len(), 3, "exactly three eth_calls");
    assert_eq!(
        reqs[0]["params"][0]["to"].as_str().unwrap().to_ascii_lowercase(),
        MEMBERSHIP_STAKE_VAULT
    );
    assert_eq!(
        reqs[2]["params"][0]["to"].as_str().unwrap().to_ascii_lowercase(),
        CITRATE_MEMBER_SBT
    );
}

#[test]
fn read_grant_status_fresh_member_honestly_reads_zero_and_no_sbt() {
    // A never-granted member: attributedStake=0, attributedShares=0, sbt=0.
    let responses = vec![
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(uint256_hex(0))),
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("fresh grant status read");

    assert_eq!(status.attributed_stake_wei, "0", "fresh member: 0 stake");
    assert_eq!(status.attributed_shares_wei, "0", "fresh member: 0 shares");
    assert!(!status.has_sbt, "fresh member holds no SBT (balanceOf==0)");
}

#[test]
fn read_grant_status_rejects_a_malformed_address_before_any_rpc() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert!(read_grant_status(&rpc, "0xnope").is_err());
    assert!(
        rpc.transport.requests().is_empty(),
        "fail closed BEFORE any node call on a bad address"
    );
}

#[test]
fn read_grant_status_refuses_to_truncate_a_value_beyond_u128() {
    // A stake return whose high 16 bytes are non-zero → refuse to truncate (Rule 1),
    // rather than silently report a wrong (truncated) attributedStake.
    let mut overflow = [0u8; 32];
    overflow[0] = 1;
    let overflow_hex = format!("0x{}", hex::encode(overflow));
    let responses = vec![ok(JsonValue::String(overflow_hex))];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    assert!(
        read_grant_status(&rpc, MEMBER_ADDR).is_err(),
        "a value beyond u128 is refused, never truncated"
    );
}

// ===========================================================================
// The GrantStatus serde shape the bridge consumes (camelCase decimal strings)
// ===========================================================================

#[test]
fn grant_status_serializes_camelcase_decimal_strings() {
    let status = GrantStatus {
        attributed_stake_wei: REQUIREMENT_WEI.to_string(),
        attributed_shares_wei: "1".to_string(),
        has_sbt: true,
    };
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(
        json["attributedStakeWei"],
        REQUIREMENT_WEI.to_string(),
        "wei as a decimal string, camelCase key (like PendingWithdrawal)"
    );
    assert_eq!(json["attributedSharesWei"], "1");
    assert_eq!(json["hasSbt"], true);
}
