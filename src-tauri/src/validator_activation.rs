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
//! | `exitOutcome`             | `ValidatorRegistry` logs under the key after its exit (`eth_getLogs`: |
//! |                           | `UnbondInitiated` / `Evicted`, `Slashed`, `Withdrawn`), read only for |
//! |                           | an Exiting key with no bonded stake and no escrow left        |
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

/// Aggregate wall-clock budget for one activation read (~18 sequential RPCs at up to 6 s each).
/// Reads not started inside it fail as unknown (None), so the gate fails closed. The whole command
/// is bounded by this plus one RPC timeout; the webview deadline (`invoke.ts`) sits above that.
pub const READ_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// An RPC transport that refuses new calls once `deadline` has passed (shared by the public and
/// local transports of one read).
pub struct Budgeted<'a, T: RpcTransport> {
    inner: &'a T,
    deadline: std::time::Instant,
}

impl<'a, T: RpcTransport> Budgeted<'a, T> {
    pub fn new(inner: &'a T, deadline: std::time::Instant) -> Self {
        Budgeted { inner, deadline }
    }
}

impl<T: RpcTransport> RpcTransport for Budgeted<'_, T> {
    fn call(&self, body: Value) -> Result<Value, crate::rpc::RpcError> {
        if std::time::Instant::now() >= self.deadline {
            return Err(crate::rpc::RpcError::Transport(
                "validator-activation: read budget spent".into(),
            ));
        }
        self.inner.call(body)
    }
}

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

/// `ValidatorRegistry` event topics (topic0 = keccak256 of the signature), drift-tested in the
/// tests. Every one of them has the validator key as topic1.
/// `UnbondInitiated(bytes32 indexed pubkey, uint256 amount, uint256 remaining, uint64 unlockEpoch)`.
pub const UNBOND_INITIATED_TOPIC: &str =
    "0x61a86f4e214db2a4a09d1e3c370fbcbc230c09ed03e605870240d70581123a0d";
/// `Evicted(bytes32 indexed pubkey, uint256 incumbentStake, bytes32 indexed byPubkey, uint256 newcomerStake)`.
pub const EVICTED_TOPIC: &str =
    "0x581d34ea5e5b3c162c0537cc0ec93c68f7905cf8e9642584a3927e95b641bca4";
/// `Withdrawn(bytes32 indexed pubkey, address indexed staker, uint256 amount)`.
pub const WITHDRAWN_TOPIC: &str =
    "0x04eda370f8b8612fa7266d7ebbd41af9d694e19793fe9d9ff31b3ddbd99b08e1";
/// `Slashed(bytes32 indexed pubkey, SlashTier tier, uint256 penalty, address indexed reporter, uint256 bounty)`.
pub const SLASHED_TOPIC: &str =
    "0xe7e5a0e7e51cdf3695c3cb141d9a9d28b6247245dd98adee084b9a89afdd0499";

/// The node serves at most 1,001 blocks per `eth_getLogs` and silently cuts a longer range, so
/// the exit scan asks for exactly this many blocks at a time.
pub const LOG_CHUNK_BLOCKS: u64 = 1000;
/// How many chunks the exit scan reads before it gives up and reports the outcome as not read
/// (32,000 blocks, ~18 h at 2 s; the registry's exit lock is 6 epochs = 6,000 blocks).
pub const EXIT_SCAN_MAX_CHUNKS: u64 = 32;

/// What became of an exit once the registry holds nothing under the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExitOutcomeKind {
    /// Proven paid out in full: the exit escrowed at least the bond's principal, a `Withdrawn`
    /// paid at least that, and no `Slashed` was logged under the key from the exit to the payout.
    Returned,
    /// A `Slashed` was logged under the key before the payout (or with none), or the exit
    /// escrowed less than the bond's principal.
    Slashed,
    /// Neither could be shown from the logs read (a failed read, no exit event, or no payout
    /// within the scan).
    NotRead,
}

