//! citrate-core — earnings: real on-chain claimable + the claim path (CORE-C2).
//! @rule8 · a claim is a SIGNED VALUE-BEARING write and routes through the
//! SignatureCeremony like any other tx.
//!
//! Two jobs, both grounded in the on-chain contract (Rule 1 — no fabricated
//! numbers, no sim decomposition presented as live):
//!
//! 1. **Real claimable poll (WP1).** Read
//!    `ContributionAccounting.claimable(walletAddress)` via `eth_call` on 40204
//!    (selector `0x402914f5`, cross-checked against the node-agent's
//!    `citrate-node-agent/crates/chainio/src/selectors.rs`) and decode the
//!    returned `uint256` to wei. The Earning tab shows THIS single real value.
//!
//! 2. **The claim (WP2, @rule8).** `claimRewards()` (selector `0x372500ab`) is a
//!    value-bearing write: it zeroes the caller's `claimable` and transfers the
//!    SALT out (`ContributionAccounting.sol:218`). It is NEVER signed here — the
//!    claim is emitted as an unsigned intent and routed through the C1.2 bridge →
//!    [`crate::ceremony::SignatureCeremony`] → human `approve_and_broadcast`
//!    (B1.4). The user's Claim button and the node-agent's `ClaimRewards`
//!    signature-request both land on the SAME ceremony path.
//!
//! ## Grounded contract ABI (citrate-chain/contracts/src/ContributionAccounting.sol)
//! - `mapping(address => uint256) public claimable;` — a public getter
//!   `claimable(address) returns (uint256)` (a single balance in wei of SALT).
//! - `function claimRewards() external` — no args; zeroes `claimable[msg.sender]`,
//!   `distributed[msg.sender] += amount`, transfers `amount` out (line 218-230).
//! - **NO per-SOURCE breakdown of `claimable` exists on-chain.** Contributions
//!   are tracked per `ContributionType` (`contributions[addr][ctype]`, 7 types:
//!   Validation, ModelHosting, AdapterCreation, DataProvision, AppDevelopment,
//!   BridgeInfra, Governance) but `claimable` collapses to ONE `uint256`. There
//!   is no `claimable(address, ContributionType)` and no on-chain
//!   validation/pinning/compute split. So the Earning tab's sim decomposition
//!   (`earnVal`/`earnPin`/`earnComp`) has NO live data source and is DROPPED for
//!   real reads (Rule 1 / I-3): we surface the single real claimable only, never
//!   a fabricated split.
//!
//! ## Address (canonical book)
//! `ContributionAccounting = 0xcdd2477387279c7d44a1053f44db5dac0fd8faef` on 40204
//! (`citrate-node-agent/crates/chainio/src/generated/addresses.json`).

// C2 seam: the earnings reader is consumed by the AgentDomain `agent_earnings`
// command + the claim bridge; some helpers are only reached by tests until the
// full connector round-trip lands (mirrors rpc.rs/agent.rs staged consumers).
#![allow(dead_code)]

/// The `ContributionAccounting` contract on 40204 (canonical address book:
/// `citrate-node-agent/crates/chainio/src/generated/addresses.json`). Lowercase
/// `0x`-hex; the `eth_call` target for `claimable` and the claim `to`.
pub const CONTRIBUTION_ACCOUNTING: &str = "0xcdd2477387279c7d44a1053f44db5dac0fd8faef";

/// 4-byte selector for `claimable(address)` — the first four bytes of
/// `keccak256("claimable(address)")`. GROUNDED + PINNED against the node-agent's
/// independently-derived value (`citrate-node-agent/crates/chainio/src/selectors.rs`
/// `selectors::claimable() = 0x402914f5`). We keep it as a pinned constant (not a
/// runtime keccak) to avoid adding a keccak dependency to this lean tree; the
/// `claimable_selector_matches_node_agent` test is the drift tripwire (Rule 11).
const CLAIMABLE_SELECTOR: [u8; 4] = [0x40, 0x29, 0x14, 0xf5];

