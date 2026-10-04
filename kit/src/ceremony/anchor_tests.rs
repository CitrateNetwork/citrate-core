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
        max_gas_price_wei: PLACEHOLDER_MAX_GAS_PRICE_WEI,
        max_gas_limit: PLACEHOLDER_MAX_GAS_LIMIT,
    }
}

/// An unlocked custody vault: the member is present (the anchor key is used only then).
fn unlocked_vault() -> crate::custody::CustodyVault {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "citrate-anchor-vault-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let v = crate::custody::CustodyVault::new(
        Box::new(FakeKeyring::default()),
        dir.join("custody.enc"),
        0,
    );
    v.init(&mut b"pw-for-tests".to_vec()).unwrap();
    v.unlock(&mut b"pw-for-tests".to_vec()).unwrap();
    v
}

/// Approve with an unlocked vault and an in-flight log that records every send.
fn approve(
    c: &AnchorCeremony,
    kr: &dyn Keyring,
    rpc: &RpcClient<MockRpc>,
    id: &str,
    registry: &str,
) -> Result<AnchorReceipt> {
    let v = unlocked_vault();
    let log = RefCell::new(Vec::new());
    let rec = |r: &AnchorReceipt| -> std::result::Result<(), String> {
        log.borrow_mut().push(r.clone());
        Ok(())
    };
    c.approve_and_broadcast(
        kr,
        rpc,
        id,
        registry,
        cfg(),
        AnchorGuards {
            vault: &v,
            before_send: &rec,
            not_sent: &|_| {},
        },
    )
}

fn good_request(day: u64, r: [u8; 32]) -> AnchorRequest {
    AnchorRequest {
        day,
        date: date_of_day(day).unwrap_or_default(),
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
        v.decoded.action.contains("2024-10-04"),
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
    let receipt = approve(&c, &kr, &rpc, &v.id, REGISTRY).unwrap();
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
        approve(&c, &kr, &rpc, &v.id, REGISTRY),
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
        nonce: None,
        from: None,
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
    let receipt = approve(&c, &kr, &rpc, &v.id, REGISTRY).unwrap();
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
        approve(&c, &kr, &rpc, &v.id, OTHER),
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
        approve(&c, &kr, &rpc, &v.id, REGISTRY),
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

// ---------------------------------------------------------------------------------------------
// review fixes (n4 adversarial review)

#[test]
fn the_card_date_is_derived_from_the_day_never_taken_on_trust() {
    assert_eq!(date_of_day(0).as_deref(), Some("1970-01-01"));
    assert_eq!(date_of_day(20_000).as_deref(), Some("2024-10-04"));
    assert_eq!(date_of_day(20_726).as_deref(), Some("2026-09-30"));
    assert_eq!(date_of_day(11_016).as_deref(), Some("2000-02-29"));
    assert_eq!(date_of_day(u64::MAX), None);
    let c = AnchorCeremony::new();
    let mut req = good_request(20_000, root(9));
    req.date = "2026-09-30 (approve to receive a refund)".into();
    assert!(matches!(
        c.request(req, REGISTRY),
        Err(AnchorError::NotAnchorCall(_))
    ));
    assert!(c.pending().is_empty());
    let mut far = good_request(u64::MAX, root(9));
    far.date = String::new();
    assert!(c.request(far, REGISTRY).is_err());
}

#[test]
fn a_receipt_poll_error_after_broadcast_keeps_the_sent_transaction() {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c.request(good_request(20_000, root(10)), REGISTRY).unwrap();
    // nonce, gas price, estimate, send succeed; the receipt query then fails.
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(json!("0x5")),
        ok(json!("0x3b9aca00")),
        ok(json!("0xb000")),
        ok(json!(format!("0x{}", "cd".repeat(32)))),
        json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "busy" } }),
    ]));
    let r = approve(&c, &kr, &rpc, &v.id, REGISTRY)
        .expect("a sent anchor is reported, with its receipt unknown");
    assert_eq!(r.tx_hash, format!("0x{}", "cd".repeat(32)));
    assert_eq!((r.block_number, r.status), (None, None));
    assert!(!receipt_confirms(&r));
}

// ---------------------------------------------------------------------------------------------
// red-team follow-ups: in-flight record before send, gas caps, vault-unlock gate

