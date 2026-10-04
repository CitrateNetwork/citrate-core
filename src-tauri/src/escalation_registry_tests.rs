// HUP-S1.5 — registry escalation (InferenceRouter + x402), core half.
//
// ABI goldens (data source): `cast call` against a local anvil deploy of citrate-chain
// `contracts/src/InferenceRouter.sol` at origin/main 0aab474b, with one provider registered by
// `registerProvider("http://127.0.0.1:18999/v1", 0.01 ether, [keccak("hermes-planner-test-model")])`
// from anvil's second default account, 100 SALT stake (2026-10-01).

use super::*;
use citrate_core_kit::ceremony::{SignatureCeremony, X402SignRequest};
use citrate_core_kit::custody::{CustodyError, CustodyVault, Keyring};
use citrate_core_kit::rpc::{HttpTransport, RpcError};
use std::cell::RefCell;
use std::collections::VecDeque as Queue;
use std::sync::Mutex as StdMutex;

const GET_PROVIDERS_RET: &str = "0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000100000000000000000000000070997970c51812dc3a010c7d01b50e0d17dc79c8";
const PROVIDERS_RET: &str = "00000000000000000000000070997970c51812dc3a010c7d01b50e0d17dc79c800000000000000000000000000000000000000000000000000000000000001200000000000000000000000000000000000000000000000056bc75e2d63100000000000000000000000000000000000000000000000000000002386f26fc10000000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000271000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000019687474703a2f2f3132372e302e302e313a31383939392f763100000000000000";
const PROVIDER: &str = "0x70997970c51812dc3a010c7d01b50e0d17dc79c8";
const ROUTER: &str = "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512";
const T0_MS: u64 = 1_790_000_000_000;

fn h(s: &str) -> Vec<u8> {
    hex::decode(s).expect("hex")
}

fn model_hash() -> String {
    format!("0x{}", "cd".repeat(32))
}

fn asset() -> RegistryAsset {
    RegistryAsset {
        domain: X402Domain::new(
            "Wrapped SALT",
            "1",
            31337,
            "0x5fbdb2315678afecb367f032d93f642f64180aa3",
        )
        .expect("domain"),
        symbol: "wSALT".into(),
        decimals: 18,
    }
}

fn route() -> RegistryRoute {
    RegistryRoute {
        router: ROUTER.into(),
        asset: asset(),
    }
}

fn provider(addr_byte: &str, price: u128) -> RouterProvider {
    RouterProvider {
        address: format!("0x{}", addr_byte.repeat(20)),
        endpoint: "https://provider.example/v1".into(),
        stake: 100,
        min_price: price,
        max_concurrent: 10,
        current_load: 0,
        total_inferences: 0,
        success_rate: 10_000,
        is_active: true,
    }
}

// --- selectors and ABI --------------------------------------------------------------------------

fn selector(sig: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let d = Keccak256::digest(sig.as_bytes());
    [d[0], d[1], d[2], d[3]]
}

#[test]
fn selectors_match_the_contract_signatures() {
    assert_eq!(SEL_GET_PROVIDERS, selector("getProviders(bytes32)"));
    assert_eq!(SEL_PROVIDERS, selector("providers(address)"));
    assert_eq!(
        SEL_AUTHORIZATION_STATE,
        selector("authorizationState(address,bytes32)")
    );
}

#[test]
fn decodes_the_routers_real_provider_list_and_struct() {
    assert_eq!(
        decode_address_array(&h(GET_PROVIDERS_RET)).expect("list"),
        vec![PROVIDER.to_string()]
    );
    let p = decode_provider(&h(PROVIDERS_RET)).expect("provider");
    assert_eq!(p.address, PROVIDER);
    assert_eq!(p.endpoint, "http://127.0.0.1:18999/v1");
    assert_eq!(p.stake, 100_000_000_000_000_000_000);
    assert_eq!(p.min_price, 10_000_000_000_000_000);
    assert_eq!(p.max_concurrent, 10);
    assert_eq!(p.current_load, 0);
    assert_eq!(p.success_rate, 10_000);
    assert!(p.is_active);
}

