//! HUP-S7.3 (core): the anchor ceremony and its single-purpose signer.

use super::*;
use crate::custody::{CustodyError, Keyring};
use crate::rpc::{RpcClient, RpcError, RpcTransport};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

const REGISTRY: &str = "0x00000000000000000000000000000000000000a1";
const OTHER: &str = "0x00000000000000000000000000000000000000b2";

#[derive(Default)]
struct FakeKeyring(Mutex<HashMap<String, Vec<u8>>>);
impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.0
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

struct DeadKeyring;
impl Keyring for DeadKeyring {
    fn get(&self, _a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
}

struct MockRpc {
    requests: RefCell<Vec<Value>>,
    responses: RefCell<VecDeque<Value>>,
}
impl MockRpc {
    fn new(responses: Vec<Value>) -> Self {
        MockRpc {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }
    fn methods(&self) -> Vec<String> {
        self.requests
            .borrow()
            .iter()
            .map(|r| r["method"].as_str().unwrap_or("").to_string())
            .collect()
    }
}
impl RpcTransport for MockRpc {
    fn call(&self, body: Value) -> std::result::Result<Value, RpcError> {
        self.requests.borrow_mut().push(body);
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}
fn ok(result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

fn root(b: u8) -> [u8; 32] {
    [b; 32]
}

fn cfg() -> AnchorTxConfig {
    AnchorTxConfig {
        poll_attempts: 2,
        poll_interval: std::time::Duration::from_millis(1),
    }
}

fn good_request(day: u64, r: [u8; 32]) -> AnchorRequest {
    AnchorRequest {
        day,
        date: "2026-09-30".into(),
        commitment: r,
        to: REGISTRY.into(),
        chain_id: 40204,
        value: 0,
        data: anchor_calldata(&r),
    }
}

/// The scripted happy path: nonce, gas price, estimate, send, receipt.
fn happy_rpc(status: &str) -> MockRpc {
    MockRpc::new(vec![
        ok(json!("0x5")),
        ok(json!("0x3b9aca00")),
        ok(json!("0xb000")),
        ok(json!(format!("0x{}", "cd".repeat(32)))),
        ok(json!({ "blockNumber": "0x2a", "status": status })),
    ])
}

// ---------------------------------------------------------------------------------------------
// calldata

#[test]
fn calldata_is_anchor_nightly_merkle_and_matches_the_runtime_vector() {
    let r = root(0x11);
    let d = anchor_calldata(&r);
    assert_eq!(d.len(), 68);
    assert_eq!(&d[..4], &[0x9e, 0x62, 0x1f, 0x4c]);
    assert!(d[4..35].iter().all(|b| *b == 0));
    assert_eq!(d[35], 2, "AnchorKind.NightlyMerkle");
    assert_eq!(&d[36..], &r);
    // selector = keccak256("anchor(uint8,bytes32)")[..4]
    use sha3::{Digest, Keccak256};
    let h = Keccak256::digest(b"anchor(uint8,bytes32)");
    assert_eq!(&h[..4], &ANCHOR_SELECTOR);
    assert_eq!(decode_anchor_calldata(&d).unwrap(), r);
}

#[test]
fn decode_refuses_anything_but_a_nightly_anchor() {
    let r = root(0x22);
    let good = anchor_calldata(&r);
    let mut wrong_kind = good.clone();
    wrong_kind[35] = 1;
    let mut dirty_word = good.clone();
    dirty_word[10] = 1;
    let mut wrong_sel = good.clone();
    wrong_sel[0] = 0;
    for bad in [
        wrong_kind,
        dirty_word,
        wrong_sel,
        good[..67].to_vec(),
        [good.clone(), vec![0]].concat(),
    ] {
        assert!(decode_anchor_calldata(&bad).is_err());
    }
}

// ---------------------------------------------------------------------------------------------
// the anchor key

#[test]
fn the_anchor_key_is_generated_once_and_only_its_address_leaves() {
    let kr = FakeKeyring::default();
    assert_eq!(anchor_address(&kr).unwrap(), None);
    let a = ensure_anchor_key(&kr).unwrap();
    assert!(a.starts_with("0x") && a.len() == 42, "{a}");
    assert_eq!(ensure_anchor_key(&kr).unwrap(), a, "idempotent");
    assert_eq!(anchor_address(&kr).unwrap(), Some(a.clone()));
    let stored =
        kr.0.lock()
            .unwrap()
            .get(ANCHOR_KEY_ACCOUNT)
            .cloned()
            .unwrap();
    assert_eq!(stored.len(), 32);
    // a second keyring gets an independent key (CSPRNG, not derived from anything shared)
    let kr2 = FakeKeyring::default();
    assert_ne!(ensure_anchor_key(&kr2).unwrap(), a);
}

#[test]
fn a_malformed_stored_key_is_refused_not_replaced() {
    let kr = FakeKeyring::default();
    kr.set(ANCHOR_KEY_ACCOUNT, &[1, 2, 3]).unwrap();
    assert_eq!(anchor_address(&kr), Err(AnchorError::KeyCorrupt));
    assert_eq!(ensure_anchor_key(&kr), Err(AnchorError::KeyCorrupt));
    assert_eq!(
        kr.0.lock().unwrap().get(ANCHOR_KEY_ACCOUNT).cloned(),
        Some(vec![1, 2, 3]),
        "never silently overwritten"
    );
}

#[test]
fn an_unreachable_keyring_fails_closed() {
    assert_eq!(anchor_address(&DeadKeyring), Err(AnchorError::Keyring));
    assert_eq!(ensure_anchor_key(&DeadKeyring), Err(AnchorError::Keyring));
}

// ---------------------------------------------------------------------------------------------
// request: only the single-purpose call is accepted

#[test]
fn request_accepts_only_the_pinned_nightly_anchor() {
    let c = AnchorCeremony::new();
    let r = root(0x33);
    let mk = |f: &dyn Fn(&mut AnchorRequest)| {
        let mut q = good_request(20_000, r);
        f(&mut q);
        q
    };
    let bad = [
        mk(&|q| q.chain_id = 1),
        mk(&|q| q.value = 1),
        mk(&|q| q.to = OTHER.into()),
        mk(&|q| q.data = anchor_calldata(&root(0x34))),
        mk(&|q| q.data[35] = 0),
        mk(&|q| q.to = "0x1234".into()),
    ];
    for q in bad {
        assert!(c.request(q, REGISTRY).is_err());
    }
    assert!(c.pending().is_empty());
    let v = c.request(good_request(20_000, r), REGISTRY).unwrap();
    assert_eq!(v.origin, ANCHOR_ORIGIN);
    assert_eq!(v.day, 20_000);
    assert_eq!(v.registry, REGISTRY);
    assert!(
        v.decoded.action.contains("2026-09-30"),
        "{}",
        v.decoded.action
    );
    assert_eq!(v.decoded.destination, REGISTRY);
    // the pinned registry comparison ignores letter case only
    let upper = REGISTRY.to_ascii_uppercase().replacen("0X", "0x", 1);
    assert!(c.request(good_request(20_001, r), &upper).is_ok());
}

#[test]
fn one_pending_anchor_per_day() {
    let c = AnchorCeremony::new();
    let a = c.request(good_request(20_000, root(1)), REGISTRY).unwrap();
    let b = c.request(good_request(20_000, root(1)), REGISTRY).unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(c.pending().len(), 1);
    // a different commitment for the same day is a conflict, never a second ceremony
    assert_eq!(
        c.request(good_request(20_000, root(2)), REGISTRY),
        Err(AnchorError::DayConflict)
    );
}

// ---------------------------------------------------------------------------------------------
// approve: sign with the anchor key, broadcast, report the receipt honestly

fn decode_raw(raw_hex: &str) -> (Vec<u8>, u128, Vec<u8>, u64, [u8; 32], [u8; 32]) {
    let raw = hex::decode(raw_hex.trim_start_matches("0x")).unwrap();
    let rlp = rlp::Rlp::new(&raw);
    let to: Vec<u8> = rlp.val_at(3).unwrap();
    let value: u128 = rlp.val_at(4).unwrap();
    let data: Vec<u8> = rlp.val_at(5).unwrap();
    let v: u64 = rlp.val_at(6).unwrap();
    let r: Vec<u8> = rlp.val_at(7).unwrap();
    let s: Vec<u8> = rlp.val_at(8).unwrap();
    let mut rr = [0u8; 32];
    rr[32 - r.len()..].copy_from_slice(&r);
    let mut ss = [0u8; 32];
    ss[32 - s.len()..].copy_from_slice(&s);
    (to, value, data, v, rr, ss)
}

#[test]
fn approve_signs_with_the_anchor_key_and_reports_the_receipt() {
    let kr = FakeKeyring::default();
    let addr = ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let r = root(0x44);
    let v = c.request(good_request(20_000, r), REGISTRY).unwrap();
    let mock = happy_rpc("0x1");
    let rpc = RpcClient::with_transport(mock);
    let receipt = c
        .approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg())
        .unwrap();
    assert_eq!(receipt.tx_hash, format!("0x{}", "cd".repeat(32)));
    assert_eq!(receipt.block_number, Some(42));
    assert_eq!(receipt.status, Some(1));
    assert!(receipt_confirms(&receipt));
    assert_eq!(receipt.day, 20_000);
    assert_eq!(receipt.commitment, r);

    let t = rpc.transport();
    assert_eq!(
        t.methods(),
        vec![
            "eth_getTransactionCount",
            "eth_gasPrice",
            "eth_estimateGas",
            "eth_sendRawTransaction",
            "eth_getTransactionReceipt"
        ]
    );
    let reqs = t.requests.borrow();
    // the nonce is the anchor key's, not the wallet's
    assert_eq!(reqs[0]["params"][0].as_str().unwrap(), addr);
    let raw = reqs[3]["params"][0].as_str().unwrap().to_string();
    let (to, value, data, v_, rr, ss) = decode_raw(&raw);
    assert_eq!(format!("0x{}", hex::encode(&to)), REGISTRY);
    assert_eq!(value, 0);
    assert_eq!(data, anchor_calldata(&r));
    // recover the signer: it is the anchor key
    use k256::ecdsa::{RecoveryId, Signature as KSig, VerifyingKey};
    let fields = citrate_wallet_core::LegacyTxFields {
        nonce: 5,
        gas_price: 1_000_000_000,
        gas_limit: 0xb000,
        to: Some(to.clone().try_into().unwrap()),
        value: 0,
        data: data.clone(),
    };
    let mut stream = rlp::RlpStream::new_list(9);
    stream.append(&fields.nonce);
    stream.append(&fields.gas_price);
    stream.append(&fields.gas_limit);
    stream.append(&to);
    stream.append(&fields.value);
    stream.append(&fields.data);
    stream.append(&40204u64);
    stream.append(&0u8);
    stream.append(&0u8);
    use sha3::{Digest, Keccak256};
    let sighash: [u8; 32] = Keccak256::digest(stream.out()).into();
    let rec = RecoveryId::from_byte((v_ - 40204 * 2 - 35) as u8).unwrap();
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&rr);
    sig[32..].copy_from_slice(&ss);
    let vk = VerifyingKey::recover_from_prehash(
        &sighash,
        &KSig::from_bytes((&sig).into()).unwrap(),
        rec,
    )
    .unwrap();
    assert_eq!(address_of(&vk), addr);

