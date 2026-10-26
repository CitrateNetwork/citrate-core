//! citrate-core — Join-8: the read behind the "Activate validator" onboarding step.
//! @rule8 · T1 staking surface. A PURE READ: it never signs and never sees key material.
//!
//! The step's gate, its state words and its durations all come from this one read, and every
//! value in it names its source (UI truth contract v1, §1 and §2):
//!
//! | Field                     | Source                                                        |
//! |---------------------------|---------------------------------------------------------------|
//! | `bondAddress`             | `MembershipStakeVault.bondOf(member)`                         |
//! | `bondDeployed`            | code at `bondAddress` (an empty `activated()` return = none)  |
//! | `bondActivated`           | `MemberBond.activated()`                                      |
//! | `principalWei`            | `MemberBond.principal()`                                      |
//! | `unlockBlock/Timestamp`   | `MemberBond.unlockBlock()` / `unlockTimestamp()`              |
//! | `memberTokenId`           | `MemberBond.memberTokenId()`                                  |
//! | `membershipActive`        | `CitrateMemberSBT.isActive(memberTokenId)`                    |
//! | `validatorPubkey`         | `MemberBond.pubkey()` once activated, else this node's key    |
//! | `registryStatus`          | `ValidatorRegistry.validatorInfo(pubkey).status` (0..3)       |
//! | `activationEpoch`         | `ValidatorRegistry.validatorInfo(pubkey).actEpoch`            |
//! | `registryBondedWei`       | `ValidatorRegistry.validatorInfo(pubkey).bonded`              |
//! | `registryEscrowWei`       | `ValidatorRegistry.validatorInfo(pubkey).escrow`              |
//! | `registryActive`          | `ValidatorRegistry.isActive(pubkey)`                          |
//! | `epochBlocks`             | `ValidatorRegistry.EPOCH()`                                   |
//! | `currentEpoch`            | `ValidatorRegistry.currentEpoch()`                            |
//! | `nodePubkey`/`bondPubkey` | this node's `proposer.key` / `MemberBond.pubkey()`            |
//! | `headBlock`               | public `eth_blockNumber`                                      |
//! | `blockTimeMs`             | timestamp delta over the last 100 public blocks               |
//! | `localBlock`/`localSyncing` | this node's `eth_blockNumber` / `eth_syncing`               |
//! | `tx`                      | `eth_getTransactionReceipt(txHash)` for a sent activation     |
//!
//! Unknown is not zero (contract rule 2): every field is an `Option`, and a failed read is
//! `None`, never a plausible default. Only a malformed member address is an error.

use serde_json::{json, Value};

use crate::rpc::RpcTransport;

/// Blocks sampled for the measured block time (contract §2: "timestamp delta over the last 100
/// blocks"). Never the configured 1 s, never an assumed 12 s.
pub const BLOCK_TIME_SAMPLE: u64 = 100;

// Selectors, pinned and drift-tested against keccak256 of the signature in the tests.
/// `MembershipStakeVault.bondOf(address)`.
pub const BOND_OF: [u8; 4] = [0x72, 0xd2, 0xb6, 0xc0];
/// `MemberBond.activated()`.
pub const ACTIVATED: [u8; 4] = [0x18, 0x66, 0x01, 0xca];
/// `MemberBond.principal()`.
pub const PRINCIPAL: [u8; 4] = [0xba, 0x5d, 0x30, 0x78];
/// `MemberBond.memberTokenId()`.
pub const MEMBER_TOKEN_ID: [u8; 4] = [0x5f, 0xce, 0x5d, 0xf8];
/// `MemberBond.pubkey()`.
pub const BOND_PUBKEY: [u8; 4] = [0x48, 0x4d, 0x59, 0xe6];
/// `MemberBond.unlockBlock()`.
pub const UNLOCK_BLOCK: [u8; 4] = [0xea, 0x35, 0xdf, 0x16];
/// `MemberBond.unlockTimestamp()`.
pub const UNLOCK_TIMESTAMP: [u8; 4] = [0xaa, 0x08, 0x2a, 0x9d];
/// `CitrateMemberSBT.isActive(uint256)`.
pub const SBT_IS_ACTIVE: [u8; 4] = [0x82, 0xaf, 0xd2, 0x3b];
/// `ValidatorRegistry.validatorInfo(bytes32)`.
pub const VALIDATOR_INFO: [u8; 4] = [0x00, 0x40, 0x88, 0x38];
/// `ValidatorRegistry.isActive(bytes32)`.
pub const REGISTRY_IS_ACTIVE: [u8; 4] = [0x5c, 0x36, 0x90, 0x1c];
/// `ValidatorRegistry.EPOCH()`.
pub const EPOCH: [u8; 4] = [0xa0, 0xdc, 0x27, 0x58];
/// `ValidatorRegistry.currentEpoch()` (chain#319: activation is `actEpoch <= currentEpoch()`).
pub const CURRENT_EPOCH: [u8; 4] = [0x76, 0x67, 0x18, 0x08];

