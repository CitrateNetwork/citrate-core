// HUP-S1.5 — InferenceRouter route tests.
//
// Unit tests: calldata layout, return decoders (round trip and hostile inputs), quoting, the tx
// JSON the ceremony decodes, and the off-on-40204 behaviour. The read client runs over a recording
// test transport (a TEST transport, never wired as a default; Rule 1).
//
// `anvil_dry_run_*` (ignored by default) is the integration proof against the real contracts:
// `scripts/anvil-registry-dryrun.sh` starts anvil with chain id 40204, builds InferenceRouter and
// WrappedSALT from citrate-chain, and runs it with CITRATE_ANVIL_RPC + CITRATE_ANVIL_ARTIFACTS.

use super::*;

fn word_u128(w: &[u8]) -> Result<u128, String> {
    if w.len() != 32 || w[..16].iter().any(|b| *b != 0) {
        return Err("abi word: value too large".into());
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&w[16..]);
    Ok(u128::from_be_bytes(b))
}
use std::cell::RefCell;

fn w_u(n: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&n.to_be_bytes());
    w
}

fn w_addr(b: u8) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(&[b; 20]);
    w
}

fn tail_bytes(b: &[u8]) -> Vec<u8> {
    let mut v = w_u(b.len() as u128).to_vec();
    v.extend_from_slice(b);
    v.resize(32 + b.len().div_ceil(32) * 32, 0);
    v
}

fn get_request_return(status: u8, out: &[u8], price: u128) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&w_addr(0xaa));
    v.extend_from_slice(&[0x5a; 32]);
    v.extend_from_slice(&w_u(status as u128));
    v.extend_from_slice(&w_u(5 * 32));
    v.extend_from_slice(&w_u(price));
    v.extend_from_slice(&tail_bytes(out));
    v
}

fn provider_return(addr: u8, endpoint: &str, min_price: u128, load: u128, active: bool) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&w_addr(addr));
    v.extend_from_slice(&w_u(9 * 32));
    v.extend_from_slice(&w_u(100 * 10u128.pow(18)));
    v.extend_from_slice(&w_u(min_price));
    v.extend_from_slice(&w_u(10));
    v.extend_from_slice(&w_u(load));
    v.extend_from_slice(&w_u(7));
    v.extend_from_slice(&w_u(10_000));
    v.extend_from_slice(&w_u(active as u128));
    v.extend_from_slice(&tail_bytes(endpoint.as_bytes()));
    v
}

fn array_return(words: &[[u8; 32]]) -> Vec<u8> {
    let mut v = w_u(32).to_vec();
    v.extend_from_slice(&w_u(words.len() as u128));
    for w in words {
        v.extend_from_slice(w);
    }
    v
}

#[test]
fn request_inference_calldata_is_the_canonical_abi_encoding() {
    let hash = [0x5a; 32];
    let d = encode_request_inference(&hash, b"plan the migration", 5_000);
    assert_eq!(&d[..4], &selector(REQUEST_INFERENCE_SIG));
    assert_eq!(&d[4..36], &hash);
    assert_eq!(&d[36..68], &w_u(0x60));
    assert_eq!(&d[68..100], &w_u(5_000));
    assert_eq!(&d[100..132], &w_u(18));
    assert_eq!(&d[132..150], b"plan the migration");
    assert_eq!(d.len(), 4 + 32 * 5, "18 bytes pad to one word");
    assert!(d[150..].iter().all(|b| *b == 0));
    // Exactly one word of input: no extra padding word.
    assert_eq!(
        encode_request_inference(&hash, &[1; 32], 1).len(),
        4 + 32 * 5
    );
    assert_eq!(encode_request_inference(&hash, &[], 1).len(), 4 + 32 * 4);
}

#[test]
fn the_selector_matches_the_deployed_abi() {
    // keccak256("requestInference(bytes32,bytes,uint256)")[..4], checked with `cast sig`.
    assert_eq!(hex::encode(selector(REQUEST_INFERENCE_SIG)), "72ae2f28");
}

