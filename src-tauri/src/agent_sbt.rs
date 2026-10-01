//! HUP-S7.4 (US-7.1) — **AgentSBT mint at onboarding**: give Hermes an on-chain identity.
//!
//! Live ABI (citrate-chain `contracts/src/cit_agent/AgentSBT.sol`, soulbound ERC-721, no
//! `ERC721Enumerable`):
//!
//! ```text
//! mintAgent(address to, uint256 parent_org_id, bytes32 did, bytes32 pubkey_fingerprint)
//!     external onlyOwner returns (uint256)          // reverts OrgNotActive() unless the parent org is active
//! balanceOf(address) / ownerOf(uint256)             // ERC-721
//! getAgent(uint256) returns ((uint256 parent_org_id, bytes32 did, bytes32 pubkey_fingerprint, bool quarantined))
//! orgContract() returns (address)                   // the parent OrganizationSBT; isActive(uint256) there
//! Transfer(address indexed from, address indexed to, uint256 indexed tokenId)   // mint = from 0
//! ```
//!
//! The contract has no `tokenOfOwnerByIndex`, so a member's tokens are found from the mint
//! `Transfer(0, member, id)` logs and checked against `balanceOf` (the token is soulbound and
//! has no burn path, so a mint log is a holding).
//!
//! # What this module does
//!
//! * builds the `mintAgent` calldata (byte-exact against `cast`; tested);
//! * gathers the facts that decide whether the onboarding step can be offered, and turns them
//!   into one honest state (`assess`). The step is offered ONLY when the address book carries
//!   AgentSBT, the address has code, the member holds none yet, the parent organization is
//!   active, this install has the identity key, and an `eth_call` preflight of the exact tx
//!   from the member's wallet succeeds. Anything else shows why, in plain words;
//! * submits the mint as a PENDING SignatureCeremony (HIC-1: the member approves it). Rule 3:
//!   nothing here signs or holds a key.
//!
//! # State on 40204 (2026-10-01)
//!
//! AgentSBT is deployed (`0xd16b1ad6…7c7b`) and owned by the CitAgent 2-of-3 timelock, and no
//! OrganizationSBT has been minted yet. So the step shows "available after the network
//! upgrade" today: first because the parent organization is not active, and after that because
//! `mintAgent` is issuer-only (`onlyOwner`). It enables itself when the chain allows the
//! member's own mint, with no app change.
//!
//! # Pending owner sign-off
//!
//! * **Parent organization.** Which OrganizationSBT parents every member's Hermes.
//!   Placeholder: org `0` ([`DEFAULT_PARENT_ORG_ID`]), overridable with
//!   `CITRATE_AGENT_PARENT_ORG_ID` for operators and test networks.
//! * **Issuance path.** `mintAgent` is `onlyOwner`, so a member cannot mint their own today.
//!   Either the contract gains a member-callable mint, or a registrar service mints on the
//!   member's request. This module preflights from the member's wallet, so either change
//!   turns the step on without an app release (the registrar path would add a request route).
//! * **Identity key.** The `pubkey_fingerprint` is `sha256(ed25519 pubkey)`, the runtime's
//!   `signer_id_from_pubkey`. Hermes has no key of its own (the sidecar is keyless), so the
//!   placeholder binds the identity to this node's ed25519 proposer key (public half only).
//! * **DID.** `did:citrate:agent:<member address, lowercase>`, hashed with keccak-256 to the
//!   `bytes32` the contract stores (the OrganizationSBT documents its `did` the same way).
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

use crate::model_registry::selector;
use crate::rpc::{LogEntry, RpcClient, RpcTransport};

/// The `mintAgent` signature the selector (`0x51d3fe66`) derives from.
pub const MINT_AGENT_SIG: &str = "mintAgent(address,uint256,bytes32,bytes32)";

