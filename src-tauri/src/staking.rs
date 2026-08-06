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

/// 4-byte selector for `requestWithdrawal(uint256)` —
/// `keccak256("requestWithdrawal(uint256)")[..4]`. PINNED here + proven by the
/// `request_withdrawal_selector_is_keccak_of_signature` drift test (Rule 11).
/// Grounded in `LiquidStakingPool.sol:157` (WITHDRAW_WP2_SCOPE.md).
///
/// NOTE (deviation from scope): WITHDRAW_WP2_SCOPE.md's selector table lists
/// `0xdf5dd1a5`, but that is NOT the keccak of `requestWithdrawal(uint256)` (an
/// independent keccak derivation — the very drift test the scope mandates — gives
/// `0x9ee679e8`). The pinned value is the keccak-correct one; the scope's table
/// value would have shipped a wrong selector (a reverting / wrong-function tx —
/// @rule8). See the drift test + the evidence note.
const REQUEST_WITHDRAWAL_SELECTOR: [u8; 4] = [0x9e, 0xe6, 0x79, 0xe8];

/// 4-byte selector for `claimWithdrawal(uint256)` —
/// `keccak256("claimWithdrawal(uint256)")[..4]`. PINNED + drift-tested. Grounded
/// in `LiquidStakingPool.sol:176`. DEVIATION: scope listed `0x423b176f`; the
/// keccak-correct value is `0xf8444436` (see the drift test / evidence note).
const CLAIM_WITHDRAWAL_SELECTOR: [u8; 4] = [0xf8, 0x44, 0x44, 0x36];

/// 4-byte selector for the `withdrawals(uint256)` public-mapping getter —
/// `keccak256("withdrawals(uint256)")[..4]`. PINNED + drift-tested. Reads the
/// 5-field `WithdrawalRequest` tuple (`LiquidStakingPool.sol:53`). DEVIATION: scope
/// listed `0xf39c38a0`; the keccak-correct value is `0x5cc07076`.
const WITHDRAWALS_SELECTOR: [u8; 4] = [0x5c, 0xc0, 0x70, 0x76];

/// 4-byte selector for the `shares(address)` public-mapping getter —
/// `keccak256("shares(address)")[..4]`. PINNED + drift-tested. Reads the caller's
/// raw stSALT shares (`LiquidStakingPool.sol:57`), used for the SALT→shares
/// proportional conversion. (This is the one scope value that IS keccak-correct.)
const SHARES_SELECTOR: [u8; 4] = [0xce, 0x7c, 0x2a, 0xc2];

/// 4-byte selector for `getSharePrice()` — `keccak256("getSharePrice()")[..4]`.
/// PINNED + drift-tested. Grounded in `LiquidStakingPool.sol:307`. Not on the
/// write path (we convert via `shares`/`balanceOf` for exactness), but pinned per
/// the scope so a signature drift trips the tripwire. DEVIATION: scope listed
/// `0xb3370044`; the keccak-correct value is `0x5b1dac60`.
const GET_SHARE_PRICE_SELECTOR: [u8; 4] = [0x5b, 0x1d, 0xac, 0x60];

/// `WITHDRAWAL_DELAY` in blocks — `LiquidStakingPool.sol:26` (~7 days at ~12s
/// blocks). A claim is gated until `requestBlock + WITHDRAWAL_DELAY`. PINNED; used
/// only to compute the human-facing `claimableAtBlock` (the contract enforces the
/// real gate on-chain — we never fabricate an early claim).
pub const WITHDRAWAL_DELAY: u64 = 50_400;

/// Explicit gas limit for the `requestWithdrawal(uint256)` tx. It burns shares +
/// pushes a queue entry (storage writes) — more than a deposit's share mint. A
/// calldata tx MUST carry explicit gas (finalize has no estimate path); ~150,000
/// is a safe ceiling.
const REQUEST_WITHDRAWAL_GAS: u64 = 150_000;