/// The exit outcome as read from the registry's logs. Wei as decimal strings.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitOutcome {
    pub outcome: ExitOutcomeKind,
    /// Block of the full exit (`UnbondInitiated` with `remaining == 0`, or `Evicted`).
    pub exit_block: Option<u64>,
    /// What the exit moved into escrow (`UnbondInitiated.amount` / `Evicted.incumbentStake`).
    pub escrowed_wei: Option<String>,
    /// The first `Withdrawn.amount` at or after the exit, and its block.
    pub paid_out_wei: Option<String>,
    pub paid_out_block: Option<u64>,
    /// Sum of the `Slashed.penalty` values logged from the exit window up to the payout (or to
    /// the end of the scan). `None` when none was seen.
    pub slashed_wei: Option<String>,
    /// The blocks the scan covered.
    pub scanned_from: u64,
    pub scanned_to: Option<u64>,
    /// Why the outcome is `notRead`.
    pub reason: Option<String>,
}

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
    /// lock ends (wei). Bonded and escrow both zero on an Exiting key means the exit SETTLED: paid
    /// out or slashed away (a non-Byzantine slash keeps the key Exiting), see `exit_outcome`.
    pub registry_escrow_wei: Option<String>,
    /// Read only when the key is Exiting with bonded and escrow both zero: whether the exit was
    /// paid out or slashed. `None` otherwise (and the step never says "returned" without it).
    pub exit_outcome: Option<ExitOutcome>,
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
    let mut exit_epoch: Option<u64> = None;
    if let Some(pk) = pubkey {
        st.validator_pubkey = Some(format!("0x{}", hex::encode(pk)));
        if let Some(info) = eth_call(public, targets.registry, &with_word(VALIDATOR_INFO, &pk)) {
            // (staker, bonded, rewards, escrow, admissionMin, actEpoch, exitEpoch,
            //  escrowUnlock, status, claimableNow)
            st.registry_bonded_wei = word(&info, 1)
                .as_ref()
                .and_then(word_u128)
                .map(|v| v.to_string());
            st.registry_escrow_wei = word(&info, 3)
                .as_ref()
                .and_then(word_u128)
                .map(|v| v.to_string());
            st.activation_epoch = word(&info, 5).as_ref().and_then(word_u64);
            exit_epoch = word(&info, 6).as_ref().and_then(word_u64);
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

    // An Exiting key with nothing left under it has settled; say how only from the logs.
    if let Some(pk) = pubkey {
        if exit_settled(&st) {
            st.exit_outcome = Some(read_exit_outcome(
                public,
                targets.registry,
                &pk,
                exit_epoch,
                st.epoch_blocks,
                st.head_block,
                st.principal_wei.as_deref(),
            ));
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

/// The key is Exiting and the registry holds neither bonded stake nor escrow under it (both
/// read). The exit has settled, but this alone does not say whether it paid out or was slashed.
fn exit_settled(st: &ActivationStatus) -> bool {
    st.registry_status == Some(2)
        && st.registry_bonded_wei.as_deref() == Some("0")
        && st.registry_escrow_wei.as_deref() == Some("0")
}

/// One registry log as the exit scan uses it.
struct RegLog {
    topic0: String,
    /// (block, logIndex): the order logs happened in. A missing logIndex orders a `Slashed` first
    /// and a payout last within its block, so a doubt always counts against "returned".
    block: u64,
    index: Option<u64>,
    data: Vec<u8>,
}

fn parse_reg_log(v: &Value) -> Option<RegLog> {
    let topic0 = v.get("topics")?.get(0)?.as_str()?.to_ascii_lowercase();
    let block = v.get("blockNumber").and_then(hex_u64)?;
    let index = v.get("logIndex").and_then(hex_u64);
    let data = hex::decode(v.get("data")?.as_str()?.trim_start_matches("0x")).ok()?;
    Some(RegLog {
        topic0,
        block,
        index,
        data,
    })
}

fn data_u128(l: &RegLog, i: usize) -> Option<u128> {
    word(&l.data, i).as_ref().and_then(word_u128)
}

/// The full exit: an unbond of everything left, or an eviction.
fn is_full_exit(l: &RegLog) -> bool {
    (l.topic0 == UNBOND_INITIATED_TOPIC && data_u128(l, 1) == Some(0)) || l.topic0 == EVICTED_TOPIC
}

/// A `Withdrawn` logged after the full exit has been seen (the scan can stop there).
fn payout_after_exit(logs: &[RegLog]) -> bool {
    let Some(exit) = logs
        .iter()
        .filter(|l| is_full_exit(l))
        .map(|l| (l.block, l.index.unwrap_or(0)))
        .min()
    else {
        return false;
    };
    logs.iter()
        .any(|l| l.topic0 == WITHDRAWN_TOPIC && (l.block, l.index.unwrap_or(u64::MAX)) > exit)
}

/// How a settled exit ended, from the registry's own logs under the key (UI truth contract rule
/// 2: a claim needs its read). The scan starts at the exit epoch and reads whole chunks the node
/// serves until it reaches the payout, the head, or [`EXIT_SCAN_MAX_CHUNKS`].
///
/// "Returned" needs all of: the full exit (`UnbondInitiated` with `remaining == 0`, or `Evicted`)
/// escrowed at least the bond's principal; the first `Withdrawn` after it paid at least that; and
/// no `Slashed` was logged under the key from the exit epoch to that payout. Anything else that
/// shows a slash is `Slashed`; everything else is `NotRead`, with the reason. The exit is the
/// earliest log the proof needs, so pruned history can only hide the proof, never a slash after it.
fn read_exit_outcome<T: RpcTransport>(
    public: &T,
    registry: &str,
    pubkey: &[u8; 32],
    exit_epoch: Option<u64>,
    epoch_blocks: Option<u64>,
    head: Option<u64>,
    principal_wei: Option<&str>,
) -> ExitOutcome {
    let mut out = ExitOutcome {
        outcome: ExitOutcomeKind::NotRead,
        exit_block: None,
        escrowed_wei: None,
        paid_out_wei: None,
        paid_out_block: None,
        slashed_wei: None,
        scanned_from: 0,
        scanned_to: None,
        reason: None,
    };
    let not_read = |mut o: ExitOutcome, why: &str| {
        o.outcome = ExitOutcomeKind::NotRead;
        o.reason = Some(why.to_string());
        o
    };
    let (Some(exit_epoch), Some(epoch_blocks), Some(head)) = (exit_epoch, epoch_blocks, head)
    else {
        return not_read(out, "the exit epoch, epoch length or head was not read");
    };
    let Some(principal) = principal_wei.and_then(|p| p.parse::<u128>().ok()) else {
        return not_read(out, "the bond's principal was not read");
    };
    let Some(from) = exit_epoch.checked_mul(epoch_blocks) else {
        return not_read(out, "the exit epoch is out of range");
    };
    out.scanned_from = from;
    let key_topic = format!("0x{}", hex::encode(pubkey));

    let mut logs: Vec<RegLog> = Vec::new();
    let mut start = from;
    let mut chunks = 0u64;
    while start <= head && chunks < EXIT_SCAN_MAX_CHUNKS {
        let end = start.saturating_add(LOG_CHUNK_BLOCKS - 1).min(head);
        let filter = json!([{
            "address": registry,
            "fromBlock": format!("0x{start:x}"),
            "toBlock": format!("0x{end:x}"),
            "topics": [
                [UNBOND_INITIATED_TOPIC, EVICTED_TOPIC, WITHDRAWN_TOPIC, SLASHED_TOPIC],
                key_topic,
            ],
        }]);
        let Some(Value::Array(found)) = rpc_result(public, "eth_getLogs", filter) else {
            return not_read(out, "the registry's logs could not be read");
        };
        for v in &found {
            let Some(l) = parse_reg_log(v) else {
                return not_read(out, "a registry log could not be decoded");
            };
            // The node filters by range; a log outside it is not trusted.
            if l.block < start || l.block > end {
                return not_read(out, "a registry log came back outside the asked range");
            }
            logs.push(l);
        }
        chunks += 1;
        out.scanned_to = Some(end);
        if payout_after_exit(&logs) || end == head {
            break;
        }
        start = end + 1;
    }
    logs.sort_by_key(|l| (l.block, l.index.unwrap_or(0)));

    let exit = logs
        .iter()
        .find(|l| is_full_exit(l))
        .map(|l| (l, data_u128(l, 0)));
    let exit_pos = exit.map(|(l, _)| (l.block, l.index.unwrap_or(0)));
    out.exit_block = exit.map(|(l, _)| l.block);
    out.escrowed_wei = exit.and_then(|(_, a)| a).map(|a| a.to_string());

    // The payout: the first Withdrawn after the exit.
    let payout = exit_pos.and_then(|ep| {
        logs.iter()
            .find(|l| l.topic0 == WITHDRAWN_TOPIC && (l.block, l.index.unwrap_or(u64::MAX)) > ep)
    });
    let payout_pos = payout.map(|l| (l.block, l.index.unwrap_or(u64::MAX)));
    if let Some(p) = payout {
        out.paid_out_block = Some(p.block);
        match data_u128(p, 0) {
            Some(a) => out.paid_out_wei = Some(a.to_string()),
            None => return not_read(out, "the payout amount could not be decoded"),
        }
    }

    // Slashes from the exit epoch up to the payout (or to the end of the scan without one).
    let mut slashed: u128 = 0;
    let mut any_slash = false;
    for l in logs.iter().filter(|l| l.topic0 == SLASHED_TOPIC) {
        let pos = (l.block, l.index.unwrap_or(0));
        if payout_pos.is_some_and(|pp| pos >= pp) {
            continue;
        }
        let Some(penalty) = data_u128(l, 1) else {
            return not_read(out, "a slash penalty could not be decoded");
        };
        any_slash = true;
        slashed = slashed.saturating_add(penalty);
    }
    if any_slash {
        out.slashed_wei = Some(slashed.to_string());
        out.outcome = ExitOutcomeKind::Slashed;
        return out;
    }

    let Some((_, escrowed)) = exit else {
        return not_read(out, "no exit was logged from the exit epoch on");
    };
    let Some(escrowed) = escrowed else {
        return not_read(out, "the exit amount could not be decoded");
    };
    if escrowed < principal {
        // A bond's stake only shrinks by a slash (it never unbonds part of it).
        out.outcome = ExitOutcomeKind::Slashed;
        return out;
    }
    let Some(paid) = out
        .paid_out_wei
        .as_deref()
        .and_then(|p| p.parse::<u128>().ok())
    else {
        return not_read(out, "no payout was logged in the blocks scanned");
    };
    if paid < escrowed {
        return not_read(out, "the payout logged is less than the exit escrowed");
    }
    out.outcome = ExitOutcomeKind::Returned;
    out
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
    // One shared deadline: reads not started inside READ_BUDGET are unknown (fail closed).
    let deadline = std::time::Instant::now() + READ_BUDGET;
    let local_budgeted = Budgeted::new(&local, deadline);
    let public_budgeted = Budgeted::new(&public, deadline);
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
        &public_budgeted,
        Some(&local_budgeted),
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
