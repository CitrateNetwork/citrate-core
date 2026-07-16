//! citrate-core — LiquidStakingPool: the real self-stake read + the `wallet_stake`
//! (deposit) command (@rule8 — a value-bearing write; it signs NOTHING).
//!
//! A user "Add stake" forwards their OWN SALT into the deployed `LiquidStakingPool`
//! via `deposit()` (payable), minting stSALT shares. This is the wallet
//! liquid-staking money action — distinct from validator onboarding: the pool has
//! no proposer-pubkey registry and no validator-tied slashing (slashing is
//! socialized via share price). Validator-set registration is a separate subsystem
//! (WO-1.3 `[dgx]` / Phase 2 `[core]`), untouched here.
//!
//! Model: this mirrors `transfer.rs::wallet_send` (the ceremony money-write) and
//! `earnings.rs::read_claimable` (the `eth_call` read). The command builds the
//! ceremony [`SignatureIntent`] for the deposit and submits it as a PENDING
//! request — it returns only the decoded [`CeremonyView`]; the human then approves
//! via the ceremony's own `sign_and_broadcast` (B1.4), which fetches nonce+gas
//! from the live 40204 RPC, signs the real EIP-155 tx with the vault key, and
//! broadcasts. Signing happens nowhere else (Rule 3).
//!
//! HONESTY (Rule 1) / @rule8: `wallet_stake` reads only the wallet's PUBLIC address
//! (never key material — `wallet::address` derives in-process and zeroizes), and
//! returns only the decoded pending view. No signing here; a locked vault fails
//! closed. `read_self_stake` is a public `eth_call` read (no key path).
//!
//! ## Grounded contract ABI (citrate-chain/contracts/src/LiquidStakingPool.sol)
//! - `function deposit() external payable returns (uint256 sharesOut)` — stakes
//!   `msg.value` SALT, mints shares. Open to any wallet (no access control).
//! - `function balanceOf(address staker) external view returns (uint256)` — the
//!   SALT value of the caller's OWN shares (`_saltForShares(shares[staker])`). The
//!   membership-granted 32k is held by the MembershipStakeVault, NOT in the user's
//!   `shares[]` — so this reads the user's SELF-stake only (the UI adds the vaulted
//!   grant separately). A fresh wallet honestly reads `0`.
//! - Withdraw is the two-step `requestWithdrawal(uint256 shares)` +
//!   `claimWithdrawal(uint256 id)` after `WITHDRAWAL_DELAY = 50400` blocks (~7d).
//!   That is a separate WP (SALT→shares conversion + queue) — NOT wired here.
//!
//! ## Address (canonical book)
//! `LiquidStakingPool = 0xfd272195b55cb4f5a240a5be75aabab0d1c5685e` on 40204
//! (`contracts/addresses/40204.json` AND the node-agent's
//! `crates/chainio/src/generated/addresses.json` — cross-checked).

// The staking reader/selectors are consumed by `earnings::wallet_balances` (the
// self-stake read) and the `wallet_stake` command; some helpers are only reached
// by tests until the full connector round-trip lands (mirrors earnings.rs).
#![allow(dead_code)]

use crate::ceremony::{CeremonyView, IntentKind, SignatureIntent};

/// The `LiquidStakingPool` contract on 40204 (canonical address book:
/// `contracts/addresses/40204.json` + node-agent `generated/addresses.json`).
/// Lowercase `0x`-hex; the `eth_call` target for `balanceOf` and the `to` of the
/// `deposit()` stake tx.
pub const LIQUID_STAKING_POOL: &str = "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e";

/// The Citrate chain id (40204).
const CITRATE_CHAIN_ID: u64 = 40204;

/// 4-byte selector for `deposit()` (no args) — `keccak256("deposit()")[..4]`. The
/// canonical WETH-style deposit selector; PINNED here and proven by the
/// `deposit_selector_is_keccak_of_signature` drift test (Rule 11), so this lean
/// tree needs no runtime keccak.
const DEPOSIT_SELECTOR: [u8; 4] = [0xd0, 0xe3, 0x0d, 0xb0];

/// 4-byte selector for `balanceOf(address)` — `keccak256("balanceOf(address)")[..4]`.
/// The canonical ERC-20/721 `balanceOf`; PINNED + proven by the drift test.
const BALANCE_OF_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

/// Explicit gas limit for the `deposit()` stake tx. A native transfer's 21,000 is
/// not enough — `deposit()` does share math + storage writes (measured ~80–120k).
/// 200,000 is a safe ceiling. It MUST be explicit: a calldata tx that omits gas
/// makes `txdecode::finalize` return `None` (there is no estimate path), so the
/// ceremony would reject it as undecodable. Mirrors `transfer.rs`'s fixed 21,000.
const DEPOSIT_GAS: u64 = 200_000;

/// The origin surfaced to the human for a user-initiated stake (displayed verbatim,
/// same as a user Send).
const LOCAL_USER_ORIGIN: &str = "local-user";

/// Selector for `deposit()` (no args).
pub fn deposit_selector() -> [u8; 4] {
    DEPOSIT_SELECTOR
}

/// Selector for `balanceOf(address)` (the self-stake read).
pub fn balance_of_selector() -> [u8; 4] {
    BALANCE_OF_SELECTOR
}

/// Errors from the staking reader/intent builder. Coarse + secret-free (this
/// module never sees key material — it does a public `eth_call` read and builds
/// an unsigned deposit intent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StakingError {
    /// The address was not a `0x`-prefixed 20-byte hex string.
    BadAddress(String),
    /// The live RPC read failed (transport / node error / missing field).
    Rpc(String),
    /// The `eth_call` return did not decode to a `uint256` (short, or beyond u128 —
    /// SALT amounts fit in u128, so a larger value means the return is not a
    /// balance and we refuse to truncate).
    Decode(String),
}