/// Explicit gas limit for the `claimWithdrawal(uint256)` tx. It flips a bool +
/// transfers SALT out; ~90,000 is a safe ceiling. MUST be explicit (finalize has
/// no estimate path).
const CLAIM_WITHDRAWAL_GAS: u64 = 90_000;

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

/// Selector for `requestWithdrawal(uint256)`.
pub fn request_withdrawal_selector() -> [u8; 4] {
    REQUEST_WITHDRAWAL_SELECTOR
}

/// Selector for `claimWithdrawal(uint256)`.
pub fn claim_withdrawal_selector() -> [u8; 4] {
    CLAIM_WITHDRAWAL_SELECTOR
}

/// Selector for the `withdrawals(uint256)` getter.
pub fn withdrawals_selector() -> [u8; 4] {
    WITHDRAWALS_SELECTOR
}

/// Selector for the `shares(address)` getter.
pub fn shares_selector() -> [u8; 4] {
    SHARES_SELECTOR
}

/// Selector for `getSharePrice()`.
pub fn get_share_price_selector() -> [u8; 4] {
    GET_SHARE_PRICE_SELECTOR
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

/// Build the `shares(address)` calldata: `selector ++ 32-byte left-padded
/// address` (36 bytes). Same word layout as `balanceOf`. `addr` must be validated.
fn encode_shares_calldata(addr: &str) -> Vec<u8> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(stripped).unwrap_or_default();
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&shares_selector());
    let mut word = [0u8; 32];
    if bytes.len() == 20 {
        word[12..32].copy_from_slice(&bytes);
    }
    calldata.extend_from_slice(&word);
    calldata
}