/// 4-byte selector for `claimRewards()` (no args) — `keccak256("claimRewards()")`.
/// GROUNDED + PINNED against the node-agent's `selectors::claim_rewards() =
/// 0x372500ab`, so the bridged ceremony signs the SAME 4 bytes the daemon serves.
const CLAIM_REWARDS_SELECTOR: [u8; 4] = [0x37, 0x25, 0x00, 0xab];

/// Selector for `claimable(address)` (the public mapping getter).
pub fn claimable_selector() -> [u8; 4] {
    CLAIMABLE_SELECTOR
}

/// Selector for `claimRewards()` (no args). The exact selector the node-agent
/// puts in its unsigned request calldata.
pub fn claim_rewards_selector() -> [u8; 4] {
    CLAIM_REWARDS_SELECTOR
}

/// **C2-F-3 — user-claim id space.** The node-agent mints its `PendingSignatureRequest`
/// ids sequentially from a small counter (grounded state.rs), so its ids live in the
/// LOW u64 range. A user-initiated Claim shares the agent bridge's dedup map
/// ([`crate::agent::AgentManager::bridged`], keyed by request id), so if the user
/// claim reused `id:0` it could ALIAS a node-agent request `id:0` — two distinct
/// intents collapsing into one dedup slot (a user claim suppressing the sweep's
/// ceremony, or vice-versa). We give user claims a DISJOINT high id space: `1 << 63`
/// (the top bit set), a range the node-agent's small sequential counter can never
/// reach in practice. A user claim and a node-agent request therefore never alias.
pub const USER_CLAIM_ID: u64 = 1u64 << 63;

/// Errors from the earnings reader. Coarse + secret-free (this module never sees
/// key material — it does a public `eth_call` read and builds unsigned calldata).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EarningsError {
    /// The `walletAddress` was not a `0x`-prefixed 20-byte hex address (so we
    /// cannot build a well-formed `claimable(address)` call — fail closed rather
    /// than call with a malformed argument).
    BadAddress(String),
    /// The live RPC read failed (transport / node error / missing field). Carries
    /// the RPC error's PUBLIC message (never key material — this path has none).
    Rpc(String),
    /// The `eth_call` returned bytes that do not decode to a `uint256` claimable
    /// (short return, or a value beyond `u128` — SALT amounts fit in `u128`, so a
    /// larger value means the return is not a claimable and we refuse to truncate).
    Decode(String),
}

impl std::fmt::Display for EarningsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EarningsError::BadAddress(m) => write!(f, "earnings: bad wallet address: {m}"),
            EarningsError::Rpc(m) => write!(f, "earnings: rpc error: {m}"),
            EarningsError::Decode(m) => write!(f, "earnings: claimable decode error: {m}"),
        }
    }
}

impl std::error::Error for EarningsError {}

/// The real, on-chain earnings snapshot surfaced to the Earning tab. Carries ONLY
/// the single real `claimable` (wei of SALT) read from `eth_call` — there is NO
/// per-source breakdown field, because the contract exposes none (Rule 1 / I-3:
/// we do not fabricate a validation/pinning/compute split).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct EarningsSnapshot {
    /// The real claimable balance in wei of SALT, from
    /// `ContributionAccounting.claimable(walletAddress)` via `eth_call`.
    #[serde(rename = "claimableWei")]
    pub claimable_wei: String,
    /// The wallet address the claimable was read for (the vault / node operator).
    #[serde(rename = "walletAddress")]
    pub wallet_address: String,
    /// The contract the value was read from (for the UI's data-source caption).
    #[serde(rename = "contract")]
    pub contract: String,
}

/// Validate a `0x`-prefixed 20-byte hex address (the ABI-encode input). Returns
/// the lowercased address (canonical) or [`EarningsError::BadAddress`].
fn validate_address(addr: &str) -> Result<String, EarningsError> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    if stripped.len() != 40 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(EarningsError::BadAddress(addr.to_string()));
    }
    Ok(format!("0x{}", stripped.to_ascii_lowercase()))
}