/// keccak256("Transfer(address,address,uint256)") — the ERC-721 Transfer event topic.
pub const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// `OwnableUnauthorizedAccount(address)` — the caller is not the contract owner (the issuer).
const ERR_NOT_OWNER: &str = "118cdaa7";
/// `OrgNotActive()` — the parent OrganizationSBT is not active.
const ERR_ORG_NOT_ACTIVE: &str = "a4dde45e";

/// Pending owner sign-off: the OrganizationSBT that parents every member's Hermes.
pub const DEFAULT_PARENT_ORG_ID: u64 = 0;
/// Operator / test-network override for [`DEFAULT_PARENT_ORG_ID`].
pub const PARENT_ORG_ENV: &str = "CITRATE_AGENT_PARENT_ORG_ID";

/// Chain id the mint targets (40204).
const CHAIN_ID: u64 = 40204;

// ------------------------------------------------------------------ identity

/// `did:citrate:agent:<member address, lowercase>`. Refuses anything that is not a 20-byte
/// `0x` address, so one member always yields one DID string.
pub fn agent_did(member: &str) -> Result<String, String> {
    let a = crate::validator::parse_address_20(member)?;
    if !member.starts_with("0x") {
        return Err("member address must be 0x-prefixed".into());
    }
    Ok(format!("did:citrate:agent:0x{}", hex::encode(a)))
}

/// keccak-256 of a DID string (the `bytes32 did` the contract stores).
pub fn did_hash(did: &str) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(did.as_bytes());
    h.finalize().into()
}

/// `sha256(pubkey)`, identical to the runtime's `hitl::signer_id_from_pubkey`.
pub fn pubkey_fingerprint(pubkey: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(pubkey);
    h.finalize().into()
}

/// Parse a `0x` + 64-hex ed25519 public key.
pub fn parse_pubkey_hex(s: &str) -> Result<[u8; 32], String> {
    let raw = s.strip_prefix("0x").unwrap_or(s);
    let v = hex::decode(raw).map_err(|e| format!("identity key is not hex: {e}"))?;
    v.try_into()
        .map_err(|v: Vec<u8>| format!("identity key must be 32 bytes, got {}", v.len()))
}

/// The parent organization id: the override when set (it must be a plain unsigned integer),
/// else [`DEFAULT_PARENT_ORG_ID`].
pub fn parse_parent_org(v: Option<&str>) -> Result<u64, String> {
    match v.map(str::trim) {
        None | Some("") => Ok(DEFAULT_PARENT_ORG_ID),
        Some(s) => s
            .parse::<u64>()
            .map_err(|_| format!("{PARENT_ORG_ENV} must be an unsigned integer, got {s:?}")),
    }
}

// ------------------------------------------------------------------ calldata

fn word_u64(n: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}

fn word_addr(a: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a);
    w
}

/// `mintAgent(to, parent_org_id, did, pubkey_fingerprint)` calldata. All four arguments are
/// static words. Pure: no I/O, no signing.
pub fn mint_agent_calldata(
    to: &[u8; 20],
    parent_org_id: u64,
    did: &[u8; 32],
    fingerprint: &[u8; 32],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 4 * 32);
    out.extend_from_slice(&selector(MINT_AGENT_SIG));
    out.extend_from_slice(&word_addr(to));
    out.extend_from_slice(&word_u64(parent_org_id));
    out.extend_from_slice(did);
    out.extend_from_slice(fingerprint);
    out
}

/// `balanceOf(owner)`.
pub fn balance_of_calldata(owner: &[u8; 20]) -> Vec<u8> {
    let mut out = selector("balanceOf(address)").to_vec();
    out.extend_from_slice(&word_addr(owner));
    out
}

/// `getAgent(tokenId)`.
pub fn get_agent_calldata(token_id: u64) -> Vec<u8> {
    let mut out = selector("getAgent(uint256)").to_vec();
    out.extend_from_slice(&word_u64(token_id));
    out
}

