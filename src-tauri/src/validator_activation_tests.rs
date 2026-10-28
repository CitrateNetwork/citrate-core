// Join-8 — the Activate-validator step's read. Load-bearing properties:
//   * pinned selectors are keccak256 of their signatures (drift tripwires);
//   * every field comes from its named source, and a failed read is None, never 0 / false;
//   * a codeless bond reads "not deployed", not "not activated";
//   * registry reads key on the bond's registered key once it has one, else this node's key;
//   * the block time is measured over 100 blocks, never assumed.
use super::*;
use crate::rpc::{RpcError, RpcTransport};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::HashMap;

const VAULT: &str = "0x00000000000000000000000000000000000000aa";
const SBT: &str = "0x00000000000000000000000000000000000000bb";
const REGISTRY: &str = "0x00000000000000000000000000000000000000cc";
const MEMBER: &str = "0x1111111111111111111111111111111111111111";
const BOND: &str = "0x2222222222222222222222222222222222222222";

fn targets() -> Targets<'static> {
    Targets { vault: VAULT, sbt: SBT, registry: REGISTRY }
}

/// A test transport that answers by (method, to, selector). Unscripted calls fail, so a
/// missing answer reads as "unknown", exactly like a failed live read.
#[derive(Default)]
struct Chain {
    calls: RefCell<HashMap<(String, String), JsonValue>>,
    methods: RefCell<HashMap<String, JsonValue>>,
    blocks: RefCell<HashMap<u64, u64>>,
    receipts: RefCell<HashMap<String, JsonValue>>,
    seen: RefCell<Vec<JsonValue>>,
    /// Logs `eth_getLogs` filters by address, topic0 (one or a list), topic1 and block range
    /// (a log with `_servedAt: N` is returned for any range that contains block N instead of its
    /// own block, like a node that answers with the wrong logs).
    logs: RefCell<Vec<JsonValue>>,
    /// `eth_getLogs` fails for any range that reaches this block.
    logs_fail_from: RefCell<Option<u64>>,
    /// `eth_getLogs` fails for the one range that contains this block (a failed middle chunk).
    logs_fail_at: RefCell<Option<u64>>,
}
impl Chain {
    fn call(self, to: &str, selector: [u8; 4], ret: String) -> Self {
        self.calls
            .borrow_mut()
            .insert((to.to_string(), hex::encode(selector)), JsonValue::String(ret));
        self
    }
    fn method(self, m: &str, result: JsonValue) -> Self {
        self.methods.borrow_mut().insert(m.to_string(), result);
        self
    }
    fn block(self, n: u64, ts: u64) -> Self {
        self.blocks.borrow_mut().insert(n, ts);
        self
    }
    fn receipt(self, hash: &str, r: JsonValue) -> Self {
        self.receipts.borrow_mut().insert(hash.to_string(), r);
        self
    }
    fn log(self, l: JsonValue) -> Self {
        self.logs.borrow_mut().push(l);
        self
    }
    fn logs_fail_at(self, block: u64) -> Self {
        *self.logs_fail_at.borrow_mut() = Some(block);
        self
    }
    fn logs_fail_from(self, block: u64) -> Self {
        *self.logs_fail_from.borrow_mut() = Some(block);
        self
    }
    /// The `eth_getLogs` requests made, as (fromBlock, toBlock).
    fn log_ranges(&self) -> Vec<(u64, u64)> {
        let hex = |v: &JsonValue| u64::from_str_radix(v.as_str().unwrap_or("").trim_start_matches("0x"), 16).unwrap_or(u64::MAX);
        self.seen
            .borrow()
            .iter()
            .filter(|b| b["method"] == "eth_getLogs")
            .map(|b| (hex(&b["params"][0]["fromBlock"]), hex(&b["params"][0]["toBlock"])))
            .collect()
    }
    fn get_logs(&self, filter: &JsonValue) -> std::result::Result<JsonValue, RpcError> {
        let hex = |v: &JsonValue| u64::from_str_radix(v.as_str().unwrap_or("").trim_start_matches("0x"), 16).ok();
        let (Some(from), Some(to)) = (hex(&filter["fromBlock"]), hex(&filter["toBlock"])) else {
            return Err(RpcError::Transport("bad range".into()));
        };
        if matches!(*self.logs_fail_from.borrow(), Some(f) if to >= f)
            || matches!(*self.logs_fail_at.borrow(), Some(f) if from <= f && f <= to)
        {
            return Err(RpcError::Transport("logs unavailable".into()));
        }
        // The node serves at most 1,001 blocks and silently cuts the rest.
        let to = to.min(from + 1000);
        let lc = |v: &JsonValue| v.as_str().unwrap_or("").to_ascii_lowercase();
        let t0: Vec<String> = match &filter["topics"][0] {
            JsonValue::Array(a) => a.iter().map(lc).collect(),
            JsonValue::String(_) => vec![lc(&filter["topics"][0])],
            _ => vec![],
        };
        let t1 = filter["topics"][1].as_str().map(str::to_ascii_lowercase);
        let out: Vec<JsonValue> = self
            .logs
            .borrow()
            .iter()
            .filter(|l| lc(&l["address"]) == lc(&filter["address"]))
            .filter(|l| {
                let at = l.get("_servedAt").and_then(JsonValue::as_u64).or_else(|| hex(&l["blockNumber"]));
                at.is_some_and(|b| b >= from && b <= to)
            })
            .filter(|l| t0.is_empty() || t0.contains(&lc(&l["topics"][0])))
            .filter(|l| t1.as_ref().is_none_or(|t| lc(&l["topics"][1]) == *t))
            .cloned()
            .collect();
        Ok(JsonValue::Array(out))
    }
}
impl RpcTransport for Chain {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.seen.borrow_mut().push(body.clone());
        let method = body["method"].as_str().unwrap_or("").to_string();
        let ok = |r: JsonValue| Ok(serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": r }));
        match method.as_str() {
            "eth_call" => {
                let to = body["params"][0]["to"].as_str().unwrap_or("").to_string();
                let data = body["params"][0]["data"].as_str().unwrap_or("");
                let sel = data.trim_start_matches("0x").get(..8).unwrap_or("").to_string();
                match self.calls.borrow().get(&(to, sel)) {
                    Some(v) => ok(v.clone()),
                    None => Err(RpcError::Transport("unscripted eth_call".into())),
                }
            }
            "eth_getBlockByNumber" => {
                let tag = body["params"][0].as_str().unwrap_or("");
                let n = u64::from_str_radix(tag.trim_start_matches("0x"), 16).unwrap_or(u64::MAX);
                match self.blocks.borrow().get(&n) {
                    Some(ts) => ok(serde_json::json!({ "number": tag, "timestamp": format!("0x{ts:x}") })),
                    None => Err(RpcError::Transport("unscripted block".into())),
                }
            }
            "eth_getLogs" => self.get_logs(&body["params"][0]).and_then(ok),
            "eth_getTransactionReceipt" => {
                let h = body["params"][0].as_str().unwrap_or("");
                ok(self.receipts.borrow().get(h).cloned().unwrap_or(JsonValue::Null))
            }
            m => match self.methods.borrow().get(m) {
                Some(v) => ok(v.clone()),
                None => Err(RpcError::Transport("unscripted method".into())),
            },
        }
    }
}

