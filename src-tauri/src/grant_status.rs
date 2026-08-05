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
//! Under the ADR 2026-07-27 bond-fund model the grant instead funds the member's
//! OWN EOA with the 32k bond (replacing `vault.grant`), so the settle gate ALSO
//! accepts `eth_getBalance(member) >= 32000e18` (native funded bond) and
//! `ValidatorRegistry.stakeOf(pubkeyOfStaker(member)) >= 32000e18` (already
//! self-bonded). `isGrantOnChain` settles on SBT + the MAX of those principal reads,
//! so a member granted under EITHER model — and whether or not they have self-bonded
//! yet — settles honestly instead of polling forever.
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
/// From the generated 40204 book — see `crate::addresses`. Was hardcoded here AND
/// in `node.rs`; a reroll moved it and the stale pin cost a day (2026-07-28).
pub fn membership_stake_vault() -> &'static str {
    crate::addresses::membership_stake_vault()
}

/// The `CitrateMemberSBT` on 40204 (canonical address book). Lowercase `0x`-hex;
/// the `eth_call` target for the SBT `balanceOf(member)` membership read.
/// RE-PINNED 2026-07-27 to the WP-11 reroll book (was `0x4ce39f89…0cf1`).
/// From the generated 40204 book — see `crate::addresses`.
pub fn citrate_member_sbt() -> &'static str {
    crate::addresses::citrate_member_sbt()
}

/// 4-byte selector for `attributedStake(address)` —
/// `keccak256("attributedStake(address)")[..4]`. PINNED here + proven by the
/// `attributed_stake_selector_is_keccak_of_signature` drift test (Rule 11), so this
/// lean tree needs no runtime keccak. A drift would read the WRONG function and
/// mis-settle S5 (@rule8).
const ATTRIBUTED_STAKE_SELECTOR: [u8; 4] = [0xb9, 0x2e, 0x7e, 0xcb];

/// 4-byte selector for `attributedPrincipal(address)` —
/// `keccak256("attributedPrincipal(address)")[..4]`. PINNED + drift-tested.
///
/// M-2 (citrate-chain #141) REPLACED `attributedShares` with this. The old name
/// was an stSALT SHARE COUNT from the pool model; there are no shares under
/// bonding, so the old selector would hit a function the vault no longer has and
/// return empty — a drift that reads as "0 shares" rather than as an error.
const ATTRIBUTED_PRINCIPAL_SELECTOR: [u8; 4] = [0x60, 0xde, 0xc6, 0xe8];

/// 4-byte selector for `bondOf(address)` — `keccak256("bondOf(address)")[..4]`.
/// PINNED + drift-tested. Returns the member's `MemberBond` escrow address,
/// CREATE2-deterministic so it resolves even before the bond is deployed.
const BOND_OF_SELECTOR: [u8; 4] = [0x72, 0xd2, 0xb6, 0xc0];

/// `MemberBond.unlockBlock()` — `keccak256("unlockBlock()")[..4]`. PINNED.
const UNLOCK_BLOCK_SELECTOR: [u8; 4] = [0xea, 0x35, 0xdf, 0x16];

/// `MemberBond.isUnlocked()` — `keccak256("isUnlocked()")[..4]`. PINNED. True
/// once EITHER lock leg has elapsed (height or wall-clock).
const IS_UNLOCKED_SELECTOR: [u8; 4] = [0x83, 0x80, 0xed, 0xb7];

/// `MemberBond.isKycVerified()` — `keccak256("isKycVerified()")[..4]`. PINNED.
/// Reads through to the SBT attestation; gates money OUT, never participation.
const IS_KYC_VERIFIED_SELECTOR: [u8; 4] = [0xb2, 0x0e, 0xa7, 0x04];

/// `MemberBond.activated()` — `keccak256("activated()")[..4]`. PINNED. True once
/// the member has bonded their principal into the registry via the ceremony.
const ACTIVATED_SELECTOR: [u8; 4] = [0x18, 0x66, 0x01, 0xca];

/// 4-byte selector for `VALIDATOR_STAKE_REQUIREMENT()` (no args) —
/// `keccak256("VALIDATOR_STAKE_REQUIREMENT()")[..4]`. PINNED + drift-tested. The
/// getter returns 32000e18 on-chain; pinned per the module scope so a signature
/// drift trips the tripwire (the store compares attributedStake against this).
const VALIDATOR_STAKE_REQUIREMENT_SELECTOR: [u8; 4] = [0xa3, 0xea, 0xc0, 0x15];