/// `OrganizationSBT.isActive(orgId)`.
pub fn is_active_calldata(org_id: u64) -> Vec<u8> {
    let mut out = selector("isActive(uint256)").to_vec();
    out.extend_from_slice(&word_u64(org_id));
    out
}

/// `AgentSBT.orgContract()`.
pub fn org_contract_calldata() -> Vec<u8> {
    selector("orgContract()").to_vec()
}

/// The `eth_getLogs` filter for mints to `owner`: `Transfer(0, owner, *)` on `contract`.
pub fn minted_to_filter(contract: &str, owner: &[u8; 20]) -> Value {
    json!({
        "address": contract,
        "fromBlock": "0x0",
        "toBlock": "latest",
        "topics": [
            TRANSFER_TOPIC,
            format!("0x{}", "0".repeat(64)),
            format!("0x{}", hex::encode(word_addr(owner))),
        ],
    })
}

// ------------------------------------------------------------------ decoding

/// A `uint256` word that fits in u128. Short input or a value beyond u128 is an error,
/// never a truncation.
pub fn decode_u128(b: &[u8]) -> Result<u128, String> {
    let w = b.get(..32).ok_or("return shorter than one word")?;
    if w[..16].iter().any(|&x| x != 0) {
        return Err("value exceeds u128".into());
    }
    let mut lo = [0u8; 16];
    lo.copy_from_slice(&w[16..]);
    Ok(u128::from_be_bytes(lo))
}

/// A `bool` word (0 or 1; anything else is malformed).
pub fn decode_bool(b: &[u8]) -> Result<bool, String> {
    match decode_u128(b)? {
        0 => Ok(false),
        1 => Ok(true),
        n => Err(format!("bool word is {n}")),
    }
}

/// An `address` word, lowercase `0x` hex.
pub fn decode_address(b: &[u8]) -> Result<String, String> {
    let w = b.get(..32).ok_or("return shorter than one word")?;
    if w[..12].iter().any(|&x| x != 0) {
        return Err("address word has high bytes set".into());
    }
    Ok(format!("0x{}", hex::encode(&w[12..])))
}

/// One AgentSBT as the profile shows it. Numbers are decimal strings (uint256 on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToken {
    pub token_id: String,
    pub parent_org_id: String,
    /// The stored `bytes32 did` (keccak of the DID string), `0x` hex.
    pub did: String,
    pub pubkey_fingerprint: String,
    pub quarantined: bool,
}

/// Decode a `getAgent` return: a static 4-word tuple.
pub fn decode_agent(token_id: u64, b: &[u8]) -> Result<AgentToken, String> {
    if b.len() < 128 {
        return Err(format!("getAgent returned {} bytes, expected 128", b.len()));
    }
    Ok(AgentToken {
        token_id: token_id.to_string(),
        parent_org_id: decode_u128(&b[0..32])?.to_string(),
        did: format!("0x{}", hex::encode(&b[32..64])),
        pubkey_fingerprint: format!("0x{}", hex::encode(&b[64..96])),
        quarantined: decode_bool(&b[96..128])?,
    })
}

/// The token id (topic 3) of an ERC-721 Transfer log.
pub fn token_id_from_log(log: &LogEntry) -> Result<u64, String> {
    let t = log
        .topics
        .get(3)
        .ok_or("Transfer log has no tokenId topic")?;
    let raw = hex::decode(t.trim_start_matches("0x")).map_err(|e| format!("tokenId topic: {e}"))?;
    let n = decode_u128(&raw)?;
    u64::try_from(n).map_err(|_| "tokenId exceeds u64".to_string())
}

// ------------------------------------------------------------------ preflight + readiness

/// What an `eth_call` of the exact mint tx from the member said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preflight {
    /// The call would succeed.
    Ok,
    /// `OwnableUnauthorizedAccount`: only the issuer can mint today.
    NotIssuer,
    /// `OrgNotActive`: the parent organization is not active.
    OrgNotActive,
    /// Any other revert or node error, verbatim.
    Reverted(String),
}