#[test]
fn the_ceremony_decodes_the_request_tx_as_a_legible_public_input_call() {
    let d = encode_request_inference(&[0x5a; 32], b"hello", 7_000);
    let raw = request_tx_json(
        "0x2222222222222222222222222222222222222222",
        "0x1111111111111111111111111111111111111111",
        &d,
        7_000,
        250_000,
    );
    let (tx, disp) = crate::txdecode::decode_transaction(&raw).unwrap_or_else(|| panic!("decodes"));
    assert_eq!(tx.value, 7_000);
    assert_eq!(tx.gas_limit, Some(250_000));
    assert!(disp.action.contains("publicly on chain"), "{}", disp.action);
    assert!(disp.action.contains("5 bytes of input"), "{}", disp.action);
}

#[test]
fn get_request_round_trips_and_rejects_hostile_returns() {
    let r = decode_get_request(&get_request_return(2, b"the answer", 1_000))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.status, RouterStatus::Completed);
    assert_eq!(r.output, b"the answer");
    assert_eq!(r.price_paid_wei, "1000");
    assert_eq!(r.requester, format!("0x{}", "aa".repeat(20)));
    assert_eq!(r.model_hash, format!("0x{}", "5a".repeat(32)));
    // Every status value maps; 5 is not a status.
    for (n, st) in [
        (0, RouterStatus::Pending),
        (1, RouterStatus::Processing),
        (3, RouterStatus::Failed),
        (4, RouterStatus::Cancelled),
    ] {
        assert_eq!(
            decode_get_request(&get_request_return(n, b"", 0)).map(|r| r.status),
            Ok(st)
        );
    }
    assert!(decode_get_request(&get_request_return(5, b"", 0)).is_err());
    // Short, offset past the end, length past the end, huge offset (no panic, no wrap).
    assert!(decode_get_request(&[0u8; 64]).is_err());
    let mut bad = get_request_return(2, b"x", 1);
    bad[96..128].copy_from_slice(&w_u(10_000));
    assert!(decode_get_request(&bad).is_err());
    let mut bad = get_request_return(2, b"x", 1);
    bad[160..192].copy_from_slice(&w_u(10_000));
    assert!(decode_get_request(&bad).is_err());
    let mut bad = get_request_return(2, b"x", 1);
    bad[96..128].copy_from_slice(&[0xff; 32]);
    assert!(decode_get_request(&bad).is_err());
    // An address word with high bytes set is not an address.
    let mut bad = get_request_return(2, b"x", 1);
    bad[0] = 1;
    assert!(decode_get_request(&bad).is_err());
}

#[test]
fn a_price_above_u128_still_decodes_as_a_decimal() {
    let mut ret = get_request_return(2, b"", 0);
    ret[128..160].copy_from_slice(&[0xff; 32]);
    let r = decode_get_request(&ret).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        r.price_paid_wei,
        "115792089237316195423570985008687907853269984665640564039457584007913129639935"
    );
}

#[test]
fn provider_getter_round_trips_and_checks_its_bool() {
    let p = decode_provider(&provider_return(0xbb, "https://p.example", 10, 1, true))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.address, format!("0x{}", "bb".repeat(20)));
    assert_eq!(p.endpoint, "https://p.example");
    assert_eq!(p.min_price_wei, "10");
    assert_eq!(p.stake_wei, "100000000000000000000");
    assert_eq!(
        (
            p.max_concurrent,
            p.current_load,
            p.total_inferences,
            p.success_bps
        ),
        (10, 1, 7, 10_000)
    );
    assert!(p.active);
    let mut bad = provider_return(0xbb, "e", 10, 1, true);
    bad[8 * 32..9 * 32].copy_from_slice(&w_u(2));
    assert!(decode_provider(&bad).is_err());
    assert!(decode_provider(&[0u8; 32 * 8]).is_err());
}

#[test]
fn eligibility_follows_the_contracts_select_provider_rule() {
    let p = |min: u128, load: u128, active: bool| {
        decode_provider(&provider_return(1, "e", min, load, active))
            .unwrap_or_else(|e| panic!("{e}"))
    };
    assert!(p(10, 0, true).eligible_at(10));
    assert!(
        !p(11, 0, true).eligible_at(10),
        "minPrice above the ceiling"
    );
    assert!(!p(10, 10, true).eligible_at(10), "no free slot");
    assert!(!p(10, 0, false).eligible_at(10), "inactive");
}

