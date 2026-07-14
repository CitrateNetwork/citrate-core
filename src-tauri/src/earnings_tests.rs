// CORE-C2 — earnings: real claimable read + claim intent (@rule8 evidence).
//
// Red-first. Covers:
//   * WP1 — the ContributionAccounting ABI facts: selectors PINNED against the
//     node-agent's values (claimable 0x402914f5, claimRewards 0x372500ab); the
//     claimable(address) calldata shape; the uint256 decode (incl. the
//     no-truncation overflow refusal); the eth_call request shape; and the real
//     claimable read over a MOCKED RPC (Rule 1 — the mock is a TEST transport).
//   * WP2 — the user Claim button's unsigned claimRewards() request shape (the
//     same the node-agent serves), which the C1.2 bridge routes to the ceremony.
//
// CI-safe + deterministic: a mock RpcTransport (no live socket). The live real
// claimable read for the vault/node address is a documented #[ignore] proof at
// the bottom (honest — likely 0 for a fresh node); a CONFIRMED claim tx needs a
// node with ACCRUED earnings and is a documented gap (NOT fabricated — Rule 1).

use super::*;
use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// A scripted mock RPC transport (mirrors rpc_tests / agent_tests).
// ---------------------------------------------------------------------------

struct MockRpc {
    requests: RefCell<Vec<JsonValue>>,
    responses: RefCell<VecDeque<JsonValue>>,
}
impl MockRpc {
    fn new(responses: Vec<JsonValue>) -> Self {
        MockRpc {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }
    fn requests(&self) -> Vec<JsonValue> {
        self.requests.borrow().clone()
    }
}
impl RpcTransport for MockRpc {
    fn call(&self, body: JsonValue) -> std::result::Result<JsonValue, RpcError> {
        self.requests.borrow_mut().push(body.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}
fn ok(result: JsonValue) -> JsonValue {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

/// A 32-byte `uint256` ABI word for `n`, as a `0x`-hex string (the shape an
/// `eth_call` claimable read returns).
fn uint256_hex(n: u128) -> String {
    let mut word = [0u8; 32];
    word[16..32].copy_from_slice(&n.to_be_bytes());
    format!("0x{}", hex::encode(word))
}

const VAULT_ADDR: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";

// ===========================================================================
// WP1 — the ContributionAccounting ABI facts (selectors pinned, decode)
// ===========================================================================

/// Derive a 4-byte selector from a canonical signature via Keccak-256 (the
/// dev-only `sha3` crate, same one the wallet tests use). This is the INDEPENDENT
/// derivation that proves the pinned production constants are correct — if a
/// constant is wrong, or a signature string drifts, this fails (Rule 11 tripwire).
fn derive_selector(sig: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// The pinned `claimable(address)` selector is the REAL keccak of the signature
/// AND equals the node-agent's independently-derived value (0x402914f5). A drift
/// here means the calldata the Earning tab reads would diverge — fail loudly.
#[test]
fn claimable_selector_matches_node_agent() {
    // Independently derived from the canonical signature.
    assert_eq!(claimable_selector(), derive_selector("claimable(address)"));
    // And equals the node-agent's pinned value.
    assert_eq!(
        format!("0x{}", hex::encode(claimable_selector())),
        "0x402914f5"
    );
}

/// The pinned `claimRewards()` selector is the REAL keccak of the signature AND
/// equals the node-agent's pinned value (0x372500ab), so the bridged ceremony
/// signs the SAME 4 bytes the daemon puts on the wire.
#[test]
fn claim_rewards_selector_matches_node_agent() {
    assert_eq!(
        claim_rewards_selector(),
        derive_selector("claimRewards()")
    );
    assert_eq!(
        format!("0x{}", hex::encode(claim_rewards_selector())),
        "0x372500ab"
    );
}

/// The `claimable(address)` calldata is `selector ++ left-padded 20-byte address`
/// (36 bytes). The address lands in the low 20 bytes of the 32-byte word.
#[test]
fn claimable_calldata_shape() {
    let calldata = encode_claimable_calldata(VAULT_ADDR);
    assert_eq!(calldata.len(), 36, "selector (4) + one 32-byte word");
    assert_eq!(&calldata[0..4], &claimable_selector());
    // 12 leading zero bytes of the word, then the 20 address bytes.
    assert!(calldata[4..16].iter().all(|&b| b == 0), "address left-padded");
    let addr_bytes = hex::decode(VAULT_ADDR.trim_start_matches("0x")).unwrap();
    assert_eq!(&calldata[16..36], &addr_bytes[..]);
}

/// A well-formed uint256 return decodes to the wei value.
#[test]
fn decodes_claimable_uint256() {
    let seven_salt = 7 * 10u128.pow(18);
    let mut word = [0u8; 32];
    word[16..32].copy_from_slice(&seven_salt.to_be_bytes());
    assert_eq!(decode_claimable(&word).unwrap(), seven_salt);
}

/// Zero claimable (a fresh node) decodes to 0 — honest, not an error.
#[test]
fn decodes_zero_claimable() {
    let word = [0u8; 32];
    assert_eq!(decode_claimable(&word).unwrap(), 0);
}

/// A short return (not a full 32-byte word) is a DECODE error — never a
/// fabricated or partial value.
#[test]
fn short_return_is_decode_error() {
    let short = [0u8; 16];
    assert!(matches!(
        decode_claimable(&short),
        Err(EarningsError::Decode(_))
    ));
}

/// A value beyond u128 (high 16 bytes non-zero) is REFUSED, not truncated
/// (Rule 1 — no silent truncation of a claimable).
#[test]
fn overflowing_claimable_is_refused_not_truncated() {
    let mut word = [0u8; 32];
    word[0] = 1; // sets a bit well above u128::MAX
    assert!(matches!(
        decode_claimable(&word),
        Err(EarningsError::Decode(_))
    ));
}

// ===========================================================================
// WP1 — the real claimable read over a MOCKED RPC (Rule 1: TEST transport)
// ===========================================================================

/// `read_claimable` sends a well-formed `eth_call` to the ContributionAccounting
/// contract with the claimable(address) calldata, and decodes the returned
/// uint256 to the real wei value.
#[test]
fn read_claimable_calls_eth_call_and_decodes() {
    let claimable = 9410000000000000000u128; // 9.41 SALT in wei
    let rpc = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(uint256_hex(
        claimable,
    )))]));
    let snap = read_claimable(&rpc, VAULT_ADDR).expect("read claimable");
    assert_eq!(snap.claimable_wei, claimable.to_string());
    assert_eq!(snap.wallet_address, VAULT_ADDR);
    assert_eq!(snap.contract, CONTRIBUTION_ACCOUNTING);