#[test]
fn malformed_router_answers_are_refused_not_guessed() {
    let full = h(PROVIDERS_RET);
    assert!(decode_provider(&full[..full.len() - 40]).is_err(), "truncated endpoint");
    assert!(decode_provider(&full[..64]).is_err(), "short head");
    let mut bad_bool = full.clone();
    bad_bool[8 * 32 + 31] = 2;
    assert!(decode_provider(&bad_bool).is_err(), "bool out of range");
    let mut dirty_addr = full.clone();
    dirty_addr[0] = 1;
    assert!(decode_provider(&dirty_addr).is_err(), "address with high bits");
    let mut long = full;
    long[9 * 32 + 30] = 0x10; // endpoint length 0x1019 > the cap
    assert!(decode_provider(&long).is_err(), "endpoint over the cap");
    assert!(decode_address_array(&h("00")).is_err());
}

// --- a scripted transport -------------------------------------------------------------------------

struct Scripted {
    requests: RefCell<Vec<serde_json::Value>>,
    results: RefCell<Queue<String>>,
}

impl Scripted {
    fn new(results: &[&str]) -> Self {
        Scripted {
            requests: RefCell::new(Vec::new()),
            results: RefCell::new(results.iter().map(|r| format!("0x{r}")).collect()),
        }
    }
}

impl RpcTransport for Scripted {
    fn call(&self, body: serde_json::Value) -> std::result::Result<serde_json::Value, RpcError> {
        self.requests.borrow_mut().push(body);
        let r = self
            .results
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| RpcError::Transport("no scripted result".into()))?;
        Ok(serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": r}))
    }
}

#[test]
fn read_providers_calls_the_router_with_the_exact_calldata() {
    let rpc = RpcClient::with_transport(Scripted::new(&[GET_PROVIDERS_RET, PROVIDERS_RET]));
    let mh = parse_model_hash(&model_hash()).expect("hash");
    let ps = read_providers(&rpc, ROUTER, &mh).expect("providers");
    assert_eq!(ps.len(), 1);
    let reqs = rpc.transport().requests.borrow();
    assert_eq!(reqs[0]["method"], "eth_call");
    assert_eq!(reqs[0]["params"][0]["to"], ROUTER);
    assert_eq!(
        reqs[0]["params"][0]["data"],
        format!("0x331795aa{}", "cd".repeat(32))
    );
    assert_eq!(
        reqs[1]["params"][0]["data"],
        format!("0x0787bc27{:0>64}", PROVIDER.trim_start_matches("0x"))
    );
}

#[test]
fn authorization_state_is_read_as_a_bool() {
    let one = format!("{:064x}", 1);
    let zero = format!("{:064x}", 0);
    let rpc = RpcClient::with_transport(Scripted::new(&[&one, &zero]));
    let nonce = format!("0x{}", "01".repeat(32));
    let payer = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
    assert_eq!(authorization_settled(&rpc, ROUTER, payer, &nonce), Ok(true));
    assert_eq!(authorization_settled(&rpc, ROUTER, payer, &nonce), Ok(false));
    let reqs = rpc.transport().requests.borrow();
    assert_eq!(
        reqs[0]["params"][0]["data"],
        format!(
            "0xe94a0102{:0>64}{}",
            payer.trim_start_matches("0x"),
            "01".repeat(32)
        )
    );
}

// --- selection --------------------------------------------------------------------------------------

#[test]
fn selection_takes_the_cheapest_usable_provider() {
    let cheap = provider("11", 5);
    let dear = provider("22", 9);
    let mut inactive = provider("33", 1);
    inactive.is_active = false;
    let mut full = provider("44", 1);
    full.current_load = 10;
    let mut free = provider("55", 0);
    free.endpoint = "https://free.example/v1".into();
    let mut plain_http = provider("66", 1);
    plain_http.endpoint = "http://remote.example/v1".into();
    let mut over = provider("77", 2);
    over.min_price = MAX_PRICE_BASE_UNITS + 1;
    let all = vec![dear.clone(), inactive, full, free, plain_http, over, cheap.clone()];
    assert_eq!(select_provider(&all, MAX_PRICE_BASE_UNITS), Some(cheap));
    assert_eq!(select_provider(&[dear.clone()], 8), None, "over the ceiling");
    assert_eq!(select_provider(&[dear.clone()], 9), Some(dear));
}

