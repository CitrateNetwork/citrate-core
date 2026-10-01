// HUP-S6.7 — contract reader tests. Pure: fixture explorer bodies and a recording RPC transport.
use super::*;
use std::cell::RefCell;

const ADDR: &str = "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed";

/// A test transport that answers each JSON-RPC method from a script and records the requests.
struct ScriptedRpc {
    answers: Vec<(&'static str, serde_json::Value)>,
    seen: RefCell<Vec<serde_json::Value>>,
}

impl ScriptedRpc {
    fn new(answers: Vec<(&'static str, serde_json::Value)>) -> Self {
        ScriptedRpc {
            answers,
            seen: RefCell::new(Vec::new()),
        }
    }
}

impl crate::rpc::RpcTransport for ScriptedRpc {
    fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, crate::rpc::RpcError> {
        self.seen.borrow_mut().push(body.clone());
        let method = body["method"].as_str().unwrap_or_default().to_string();
        let ans = self
            .answers
            .iter()
            .find(|(m, _)| *m == method)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| crate::rpc::RpcError::Transport(format!("no answer for {method}")))?;
        Ok(ans)
    }
}

/// A test explorer that answers one fixed status + body and records the URLs it was asked for.
struct FixedHttp {
    status: u16,
    body: String,
    urls: RefCell<Vec<String>>,
}

impl FixedHttp {
    fn new(status: u16, body: &str) -> Self {
        FixedHttp {
            status,
            body: body.to_string(),
            urls: RefCell::new(Vec::new()),
        }
    }
}

impl ExplorerHttp for FixedHttp {
    fn get(&self, url: &str) -> Result<(u16, String), String> {
        self.urls.borrow_mut().push(url.to_string());
        Ok((self.status, self.body.clone()))
    }
    fn post_json(&self, url: &str, _body: &serde_json::Value) -> Result<(u16, String), String> {
        self.urls.borrow_mut().push(url.to_string());
        Ok((self.status, self.body.clone()))
    }
}

// ---- addresses, calldata, targets ----

#[test]
fn addresses_are_normalized_and_junk_is_refused() {
    assert_eq!(
        normalize_address("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed").unwrap(),
        ADDR
    );
    assert_eq!(normalize_address(&format!("  {ADDR} ")).unwrap(), ADDR);
    for bad in [
        "",
        "0x",
        "5aaeb6053f3e94c9b9a09f33669435e7ef1beaed",
        "0x123",
        "0xzz",
        &format!("{ADDR}00"),
    ] {
        assert!(normalize_address(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn eip55_checksum_matches_the_reference_vectors() {
    // EIP-55 reference vectors.
    for want in [
        "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
        "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
        "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
        "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
    ] {
        let lower = normalize_address(want).unwrap();
        assert_eq!(checksum_address(&lower), want);
    }
}

#[test]
fn calldata_needs_a_selector_and_is_bounded() {
    assert_eq!(
        parse_calldata("0x06fdde03").unwrap(),
        vec![0x06, 0xfd, 0xde, 0x03]
    );
    assert!(parse_calldata("0x06fd").is_err(), "shorter than a selector");
    assert!(parse_calldata("").is_err());
    assert!(parse_calldata("0xnothex0").is_err());
    let huge = format!("0x{}", "00".repeat(MAX_CALLDATA_BYTES + 1));
    assert!(parse_calldata(&huge).is_err(), "over the size cap");
}

#[test]
fn read_target_is_chain_40204_or_a_loopback_fork_only() {
    assert_eq!(parse_target(None).unwrap(), ReadTarget::Citrate);
    assert_eq!(parse_target(Some("citrate")).unwrap(), ReadTarget::Citrate);
    assert_eq!(parse_target(Some("40204")).unwrap(), ReadTarget::Citrate);
    assert_eq!(
        parse_target(None).unwrap().rpc_url(),
        crate::rpc::CITRATE_RPC_URL
    );
    for ok in [
        "http://127.0.0.1:8545",
        "http://localhost:9545",
        "http://[::1]:8545",
    ] {
        let t = parse_target(Some(ok)).unwrap();
        assert_eq!(t, ReadTarget::Fork(ok.to_string()));
        assert_eq!(t.rpc_url(), ok);
    }
    for bad in [
        "https://127.0.0.1:8545",
        "http://example.com:8545",
        "http://10.0.0.5:8545",
        "http://127.0.0.1.example.com",
        "ftp://127.0.0.1",
        "http://user@127.0.0.1:8545",
        "not a url",
    ] {
        assert!(parse_target(Some(bad)).is_err(), "{bad:?} must be refused");
    }
}

// ---- explorer source ----

#[test]
fn a_full_match_reads_as_verified_with_its_abi_and_source() {
    let body = serde_json::json!({
        "address": "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
        "isContract": true,
        "codeSize": 4321,
        "verification": {
            "verified": true,
            "status": "verified",
            "matchType": "full",
            "contractName": "LemonDrops",
            "compilerVersion": "0.8.36",
            "source": "contract LemonDrops {}",
            "abi": [{"type": "function", "name": "name", "inputs": [], "outputs": [{"type": "string"}], "stateMutability": "view"}]
        }
    })
    .to_string();
    let v = parse_explorer_contract(&body).unwrap();
    assert_eq!(v.status, SourceStatus::Verified);
    assert!(v.is_contract);
    assert_eq!(v.code_size, Some(4321));
    assert_eq!(v.contract_name.as_deref(), Some("LemonDrops"));
    assert_eq!(v.source.as_deref(), Some("contract LemonDrops {}"));
    assert_eq!(
        v.abi.as_ref().and_then(|a| a.as_array()).map(Vec::len),
        Some(1)
    );
}

#[test]
fn a_partial_match_is_never_called_verified() {
    let body = serde_json::json!({
        "isContract": true,
        "verification": {"verified": false, "status": "partial", "matchType": "partial",
                         "abi": [], "source": "x", "note": "Partial match only"}
    })
    .to_string();
    let v = parse_explorer_contract(&body).unwrap();
    assert_eq!(v.status, SourceStatus::Partial);
    assert_eq!(v.note.as_deref(), Some("Partial match only"));
}

#[test]
fn unverified_and_eoa_are_reported_as_such() {
    let un = serde_json::json!({"isContract": true, "codeSize": 10,
        "verification": {"verified": false, "status": "unverified", "note": "Not verified."}})
    .to_string();
    let v = parse_explorer_contract(&un).unwrap();
    assert_eq!(v.status, SourceStatus::Unverified);
    assert!(v.abi.is_none());

    let eoa = serde_json::json!({"isContract": false, "codeSize": 0, "note": "Externally-owned account (EOA): no contract code."}).to_string();
    let v = parse_explorer_contract(&eoa).unwrap();
    assert_eq!(v.status, SourceStatus::NotContract);
    assert!(!v.is_contract);
}

#[test]
fn an_abi_that_is_not_an_array_is_dropped_not_trusted() {
    let body = serde_json::json!({"isContract": true,
        "verification": {"verified": true, "status": "verified", "matchType": "full", "abi": {"evil": true}}})
    .to_string();
    let v = parse_explorer_contract(&body).unwrap();
    assert!(v.abi.is_none());
}

#[test]
fn explorer_failures_are_honest() {
    let rl = FixedHttp::new(429, "{\"error\":\"rate limited\"}");
    let e = fetch_verified_source(&rl, ADDR).unwrap_err();
    assert!(e.contains("rate limit"), "{e}");
    let down = FixedHttp::new(502, "{\"error\":\"upstream\"}");
    let e = fetch_verified_source(&down, ADDR).unwrap_err();
    assert!(e.contains("502"), "{e}");
    let junk = FixedHttp::new(200, "<html>");
    assert!(fetch_verified_source(&junk, ADDR).is_err());
}

#[test]
fn source_lookups_go_to_the_pinned_explorer_contract_endpoint() {
    let ok = FixedHttp::new(200, "{\"isContract\":false}");
    fetch_verified_source(&ok, ADDR).unwrap();
    assert_eq!(
        ok.urls.borrow().as_slice(),
        [format!(
            "{}/api/contract/{ADDR}",
            crate::activity::EXPLORER_BASE
        )]
    );
}

// ---- reads ----

#[test]
fn a_view_call_is_an_eth_call_to_the_address_with_the_calldata() {
    let rpc = crate::rpc::RpcClient::with_transport(ScriptedRpc::new(vec![(
        "eth_call",
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": "0x00000000000000000000000000000000000000000000000000000000000001f4"}),
    )]));
    let out = view_call(&rpc, ADDR, &[0x18, 0x16, 0x0d, 0xdd]).unwrap();
    assert_eq!(
        out,
        "0x00000000000000000000000000000000000000000000000000000000000001f4"
    );
    let seen = rpc.transport().seen.borrow();
    assert_eq!(seen[0]["method"], "eth_call");
    assert_eq!(seen[0]["params"][0]["to"], ADDR);
    assert_eq!(seen[0]["params"][0]["data"], "0x18160ddd");
    assert!(
        seen[0]["params"][0].get("from").is_none(),
        "a read carries no sender"
    );
}

#[test]
fn a_reverting_view_call_reports_the_node_message() {
    let rpc = crate::rpc::RpcClient::with_transport(ScriptedRpc::new(vec![(
        "eth_call",
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "error": {"code": 3, "message": "execution reverted"}}),
    )]));
    let e = view_call(&rpc, ADDR, &[1, 2, 3, 4]).unwrap_err();
    assert!(e.contains("execution reverted"), "{e}");
}

#[test]
fn code_size_reads_eth_get_code() {
    let rpc = crate::rpc::RpcClient::with_transport(ScriptedRpc::new(vec![(
        "eth_getCode",
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": "0x6080604052"}),
    )]));
    assert_eq!(code_size(&rpc, ADDR).unwrap(), 5);
    let empty = crate::rpc::RpcClient::with_transport(ScriptedRpc::new(vec![(
        "eth_getCode",
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": "0x"}),
    )]));
    assert_eq!(code_size(&empty, ADDR).unwrap(), 0);
}

// ---- writes ----

#[test]
fn a_write_is_a_40204_transaction_intent_with_explicit_gas() {
    let intent = write_intent(
        "0x1111111111111111111111111111111111111111",
        ADDR,
        &[0xa0, 0x71, 0x2d, 0x68, 0, 0, 0, 1],
        5_000_000_000_000_000_000,
        123_456,
    );
    assert_eq!(intent.chain_id, 40204);
    assert_eq!(intent.origin, READER_ORIGIN);
    assert!(matches!(
        intent.kind,
        crate::ceremony::IntentKind::Transaction
    ));
    let v: serde_json::Value = serde_json::from_str(&intent.raw).unwrap();
    assert_eq!(v["to"], ADDR);
    assert_eq!(v["data"], "0xa0712d6800000001");
    assert_eq!(v["value"], "0x4563918244f40000");
    assert_eq!(v["gas"], "0x1e240");
    assert_eq!(v["chainId"], "0x9d0c");
    // The shared ceremony decoder parses it as a call to the address (Rule 3: what is shown is
    // what gets signed).
    let (parsed, _) = crate::txdecode::decode_transaction(&intent.raw).expect("decodes");
    assert!(parsed.to.is_some());
}

#[test]
fn gas_gets_headroom_and_is_capped() {
    assert_eq!(with_headroom(100_000), 120_000);
    assert_eq!(with_headroom(0), 21_000);
    assert_eq!(with_headroom(u64::MAX), MAX_WRITE_GAS);
}

#[test]
fn a_write_value_is_a_decimal_wei_string() {
    assert_eq!(parse_value_wei(None).unwrap(), 0);
    assert_eq!(parse_value_wei(Some("")).unwrap(), 0);
    assert_eq!(
        parse_value_wei(Some("5000000000000000000")).unwrap(),
        5_000_000_000_000_000_000
    );
    assert!(parse_value_wei(Some("0x10")).is_err());
    assert!(parse_value_wei(Some("-1")).is_err());
    assert!(parse_value_wei(Some("1.5")).is_err());
}

#[test]
fn a_write_gas_estimate_failure_is_a_refusal_not_a_guess() {
    let rpc = crate::rpc::RpcClient::with_transport(ScriptedRpc::new(vec![(
        "eth_estimateGas",
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "error": {"code": 3, "message": "execution reverted: MaxSupply"}}),
    )]));
    let e = estimate_write_gas(
        &rpc,
        "0x1111111111111111111111111111111111111111",
        ADDR,
        &[1, 2, 3, 4],
        0,
    )
    .unwrap_err();
    assert!(e.contains("MaxSupply"), "{e}");
    assert!(e.contains("not proposed"), "{e}");
}

#[test]
fn only_a_full_match_is_verified_even_if_the_flag_says_otherwise() {
    // A verified flag with a partial match type is still a partial match: the green state needs
    // both (the explorer's own badge rule, FWA-C12-05).
    let body = serde_json::json!({"isContract": true,
        "verification": {"verified": true, "status": "verified", "matchType": "partial", "abi": []}})
    .to_string();
    assert_eq!(
        parse_explorer_contract(&body).unwrap().status,
        SourceStatus::Partial
    );
    let no_flag = serde_json::json!({"isContract": true,
        "verification": {"verified": false, "matchType": "full", "abi": []}})
    .to_string();
    assert_eq!(
        parse_explorer_contract(&no_flag).unwrap().status,
        SourceStatus::Partial
    );
}