/// Build the `claimable(address)` calldata: `selector ++ left-padded 20-byte
/// address in a 32-byte word` (36 bytes total). `addr` must already be validated.
fn encode_claimable_calldata(addr: &str) -> Vec<u8> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    // Validated upstream, so this decode succeeds; default to empty on the
    // impossible malformed path (the 32-byte word then stays zero, which the
    // node rejects — we never silently call with a wrong address).
    let bytes = hex::decode(stripped).unwrap_or_default();
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&claimable_selector());
    // 32-byte word: 12 zero bytes + 20 address bytes (left-padded).
    let mut word = [0u8; 32];
    if bytes.len() == 20 {
        word[12..32].copy_from_slice(&bytes);
    }
    calldata.extend_from_slice(&word);
    calldata
}

/// The `eth_call` call object for `claimable(walletAddress)`: `{to, data}` where
/// `to` is the `ContributionAccounting` contract and `data` is the calldata.
/// Built here so the RPC client stays transport-only.
fn claimable_call(wallet_address: &str) -> serde_json::Value {
    let calldata = encode_claimable_calldata(wallet_address);
    serde_json::json!({
        "to": CONTRIBUTION_ACCOUNTING,
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// Decode a 32-byte `uint256` `eth_call` return to a `u128` (wei of SALT). The
/// return is a single ABI word. A SALT amount fits in `u128` (>~3.4e20 SALT), so
/// a value in the high 16 bytes means the return is NOT a plain claimable — we
/// refuse to truncate (Rule 1) and error. A short return also errors.
pub fn decode_claimable(ret: &[u8]) -> Result<u128, EarningsError> {
    if ret.len() < 32 {
        return Err(EarningsError::Decode(format!(
            "expected a 32-byte uint256 word, got {} bytes",
            ret.len()
        )));
    }
    // High 16 bytes must be zero for the value to fit in u128 (no silent truncation).
    if ret[0..16].iter().any(|&b| b != 0) {
        return Err(EarningsError::Decode(
            "claimable exceeds u128 — refusing to truncate".to_string(),
        ));
    }
    let mut low = [0u8; 16];
    low.copy_from_slice(&ret[16..32]);
    Ok(u128::from_be_bytes(low))
}

/// **WP1 — the real claimable read.** Read
/// `ContributionAccounting.claimable(wallet_address)` on the live 40204 RPC via
/// `eth_call` and decode the `uint256` to wei of SALT. Rule 1: the value comes
/// from the chain, not a sim; a fresh node with no accrued rewards honestly reads
/// `0`. `rpc` is injected so tests script a mock transport (never a fabricated
/// read); production wires [`crate::rpc::RpcClient::citrate`].
pub fn read_claimable<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    wallet_address: &str,
) -> Result<EarningsSnapshot, EarningsError> {
    let addr = validate_address(wallet_address)?;
    let ret = rpc
        .eth_call(claimable_call(&addr))
        .map_err(|e| EarningsError::Rpc(e.to_string()))?;
    let claimable_wei = decode_claimable(&ret)?;
    Ok(EarningsSnapshot {
        claimable_wei: claimable_wei.to_string(),
        wallet_address: addr,
        contract: CONTRIBUTION_ACCOUNTING.to_string(),
    })
}

/// Build the unsigned `claimRewards()` [`crate::agent::AgentSignatureRequest`] a
/// USER's Claim button emits (the same shape the node-agent's earnings sweep
/// serves). It carries ONLY public facts: the selector-only calldata, the
/// `ContributionAccounting` target, zero value (the contract PAYS the caller —
/// the claim tx itself sends no SALT, only gas), and chain 40204. It is NEVER
/// signed here — it is routed through the C1.2 bridge → ceremony → B1.4 like the
/// node-agent's request. `claimable_wei` is folded into the human-readable
/// `context` so the approval UI shows what is being swept (the REAL value the
/// caller just read, not a fabricated one).
pub fn user_claim_request(claimable_wei: u128) -> crate::agent::AgentSignatureRequest {
    let calldata = format!("0x{}", hex::encode(claim_rewards_selector()));
    crate::agent::AgentSignatureRequest {
        // C2-F-3: a synthetic id in the DISJOINT user-claim id space (`1 << 63`,
        // top bit set) so a user claim can NEVER alias a node-agent request in the
        // agent bridge's shared dedup map. The node-agent's ids are small +
        // sequential and never reach this range.
        id: USER_CLAIM_ID,
        intent: "claimRewards".to_string(),
        to: CONTRIBUTION_ACCOUNTING.to_string(),
        calldata,
        value_wei: "0".to_string(),
        chain_id: crate::rpc::CITRATE_CHAIN_ID,
        context: format!("claimRewards ({claimable_wei} wei claimable)"),
        expires_block: "0".to_string(),
        status: "pending".to_string(),
        tx_hash: None,
    }
}

// ---------------------------------------------------------------------------
// Tauri command — the Earning tab's real claimable read. Returns the single real
// claimable (wei) + its data source. NEVER a fabricated per-source breakdown.
// ---------------------------------------------------------------------------

/// **Command — agent_earnings.** Read the vault wallet's REAL claimable from
/// `ContributionAccounting.claimable(address)` via `eth_call` on the live 40204
/// RPC. Requires the vault UNLOCKED (to read the wallet's public address — the
/// key is never touched); a locked/absent vault fails closed with a clear error.
/// Returns the single real claimable (wei) — no sim decomposition (Rule 1).
#[tauri::command]
pub fn agent_earnings(
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<EarningsSnapshot, String> {
    // The wallet address to read claimable for = THIS vault's node-operator
    // wallet (reads the public identity, NOT the key).
    let wallet = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;
    let rpc = crate::rpc::RpcClient::citrate();
    read_claimable(&rpc, &wallet.address).map_err(|e| e.to_string())
}

/// Real wallet balances for the Wallet surface. Carries the three balances that
/// are grounded reads on 40204 today: native `liquid` SALT (`eth_getBalance`), the
/// real `claimable` (`ContributionAccounting.claimable`), and the real `staked`
/// SELF-stake (`LiquidStakingPool.balanceOf` — the user's OWN pool shares in SALT;
/// the membership-granted 32k is vault-held and NOT in this value, so the UI adds
/// the vaulted grant separately). All amounts are wei strings; the bridge converts
/// to SALT for display. Rule 1: every field is a live read, never a sim.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct WalletBalances {
    #[serde(rename = "liquidWei")]
    pub liquid_wei: String,
    #[serde(rename = "claimableWei")]
    pub claimable_wei: String,
    /// Self-stake in wei of SALT, from `LiquidStakingPool.balanceOf(address)`.
    #[serde(rename = "stakedWei")]
    pub staked_wei: String,
    #[serde(rename = "address")]
    pub address: String,
}

/// `wallet_balances` command — the REAL liquid + claimable read (CORE wallet, Rule
/// 1). Requires the vault UNLOCKED (to read the wallet's public address; the key is
/// never touched). Any RPC/decode failure fails closed with a clear error — the UI
/// keeps its last honest value rather than a fabricated one.
#[tauri::command]
pub fn wallet_balances(
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<WalletBalances, String> {
    let wallet = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;
    let rpc = crate::rpc::RpcClient::citrate();
    let liquid = rpc
        .get_balance(&wallet.address)
        .map_err(|e| e.to_string())?;
    let snap = read_claimable(&rpc, &wallet.address).map_err(|e| e.to_string())?;
    // Real self-stake (LiquidStakingPool.balanceOf) — same all-or-nothing contract
    // as liquid/claimable: an RPC hiccup fails the whole refresh, and the store
    // keeps the last honest values rather than showing a fabricated one (Rule 1).
    let staked =
        crate::staking::read_self_stake(&rpc, &wallet.address).map_err(|e| e.to_string())?;
    Ok(WalletBalances {
        liquid_wei: liquid.to_string(),
        claimable_wei: snap.claimable_wei,
        staked_wei: staked.to_string(),
        address: wallet.address,
    })
}

#[cfg(test)]
mod tests {
    include!("earnings_tests.rs");
}
