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

/// The `LiquidStakingPool` contract on 40204, from the generated address book
/// (`crate::addresses`, synced from citrate-chain `contracts/addresses/40204.json`).
/// Lowercase `0x`-hex; the `eth_call` target for `balanceOf` and the `to` of the
/// `deposit()` stake tx.
pub fn liquid_staking_pool() -> &'static str {
    crate::addresses::liquid_staking_pool()
}

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
    /// C1-W: the time budget ran out before a node read outside the paging loop.
    OutOfBudget,
    /// C1-W: the node kept answering -32002 ("retry") for one window after the bounded back-off.
    Indexing(String),
}

impl std::fmt::Display for StakingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StakingError::BadAddress(m) => write!(f, "staking: bad address: {m}"),
            StakingError::Rpc(m) => write!(f, "staking: rpc error: {m}"),
            StakingError::Decode(m) => write!(f, "staking: balance decode error: {m}"),
            StakingError::Indexing(m) => write!(f, "still indexing chain history ({m})"),
            StakingError::OutOfBudget => write!(
                f,
                "still reading chain history: the time budget ran out before the node was read"
            ),
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
        "to": liquid_staking_pool(),
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
        "to": liquid_staking_pool(),
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
        "to": liquid_staking_pool(),
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
    read_pending_withdrawals_with(rpc, wallet_address, &mut WithdrawalScan::default(), None)
}

/// C1-W — the block the `LiquidStakingPool` was created in on 40204: block 427 in
/// citrate-chain `contracts/addresses/40204.provenance.json` (the on-chain deployer-tx ledger of
/// the 2026-10-05 reroll). The generated address book carries no deployment block, so it is pinned
/// here; a reroll that moves the pool moves this too (the book's address pin fails first).
pub const LIQUID_STAKING_POOL_DEPLOY_BLOCK: u64 = 427;

/// C1-W — the widest `eth_getLogs` window, in blocks (inclusive, `toBlock - fromBlock = 999`).
/// Matches citrate-explorer `MAX_LOG_CHUNK`. Today's node silently clamps a wider range to its first
/// 1000 blocks; citrate-chain#334 answers -32005 instead.
pub const LOG_WINDOW_BLOCKS: u64 = 1000;

/// C1-W (citrate-chain#337) — how far below min(h1, h2) of `citrate_executedHead` progress is
/// committed: a PERSISTENCE margin, not a reorg bound. It covers the two small gaps the protocol
/// knows of:
/// - the head is not atomic with `eth_getLogs` (h1 = 5 was seen while getLogs served only up to 4);
/// - the producer writes receipts AFTER it commits the tip, so the newest blocks can be executed
///   but not yet served.
///
/// Both are one or a few blocks. 100 is also the chain's MAX_REORG_DEPTH (the in-memory rollback)
/// and ghostdag finality depth, but it does NOT bound a reorg: a deeper fork goes through the spine
/// rebuild, bounded only by the finalized floor, which is 0 while checkpoint finality is off.
/// Reorgs are handled by the anchor check (`citrate_isExecuted`), not by this margin.
pub const EXECUTED_HEAD_MARGIN_BLOCKS: u64 = 100;

/// C1-W — how long before the deadline the log paging stops, so the second executed-head read
/// (h2) still fits: one RPC timeout (6 s) plus a second. Only when the node serves the head.
pub const H2_RESERVE: std::time::Duration = std::time::Duration::from_secs(7);

/// C1-W — how many `withdrawals(id)` reads go in one JSON-RPC batch.
pub const WITHDRAWAL_READ_BATCH: usize = 50;

/// C1-W — how many times one window is asked for when the node answers -32002 ("retry").
pub const LOGS_RETRY_ATTEMPTS: u32 = 5;

/// C1-W — the wait before retry `attempt` (0-based): 250 ms doubling, capped at 2 s.
pub fn logs_retry_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis((250u64 << attempt.min(3)).min(2_000))
}

/// C1-W — load the saved scan for `key` from `file` (a missing or unreadable file starts over).
pub fn load_scan(file: &std::path::Path, key: &str) -> WithdrawalScan {
    std::fs::read(file)
        .ok()
        .and_then(|b| {
            serde_json::from_slice::<std::collections::HashMap<String, WithdrawalScan>>(&b).ok()
        })
        .and_then(|mut m| m.remove(key))
        .unwrap_or_default()
}