/// A sent activation transaction, as its receipt reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationTx {
    pub hash: String,
    /// `None` when the receipt read failed; `Some(false)` while it is still pending.
    pub included: Option<bool>,
    pub block_number: Option<u64>,
    /// Receipt `status == 1`. `None` until included (or when the node omits it).
    pub success: Option<bool>,
}

/// The member's activation send as the durable submitted-tx record names it (written and
/// fsync'd before the send), so the step resumes to Pending after a restart.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedActivationTx {
    pub hash: String,
    /// When it was recorded (ms since the Unix epoch), if known.
    pub at_ms: Option<u64>,
}

/// Finds the member's latest recorded `MemberBond.activate` send to the given bond.
pub type RecordedLookup<'a> = &'a dyn Fn(&str) -> Option<RecordedActivationTx>;

/// Everything the Activate-validator step reads. Wei as decimal strings (beyond JS range).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationStatus {
    pub bond_address: Option<String>,
    pub bond_deployed: Option<bool>,
    pub bond_activated: Option<bool>,
    pub principal_wei: Option<String>,
    pub unlock_block: Option<u64>,
    pub unlock_timestamp: Option<u64>,
    pub member_token_id: Option<String>,
    pub membership_active: Option<bool>,
    /// The key the registry reads are keyed on (`0x` + 64 hex).
    pub validator_pubkey: Option<String>,
    /// `"bond"` (the activated bond's key) or `"node"` (this node's proposer key).
    pub pubkey_source: Option<String>,
    /// Whether this node has a proposer key on disk (the key activation signs with).
    pub node_key_ready: bool,
    /// This node's proposer key (`0x` + 64 hex), when it has one.
    pub node_pubkey: Option<String>,
    /// `MemberBond.pubkey()`: the key the bond registered with, once activated (zero = none).
    pub bond_pubkey: Option<String>,
    pub registry_status: Option<u8>,
    /// `validatorInfo(pubkey).bonded`: stake still bonded under the key (wei).
    pub registry_bonded_wei: Option<String>,
    /// `validatorInfo(pubkey).escrow`: unbonded stake the registry still holds until the exit
    /// lock ends (wei). Bonded and escrow both zero on an Exiting key means the exit paid out.
    pub registry_escrow_wei: Option<String>,
    pub activation_epoch: Option<u64>,
    pub registry_active: Option<bool>,
    pub epoch_blocks: Option<u64>,
    /// `ValidatorRegistry.currentEpoch()`. `isActive` is already true at registration
    /// (chain#319), so "Validator active" needs `activation_epoch <= current_epoch` too.
    pub current_epoch: Option<u64>,
    pub head_block: Option<u64>,
    pub block_time_ms: Option<u64>,
    pub local_block: Option<u64>,
    pub local_syncing: Option<bool>,
    pub tx: Option<ActivationTx>,
    /// The latest recorded (not forgotten) activation send for this member and bond.
    pub recorded_tx: Option<RecordedActivationTx>,
}

