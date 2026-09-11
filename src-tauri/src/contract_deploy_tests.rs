// Hermes P3 / WP3.2 — contract-deploy (creation-tx) tests. Pure — no signing, no chain.
use super::*;

#[test]
fn initcode_is_bytecode_then_constructor_args() {
    let code = [0x60, 0x80, 0x60, 0x40]; // a bytecode prefix
    let args = [0xaa; 32]; // one ABI word
    let ic = deploy_initcode(&code, &args);
    assert_eq!(&ic[..4], &code);
    assert_eq!(&ic[4..], &args);
    // No args → init code is exactly the bytecode.
    assert_eq!(deploy_initcode(&code, &[]), code);
}

#[test]
fn parse_hex_accepts_prefixed_bare_and_empty_and_rejects_junk() {
    assert_eq!(parse_hex("0xdeadbeef", "b").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(parse_hex("deadbeef", "b").unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
    assert!(parse_hex("", "b").unwrap().is_empty());
    assert!(parse_hex("0xnothex", "b").is_err());
}

#[test]
fn deploy_tx_json_is_a_to_less_creation_the_decoder_renders_honestly() {
    // A real (tiny) init code. The produced tx JSON must decode as a contract creation
    // (to == None) so the ceremony shows the honest "contract creation" action — and,
    // because the action is recognized, it is approvable (not stuck behind a raw-ack).
    let initcode = hex::decode("6080604052").unwrap();
    let raw = encode_deploy_tx_json("0x1111111111111111111111111111111111111111", &initcode, 0, 2_000_000);

    // No recipient in the JSON.
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(v.get("to").is_none(), "a creation tx carries no `to`");
    assert_eq!(v["data"], "0x6080604052");
    assert_eq!(v["chainId"], "0x9d0c"); // 40204

    // The shared decoder agrees: to == None, action == "contract creation".
    let (parsed, display) = crate::txdecode::decode_transaction(&raw).expect("decodes");
    assert!(parsed.to.is_none(), "decoded as contract creation");
    assert!(
        display.action.to_lowercase().contains("contract"),
        "human sees a contract deploy/creation, got {:?}",
        display.action
    );
}

#[test]
fn value_defaults_to_zero_and_encodes_as_hex() {
    let raw = encode_deploy_tx_json("0xabc", &[0x00], 0, 21000);
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["value"], "0x0");
    let raw2 = encode_deploy_tx_json("0xabc", &[0x00], 1_000_000_000_000_000_000, 21000);
    let v2: serde_json::Value = serde_json::from_str(&raw2).unwrap();
    assert_eq!(v2["value"], "0xde0b6b3a7640000"); // 1 SALT in wei
}