impl std::fmt::Display for StakingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StakingError::BadAddress(m) => write!(f, "staking: bad address: {m}"),
            StakingError::Rpc(m) => write!(f, "staking: rpc error: {m}"),
            StakingError::Decode(m) => write!(f, "staking: balance decode error: {m}"),
        }
    }
}

impl std::error::Error for StakingError {}

/// Validate a `0x`-prefixed 20-byte hex address; return the lowercased canonical
/// form (fail closed rather than call/transfer with a malformed address).
fn validate_address(addr: &str) -> Result<String, StakingError> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    if stripped.len() != 40 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(StakingError::BadAddress(addr.to_string()));
    }
    Ok(format!("0x{}", stripped.to_ascii_lowercase()))
}

/// Build the `balanceOf(address)` calldata: `selector ++ 32-byte left-padded
/// address` (36 bytes). `addr` must already be validated.
fn encode_balance_of_calldata(addr: &str) -> Vec<u8> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(stripped).unwrap_or_default();
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&balance_of_selector());
    let mut word = [0u8; 32];
    if bytes.len() == 20 {
        word[12..32].copy_from_slice(&bytes);
    }
    calldata.extend_from_slice(&word);
    calldata
}

/// The `eth_call` object for `balanceOf(addr)` on the pool: `{to, data}`.
fn balance_of_call(addr: &str) -> serde_json::Value {
    let calldata = encode_balance_of_calldata(addr);
    serde_json::json!({
        "to": LIQUID_STAKING_POOL,
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// Decode a 32-byte `uint256` `eth_call` return to a `u128` (wei of SALT). A SALT
/// amount fits in u128, so a value in the high 16 bytes means the return is NOT a
/// plain balance — we refuse to truncate (Rule 1) and error. A short return errors.
pub fn decode_uint256_word(ret: &[u8]) -> Result<u128, StakingError> {
    if ret.len() < 32 {
        return Err(StakingError::Decode(format!(
            "expected a 32-byte uint256 word, got {} bytes",
            ret.len()
        )));
    }
    if ret[0..16].iter().any(|&b| b != 0) {
        return Err(StakingError::Decode(
            "balance exceeds u128 — refusing to truncate".to_string(),
        ));
    }
    let mut low = [0u8; 16];
    low.copy_from_slice(&ret[16..32]);
    Ok(u128::from_be_bytes(low))
}

/// **The real self-stake read.** Read `LiquidStakingPool.balanceOf(addr)` on the
/// live 40204 RPC via `eth_call` and decode the `uint256` to wei of SALT — the
/// user's OWN pool position (the vaulted membership grant is held by the vault and
/// is NOT in this value). Rule 1: the number comes from chain, not a sim; a fresh
/// wallet with no self-stake honestly reads `0`. `rpc` is injected so tests script
/// a mock transport; production wires [`crate::rpc::RpcClient::citrate`].
pub fn read_self_stake<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    wallet_address: &str,
) -> Result<u128, StakingError> {
    let addr = validate_address(wallet_address)?;
    let ret = rpc
        .eth_call(balance_of_call(&addr))
        .map_err(|e| StakingError::Rpc(e.to_string()))?;
    decode_uint256_word(&ret)
}

/// Build the ceremony tx-JSON for a `deposit()` stake: `to` = the pool, `value` =
/// the staked SALT (forwarded as `msg.value`), `data` = the `deposit()` selector
/// (no args), `gas` = the explicit [`DEPOSIT_GAS`]. Same `{from,to,value,data,gas,
/// chainId}` object shape `txdecode::decode_transaction` consumes (`0x`-hex
/// quantities). Returned as a JSON string — for a `Transaction` intent the ceremony
/// reads `raw` as JSON.
fn encode_stake_json(from: &str, value_wei: u128) -> String {
    let data = format!("0x{}", hex::encode(deposit_selector()));
    serde_json::json!({
        "from": from,
        "to": LIQUID_STAKING_POOL,
        "value": format!("0x{value_wei:x}"),
        "data": data,
        "gas": format!("0x{DEPOSIT_GAS:x}"),
        "chainId": format!("0x{CITRATE_CHAIN_ID:x}"),
    })
    .to_string()
}

/// `wallet_stake` command — submit a `deposit()` stake of `amount_wei` SALT to the
/// `LiquidStakingPool` as a PENDING ceremony and return the decoded view for human
/// approval. Signs NOTHING (the human approves via `sign_and_broadcast`). Requires
/// the vault UNLOCKED to read the sender's public address; a locked/absent vault
/// fails closed. A zero/garbage amount is rejected before any ceremony state is
/// created. The staked SALT is forwarded as `msg.value` (the deposit is payable).
#[tauri::command]
pub fn wallet_stake(
    amount_wei: String,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<CeremonyView, String> {
    let value_wei: u128 = amount_wei
        .parse()
        .map_err(|_| "staking: amount is not a u128 wei value".to_string())?;
    if value_wei == 0 {
        return Err("staking: stake amount must be greater than zero".to_string());
    }
    // The staker = THIS vault's wallet (public address only; never the key).
    let wallet = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;
    let intent = SignatureIntent {
        origin: LOCAL_USER_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: CITRATE_CHAIN_ID,
        raw: encode_stake_json(&wallet.address, value_wei),
    };
    // Store PENDING + return the decoded view; the human approves via B1.4.
    Ok(ceremony.0.request(intent))
}

#[cfg(test)]
mod tests {
    include!("staking_tests.rs");
}
