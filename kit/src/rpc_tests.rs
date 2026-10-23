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

/// HUP-S7.5 (D-27): the receipt's gas used and effective price are read when present.
#[test]
fn receipt_reports_gas_used_and_the_price_paid_when_the_node_gives_them() {
    let tx_hash = "0xdeadbeef00000000000000000000000000000000000000000000000000000001";
    let mock = MockTransport::new(vec![
        ok_result(serde_json::json!({
            "blockNumber": "0x1",
            "status": "0x1",
            "gasUsed": "0xbb80",
            "effectiveGasPrice": "0x3b9aca00",
        })),
        ok_result(serde_json::json!({ "blockNumber": "0x2", "status": "0x0", "gasUsed": "nope" })),
    ]);
    let client = RpcClient::with_transport(mock);
    let r = client.transaction_receipt(tx_hash).unwrap().unwrap();
    assert_eq!(r.gas_used, Some(48_000));
    assert_eq!(r.effective_gas_price, Some(1_000_000_000));
    let r = client.transaction_receipt(tx_hash).unwrap().unwrap();
    assert_eq!((r.status, r.gas_used, r.effective_gas_price), (Some(0), None, None));
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
fn get_logs_is_well_formed_and_decodes_entries() {
    // The node returns one WithdrawalRequested log: topic0 = event sig hash,
    // topic1 = id (indexed), topic2 = staker (indexed), data = the two
    // non-indexed uints, blockNumber = 0x64 (100).
    let topic0 = "0x1111111111111111111111111111111111111111111111111111111111111111";
    let topic1 = "0x0000000000000000000000000000000000000000000000000000000000000007"; // id 7
    let topic2 = "0x0000000000000000000000009858effd232b4033e47d90003d41ec34ecaeda94";
    let log = serde_json::json!({
        "address": "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e",
        "topics": [topic0, topic1, topic2],
        "data": "0x00000000000000000000000000000000000000000000000000000000000000ff",
        "blockNumber": "0x64",
    });
    let mock = MockTransport::new(vec![ok_result(Value::Array(vec![log]))]);
    let client = RpcClient::with_transport(mock);

    let filter = serde_json::json!({
        "address": "0xfd272195b55cb4f5a240a5be75aabab0d1c5685e",
        "topics": [topic0, Value::Null, topic2],
        "fromBlock": "earliest",
        "toBlock": "latest",
    });
    let logs = client.get_logs(filter.clone()).expect("get_logs");
    assert_eq!(logs.len(), 1, "one matching log");
    assert_eq!(logs[0].topics.len(), 3, "topic0 + 2 indexed args");
    assert_eq!(logs[0].topics[1], topic1, "topic1 = id (indexed)");
    assert_eq!(logs[0].block_number, 100, "0x64 → 100");

    let reqs = client.transport.requests();
    assert_eq!(reqs[0]["method"], "eth_getLogs");
    assert_eq!(reqs[0]["params"][0], filter, "the filter is passed verbatim");
}

#[test]
fn get_logs_empty_is_an_honest_empty_list() {
    // A fresh wallet with no WithdrawalRequested events → an empty array, decoded
    // to an empty Vec (honest, not an error — Rule 1).
    let mock = MockTransport::new(vec![ok_result(Value::Array(vec![]))]);
    let client = RpcClient::with_transport(mock);
    let logs = client.get_logs(serde_json::json!({})).expect("empty logs");
    assert!(logs.is_empty(), "no logs → empty Vec, never a fabricated entry");
}

#[test]
fn get_logs_malformed_blocknumber_is_a_field_error() {
    // A log missing its blockNumber must error, never silently produce a
    // block-0 entry (which would mislead the claimable-delay math).
    let log = serde_json::json!({
        "topics": ["0x00"],
        "data": "0x",
        // blockNumber omitted
    });
    let mock = MockTransport::new(vec![ok_result(Value::Array(vec![log]))]);
    let client = RpcClient::with_transport(mock);
    assert!(
        matches!(client.get_logs(serde_json::json!({})), Err(RpcError::MissingField(_))),
        "a log missing blockNumber is rejected, not silently zeroed"
    );
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

/// A loopback RPC target must not be able to send the request somewhere else: the HTTP transport
/// never follows a redirect (the fork-RPC guard checks the URL it was given, not where a 3xx goes).
#[test]
fn the_http_transport_never_follows_a_redirect() {
    for code in ["301 Moved Permanently", "302 Found", "303 See Other", "307 Temporary Redirect", "308 Permanent Redirect"] {
        redirect_case(code);
    }
}

fn redirect_case(status: &'static str) {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let elsewhere = TcpListener::bind("127.0.0.1:0").unwrap();
    let elsewhere_addr = elsewhere.local_addr().unwrap();
    elsewhere.set_nonblocking(true).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let first = TcpListener::bind("127.0.0.1:0").unwrap();
    let first_addr = first.local_addr().unwrap();
    let t = std::thread::spawn(move || {
        if let Ok((mut s, _)) = first.accept() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 {status}\r\nLocation: http://{elsewhere_addr}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    let t2 = HttpTransport::new(format!("http://{first_addr}/"));
    let r = t2.call(serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}));
    let _ = t.join();
    // Anything that reached the second listener would be waiting in its accept queue.
    std::thread::sleep(std::time::Duration::from_millis(50));
    if elsewhere.accept().is_ok() {
        hits.fetch_add(1, Ordering::SeqCst);
    }
    assert_eq!(hits.load(Ordering::SeqCst), 0, "{status}: the redirect was followed");
    assert!(r.is_err(), "{status}: a redirect is an error, not an answer: {r:?}");
}

// UI truth contract v1.1 §1: the node's rate limiter answers HTTP 429 before the handler runs, so
// it is a definitive refusal of the request; the transport keeps it distinct from other failures.
#[test]
fn http_429_maps_to_rate_limited_and_other_statuses_stay_transport() {
    assert_eq!(map_ureq_error(ureq::Error::StatusCode(429)), RpcError::RateLimited);
    assert!(matches!(map_ureq_error(ureq::Error::StatusCode(502)), RpcError::Transport(_)));
}


// C1-W — citrate-chain#334 answers an eth_getLogs range over 1000 blocks with -32005; a paging
// caller must be able to tell that from any other failure, to halve its window.
#[test]
fn get_logs_range_tells_a_too_wide_range_from_other_errors() {
    let too_wide = json!({ "jsonrpc": "2.0", "id": 1, "error": {
        "code": -32005, "message": "block range too large", "data": { "maxBlockRange": 1000 } } });
    let other = json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "boom" } });
    let fine = json!({ "jsonrpc": "2.0", "id": 1, "result": [
        { "topics": ["0x01"], "data": "0x", "blockNumber": "0x10" } ] });
    let rpc = RpcClient::with_transport(MockTransport::new(vec![too_wide, other, fine]));
    let f = json!({ "fromBlock": "0x0", "toBlock": "0x7d0" });
    assert!(matches!(rpc.get_logs_range(f.clone()), Err(GetLogsError::RangeTooLarge(m)) if m.contains("too large")));
    assert!(matches!(rpc.get_logs_range(f.clone()), Err(GetLogsError::Rpc(RpcError::Node(m))) if m == "boom"));
    let logs = rpc.get_logs_range(f).expect("logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].block_number, 16);
}