/// The ValidatorRegistry. Under the bond-fund model (ADR 2026-07-27) the member's
/// 32k lives HERE, not in the vault — see the note on [`read_grant_status`]. Same
/// address the node is configured with (`node::NODE_VALIDATOR_REGISTRY_VALUE`).
/// From the generated 40204 book — see `crate::addresses`.
pub fn citrate_validator_registry() -> &'static str {
    crate::addresses::validator_registry()
}

/// 4-byte selector for `pubkeyOfStaker(address)` —
/// `keccak256("pubkeyOfStaker(address)")[..4]`. PINNED + drift-tested (Rule 11).
const PUBKEY_OF_STAKER_SELECTOR: [u8; 4] = [0xe7, 0xbe, 0x52, 0x9e];

/// 4-byte selector for `stakeOf(bytes32)` — `keccak256("stakeOf(bytes32)")[..4]`.
/// Returns the validator's `bondedStake`. PINNED + drift-tested (Rule 11).
const STAKE_OF_SELECTOR: [u8; 4] = [0x07, 0x17, 0x7c, 0x9c];

/// Selector for `pubkeyOfStaker(address)`.
pub fn pubkey_of_staker_selector() -> [u8; 4] {
    PUBKEY_OF_STAKER_SELECTOR
}

/// Selector for `stakeOf(bytes32)`.
pub fn stake_of_selector() -> [u8; 4] {
    STAKE_OF_SELECTOR
}

/// 4-byte selector for `balanceOf(address)` — `keccak256("balanceOf(address)")[..4]`.
/// The canonical ERC-20/721 `balanceOf`; PINNED + drift-tested. Reused here for the
/// `CitrateMemberSBT.balanceOf(member)` membership read (== 1 for a member).
const SBT_BALANCE_OF_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

/// Selector for `attributedStake(address)`.
pub fn attributed_stake_selector() -> [u8; 4] {
    ATTRIBUTED_STAKE_SELECTOR
}

/// Selector for `attributedPrincipal(address)`.
pub fn attributed_principal_selector() -> [u8; 4] {
    ATTRIBUTED_PRINCIPAL_SELECTOR
}

/// Selector for `bondOf(address)`.
pub fn bond_of_selector() -> [u8; 4] {
    BOND_OF_SELECTOR
}

/// Selector for `MemberBond.unlockBlock()`.
pub fn unlock_block_selector() -> [u8; 4] {
    UNLOCK_BLOCK_SELECTOR
}

/// Selector for `MemberBond.isUnlocked()`.
pub fn is_unlocked_selector() -> [u8; 4] {
    IS_UNLOCKED_SELECTOR
}

/// Selector for `MemberBond.isKycVerified()`.
pub fn is_kyc_verified_selector() -> [u8; 4] {
    IS_KYC_VERIFIED_SELECTOR
}