/// Classify a JSON-RPC `error` object from an `eth_call`. The revert data is in `error.data`
/// on anvil and inside `error.message` on the Citrate node, so both are searched for the
/// known error selectors.
pub fn classify_revert(err: &Value) -> Preflight {
    let msg = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let data = err
        .get("data")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let hay = format!("{data} {}", msg.to_ascii_lowercase());
    let has = |sel: &str| {
        hay.match_indices("0x")
            .any(|(i, _)| hay[i + 2..].starts_with(sel))
    };
    if has(ERR_NOT_OWNER) {
        Preflight::NotIssuer
    } else if has(ERR_ORG_NOT_ACTIVE) {
        Preflight::OrgNotActive
    } else if msg.is_empty() {
        Preflight::Reverted(err.to_string())
    } else {
        Preflight::Reverted(msg)
    }
}

/// The one state the onboarding step and the profile render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MintState {
    /// Every check passed: the step is offered.
    Ready,
    /// The member already holds an AgentSBT.
    Minted,
    /// This build's address book has no AgentSBT.
    NotInBook,
    /// The AgentSBT address has no code on chain.
    NoCode,
    /// The parent organization is not active (or does not exist yet).
    OrgNotActive,
    /// Only the issuer can mint today.
    NotIssuer,
    /// This install does not have the identity key yet.
    IdentityKeyMissing,
    /// A chain read failed.
    ChainUnreachable,
    /// The preflight reverted for another reason.
    Reverted,
}

/// The facts [`assess`] decides on. Each `Err` is a read that failed, with its reason.
#[derive(Debug, Clone)]
pub struct Facts {
    pub contract: Option<String>,
    pub has_code: Result<bool, String>,
    pub held: Result<u128, String>,
    pub org_active: Result<bool, String>,
    pub identity_key: Result<[u8; 32], String>,
    pub preflight: Result<Preflight, String>,
}

/// The verdict: a state, whether the mint is offered, and the words the member reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Readiness {
    pub state: MintState,
    pub available: bool,
    pub message: String,
}

/// The member-facing sentence for a state. `detail` carries a read error or revert reason
/// where the state has one.
pub fn message_for(state: MintState, detail: &str) -> String {
    const LATER: &str = "Hermes identity is available after the network upgrade.";
    match state {
        MintState::Ready => "Give Hermes an on-chain identity, an AgentSBT bound to your wallet. \
             You approve one transaction in the Signature Ceremony; it costs only network gas."
            .into(),
        MintState::Minted => {
            "Hermes has an on-chain identity (AgentSBT) bound to your wallet.".into()
        }
        MintState::NotInBook => {
            format!("{LATER} This build does not list the AgentSBT contract yet.")
        }
        MintState::NoCode => {
            format!("{LATER} The AgentSBT contract is not deployed on chain 40204 yet.")
        }
        MintState::OrgNotActive => format!(
            "{LATER} The organization that issues Hermes identities is not set up on chain yet."
        ),
        MintState::NotIssuer => format!(
            "{LATER} Identities are issued by the network's agent registrar today, and \
             self-service issuance for members is not open yet."
        ),
        MintState::IdentityKeyMissing => format!(
            "Hermes identity uses this node's key, which your node creates once it finishes \
             syncing. This step unlocks then. ({detail})"
        ),
        MintState::ChainUnreachable => {
            format!("Could not check chain 40204 for Hermes identity right now: {detail}")
        }
        MintState::Reverted => {
            format!("The identity mint would fail on chain, so it is not offered: {detail}")
        }
    }
}

fn verdict(state: MintState, detail: &str) -> Readiness {
    Readiness {
        state,
        available: state == MintState::Ready,
        message: message_for(state, detail),
    }
}

