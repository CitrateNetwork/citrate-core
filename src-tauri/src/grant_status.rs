//! citrate-core — BC-1.3: the REAL on-chain membership grant-status read.
//! @rule8 · T1 money-path surface.
//!
//! The S5 onboarding step (grant + stake ceremony) settles ONLY when the
//! 32,000-SALT membership grant is genuinely on-chain. This module reads that
//! truth from the live 40204 RPC via `eth_call` — it NEVER fabricates a settlement
//! (Rule 1). A fresh/never-granted member honestly reads 0 stake / 0 shares / no
//! SBT, and S5 stays in its honest "still settling" state.
//!
//! ## What "granted" means (grounded against live 40204)
//! The settle decision (`isGrantOnChain`, store.ts) gates on exactly two reads:
//! - `MembershipStakeVault.attributedStake(member) >= VALIDATOR_STAKE_REQUIREMENT`
//!   (== 32000e18) — the treasury deposited the grant into the vault attributed to
//!   the member's AA wallet and staked it.
//! - `CitrateMemberSBT.balanceOf(member) == 1` — the member holds the SBT.
//!
//! `attributedShares(member)` is ALSO read and returned (a granted member reads
//! `> 0`), but it is NOT part of the settle gate — stake + SBT is the actual gate.
//! It is surfaced for display/telemetry only.
//!
//! The store combines these with the `/userinfo` entitlement leg (paid tier +
//! active) to decide S5 "fully settled".
//!
//! The `member` address is the AA smart-wallet the grant targets = the OIDC
//! `wallet_address` claim, NOT the custody EOA. The frontend passes that claim
//! address to `membership_grant_status`.
//!
//! ## Secret discipline (@rule8 / Rule 3)
//! This module is a PURE READ. It never sees key material, never signs, and its
//! `#[tauri::command]` returns only decoded PUBLIC chain data. Coarse, secret-free
//! errors. Mirrors `staking.rs`'s house style (validate_address,
//! decode_uint256_word, pinned selectors + keccak drift tests, injectable RpcClient).

// Some helper surface (calldata builders, selector accessors) is reached only by
// tests until the full frontend round-trip lands (mirrors staking.rs / earnings.rs).
#![allow(dead_code)]

use crate::staking::decode_uint256_word;

/// The `MembershipStakeVault` on 40204 (canonical address book:
/// `contracts/addresses/40204.json`). Lowercase `0x`-hex; the `eth_call` target for
/// `attributedStake`, `attributedShares`, and `VALIDATOR_STAKE_REQUIREMENT`.
pub const MEMBERSHIP_STAKE_VAULT: &str = "0x61e324cfd6b7cb106ac0ad1df163bdfef2b74268";

/// The `CitrateMemberSBT` on 40204 (canonical address book). Lowercase `0x`-hex;
/// the `eth_call` target for the SBT `balanceOf(member)` membership read.
pub const CITRATE_MEMBER_SBT: &str = "0x3e0c2b1cd29a615e4ea2e263c8e7df3aef243e42";

/// 4-byte selector for `attributedStake(address)` —
/// `keccak256("attributedStake(address)")[..4]`. PINNED here + proven by the
/// `attributed_stake_selector_is_keccak_of_signature` drift test (Rule 11), so this
/// lean tree needs no runtime keccak. A drift would read the WRONG function and
/// mis-settle S5 (@rule8).
const ATTRIBUTED_STAKE_SELECTOR: [u8; 4] = [0xb9, 0x2e, 0x7e, 0xcb];

/// 4-byte selector for `attributedShares(address)` —
/// `keccak256("attributedShares(address)")[..4]`. PINNED + drift-tested.
const ATTRIBUTED_SHARES_SELECTOR: [u8; 4] = [0x90, 0x81, 0x00, 0x72];

/// 4-byte selector for `VALIDATOR_STAKE_REQUIREMENT()` (no args) —
/// `keccak256("VALIDATOR_STAKE_REQUIREMENT()")[..4]`. PINNED + drift-tested. The
/// getter returns 32000e18 on-chain; pinned per the module scope so a signature
/// drift trips the tripwire (the store compares attributedStake against this).
const VALIDATOR_STAKE_REQUIREMENT_SELECTOR: [u8; 4] = [0xa3, 0xea, 0xc0, 0x15];

/// 4-byte selector for `balanceOf(address)` — `keccak256("balanceOf(address)")[..4]`.
/// The canonical ERC-20/721 `balanceOf`; PINNED + drift-tested. Reused here for the
/// `CitrateMemberSBT.balanceOf(member)` membership read (== 1 for a member).
const SBT_BALANCE_OF_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

/// Selector for `attributedStake(address)`.
pub fn attributed_stake_selector() -> [u8; 4] {
    ATTRIBUTED_STAKE_SELECTOR
}

/// Selector for `attributedShares(address)`.
pub fn attributed_shares_selector() -> [u8; 4] {
    ATTRIBUTED_SHARES_SELECTOR
}

/// Selector for `VALIDATOR_STAKE_REQUIREMENT()`.
pub fn validator_stake_requirement_selector() -> [u8; 4] {
    VALIDATOR_STAKE_REQUIREMENT_SELECTOR
}

/// Selector for the SBT `balanceOf(address)` read.
pub fn sbt_balance_of_selector() -> [u8; 4] {
    SBT_BALANCE_OF_SELECTOR
}

/// Errors from the grant-status reader. Coarse + secret-free (this module never sees
/// key material — it does public `eth_call` reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantStatusError {
    /// The address was not a `0x`-prefixed 20-byte hex string.
    BadAddress(String),
    /// The live RPC read failed (transport / node error / missing field).
    Rpc(String),
    /// The `eth_call` return did not decode to a `uint256` (short, or beyond u128 —
    /// grant amounts fit in u128, so a larger value means the return is not a plain
    /// balance and we refuse to truncate).
    Decode(String),
}