#[test]
fn ties_prefer_the_more_reliable_then_less_loaded_provider() {
    let mut a = provider("11", 5);
    a.success_rate = 9_000;
    let b = provider("22", 5);
    assert_eq!(select_provider(&[a.clone(), b.clone()], 10), Some(b.clone()));
    let mut c = provider("33", 5);
    c.current_load = 3;
    assert_eq!(select_provider(&[c, b.clone()], 10), Some(b));
}

// --- route configuration ----------------------------------------------------------------------------

#[test]
fn the_route_is_off_and_names_both_missing_pieces() {
    let off = route_from(None, &[], 40204).expect_err("off");
    assert_eq!(off.len(), 2);
    assert!(off[0].contains("InferenceRouter"));
    assert!(off[1].contains("O-1"));
    let v = status_view(&Err(off));
    assert!(!v.enabled);
    assert!(v.reason.contains("not available yet"));
}

#[test]
fn the_route_needs_an_asset_on_the_same_chain() {
    let a = citrate_core_kit::web_budget::X402Asset {
        chain_id: 31337,
        verifying_contract: "0x5fbdb2315678afecb367f032d93f642f64180aa3",
        name: "Wrapped SALT",
        version: "1",
    };
    let other_chain = route_from(Some(ROUTER), std::slice::from_ref(&a), 40204).expect_err("off");
    assert_eq!(other_chain.len(), 1);
    let on = route_from(Some(ROUTER), &[a], 31337).expect("on");
    assert_eq!(on.router, ROUTER);
    assert_eq!(on.network(), "eip155:31337");
    let v = status_view(&Ok(on));
    assert!(v.enabled);
    assert!(v.missing.is_empty());
}

#[test]
fn on_40204_today_the_route_is_off_so_members_see_no_change() {
    // Defaults change nothing: no router pin in the core address book and an empty allowlist.
    let r = pinned_route();
    assert!(r.is_err());
    assert!(citrate_core_kit::web_budget::X402_ASSET_ALLOWLIST.is_empty());
}

// --- quotes, approval, runs ----------------------------------------------------------------------

const CANONICAL_ADDRESS: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";

#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<std::collections::HashMap<String, Vec<u8>>>,
}

impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account);
        Ok(())
    }
}

/// A vault holding the wallet for `mnemonic` (built at runtime from public test vectors).
fn vault_with(mnemonic: &str) -> CustodyVault {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-registry-test-{}-{}.enc",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p, 0);
    let pass = || b"registry escalation test passphrase".to_vec();
    v.init(&mut pass()).expect("init");
    v.unlock(&mut pass()).expect("unlock");
    citrate_core_kit::wallet::import(&v, mnemonic).expect("import");
    v
}

/// The canonical BIP44 vector ("abandon" x11 + "about"), assembled at runtime.
fn canonical_mnemonic() -> String {
    let mut w = vec!["abandon"; 11];
    w.push("about");
    w.join(" ")
}

fn quoted_book(price: u128) -> (RegistryBook, RegistryQuoteView) {
    let mut b = RegistryBook::default();
    let mut p = provider("70", price);
    p.address = PROVIDER.into();
    let q = b
        .quote(&route(), &model_hash(), &p, "Plan it.", None, 64, T0_MS, "rq-1".into())
        .expect("quote");
    (b, q)
}

/// Request the ceremony for an authorization and approve it, as the member would.
fn approve(auth: &X402Authorization, vault: &CustodyVault) -> String {
    let c = SignatureCeremony::new();
    let view = c
        .request_x402(X402SignRequest {
            origin: "Hermes (registry escalation)".into(),
            domain: route().asset.domain,
            authorization: auth.clone(),
            resource: "test".into(),
            asset_symbol: "wSALT".into(),
            asset_decimals: 18,
        })
        .expect("request");
    c.approve(vault, &view.id, false).expect("approve").sig_hex
}