/// The `eth_call` object for `shares(addr)` on the pool: `{to, data}`.
fn shares_call(addr: &str) -> serde_json::Value {
    let calldata = encode_shares_calldata(addr);
    serde_json::json!({
        "to": LIQUID_STAKING_POOL,
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// Read `LiquidStakingPool.shares(addr)` (raw stSALT shares) on the live 40204 RPC
/// via `eth_call` and decode the `uint256`. Used for the SALT→shares conversion.
/// A fresh wallet honestly reads `0`. Data source: `shares(address)` (Rule 1).
pub fn read_self_shares<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    wallet_address: &str,
) -> Result<u128, StakingError> {
    let addr = validate_address(wallet_address)?;
    let ret = rpc
        .eth_call(shares_call(&addr))
        .map_err(|e| StakingError::Rpc(e.to_string()))?;
    decode_uint256_word(&ret)
}

/// Convert a SALT amount (wei) the user asked to withdraw into a stSALT
/// `shareAmount` to burn, from LIVE reads (Rule 1 — no fabricated conversion):
/// - `self_stake_salt` = `balanceOf(self)` (the SALT value of the user's shares);
/// - `self_shares` = `shares(self)` (the user's raw share count).
///
/// If the user asks for their whole self-stake (or more), we burn ALL their shares
/// exactly (`self_shares`) — no dust, no share-price rounding drift, and it can
/// never exceed `shares[self]` (avoids the "Insufficient shares" revert). Below
/// that, the burn is proportional: `shareAmount = amount * self_shares /
/// self_stake_salt`, floored (so it is always `<= self_shares`). The multiply uses
/// `u128 → U256`-equivalent (128×128 → 256) math via `u128::widening`-free
/// primitives to avoid overflow on large SALT amounts. Returns `Err` for the
/// empty-position / zero-result cases so the command surfaces an honest error
/// rather than building a reverting tx.
pub fn salt_to_share_amount(
    amount_wei: u128,
    self_stake_salt: u128,
    self_shares: u128,
) -> Result<u128, StakingError> {
    if amount_wei == 0 {
        return Err(StakingError::Decode(
            "withdraw amount must be greater than zero".to_string(),
        ));
    }
    if self_stake_salt == 0 || self_shares == 0 {
        return Err(StakingError::Decode(
            "no self-stake to withdraw".to_string(),
        ));
    }
    // Withdraw-all (or over) → burn every share exactly (no dust, no rounding).
    if amount_wei >= self_stake_salt {
        return Ok(self_shares);
    }
    // Proportional floor: amount * self_shares / self_stake_salt, computed in u256
    // to avoid overflow of the 128×128 product.
    let share_amount = mul_div_floor(amount_wei, self_shares, self_stake_salt);
    if share_amount == 0 {
        return Err(StakingError::Decode(
            "withdraw amount is below one share — nothing to withdraw".to_string(),
        ));
    }
    Ok(share_amount)
}

/// `floor(a * b / d)` computed in 256-bit precision so the intermediate `a * b`
/// (up to `u128::MAX²`) never overflows. `d` is non-zero (checked by the caller).
/// Uses `u128` halves — no external bigint dependency in this lean tree.
fn mul_div_floor(a: u128, b: u128, d: u128) -> u128 {
    // Full 256-bit product a*b as (hi, lo) 128-bit limbs.
    let (hi, lo) = mul_u128_full(a, b);
    // Long division of the 256-bit numerator (hi:lo) by the 128-bit divisor d.
    div_256_by_128(hi, lo, d)
}

/// 128×128 → 256-bit multiply, returned as `(hi, lo)` 128-bit limbs.
fn mul_u128_full(a: u128, b: u128) -> (u128, u128) {
    let a_lo = a & u64::MAX as u128;
    let a_hi = a >> 64;
    let b_lo = b & u64::MAX as u128;
    let b_hi = b >> 64;

    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;

    // Sum the cross terms with carry tracking.
    let mid = lh + (ll >> 64) + (hl & u64::MAX as u128);
    let lo = (ll & u64::MAX as u128) | (mid << 64);
    let hi = hh + (hl >> 64) + (mid >> 64);
    (hi, lo)
}

/// Divide a 256-bit numerator `(hi:lo)` by a 128-bit `d`, returning the 128-bit
/// quotient (floored). Assumes the quotient fits in `u128` (it always does here:
/// the numerator is `amount * self_shares` with `amount < self_stake_salt`, so the
/// quotient `< self_shares ≤ u128::MAX`). Simple bit-by-bit long division.
fn div_256_by_128(mut hi: u128, mut lo: u128, d: u128) -> u128 {
    let mut quotient: u128 = 0;
    let mut rem: u128 = 0;
    // Iterate the 256 bits from MSB to LSB.
    for _ in 0..256 {
        // rem = (rem << 1) | next_bit; next_bit = MSB of (hi:lo).
        let top = hi >> 127;
        rem = (rem << 1) | top;
        // Shift (hi:lo) left by 1.
        hi = (hi << 1) | (lo >> 127);
        lo <<= 1;
        // Shift quotient left; if rem >= d, subtract and set the low bit.
        quotient <<= 1;
        if rem >= d {
            rem -= d;
            quotient |= 1;
        }
    }
    quotient
}

/// Build a `selector ++ 32-byte uint256` calldata (36 bytes) for a single-uint
/// function (`requestWithdrawal`, `claimWithdrawal`, `withdrawals`). The argument
/// is right-aligned in the 32-byte word (ABI uint encoding).
fn encode_uint_calldata(selector: [u8; 4], arg: u128) -> Vec<u8> {
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&selector);
    let mut word = [0u8; 32];
    word[16..32].copy_from_slice(&arg.to_be_bytes());
    calldata.extend_from_slice(&word);
    calldata
}

/// Build the ceremony tx-JSON for a single-uint-arg pool call (`to` = the pool,
/// `value = 0` — these txs move no SALT via `msg.value`, the contract handles the
/// transfer), the given `data`, and the explicit `gas`. Same `{from,to,value,data,
/// gas,chainId}` object shape `txdecode::decode_transaction` consumes.
fn encode_pool_call_json(from: &str, data: &[u8], gas: u64) -> String {
    serde_json::json!({
        "from": from,
        "to": LIQUID_STAKING_POOL,
        "value": "0x0",
        "data": format!("0x{}", hex::encode(data)),
        "gas": format!("0x{gas:x}"),
        "chainId": format!("0x{CITRATE_CHAIN_ID:x}"),
    })
    .to_string()
}

/// The ceremony tx-JSON for `requestWithdrawal(shareAmount)`.
fn encode_request_withdrawal_json(from: &str, share_amount: u128) -> String {
    let data = encode_uint_calldata(request_withdrawal_selector(), share_amount);
    encode_pool_call_json(from, &data, REQUEST_WITHDRAWAL_GAS)
}

/// The ceremony tx-JSON for `claimWithdrawal(requestId)`.
fn encode_claim_withdrawal_json(from: &str, request_id: u128) -> String {
    let data = encode_uint_calldata(claim_withdrawal_selector(), request_id);
    encode_pool_call_json(from, &data, CLAIM_WITHDRAWAL_GAS)
}

/// A pending (unclaimed) withdrawal surfaced to the Staking tab, all fields from
/// live on-chain state (Rule 1). Serde camelCase for the bridge.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PendingWithdrawal {
    /// The withdrawal request id (the `requestId` returned by `requestWithdrawal`
    /// and the arg `claimWithdrawal` takes). Decimal string.
    #[serde(rename = "id")]
    pub id: String,
    /// The SALT payout in wei, from the on-chain `WithdrawalRequest.saltAmount`.
    #[serde(rename = "saltWei")]
    pub salt_wei: String,
    /// The block the withdrawal was requested in (`WithdrawalRequest.requestBlock`).
    #[serde(rename = "requestBlock")]
    pub request_block: u64,
    /// The block at/after which the claim is allowed (`requestBlock +
    /// WITHDRAWAL_DELAY`). The contract enforces this on-chain; this is the honest
    /// display value.
    #[serde(rename = "claimableAtBlock")]
    pub claimable_at_block: u64,
    /// Whether the claim is allowed as of the current head (`currentBlock >=
    /// claimableAtBlock`). The UI enables Claim only when true.
    #[serde(rename = "claimable")]
    pub claimable: bool,
}