    // single-use: the ceremony is consumed
    assert!(c.pending().is_empty());
    assert_eq!(
        c.approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg()),
        Err(AnchorError::UnknownCeremony)
    );
}

#[test]
fn a_reverted_or_unmined_receipt_never_confirms() {
    let mk = |block: Option<u64>, status: Option<u64>| AnchorReceipt {
        day: 1,
        commitment: root(1),
        tx_hash: "0x".into(),
        block_number: block,
        status,
    };
    assert!(receipt_confirms(&mk(Some(1), Some(1))));
    assert!(!receipt_confirms(&mk(Some(1), Some(0))));
    assert!(!receipt_confirms(&mk(Some(1), None)));
    assert!(!receipt_confirms(&mk(None, Some(1))));
    assert!(!receipt_confirms(&mk(None, None)));

    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c.request(good_request(20_000, root(5)), REGISTRY).unwrap();
    let rpc = RpcClient::with_transport(happy_rpc("0x0"));
    let receipt = c
        .approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg())
        .unwrap();
    assert_eq!(receipt.status, Some(0));
    assert!(!receipt_confirms(&receipt));
}

#[test]
fn a_registry_change_between_request_and_approve_sends_nothing() {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c.request(good_request(20_000, root(6)), REGISTRY).unwrap();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert_eq!(
        c.approve_and_broadcast(&kr, &rpc, &v.id, OTHER, cfg()),
        Err(AnchorError::RegistryMismatch)
    );
    assert!(rpc.transport().requests.borrow().is_empty());
}