#[test]
fn a_quote_names_provider_host_and_price() {
    let (_, q) = quoted_book(10_000_000_000_000_000);
    assert_eq!(q.provider, PROVIDER);
    assert_eq!(q.provider_host, "provider.example");
    assert_eq!(q.price_base_units, "10000000000000000");
    assert_eq!(q.price_label, "0.01 wSALT");
    assert_eq!(q.expires_at_ms, T0_MS + QUOTE_TTL_MS);
}

#[test]
fn a_quote_refuses_bad_prompts_and_token_counts() {
    let mut b = RegistryBook::default();
    let p = provider("70", 1);
    let r = route();
    assert!(b.quote(&r, &model_hash(), &p, " ", None, 64, T0_MS, "a".into()).is_err());
    assert!(b.quote(&r, &model_hash(), &p, "x", None, 0, T0_MS, "b".into()).is_err());
    assert!(b.quote(&r, &model_hash(), &p, "x", None, 8193, T0_MS, "c".into()).is_err());
    let big = "x".repeat(MAX_PROMPT_BYTES + 1);
    assert!(b.quote(&r, &model_hash(), &p, &big, None, 64, T0_MS, "d".into()).is_err());
}

#[test]
fn authorizing_needs_the_shown_price_and_a_live_quote() {
    let (mut b, q) = quoted_book(10_000_000_000_000_000);
    let n = || format!("0x{}", "01".repeat(32));
    assert_eq!(
        b.authorize(&route(), &q.quote_id, "1", CANONICAL_ADDRESS, T0_MS, n()),
        Err(RegError::PriceNotShown {
            quoted: "10000000000000000".into(),
            shown: "1".into()
        })
    );
    let auth = b
        .authorize(&route(), &q.quote_id, &q.price_base_units, CANONICAL_ADDRESS, T0_MS, n())
        .expect("auth");
    assert_eq!(auth.to, PROVIDER);
    assert_eq!(auth.value, "10000000000000000");
    assert_eq!(auth.valid_before, T0_MS / 1000 + AUTHORIZATION_VALIDITY_SECS);
    // One authorization per quote.
    assert_eq!(
        b.authorize(&route(), &q.quote_id, &q.price_base_units, CANONICAL_ADDRESS, T0_MS, n()),
        Err(RegError::UnknownQuote)
    );
    let (mut b, q) = quoted_book(1);
    assert_eq!(
        b.authorize(&route(), &q.quote_id, "1", CANONICAL_ADDRESS, T0_MS + QUOTE_TTL_MS, n()),
        Err(RegError::QuoteExpired)
    );
}

#[test]
fn a_run_needs_the_members_signature_over_exactly_that_authorization() {
    let vault = vault_with(&canonical_mnemonic());
    let (mut b, q) = quoted_book(10_000_000_000_000_000);
    let auth = b
        .authorize(
            &route(),
            &q.quote_id,
            &q.price_base_units,
            CANONICAL_ADDRESS,
            T0_MS,
            x402::fresh_nonce(),
        )
        .expect("auth");
    // A signature over a different authorization (another nonce) is refused, and the quote stays.
    let mut other = auth.clone();
    other.nonce = format!("0x{}", "02".repeat(32));
    let wrong = approve(&other, &vault);
    assert_eq!(
        b.take_approved(&q.quote_id, &wrong, T0_MS).map(|_| ()),
        Err(RegError::Signature)
    );
    let sig = approve(&auth, &vault);
    let a = b.take_approved(&q.quote_id, &sig, T0_MS).expect("approved");
    assert_eq!(a.authorization, auth);
    assert_eq!(a.endpoint, "https://provider.example/v1");
    // One run per quote.
    assert_eq!(
        b.take_approved(&q.quote_id, &sig, T0_MS).map(|_| ()),
        Err(RegError::UnknownQuote)
    );
}

#[test]
fn a_run_is_refused_when_the_authorization_is_about_to_expire() {
    let vault = vault_with(&canonical_mnemonic());
    let (mut b, q) = quoted_book(5);
    let auth = b
        .authorize(&route(), &q.quote_id, "5", CANONICAL_ADDRESS, T0_MS, x402::fresh_nonce())
        .expect("auth");
    let sig = approve(&auth, &vault);
    let late = (auth.valid_before - RUN_MARGIN_SECS) * 1000;
    assert_eq!(
        b.take_approved(&q.quote_id, &sig, late).map(|_| ()),
        Err(RegError::QuoteExpired)
    );
}