fn w(n: u128) -> String {
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&n.to_be_bytes());
    hex::encode(word)
}
fn ret(words: &[String]) -> String {
    format!("0x{}", words.concat())
}
fn addr_word(a: &str) -> String {
    format!("{:0>64}", a.trim_start_matches("0x"))
}
fn info(act_epoch: u128, status: u128) -> String {
    let mut words = vec![addr_word(BOND), w(32_000), w(0), w(0), w(32_000), w(act_epoch), w(0), w(0), w(status), w(0)];
    words.truncate(10);
    ret(&words)
}
const PRINCIPAL_WEI: u128 = 32_000 * 1_000_000_000_000_000_000;
const NODE_KEY: [u8; 32] = [0x07; 32];
const BOND_KEY: [u8; 32] = [0x09; 32];

/// A member whose grant landed: bond deployed + funded, membership active, not activated.
fn granted(activated: bool, bond_key: Option<[u8; 32]>) -> Chain {
    Chain::default()
        .call(VAULT, BOND_OF, ret(&[addr_word(BOND)]))
        .call(BOND, ACTIVATED, ret(&[w(activated as u128)]))
        .call(BOND, PRINCIPAL, ret(&[w(PRINCIPAL_WEI)]))
        .call(BOND, UNLOCK_BLOCK, ret(&[w(16_200_000)]))
        .call(BOND, UNLOCK_TIMESTAMP, ret(&[w(1_822_000_000)]))
        .call(BOND, MEMBER_TOKEN_ID, ret(&[w(4)]))
        .call(BOND, BOND_PUBKEY, format!("0x{}", hex::encode(bond_key.unwrap_or([0u8; 32]))))
        .call(SBT, SBT_IS_ACTIVE, ret(&[w(1)]))
        .call(REGISTRY, EPOCH, ret(&[w(1000)]))
        .method("eth_blockNumber", JsonValue::String("0x1f4".into())) // 500
        .block(500, 10_200)
        .block(400, 10_000)
}

fn read(public: &Chain, local: Option<&Chain>, node_key: Option<[u8; 32]>, tx: Option<&str>) -> ActivationStatus {
    read_activation_status(public, local, &targets(), MEMBER, node_key, tx, &|_| None).expect("valid member")
}

