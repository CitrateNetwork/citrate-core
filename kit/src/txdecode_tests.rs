// CORE-B1.4 — legacy tx intent decode tests. These are pure/CI-safe (no RPC, no
// key): they prove the JSON tx object → LegacyTxFields mapping and the human
// display, and that genuinely-undecodable payloads return None (so the ceremony
// keeps raw-ack gating them — Rule 1 / B1.2-ADV-5).

use super::*;

/// A well-formed EIP-1193 value-transfer tx object (as the connector marshals).
fn transfer_json(to: &str, value_hex: &str) -> String {
    serde_json::json!({
        "from": "0x98a32D944e9138B14A35b5D4dcE53339570F371A",
        "to": to,
        "value": value_hex,
        "data": "0x",
    })
    .to_string()
}

#[test]
fn decodes_value_transfer_to_fields_and_display() {
    let raw = transfer_json("0x3535353535353535353535353535353535353535", "0xde0b6b3a7640000"); // 1e18
    let (parsed, display) = decode_transaction(&raw).expect("value transfer decodes");

    assert_eq!(
        parsed.to,
        Some([0x35u8; 20]),
        "recipient decoded to 20 bytes"
    );
    assert_eq!(parsed.value, 1_000_000_000_000_000_000u128, "value 1e18 wei");
    assert!(parsed.data.is_empty(), "empty calldata for a plain transfer");
    assert_eq!(parsed.nonce, None, "nonce omitted → RPC-resolved");
    assert_eq!(parsed.gas_price, None, "gasPrice omitted → RPC-resolved");
    assert_eq!(parsed.gas_limit, None, "gas omitted → defaulted at finalize");
    assert_eq!(
        parsed.from.as_deref(),
        Some("0x98a32D944e9138B14A35b5D4dcE53339570F371A")
    );

    assert!(display.action.contains("Send"), "human action: {}", display.action);
    assert!(display.action.contains("1000000000000000000"), "value shown in wei");
    assert_eq!(display.destination, "0x3535353535353535353535353535353535353535");
    assert_eq!(display.cost, "1000000000000000000 wei");
}

#[test]
fn finalize_merges_rpc_nonce_gas_and_defaults_transfer_gas() {
    let raw = transfer_json("0x3535353535353535353535353535353535353535", "0x1");
    let (parsed, _) = decode_transaction(&raw).expect("decodes");
    let fields = parsed.finalize(7, 20_000_000_000).expect("finalize a transfer");
    assert_eq!(fields.nonce, 7, "RPC-fetched nonce used");
    assert_eq!(fields.gas_price, 20_000_000_000, "RPC-fetched gas price used");
    assert_eq!(fields.gas_limit, DEFAULT_TRANSFER_GAS, "transfer gas defaults to 21000");
    assert_eq!(fields.value, 1u128);
    assert_eq!(fields.to, Some([0x35u8; 20]));
}

#[test]
fn dapp_supplied_nonce_and_gas_win_over_fetched() {
    let raw = serde_json::json!({
        "from": "0x98a32D944e9138B14A35b5D4dcE53339570F371A",
        "to": "0x3535353535353535353535353535353535353535",
        "value": "0x0",
        "nonce": "0x5",
        "gasPrice": "0x77359400", // 2e9
        "gas": "0x5208",          // 21000
    })
    .to_string();
    let (parsed, _) = decode_transaction(&raw).expect("decodes");
    assert_eq!(parsed.nonce, Some(5));
    assert_eq!(parsed.gas_price, Some(2_000_000_000));
    assert_eq!(parsed.gas_limit, Some(21_000));
    // finalize must PREFER the dApp values, ignoring the fetched fallbacks.
    let fields = parsed.finalize(999, 999).expect("finalize");
    assert_eq!(fields.nonce, 5);
    assert_eq!(fields.gas_price, 2_000_000_000);
    assert_eq!(fields.gas_limit, 21_000);
}