/// C1-W (#371 review) — two scans of the same key, merged so progress never goes back. A newer
/// generation (a rewind after a reorg) wins the position and the anchor; within one generation the
/// larger position wins. Ids are kept from both: each is read from contract state on every pass
/// before it is listed, so no saved state can hide a withdrawal. The session-only state comes from
/// `b`, the newer of the two.
pub fn merge_scans(a: &WithdrawalScan, b: &WithdrawalScan) -> WithdrawalScan {
    let lead = if a.generation != b.generation {
        if a.generation > b.generation {
            a
        } else {
            b
        }
    } else if a.next_block >= b.next_block {
        a
    } else {
        b
    };
    WithdrawalScan {
        next_block: lead.next_block,
        anchor: lead.anchor.clone(),
        generation: lead.generation,
        ids: a.ids.union(&b.ids).copied().collect(),
        session: b.session.clone(),
        last_read_confirmed: false,
        last_read_complete: false,
        last_read_note: None,
    }
}

/// C1-W — save the scan for `key` into `file` (public chain facts only: a block, request ids and
/// the anchor), merged with what is saved ([`merge_scans`]), one save at a time,
/// written to a temp file and renamed over the old one so a reader never sees a partial file. The
/// session-only state is never saved.
pub fn save_scan(file: &std::path::Path, key: &str, scan: &WithdrawalScan) -> std::io::Result<()> {
    static SAVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one_at_a_time = SAVE.lock().unwrap_or_else(|e| e.into_inner());
    let mut all: std::collections::HashMap<String, WithdrawalScan> = std::fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let merged = match all.get(key) {
        Some(saved) => merge_scans(saved, scan),
        None => scan.clone(),
    };
    all.insert(key.to_string(), merged);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_vec(&all).map_err(std::io::Error::other)?;
    let tmp = file.with_extension(format!("json.tmp-{}", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// C1-W — one `withdrawals(id)` read: the request facts and the claim flag at `latest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalDetail {
    pub staker: String,
    pub salt_wei: u128,
    pub request_block: u64,
    pub claimed: bool,
}

/// C1-W — the session-only part of a scan: kept in memory for this app session, NEVER saved, and
/// reset on app restart (owner decision, DGX B1). It lets reads continue across calls when the node
/// gives no evidence of how far it executed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanSession {
    /// The first block not yet read in this session (0 = none).
    pub next_block: u64,
    /// The detail pass in progress: ids still to read, and the results read so far.
    pub pass_remaining: Vec<u128>,
    pub pass_results: std::collections::BTreeMap<u128, WithdrawalDetail>,
    /// The last COMPLETE detail pass, shown for ids not yet re-read in the current pass.
    pub last_details: std::collections::BTreeMap<u128, WithdrawalDetail>,
}

/// C1-W — what the paged scan has read for one staker. The saved part (position, ids, anchor,
/// generation) only advances on evidence of what the node executed; the session part
/// carries reads forward within this app session.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalScan {
    /// The first block not yet committed (0 = nothing committed yet). Never past
    /// min(h1, h2) - EXECUTED_HEAD_MARGIN_BLOCKS of `citrate_executedHead`.
    pub next_block: u64,
    /// Every `WithdrawalRequested` id seen for this staker.
    pub ids: std::collections::BTreeSet<u128>,
    /// The executed head `(number, hash)` the position was committed against, checked on the next
    /// read with `citrate_isExecuted` (or, on a node without it, against the executed head).
    #[serde(default)]
    pub anchor: Option<(u64, String)>,
    /// Bumped by every rewind, so a merge never undoes a rewind with older, further progress.
    #[serde(default)]
    pub generation: u64,
    /// Session-only (never saved).
    #[serde(skip)]
    pub session: ScanSession,
    /// The LAST read reached the head the node reported (`eth_blockNumber`) with every withdrawal
    /// read, on evidence of what the node executed (citrate_executedHead read twice, the anchor
    /// checked). Status of one read, never saved.
    #[serde(skip)]
    pub last_read_confirmed: bool,
    /// The LAST read reached the end of what the node serves and finished every withdrawal read.
    #[serde(skip)]
    pub last_read_complete: bool,
    /// Why the last read is not confirmed, in words for the Wallet.
    #[serde(skip)]
    pub last_read_note: Option<String>,
}

impl WithdrawalScan {
    /// RED STUB (C1-W, replaced in the fix commit): the last persisted `through`. Keeps the old
    /// behaviour: this scan has no served-through cursor.
    pub fn cursor(&self) -> Option<(u64, String)> {
        None
    }