fn keccak4(sig: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

#[test]
fn pinned_selectors_are_keccak_of_their_signatures() {
    assert_eq!(BOND_OF, keccak4("bondOf(address)"));
    assert_eq!(ACTIVATED, keccak4("activated()"));
    assert_eq!(PRINCIPAL, keccak4("principal()"));
    assert_eq!(MEMBER_TOKEN_ID, keccak4("memberTokenId()"));
    assert_eq!(BOND_PUBKEY, keccak4("pubkey()"));
    assert_eq!(UNLOCK_BLOCK, keccak4("unlockBlock()"));
    assert_eq!(UNLOCK_TIMESTAMP, keccak4("unlockTimestamp()"));
    assert_eq!(SBT_IS_ACTIVE, keccak4("isActive(uint256)"));
    assert_eq!(VALIDATOR_INFO, keccak4("validatorInfo(bytes32)"));
    assert_eq!(REGISTRY_IS_ACTIVE, keccak4("isActive(bytes32)"));
    assert_eq!(EPOCH, keccak4("EPOCH()"));
    assert_eq!(CURRENT_EPOCH, keccak4("currentEpoch()"));
    assert_eq!(CURRENT_EPOCH, [0x76, 0x67, 0x18, 0x08]);
}

#[test]
fn a_bad_member_address_fails_closed_before_any_read() {
    let chain = Chain::default();
    let err = read_activation_status(&chain, None::<&Chain>, &targets(), "0x12", None, None, &|_| None);
    assert!(err.is_err());
    assert!(chain.seen.borrow().is_empty(), "no RPC for a malformed address");
}

#[test]
fn granted_not_activated_reads_the_bond_and_the_node_key_status_none() {
    let chain = granted(false, None)
        .call(REGISTRY, VALIDATOR_INFO, info(0, 0))
        .call(REGISTRY, REGISTRY_IS_ACTIVE, ret(&[w(0)]));
    let st = read(&chain, None, Some(NODE_KEY), None);
    assert_eq!(st.bond_address.as_deref(), Some(BOND));
    assert_eq!(st.bond_deployed, Some(true));
    assert_eq!(st.bond_activated, Some(false));
    assert_eq!(st.principal_wei.as_deref(), Some(PRINCIPAL_WEI.to_string().as_str()));
    assert_eq!(st.unlock_block, Some(16_200_000));
    assert_eq!(st.unlock_timestamp, Some(1_822_000_000));
    assert_eq!(st.member_token_id.as_deref(), Some("4"));
    assert_eq!(st.membership_active, Some(true));
    assert_eq!(st.pubkey_source.as_deref(), Some("node"));
    assert_eq!(st.validator_pubkey, Some(format!("0x{}", hex::encode(NODE_KEY))));
    assert!(st.node_key_ready);
    assert_eq!(st.registry_status, Some(0));
    assert_eq!(st.registry_active, Some(false));
    assert_eq!(st.epoch_blocks, Some(1000));
    assert_eq!(st.head_block, Some(500));
}

#[test]
fn once_activated_the_registry_is_keyed_on_the_bonds_key_not_the_nodes() {
    let bond_key_hex = hex::encode(BOND_KEY);
    let chain = granted(true, Some(BOND_KEY))
        .call(REGISTRY, VALIDATOR_INFO, info(502, 1))
        .call(REGISTRY, REGISTRY_IS_ACTIVE, ret(&[w(1)]));
    let st = read(&chain, None, Some(NODE_KEY), None);
    assert_eq!(st.pubkey_source.as_deref(), Some("bond"));
    assert_eq!(st.validator_pubkey, Some(format!("0x{bond_key_hex}")));
    assert_eq!(st.bond_activated, Some(true));
    assert_eq!(st.registry_status, Some(1));
    assert_eq!(st.activation_epoch, Some(502));
    assert_eq!(st.registry_active, Some(true));
    // The registry calls carried the BOND key.
    let seen = chain.seen.borrow();
    let info_call = seen
        .iter()
        .find(|b| b["params"][0]["data"].as_str().is_some_and(|d| d.starts_with("0x00408838")))
        .expect("validatorInfo read");
    assert!(info_call["params"][0]["data"].as_str().is_some_and(|d| d.ends_with(&bond_key_hex)));
}

#[test]
fn a_codeless_bond_is_not_deployed_and_nothing_downstream_is_invented() {
    let chain = Chain::default()
        .call(VAULT, BOND_OF, ret(&[addr_word(BOND)]))
        .call(BOND, ACTIVATED, "0x".into());
    let st = read(&chain, None, None, None);
    assert_eq!(st.bond_address.as_deref(), Some(BOND));
    assert_eq!(st.bond_deployed, Some(false));
    assert_eq!(st.bond_activated, None);
    assert_eq!(st.membership_active, None);
    assert_eq!(st.principal_wei, None);
    assert_eq!(st.registry_status, None, "no key: the registry is not asked");
    assert!(!st.node_key_ready);
}

#[test]
fn unknown_is_not_zero_every_failed_read_is_none() {
    let chain = Chain::default(); // every call fails
    let local = Chain::default();
    let st = read(&chain, Some(&local), Some(NODE_KEY), None);
    assert_eq!(st.bond_address, None);
    assert_eq!(st.bond_deployed, None);
    assert_eq!(st.bond_activated, None);
    assert_eq!(st.registry_status, None);
    assert_eq!(st.registry_active, None);
    assert_eq!(st.epoch_blocks, None);
    assert_eq!(st.head_block, None);
    assert_eq!(st.block_time_ms, None);
    assert_eq!(st.local_block, None);
    assert_eq!(st.local_syncing, None);
}

#[test]
fn block_time_is_measured_over_the_last_100_blocks() {
    // 200 s over 100 blocks = 2,000 ms per block (40204's measured ~2 s), never 12 s.
    let st = read(&granted(false, None), None, None, None);
    assert_eq!(st.block_time_ms, Some(2_000));
    assert_eq!(measured_block_time_ms(10_000, 10_200, 100), Some(2_000));
    assert_eq!(measured_block_time_ms(10_000, 10_000, 100), None, "no span, no measurement");
    assert_eq!(measured_block_time_ms(10_200, 10_000, 100), None);
}

#[test]
fn local_sync_reads_eth_syncing_false_or_an_object() {
    let synced = Chain::default()
        .method("eth_blockNumber", JsonValue::String("0x1f2".into()))
        .method("eth_syncing", JsonValue::Bool(false));
    let st = read(&Chain::default(), Some(&synced), None, None);
    assert_eq!(st.local_block, Some(498));
    assert_eq!(st.local_syncing, Some(false));

    let syncing = Chain::default()
        .method("eth_blockNumber", JsonValue::String("0x10".into()))
        .method("eth_syncing", serde_json::json!({ "currentBlock": "0x10", "highestBlock": "0x1f4" }));
    let st = read(&Chain::default(), Some(&syncing), None, None);
    assert_eq!(st.local_syncing, Some(true));
}

#[test]
fn a_sent_activation_reads_pending_included_or_reverted_from_its_receipt() {
    let pending = read(&Chain::default(), None, None, Some("0xabc"));
    let tx = pending.tx.expect("tx read");
    assert_eq!(tx.included, Some(false));
    assert_eq!(tx.success, None);

    let ok = Chain::default().receipt("0xabc", serde_json::json!({ "blockNumber": "0x1f5", "status": "0x1" }));
    let tx = read(&ok, None, None, Some("0xabc")).tx.expect("tx read");
    assert_eq!((tx.included, tx.block_number, tx.success), (Some(true), Some(501), Some(true)));

    let reverted = Chain::default().receipt("0xabc", serde_json::json!({ "blockNumber": "0x1f5", "status": "0x0" }));
    let tx = read(&reverted, None, None, Some("0xabc")).tx.expect("tx read");
    assert_eq!(tx.success, Some(false));
}

#[test]
fn a_registry_status_outside_the_enum_is_not_reported() {
    let chain = granted(false, None).call(REGISTRY, VALIDATOR_INFO, info(0, 9));
    let st = read(&chain, None, Some(NODE_KEY), None);
    assert_eq!(st.registry_status, None);
}

#[test]
fn the_command_is_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    assert!(acl.contains("\"validator_activation_status\""));
    assert!(acl.contains("\"validator_activation_forget\""), "Forget writes the durable record");
}