/// Selector for `MemberBond.activated()`.
pub fn activated_selector() -> [u8; 4] {
    ACTIVATED_SELECTOR
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
    /// `MembershipStakeVault.attributedPrincipal(member)` in wei (decimal string).
    /// M-2 replaced `attributedShares` (an stSALT share count) with raw bonded
    /// principal. A granted member reads `> 0`.
    #[serde(rename = "attributedPrincipalWei")]
    pub attributed_principal_wei: String,
    /// `MembershipStakeVault.bondOf(member)` — the member's bond escrow. Always
    /// present (CREATE2-deterministic); `bond_deployed` says whether it is real.
    #[serde(rename = "bondAddress")]
    pub bond_address: String,
    /// Whether the escrow at `bond_address` actually exists on chain yet. The
    /// address alone would imply a bond that may never have been created.
    #[serde(rename = "bondDeployed")]
    pub bond_deployed: bool,
    /// `MemberBond.unlockBlock()` — the height leg of the 1-year lock. `None`
    /// until the bond exists.
    #[serde(rename = "unlockBlock")]
    pub unlock_block: Option<u64>,
    /// `MemberBond.isUnlocked()` — true once EITHER lock leg has elapsed (height
    /// or wall-clock). Unlock makes exit ELIGIBLE; it never moves funds itself.
    #[serde(rename = "isUnlocked")]
    pub is_unlocked: bool,
    /// `MemberBond.isKycVerified()` — the SBT attestation the bond reads FIRST on
    /// every value-out path. Gates money OUT only: an unverified member keeps
    /// membership, access and their validator slot (owner decision A.7).
    #[serde(rename = "isKycVerified")]
    pub is_kyc_verified: bool,
    /// `CitrateMemberSBT.balanceOf(member) == 1` — the member holds the SBT.
    #[serde(rename = "hasSbt")]
    pub has_sbt: bool,
    /// `ValidatorRegistry.stakeOf(pubkeyOfStaker(bond))` in wei (decimal string) —
    /// the principal actually bonded in the registry. Zero until the member runs
    /// the activation ceremony; the vault's attribution is set from grant time, so
    /// this is NOT the settle signal.
    #[serde(rename = "bondedStakeWei")]
    pub bonded_stake_wei: String,
    /// `pubkeyOfStaker(bondOf(member)) != 0` — the member has ACTIVATED a
    /// validator. NOTE the staker is the member's BOND CLONE, not the member.
    #[serde(rename = "hasValidator")]
    pub has_validator: bool,
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
        "to": membership_stake_vault(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// The `eth_call` object for `attributedPrincipal(member)` on the vault.
fn attributed_principal_call(addr: &str) -> serde_json::Value {
    let calldata = encode_address_calldata(attributed_principal_selector(), addr);
    serde_json::json!({
        "to": membership_stake_vault(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// The `eth_call` object for `bondOf(member)` on the vault: `{to, data}`.
fn bond_of_call(addr: &str) -> serde_json::Value {
    let calldata = encode_address_calldata(bond_of_selector(), addr);
    serde_json::json!({
        "to": membership_stake_vault(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// An `eth_call` object for a no-argument getter on the member's bond escrow.
fn bond_nullary_call(bond: &str, selector: [u8; 4]) -> serde_json::Value {
    serde_json::json!({
        "to": bond,
        "data": format!("0x{}", hex::encode(selector)),
    })
}

/// The `eth_call` object for `pubkeyOfStaker(staker)` on the registry.
///
/// The staker under M-2 is the member's BOND CLONE, not the member — passing the
/// member address here reads zero forever.
fn pubkey_of_staker_call(addr: &str) -> serde_json::Value {
    let calldata = encode_address_calldata(pubkey_of_staker_selector(), addr);
    serde_json::json!({
        "to": citrate_validator_registry(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// The `eth_call` object for `stakeOf(pubkey)` on the registry: `{to, data}`.
/// `pubkey_word` is the raw 32-byte word returned by `pubkeyOfStaker`.
fn stake_of_call(pubkey_word: &[u8; 32]) -> serde_json::Value {
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&stake_of_selector());
    calldata.extend_from_slice(pubkey_word);
    serde_json::json!({
        "to": citrate_validator_registry(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// Decode a 32-byte word from an `eth_call` return, or `None` if it is short.
/// Mirrors `decode_uint256_word`'s input shape (the transport returns raw bytes).
fn decode_word32(ret: &[u8]) -> Option<[u8; 32]> {
    if ret.len() < 32 {
        return None;
    }
    let mut word = [0u8; 32];
    word.copy_from_slice(&ret[..32]);
    Some(word)
}

/// The `eth_call` object for `balanceOf(member)` on the SBT contract: `{to, data}`.
fn sbt_balance_of_call(addr: &str) -> serde_json::Value {
    let calldata = encode_address_calldata(sbt_balance_of_selector(), addr);
    serde_json::json!({
        "to": citrate_member_sbt(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// **The real grant-status read.** Read, on the live 40204 RPC via `eth_call`:
/// - `MembershipStakeVault.attributedStake(member)` → wei,
/// - `MembershipStakeVault.attributedShares(member)` → wei,
/// - `CitrateMemberSBT.balanceOf(member)` → `has_sbt = (balance == 1)`,
/// - `ValidatorRegistry.pubkeyOfStaker(member)` → `has_validator`, and when set,
///   `ValidatorRegistry.stakeOf(pubkey)` → `bonded_stake_wei`.
///
/// WHY BOTH STAKE READS. Before ADR 2026-07-27 the grant staked into the vault, so
/// `attributedStake` was the member's principal. Under the bond-fund model the
/// treasury funds the member's EOA and the member self-bonds, so the principal
/// lives in the ValidatorRegistry and the vault reads 0 FOREVER. Reading only the
/// vault made a correctly-bonded validator display 0 stake and never settle its
/// grant leg — working, but indistinguishable from broken. Both are read so members
/// granted under EITHER model report honestly; the caller settles on either.
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

    let principal_ret = rpc
        .eth_call(attributed_principal_call(&addr))
        .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
    let attributed_principal =
        decode_uint256_word(&principal_ret).map_err(|e| GrantStatusError::Decode(e.to_string()))?;

    let sbt_ret = rpc
        .eth_call(sbt_balance_of_call(&addr))
        .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
    let sbt_balance =
        decode_uint256_word(&sbt_ret).map_err(|e| GrantStatusError::Decode(e.to_string()))?;

    // The member's bond escrow. CREATE2-deterministic, so this answers even for a
    // member who has never been granted — which is exactly why the address alone is
    // not evidence of anything and `bond_deployed` is read separately.
    let bond_ret = rpc
        .eth_call(bond_of_call(&addr))
        .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
    let bond_word = decode_word32(&bond_ret).ok_or_else(|| {
        GrantStatusError::Decode(format!(
            "short bondOf return ({} bytes, need 32)",
            bond_ret.len()
        ))
    })?;
    let bond_address = format!("0x{}", hex::encode(&bond_word[12..32]));

    // Bond getters. An `eth_call` to an address with NO CODE returns empty, so a
    // short return here means "not deployed yet" rather than a decode fault — the
    // honest reading for a member whose grant has not landed. Every downstream
    // field then reports its fail-closed value (locked, unverified, no validator).
    let unlock_ret = rpc
        .eth_call(bond_nullary_call(&bond_address, unlock_block_selector()))
        .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
    let bond_deployed = unlock_ret.len() >= 32;

    let (unlock_block, is_unlocked, is_kyc_verified) = if bond_deployed {
        let unlock =
            decode_uint256_word(&unlock_ret).map_err(|e| GrantStatusError::Decode(e.to_string()))?;
        let unlocked_ret = rpc
            .eth_call(bond_nullary_call(&bond_address, is_unlocked_selector()))
            .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
        let kyc_ret = rpc
            .eth_call(bond_nullary_call(&bond_address, is_kyc_verified_selector()))
            .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
        (
            Some(unlock as u64),
            decode_uint256_word(&unlocked_ret).map(|v| v == 1).unwrap_or(false),
            decode_uint256_word(&kyc_ret).map(|v| v == 1).unwrap_or(false),
        )
    } else {
        (None, false, false)
    };

    // ValidatorRegistry: the STAKER's pubkey binding, then its bonded principal.
    //
    // The staker is the member's BOND CLONE, not the member — reading
    // `pubkeyOfStaker(member)` returns zero forever under M-2 and would report a
    // fully-activated validator as having none. Skipped entirely when the bond does
    // not exist: `pubkeyOfStaker` on a codeless address is meaningless rather than
    // an honest zero, and so is `stakeOf(0)`.
    let (has_validator, bonded_stake) = if bond_deployed {
        let pubkey_ret = rpc
            .eth_call(pubkey_of_staker_call(&bond_address))
            .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
        let pubkey_word = decode_word32(&pubkey_ret).ok_or_else(|| {
            GrantStatusError::Decode(format!(
                "short pubkeyOfStaker return ({} bytes, need 32)",
                pubkey_ret.len()
            ))
        })?;
        let has = pubkey_word.iter().any(|b| *b != 0);
        let staked = if has {
            let bonded_ret = rpc
                .eth_call(stake_of_call(&pubkey_word))
                .map_err(|e| GrantStatusError::Rpc(e.to_string()))?;
            decode_uint256_word(&bonded_ret)
                .map_err(|e| GrantStatusError::Decode(e.to_string()))?
        } else {
            0
        };
        (has, staked)
    } else {
        (false, 0)
    };

    Ok(GrantStatus {
        attributed_stake_wei: attributed_stake.to_string(),
        attributed_principal_wei: attributed_principal.to_string(),
        has_sbt: sbt_balance == 1,
        bond_address,
        bond_deployed,
        bonded_stake_wei: bonded_stake.to_string(),
        has_validator,
        unlock_block,
        is_unlocked,
        is_kyc_verified,
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