#[test]
fn contract_creation_with_initcode_decodes() {
    let raw = serde_json::json!({
        "from": "0x98a32D944e9138B14A35b5D4dcE53339570F371A",
        "to": serde_json::Value::Null,
        "data": "0x6080604052",
        "gas": "0xf4240", // 1_000_000 — creation gas supplied
    })
    .to_string();
    let (parsed, display) = decode_transaction(&raw).expect("contract creation decodes");
    assert_eq!(parsed.to, None, "contract creation has no recipient");
    assert_eq!(parsed.data, vec![0x60, 0x80, 0x60, 0x40, 0x52]);
    assert!(display.action.contains("Deploy contract"), "action: {}", display.action);
    assert_eq!(display.destination, "contract creation");
    let fields = parsed.finalize(0, 1).expect("finalize creation with supplied gas");
    assert_eq!(fields.gas_limit, 1_000_000);
    assert_eq!(fields.to, None);
}

#[test]
fn calldata_with_no_gas_cannot_finalize() {
    // A contract CALL (calldata present) without a supplied gas limit: we must
    // NOT guess execution gas. It parses for the broadcast path but finalize
    // returns None.
    let raw = serde_json::json!({
        "from": "0x98a32D944e9138B14A35b5D4dcE53339570F371A",
        "to": "0x3535353535353535353535353535353535353535",
        "data": "0xa9059cbb", // transfer(...) selector
    })
    .to_string();
    let (parsed, display) = decode_transaction(&raw).expect("call decodes for display");
    // `transfer(...)` is a KNOWN selector (CORE-B-003 allowlist), so it stays
    // legible — the action names the call. finalize still returns None (no gas).
    assert!(display.action.contains("Call"), "action: {}", display.action);
    assert!(parsed.finalize(0, 1).is_none(), "no gas for a call → cannot finalize");
}

// ---- undecodable payloads STILL return None (raw-gated by the ceremony) ----

#[test]
fn non_json_payload_is_undecodable() {
    assert!(decode_transaction("0x02f8016b82").is_none(), "opaque bytes are not a tx object");
    assert!(decode_transaction("not json at all").is_none());
    assert!(decode_transaction("").is_none());
}

#[test]
fn json_that_is_not_a_tx_object_is_undecodable() {
    assert!(decode_transaction("[]").is_none(), "an array is not a tx object");
    assert!(decode_transaction("\"a string\"").is_none());
    assert!(decode_transaction("42").is_none());
}

#[test]
fn empty_contract_creation_no_initcode_is_undecodable() {
    // No `to` AND no calldata: not a legible action → raw-gated.
    let raw = serde_json::json!({ "from": "0x98a3", "value": "0x0" }).to_string();
    assert!(decode_transaction(&raw).is_none());
}

#[test]
fn malformed_field_is_undecodable() {
    // A present-but-garbage value quantity → undecodable (never truncated/guessed).
    let raw = serde_json::json!({
        "to": "0x3535353535353535353535353535353535353535",
        "value": "0xZZZ",
        "data": "0x",
    })
    .to_string();
    assert!(decode_transaction(&raw).is_none());
    // A bad `to` (not 20 bytes) → undecodable.
    let raw2 = serde_json::json!({ "to": "0x1234", "value": "0x0", "data": "0x" }).to_string();
    assert!(decode_transaction(&raw2).is_none());
}

#[test]
fn value_overflowing_u128_is_rejected_not_truncated() {
    // 0xffff…(33 bytes) > u128::MAX → rejected as undecodable (Rule 1: no silent truncation).
    let big = format!("0x{}", "f".repeat(66));
    let raw = serde_json::json!({
        "to": "0x3535353535353535353535353535353535353535",
        "value": big,
        "data": "0x",
    })
    .to_string();
    assert!(decode_transaction(&raw).is_none());
}

// HUP-S1.5: the registry escalation call is legible and says the input goes on chain publicly.
fn request_inference_calldata(input: &[u8], max_price: u128) -> Vec<u8> {
    let mut d = selector_of("requestInference(bytes32,bytes,uint256)").to_vec();
    d.extend_from_slice(&[0x5a; 32]);
    let mut w = [0u8; 32];
    w[31] = 0x60;
    d.extend_from_slice(&w);
    let mut m = [0u8; 32];
    m[16..].copy_from_slice(&max_price.to_be_bytes());
    d.extend_from_slice(&m);
    let mut l = [0u8; 32];
    l[24..].copy_from_slice(&(input.len() as u64).to_be_bytes());
    d.extend_from_slice(&l);
    d.extend_from_slice(input);
    d.resize(d.len() + (32 - input.len() % 32) % 32, 0);
    d
}