/// Decode the 5-word `withdrawals(id)` tuple return `(address staker, uint256
/// shareAmount, uint256 saltAmount, uint256 requestBlock, bool claimed)`. Returns
/// `(staker_lower_hex, salt_amount_wei, request_block, claimed)` — the fields the
/// pending list needs. Refuses to truncate a `saltAmount`/`requestBlock` beyond
/// `u128`/`u64` (Rule 1). A short return errors.
#[allow(clippy::type_complexity)]
fn decode_withdrawal_tuple(ret: &[u8]) -> Result<(String, u128, u64, bool), StakingError> {
    if ret.len() < 160 {
        return Err(StakingError::Decode(format!(
            "expected a 5-word (160-byte) withdrawals tuple, got {} bytes",
            ret.len()
        )));
    }
    // Word 0: staker address (low 20 bytes of the first 32-byte word).
    let staker = format!("0x{}", hex::encode(&ret[12..32]));
    // Word 2: saltAmount (bytes 64..96).
    let salt_amount = decode_uint256_word(&ret[64..96])?;
    // Word 3: requestBlock (bytes 96..128) — a block height fits in u64.
    let request_block_u128 = decode_uint256_word(&ret[96..128])?;
    let request_block = u64::try_from(request_block_u128)
        .map_err(|_| StakingError::Decode("requestBlock exceeds u64".to_string()))?;
    // Word 4: claimed bool (bytes 128..160) — non-zero low byte == true.
    let claimed = ret[128..160].iter().any(|&b| b != 0);
    Ok((staker, salt_amount, request_block, claimed))
}

/// Left-pad a validated 20-byte address into a 32-byte topic word (`0x…64 hex`),
/// the shape an indexed `address` event topic takes.
fn address_topic(addr: &str) -> String {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    let bytes = hex::decode(stripped).unwrap_or_default();
    let mut word = [0u8; 32];
    if bytes.len() == 20 {
        word[12..32].copy_from_slice(&bytes);
    }
    format!("0x{}", hex::encode(word))
}

