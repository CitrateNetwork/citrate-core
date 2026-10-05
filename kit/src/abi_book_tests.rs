// HUP-S6 — gated-contract calls decoded from the contract's own ABI, and everything else raw.
use super::*;
use crate::ceremony::{IntentKind, SignatureCeremony, SignatureIntent, UNRECOGNIZED_ACTION};

/// The state-changing part of the hello-mint (erc721 template) ABI as forge writes it, plus a
/// view and a dynamic-argument function that must be left out.
fn hello_mint_abi() -> serde_json::Value {
    serde_json::json!([
        {"type": "constructor", "inputs": [], "stateMutability": "nonpayable"},
        {"type": "function", "name": "mint", "stateMutability": "payable",
         "inputs": [{"name": "quantity", "type": "uint256", "internalType": "uint256"}], "outputs": []},
        {"type": "function", "name": "withdraw", "stateMutability": "nonpayable", "inputs": [], "outputs": []},
        {"type": "function", "name": "setBaseURI", "stateMutability": "nonpayable",
         "inputs": [{"name": "baseURI", "type": "string"}], "outputs": []},
        {"type": "function", "name": "transferFrom", "stateMutability": "nonpayable",
         "inputs": [{"name": "from", "type": "address"}, {"name": "to", "type": "address"},
                    {"name": "tokenId", "type": "uint256"}], "outputs": []},
        {"type": "function", "name": "setApprovalForAll", "stateMutability": "nonpayable",
         "inputs": [{"name": "operator", "type": "address"}, {"name": "approved", "type": "bool"}], "outputs": []},
        {"type": "function", "name": "balanceOf", "stateMutability": "view",
         "inputs": [{"name": "owner", "type": "address"}], "outputs": [{"type": "uint256"}]},
        {"type": "event", "name": "Withdrawn", "inputs": []}
    ])
}

const ADDR: [u8; 20] = [0x5f; 20];
const ADDR_HEX: &str = "0x5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f";

fn mint_calldata(q: u8) -> Vec<u8> {
    let mut d = vec![0xa0, 0x71, 0x2d, 0x68];
    let mut w = [0u8; 32];
    w[31] = q;
    d.extend_from_slice(&w);
    d
}

fn mint_intent(data: &[u8], value_wei: u128) -> SignatureIntent {
    SignatureIntent {
        origin: "hello-mint page".into(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": "0x0000000000000000000000000000000000000001",
            "to": ADDR_HEX,
            "value": format!("0x{value_wei:x}"),
            "data": format!("0x{}", hex::encode(data)),
            "gas": "0x7a120",
        })
        .to_string(),
    }
}

fn book() -> ContractAbi {
    ContractAbi::from_abi_json("LemonDrops", &hello_mint_abi()).expect("abi")
}

#[test]
fn only_state_changing_static_functions_are_kept() {
    let mut names: Vec<String> = book().functions.iter().map(|f| f.name.clone()).collect();
    names.sort();
    assert_eq!(
        names,
        ["mint", "setApprovalForAll", "transferFrom", "withdraw"]
    );
    let mint = book()
        .functions
        .into_iter()
        .find(|f| f.name == "mint")
        .expect("mint");
    assert_eq!(
        mint.selector,
        [0xa0, 0x71, 0x2d, 0x68],
        "keccak(\"mint(uint256)\")[..4]"
    );
    assert!(mint.payable);
}

#[test]
fn mint_decodes_with_its_argument_name() {
    assert_eq!(
        book()
            .decode_call(&mint_calldata(1), 5_000_000_000_000_000_000)
            .as_deref(),
        Some("mint(quantity=1)")
    );
}