    /// Rewind after a reorg: start again at the deployment block, forget the anchor and the
    /// session cursor, and bump the generation.
    fn rewind(&mut self) {
        self.next_block = LIQUID_STAKING_POOL_DEPLOY_BLOCK;
        self.anchor = None;
        self.session = ScanSession::default();
        self.generation += 1;
    }
}

/// [`read_pending_withdrawals`], continuing `scan`, following the citrate-chain#337 scanner
/// protocol:
/// 1. h1 = `citrate_executedHead`; the anchor is checked with `citrate_isExecuted` (on a node
///    without it, -32601: a rewind when h1 is below the anchor, or at its height with another
///    hash). Any other answer that isn't evidence means no advance this read.
/// 2. `eth_getLogs` in windows of at most [`LOG_WINDOW_BLOCKS`] up to min(eth_blockNumber, h1),
///    from the committed position; -32005 halves the window, a retryable -32002 retries it. Only
///    when nothing can be committed this read does the session cursor (less the margin) move the
///    start on: its coverage was read without validated evidence, so it is never committed.
/// 3. h2 = `citrate_executedHead` again; commit at most min(h1, h2) - EXECUTED_HEAD_MARGIN_BLOCKS
///    (nothing when h1 and h2 disagree at one height, or h2 can't be read).
/// 4. Every id is read from contract state with `withdrawals(id)` on every pass, in batches,
///    resuming across calls. A claim is never stored: an `eth_call` at `latest` is not bound to a
///    validated block, so a claim it shows can be on a branch that is later replaced.
///
/// Without `citrate_executedHead` (-32601, today's public RPC), or with an error, there is no
/// evidence: nothing is committed (eth_syncing is NOT evidence, owner decision), the session cursor
/// still carries the read forward, and the rows found are returned with `last_read_confirmed`
/// false. A spent `deadline` ends the read early with the rows found so far
/// (`last_read_complete` false); a failed window is an error naming it.
pub fn read_pending_withdrawals_with<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    wallet_address: &str,
    scan: &mut WithdrawalScan,
    deadline: Option<std::time::Instant>,
) -> Result<Vec<PendingWithdrawal>, StakingError> {
    let addr = validate_address(wallet_address)?;
    let staker_topic = address_topic(&addr);
    scan.last_read_confirmed = false;
    scan.last_read_complete = false;
    scan.last_read_note = None;
    // Every node read outside the paging loop honours the deadline too.
    let spent = || deadline.is_some_and(|d| std::time::Instant::now() >= d);

    // Current head, read once: the scan's end and the claimable gate.
    if spent() {
        return Err(StakingError::OutOfBudget);
    }
    let current_block = rpc
        .block_number()
        .map_err(|e| StakingError::Rpc(e.to_string()))?;

    // 1. h1, then the anchor check.
    if spent() {
        return Err(StakingError::OutOfBudget);
    }
    let executed = rpc.executed_head();
    let h1 = match &executed {
        Ok(Some(h)) => Some(h.clone()),
        _ => None,
    };
    let mut evidence = h1.is_some();
    let mut note: Option<String> = None;
    if let Some((ah, ahash)) = scan.anchor.clone() {
        if spent() {
            return Err(StakingError::OutOfBudget);
        }
        match rpc.is_executed(&ahash) {
            Ok(Some((true, Some(n)))) if n == ah => {}
            // Abandoned (still stored, not executed), or executed at another height: a reorg.
            Ok(Some(_)) => scan.rewind(),
            // An older node without citrate_isExecuted: the executed head below the anchor, or at
            // its height with another hash, is a reorg (DGX probe 6).
            Ok(None) => {
                if let Some((n, hash)) = &h1 {
                    if *n < ah || (*n == ah && !hash.eq_ignore_ascii_case(&ahash)) {
                        scan.rewind();
                    }
                }
            }
            // -32603, -32602, a timeout or a malformed answer: no evidence this read.
            Err(_) => {
                evidence = false;
                note = Some("the node could not confirm the block the scan was saved at".into());
            }
        }
    }
    let scan_end = h1
        .as_ref()
        .map_or(current_block, |(n, _)| (*n).min(current_block));
    // The committed position never passes the executed head less the margin; without evidence it
    // does not move.
    let commit_cap: Option<u64> = match &h1 {
        Some((n, _)) if evidence => Some(
            ((*n).min(current_block) + 1)
                .saturating_sub(EXECUTED_HEAD_MARGIN_BLOCKS)
                .max(LIQUID_STAKING_POOL_DEPLOY_BLOCK),
        ),
        _ => None,
    };

    // 2. The log paging up to the executed head. With evidence it starts at the committed position:
    // the session cursor may hold blocks read on a branch that was replaced, or read before there
    // was evidence (#371 Codex P1), so it is never promoted into committed coverage. Without
    // evidence nothing is committed, and the session cursor (less the margin, so the newest blocks
    // are read again) carries the read forward.
    let page_deadline = if h1.is_some() {
        deadline.map(|d| d.checked_sub(H2_RESERVE).unwrap_or(d))
    } else {
        deadline
    };
    let start = match commit_cap {
        Some(_) => scan.next_block,
        None => scan.next_block.max(
            scan.session
                .next_block
                .saturating_sub(EXECUTED_HEAD_MARGIN_BLOCKS),
        ),
    }
    .max(LIQUID_STAKING_POOL_DEPLOY_BLOCK);
    let mut from = start;
    // The last block of the contiguous range read in this call; committed only at the end.
    let mut read_to: Option<u64> = None;
    let mut window = LOG_WINDOW_BLOCKS;
    let mut retries = 0u32;
    while from <= scan_end {
        // At least one window per call, so every call makes progress.
        if read_to.is_some() && page_deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            break;
        }
        let to = from.saturating_add(window - 1).min(scan_end);
        // topic2 = staker (the SECOND indexed arg -> topics[2]); topic1 (id) left null.
        let filter = serde_json::json!({
            "address": liquid_staking_pool(),
            "topics": [
                WITHDRAWAL_REQUESTED_TOPIC0,
                serde_json::Value::Null,
                staker_topic,
            ],
            "fromBlock": format!("0x{from:x}"),
            "toBlock": format!("0x{to:x}"),
        });
        match rpc.get_logs_range(filter) {
            Ok(logs) => {
                for log in &logs {
                    // topic1 = the indexed id (uint256). A log without one is skipped, never
                    // given a made-up id.
                    if let Some(id_topic) = log.topics.get(1) {
                        scan.ids.insert(decode_uint256_topic(id_topic)?);
                    }
                }
                read_to = Some(to);
                scan.session.next_block = scan.session.next_block.max(to + 1);
                from = to + 1;
                retries = 0;
            }
            // citrate-chain#334: -32002 = the node is still filling its cache for a deep range.
            // Retry the SAME window after a bounded back-off; past the last attempt, "still indexing".
            Err(crate::rpc::GetLogsError::Retry(m)) => {
                retries += 1;
                if retries >= LOGS_RETRY_ATTEMPTS {
                    return Err(StakingError::Indexing(format!("blocks {from}-{to}: {m}")));
                }
                std::thread::sleep(logs_retry_backoff(retries - 1));
            }
            Err(crate::rpc::GetLogsError::RangeTooLarge(m)) => {
                if window == 1 {
                    return Err(StakingError::Rpc(format!(
                        "eth_getLogs blocks {from}-{to}: {m}"
                    )));
                }
                window = (window / 2).max(1);
            }
            Err(crate::rpc::GetLogsError::Rpc(e)) => {
                return Err(StakingError::Rpc(format!(
                    "eth_getLogs blocks {from}-{to}: {e}"
                )));
            }
            // RED STUB: never produced by get_logs_range.
            Err(e) => {
                return Err(StakingError::Rpc(format!(
                    "eth_getLogs blocks {from}-{to}: {e}"
                )));
            }
        }
    }
    let reached_end = from > scan_end;

    // 3. h2, and the commit.
    let served = commit_read(rpc, scan, h1.as_ref(), commit_cap, read_to, deadline);
    if served.is_none() && evidence {
        note.get_or_insert_with(|| {
            "the node's executed head could not be confirmed this pass".into()
        });
    }

    // 4. The withdrawal reads, every id on every pass, resuming across calls.
    let low_head = served.unwrap_or(current_block);
    let wanted: Vec<u128> = scan.ids.iter().copied().collect();
    for id in &wanted {
        if !scan.session.pass_results.contains_key(id) && !scan.session.pass_remaining.contains(id)
        {
            scan.session.pass_remaining.push(*id);
        }
    }
    let mut batches = 0usize;
    while !scan.session.pass_remaining.is_empty() {
        // At least one batch per call, so a long list finishes in a bounded number of calls.
        if batches > 0 && spent() {
            break;
        }
        let n = scan.session.pass_remaining.len().min(WITHDRAWAL_READ_BATCH);
        let chunk: Vec<u128> = scan.session.pass_remaining[..n].to_vec();
        let calls: Vec<serde_json::Value> = chunk.iter().map(|id| withdrawals_call(*id)).collect();
        let rets = rpc
            .eth_call_batch(&calls)
            .map_err(|e| StakingError::Rpc(e.to_string()))?;
        for (&id, ret) in chunk.iter().zip(rets.iter()) {
            let (staker, salt_wei, request_block, claimed) = decode_withdrawal_tuple(ret)?;
            scan.session.pass_results.insert(
                id,
                WithdrawalDetail {
                    staker,
                    salt_wei,
                    request_block,
                    claimed,
                },
            );
        }
        scan.session.pass_remaining.drain(..n);
        batches += 1;
    }
    let details_done = scan.session.pass_remaining.is_empty();
    if details_done {
        scan.session.last_details = std::mem::take(&mut scan.session.pass_results);
    }

    let mut out = Vec::new();
    for id in &wanted {
        let Some(d) = scan
            .session
            .pass_results
            .get(id)
            .or_else(|| scan.session.last_details.get(id))
        else {
            continue; // not read yet: the read is incomplete
        };
        if d.claimed || !d.staker.eq_ignore_ascii_case(&addr) {
            continue; // EXCLUDE claimed; defensive: not this staker (topic filter miss)
        }
        let claimable_at_block = d.request_block.saturating_add(WITHDRAWAL_DELAY);
        out.push(PendingWithdrawal {
            id: id.to_string(),
            salt_wei: d.salt_wei.to_string(),
            request_block: d.request_block,
            claimable_at_block,
            claimable: current_block >= claimable_at_block,
        });
    }

    scan.last_read_complete = reached_end && details_done;
    let covered = served.is_some_and(|h| h >= current_block);
    scan.last_read_confirmed = scan.last_read_complete && evidence && covered;
    scan.last_read_note = if scan.last_read_confirmed {
        None
    } else if !reached_end {
        Some(format!(
            "reading chain history: blocks {start}-{} of {scan_end} read",
            from.saturating_sub(1)
        ))
    } else if !details_done {
        Some(format!(
            "reading withdrawal details: {} of {} read",
            wanted.len() - scan.session.pass_remaining.len(),
            wanted.len()
        ))
    } else if h1.is_none() {
        Some(
            "the RPC does not report the block it has executed (citrate_executedHead), so the scan \
             can't confirm it reached the chain head"
                .into(),
        )
    } else if let Some(n) = note {
        Some(n)
    } else {
        Some(format!(
            "the node has served blocks up to {low_head} of {current_block}; reading again shortly"
        ))
    };
    Ok(out)
}