fn guarded<'a>(
    v: &'a crate::custody::CustodyVault,
    rec: &'a dyn Fn(&AnchorReceipt) -> std::result::Result<(), String>,
) -> AnchorGuards<'a> {
    AnchorGuards {
        vault: v,
        before_send: rec,
        not_sent: &|_| {},
    }
}

#[test]
fn the_transaction_is_recorded_as_in_flight_before_it_is_sent() {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c
        .request(good_request(20_000, root(0x51)), REGISTRY)
        .unwrap();
    let rpc = RpcClient::with_transport(happy_rpc("0x1"));
    let vault = unlocked_vault();
    let seen = RefCell::new(Vec::<(AnchorReceipt, usize)>::new());
    let rec = |r: &AnchorReceipt| -> std::result::Result<(), String> {
        // How many RPC calls had happened when the record was written: the send is not one.
        seen.borrow_mut()
            .push((r.clone(), rpc.transport().requests.borrow().len()));
        Ok(())
    };
    let receipt = c
        .approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg(), guarded(&vault, &rec))
        .unwrap();
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 1);
    let (r, calls_before) = &seen[0];
    assert_eq!(
        *calls_before, 3,
        "nonce, gas price, estimate; not yet the send"
    );
    assert_eq!((r.day, r.commitment), (20_000, root(0x51)));
    assert_eq!((r.block_number, r.status), (None, None));
    assert!(r.tx_hash.starts_with("0x") && r.tx_hash.len() == 66);
    assert!(receipt_confirms(&receipt));
}

#[test]
fn if_the_in_flight_record_cannot_be_written_nothing_is_sent() {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c
        .request(good_request(20_000, root(0x52)), REGISTRY)
        .unwrap();
    let rpc = RpcClient::with_transport(happy_rpc("0x1"));
    let vault = unlocked_vault();
    let fail = |_: &AnchorReceipt| -> std::result::Result<(), String> { Err("disk full".into()) };
    let r = c.approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg(), guarded(&vault, &fail));
    assert!(matches!(r, Err(AnchorError::NotRecorded(_))), "{r:?}");
    assert!(!rpc
        .transport()
        .methods()
        .contains(&"eth_sendRawTransaction".to_string()));
    assert_eq!(c.pending().len(), 1, "the card stays for another try");
}

#[test]
fn gas_over_the_caps_signs_nothing() {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let vault = unlocked_vault();
    let rec = |_: &AnchorReceipt| -> std::result::Result<(), String> { Ok(()) };
    for (price, limit) in [
        (PLACEHOLDER_MAX_GAS_PRICE_WEI + 1, 0xb000u64),
        (1_000_000_000u128, PLACEHOLDER_MAX_GAS_LIMIT + 1),
    ] {
        let c = AnchorCeremony::new();
        let v = c
            .request(good_request(20_000, root(0x53)), REGISTRY)
            .unwrap();
        let rpc = RpcClient::with_transport(MockRpc::new(vec![
            ok(json!("0x5")),
            ok(json!(format!("0x{price:x}"))),
            ok(json!(format!("0x{limit:x}"))),
        ]));
        let r = c.approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg(), guarded(&vault, &rec));
        assert!(matches!(r, Err(AnchorError::GasOverCap(_))), "{r:?}");
        assert_eq!(rpc.transport().methods().len(), 3, "nothing sent");
        assert_eq!(c.pending().len(), 1, "the card stays");
    }
}

#[test]
fn a_locked_vault_keeps_the_anchor_key_unused() {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c
        .request(good_request(20_000, root(0x54)), REGISTRY)
        .unwrap();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    let vault = unlocked_vault();
    vault.lock();
    let rec = |_: &AnchorReceipt| -> std::result::Result<(), String> { Ok(()) };
    assert_eq!(
        c.approve_and_broadcast(&kr, &rpc, &v.id, REGISTRY, cfg(), guarded(&vault, &rec)),
        Err(AnchorError::Locked)
    );
    assert!(rpc.transport().requests.borrow().is_empty());
    assert_eq!(c.pending().len(), 1);
}

#[test]
fn the_gas_caps_are_placeholders_pending_owner_sign_off() {
    const { assert!(GAS_CAPS_PENDING_OWNER_SIGNOFF) };
    assert_eq!(PLACEHOLDER_MAX_GAS_LIMIT, 400_000);
    assert_eq!(PLACEHOLDER_MAX_GAS_PRICE_WEI, 50_000_000_000);
}