    // The request was an eth_call to the contract with the right calldata + tag.
    let reqs = rpc.transport.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0]["method"], "eth_call");
    let call = &reqs[0]["params"][0];
    assert_eq!(call["to"], CONTRIBUTION_ACCOUNTING);
    let data = call["data"].as_str().unwrap();
    assert!(data.starts_with("0x402914f5"), "claimable selector: {data}");
    assert_eq!(reqs[0]["params"][1], "latest", "block tag pinned to latest");
}

/// A fresh node with no accrued rewards reads a REAL 0 — the honest common case
/// (Rule 1: 0 is a real read, not a placeholder).
#[test]
fn read_claimable_zero_for_fresh_node() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(uint256_hex(0)))]));
    let snap = read_claimable(&rpc, VAULT_ADDR).expect("read claimable");
    assert_eq!(snap.claimable_wei, "0");
}

/// A node RPC error surfaces as `EarningsError::Rpc` (never a fabricated value on
/// a failed read).
#[test]
fn read_claimable_surfaces_rpc_error() {
    let err = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "error": { "code": -32000, "message": "execution reverted" }
    });
    let rpc = RpcClient::with_transport(MockRpc::new(vec![err]));
    let r = read_claimable(&rpc, VAULT_ADDR);
    assert!(matches!(r, Err(EarningsError::Rpc(_))), "got: {r:?}");
}

/// A malformed wallet address fails closed (never an eth_call with a wrong arg).
#[test]
fn read_claimable_rejects_bad_address() {
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    let r = read_claimable(&rpc, "not-an-address");
    assert!(matches!(r, Err(EarningsError::BadAddress(_))), "got: {r:?}");
    // No RPC call was attempted (fail closed before the read).
    assert_eq!(rpc.transport.requests().len(), 0);
}

// ===========================================================================
// WP2 — the user Claim button's unsigned claimRewards() request shape
// ===========================================================================