/// C1-W — commit the range read up to `read_to` into `scan`, bounded by `commit_cap` and by a
/// SECOND read of `citrate_executedHead` (h2): the position never passes
/// min(h1, h2) - EXECUTED_HEAD_MARGIN_BLOCKS. Commits nothing without evidence (`commit_cap` None),
/// when h2 can't be read in time, or when h1 and h2 name one height with two hashes (a replacement
/// in flight). The lower head becomes the anchor the next read checks. Returns the lower head (the
/// height the node has served to) when there was evidence.
fn commit_read<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    scan: &mut WithdrawalScan,
    h1: Option<&(u64, String)>,
    commit_cap: Option<u64>,
    read_to: Option<u64>,
    deadline: Option<std::time::Instant>,
) -> Option<u64> {
    let commit_cap = commit_cap?;
    let (h1, hash1) = h1?;
    if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
        return None;
    }
    let (h2, hash2) = rpc.executed_head().ok().flatten()?;
    if h2 == *h1 && !hash2.eq_ignore_ascii_case(hash1) {
        return None;
    }
    let low = if h2 < *h1 {
        (h2, hash2)
    } else {
        (*h1, hash1.clone())
    };
    let commit_cap = commit_cap.min((low.0 + 1).saturating_sub(EXECUTED_HEAD_MARGIN_BLOCKS));
    if let Some(read_to) = read_to {
        let before = scan.next_block;
        scan.next_block = scan.next_block.max((read_to + 1).min(commit_cap));
        if scan.next_block != before {
            scan.anchor = Some(low.clone());
        }
    }
    Some(low.0)
}