// C1-W (#334 review) — -32002 means "retry this range later": a paging caller retries it.
#[test]
fn get_logs_range_reports_a_retry_answer() {
    let retry = json!({ "jsonrpc": "2.0", "id": 1, "error": {
        "code": -32002, "message": "retry", "data": { "retryable": true } } });
    // #371 review: only an error that says it is retryable is retried.
    let not_marked = json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32002, "message": "busy" } });
    let marked_false = json!({ "jsonrpc": "2.0", "id": 1, "error": {
        "code": -32002, "message": "busy", "data": { "retryable": false } } });
    let rpc = RpcClient::with_transport(MockTransport::new(vec![retry, not_marked, marked_false]));
    let f = json!({ "fromBlock": "0x1ab", "toBlock": "0x592" });
    assert!(matches!(rpc.get_logs_range(f.clone()), Err(GetLogsError::Retry(m)) if m == "retry"));
    assert!(matches!(rpc.get_logs_range(f.clone()), Err(GetLogsError::Rpc(RpcError::Node(m))) if m == "busy"));
    assert!(matches!(rpc.get_logs_range(f), Err(GetLogsError::Rpc(RpcError::Node(m))) if m == "busy"));
}

// C1-W — eth_call_batch answers in call order whatever order the batch reply comes back in.
#[test]
fn eth_call_batch_answers_in_call_order() {
    // build_request ids 1 and 2 are taken below, so the batch uses 3 and 4: answer them out of order.
    let reply = json!([
        { "jsonrpc": "2.0", "id": 4, "result": "0x02" },
        { "jsonrpc": "2.0", "id": 3, "result": "0x01" },
    ]);
    let rpc2 = RpcClient::with_transport(MockTransport::new(vec![reply]));
    let _ = rpc2.build_request("x", json!([]));
    let _ = rpc2.build_request("x", json!([]));
    let got = rpc2
        .eth_call_batch(&[json!({"to": "0x01"}), json!({"to": "0x02"})])
        .expect("batch");
    assert_eq!(got, vec![vec![1u8], vec![2u8]]);
    let failed = json!([
        { "jsonrpc": "2.0", "id": 1, "result": "0x01" },
        { "jsonrpc": "2.0", "id": 2, "error": { "code": -32000, "message": "boom" } },
    ]);
    let rpc3 = RpcClient::with_transport(MockTransport::new(vec![failed]));
    assert!(rpc3.eth_call_batch(&[json!({}), json!({})]).is_err(), "no partial list");
}