fn router_tx(data: &[u8], value: u128) -> String {
    serde_json::json!({
        "to": "0x1111111111111111111111111111111111111111",
        "value": format!("0x{value:x}"),
        "data": format!("0x{}", hex::encode(data)),
        "gas": "0x30d40",
    })
    .to_string()
}

#[test]
fn request_inference_is_legible_and_warns_the_input_is_public() {
    let data = request_inference_calldata(b"plan the migration", 5_000);
    let (_, d) = decode_transaction(&router_tx(&data, 5_000)).unwrap_or_else(|| panic!("decodes"));
    assert!(d.action.contains("Request inference"), "{}", d.action);
    assert!(d.action.contains(&format!("0x{}", "5a".repeat(32))), "{}", d.action);
    assert!(d.action.contains("18 bytes of input"), "{}", d.action);
    assert!(d.action.contains("publicly on chain"), "{}", d.action);
    assert!(d.action.contains("price ceiling 5000 wei"), "{}", d.action);
    assert_eq!(d.cost, "5000 wei");
    assert_ne!(d.action, crate::ceremony::UNRECOGNIZED_ACTION);
}

#[test]
fn malformed_request_inference_falls_back_to_the_generic_label() {
    let mut data = request_inference_calldata(b"abc", 1);
    // Non-zero padding is not the canonical encoding.
    let last = data.len() - 1;
    data[last] = 1;
    let (_, d) = decode_transaction(&router_tx(&data, 1)).unwrap_or_else(|| panic!("decodes"));
    assert!(d.action.starts_with("Call requestInference()"), "{}", d.action);
    // A wrong offset word too.
    let mut data = request_inference_calldata(b"abc", 1);
    data[4 + 63] = 0x40;
    let (_, d) = decode_transaction(&router_tx(&data, 1)).unwrap_or_else(|| panic!("decodes"));
    assert!(d.action.starts_with("Call requestInference()"), "{}", d.action);
}

#[test]
fn claim_refund_is_a_known_call() {
    let data = selector_of("claimRefund()").to_vec();
    let (_, d) = decode_transaction(&router_tx(&data, 0)).unwrap_or_else(|| panic!("decodes"));
    assert!(d.action.starts_with("Call claimRefund()"), "{}", d.action);
}

#[test]
fn expire_request_is_a_known_call() {
    let mut data = selector_of("expireRequest(uint256)").to_vec();
    data.extend_from_slice(&[0u8; 32]);
    let (_, d) = decode_transaction(&router_tx(&data, 0)).unwrap_or_else(|| panic!("decodes"));
    assert!(d.action.starts_with("Call expireRequest()"), "{}", d.action);
}

// Reroll 2026-10-05: the member's own AgentSBT mint (built by core's agent_sbt module and approved
// in the ceremony, HIC-1) is a known call. The owner-only `mintAgent` is not one: core never builds it.
#[test]
fn member_agent_sbt_mint_is_a_known_call_and_the_owner_mint_is_not() {
    let mut data = selector_of("mintAgentAsMember(bytes32,bytes32)").to_vec();
    data.extend_from_slice(&[0x8a; 32]);
    data.extend_from_slice(&[0x21; 32]);
    let (_, d) = decode_transaction(&router_tx(&data, 0)).unwrap_or_else(|| panic!("decodes"));
    assert!(d.action.starts_with("Call mintAgentAsMember()"), "{}", d.action);
    let mut owner = selector_of("mintAgent(address,uint256,bytes32,bytes32)").to_vec();
    owner.extend_from_slice(&[0u8; 128]);
    let (_, d) = decode_transaction(&router_tx(&owner, 0)).unwrap_or_else(|| panic!("decodes"));
    assert!(!d.action.contains("mintAgent"), "{}", d.action);
}
