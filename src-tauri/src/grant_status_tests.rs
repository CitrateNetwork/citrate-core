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

/// An `eth_getBalance` return — a bare `0x`-hex QUANTITY (not a 32-byte word), the
/// shape `RpcClient::get_balance` parses. `read_grant_status` reads the member's
/// native balance LAST, so every scripted response list ends with one of these.
fn hex_quantity(n: u128) -> String {
    format!("0x{n:x}")
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
///
/// Updated for the 2026-08-04 re-roll (was `0x61e324cf…`). MembershipStakeVault
/// is deployed with plain CREATE, so its address is deployer-nonce-derived and
/// MOVES on every re-roll — unlike the CREATE2 contracts in this book, which
/// reproduced byte-identically. This tripwire firing on a re-roll is it working:
/// update it from `contracts/addresses/40204.json`, never from a runbook's
/// projection (the 2026-08-04 orchestrator projected `0x61E324cF…` and was wrong,
/// because the dry-run's deploy ORDER differed from the real one).
#[test]
fn membership_stake_vault_is_the_canonical_40204_value() {
    assert_eq!(
        membership_stake_vault(),
        "0x04c32967816187b2efdcd4937dbba59e051f99db"
    );
}

/// The SBT address MUST be the canonical 40204.json value.
///
/// Updated for the 2026-08-04 re-roll (was `0x4ce39f89…`) — plain CREATE, so
/// nonce-derived and it moves on every re-roll. See the vault pin above.
#[test]
fn citrate_member_sbt_is_the_canonical_40204_value() {
    assert_eq!(
        citrate_member_sbt(),
        "0xad826d0439f7ad5a3512a8927b632cbca2840e10"
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
        membership_stake_vault(),
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
        citrate_member_sbt(),
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
        ok(JsonValue::String(uint256_hex(0))),               // pubkeyOfStaker == 0 (vault-era member)
        ok(JsonValue::String(hex_quantity(0))),              // eth_getBalance (native) == 0
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
    assert_eq!(status.native_balance_wei, "0", "vault-era member: 0 native");

    // The calls targeted the right contracts in order: attributedStake, shares, SBT,
    // pubkeyOfStaker (this member has no validator, so stakeOf is correctly skipped),
    // then eth_getBalance (native) last.
    let reqs = rpc.transport().requests();
    assert_eq!(reqs.len(), 5, "stake, shares, sbt, pubkeyOfStaker, getBalance");
    assert_eq!(reqs[4]["method"].as_str().unwrap(), "eth_getBalance");
    assert_eq!(
        reqs[0]["params"][0]["to"].as_str().unwrap().to_ascii_lowercase(),
        membership_stake_vault()
    );
    assert_eq!(
        reqs[2]["params"][0]["to"].as_str().unwrap().to_ascii_lowercase(),
        citrate_member_sbt()
    );
}

#[test]
fn read_grant_status_fresh_member_honestly_reads_zero_and_no_sbt() {
    // A never-granted member: attributedStake=0, attributedShares=0, sbt=0.
    let responses = vec![
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(uint256_hex(0))), // pubkeyOfStaker == 0 (no validator)
        ok(JsonValue::String(hex_quantity(0))), // eth_getBalance (native) == 0
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("fresh grant status read");

    assert_eq!(status.attributed_stake_wei, "0", "fresh member: 0 stake");
    assert_eq!(status.attributed_shares_wei, "0", "fresh member: 0 shares");
    assert!(!status.has_sbt, "fresh member holds no SBT (balanceOf==0)");
    assert_eq!(status.native_balance_wei, "0", "fresh member: 0 native");
}

#[test]
fn read_grant_status_rejects_a_malformed_address_before_any_rpc() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert!(read_grant_status(&rpc, "0xnope").is_err());
    assert!(
        rpc.transport().requests().is_empty(),
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
        bonded_stake_wei: "0".to_string(),
        has_validator: false,
        native_balance_wei: REQUIREMENT_WEI.to_string(),
    };
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(
        json["attributedStakeWei"],
        REQUIREMENT_WEI.to_string(),
        "wei as a decimal string, camelCase key (like PendingWithdrawal)"
    );
    assert_eq!(json["attributedSharesWei"], "1");
    assert_eq!(json["hasSbt"], true);
    assert_eq!(
        json["nativeBalanceWei"],
        REQUIREMENT_WEI.to_string(),
        "the funded-bond native balance crosses the bridge as a camelCase decimal string"
    );
}

/// The pinned `pubkeyOfStaker(address)` selector is the REAL keccak of the
/// signature (Rule 11 drift tripwire). This read is what tells a bonded member
/// apart from an ungranted one now that the vault is never credited.
#[test]
fn pubkey_of_staker_selector_is_keccak_of_signature() {
    assert_eq!(
        pubkey_of_staker_selector(),
        derive_selector("pubkeyOfStaker(address)")
    );
}

/// The pinned `stakeOf(bytes32)` selector is the REAL keccak of the signature
/// (Rule 11 drift tripwire).
#[test]
fn stake_of_selector_is_keccak_of_signature() {
    assert_eq!(stake_of_selector(), derive_selector("stakeOf(bytes32)"));
}

/// A member with NO validator reads `hasValidator: false` and a ZERO bonded stake,
/// and `stakeOf` is NEVER called — `stakeOf(0x00…)` is a meaningless read, not an
/// honest zero, so we must not dress it up as one.
#[test]
fn no_validator_reads_zero_bonded_and_never_calls_stake_of() {
    let responses = vec![
        ok(JsonValue::String(uint256_hex(0))),  // attributedStake
        ok(JsonValue::String(uint256_hex(0))),  // attributedShares
        ok(JsonValue::String(uint256_hex(0))),  // SBT balanceOf
        ok(JsonValue::String(uint256_hex(0))),  // pubkeyOfStaker == 0
        ok(JsonValue::String(hex_quantity(0))), // eth_getBalance (native)
    ];
    let transport = MockRpc::new(responses);
    let rpc = RpcClient::with_transport(transport);
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert!(!status.has_validator);
    assert_eq!(status.bonded_stake_wei, "0");
    // Five reads: stake, shares, sbt, pubkeyOfStaker (4 eth_call), then the native
    // eth_getBalance. Exactly FOUR eth_calls proves stakeOf — the 5th eth_call — was
    // NOT sent (a meaningless `stakeOf(0x00…)` we must not dress up as an honest zero).
    let reqs = rpc.transport().requests();
    let eth_calls = reqs.iter().filter(|r| r["method"] == "eth_call").count();
    let balance_calls = reqs.iter().filter(|r| r["method"] == "eth_getBalance").count();
    assert_eq!(eth_calls, 4, "stakeOf must not be called without a pubkey");
    assert_eq!(balance_calls, 1, "the native balance is read once");
}

/// A BOND-FUND member: nothing in the vault, but a registered validator holding the
/// full bond. This is the shape that read as "ungranted" before the registry reads
/// were added — working, but indistinguishable from broken.
#[test]
fn bonded_member_reads_the_registry_stake_even_with_an_empty_vault() {
    let pubkey_word = format!("0x{}", hex::encode([0x11u8; 32]));
    let responses = vec![
        ok(JsonValue::String(uint256_hex(0))),               // attributedStake  == 0 (vault never credited)
        ok(JsonValue::String(uint256_hex(0))),               // attributedShares == 0
        ok(JsonValue::String(uint256_hex(1))),               // SBT balanceOf    == 1
        ok(JsonValue::String(pubkey_word)),                  // pubkeyOfStaker   != 0
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // stakeOf(pubkey)  == 32000e18
        ok(JsonValue::String(hex_quantity(0))),              // eth_getBalance (native, spent on the bond)
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert!(status.has_validator, "pubkeyOfStaker != 0 ⇒ the member has a validator");
    assert_eq!(
        status.bonded_stake_wei,
        REQUIREMENT_WEI.to_string(),
        "the bonded principal comes from the registry, not the vault"
    );
    assert_eq!(status.attributed_stake_wei, "0", "and the vault honestly reads 0");
}

/// THE STALL REPRO (2026-07-28): a member the ADR 2026-07-27 grant just funded — the
/// treasury sent the 32k bond to the member's OWN EOA (native) + minted the SBT, but
/// the member has NOT self-bonded yet. The vault reads 0, the registry reads 0 (no
/// validator), and ONLY the native balance carries the principal. Before the native
/// read this member polled "still settling" forever despite a perfect grant.
#[test]
fn funded_eoa_member_reads_the_native_bond_before_self_bonding() {
    let responses = vec![
        ok(JsonValue::String(uint256_hex(0))),               // attributedStake  == 0
        ok(JsonValue::String(uint256_hex(0))),               // attributedShares == 0
        ok(JsonValue::String(uint256_hex(1))),               // SBT balanceOf    == 1 (granted)
        ok(JsonValue::String(uint256_hex(0))),               // pubkeyOfStaker   == 0 (not yet registered)
        ok(JsonValue::String(hex_quantity(REQUIREMENT_WEI))), // eth_getBalance   == 32000e18 (funded bond)
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert!(status.has_sbt, "the grant minted the SBT");
    assert!(!status.has_validator, "the member has not self-bonded yet");
    assert_eq!(status.attributed_stake_wei, "0", "vault never credited (bond-fund model)");
    assert_eq!(status.bonded_stake_wei, "0", "registry empty until the member registers");
    assert_eq!(
        status.native_balance_wei,
        REQUIREMENT_WEI.to_string(),
        "the 32k grant principal is the member's native EOA balance — the settle signal"
    );
}