/// The `eth_call` object for `withdrawals(id)` on the pool: `{to, data}`.
fn withdrawals_call(id: u128) -> serde_json::Value {
    let calldata = encode_uint_calldata(withdrawals_selector(), id);
    serde_json::json!({
        "to": liquid_staking_pool(),
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
        "to": liquid_staking_pool(),
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
pub async fn wallet_stake(
    app_h: tauri::AppHandle,
    amount_wei: String,
) -> std::result::Result<CeremonyView, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st1 = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st2 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        wallet_stake_sync(amount_wei, st1, st2)
    })
    .await
}

/// Blocking body of [`wallet_stake`]; reached only through [`crate::blocking::off_main`].
pub fn wallet_stake_sync(
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
pub async fn wallet_request_withdrawal(
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
pub async fn wallet_claim_withdrawal(
    app_h: tauri::AppHandle,
    request_id: String,
) -> std::result::Result<CeremonyView, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st1 = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st2 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        wallet_claim_withdrawal_sync(request_id, st1, st2)
    })
    .await
}

/// Blocking body of [`wallet_claim_withdrawal`]; reached only through [`crate::blocking::off_main`].
pub fn wallet_claim_withdrawal_sync(
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
///
/// C1-W: reads from the public RPC (`rpc.citrate.ai`): only producer nodes store receipts and
/// logs today, so a member's own follower node can't answer `eth_getLogs`. The scan is paged (see
/// [`read_pending_withdrawals_with`]) and kept per staker for this app session, so a refresh reads
/// only the trailing blocks and new ones; the first scan is spread over refreshes by [`SCAN_BUDGET`].
#[tauri::command]
pub async fn wallet_pending_withdrawals(
    app: tauri::AppHandle,
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<PendingWithdrawalsRead, String> {
    use tauri::Manager as _;
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    // Kept on disk too (only what was committed on evidence), so a relaunch resumes instead of
    // rescanning from the deployment block. The session cursor stays in memory.
    let file = app
        .path()
        .app_local_data_dir()
        .ok()
        .map(|d| d.join("withdrawal-scans.json"));
    crate::blocking::off_main(move || {
        let key = format!(
            "{}:{}:{}",
            crate::addresses::genesis_hash(),
            liquid_staking_pool(),
            wallet.address.to_ascii_lowercase()
        );
        let cached = scans()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned();
        let mut scan = cached
            .or_else(|| file.as_deref().map(|f| load_scan(f, &key)))
            .unwrap_or_default();
        let rpc = crate::rpc::RpcClient::citrate();
        let deadline = std::time::Instant::now() + SCAN_BUDGET;
        let out = read_pending_withdrawals_with(&rpc, &wallet.address, &mut scan, Some(deadline))
            .map(|withdrawals| PendingWithdrawalsRead {
                withdrawals,
                confirmed: scan.last_read_confirmed,
                complete: scan.last_read_complete,
                note: scan.last_read_note.clone(),
            });
        // Keep the progress even when this read failed or ran out of budget, merged with any read
        // that overlapped this one so progress never goes back.
        {
            let mut all = scans().lock().unwrap_or_else(|e| e.into_inner());
            let merged = match all.get(&key) {
                Some(cur) => merge_scans(cur, &scan),
                None => scan.clone(),
            };
            all.insert(key.clone(), merged);
        }
        if let Some(f) = file.as_deref() {
            // Best effort: a failed save only means the next launch rescans.
            let _ = save_scan(f, &key, &scan);
        }
        out.map_err(|e| e.to_string())
    })
    .await
}

/// C1-W — what `wallet_pending_withdrawals` returns: the pending withdrawals found (each confirmed
/// from contract state); `complete` = the read reached the end of what the node serves and read
/// every withdrawal; `confirmed` = also, on evidence, up to the head the node reports. When not
/// confirmed, `note` says why, and an empty list is not "none".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingWithdrawalsRead {
    pub withdrawals: Vec<PendingWithdrawal>,
    pub confirmed: bool,
    pub complete: bool,
    pub note: Option<String>,
}

/// C1-W — how long one `wallet_pending_withdrawals` call may scan. Worst case it overruns by one
/// window, the h2 read and one withdrawal batch (6 s each at the RPC timeout): 15 + 12 s with the
/// H2_RESERVE taken out of the paging, under the webview's 30 s deadline for this command.
pub const SCAN_BUDGET: std::time::Duration = std::time::Duration::from_secs(15);

/// The per-staker scans of this app session (pool address + staker -> progress).
fn scans() -> &'static std::sync::Mutex<std::collections::HashMap<String, WithdrawalScan>> {
    static SCANS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, WithdrawalScan>>,
    > = std::sync::OnceLock::new();
    SCANS.get_or_init(Default::default)
}

#[cfg(test)]
mod tests {
    include!("staking_tests.rs");
}