impl std::fmt::Display for GrantStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantStatusError::BadAddress(m) => write!(f, "grant-status: bad address: {m}"),
            GrantStatusError::Rpc(m) => write!(f, "grant-status: rpc error: {m}"),
            GrantStatusError::Decode(m) => write!(f, "grant-status: decode error: {m}"),
        }
    }
}

impl std::error::Error for GrantStatusError {}

/// The on-chain membership grant status the store folds into the S5 settle decision.
/// Every field is a live chain read (Rule 1). Serde camelCase; wei as decimal
/// strings (mirrors `staking::PendingWithdrawal`) so a value beyond JS number range
/// survives the bridge.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GrantStatus {
    /// `MembershipStakeVault.attributedStake(member)` in wei (decimal string). The
    /// store settles the grant leg when this `>= VALIDATOR_STAKE_REQUIREMENT`.
    #[serde(rename = "attributedStakeWei")]
    pub attributed_stake_wei: String,
    /// `MembershipStakeVault.attributedShares(member)` in wei (decimal string). A
    /// granted member reads `> 0`.
    #[serde(rename = "attributedSharesWei")]
    pub attributed_shares_wei: String,
    /// `CitrateMemberSBT.balanceOf(member) == 1` — the member holds the SBT.
    #[serde(rename = "hasSbt")]
    pub has_sbt: bool,
}

/// Validate a `0x`-prefixed 20-byte hex address; return the lowercased canonical
/// form (fail closed rather than read with a malformed address).
fn validate_address(addr: &str) -> Result<String, GrantStatusError> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    if stripped.len() != 40 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GrantStatusError::BadAddress(addr.to_string()));
    }
    Ok(format!("0x{}", stripped.to_ascii_lowercase()))
}

/// Build `selector ++ 32-byte left-padded address` calldata (36 bytes) for a
/// single-address getter. `addr` must already be validated.
fn encode_address_calldata(selector: [u8; 4], addr: &str) -> Vec<u8> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(stripped).unwrap_or_default();
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&selector);
    let mut word = [0u8; 32];
    if bytes.len() == 20 {
        word[12..32].copy_from_slice(&bytes);
    }
    calldata.extend_from_slice(&word);
    calldata
}

/// The `eth_call` object for `attributedStake(member)` on the vault: `{to, data}`.
fn attributed_stake_call(addr: &str) -> serde_json::Value {
    let calldata = encode_address_calldata(attributed_stake_selector(), addr);
    serde_json::json!({
        "to": MEMBERSHIP_STAKE_VAULT,
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// The `eth_call` object for `attributedShares(member)` on the vault: `{to, data}`.
fn attributed_shares_call(addr: &str) -> serde_json::Value {
    let calldata = encode_address_calldata(attributed_shares_selector(), addr);
    serde_json::json!({
        "to": MEMBERSHIP_STAKE_VAULT,
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// The `eth_call` object for `balanceOf(member)` on the SBT contract: `{to, data}`.
fn sbt_balance_of_call(addr: &str) -> serde_json::Value {
    let calldata = encode_address_calldata(sbt_balance_of_selector(), addr);
    serde_json::json!({
        "to": CITRATE_MEMBER_SBT,
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// **The real grant-status read.** Read, on the live 40204 RPC via `eth_call`:
/// - `MembershipStakeVault.attributedStake(member)` → wei,
/// - `MembershipStakeVault.attributedShares(member)` → wei,
/// - `CitrateMemberSBT.balanceOf(member)` → `has_sbt = (balance == 1)`.
///
/// Rule 1: every value comes from chain, not a sim; a fresh/never-granted member
/// honestly reads 0 / 0 / false. `rpc` is injected so tests script a mock transport;
/// production wires [`crate::rpc::RpcClient::citrate`]. Decodes refuse to truncate a
/// value beyond u128 (never a wrong, silently-truncated stake).
pub fn read_grant_status<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    member_address: &str,
) -> Result<GrantStatus, GrantStatusError> {
    let addr = validate_address(member_address)?;

    let stake_ret = rpc
        .eth_call(attributed_stake_call(&addr))
        .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
    let attributed_stake =
        decode_uint256_word(&stake_ret).map_err(|e| GrantStatusError::Decode(e.to_string()))?;

    let shares_ret = rpc
        .eth_call(attributed_shares_call(&addr))
        .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
    let attributed_shares =
        decode_uint256_word(&shares_ret).map_err(|e| GrantStatusError::Decode(e.to_string()))?;

    let sbt_ret = rpc
        .eth_call(sbt_balance_of_call(&addr))
        .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
    let sbt_balance =
        decode_uint256_word(&sbt_ret).map_err(|e| GrantStatusError::Decode(e.to_string()))?;

    Ok(GrantStatus {
        attributed_stake_wei: attributed_stake.to_string(),
        attributed_shares_wei: attributed_shares.to_string(),
        has_sbt: sbt_balance == 1,
    })
}

/// `membership_grant_status` command — read the member's REAL on-chain grant status
/// from the live 40204 RPC and return the decoded [`GrantStatus`]. A PURE READ:
/// @rule8 / Rule 3 — no signing, no custody, no key material. `member_address` is
/// the AA smart-wallet the grant targets (the OIDC `wallet_address` claim), NOT the
/// custody EOA. A malformed address fails closed before any node call.
#[tauri::command]
pub fn membership_grant_status(
    member_address: String,
) -> std::result::Result<GrantStatus, String> {
    let rpc = crate::rpc::RpcClient::citrate();
    read_grant_status(&rpc, &member_address).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("grant_status_tests.rs");
}