#[test]
fn current_epoch_is_read_so_the_step_can_compare_it_with_act_epoch() {
    // chain#319: isActive is true AT REGISTRATION; activation is actEpoch <= currentEpoch().
    let chain = granted(true, Some(BOND_KEY))
        .call(REGISTRY, VALIDATOR_INFO, info(502, 1))
        .call(REGISTRY, REGISTRY_IS_ACTIVE, ret(&[w(1)]))
        .call(REGISTRY, CURRENT_EPOCH, ret(&[w(500)]));
    let st = read(&chain, None, Some(NODE_KEY), None);
    assert_eq!(st.current_epoch, Some(500));
    assert_eq!(st.activation_epoch, Some(502));
    let st = read(&granted(true, Some(BOND_KEY)), None, None, None);
    assert_eq!(st.current_epoch, None, "an unread epoch is unknown, not 0");
}

#[test]
fn both_keys_are_reported_so_a_mismatch_can_be_shown() {
    let chain = granted(true, Some(BOND_KEY));
    let st = read(&chain, None, Some(NODE_KEY), None);
    assert_eq!(st.bond_pubkey, Some(format!("0x{}", hex::encode(BOND_KEY))));
    assert_eq!(st.node_pubkey, Some(format!("0x{}", hex::encode(NODE_KEY))));
    let st = read(&granted(false, None), None, None, None);
    assert_eq!(st.bond_pubkey, None, "a zero bond key is no key");
    assert_eq!(st.node_pubkey, None);
}

#[test]
fn an_eth_block_number_of_0x0_is_unknown_not_zero() {
    // eth_blockNumber returns 0x0 on error (contract §2/§3): never data.
    let public = Chain::default().method("eth_blockNumber", JsonValue::String("0x0".into()));
    let local = Chain::default()
        .method("eth_blockNumber", JsonValue::String("0x0".into()))
        .method("eth_syncing", JsonValue::Bool(false));
    let st = read(&public, Some(&local), None, None);
    assert_eq!(st.head_block, None);
    assert_eq!(st.local_block, None);
}

// Join-8 x #347: the activation record is the fsync'd submitted-tx record, read back here.
const RECORDED: &str = "0xfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeedfeed";

#[test]
fn a_recorded_activation_is_returned_and_its_receipt_read_without_a_ui_hash() {
    let public = granted(false, None).receipt(RECORDED, JsonValue::Null);
    let asked = RefCell::new(Vec::<String>::new());
    let lookup = |bond: &str| {
        asked.borrow_mut().push(bond.to_string());
        Some(RecordedActivationTx { hash: RECORDED.into(), at_ms: Some(1_800_000_000_000) })
    };
    let st = read_activation_status(&public, None::<&Chain>, &targets(), MEMBER, Some(NODE_KEY), None, &lookup).expect("read");
    assert_eq!(asked.borrow().as_slice(), [BOND.to_string()], "looked up for the bond read from bondOf");
    assert_eq!(st.recorded_tx, Some(RecordedActivationTx { hash: RECORDED.into(), at_ms: Some(1_800_000_000_000) }));
    let tx = st.tx.expect("the recorded tx's receipt is read");
    assert_eq!(tx.hash, RECORDED);
    assert_eq!(tx.included, Some(false));
}

#[test]
fn no_bond_read_means_no_record_lookup_and_no_record() {
    let public = Chain::default(); // every read fails
    let asked = RefCell::new(0);
    let lookup = |_: &str| {
        *asked.borrow_mut() += 1;
        Some(RecordedActivationTx { hash: RECORDED.into(), at_ms: None })
    };
    let st = read_activation_status(&public, None::<&Chain>, &targets(), MEMBER, None, None, &lookup).expect("read");
    assert_eq!(*asked.borrow(), 0);
    assert_eq!(st.recorded_tx, None);
    assert_eq!(st.tx, None);
}

#[test]
fn tripwire_the_command_reads_the_record_from_the_ceremonys_submitted_log() {
    let src = include_str!("validator_activation.rs");
    let start = src.find("pub fn validator_activation_status_sync(").expect("command body");
    let body = &src[start..];
    let body = &body[..body.find("\n}\n").map(|e| e + 3).unwrap_or(body.len())];
    assert!(body.contains("submitted_log()"), "the lookup reads the ceremony's durable record");
    assert!(body.contains("latest_call("), "it looks up the activation call");
    assert!(!body.contains("&|_| None"), "the command never skips the record");
}

