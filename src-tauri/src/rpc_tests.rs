// CORE-B1.4 — JSON-RPC client tests over a MOCK transport (CI-safe; Rule 1: the
// mock is a TEST transport, never wired as the production default). They assert
// the exact request shapes (method + raw hex), hex-quantity parsing, node-error
// surfacing, and the receipt-poll logic (pending → mined → timeout).

use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;

/// A scripted transport: records each request body and replies with the next
/// queued response. Lets tests assert the request shape AND drive multi-call
/// flows (nonce, gas, send, then a sequence of receipt polls).
struct MockTransport {
    requests: RefCell<Vec<Value>>,
    responses: RefCell<VecDeque<Value>>,
}

impl MockTransport {
    fn new(responses: Vec<Value>) -> Self {
        MockTransport {
            requests: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into_iter().collect()),
        }
    }
    fn requests(&self) -> Vec<Value> {
        self.requests.borrow().clone()
    }
}

impl RpcTransport for MockTransport {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        self.requests.borrow_mut().push(body.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("mock: no scripted response".into()))
    }
}

/// A JSON-RPC success envelope with the given result.
fn ok_result(result: Value) -> Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

#[test]
fn pending_nonce_request_is_well_formed_and_parsed() {
    let mock = MockTransport::new(vec![ok_result(Value::String("0x2a".into()))]);
    let client = RpcClient::with_transport(mock);
    let nonce = client
        .pending_nonce("0x98a32D944e9138B14A35b5D4dcE53339570F371A")
        .expect("nonce");
    assert_eq!(nonce, 42, "0x2a → 42");

    let reqs = client.transport.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0]["method"], "eth_getTransactionCount");
    assert_eq!(reqs[0]["params"][0], "0x98a32D944e9138B14A35b5D4dcE53339570F371A");
    assert_eq!(reqs[0]["params"][1], "pending", "pending nonce (not latest)");
    assert_eq!(reqs[0]["jsonrpc"], "2.0");
}

#[test]
fn get_balance_is_well_formed_and_parses_a_u128_beyond_u64() {
    // 2^64 wei — one past u64::MAX, so a u64 parse would OVERFLOW/fail; this
    // proves the native-SALT balance read uses the wide u128 path (a 32,000-SALT
    // grant is ~3.2e22 wei, far beyond u64). Rule 1: never truncate a balance.
    let mock = MockTransport::new(vec![ok_result(Value::String("0x10000000000000000".into()))]);
    let client = RpcClient::with_transport(mock);
    let bal = client
        .get_balance("0x9858effd232b4033e47d90003d41ec34ecaeda94")
        .expect("balance");
    assert_eq!(bal, 18_446_744_073_709_551_616u128, "0x1<<64 → 2^64 wei");

    let reqs = client.transport.requests();
    assert_eq!(reqs[0]["method"], "eth_getBalance");
    assert_eq!(reqs[0]["params"][0], "0x9858effd232b4033e47d90003d41ec34ecaeda94");
    assert_eq!(reqs[0]["params"][1], "latest", "balance at latest block");
}

#[test]
fn gas_price_request_is_well_formed_and_parsed() {
    let mock = MockTransport::new(vec![ok_result(Value::String("0x77359400".into()))]); // 2e9
    let client = RpcClient::with_transport(mock);
    let gp = client.gas_price().expect("gas price");
    assert_eq!(gp, 2_000_000_000);
    let reqs = client.transport.requests();
    assert_eq!(reqs[0]["method"], "eth_gasPrice");
}

#[test]
fn send_raw_transaction_request_is_well_formed_raw_hex() {
    let tx_hash = "0xabc0000000000000000000000000000000000000000000000000000000000001";
    let mock = MockTransport::new(vec![ok_result(Value::String(tx_hash.into()))]);
    let client = RpcClient::with_transport(mock);

    // The raw signed tx bytes → canonical 0x hex in the request.
    let raw = vec![0xf8u8, 0x6c, 0x09, 0xde, 0xad];
    let returned = client.send_raw_transaction(&raw).expect("send");
    assert_eq!(returned, tx_hash, "the node-accepted hash is returned verbatim");

    let reqs = client.transport.requests();
    assert_eq!(reqs[0]["method"], "eth_sendRawTransaction");
    assert_eq!(
        reqs[0]["params"][0], "0xf86c09dead",
        "raw tx submitted as 0x-prefixed hex"
    );
}

#[test]
fn node_error_object_is_surfaced_not_swallowed() {
    // The canonical "no funds" case: proves the round-trip reached the node even
    // without gas (the honest funded-account gap the scope calls out).
    let err = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "error": { "code": -32000, "message": "insufficient funds for gas * price + value" }
    });
    let mock = MockTransport::new(vec![err]);
    let client = RpcClient::with_transport(mock);
    let r = client.send_raw_transaction(&[0xf8, 0x6c]);
    assert_eq!(
        r,
        Err(RpcError::Node("insufficient funds for gas * price + value".into())),
        "a node error object surfaces verbatim (public info), not a fabricated success"
    );
}

#[test]
fn receipt_null_is_pending_then_mined_yields_block() {
    let tx_hash = "0xdeadbeef00000000000000000000000000000000000000000000000000000000";
    let receipt = serde_json::json!({
        "transactionHash": tx_hash,
        "blockNumber": "0x1a4", // 420
        "status": "0x1",
    });
    // First poll: null (pending). Second poll: the mined receipt.
    let mock = MockTransport::new(vec![
        ok_result(Value::Null),
        ok_result(receipt),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = client
        .poll_receipt(tx_hash, 3, std::time::Duration::from_millis(1))
        .expect("receipt after one pending poll");
    assert_eq!(r.tx_hash, tx_hash);
    assert_eq!(r.block_number, 420, "0x1a4 → 420 (block inclusion proof)");
    assert_eq!(r.status, Some(1), "status 0x1 = success");

    let reqs = client.transport.requests();
    assert_eq!(reqs.len(), 2, "polled twice: pending then mined");
    assert_eq!(reqs[0]["method"], "eth_getTransactionReceipt");
    assert_eq!(reqs[0]["params"][0], tx_hash);
}

#[test]
fn receipt_poll_times_out_when_never_mined() {
    let tx_hash = "0xfeed000000000000000000000000000000000000000000000000000000000000";
    // Always pending.
    let mock = MockTransport::new(vec![
        ok_result(Value::Null),
        ok_result(Value::Null),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = client.poll_receipt(tx_hash, 2, std::time::Duration::from_millis(1));
    assert_eq!(r, Err(RpcError::ReceiptTimeout), "never-mined → timeout, no fake block");
}

#[test]
fn missing_0x_prefix_quantity_is_a_field_error() {
    let mock = MockTransport::new(vec![ok_result(Value::String("2a".into()))]);
    let client = RpcClient::with_transport(mock);
    assert!(
        matches!(client.gas_price(), Err(RpcError::MissingField(_))),
        "a non-0x quantity is rejected, not silently mis-parsed"
    );
}