/// The user's Claim button emits the SAME unsigned request shape the node-agent
/// serves: selector-only `claimRewards()` calldata, the ContributionAccounting
/// target, zero value, chain 40204, and the real claimable folded into context.
/// It carries NO signature — the bridge routes it to the ceremony.
#[test]
fn user_claim_request_is_the_node_agent_shape() {
    let claimable = 9410000000000000000u128;
    let req = user_claim_request(claimable);
    assert_eq!(req.intent, "claimRewards");
    assert_eq!(req.to, CONTRIBUTION_ACCOUNTING);
    assert_eq!(req.value_wei, "0", "the claim tx sends no SALT (the contract pays)");
    assert_eq!(req.chain_id, 40204);
    assert!(req.is_pending());
    assert_eq!(req.tx_hash, None);
    // Selector-only calldata (claimRewards() takes no args).
    assert_eq!(req.calldata, "0x372500ab");
    // The real claimable is shown in the human-readable context (not fabricated).
    assert!(
        req.context.contains(&claimable.to_string()),
        "context surfaces the real claimable: {}",
        req.context
    );
}

/// C2-F-3: the user claim request carries the DISJOINT `USER_CLAIM_ID` (top bit
/// set), NOT the old hardcoded `id: 0`. This is what keeps a user claim from
/// aliasing a node-agent request (whose ids are small + sequential) in the agent
/// bridge's shared dedup map.
#[test]
fn user_claim_request_uses_disjoint_id_space() {
    let req = user_claim_request(1);
    assert_eq!(req.id, USER_CLAIM_ID, "user claim uses the disjoint id space");
    assert_eq!(USER_CLAIM_ID, 1u64 << 63, "top bit set — unreachable by the node-agent counter");
    assert_ne!(req.id, 0, "must NOT be the old colliding id: 0 (C2-F-3)");
}

/// The user claim request routes cleanly through the SAME bridge intent builder
/// the node-agent path uses (origin "agent:node-agent", a legible Call, not
/// raw-gated) — proving the user Claim and the sweep share ONE ceremony path.
#[test]
fn user_claim_request_bridges_to_a_legible_ceremony_intent() {
    let req = user_claim_request(5 * 10u128.pow(18));
    let intent = crate::agent::intent_from_request(&req, VAULT_ADDR, Some(0x8000));
    assert_eq!(intent.origin, crate::agent::AGENT_ORIGIN);
    // txdecode parses it to a legible contract call (not raw-ack gated).
    let (_parsed, display) = crate::txdecode::decode_transaction(&intent.raw)
        .expect("claim intent decodes to a legible tx");
    assert!(
        display.action.contains("Call"),
        "claim is a legible Call: {}",
        display.action
    );
    assert!(
        display.destination.eq_ignore_ascii_case(CONTRIBUTION_ACCOUNTING),
        "destination is the accounting contract: {}",
        display.destination
    );
}

// ===========================================================================
// LIVE (documented, #[ignore]) — the REAL claimable read on 40204
// ===========================================================================
//
// The honest live-path proof: read the REAL claimable for the vault/node address
// on the live 40204 RPC. Likely 0 for a fresh node (ContributionAccounting only
// credits contributors). Run explicitly:
//
//   CITRATE_EARNINGS_ADDR=0x<vault-or-node-operator-address> \
//     cargo test --locked earnings::tests::live_real_claimable_read -- --ignored --nocapture
//
// A CONFIRMED claim tx (claimRewards broadcast) needs a node with ACCRUED
// earnings AND an unlocked funded vault. That is a documented GAP — NOT fabricated
// here (Rule 1). The exact command once earnings exist + the vault is unlocked:
//
//   # in-app: Node → Earning → Claim (routes through the ceremony → B1.4 broadcast)
//   # or headless via the bridge: mgr.bridge_one_pending(..) → approve_bridged_and_report(..)
#[test]
#[ignore = "live: reads the REAL claimable on 40204 for a supplied address"]
fn live_real_claimable_read() {
    let addr = std::env::var("CITRATE_EARNINGS_ADDR")
        .expect("set CITRATE_EARNINGS_ADDR to the vault / node-operator address");
    let rpc = RpcClient::citrate();
    match read_claimable(&rpc, &addr) {
        Ok(snap) => eprintln!(
            "[live] claimable({addr}) = {} wei (contract {})",
            snap.claimable_wei, snap.contract
        ),
        Err(e) => eprintln!("[live] read_claimable error: {e}"),
    }
}