/// Decide the state. Order: the book, the code, what the member already holds, the parent
/// organization, the identity key, then the preflight of the exact tx.
pub fn assess(f: &Facts) -> Readiness {
    if f.contract.is_none() {
        return verdict(MintState::NotInBook, "");
    }
    match &f.has_code {
        Err(e) => return verdict(MintState::ChainUnreachable, e),
        Ok(false) => return verdict(MintState::NoCode, ""),
        Ok(true) => {}
    }
    match &f.held {
        Err(e) => return verdict(MintState::ChainUnreachable, e),
        Ok(n) if *n > 0 => return verdict(MintState::Minted, ""),
        Ok(_) => {}
    }
    match &f.org_active {
        Err(e) => return verdict(MintState::ChainUnreachable, e),
        Ok(false) => return verdict(MintState::OrgNotActive, ""),
        Ok(true) => {}
    }
    if let Err(e) = &f.identity_key {
        return verdict(MintState::IdentityKeyMissing, e);
    }
    match &f.preflight {
        Err(e) => verdict(MintState::ChainUnreachable, e),
        Ok(Preflight::Ok) => verdict(MintState::Ready, ""),
        Ok(Preflight::NotIssuer) => verdict(MintState::NotIssuer, ""),
        Ok(Preflight::OrgNotActive) => verdict(MintState::OrgNotActive, ""),
        Ok(Preflight::Reverted(m)) => verdict(MintState::Reverted, m),
    }
}

// ------------------------------------------------------------------ gather (chain reads)

/// Everything the onboarding step and the profile need, in one IPC answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSbtStatus {
    /// The AgentSBT address from the address book, or null when the book has none.
    pub contract: Option<String>,
    pub member: String,
    /// The DID string the mint binds (null when the member address is malformed).
    pub did: Option<String>,
    pub parent_org_id: String,
    /// `balanceOf(member)` as a decimal string; null when it could not be read.
    pub balance: Option<String>,
    /// The member's tokens; null when they could not be listed (never a guessed empty list).
    pub tokens: Option<Vec<AgentToken>>,
    /// Why `tokens` is null, when it is.
    pub tokens_note: Option<String>,
    pub state: MintState,
    pub available: bool,
    pub message: String,
}