/// After a full exit the key stays `Exiting` in the registry for good, so the status word alone
/// cannot tell an exit still in its lock from one that paid out. The read carries what the
/// registry still holds under the key (bonded and escrow), so the step can say which.
#[test]
fn an_exiting_key_reports_what_the_registry_still_holds_bonded_and_escrow() {
    let exiting = |bonded: u128, escrow: u128| {
        let words = [addr_word(BOND), w(bonded), w(0), w(escrow), w(32_000), w(1), w(1), w(7), w(2), w(0)];
        granted(true, Some(BOND_KEY)).call(REGISTRY, VALIDATOR_INFO, ret(&words))
    };
    let st = read(&exiting(0, PRINCIPAL_WEI), None, Some(NODE_KEY), None);
    assert_eq!(st.registry_status, Some(2));
    assert_eq!(st.registry_bonded_wei.as_deref(), Some("0"));
    assert_eq!(st.registry_escrow_wei, Some(PRINCIPAL_WEI.to_string()), "in the exit lock");

    let st = read(&exiting(0, 0), None, Some(NODE_KEY), None);
    assert_eq!(st.registry_status, Some(2), "the key stays Exiting after the payout");
    assert_eq!(st.registry_bonded_wei.as_deref(), Some("0"));
    assert_eq!(st.registry_escrow_wei.as_deref(), Some("0"), "paid out");

    // Unknown is not zero: no validatorInfo read, nothing reported.
    let st = read(&granted(true, Some(BOND_KEY)), None, Some(NODE_KEY), None);
    assert_eq!((st.registry_bonded_wei, st.registry_escrow_wei), (None, None));
}

// ---------------------------------------------------------------------------------------------
// The exit outcome. After a full exit the key stays Exiting for good, and a non-Byzantine slash
// also leaves it Exiting while it draws the penalty from bonded, then escrow, then vested rewards
// (ValidatorRegistry._slash). So "bonded 0 and escrow 0" says the exit SETTLED, not that it paid
// out. "Returned" needs the registry's own logs: the exit escrowed at least the principal, a
// Withdrawn paid at least that, and no Slashed was logged under the key before the payout.
// ---------------------------------------------------------------------------------------------

const SALT_WEI: u128 = 1_000_000_000_000_000_000;

fn keccak32_hex(sig: &str) -> String {
    use sha3::{Digest, Keccak256};
    format!("0x{}", hex::encode(Keccak256::digest(sig.as_bytes())))
}

#[test]
fn pinned_exit_event_topics_are_keccak_of_their_signatures() {
    assert_eq!(UNBOND_INITIATED_TOPIC, keccak32_hex("UnbondInitiated(bytes32,uint256,uint256,uint64)"));
    assert_eq!(EVICTED_TOPIC, keccak32_hex("Evicted(bytes32,uint256,bytes32,uint256)"));
    assert_eq!(WITHDRAWN_TOPIC, keccak32_hex("Withdrawn(bytes32,address,uint256)"));
    // SlashTier is an enum: uint8 in the ABI signature.
    assert_eq!(SLASHED_TOPIC, keccak32_hex("Slashed(bytes32,uint8,uint256,address,uint256)"));
}

fn key_topic() -> String {
    format!("0x{}", hex::encode(BOND_KEY))
}
fn reg_log(block: u64, index: u64, topic0: &str, extra_topics: &[String], data: &[String]) -> JsonValue {
    let mut topics = vec![JsonValue::String(topic0.to_string()), JsonValue::String(key_topic())];
    topics.extend(extra_topics.iter().map(|t| JsonValue::String(t.clone())));
    serde_json::json!({
        "address": REGISTRY,
        "topics": topics,
        "data": format!("0x{}", data.concat()),
        "blockNumber": format!("0x{block:x}"),
        "logIndex": format!("0x{index:x}"),
    })
}
fn unbond(block: u64, amount: u128, remaining: u128) -> JsonValue {
    reg_log(block, 0, UNBOND_INITIATED_TOPIC, &[], &[w(amount), w(remaining), w(7)])
}
fn evicted(block: u64, incumbent: u128) -> JsonValue {
    reg_log(block, 0, EVICTED_TOPIC, &[format!("0x{}", hex::encode([0x0a; 32]))], &[w(incumbent), w(incumbent + 1)])
}
fn withdrawn(block: u64, index: u64, amount: u128) -> JsonValue {
    reg_log(block, index, WITHDRAWN_TOPIC, &[format!("0x{}", addr_word(BOND))], &[w(amount)])
}
fn slashed(block: u64, index: u64, penalty: u128) -> JsonValue {
    // tier Latency (0), no reporter, no bounty.
    reg_log(block, index, SLASHED_TOPIC, &[format!("0x{}", addr_word("0x0"))], &[w(0), w(penalty), w(0)])
}

/// A bond whose key exited in epoch 1 and whose registry entry holds nothing any more.
/// Head 8,000 (epoch 8): the 6-epoch exit lock has passed.
fn settled_exit() -> Chain {
    let words = [addr_word(BOND), w(0), w(0), w(0), w(32_000), w(1), w(1), w(7), w(2), w(0)];
    granted(true, Some(BOND_KEY))
        .call(REGISTRY, VALIDATOR_INFO, ret(&words))
        .method("eth_blockNumber", JsonValue::String("0x1f40".into())) // 8,000
        .block(8000, 30_000)
        .block(7900, 29_800)
}

fn outcome(chain: &Chain) -> ExitOutcome {
    read(chain, None, Some(NODE_KEY), None).exit_outcome.expect("a settled exit reads its outcome")
}