// chain#337 citrate_isExecuted(hash) -> {executed, number|null}. -32601 = an older node (None);
// -32603 (no tip), -32602 (malformed hash) and a malformed result are errors.
#[test]
fn is_executed_answers() {
    let h = format!("0x{}", "ab".repeat(32));
    let yes = json!({ "jsonrpc": "2.0", "id": 1, "result": { "executed": true, "number": "0x1770" } });
    let no = json!({ "jsonrpc": "2.0", "id": 2, "result": { "executed": false, "number": null } });
    let absent = json!({ "jsonrpc": "2.0", "id": 3, "error": { "code": -32601, "message": "Method not found" } });
    let no_tip = json!({ "jsonrpc": "2.0", "id": 4, "error": { "code": -32603, "message": "no applied tip" } });
    let bad_hash = json!({ "jsonrpc": "2.0", "id": 5, "error": { "code": -32602, "message": "malformed hash" } });
    let malformed = json!({ "jsonrpc": "2.0", "id": 6, "result": { "executed": "yes" } });
    let rpc = RpcClient::with_transport(MockTransport::new(vec![yes, no, absent, no_tip, bad_hash, malformed]));
    assert_eq!(rpc.is_executed(&h), Ok(Some((true, Some(6000)))));
    assert_eq!(rpc.is_executed(&h), Ok(Some((false, None))));
    assert_eq!(rpc.is_executed(&h), Ok(None));
    assert!(rpc.is_executed(&h).is_err(), "-32603");
    assert!(rpc.is_executed(&h).is_err(), "-32602");
    assert!(rpc.is_executed(&h).is_err(), "malformed");
    assert_eq!(rpc.transport().requests()[0]["method"], "citrate_isExecuted");
    assert_eq!(rpc.transport().requests()[0]["params"][0], json!(h));
}

// C1-W (citrate-chain#337 @ 600d2984) — -32005 has two sources on the chain: a log range over
// MAX_LOG_BLOCK_RANGE carries `data.maxBlockRange` (core/api/src/canonical_logs.rs:127-146), while
// the per-method budget answers -32005 with no data (core/api/src/rate_limit.rs:96-101). Only the
// first is a reason to halve the window.
#[test]
fn get_logs_range_halves_only_on_a_range_limit() {
    let budget = json!({ "jsonrpc": "2.0", "id": 1, "error": {
        "code": -32005, "message": "Method budget exceeded. Reduce call frequency for expensive methods." } });
    let rpc = RpcClient::with_transport(MockTransport::new(vec![budget]));
    let got = rpc.get_logs_range(json!({ "fromBlock": "0x1ab", "toBlock": "0x592" }));
    assert!(matches!(got, Err(GetLogsError::Rpc(RpcError::Node(ref m))) if m.contains("budget")), "{got:?}");
}