#[test]
fn quotes_name_the_ceiling_in_salt_and_refuse_unserviceable_requests() {
    let hash = [0x5a; 32];
    let providers = vec![
        decode_provider(&provider_return(1, "a", 3_000, 0, true)).unwrap_or_else(|e| panic!("{e}")),
        decode_provider(&provider_return(2, "b", 2_000, 0, true)).unwrap_or_else(|e| panic!("{e}")),
        decode_provider(&provider_return(3, "c", 1, 0, false)).unwrap_or_else(|e| panic!("{e}")),
    ];
    let q = quote_registry(
        "0xAB00000000000000000000000000000000000001",
        &hash,
        &providers,
        "hi",
        1_500_000_000_000_000_000,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(q.max_price_salt, "1.5");
    assert_eq!(q.eligible_providers, 2);
    assert_eq!(q.cheapest_min_price_wei.as_deref(), Some("2000"));
    assert!(q.input_is_public);
    assert_eq!(q.router, "0xab00000000000000000000000000000000000001");
    assert!(
        quote_registry("0x01", &hash, &providers, "hi", 1_000).is_err(),
        "no provider at 1000 wei"
    );
    assert!(quote_registry("0x01", &hash, &providers, " ", 10_000).is_err());
    assert!(quote_registry("0x01", &hash, &providers, "hi", 0).is_err());
    assert!(quote_registry("0x01", &hash, &providers, "hi", MAX_REGISTRY_PRICE_WEI + 1).is_err());
    let long = "x".repeat(MAX_REGISTRY_INPUT_BYTES + 1);
    assert!(quote_registry("0x01", &hash, &providers, &long, 10_000).is_err());
}

#[test]
fn model_hashes_parse_strictly() {
    assert_eq!(
        parse_model_hash(&format!("0x{}", "5a".repeat(32))),
        Ok([0x5a; 32])
    );
    assert!(parse_model_hash(&"5a".repeat(32)).is_err());
    assert!(parse_model_hash(&format!("0x{}", "5a".repeat(31))).is_err());
    assert!(parse_model_hash(&format!("0x{}", "zz".repeat(32))).is_err());
}

#[test]
fn result_view_fences_size_and_encoding() {
    let r = RouterRequest {
        requester: "0x".into(),
        model_hash: "0x".into(),
        status: RouterStatus::Completed,
        output: vec![b'a'; MAX_OUTPUT_BYTES + 5],
        price_paid_wei: "1".into(),
    };
    let v = result_view(3, &r);
    assert!(v.output_truncated);
    assert_eq!(v.output.as_deref().map(str::len), Some(MAX_OUTPUT_BYTES));
    let v = result_view(
        3,
        &RouterRequest {
            output: vec![0xff, 0x00],
            ..r.clone()
        },
    );
    assert!(v.output_is_hex);
    assert_eq!(v.output.as_deref(), Some("0xff00"));
    let v = result_view(
        3,
        &RouterRequest {
            output: vec![],
            status: RouterStatus::Processing,
            ..r
        },
    );
    assert_eq!(v.output, None);
}

/// A recording test transport answering `eth_call` by selector.
struct ByCall {
    answers: Vec<([u8; 4], Vec<u8>)>,
    seen: RefCell<Vec<serde_json::Value>>,
}

impl crate::rpc::RpcTransport for ByCall {
    fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, crate::rpc::RpcError> {
        self.seen.borrow_mut().push(body.clone());
        let data = body["params"][0]["data"].as_str().unwrap_or("");
        let bytes = hex::decode(data.trim_start_matches("0x")).unwrap_or_default();
        let ret = self
            .answers
            .iter()
            .find(|(sel, _)| bytes.len() >= 4 && bytes[..4] == sel[..])
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        Ok(
            serde_json::json!({"jsonrpc":"2.0","id":body["id"],"result": format!("0x{}", hex::encode(ret))}),
        )
    }
}

#[test]
fn the_reader_reads_a_models_route_from_the_router_it_was_given() {
    let t = ByCall {
        answers: vec![
            (selector(GET_PROVIDERS_SIG), array_return(&[w_addr(0xbb)])),
            (
                selector(PROVIDERS_SIG),
                provider_return(0xbb, "https://p.example", 10, 0, true),
            ),
            (
                selector(GET_USER_REQUESTS_SIG),
                array_return(&[w_u(0), w_u(4)]),
            ),
            (selector(REFUND_OWED_SIG), w_u(42).to_vec()),
            (selector(GET_REQUEST_SIG), get_request_return(1, b"", 0)),
        ],
        seen: RefCell::new(Vec::new()),
    };
    let rpc = RpcClient::with_transport(t);
    let r = RouterReader::new(&rpc, "0xAB00000000000000000000000000000000000001")
        .unwrap_or_else(|e| panic!("{e}"));
    let ps = r.providers(&[0x5a; 32]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].endpoint, "https://p.example");
    assert_eq!(
        r.user_requests("0x2222222222222222222222222222222222222222"),
        Ok(vec![0, 4])
    );
    assert_eq!(
        r.refund_owed("0x2222222222222222222222222222222222222222"),
        Ok("42".into())
    );
    assert_eq!(r.request(4).map(|q| q.status), Ok(RouterStatus::Processing));
    let seen = rpc.transport().seen.borrow();
    assert!(seen.iter().all(|b| b["method"] == "eth_call"));
    assert!(seen
        .iter()
        .all(|b| b["params"][0]["to"] == "0xab00000000000000000000000000000000000001"));
    // getRequest carries the id as one word.
    let last = seen
        .last()
        .map(|b| b["params"][0]["data"].as_str().unwrap_or("").to_string());
    assert_eq!(
        last,
        Some(format!(
            "0x{}{}",
            hex::encode(selector(GET_REQUEST_SIG)),
            hex::encode(w_u(4))
        ))
    );
    assert!(RouterReader::new(&rpc, "not-an-address").is_err());
}