#[test]
fn a_clean_exit_paid_out_in_full_reads_returned_with_the_amounts() {
    let chain = settled_exit()
        .log(unbond(1500, PRINCIPAL_WEI, 0))
        .log(withdrawn(7100, 3, PRINCIPAL_WEI + 5 * SALT_WEI)); // escrow + terminal rewards
    let o = outcome(&chain);
    assert_eq!(o.outcome, ExitOutcomeKind::Returned);
    assert_eq!(o.exit_block, Some(1500));
    assert_eq!(o.escrowed_wei, Some(PRINCIPAL_WEI.to_string()));
    assert_eq!(o.paid_out_wei, Some((PRINCIPAL_WEI + 5 * SALT_WEI).to_string()));
    assert_eq!(o.paid_out_block, Some(7100));
    assert_eq!(o.slashed_wei, None);
    assert_eq!(o.scanned_from, 1000, "from the start of the exit epoch");
}

/// The review case: a non-Byzantine slash on an Exiting key drains the escrow to 0 and leaves the
/// status Exiting; the later terminal withdraw pays only rewards. With large vested rewards the
/// payout can even exceed the principal. Never "returned".
#[test]
fn an_exit_drained_by_a_slash_reads_slashed_even_when_a_later_withdraw_pays_more_than_the_principal() {
    for paid in [5 * SALT_WEI, PRINCIPAL_WEI + 1, 2 * PRINCIPAL_WEI] {
        let chain = settled_exit()
            .log(unbond(1500, PRINCIPAL_WEI, 0))
            .log(slashed(3000, 0, PRINCIPAL_WEI))
            .log(withdrawn(7100, 0, paid));
        let o = outcome(&chain);
        assert_eq!(o.outcome, ExitOutcomeKind::Slashed, "paid {paid}");
        assert_eq!(o.slashed_wei, Some(PRINCIPAL_WEI.to_string()));
        assert_eq!(o.paid_out_wei, Some(paid.to_string()));
    }
}

#[test]
fn a_slash_drained_exit_with_no_withdraw_yet_reads_slashed() {
    let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(slashed(2500, 0, 30_000 * SALT_WEI)).log(slashed(2600, 1, 2_000 * SALT_WEI));
    let o = outcome(&chain);
    assert_eq!(o.outcome, ExitOutcomeKind::Slashed);
    assert_eq!(o.slashed_wei, Some(PRINCIPAL_WEI.to_string()), "penalties summed");
    assert_eq!(o.paid_out_wei, None);
}

#[test]
fn a_slash_in_the_payout_block_before_the_payout_counts_and_one_after_does_not() {
    let before = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(slashed(7100, 1, SALT_WEI)).log(withdrawn(7100, 2, PRINCIPAL_WEI));
    assert_eq!(outcome(&before).outcome, ExitOutcomeKind::Slashed);
    // After the payout the escrow is gone; a later slash can only reach rewards.
    let after = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 2, PRINCIPAL_WEI)).log(slashed(7100, 3, SALT_WEI)).log(slashed(7500, 0, SALT_WEI));
    assert_eq!(outcome(&after).outcome, ExitOutcomeKind::Returned);
}

#[test]
fn a_withdraw_before_the_exit_does_not_end_the_scan_and_is_not_the_payout() {
    let chain = settled_exit().log(withdrawn(1200, 0, SALT_WEI)).log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 0, PRINCIPAL_WEI));
    let o = outcome(&chain);
    assert_eq!(o.outcome, ExitOutcomeKind::Returned);
    assert_eq!(o.paid_out_block, Some(7100));
    assert_eq!(o.paid_out_wei, Some(PRINCIPAL_WEI.to_string()));
}

#[test]
fn an_exit_that_escrowed_less_than_the_principal_reads_slashed() {
    // A slash while Active (before this scan's window) shrank the bond before the exit.
    let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI - 1_600 * SALT_WEI, 0)).log(withdrawn(7100, 0, PRINCIPAL_WEI));
    let o = outcome(&chain);
    assert_eq!(o.outcome, ExitOutcomeKind::Slashed);
    assert_eq!(o.escrowed_wei, Some((PRINCIPAL_WEI - 1_600 * SALT_WEI).to_string()));
}

#[test]
fn a_demotion_exit_slash_with_no_unbond_reads_slashed() {
    // _slash -> _beginExit: Demoted + Slashed in the same tx, no UnbondInitiated.
    let chain = settled_exit().log(slashed(1500, 1, 1_600 * SALT_WEI)).log(withdrawn(7100, 0, 30_400 * SALT_WEI));
    assert_eq!(outcome(&chain).outcome, ExitOutcomeKind::Slashed);
}

#[test]
fn an_eviction_exit_paid_out_in_full_reads_returned() {
    let chain = settled_exit().log(evicted(1500, PRINCIPAL_WEI)).log(withdrawn(7100, 0, PRINCIPAL_WEI));
    let o = outcome(&chain);
    assert_eq!(o.outcome, ExitOutcomeKind::Returned);
    assert_eq!(o.escrowed_wei, Some(PRINCIPAL_WEI.to_string()));
}