/// One JSON-RPC call; `None` on a transport error or an `error` object (never a default).
fn rpc_result<T: RpcTransport>(t: &T, method: &str, params: Value) -> Option<Value> {
    let resp = t
        .call(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .ok()?;
    if resp.get("error").is_some() {
        return None;
    }
    resp.get("result").cloned()
}

/// `eth_blockNumber`, with `0x0` read as unknown: the node returns `0x0` on error (contract §3),
/// and a live 40204 node is never at block 0.
fn block_number<T: RpcTransport>(t: &T) -> Option<u64> {
    rpc_result(t, "eth_blockNumber", json!([]))
        .and_then(|v| hex_u64(&v))
        .filter(|n| *n > 0)
}

fn hex_u64(v: &Value) -> Option<u64> {
    u64::from_str_radix(v.as_str()?.trim_start_matches("0x"), 16).ok()
}

/// `eth_call` → raw return bytes (an empty `0x` is `Some(vec![])`: a codeless target).
fn eth_call<T: RpcTransport>(t: &T, to: &str, data: &[u8]) -> Option<Vec<u8>> {
    let v = rpc_result(
        t,
        "eth_call",
        json!([{ "to": to, "data": format!("0x{}", hex::encode(data)) }, "latest"]),
    )?;
    hex::decode(v.as_str()?.trim_start_matches("0x")).ok()
}

fn word(ret: &[u8], index: usize) -> Option<[u8; 32]> {
    let start = index.checked_mul(32)?;
    let slice = ret.get(start..start + 32)?;
    let mut w = [0u8; 32];
    w.copy_from_slice(slice);
    Some(w)
}

/// A `uint256` word as u128; `None` beyond u128 (refuse to truncate).
fn word_u128(w: &[u8; 32]) -> Option<u128> {
    if w[..16].iter().any(|b| *b != 0) {
        return None;
    }
    let mut lo = [0u8; 16];
    lo.copy_from_slice(&w[16..]);
    Some(u128::from_be_bytes(lo))
}

fn word_u64(w: &[u8; 32]) -> Option<u64> {
    u64::try_from(word_u128(w)?).ok()
}

/// A bool word: exactly 0 or 1, anything else is not a bool.
fn word_bool(w: &[u8; 32]) -> Option<bool> {
    match word_u128(w)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn with_word(selector: [u8; 4], arg: &[u8; 32]) -> Vec<u8> {
    let mut out = selector.to_vec();
    out.extend_from_slice(arg);
    out
}

fn validate_address(addr: &str) -> Result<String, String> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    if stripped.len() != 40 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("validator-activation: bad member address: {addr}"));
    }
    Ok(format!("0x{}", stripped.to_ascii_lowercase()))
}

fn address_word(addr: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(addr.trim_start_matches("0x")).ok()?;
    if bytes.len() != 20 {
        return None;
    }
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(&bytes);
    Some(w)
}

/// Contract addresses the read targets (the bundled 40204 book in production).
pub struct Targets<'a> {
    pub vault: &'a str,
    pub sbt: &'a str,
    pub registry: &'a str,
}