/// `keccak256("WithdrawalRequested(uint256,address,uint256,uint256)")` — the
/// topic0 the pending enumeration filters on. Grounded in
/// `LiquidStakingPool.sol:101` (WITHDRAW_WP2_SCOPE.md). PINNED here + proven by the
/// `withdrawal_requested_topic0_is_keccak_of_signature` drift test (Rule 11).
const WITHDRAWAL_REQUESTED_TOPIC0: &str =
    "0xe6d14ce42ad5a4efe0f111f81c1a1123db4ee41f561c3161e0960d54a9221ebe";

/// The canonical `WithdrawalRequested` event signature (proven-against by the
/// drift test; the topic0 const above is the pinned keccak of THIS string).
pub const WITHDRAWAL_REQUESTED_SIG: &str = "WithdrawalRequested(uint256,address,uint256,uint256)";

/// **Enumerate a wallet's PENDING withdrawals** from live chain state (Rule 1).
/// 1. `getLogs` for `WithdrawalRequested` with `topic0 = keccak(sig)` and `topic2
///    = staker` (indexed `address`), over the full range — ids come from `topic1`.
/// 2. For each id, `withdrawals(id)` `eth_call` → the 5-word tuple; skip any whose
///    `claimed == true` (EXCLUDE claimed) or whose `staker` does not match (belt +
///    braces against a topic filter miss).
/// 3. Read `block_number()` once → `claimable = currentBlock >= requestBlock +
///    WITHDRAWAL_DELAY`.
///
/// A fresh wallet with no requests honestly returns an empty list. `rpc` is
/// injected so tests script a mock transport.
pub fn read_pending_withdrawals<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    wallet_address: &str,
) -> Result<Vec<PendingWithdrawal>, StakingError> {
    let addr = validate_address(wallet_address)?;
    let staker_topic = address_topic(&addr);
    // topic2 = staker (the SECOND indexed arg → topics[2]); topic1 (id) left null.
    let filter = serde_json::json!({
        "address": LIQUID_STAKING_POOL,
        "topics": [
            WITHDRAWAL_REQUESTED_TOPIC0,
            serde_json::Value::Null,
            staker_topic,
        ],
        "fromBlock": "earliest",
        "toBlock": "latest",
    });
    let logs = rpc
        .get_logs(filter)
        .map_err(|e| StakingError::Rpc(e.to_string()))?;

    // Current head, read once, for the claimable gate.
    let current_block = rpc
        .block_number()
        .map_err(|e| StakingError::Rpc(e.to_string()))?;

    let mut out = Vec::new();
    for log in &logs {
        // topic1 = the indexed id (uint256).
        let id_topic = match log.topics.get(1) {
            Some(t) => t,
            None => continue, // malformed log — skip rather than fabricate an id
        };
        let id = decode_uint256_topic(id_topic)?;

        // Read the authoritative on-chain tuple (the log only carries the request-
        // time facts; `claimed` can flip after the event, so we re-read state).
        let ret = rpc
            .eth_call(withdrawals_call(id))
            .map_err(|e| StakingError::Rpc(e.to_string()))?;
        let (staker, salt_wei, request_block, claimed) = decode_withdrawal_tuple(&ret)?;

        if claimed {
            continue; // EXCLUDE claimed
        }
        if !staker.eq_ignore_ascii_case(&addr) {
            continue; // defensive: not this staker (topic filter miss)
        }

        let claimable_at_block = request_block.saturating_add(WITHDRAWAL_DELAY);
        out.push(PendingWithdrawal {
            id: id.to_string(),
            salt_wei: salt_wei.to_string(),
            request_block,
            claimable_at_block,
            claimable: current_block >= claimable_at_block,
        });
    }
    Ok(out)
}