#[test]
fn the_registry_commands_refuse_on_40204_while_no_router_is_pinned() {
    assert!(crate::addresses::inference_router().is_none());
    let hash = format!("0x{}", "5a".repeat(32));
    assert_eq!(
        tauri::async_runtime::block_on(escalation_registry_quote(hash, "hi".into(), "1000".into()))
            .err()
            .as_deref(),
        Some(ROUTE_OFF)
    );
    assert_eq!(
        tauri::async_runtime::block_on(escalation_registry_result(0))
            .err()
            .as_deref(),
        Some(ROUTE_OFF)
    );
}

// ---------------------------------------------------------------------------------------------
// Anvil dry run (ignored by default; scripts/anvil-registry-dryrun.sh runs it)
// ---------------------------------------------------------------------------------------------

struct Anvil {
    t: crate::rpc::HttpTransport,
    artifacts: std::path::PathBuf,
}

impl Anvil {
    fn from_env() -> Option<Anvil> {
        let url = std::env::var("CITRATE_ANVIL_RPC").ok()?;
        let artifacts = std::env::var("CITRATE_ANVIL_ARTIFACTS").ok()?;
        Some(Anvil {
            t: crate::rpc::HttpTransport::new(url),
            artifacts: artifacts.into(),
        })
    }

    fn rpc(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        use crate::rpc::RpcTransport as _;
        let resp = self
            .t
            .call(serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .map_err(|e| e.to_string())?;
        if let Some(e) = resp.get("error") {
            return Err(e.to_string());
        }
        Ok(resp["result"].clone())
    }

    fn bytecode(&self, name: &str) -> Vec<u8> {
        let p = self
            .artifacts
            .join(format!("{name}.sol"))
            .join(format!("{name}.json"));
        let j: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&p).unwrap_or_else(|e| panic!("artifact {}: {e}", p.display())),
        )
        .unwrap_or_else(|e| panic!("artifact json: {e}"));
        let hexs = j["bytecode"]["object"].as_str().unwrap_or("");
        hex::decode(hexs.trim_start_matches("0x")).unwrap_or_else(|e| panic!("bytecode hex: {e}"))
    }

    /// Send from an unlocked anvil account; returns (status, contractAddress).
    fn send(&self, tx: serde_json::Value) -> Result<(u64, Option<String>), String> {
        let hash = self.rpc("eth_sendTransaction", serde_json::json!([tx]))?;
        // Poll: anvil automines, but the receipt can lag the send by a moment under load.
        let mut rc = serde_json::Value::Null;
        for _ in 0..100 {
            rc = self.rpc(
                "eth_getTransactionReceipt",
                serde_json::json!([hash.clone()]),
            )?;
            if !rc.is_null() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if rc.is_null() {
            return Err("no receipt".into());
        }
        let status = u64::from_str_radix(
            rc["status"]
                .as_str()
                .unwrap_or("0x0")
                .trim_start_matches("0x"),
            16,
        )
        .unwrap_or(0);
        Ok((status, rc["contractAddress"].as_str().map(str::to_string)))
    }

    fn call(&self, to: &str, data: &[u8]) -> Vec<u8> {
        let r = self
            .rpc("eth_call", serde_json::json!([{"to": to, "data": format!("0x{}", hex::encode(data))}, "latest"]))
            .unwrap_or_else(|e| panic!("eth_call: {e}"));
        hex::decode(r.as_str().unwrap_or("0x").trim_start_matches("0x")).unwrap_or_default()
    }
}