// ---------------------------------------------------------------------------------------------
// a refused send does not hold the day forever

fn err(msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": msg } })
}

#[test]
fn the_record_carries_the_nonce_and_the_sender() {
    let kr = FakeKeyring::default();
    let addr = ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c
        .request(good_request(20_000, root(0x61)), REGISTRY)
        .unwrap();
    let rpc = RpcClient::with_transport(happy_rpc("0x1"));
    let r = approve(&c, &kr, &rpc, &v.id, REGISTRY).unwrap();
    assert_eq!(r.nonce, Some(5));
    assert_eq!(
        r.from.as_deref().map(str::to_ascii_lowercase),
        Some(addr.to_ascii_lowercase())
    );
}

#[test]
fn a_send_the_node_refused_and_does_not_hold_withdraws_the_record_and_keeps_the_card() {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c
        .request(good_request(20_000, root(0x62)), REGISTRY)
        .unwrap();
    let rpc = RpcClient::with_transport(MockRpc::new(vec![
        ok(json!("0x5")),
        ok(json!("0x3b9aca00")),
        ok(json!("0xb000")),
        err("insufficient funds for gas * price + value"),
        ok(Value::Null),
    ]));
    let vault = unlocked_vault();
    let recorded = RefCell::new(Vec::<AnchorReceipt>::new());
    let withdrawn = RefCell::new(Vec::<AnchorReceipt>::new());
    let rec = |r: &AnchorReceipt| -> std::result::Result<(), String> {
        recorded.borrow_mut().push(r.clone());
        Ok(())
    };
    let gone = |r: &AnchorReceipt| withdrawn.borrow_mut().push(r.clone());
    let out = c.approve_and_broadcast(
        &kr,
        &rpc,
        &v.id,
        REGISTRY,
        cfg(),
        AnchorGuards {
            vault: &vault,
            before_send: &rec,
            not_sent: &gone,
        },
    );
    assert!(matches!(out, Err(AnchorError::Rpc(_))), "{out:?}");
    assert_eq!(recorded.borrow().len(), 1);
    assert_eq!(
        *withdrawn.borrow(),
        *recorded.borrow(),
        "the same record is withdrawn"
    );
    assert_eq!(c.pending().len(), 1, "the card stays for another try");
    assert_eq!(
        rpc.transport().methods().last().map(String::as_str),
        Some("eth_getTransactionByHash")
    );
}