/// The `eth_call` object for `withdrawals(id)` on the pool: `{to, data}`.
fn withdrawals_call(id: u128) -> serde_json::Value {
    let calldata = encode_uint_calldata(withdrawals_selector(), id);
    serde_json::json!({
        "to": LIQUID_STAKING_POOL,
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// Decode a 32-byte topic word (`0x…64 hex`) to a `u128` (the indexed id). Refuses
/// a value beyond `u128` (an id far larger than the sequential counter can reach —
/// a malformed topic) rather than truncate (Rule 1).
fn decode_uint256_topic(topic: &str) -> Result<u128, StakingError> {
    let stripped = topic.strip_prefix("0x").unwrap_or(topic);
    let bytes = hex::decode(stripped)
        .map_err(|_| StakingError::Decode(format!("topic not hex: {topic}")))?;
    decode_uint256_word(&bytes)
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
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let intent = SignatureIntent {
        origin: LOCAL_USER_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: CITRATE_CHAIN_ID,
        raw: encode_stake_json(&wallet.address, value_wei),
    };
    // Store PENDING + return the decoded view; the human approves via B1.4.
    Ok(ceremony.0.request(intent))
}

/// `wallet_request_withdrawal` command (@rule8) — build a `requestWithdrawal(
/// shareAmount)` ceremony that burns the caller's stSALT shares into the ~7-day
/// queue. `amount_wei` is the SALT the user wants to withdraw; the shareAmount is
/// converted from LIVE reads (`shares(self)` + `balanceOf(self)`) so the number is
/// never fabricated (Rule 1) and can never exceed the caller's shares (avoids the
/// "Insufficient shares" revert). Signs NOTHING — the human approves via
/// `sign_and_broadcast` (B1.4). Requires the vault UNLOCKED to read the caller's
/// public address; a locked/absent vault fails closed. Mirrors `wallet_stake`.
#[tauri::command]
pub fn wallet_request_withdrawal(
    amount_wei: String,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<CeremonyView, String> {
    let amount: u128 = amount_wei
        .parse()
        .map_err(|_| "staking: withdraw amount is not a u128 wei value".to_string())?;
    if amount == 0 {
        return Err("staking: withdraw amount must be greater than zero".to_string());
    }
    // The staker = THIS vault's wallet (public address only; never the key).
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    // Live reads: the caller's self-stake SALT (balanceOf) + raw shares (shares).
    let rpc = crate::rpc::RpcClient::citrate();
    let self_stake_salt = read_self_stake(&rpc, &wallet.address).map_err(|e| e.to_string())?;
    let self_shares = read_self_shares(&rpc, &wallet.address).map_err(|e| e.to_string())?;
    let share_amount =
        salt_to_share_amount(amount, self_stake_salt, self_shares).map_err(|e| e.to_string())?;
    let intent = SignatureIntent {
        origin: LOCAL_USER_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: CITRATE_CHAIN_ID,
        raw: encode_request_withdrawal_json(&wallet.address, share_amount),
    };
    Ok(ceremony.0.request(intent))
}

/// `wallet_claim_withdrawal` command (@rule8) — build a `claimWithdrawal(
/// requestId)` ceremony that pays out a matured withdrawal. The contract enforces
/// the `requestBlock + WITHDRAWAL_DELAY` gate on-chain (a too-early claim reverts);
/// this command builds the intent only. Signs NOTHING — the human approves via
/// `sign_and_broadcast`. `request_id` is a decimal id (from the pending list).
#[tauri::command]
pub fn wallet_claim_withdrawal(
    request_id: String,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<CeremonyView, String> {
    let id: u128 = request_id
        .parse()
        .map_err(|_| "staking: request id is not a u128 value".to_string())?;
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let intent = SignatureIntent {
        origin: LOCAL_USER_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: CITRATE_CHAIN_ID,
        raw: encode_claim_withdrawal_json(&wallet.address, id),
    };
    Ok(ceremony.0.request(intent))
}

/// `wallet_pending_withdrawals` command — read the vault wallet's PENDING
/// withdrawals from live chain state (`getLogs` + `withdrawals(id)` +
/// `block_number`). Requires the vault UNLOCKED (to read the public address; the
/// key is never touched). A fresh wallet honestly returns an empty list. Mirrors
/// `wallet_balances`.
#[tauri::command]
pub fn wallet_pending_withdrawals(
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<Vec<PendingWithdrawal>, String> {
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let rpc = crate::rpc::RpcClient::citrate();
    read_pending_withdrawals(&rpc, &wallet.address).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("staking_tests.rs");
}