/// **The read.** `public` is the 40204 RPC, `local` this node's RPC (`None` when there is no
/// node to ask), `node_pubkey` this node's proposer key when it has one, `tx_hash` a sent
/// activation to look up.
pub fn read_activation_status<P: RpcTransport, L: RpcTransport>(
    public: &P,
    local: Option<&L>,
    targets: &Targets<'_>,
    member: &str,
    node_pubkey: Option<[u8; 32]>,
    tx_hash: Option<&str>,
    recorded: RecordedLookup<'_>,
) -> Result<ActivationStatus, String> {
    let member = validate_address(member)?;
    let member_word = address_word(&member)
        .ok_or_else(|| "validator-activation: bad member address".to_string())?;
    let mut st = ActivationStatus {
        node_key_ready: node_pubkey.is_some(),
        node_pubkey: node_pubkey.map(|k| format!("0x{}", hex::encode(k))),
        ..ActivationStatus::default()
    };

    // The member's bond. CREATE2-deterministic: the address alone proves nothing.
    let bond = eth_call(public, targets.vault, &with_word(BOND_OF, &member_word))
        .and_then(|r| word(&r, 0))
        .map(|w| format!("0x{}", hex::encode(&w[12..])));
    st.bond_address = bond.clone();

    let mut bond_pubkey: Option<[u8; 32]> = None;
    if let Some(bond) = bond.as_deref() {
        // A codeless address answers `eth_call` with an empty return: no bond yet.
        match eth_call(public, bond, &ACTIVATED) {
            Some(r) if r.is_empty() => st.bond_deployed = Some(false),
            Some(r) => {
                st.bond_deployed = Some(true);
                st.bond_activated = word(&r, 0).as_ref().and_then(word_bool);
                let read = |sel: [u8; 4]| eth_call(public, bond, &sel).and_then(|r| word(&r, 0));
                st.principal_wei = read(PRINCIPAL)
                    .as_ref()
                    .and_then(word_u128)
                    .map(|v| v.to_string());
                st.unlock_block = read(UNLOCK_BLOCK).as_ref().and_then(word_u64);
                st.unlock_timestamp = read(UNLOCK_TIMESTAMP).as_ref().and_then(word_u64);
                let token = read(MEMBER_TOKEN_ID);
                st.member_token_id = token.as_ref().and_then(word_u128).map(|v| v.to_string());
                if let Some(token) = token {
                    st.membership_active =
                        eth_call(public, targets.sbt, &with_word(SBT_IS_ACTIVE, &token))
                            .and_then(|r| word(&r, 0))
                            .as_ref()
                            .and_then(word_bool);
                }
                bond_pubkey = read(BOND_PUBKEY).filter(|w| w.iter().any(|b| *b != 0));
            }
            None => {}
        }
    }

    // Registry reads are keyed on the bond's registered key once it has one, else this node's.
    st.bond_pubkey = bond_pubkey.map(|k| format!("0x{}", hex::encode(k)));
    let (pubkey, source) = match (bond_pubkey, node_pubkey) {
        (Some(k), _) => (Some(k), Some("bond")),
        (None, Some(k)) => (Some(k), Some("node")),
        (None, None) => (None, None),
    };
    st.pubkey_source = source.map(str::to_string);
    if let Some(pk) = pubkey {
        st.validator_pubkey = Some(format!("0x{}", hex::encode(pk)));
        if let Some(info) = eth_call(public, targets.registry, &with_word(VALIDATOR_INFO, &pk)) {
            // (staker, bonded, rewards, escrow, admissionMin, actEpoch, exitEpoch,
            //  escrowUnlock, status, claimableNow)
            st.activation_epoch = word(&info, 5).as_ref().and_then(word_u64);
            st.registry_status = word(&info, 8)
                .as_ref()
                .and_then(word_u64)
                .filter(|s| *s <= 3)
                .map(|s| s as u8);
        }
        st.registry_active = eth_call(
            public,
            targets.registry,
            &with_word(REGISTRY_IS_ACTIVE, &pk),
        )
        .and_then(|r| word(&r, 0))
        .as_ref()
        .and_then(word_bool);
    }
    st.current_epoch = eth_call(public, targets.registry, &CURRENT_EPOCH)
        .and_then(|r| word(&r, 0))
        .as_ref()
        .and_then(word_u64);
    st.epoch_blocks = eth_call(public, targets.registry, &EPOCH)
        .and_then(|r| word(&r, 0))
        .as_ref()
        .and_then(word_u64)
        .filter(|e| *e > 0);

    // Network head and the measured block time.
    st.head_block = block_number(public);
    if let Some(head) = st.head_block.filter(|h| *h >= BLOCK_TIME_SAMPLE) {
        let ts = |n: u64| {
            rpc_result(
                public,
                "eth_getBlockByNumber",
                json!([format!("0x{n:x}"), false]),
            )
            .and_then(|b| b.get("timestamp").and_then(hex_u64))
        };
        if let (Some(t1), Some(t0)) = (ts(head), ts(head - BLOCK_TIME_SAMPLE)) {
            st.block_time_ms = measured_block_time_ms(t0, t1, BLOCK_TIME_SAMPLE);
        }
    }

    // This node: `eth_syncing` is `false` when caught up, an object while syncing.
    if let Some(local) = local {
        st.local_block = block_number(local);
        st.local_syncing = rpc_result(local, "eth_syncing", json!([])).and_then(|v| match v {
            Value::Bool(b) => Some(b),
            Value::Object(_) => Some(true),
            _ => None,
        });
    }

    // The durable record of this member's activation send to this bond (#347's fsync'd log),
    // looked up only once the bond address is read. Its receipt is read when the UI has no hash
    // of its own (after a restart).
    st.recorded_tx = st.bond_address.as_deref().and_then(recorded);
    let recorded_hash = st.recorded_tx.as_ref().map(|r| r.hash.clone());
    let tx_hash = tx_hash
        .filter(|h| !h.is_empty())
        .or(recorded_hash.as_deref());
    if let Some(hash) = tx_hash {
        let mut tx = ActivationTx {
            hash: hash.to_string(),
            included: None,
            block_number: None,
            success: None,
        };
        match public.call(json!({
            "jsonrpc": "2.0", "id": 1, "method": "eth_getTransactionReceipt", "params": [hash]
        })) {
            Ok(resp) if resp.get("error").is_none() => match resp.get("result") {
                Some(Value::Null) | None => tx.included = Some(false),
                Some(r) => {
                    tx.included = Some(true);
                    tx.block_number = r.get("blockNumber").and_then(hex_u64);
                    tx.success = r.get("status").and_then(hex_u64).map(|s| s == 1);
                }
            },
            _ => {}
        }
        st.tx = Some(tx);
    }
    Ok(st)
}

