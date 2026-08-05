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

/// The member's `MemberBond` escrow (M-2). A DISTINCT address from `MEMBER_ADDR`
/// on purpose: these tests exist to prove the registry's staker is the CLONE, and
/// a read accidentally pointed at the member would pass unnoticed if the two
/// values were the same.
const BOND_ADDR: &str = "0x00000000000000000000000000000000000b0d1e";
/// `bondOf`'s 32-byte return word: the address left-padded to a full word.
const BOND_WORD: &str = "0x00000000000000000000000000000000000000000000000000000000000b0d1e";

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

/// The pinned `attributedPrincipal(address)` selector is the REAL keccak of the
/// signature (Rule 11 drift tripwire).
///
/// M-2 replaced `attributedShares` with this. The old selector would hit a
/// function the vault no longer has and return empty — a drift that reads as
/// "0 shares" rather than as an error.
#[test]
fn attributed_principal_selector_is_keccak_of_signature() {
    assert_eq!(
        attributed_principal_selector(),
        derive_selector("attributedPrincipal(address)")
    );
}

/// The pinned M-2 bond selectors are the REAL keccaks of their signatures (Rule 11
/// drift tripwires). A drift here silently mis-reports the LOCK or the KYC gate on
/// a T1 money surface.
#[test]
fn bond_selectors_are_keccak_of_signatures() {
    assert_eq!(bond_of_selector(), derive_selector("bondOf(address)"));
    assert_eq!(unlock_block_selector(), derive_selector("unlockBlock()"));
    assert_eq!(is_unlocked_selector(), derive_selector("isUnlocked()"));
    assert_eq!(is_kyc_verified_selector(), derive_selector("isKycVerified()"));
    assert_eq!(activated_selector(), derive_selector("activated()"));
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
    // M-2 call order: attributedStake, attributedPrincipal, balanceOf(SBT),
    // bondOf, unlockBlock, isUnlocked, isKycVerified, pubkeyOfStaker(BOND).
    let responses = vec![
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // attributedStake == 32000e18
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // attributedPrincipal
        ok(JsonValue::String(uint256_hex(1))),               // SBT balanceOf == 1
        ok(JsonValue::String(BOND_WORD.to_string())),        // bondOf -> the escrow
        ok(JsonValue::String(uint256_hex(300_000))),         // unlockBlock (bond IS deployed)
        ok(JsonValue::String(uint256_hex(0))),               // isUnlocked == false
        ok(JsonValue::String(uint256_hex(0))),               // isKycVerified == false
        ok(JsonValue::String(uint256_hex(0))),               // pubkeyOfStaker(bond) == 0
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert_eq!(
        status.attributed_stake_wei,
        REQUIREMENT_WEI.to_string(),
        "the real attributedStake, not a sim"
    );
    assert_eq!(
        status.attributed_principal_wei,
        REQUIREMENT_WEI.to_string(),
        "the real attributedPrincipal"
    );
    assert!(status.has_sbt, "SBT balanceOf==1 -> member holds the SBT");
    assert!(status.bond_deployed, "the escrow exists");
    assert_eq!(status.unlock_block, Some(300_000), "the height leg of the lock");
    assert!(!status.is_unlocked, "still inside the 1-year lock");
    assert!(!status.is_kyc_verified, "unverified until the orchestrator attests");

    // The staker read MUST target the BOND, not the member: under M-2 the clone is
    // the staker, so asking about the member reads zero forever and would report a
    // fully-activated validator as having none.
    let reqs = rpc.transport().requests();
    assert_eq!(reqs.len(), 8, "stake, principal, sbt, bondOf, 3 bond getters, pubkeyOfStaker");
    let staker_calldata = reqs[7]["params"][0]["data"].as_str().unwrap().to_ascii_lowercase();
    assert!(
        staker_calldata.ends_with(&BOND_ADDR[2..].to_ascii_lowercase()),
        "pubkeyOfStaker must be asked about the BOND ({BOND_ADDR}); calldata was {staker_calldata}"
    );
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
    // A never-granted member: 0 stake, 0 principal, no SBT, and a bond address that
    // RESOLVES (CREATE2 is deterministic) but has no code — so the bond getters
    // return empty and every downstream field reports fail-closed.
    let responses = vec![
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(uint256_hex(0))),
        ok(JsonValue::String(BOND_WORD.to_string())), // bondOf still answers
        ok(JsonValue::String("0x".to_string())),      // unlockBlock: NO CODE -> empty
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("fresh grant status read");

    assert_eq!(status.attributed_stake_wei, "0", "fresh member: 0 stake");
    assert_eq!(status.attributed_principal_wei, "0", "fresh member: 0 principal");
    assert!(!status.has_sbt, "fresh member holds no SBT (balanceOf==0)");
    assert_eq!(
        status.bond_address, BOND_ADDR,
        "the address resolves even for a never-granted member (CREATE2)"
    );
    assert!(
        !status.bond_deployed,
        "but it does NOT exist - the address alone must never imply a grant"
    );
    assert_eq!(status.unlock_block, None, "no lock without a bond");
    assert!(!status.is_unlocked, "fail-closed: locked");
    assert!(!status.is_kyc_verified, "fail-closed: unverified");
    assert!(!status.has_validator, "no bond, no validator read");
    assert_eq!(
        rpc.transport().requests().len(),
        5,
        "an undeployed bond short-circuits: no isUnlocked/isKyc/pubkeyOfStaker reads"
    );
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
        attributed_principal_wei: REQUIREMENT_WEI.to_string(),
        has_sbt: true,
        bond_address: BOND_ADDR.to_string(),
        bond_deployed: true,
        bonded_stake_wei: "0".to_string(),
        has_validator: false,
        unlock_block: Some(300_000),
        is_unlocked: false,
        is_kyc_verified: false,
    };
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(
        json["attributedStakeWei"],
        REQUIREMENT_WEI.to_string(),
        "wei as a decimal string, camelCase key (like PendingWithdrawal)"
    );
    assert_eq!(json["attributedPrincipalWei"], REQUIREMENT_WEI.to_string());
    assert_eq!(json["hasSbt"], true);
    assert_eq!(json["bondAddress"], BOND_ADDR);
    assert_eq!(json["bondDeployed"], true);
    assert_eq!(json["unlockBlock"], 300_000, "the lock crosses the bridge as a number");
    assert_eq!(json["isUnlocked"], false);
    assert_eq!(json["isKycVerified"], false);
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
fn no_bond_skips_both_registry_reads_entirely() {
    // No escrow at all (never granted). `pubkeyOfStaker` on a codeless address is
    // meaningless rather than an honest zero, and so is `stakeOf(0x00…)` — neither
    // may be dressed up as a real read.
    let responses = vec![
        ok(JsonValue::String(uint256_hex(0))),        // attributedStake
        ok(JsonValue::String(uint256_hex(0))),        // attributedPrincipal
        ok(JsonValue::String(uint256_hex(0))),        // SBT balanceOf
        ok(JsonValue::String(BOND_WORD.to_string())), // bondOf (resolves, CREATE2)
        ok(JsonValue::String("0x".to_string())),      // unlockBlock: NO CODE
    ];
    let transport = MockRpc::new(responses);
    let rpc = RpcClient::with_transport(transport);
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert!(!status.has_validator);
    assert_eq!(status.bonded_stake_wei, "0");
    assert!(!status.bond_deployed);

    let reqs = rpc.transport().requests();
    let eth_calls = reqs.iter().filter(|r| r["method"] == "eth_call").count();
    assert_eq!(
        eth_calls, 5,
        "an undeployed bond stops after unlockBlock: no isUnlocked/isKyc, and \
         neither pubkeyOfStaker nor stakeOf"
    );
    assert_eq!(
        reqs.iter().filter(|r| r["method"] == "eth_getBalance").count(),
        0,
        "M-2 reads no native balance: the principal is in the escrow, never the member"
    );
}


/// An ACTIVATED member: the escrow has bonded its principal into the registry and
/// the member is validating. Attribution STAYS set the whole time — under M-2 the
/// vault is credited at grant and stays credited, so unlike the bond-fund era there
/// is no window where a working validator reads as ungranted.
#[test]
fn activated_member_reads_the_registry_stake_via_the_bond() {
    let pubkey_word = format!("0x{}", hex::encode([0x11u8; 32]));
    let responses = vec![
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // attributedStake
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // attributedPrincipal
        ok(JsonValue::String(uint256_hex(1))),               // SBT balanceOf == 1
        ok(JsonValue::String(BOND_WORD.to_string())),        // bondOf
        ok(JsonValue::String(uint256_hex(300_000))),         // unlockBlock
        ok(JsonValue::String(uint256_hex(0))),               // isUnlocked == false
        ok(JsonValue::String(uint256_hex(1))),               // isKycVerified == true
        ok(JsonValue::String(pubkey_word)),                  // pubkeyOfStaker(BOND) != 0
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // stakeOf(pubkey)
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert!(status.has_validator, "pubkeyOfStaker(bond) != 0 => activated");
    assert_eq!(
        status.bonded_stake_wei,
        REQUIREMENT_WEI.to_string(),
        "the bonded principal comes from the registry"
    );
    assert_eq!(
        status.attributed_stake_wei,
        REQUIREMENT_WEI.to_string(),
        "and attribution REMAINS set under M-2 - bonding does not empty the vault"
    );
    assert!(status.is_kyc_verified, "verified, so withdrawal is gated only by the lock");
    assert!(!status.is_unlocked, "which has not elapsed");
}

/// THE M-2 WINDOW: granted, escrow funded, but the member has not run the
/// activation ceremony yet — which can be days. The registry reads 0 the whole
/// time, so settling on it would report a granted member as ungranted: the mirror
/// image of the 2026-07-28 stall the native-balance bridge was added to fix.
/// Attribution is what carries this window now.
#[test]
fn granted_but_unactivated_member_settles_on_attribution() {
    let responses = vec![
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // attributedStake
        ok(JsonValue::String(uint256_hex(REQUIREMENT_WEI))), // attributedPrincipal
        ok(JsonValue::String(uint256_hex(1))),               // SBT balanceOf == 1 (granted)
        ok(JsonValue::String(BOND_WORD.to_string())),        // bondOf
        ok(JsonValue::String(uint256_hex(300_000))),         // unlockBlock (escrow funded)
        ok(JsonValue::String(uint256_hex(0))),               // isUnlocked
        ok(JsonValue::String(uint256_hex(0))),               // isKycVerified
        ok(JsonValue::String(uint256_hex(0))),               // pubkeyOfStaker == 0 (not activated)
    ];
    let rpc = RpcClient::with_transport(MockRpc::new(responses));
    let status = read_grant_status(&rpc, MEMBER_ADDR).expect("grant status read");

    assert!(status.has_sbt, "the grant minted the SBT");
    assert!(status.bond_deployed, "and funded the escrow");
    assert!(!status.has_validator, "but the member has not activated yet");
    assert_eq!(status.bonded_stake_wei, "0", "registry empty until activation");
    assert_eq!(
        status.attributed_stake_wei,
        REQUIREMENT_WEI.to_string(),
        "attribution is the settle signal across this window"
    );
}
