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
    read_activation_status(public, local, &targets(), MEMBER, node_key, tx).expect("valid member")
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
}

#[test]
fn a_bad_member_address_fails_closed_before_any_read() {
    let chain = Chain::default();
    let err = read_activation_status(&chain, None::<&Chain>, &targets(), "0x12", None, None);
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
}
