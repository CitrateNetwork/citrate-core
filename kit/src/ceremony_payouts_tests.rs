// The FL payouts gate at the signer: every transaction the ceremony signs is checked after the
// network gate and before the nonce read, so a call the webview builds itself (any origin, any
// calldata, any value) is refused there too. Uses the helpers of ceremony_tests.rs.

fn fl_policy(open: bool) -> crate::payouts_gate::PayoutsPolicy {
    use crate::payouts_gate::{FlContract, FlPin, PayoutsPolicy};
    use sha3::{Digest, Keccak256};
    let pin = |contract, byte: u8, code: &[u8]| FlPin {
        contract,
        address: Some([byte; 20]),
        code_hash: Some(Keccak256::digest(code).into()),
        pending: None,
    };
    PayoutsPolicy {
        network: "testnet".into(),
        payouts_open: open,
        pins: vec![
            pin(FlContract::WorkMarket, 0x11, b"wm"),
            pin(FlContract::FlRoundVerifier, 0x22, b"verifier"),
        ],
    }
}

const FL_WM: &str = "0x1111111111111111111111111111111111111111";
const FL_VERIFIER: &str = "0x2222222222222222222222222222222222222222";

/// A join(uint256,uint128,bytes32[]) to WorkMarket carrying a join bond, as a webview would build
/// it for `sign_request`.
fn forged_join(from: &str) -> SignatureIntent {
    let mut data = crate::payouts_gate::selector_of("join(uint256,uint128,bytes32[])").to_vec();
    data.extend_from_slice(&[0u8; 96]);
    SignatureIntent {
        origin: "https://app.citrate.ai".to_string(),
        kind: IntentKind::Transaction,
        chain_id: 40204,
        raw: serde_json::json!({
            "from": from,
            "to": FL_WM,
            "value": "0xde0b6b3a7640000",
            "data": format!("0x{}", hex::encode(data)),
            "gas": "0x40000",
        })
        .to_string(),
    }
}

#[test]
fn payouts_dark_a_forged_join_is_refused_at_the_signer_before_any_rpc() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    assert!(c.set_payouts_policy(fl_policy(false)));
    // `request` itself never refuses (it signs nothing); the signer does.
    let view = c.request(forged_join(&canonical_addr_lower()));
    let client = RpcClient::with_transport(MockRpc::new(vec![]));
    let err = c
        .approve_and_broadcast(&v, &client, &view.id, true, bcfg(1))
        .expect_err("a payout call while payouts are dark is not signed");
    assert_eq!(
        err,
        CeremonyError::PayoutsRefused(
            "FL payouts are not open on network \"testnet\", so this WorkMarket join call is not signed"
                .into()
        )
    );
    assert!(
        client.transport().requests.borrow().is_empty(),
        "no nonce, gas price or send was asked for: nothing was signed"
    );
    assert_eq!(c.pending_count(), 0, "the refused request is consumed, not left approvable");
}

#[test]
fn payouts_dark_the_prepare_path_refuses_too() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    c.set_payouts_policy(fl_policy(false));
    let view = c.request(forged_join(&canonical_addr_lower()));
    let client = RpcClient::with_transport(MockRpc::new(vec![]));
    let err = c
        .approve_and_sign(&v, &client, &view.id, true, 40204)
        .expect_err("refused");
    assert!(matches!(err, CeremonyError::PayoutsRefused(_)), "{err:?}");
    assert_eq!(c.prepared_len(), 0, "nothing signed is held for a send");
}

#[test]
fn payouts_dark_propose_checked_refuses_before_a_card_opens() {
    let c = SignatureCeremony::new();
    c.set_payouts_policy(fl_policy(false));
    let client = RpcClient::with_transport(MockRpc::new(vec![]));
    let err = propose_checked(&c, forged_join(&canonical_addr_lower()), &client)
        .expect_err("refused at the proposal");
    assert!(err.contains("FL payouts are not open"), "{err}");
    assert_eq!(c.pending_count(), 0, "no card opened");
    // A plain transfer elsewhere still opens its card.
    let ok = propose_checked(
        &c,
        tx_intent(&canonical_addr_lower(), "0x3535353535353535353535353535353535353535", "0x1"),
        &client,
    );
    assert!(ok.is_ok());
    assert_eq!(c.pending_count(), 1);
}

#[test]
fn payouts_open_with_matching_code_signs_and_reads_the_code_first() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    c.set_payouts_policy(fl_policy(true));
    let view = c.request(forged_join(&canonical_addr_lower()));
    let tx_hash = "0xabc0000000000000000000000000000000000000000000000000000000000abc";
    let client = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String(format!("0x{}", hex::encode(b"wm")))), // eth_getCode WorkMarket
        ok(JsonValue::String(format!("0x{}", hex::encode(b"verifier")))), // eth_getCode verifier
        ok(JsonValue::String("0x7".into())),                        // eth_getTransactionCount
        ok(JsonValue::String("0x77359400".into())),                 // eth_gasPrice
        ok(JsonValue::String(tx_hash.into())),                      // eth_sendRawTransaction
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]));
    let r = c
        .approve_and_broadcast(&v, &client, &view.id, true, bcfg(2))
        .expect("open and pinned: the HIC-1 approval signs");
    assert_eq!(r.tx_hash, tx_hash);
    assert_eq!(r.value_wei.as_deref(), Some("1000000000000000000"), "the exact wei the card showed");
    let reqs = client.transport().requests.borrow();
    let methods: Vec<&str> = reqs.iter().filter_map(|r| r["method"].as_str()).collect();
    assert_eq!(&methods[..3], ["eth_getCode", "eth_getCode", "eth_getTransactionCount"]);
    assert_eq!(reqs[0]["params"][0], FL_WM);
    assert_eq!(reqs[1]["params"][0], FL_VERIFIER);
}

#[test]
fn payouts_open_with_other_code_is_refused_before_the_nonce() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    c.set_payouts_policy(fl_policy(true));
    let view = c.request(forged_join(&canonical_addr_lower()));
    let client = RpcClient::with_transport(MockRpc::new(vec![ok(JsonValue::String(
        "0x6060".into(),
    ))]));
    let err = c
        .approve_and_broadcast(&v, &client, &view.id, true, bcfg(1))
        .expect_err("another build at the pinned address");
    match err {
        CeremonyError::PayoutsRefused(m) => assert!(m.contains("not the pinned build"), "{m}"),
        other => panic!("expected a payouts refusal, got {other:?}"),
    }
    assert_eq!(client.transport().requests.borrow().len(), 1, "only the code was read");
}

#[test]
fn payouts_gate_leaves_other_transactions_alone() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    c.set_payouts_policy(fl_policy(false));
    let view = c.request(tx_intent(
        &canonical_addr_lower(),
        "0x3535353535353535353535353535353535353535",
        "0x1",
    ));
    let client = RpcClient::with_transport(MockRpc::new(vec![
        ok(JsonValue::String("0x7".into())),
        ok(JsonValue::String("0x77359400".into())),
        ok(JsonValue::String(
            "0xabc0000000000000000000000000000000000000000000000000000000000abc".into(),
        )),
        ok(serde_json::json!({ "blockNumber": "0x64", "status": "0x1" })),
    ]));
    c.approve_and_broadcast(&v, &client, &view.id, false, bcfg(2))
        .expect("a plain transfer is not a payout call");
}

#[test]
fn payouts_refusal_reads_as_not_submitted() {
    let e = send_prepared_error(CeremonyError::PayoutsRefused("x".into()));
    assert_eq!(e.code, "failed");
    assert!(e.message.starts_with("not submitted: "), "{}", e.message);
}
