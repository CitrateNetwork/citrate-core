// HUP-S1.5: x402 payment authorizations through the SignatureCeremony (HIC-1).
//
// Included into ceremony.rs's test module after ceremony_tests.rs, so it shares the in-memory
// keyring vault (`vault_with_wallet`, canonical BIP44 wallet) and `MockRpc`.
//
// Cross-implementation golden: X402_GOLDEN_SIG is `cast wallet sign --no-hash` (foundry 1.5.1)
// with the canonical BIP44 test mnemonic over the digest pinned in ceremony/x402_tests.rs. k256
// and foundry both use RFC 6979 nonces, so the bytes must be identical.

const X402_GOLDEN_SIG: &str = "b6c11594a5170b0c42a22642d4a63b39d3e855516b92ac2146a91c48020f1c2c5d7a801d6823b168f4f2b5118e182dac8ed1a07b87a7eddad0579e53869af1c91c";
const X402_WSALT_40204: &str = "0xAa918302B94a4B0E75E01e019cc6b819B4F7c906";
const X402_PAYEE: &str = "0x70997970c51812dc3a010c7d01b50e0d17dc79c8";

fn x402_domain() -> x402::X402Domain {
    x402::X402Domain::new("Wrapped SALT", "1", 40204, X402_WSALT_40204).expect("domain")
}

fn x402_golden_auth() -> x402::X402Authorization {
    x402::X402Authorization {
        from: CANONICAL_ADDRESS.to_lowercase(),
        to: X402_PAYEE.into(),
        value: "10000000000000000".into(),
        valid_after: 1_790_000_000,
        valid_before: 1_790_000_600,
        nonce: format!("0x{}", "01".repeat(32)),
    }
}

fn x402_request(auth: x402::X402Authorization) -> X402SignRequest {
    X402SignRequest {
        origin: "hermes (registry escalation)".into(),
        domain: x402_domain(),
        authorization: auth,
        resource: "model 0xabc via provider api.example.org".into(),
        asset_symbol: "wSALT".into(),
        asset_decimals: 18,
    }
}

#[test]
fn x402_request_is_pending_decoded_and_signs_nothing() {
    let (_v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request_x402(x402_request(x402_golden_auth())).expect("request");
    assert_eq!(view.kind, IntentKind::TypedData);
    assert_eq!(view.chain_id, 40204);
    assert!(!view.requires_raw_ack);
    assert!(view.decoded.action.contains("x402 TransferWithAuthorization"));
    assert!(view.decoded.cost.starts_with("0.01 wSALT"), "{}", view.decoded.cost);
    assert_eq!(view.decoded.destination, X402_PAYEE);
    assert_eq!(c.pending_count(), 1);
    assert_eq!(c.status(&view.id), Some(view));
}

#[test]
fn x402_approve_signs_the_pinned_digest_matching_foundry() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request_x402(x402_request(x402_golden_auth())).expect("request");
    let sig = c.approve(&v, &view.id, false).expect("approve");
    assert_eq!(sig.kind, IntentKind::TypedData);
    assert_eq!(sig.sig_hex, X402_GOLDEN_SIG);
    x402::verify(&x402_domain(), &x402_golden_auth(), &sig.sig_hex).expect("recovers to payer");
    let v_byte = u8::from_str_radix(&sig.sig_hex[128..130], 16).expect("v");
    assert!(v_byte == 27 || v_byte == 28, "EIP-3009 verifiers need v in 27/28");
    // Single use.
    assert_eq!(
        c.approve(&v, &view.id, false),
        Err(CeremonyError::UnknownCeremony)
    );
}

#[test]
fn generic_typed_data_with_the_same_bytes_is_still_refused() {
    // A caller (webview, agent, micro-app) cannot reach the x402 signer by submitting the same
    // authorization as ordinary typed data: only request_x402 marks a ceremony as core-built.
    // NEGATIVE CONTROL: make approve sign whenever `kind == TypedData` and this fails.
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let raw = hex::encode(serde_json::to_vec(&x402_golden_auth()).expect("json"));
    let view = c.request(SignatureIntent {
        origin: "https://evil.example".into(),
        kind: IntentKind::TypedData,
        chain_id: 40204,
        raw,
    });
    assert_eq!(
        c.approve(&v, &view.id, true),
        Err(CeremonyError::UnsupportedSigningKind)
    );
}

#[test]
fn x402_payer_must_be_this_wallet() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let mut auth = x402_golden_auth();
    auth.from = "0x1111111111111111111111111111111111111111".into();
    let view = c.request_x402(x402_request(auth)).expect("request");
    match c.approve(&v, &view.id, false) {
        Err(CeremonyError::FromMismatch { claimed, wallet }) => {
            assert_eq!(claimed, "0x1111111111111111111111111111111111111111");
            assert_eq!(wallet, CANONICAL_ADDRESS.to_lowercase());
        }
        other => panic!("expected FromMismatch, got {other:?}"),
    }
    assert_eq!(c.pending_count(), 0, "consumed even when refused");
}

#[test]
fn x402_fails_closed_on_a_locked_vault() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request_x402(x402_request(x402_golden_auth())).expect("request");
    v.lock();
    assert!(matches!(
        c.approve(&v, &view.id, false),
        Err(CeremonyError::VaultLocked) | Err(CeremonyError::NoWallet)
    ));
}

#[test]
fn x402_is_never_broadcast_as_a_transaction() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let view = c.request_x402(x402_request(x402_golden_auth())).expect("request");
    let rpc = RpcClient::with_transport(MockRpc::new(vec![]));
    assert_eq!(
        c.approve_and_broadcast(&v, &rpc, &view.id, true, bcfg(1)),
        Err(CeremonyError::UnsupportedSigningKind)
    );
    assert!(rpc.transport().requests.borrow().is_empty(), "no RPC traffic");
}

#[test]
fn x402_request_refuses_an_authorization_the_hasher_cannot_take() {
    let c = SignatureCeremony::new();
    let mut auth = x402_golden_auth();
    auth.value = "0".into();
    assert_eq!(
        c.request_x402(x402_request(auth)),
        Err(CeremonyError::SignFailed)
    );
    assert_eq!(c.pending_count(), 0);
}