#[test]
fn a_quote_that_was_never_authorized_cannot_run() {
    let (mut b, q) = quoted_book(5);
    assert_eq!(
        b.take_approved(&q.quote_id, &format!("0x{}", "11".repeat(65)), T0_MS)
            .map(|_| ()),
        Err(RegError::UnknownQuote)
    );
}

#[test]
fn the_sidecar_body_matches_the_runtime_contract() {
    let vault = vault_with(&canonical_mnemonic());
    let (mut b, q) = quoted_book(5);
    let auth = b
        .authorize(&route(), &q.quote_id, "5", CANONICAL_ADDRESS, T0_MS, x402::fresh_nonce())
        .expect("auth");
    let sig = approve(&auth, &vault);
    let a = b.take_approved(&q.quote_id, &sig, T0_MS).expect("approved");
    let v: serde_json::Value = serde_json::from_str(&sidecar_body(&a, "esc-reg-x")).expect("json");
    let mut keys: Vec<String> = v.as_object().expect("obj").keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["baseUrl", "escalationId", "maxTokens", "model", "payment", "prompt", "system"]
    );
    let mut pk: Vec<String> = v["payment"].as_object().expect("obj").keys().cloned().collect();
    pk.sort();
    assert_eq!(
        pk,
        ["asset", "from", "network", "nonce", "signature", "to", "validAfter", "validBefore", "value"]
    );
    assert_eq!(v["payment"]["network"], "eip155:31337");
    assert_eq!(v["model"], model_hash());
    assert!(!v.to_string().contains("apiKey"), "no key on the registry route");
}