#[test]
fn a_send_error_for_a_transaction_the_node_holds_or_cannot_say_keeps_the_day_held() {
    for lookup in [ok(json!({ "hash": "0xcd" })), err("lookup failed")] {
        let kr = FakeKeyring::default();
        ensure_anchor_key(&kr).unwrap();
        let c = AnchorCeremony::new();
        let v = c
            .request(good_request(20_000, root(0x63)), REGISTRY)
            .unwrap();
        let rpc = RpcClient::with_transport(MockRpc::new(vec![
            ok(json!("0x5")),
            ok(json!("0x3b9aca00")),
            ok(json!("0xb000")),
            err("timeout"),
            lookup,
        ]));
        let vault = unlocked_vault();
        let rec = |_: &AnchorReceipt| -> std::result::Result<(), String> { Ok(()) };
        let withdrawn = RefCell::new(0usize);
        let gone = |_: &AnchorReceipt| *withdrawn.borrow_mut() += 1;
        let out = c.approve_and_broadcast(
            &kr,
            &rpc,
            &v.id,
            REGISTRY,
            cfg(),
            AnchorGuards {
                vault: &vault,
                before_send: &rec,
                not_sent: &gone,
            },
        );
        assert!(matches!(out, Err(AnchorError::Rpc(_))), "{out:?}");
        assert_eq!(*withdrawn.borrow(), 0, "possibly sent: the day stays held");
        assert!(
            c.pending().is_empty(),
            "no second card for a possibly sent day"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// P-anchor lane (stacked): gas-cap room and the refused-send fallback

/// The cap must leave room for the registry version the next redeploy ships: on the anvil
/// rehearsal (scripts/anvil-anchor-e2e.sh, 2026-10-01) its `anchor()` estimated 335,227 gas,
/// because it keeps a second, per-committer record. A cap below that refuses every anchor.
#[test]
fn the_gas_cap_leaves_room_for_the_next_registry_version() {
    const NEXT_REGISTRY_ANCHOR_GAS: u64 = 335_227;
    const ROOM: u64 = NEXT_REGISTRY_ANCHOR_GAS + NEXT_REGISTRY_ANCHOR_GAS / 10;
    const { assert!(PLACEHOLDER_MAX_GAS_LIMIT >= ROOM) };
}

// ---------------------------------------------------------------------------------------------
// review follow-up: a send the node refused is not in flight

fn node_error(msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": msg } })
}

/// Approve with scripted RPC answers; returns the result and the records passed to
/// `before_send` and `not_sent`.
fn approve_logged(
    send: &[Value],
) -> (
    Result<AnchorReceipt>,
    Vec<AnchorReceipt>,
    Vec<AnchorReceipt>,
    usize,
) {
    let kr = FakeKeyring::default();
    ensure_anchor_key(&kr).unwrap();
    let c = AnchorCeremony::new();
    let v = c
        .request(good_request(20_000, root(0x61)), REGISTRY)
        .unwrap();
    let mut answers = vec![
        ok(json!("0x5")),
        ok(json!("0x3b9aca00")),
        ok(json!("0xb000")),
    ];
    // An empty list: the transport fails on the send (no scripted answer), so its outcome is
    // unknown. Answers after the send's are the node's reply to the "do you hold it" lookup.
    answers.extend(send.iter().cloned());
    let rpc = RpcClient::with_transport(MockRpc::new(answers));
    let vault = unlocked_vault();
    let recorded = RefCell::new(Vec::new());
    let forgotten = RefCell::new(Vec::new());
    let rec = |r: &AnchorReceipt| -> std::result::Result<(), String> {
        recorded.borrow_mut().push(r.clone());
        Ok(())
    };
    let forget = |r: &AnchorReceipt| forgotten.borrow_mut().push(r.clone());
    let r = c.approve_and_broadcast(
        &kr,
        &rpc,
        &v.id,
        REGISTRY,
        cfg(),
        AnchorGuards {
            vault: &vault,
            before_send: &rec,
            not_sent: &forget,
        },
    );
    let pending = c.pending().len();
    (r, recorded.into_inner(), forgotten.into_inner(), pending)
}

#[test]
fn a_send_the_node_refuses_is_forgotten_and_the_card_stays() {
    let (r, recorded, forgotten, pending) = approve_logged(&[
        node_error("insufficient funds for gas * price + value"),
        ok(Value::Null),
    ]);
    assert!(matches!(r, Err(AnchorError::Rpc(_))), "{r:?}");
    assert_eq!(recorded.len(), 1);
    assert_eq!(forgotten, recorded, "the same record is forgotten");
    assert_eq!(pending, 1, "the card goes back for another try");
}

#[test]
fn a_send_with_an_unknown_outcome_stays_in_flight() {
    // Transport failure: the node may have the transaction.
    let (r, recorded, forgotten, pending) = approve_logged(&[]);
    assert!(matches!(r, Err(AnchorError::Rpc(_))), "{r:?}");
    assert_eq!(recorded.len(), 1);
    assert!(forgotten.is_empty(), "the re-poll decides, not the error");
    assert_eq!(pending, 0, "no second approval while it may be on the way");
    // The node already has it: accepted, so it stays in flight too.
    // Even when the lookup then says it is not held (a node behind a load balancer).
    let (_, recorded, forgotten, pending) =
        approve_logged(&[node_error("already known"), ok(Value::Null)]);
    assert_eq!(recorded.len(), 1);
    assert!(forgotten.is_empty());
    assert_eq!(pending, 0);
}

#[test]
fn only_a_node_error_proves_a_send_was_refused() {
    assert!(send_refused(&RpcError::Node("nonce too low".into())));
    assert!(send_refused(&RpcError::Node("insufficient funds".into())));
    assert!(!send_refused(&RpcError::Node("Already Known".into())));
    assert!(!send_refused(&RpcError::Node(
        "known transaction: 0xab".into()
    )));
    assert!(!send_refused(&RpcError::Transport("timeout".into())));
    assert!(!send_refused(&RpcError::BadResponse("html".into())));
    assert!(!send_refused(&RpcError::MissingField("hash".into())));
}