#[test]
fn without_proof_either_way_the_outcome_is_not_read() {
    let cases: Vec<(&str, Chain)> = vec![
        ("no exit event", settled_exit().log(withdrawn(7100, 0, PRINCIPAL_WEI))),
        ("a partial unbond is not the exit", settled_exit().log(unbond(1500, SALT_WEI, PRINCIPAL_WEI - SALT_WEI)).log(withdrawn(7100, 0, PRINCIPAL_WEI))),
        ("no payout logged", settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0))),
        ("a payout before the exit is not its payout", settled_exit().log(withdrawn(1200, 0, PRINCIPAL_WEI)).log(unbond(1500, PRINCIPAL_WEI, 0))),
        ("a log outside the asked range", settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log({
            let mut l = withdrawn(7100, 0, PRINCIPAL_WEI);
            l["blockNumber"] = JsonValue::String("0x1".into()); // block 1, never in an asked range
            l["_servedAt"] = JsonValue::from(1000u64); // a node that ignores the range
            l
        })),
        ("paid less than escrowed", settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 0, PRINCIPAL_WEI - 1))),
        ("logs unavailable", settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 0, PRINCIPAL_WEI)).logs_fail_from(5000)),
        ("principal not read", settled_exit().call(BOND, PRINCIPAL, "0x".into()).log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 0, PRINCIPAL_WEI))),
        ("head not read", settled_exit().method("eth_blockNumber", JsonValue::String("0x0".into())).log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 0, PRINCIPAL_WEI))),
    ];
    for (why, chain) in cases {
        let o = outcome(&chain);
        assert_eq!(o.outcome, ExitOutcomeKind::NotRead, "{why}");
        assert!(o.reason.as_deref().is_some_and(|r| !r.is_empty()), "{why}: says why");
    }
}

#[test]
fn a_malformed_log_is_not_read_never_guessed() {
    let mut bad = withdrawn(7100, 0, PRINCIPAL_WEI);
    bad["data"] = JsonValue::String("0x12".into());
    let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(bad);
    assert_eq!(outcome(&chain).outcome, ExitOutcomeKind::NotRead);
}

#[test]
fn the_scan_reads_registry_logs_under_the_key_in_ranges_the_node_serves_whole_and_stops_at_the_payout() {
    let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 0, PRINCIPAL_WEI));
    let _ = outcome(&chain);
    let ranges = chain.log_ranges();
    assert_eq!(ranges.first(), Some(&(1000, 1999)));
    assert!(ranges.iter().all(|(f, t)| t >= f && t - f < LOG_CHUNK_BLOCKS), "{ranges:?}");
    assert!(ranges.windows(2).all(|p| p[1].0 == p[0].1 + 1), "contiguous: {ranges:?}");
    assert_eq!(ranges.last(), Some(&(7000, 7999)), "stops after the chunk with the payout");
    for body in chain.seen.borrow().iter().filter(|b| b["method"] == "eth_getLogs") {
        let f = &body["params"][0];
        assert_eq!(f["address"], REGISTRY);
        assert_eq!(f["topics"][1].as_str(), Some(key_topic().as_str()));
    }
}

#[test]
fn the_scan_is_capped_and_a_payout_beyond_it_is_not_read() {
    let far = 1000 + EXIT_SCAN_MAX_CHUNKS * LOG_CHUNK_BLOCKS + 500;
    let chain = settled_exit()
        .method("eth_blockNumber", JsonValue::String(format!("0x{:x}", far + 100)))
        .block(far + 100, 90_000)
        .block(far, 89_800)
        .log(unbond(1500, PRINCIPAL_WEI, 0))
        .log(withdrawn(far, 0, PRINCIPAL_WEI));
    let o = outcome(&chain);
    assert_eq!(o.outcome, ExitOutcomeKind::NotRead);
    assert_eq!(chain.log_ranges().len() as u64, EXIT_SCAN_MAX_CHUNKS);
}

#[test]
fn the_outcome_is_read_only_for_a_settled_exit() {
    // Still in the exit lock (escrow held), or not exiting: no outcome and no log reads.
    let in_lock = {
        let words = [addr_word(BOND), w(0), w(0), w(PRINCIPAL_WEI), w(32_000), w(1), w(1), w(7), w(2), w(0)];
        granted(true, Some(BOND_KEY)).call(REGISTRY, VALIDATOR_INFO, ret(&words))
    };
    let active = granted(true, Some(BOND_KEY)).call(REGISTRY, VALIDATOR_INFO, info(1, 1));
    let unread = granted(true, Some(BOND_KEY));
    for chain in [in_lock, active, unread] {
        assert_eq!(read(&chain, None, Some(NODE_KEY), None).exit_outcome, None);
        assert!(chain.log_ranges().is_empty());
    }
}

// Review round 3 (#516): each fail-closed guard in the scan has a test that reads "returned" without
// it. A failed or partial read, or a log the node should not have served, is never "returned".

#[test]
fn a_failed_middle_chunk_hiding_a_slash_is_not_read_never_returned() {
    // Range 3000..3999 fails; the Slashed in it is never seen; the full payout at 7100 is.
    let chain = settled_exit()
        .log(unbond(1500, PRINCIPAL_WEI, 0))
        .log(slashed(3000, 0, PRINCIPAL_WEI))
        .log(withdrawn(7100, 0, PRINCIPAL_WEI))
        .logs_fail_at(3500);
    let o = outcome(&chain);
    assert_eq!(o.outcome, ExitOutcomeKind::NotRead);
    assert_eq!(chain.log_ranges().last(), Some(&(3000, 3999)), "the scan stops at the failed range");
}

#[test]
fn a_payout_served_for_an_earlier_range_than_its_block_is_not_read_never_returned() {
    // The node returns the real payout (block 7100) for the FIRST range (1000..1999). Trusting it
    // would end the scan before the Slashed at 3000.
    let mut early = withdrawn(7100, 0, PRINCIPAL_WEI);
    early["_servedAt"] = JsonValue::from(1000u64);
    let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(slashed(3000, 0, PRINCIPAL_WEI)).log(early);
    assert_eq!(outcome(&chain).outcome, ExitOutcomeKind::NotRead);
}