fn hexs(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

fn addr_word(a: &str) -> [u8; 32] {
    address_arg(a).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
#[ignore = "needs anvil + citrate-chain artifacts: run scripts/anvil-registry-dryrun.sh"]
fn anvil_dry_run_inference_router_and_wsalt_authorization() {
    use citrate_core_kit::eip712;
    use citrate_core_kit::web_budget::{
        build_x402_authorization, fresh_x402_nonce, X402Asset, X402Request,
    };
    let Some(a) = Anvil::from_env() else {
        panic!(
            "set CITRATE_ANVIL_RPC and CITRATE_ANVIL_ARTIFACTS (scripts/anvil-registry-dryrun.sh)"
        );
    };
    let chain_id = u64::from_str_radix(
        a.rpc("eth_chainId", serde_json::json!([]))
            .unwrap_or_default()
            .as_str()
            .unwrap_or("0x0")
            .trim_start_matches("0x"),
        16,
    )
    .unwrap_or(0);
    assert_eq!(chain_id, 40204, "start anvil with --chain-id 40204");
    let accts: Vec<String> = a
        .rpc("eth_accounts", serde_json::json!([]))
        .unwrap_or_else(|e| panic!("{e}"))
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert!(accts.len() >= 4);
    let (admin, provider, member, relayer) = (&accts[0], &accts[1], &accts[2], &accts[3]);

    // Deploy InferenceRouter(modelRegistry, admin) and WrappedSALT() from citrate-chain source.
    let mut init = a.bytecode("InferenceRouter");
    init.extend_from_slice(&addr_word(admin));
    init.extend_from_slice(&addr_word(admin));
    let (st, router) = a
        .send(serde_json::json!({"from": admin, "data": hexs(&init), "gas": "0x1c9c380"}))
        .unwrap_or_else(|e| panic!("deploy router: {e}"));
    assert_eq!(st, 1);
    let router = router.unwrap_or_else(|| panic!("router address"));
    let (st, wsalt) = a
        .send(serde_json::json!({"from": admin, "data": hexs(&a.bytecode("WrappedSALT")), "gas": "0x1c9c380"}))
        .unwrap_or_else(|e| panic!("deploy wsalt: {e}"));
    assert_eq!(st, 1);
    let wsalt = wsalt.unwrap_or_else(|| panic!("wsalt address"));

    // A provider registers for one model: registerProvider(string,uint256,bytes32[]) with 100 SALT.
    let model = [0x5a; 32];
    let min_price: u128 = 1_000_000_000_000_000; // 0.001 SALT
    let mut reg = selector("registerProvider(string,uint256,bytes32[])").to_vec();
    let endpoint = tail_bytes(b"https://provider.test");
    reg.extend_from_slice(&w_u(0x60));
    reg.extend_from_slice(&w_u(min_price));
    reg.extend_from_slice(&w_u(0x60 + endpoint.len() as u128));
    reg.extend_from_slice(&endpoint);
    reg.extend_from_slice(&w_u(1));
    reg.extend_from_slice(&model);
    let (st, _) = a
        .send(serde_json::json!({"from": provider, "to": router, "data": hexs(&reg), "value": format!("0x{:x}", 100u128 * 10u128.pow(18)), "gas": "0x7a120"}))
        .unwrap_or_else(|e| panic!("registerProvider: {e}"));
    assert_eq!(st, 1);

    // The member's escalation: quote from the live route, estimate gas, and send the exact tx
    // JSON the ceremony would carry (anvil's unlocked account stands in for the ceremony signer).
    let max: u128 = 10_000_000_000_000_000; // 0.01 SALT ceiling
    let rpc = RpcClient::with_transport(crate::rpc::HttpTransport::new(
        std::env::var("CITRATE_ANVIL_RPC").unwrap_or_default(),
    ));
    let reader = RouterReader::new(&rpc, &router).unwrap_or_else(|e| panic!("{e}"));
    let providers = reader
        .providers(&model)
        .unwrap_or_else(|e| panic!("providers: {e}"));
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].address, provider.to_ascii_lowercase());
    assert_eq!(providers[0].endpoint, "https://provider.test");
    assert_eq!(providers[0].min_price_wei, min_price.to_string());
    let q = quote_registry(&router, &model, &providers, "plan the migration", max)
        .unwrap_or_else(|e| panic!("quote: {e}"));
    assert_eq!(q.max_price_salt, "0.01");
    let calldata = encode_request_inference(&model, b"plan the migration", max);
    let gas = estimate_request_gas(&rpc, member, &router, &calldata, max)
        .unwrap_or_else(|e| panic!("estimate: {e}"));
    let raw = request_tx_json(member, &router, &calldata, max, gas);
    let mut tx: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    if let Some(o) = tx.as_object_mut() {
        o.remove("chainId");
    }
    let (st, _) = a
        .send(tx)
        .unwrap_or_else(|e| panic!("requestInference: {e}"));
    assert_eq!(st, 1);
    let ids = reader
        .user_requests(member)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ids, vec![0]);
    let r = reader.request(0).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.status, RouterStatus::Processing);
    assert_eq!(r.requester, member.to_ascii_lowercase());

    // The provider completes; the member reads the answer, the price paid and the refund.
    let mut done = selector("completeInference(uint256,bytes)").to_vec();
    done.extend_from_slice(&w_u(0));
    done.extend_from_slice(&w_u(0x40));
    done.extend_from_slice(&tail_bytes(b"step 1: freeze writes"));
    let (st, _) = a
        .send(serde_json::json!({"from": provider, "to": router, "data": hexs(&done), "gas": "0x7a120"}))
        .unwrap_or_else(|e| panic!("completeInference: {e}"));
    assert_eq!(st, 1);
    let r = reader.request(0).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.status, RouterStatus::Completed);
    let v = result_view(0, &r);
    assert_eq!(v.output.as_deref(), Some("step 1: freeze writes"));
    assert_eq!(v.price_paid_wei, min_price.to_string());
    assert_eq!(
        reader.refund_owed(member),
        Ok((max - min_price).to_string())
    );
    let (st, _) = a
        .send(serde_json::json!({"from": member, "to": router, "data": hexs(&selector(CLAIM_REFUND_SIG)), "gas": "0x30d40"}))
        .unwrap_or_else(|e| panic!("claimRefund: {e}"));
    assert_eq!(st, 1);
    assert_eq!(reader.refund_owed(member), Ok("0".into()));

    // WrappedSALT: the kit's EIP-712 domain separator and type hash equal the contract's.
    let asset_addr: &'static str = Box::leak(wsalt.to_ascii_lowercase().into_boxed_str());
    let asset = X402Asset {
        chain_id: 40204,
        verifying_contract: asset_addr,
        name: "Wrapped SALT",
        version: "1",
    };
    let domain = asset.domain().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        a.call(&wsalt, &selector("DOMAIN_SEPARATOR()")),
        domain.separator().to_vec()
    );
    assert_eq!(
        a.call(&wsalt, &selector("TRANSFER_WITH_AUTHORIZATION_TYPEHASH()")),
        eip712::type_hash(eip712::TRANSFER_WITH_AUTHORIZATION_TYPE).to_vec()
    );

    // A fresh key that never pays gas authorizes a wSALT payment; a relayer submits it.
    let key = k256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng);
    let pubkey = k256::ecdsa::VerifyingKey::from(&key).to_encoded_point(false);
    let from = hexs(&eip712::keccak256(&pubkey.as_bytes()[1..])[12..]);
    let mut dep = selector("deposit()").to_vec();
    dep.truncate(4);
    let two = 2u128 * 10u128.pow(18);
    let (st, _) = a
        .send(serde_json::json!({"from": admin, "to": wsalt, "data": hexs(&dep), "value": format!("0x{two:x}"), "gas": "0x30d40"}))
        .unwrap_or_else(|e| panic!("deposit: {e}"));
    assert_eq!(st, 1);
    let amount: u128 = 1_500_000_000_000_000_000;
    let mut xfer = selector("transfer(address,uint256)").to_vec();
    xfer.extend_from_slice(&addr_word(&from));
    xfer.extend_from_slice(&w_u(amount));
    let (st, _) = a
        .send(
            serde_json::json!({"from": admin, "to": wsalt, "data": hexs(&xfer), "gas": "0x30d40"}),
        )
        .unwrap_or_else(|e| panic!("transfer: {e}"));
    assert_eq!(st, 1);

    let block = a
        .rpc("eth_getBlockByNumber", serde_json::json!(["latest", false]))
        .unwrap_or_default();
    let now = u64::from_str_radix(
        block["timestamp"]
            .as_str()
            .unwrap_or("0x0")
            .trim_start_matches("0x"),
        16,
    )
    .unwrap_or(0);
    let payee = provider.to_ascii_lowercase();
    let req = X402Request {
        recipient: payee.clone(),
        asset: asset_addr.to_string(),
        amount: amount.to_string(),
    };
    let auth = build_x402_authorization(&asset, &payee, &req, &from, now, 300, fresh_x402_nonce())
        .unwrap_or_else(|e| panic!("{e:?}"));
    let digest = auth.digest(&domain);
    let (sig, recid) = key
        .sign_prehash_recoverable(&digest)
        .unwrap_or_else(|e| panic!("sign: {e}"));
    let (v, r, s) = eip712::canonical_vrs(&sig, recid);
    let mut twa = selector("transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)").to_vec();
    twa.extend_from_slice(&eip712::address_word(&auth.from));
    twa.extend_from_slice(&eip712::address_word(&auth.to));
    twa.extend_from_slice(&auth.value);
    twa.extend_from_slice(&eip712::u64_word(auth.valid_after));
    twa.extend_from_slice(&eip712::u64_word(auth.valid_before));
    twa.extend_from_slice(&auth.nonce);
    twa.extend_from_slice(&eip712::u64_word(u64::from(v)));
    twa.extend_from_slice(&r);
    twa.extend_from_slice(&s);
    let mut bal = selector("balanceOf(address)").to_vec();
    bal.extend_from_slice(&addr_word(&payee));
    let before = a.call(&wsalt, &bal);
    let (st, _) = a
        .send(
            serde_json::json!({"from": relayer, "to": wsalt, "data": hexs(&twa), "gas": "0x30d40"}),
        )
        .unwrap_or_else(|e| panic!("transferWithAuthorization: {e}"));
    assert_eq!(
        st, 1,
        "the contract accepted the kit's digest and signature"
    );
    let after = a.call(&wsalt, &bal);
    let as_u = |v: &[u8]| word_u128(&v[..32]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(as_u(&after) - as_u(&before), amount);
    // The same authorization cannot be used twice.
    let replay = a.send(
        serde_json::json!({"from": relayer, "to": wsalt, "data": hexs(&twa), "gas": "0x30d40"}),
    );
    assert!(
        matches!(replay, Err(_) | Ok((0, _))),
        "replay must fail: {replay:?}"
    );
}

#[test]
fn the_claim_refund_tx_is_a_known_zero_value_call() {
    let raw = claim_refund_tx_json(
        "0x2222222222222222222222222222222222222222",
        "0x1111111111111111111111111111111111111111",
        CLAIM_REFUND_GAS,
    );
    let (tx, d) = crate::txdecode::decode_transaction(&raw).unwrap_or_else(|| panic!("decodes"));
    assert_eq!(tx.value, 0);
    assert_eq!(tx.data, selector(CLAIM_REFUND_SIG).to_vec());
    assert!(d.action.starts_with("Call claimRefund()"), "{}", d.action);
    assert_eq!(
        tauri::async_runtime::block_on(escalation_registry_result(1))
            .err()
            .as_deref(),
        Some(ROUTE_OFF)
    );
}