/// Milliseconds per block over `blocks` blocks between timestamps `t0` and `t1` (seconds).
/// `None` when the span is not positive: a measurement, never a guess.
pub fn measured_block_time_ms(t0: u64, t1: u64, blocks: u64) -> Option<u64> {
    if blocks == 0 || t1 <= t0 {
        return None;
    }
    Some((t1 - t0).saturating_mul(1000) / blocks)
}

fn parse_pubkey(hex_str: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(hex_str.trim_start_matches("0x")).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&bytes);
    Some(k)
}

/// `validator_activation_status` command — the Activate-validator step's one read. Runs on the
/// blocking pool (it makes ~16 sequential RPC round trips).
#[tauri::command]
pub async fn validator_activation_status(
    app_h: tauri::AppHandle,
    member_address: String,
    tx_hash: Option<String>,
) -> std::result::Result<ActivationStatus, String> {
    crate::blocking::off_main(move || {
        let node = tauri::Manager::try_state::<crate::node::NodeState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        validator_activation_status_sync(node, ceremony, &member_address, tx_hash.as_deref())
    })
    .await
}

/// Blocking body of [`validator_activation_status`]; reached only through `off_main`.
pub fn validator_activation_status_sync(
    node: tauri::State<'_, crate::node::NodeState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    member_address: &str,
    tx_hash: Option<&str>,
) -> std::result::Result<ActivationStatus, String> {
    let node_pubkey = node
        .0
        .proposer_pubkey()
        .ok()
        .as_deref()
        .and_then(parse_pubkey);
    let local = crate::rpc::HttpTransport::new(node.0.local_rpc_url());
    let public = crate::rpc::HttpTransport::citrate();
    // The ceremony's durable submitted-tx record (#347): the activation send is written there,
    // fsync'd, before it is sent.
    let log = ceremony.0.submitted_log();
    let activate_selector = format!(
        "0x{}",
        hex::encode(crate::validator::MEMBER_BOND_ACTIVATE_SELECTOR)
    );
    let targets = Targets {
        vault: crate::addresses::membership_stake_vault(),
        sbt: crate::addresses::citrate_member_sbt(),
        registry: crate::addresses::validator_registry(),
    };
    read_activation_status(
        &public,
        Some(&local),
        &targets,
        member_address,
        node_pubkey,
        tx_hash,
        &|bond: &str| {
            log.latest_call(
                member_address,
                &citrate_core_kit::submitted_log::CallMeta {
                    to: bond.to_string(),
                    selector: activate_selector.clone(),
                },
            )
            .map(|c| RecordedActivationTx {
                hash: c.hash,
                at_ms: c.at_ms,
            })
        },
    )
}

/// `validator_activation_forget` command — S7/Node Forget: mark the recorded activation send
/// `tx_hash` as forgotten in the durable submitted-tx record (fsync'd). The UI applies the
/// 10-minute unseen rule first. The hash stays recorded, so re-sending those bytes is never a
/// first submission. `Ok(false)` when the record does not hold it; an error when the write
/// fails (the UI then keeps the activation as Pending).
#[tauri::command]
pub async fn validator_activation_forget(
    app_h: tauri::AppHandle,
    tx_hash: String,
) -> std::result::Result<bool, String> {
    crate::blocking::off_main(move || {
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        ceremony.0.submitted_log().forget(&tx_hash)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("validator_activation_tests.rs");
}