#[test]
fn an_undecodable_slash_log_is_not_read_never_returned() {
    for data in ["0xzz", "not hex"] {
        let mut bad = slashed(3000, 0, PRINCIPAL_WEI);
        bad["data"] = JsonValue::String(data.into());
        let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(bad).log(withdrawn(7100, 0, PRINCIPAL_WEI));
        assert_eq!(outcome(&chain).outcome, ExitOutcomeKind::NotRead, "{data}");
    }
    // A Slashed whose penalty word is missing is not a slash of 0.
    let mut short = slashed(3000, 0, PRINCIPAL_WEI);
    short["data"] = JsonValue::String(format!("0x{}", w(0)));
    let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(short).log(withdrawn(7100, 0, PRINCIPAL_WEI));
    assert_eq!(outcome(&chain).outcome, ExitOutcomeKind::NotRead);
}

#[test]
fn a_slash_with_no_log_index_in_the_payout_block_counts_before_the_payout() {
    let mut s = slashed(7100, 0, SALT_WEI);
    s.as_object_mut().map(|o| o.remove("logIndex"));
    let chain = settled_exit().log(unbond(1500, PRINCIPAL_WEI, 0)).log(withdrawn(7100, 5, PRINCIPAL_WEI)).log(s);
    assert_eq!(outcome(&chain).outcome, ExitOutcomeKind::Slashed);
}

/// Wraps a transport and sleeps before each call (a slow but healthy endpoint).
struct Slow<'a> {
    inner: &'a Chain,
    per_call: std::time::Duration,
}
impl RpcTransport for Slow<'_> {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        std::thread::sleep(self.per_call);
        self.inner.call(body)
    }
}

#[test]
fn a_spent_read_budget_reads_unknown_never_zero_and_sends_nothing() {
    let public = granted(false, None);
    let local = Chain::default().method("eth_blockNumber", JsonValue::String("0x1f4".into()));
    let past = std::time::Instant::now();
    let p = Budgeted::new(&public, past);
    let l = Budgeted::new(&local, past);
    let st = read_activation_status(&p, Some(&l), &targets(), MEMBER, None, Some("0xfeed"), &|_| None).expect("valid member");
    assert!(public.seen.borrow().is_empty(), "no call after the budget is spent");
    assert!(local.seen.borrow().is_empty(), "no local call after the budget is spent");
    assert_eq!(st.bond_address, None);
    assert_eq!(st.bond_deployed, None);
    assert_eq!(st.head_block, None);
    assert_eq!(st.local_block, None);
    let tx = st.tx.expect("tx view");
    assert_eq!(tx.included, None, "an unread receipt is unknown, not pending");
}

#[test]
fn a_slow_endpoint_is_bounded_by_the_read_budget() {
    let public = granted(false, None);
    let slow = Slow { inner: &public, per_call: std::time::Duration::from_millis(30) };
    let budget = std::time::Duration::from_millis(100);
    let started = std::time::Instant::now();
    let p = Budgeted::new(&slow, started + budget);
    let st = read_activation_status::<_, Chain>(&p, None, &targets(), MEMBER, None, None, &|_| None).expect("valid member");
    let elapsed = started.elapsed();
    // At most one call starts at the deadline and runs its full 30 ms; generous slack for CI.
    assert!(elapsed < budget + std::time::Duration::from_millis(30) + std::time::Duration::from_millis(150), "took {elapsed:?}");
    assert!(public.seen.borrow().len() < 8, "calls past the budget were refused ({} sent)", public.seen.borrow().len());
    assert_eq!(st.head_block, None, "a read refused by the budget is unknown");
}

#[test]
fn tripwire_the_command_budgets_both_transports() {
    let src = include_str!("validator_activation.rs");
    let start = src.find("pub fn validator_activation_status_sync(").expect("command body");
    let body = &src[start..];
    let body = &body[..body.find("\n}\n").map(|e| e + 3).unwrap_or(body.len())];
    assert!(body.contains("READ_BUDGET"), "the read has a deadline");
    assert!(body.contains("Budgeted::new(&public"), "the public transport is budgeted");
    assert!(body.contains("Budgeted::new(&local"), "the local transport is budgeted");
    assert!(!body.contains("read_activation_status(\n        &public,"), "the raw public transport is not passed");
}

// chain#331 / owner decision 2026-10-19: activation only inside the bond lock. `MemberBond.activate`
// reverts `ActivationWindowClosed` once `isUnlocked()` is true, so the step's gate needs the flag AS
// READ: a failed or non-bool answer is None (unread), never "locked".
#[test]
fn the_bonds_lock_is_read_as_is_and_an_unread_lock_is_none() {
    assert_eq!(IS_UNLOCKED, keccak4("isUnlocked()"));
    for (word, want) in [(Some(0u128), Some(false)), (Some(1u128), Some(true)), (Some(7u128), None), (None, None)] {
        let mut chain = granted(false, None);
        if let Some(v) = word {
            chain = chain.call(BOND, IS_UNLOCKED, ret(&[w(v)]));
        }
        let st = read(&chain, None, Some(NODE_KEY), None);
        assert_eq!(st.bond_unlocked, want, "isUnlocked() word {word:?}");
        assert_eq!(st.bond_deployed, Some(true), "the other bond reads still stand");
    }
}

#[test]
fn no_bond_means_no_lock_read() {
    let chain = Chain::default().call(VAULT, BOND_OF, ret(&[addr_word(BOND)])).call(BOND, ACTIVATED, "0x".to_string());
    let st = read(&chain, None, Some(NODE_KEY), None);
    assert_eq!(st.bond_deployed, Some(false));
    assert_eq!(st.bond_unlocked, None);
}