#[test]
fn sidecar_answers_are_interpreted_with_the_sent_flag() {
    let ok = serde_json::json!({"content": "plan", "receipt": {"success": true,
        "transaction": format!("0x{}", "12".repeat(32))}})
    .to_string();
    let (content, receipt) = interpret_sidecar(200, &ok).expect("ok");
    assert_eq!(content, "plan");
    assert!(receipt.expect("receipt").success);
    assert_eq!(
        interpret_sidecar(422, r#"{"error":"expired","sent":false}"#),
        Err((false, "expired".into()))
    );
    assert_eq!(
        interpret_sidecar(502, "not json"),
        Err((true, "the registry escalation failed".into()))
    );
    assert!(interpret_sidecar(200, "{}").is_err());
}

#[test]
fn history_round_trips_and_an_unreadable_file_is_kept_aside() {
    let dir = std::env::temp_dir().join(format!("citrate-registry-history-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let rec = RegistryRunRecord {
        escalation_id: "esc-reg-1".into(),
        quote_id: "rq-1".into(),
        at_ms: T0_MS,
        model_hash: model_hash(),
        provider: PROVIDER.into(),
        asset: route().asset.domain.verifying_contract,
        value_base_units: "5".into(),
        payer: CANONICAL_ADDRESS.into(),
        nonce: format!("0x{}", "01".repeat(32)),
        answered: true,
        sent: true,
        claimed_transaction: None,
        settled_on_chain: Some(true),
    };
    let mut b = RegistryBook::default();
    b.record(rec.clone());
    save_history(&dir, &b.history).expect("save");
    assert_eq!(load_history(&dir, T0_MS), Queue::from(vec![rec]));
    std::fs::write(dir.join(HISTORY_FILE), b"garbage").expect("write");
    assert!(load_history(&dir, T0_MS).is_empty());
    assert!(dir
        .join(format!("registry-history.unreadable-{T0_MS}.json"))
        .exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- local chain end to end ---------------------------------------------------------------------
//
// scripts/escalation-registry-anvil-e2e.sh deploys WrappedSALT + InferenceRouter from citrate-chain
// on anvil, registers a provider, wraps SALT for anvil account 0, then runs:
//   1. anvil_e2e_registry_quote_and_approve (here): read the router, quote, build, approve, verify,
//      and write the sidecar body;
//   2. the runtime's anvil_e2e_registry_payment_settles_on_chain: send it to a provider that
//      settles on chain and answers with the receipt;
//   3. anvil_e2e_registry_settlement_is_confirmed (here): core's own on-chain check.

fn e2e_env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set (run the e2e script)"))
}

fn anvil_mnemonic() -> String {
    let mut w = vec!["test"; 11];
    w.push("junk");
    w.join(" ")
}

#[test]
#[ignore = "needs a local anvil chain: run scripts/escalation-registry-anvil-e2e.sh"]
fn anvil_e2e_registry_quote_and_approve() {
    let rpc = RpcClient::with_transport(HttpTransport::new(e2e_env("CITRATE_X402_E2E_RPC")));
    let chain_id: u64 = e2e_env("CITRATE_X402_E2E_CHAIN_ID").parse().expect("chain id");
    let asset_addr = e2e_env("CITRATE_X402_E2E_ASSET");
    let a = citrate_core_kit::web_budget::X402Asset {
        chain_id,
        verifying_contract: Box::leak(asset_addr.into_boxed_str()),
        name: "Wrapped SALT",
        version: "1",
    };
    let route = route_from(Some(&e2e_env("CITRATE_X402_E2E_ROUTER")), &[a], chain_id)
        .expect("route on");
    let mh_str = e2e_env("CITRATE_X402_E2E_MODEL_HASH");
    let mh = parse_model_hash(&mh_str).expect("model hash");
    let providers = read_providers(&rpc, &route.router, &mh).expect("router read");
    let p = select_provider(&providers, MAX_PRICE_BASE_UNITS).expect("a provider");
    let vault = vault_with(&anvil_mnemonic());
    let payer = citrate_core_kit::wallet::address(&vault).expect("addr").address;
    let mut b = RegistryBook::default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    let q = b
        .quote(&route, &mh_str, &p, "Plan the mint page.", None, 64, now, "rq-e2e".into())
        .expect("quote");
    let auth = b
        .authorize(&route, &q.quote_id, &q.price_base_units, &payer, now, x402::fresh_nonce())
        .expect("auth");
    let c = SignatureCeremony::new();
    let view = c
        .request_x402(X402SignRequest {
            origin: "Hermes (registry escalation)".into(),
            domain: route.asset.domain.clone(),
            authorization: auth,
            resource: format!("model {} via {}", q.model_hash, q.provider_host),
            asset_symbol: route.asset.symbol.clone(),
            asset_decimals: route.asset.decimals,
        })
        .expect("ceremony");
    let sig = c.approve(&vault, &view.id, false).expect("member approves").sig_hex;
    let approved = b.take_approved(&q.quote_id, &sig, now).expect("verified");
    std::fs::write(
        e2e_env("CITRATE_X402_E2E_REQUEST"),
        sidecar_body(&approved, "esc-reg-anvil"),
    )
    .expect("write request");
    println!(
        "quoted {} from {} at {}; approved ceremony {}",
        q.price_label, q.provider, q.provider_host, view.id
    );
}

#[test]
#[ignore = "needs a local anvil chain: run scripts/escalation-registry-anvil-e2e.sh"]
fn anvil_e2e_registry_settlement_is_confirmed() {
    let rpc = RpcClient::with_transport(HttpTransport::new(e2e_env("CITRATE_X402_E2E_RPC")));
    let body: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(e2e_env("CITRATE_X402_E2E_REQUEST")).expect("request file"),
    )
    .expect("json");
    let pay = &body["payment"];
    let s = |k: &str| pay[k].as_str().expect("field").to_string();
    assert_eq!(
        authorization_settled(&rpc, &s("asset"), &s("from"), &s("nonce")),
        Ok(true),
        "core sees the authorization consumed on chain"
    );
    let outcome: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(e2e_env("CITRATE_X402_E2E_OUTCOME")).expect("outcome file"),
    )
    .expect("json");
    let (content, receipt) = interpret_sidecar(200, &outcome.to_string()).expect("outcome");
    assert!(!content.is_empty());
    assert!(receipt.expect("receipt").success);
}