#[test]
fn malformed_or_unknown_calls_are_not_decoded() {
    let b = book();
    let mut long = mint_calldata(1);
    long.push(0);
    assert_eq!(b.decode_call(&long, 0), None, "trailing byte");
    assert_eq!(b.decode_call(&mint_calldata(1)[..20], 0), None, "short");
    assert_eq!(
        b.decode_call(&[0xde, 0xad, 0xbe, 0xef], 0),
        None,
        "unknown selector"
    );
    // A dirty address word (high bytes set) is not a canonical address.
    let mut sel = selector("setApprovalForAll(address,bool)").to_vec();
    let mut addr = [0u8; 32];
    addr[0] = 1;
    sel.extend_from_slice(&addr);
    sel.extend_from_slice(&[0u8; 32]);
    assert_eq!(b.decode_call(&sel, 0), None);
    // A bool word of 2 is not a bool.
    let mut sel = selector("setApprovalForAll(address,bool)").to_vec();
    sel.extend_from_slice(&[0u8; 32]);
    let mut two = [0u8; 32];
    two[31] = 2;
    sel.extend_from_slice(&two);
    assert_eq!(b.decode_call(&sel, 0), None);
    // Value sent to a nonpayable function.
    assert_eq!(b.decode_call(&selector("withdraw()"), 1), None);
    assert_eq!(
        b.decode_call(&selector("withdraw()"), 0).as_deref(),
        Some("withdraw()")
    );
}

#[test]
fn types_decode_canonically() {
    let mut w = [0u8; 32];
    w[31] = 0xff;
    assert_eq!(AbiType::Uint(8).decode(&w).as_deref(), Some("255"));
    assert_eq!(AbiType::Int(8).decode(&[0xff; 32]).as_deref(), Some("-1"));
    let mut big = [0u8; 32];
    big[0] = 1;
    assert_eq!(AbiType::Uint(8).decode(&big), None);
    assert_eq!(
        AbiType::Uint(256).decode(&[0xff; 32]).as_deref(),
        Some("115792089237316195423570985008687907853269984665640564039457584007913129639935")
    );
    assert_eq!(AbiType::parse("uint"), Some(AbiType::Uint(256)));
    assert_eq!(AbiType::parse("uint7"), None);
    assert_eq!(AbiType::parse("bytes"), None);
    assert_eq!(AbiType::parse("bytes33"), None);
    assert_eq!(AbiType::parse("uint256[]"), None);
    assert_eq!(AbiType::parse("tuple"), None);
}

#[test]
fn a_registered_gated_contract_mint_needs_no_raw_ack() {
    let c = SignatureCeremony::new();
    c.register_gated_contract(ADDR, book());
    let view = c.request(mint_intent(&mint_calldata(1), 5_000_000_000_000_000_000));
    assert!(!view.requires_raw_ack, "{view:?}");
    assert!(
        view.decoded
            .action
            .starts_with("Call mint(quantity=1) on LemonDrops at "),
        "{}",
        view.decoded.action
    );
    assert_eq!(view.decoded.cost, "5000000000000000000 wei");
}

/// The mutant the task asks for: the same mint without the ABI falls back to raw data.
#[test]
fn without_the_abi_the_same_mint_falls_back_to_raw_data() {
    let c = SignatureCeremony::new();
    let view = c.request(mint_intent(&mint_calldata(1), 5_000_000_000_000_000_000));
    assert!(view.requires_raw_ack);
    assert_eq!(view.decoded.action, UNRECOGNIZED_ACTION);
    // Registered for another address only: still raw.
    let c = SignatureCeremony::new();
    c.register_gated_contract([0x11; 20], book());
    assert!(
        c.request(mint_intent(&mint_calldata(1), 1))
            .requires_raw_ack
    );
    // A call the ABI does not decode exactly stays raw too.
    let c = SignatureCeremony::new();
    c.register_gated_contract(ADDR, book());
    let mut bad = mint_calldata(1);
    bad.push(7);
    assert!(c.request(mint_intent(&bad, 1)).requires_raw_ack);
}

#[test]
fn the_book_is_bounded_and_names_are_plain() {
    let b = AbiBook::default();
    for i in 0..(MAX_CONTRACTS + 3) {
        let mut a = [0u8; 20];
        a[..8].copy_from_slice(&(i as u64).to_be_bytes());
        b.register(a, book());
    }
    assert_eq!(b.len(), MAX_CONTRACTS);
    assert!(b.get(&[0u8; 20]).is_none(), "the oldest went first");
    assert!(ContractAbi::from_abi_json("Lemon Drops", &hello_mint_abi()).is_err());
    assert!(ContractAbi::from_abi_json("X", &serde_json::json!({})).is_err());
}