#[test]
fn approve_without_an_anchor_key_signs_nothing() {
    let kr = FakeKeyring::default();
    let c = AnchorCeremony::new();
    let v = c.request(good_request(20_000, root(7)), REGISTRY).unwrap();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert_eq!(
        c.approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg()),
        Err(AnchorError::NoAnchorKey)
    );
    assert!(rpc.transport().requests.borrow().is_empty());
}

#[test]
fn reject_consumes_without_signing() {
    let c = AnchorCeremony::new();
    let v = c.request(good_request(20_000, root(8)), REGISTRY).unwrap();
    c.reject(&v.id).unwrap();
    assert!(c.pending().is_empty());
    assert_eq!(c.reject(&v.id), Err(AnchorError::UnknownCeremony));
}

/// Rule 3 tripwire: the anchor module signs only through `sign_eip155_legacy_tx` over fields it
/// builds itself (a fixed destination, zero value, the encoded anchor call), and never reaches
/// the wallet's gated signers.
#[test]
fn the_anchor_module_never_touches_the_wallet_signers() {
    let src = include_str!("anchor.rs");
    let non_test = match src.find("#[cfg(test)]") {
        Some(i) => &src[..i],
        None => src,
    };
    for forbidden in [
        ["wallet::", "sign_"].concat(),
        ["read_", "entropy"].concat(),
        ["derive_scoped", "_secret"].concat(),
    ] {
        assert!(
            !non_test.contains(&forbidden),
            "anchor.rs must not use {forbidden}"
        );
    }
    assert_eq!(
        non_test
            .matches(&["sign_eip155", "_legacy_tx("].concat())
            .count(),
        1,
        "exactly one signing site"
    );
}