// citrate_getLogs([filter]) -> {"logs": [...], "through": {number, hash} | null}
// (core/api/src/eth_rpc.rs:1946-2026; core/api/tests/executed_head.rs:910
// get_logs_answers_logs_and_the_block_it_is_complete_through).
#[test]
fn citrate_get_logs_answers_logs_and_through() {
    let h = format!("0x{}", "ab".repeat(32));
    let served = json!({ "jsonrpc": "2.0", "id": 1, "result": {
        "logs": [ { "topics": ["0x01"], "data": "0x", "blockNumber": "0x10" } ],
        "through": { "number": "0x12", "hash": h } } });
    let nothing = json!({ "jsonrpc": "2.0", "id": 2, "result": { "logs": [], "through": null } });
    let rpc = RpcClient::with_transport(MockTransport::new(vec![served, nothing]));
    let f = json!({ "fromBlock": "0x10", "toBlock": "0x3f7", "afterBlockHash": h });
    let got = rpc.citrate_get_logs(f.clone());
    assert!(
        matches!(&got, Ok(s) if s.logs.len() == 1 && s.through == Some(BlockRef { number: 0x12, hash: h.clone() })),
        "{got:?}"
    );
    let got = rpc.citrate_get_logs(f.clone());
    assert!(matches!(&got, Ok(s) if s.logs.is_empty() && s.through.is_none()), "{got:?}");
    let req = &rpc.transport().requests()[0];
    assert_eq!(req["method"], "citrate_getLogs");
    assert_eq!(req["params"][0], f, "the filter, afterBlockHash included, is sent as is");
}

// The error answers a scanner acts on, and the malformed answers it must refuse
// (core/api/src/canonical_logs.rs:646-673 for -32002 / -32001; -32601 = the method is absent, as
// on today's public RPC).
#[test]
fn citrate_get_logs_classifies_errors_and_refuses_malformed_answers() {
    let err = |code: i64, data: Value| json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": code, "message": "m", "data": data } });
    let res = |r: Value| json!({ "jsonrpc": "2.0", "id": 1, "result": r });
    let h = format!("0x{}", "cd".repeat(32));
    let answers = vec![
        err(-32001, json!({ "reorged": true, "height": "0x1aa" })),
        err(-32001, Value::Null),
        err(-32601, Value::Null),
        err(-32005, json!({ "maxBlockRange": 1000 })),
        err(-32005, Value::Null),
        err(-32002, json!({ "retryable": true, "blockNumber": "0x1194" })),
        err(-32002, Value::Null),
        res(json!({ "logs": [] })),
        res(json!({ "logs": [], "through": { "number": "0x5", "hash": "0x12" } })),
        res(json!({ "logs": [], "through": { "number": "five", "hash": h } })),
        res(json!({ "logs": {}, "through": null })),
        res(json!([])),
    ];
    let rpc = RpcClient::with_transport(MockTransport::new(answers));
    let f = json!({ "fromBlock": "0x1ab", "toBlock": "0x592" });
    let mut got = Vec::new();
    for _ in 0..12 {
        got.push(rpc.citrate_get_logs(f.clone()));
    }
    assert!(matches!(got[0], Err(GetLogsError::Reorged(_))), "-32001 reorged: {:?}", got[0]);
    assert!(matches!(got[1], Err(GetLogsError::Rpc(_))), "-32001 without data.reorged: {:?}", got[1]);
    assert!(matches!(got[2], Err(GetLogsError::MethodNotFound)), "-32601: {:?}", got[2]);
    assert!(matches!(got[3], Err(GetLogsError::RangeTooLarge(_))), "-32005 range: {:?}", got[3]);
    assert!(matches!(got[4], Err(GetLogsError::Rpc(_))), "-32005 budget: {:?}", got[4]);
    assert!(matches!(got[5], Err(GetLogsError::Retry(_))), "-32002 retryable: {:?}", got[5]);
    assert!(matches!(got[6], Err(GetLogsError::Rpc(_))), "-32002 not retryable: {:?}", got[6]);
    for (i, g) in got.iter().enumerate().skip(7) {
        assert!(matches!(g, Err(GetLogsError::Rpc(_))), "malformed answer {i}: {g:?}");
    }
}