fn hex_data(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// A raw JSON-RPC exchange that keeps the `error` object (the kit client keeps only its
/// message, and the preflight needs the revert data).
fn raw_call<T: RpcTransport>(
    rpc: &RpcClient<T>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let body = rpc.build_request(method, params);
    rpc.transport().call(body).map_err(|e| e.to_string())
}

fn read_code<T: RpcTransport>(rpc: &RpcClient<T>, contract: &str) -> Result<bool, String> {
    let r = raw_call(rpc, "eth_getCode", json!([contract, "latest"]))?;
    if let Some(e) = r.get("error") {
        return Err(format!("eth_getCode: {e}"));
    }
    let code = r
        .get("result")
        .and_then(Value::as_str)
        .ok_or("eth_getCode: no result")?;
    Ok(!matches!(code, "0x" | "0x0" | ""))
}

fn call<T: RpcTransport>(rpc: &RpcClient<T>, to: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    rpc.eth_call(json!({"to": to, "data": hex_data(data)}))
        .map_err(|e| e.to_string())
}

fn preflight<T: RpcTransport>(
    rpc: &RpcClient<T>,
    from: &str,
    contract: &str,
    calldata: &[u8],
) -> Result<Preflight, String> {
    let r = raw_call(
        rpc,
        "eth_call",
        json!([{"from": from, "to": contract, "data": hex_data(calldata)}, "latest"]),
    )?;
    match r.get("error") {
        Some(e) => Ok(classify_revert(e)),
        None if r.get("result").is_some() => Ok(Preflight::Ok),
        None => Err("eth_call: no result".into()),
    }
}

fn list_tokens<T: RpcTransport>(
    rpc: &RpcClient<T>,
    contract: &str,
    member: &[u8; 20],
) -> Result<Vec<AgentToken>, String> {
    let logs = rpc
        .get_logs(minted_to_filter(contract, member))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(logs.len());
    for log in &logs {
        let id = token_id_from_log(log)?;
        let ret = call(rpc, contract, &get_agent_calldata(id))?;
        out.push(decode_agent(id, &ret)?);
    }
    Ok(out)
}

/// The mint calldata for `member`, or why it cannot be built.
fn member_calldata(member: &str, parent_org: u64, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let to = crate::validator::parse_address_20(member)?;
    let did = agent_did(member)?;
    Ok(mint_agent_calldata(
        &to,
        parent_org,
        &did_hash(&did),
        &pubkey_fingerprint(key),
    ))
}

/// Read everything from the chain and decide. `contract` is the address-book entry (None =
/// absent: no RPC call is made). `identity_key` is this install's ed25519 public key or why
/// it is not available.
pub fn gather<T: RpcTransport>(
    rpc: &RpcClient<T>,
    contract: Option<&str>,
    member: &str,
    parent_org: u64,
    identity_key: Result<[u8; 32], String>,
) -> AgentSbtStatus {
    let did = agent_did(member).ok();
    let mut st = AgentSbtStatus {
        contract: contract.map(str::to_string),
        member: member.to_string(),
        did,
        parent_org_id: parent_org.to_string(),
        balance: None,
        tokens: None,
        tokens_note: None,
        state: MintState::NotInBook,
        available: false,
        message: String::new(),
    };
    let skipped = || Err::<bool, String>("not checked".into());
    let mut facts = Facts {
        contract: st.contract.clone(),
        has_code: skipped(),
        held: Err("not checked".into()),
        org_active: skipped(),
        identity_key,
        preflight: Err("not checked".into()),
    };
    if let Some(c) = contract {
        facts.has_code = read_code(rpc, c);
        let member20 = crate::validator::parse_address_20(member);
        if matches!(facts.has_code, Ok(true)) {
            facts.held = match &member20 {
                Ok(m) => call(rpc, c, &balance_of_calldata(m)).and_then(|r| decode_u128(&r)),
                Err(e) => Err(e.clone()),
            };
            if let Ok(n) = &facts.held {
                st.balance = Some(n.to_string());
            }
            if let (Ok(n), Ok(m)) = (&facts.held, &member20) {
                if *n > 0 {
                    match list_tokens(rpc, c, m) {
                        Ok(t) => st.tokens = Some(t),
                        Err(e) => st.tokens_note = Some(format!("could not list tokens: {e}")),
                    }
                } else {
                    st.tokens = Some(Vec::new());
                }
            }
        }
        if matches!(facts.held, Ok(0)) {
            facts.org_active = call(rpc, c, &org_contract_calldata())
                .and_then(|r| decode_address(&r))
                .and_then(|org| call(rpc, &org, &is_active_calldata(parent_org)))
                .and_then(|r| decode_bool(&r));
            if let (Ok(true), Ok(key)) = (&facts.org_active, &facts.identity_key) {
                facts.preflight = member_calldata(member, parent_org, key)
                    .and_then(|cd| preflight(rpc, member, c, &cd));
            }
        }
    }
    let r = assess(&facts);
    st.state = r.state;
    st.available = r.available;
    st.message = r.message;
    st
}

// ------------------------------------------------------------------ the ceremony tx

/// Execution gas with a 25% margin over the node's estimate (saturating).
pub fn with_gas_margin(estimate: u64) -> u64 {
    estimate.saturating_add(estimate / 4)
}

/// The pending-ceremony tx JSON: from the member, to the AgentSBT, value 0.
pub fn mint_tx_json(from: &str, contract: &str, calldata: &[u8], gas: u64) -> String {
    json!({
        "from": from,
        "to": contract,
        "value": "0x0",
        "data": hex_data(calldata),
        "gas": format!("0x{gas:x}"),
        "chainId": format!("0x{CHAIN_ID:x}"),
    })
    .to_string()
}

/// The pending-ceremony tx for a member whose readiness is `st`: refuses with the
/// member-facing reason unless `st.available` (and then never asks the node for gas), else
/// builds the exact `mintAgent` calldata, estimates gas through `estimate` and adds the
/// margin. Pure apart from `estimate`; signs nothing.
pub fn prepare_mint_tx(
    st: &AgentSbtStatus,
    parent_org: u64,
    key: &[u8; 32],
    estimate: impl FnOnce(Value) -> Result<u64, String>,
) -> Result<String, String> {
    if !st.available {
        return Err(st.message.clone());
    }
    let contract = st
        .contract
        .as_deref()
        .ok_or_else(|| message_for(MintState::NotInBook, ""))?;
    let calldata = member_calldata(&st.member, parent_org, key)?;
    let gas = estimate(json!({"from": st.member, "to": contract, "data": hex_data(&calldata)}))
        .map_err(|e| format!("could not estimate gas for the identity mint: {e}"))?;
    Ok(mint_tx_json(
        &st.member,
        contract,
        &calldata,
        with_gas_margin(gas),
    ))
}

// ------------------------------------------------------------------ commands

/// The parent org id in effect (env override or the placeholder).
fn parent_org() -> Result<u64, String> {
    parse_parent_org(std::env::var(PARENT_ORG_ENV).ok().as_deref())
}

/// This install's identity key: the node's ed25519 proposer public key (placeholder source,
/// pending owner sign-off). Only the public half is read.
fn identity_key(app_h: &tauri::AppHandle) -> Result<[u8; 32], String> {
    let node = tauri::Manager::try_state::<crate::node::NodeState>(app_h)
        .ok_or_else(|| "internal: node state unavailable".to_string())?;
    let hex = node.0.proposer_pubkey()?;
    parse_pubkey_hex(&hex)
}

fn member_address(app_h: &tauri::AppHandle) -> Result<String, String> {
    let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(app_h)
        .ok_or_else(|| "internal: managed state unavailable".to_string())?;
    let w = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    Ok(w.address)
}

fn status_now(app_h: &tauri::AppHandle) -> Result<AgentSbtStatus, String> {
    let member = member_address(app_h)?;
    let org = parent_org()?;
    let rpc = RpcClient::citrate();
    Ok(gather(
        &rpc,
        crate::addresses::agent_sbt(),
        &member,
        org,
        identity_key(app_h),
    ))
}

/// **Command — agent_sbt_status.** The member's Hermes identity: the AgentSBT they hold (if
/// any) and whether the onboarding mint can be offered, with the reason when it cannot.
/// Read-only chain calls.
#[tauri::command]
pub async fn agent_sbt_status(app_h: tauri::AppHandle) -> Result<AgentSbtStatus, String> {
    crate::blocking::off_main(move || status_now(&app_h)).await
}

/// **Command — agent_sbt_mint.** Re-checks readiness, then submits the `mintAgent` tx as a
/// PENDING SignatureCeremony and returns its view for the member to approve (HIC-1). Refuses
/// with the member-facing reason when the mint is not available. Nothing signs here (Rule 3).
#[tauri::command]
pub async fn agent_sbt_mint(
    app_h: tauri::AppHandle,
) -> Result<crate::ceremony::CeremonyView, String> {
    crate::blocking::off_main(move || {
        let st = status_now(&app_h)?;
        if !st.available {
            return Err(st.message);
        }
        let key = identity_key(&app_h)?;
        let rpc = RpcClient::citrate();
        let raw = prepare_mint_tx(&st, parent_org()?, &key, |q| {
            rpc.estimate_gas(q).map_err(|e| e.to_string())
        })?;
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        Ok(ceremony.0.request(crate::ceremony::SignatureIntent {
            origin: "local-user".to_string(),
            kind: crate::ceremony::IntentKind::Transaction,
            chain_id: CHAIN_ID,
            raw,
        }))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("agent_sbt_tests.rs");
}
