// HUP-S6 g3-gate + g3-e2e (local half): the hello-mint run, from a gated build to a ceremony
// deploy, a local verify and a test mint, on a local anvil fork of 40204.
//
// Two kinds of test live here:
//
// 1. Always-on tests of `propose_deploy` (the one deploy body behind `contract_deploy`): no gate
//    record or a NOT READY record means a refusal and no ceremony; a READY record opens exactly
//    one contract-creation ceremony from the vault wallet; a later NOT READY rejects it. These
//    use hand-written tool outputs as fixtures.
//
// 2. `e2e_hello_mint_on_an_anvil_fork`, which only runs under `scripts/e2e-hello-mint.sh` (it
//    skips when `CITRATE_E2E_HM_DIR` is unset). The script runs the interview through the real
//    sidecar, renders the template, runs the real tools (forge test, slither, aderyn, medusa)
//    and the fork dry run, and writes their raw outputs as `GateInputs` JSON. This test then
//    evaluates them with the production gate, and when READY deploys through the real
//    SignatureCeremony (`approve_and_broadcast`, the same signer the app uses) against the
//    fork, verifies the deployed code against the compiled artifact, and mints one token
//    through a second ceremony. The injected-bug build must come back NOT READY and its deploy
//    must be refused with no ceremony opened.
//
// Nothing here touches chain 40204 itself: every transaction goes to a loopback anvil.

use super::*;
use crate::custody::{CustodyError, CustodyVault, Keyring};
use crate::deploy_gate::{
    evaluate, initcode_hash, CompilerSettings, ForkDryRunInput, GateInputs, GateItemId,
    GateStore, MedusaInput, PrecompileUse, ToolRun, Verdict,
};
use sha3::{Digest as _, Keccak256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

// ------------------------------------------------------------------------ test vault

#[derive(Default)]
struct MemKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
}

impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

fn temp_vault_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-hello-mint-{tag}-{}-{}.enc",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&p);
    p
}

/// A vault on an in-memory keyring, opened the way the app opens it (device passphrase), with
/// `mnemonic` imported, or a fresh wallet when `None`.
fn vault(tag: &str, mnemonic: Option<&str>) -> CustodyVault {
    let v = CustodyVault::new(Box::<MemKeyring>::default(), temp_vault_path(tag), 0);
    v.ensure_auto_unlocked().expect("vault opens");
    match mnemonic {
        Some(m) => {
            crate::wallet::import(&v, m).expect("import wallet");
        }
        None => {
            crate::wallet::create(&v).expect("create wallet");
        }
    }
    v
}

// ------------------------------------------------------------------------ fixtures

/// A small but real init code (`PUSH1 0 PUSH1 0 RETURN`-style creation stub + one byte).
fn stub_bytecode() -> String {
    "0x600a600c600039600a6000f3602a60005260206000f3".to_string()
}

fn compiler() -> CompilerSettings {
    CompilerSettings {
        solc_version: "0.8.36".into(),
        optimizer: true,
        optimizer_runs: 200,
        evm_version: "cancun".into(),
        via_ir: false,
    }
}

fn ran(output: &str) -> ToolRun {
    ToolRun::Ran {
        output: output.to_string(),
        duration_ms: 1,
        tool_version: Some("fixture".into()),
    }
}

/// Fixture outputs in each tool's report format; every item passes.
fn ready_inputs(bytecode_hex: &str) -> GateInputs {
    let forge = r#"{"test/T.t.sol:T":{"test_results":{"test_mint()":{"status":"Success"}}}}"#;
    let slither = r#"{"success":true,"error":null,"results":{"detectors":[]}}"#;
    let aderyn = r#"{"issue_count":{"high":0,"low":0},"high_issues":{"issues":[]}}"#;
    let medusa = "fuzz: elapsed: 3s, calls: 20000 (6000/sec)\nTest summary: 3 test(s) passed, 0 test(s) failed\n";
    let receipt = r#"{"status":"0x1","contractAddress":"0x5fbdb2315678afecb367f032d93f642f64180aa3","gasUsed":"0x5208"}"#;
    GateInputs {
        bytecode_hex: bytecode_hex.to_string(),
        constructor_args_hex: None,
        compiler: compiler(),
        forge_tests: ran(forge),
        slither: ran(slither),
        aderyn: ran(aderyn),
        medusa: MedusaInput {
            run: ran(medusa),
            call_budget: 10_000,
        },
        fork_dry_run: ForkDryRunInput {
            run: ran(receipt),
            tx_input_hex: bytecode_hex.to_string(),
            citrate_precompiles: PrecompileUse::None,
        },
    }
}

fn initcode_of(bytecode_hex: &str) -> Vec<u8> {
    hex::decode(bytecode_hex.trim_start_matches("0x")).expect("hex")
}

/// The id the ceremony will hand out next, probed by opening (and rejecting) one throwaway
/// ceremony. Ids are monotonic, so "no ceremony was opened" is "the probe id moved by one".
fn probe_next_id(ceremony: &crate::ceremony::SignatureCeremony) -> u64 {
    let view = ceremony.request(crate::ceremony::SignatureIntent {
        origin: "probe".into(),
        kind: crate::ceremony::IntentKind::PersonalSign,
        chain_id: 40204,
        raw: "probe".into(),
    });
    let _ = ceremony.reject(&view.id);
    view.id.parse().expect("numeric id")
}

// ------------------------------------------------------------------------ always-on tests

#[test]
fn propose_deploy_without_a_gate_record_refuses_and_opens_no_ceremony() {
    let v = vault("norecord", None);
    let ceremony = crate::ceremony::SignatureCeremony::new();
    let gate = GateStore::default();
    let before = probe_next_id(&ceremony);
    let err = propose_deploy(&v, &ceremony, &gate, &stub_bytecode(), None, None, None)
        .expect_err("no gate record, no deploy");
    assert!(err.contains("no D-4 deploy gate record"), "{err}");
    assert_eq!(probe_next_id(&ceremony), before + 1, "no ceremony was opened");
}

#[test]
fn propose_deploy_on_a_not_ready_record_names_the_failing_items_and_opens_no_ceremony() {
    let v = vault("notready", None);
    let ceremony = crate::ceremony::SignatureCeremony::new();
    let gate = GateStore::default();
    let mut inputs = ready_inputs(&stub_bytecode());
    inputs.aderyn = ToolRun::NotInstalled;
    inputs.medusa.run = ToolRun::NotInstalled;
    let rec = evaluate(&inputs, 1).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::NotReady);
    gate.record_and_revoke(rec, |_| {}).expect("stored");
    let before = probe_next_id(&ceremony);
    let err = propose_deploy(&v, &ceremony, &gate, &stub_bytecode(), None, None, None)
        .expect_err("NOT READY, no deploy");
    assert!(err.contains("NOT READY"), "{err}");
    assert!(err.contains("Aderyn: aderyn is not installed"), "{err}");
    assert!(err.contains("Medusa campaign: medusa is not installed"), "{err}");
    assert!(!err.contains("Forge tests:"), "passing items are not named: {err}");
    assert_eq!(probe_next_id(&ceremony), before + 1, "no ceremony was opened");
}

#[test]
fn propose_deploy_on_ready_opens_one_creation_ceremony_bound_to_the_gated_bytes() {
    let v = vault("ready", None);
    let ceremony = crate::ceremony::SignatureCeremony::new();
    let gate = GateStore::default();
    let rec = evaluate(&ready_inputs(&stub_bytecode()), 1).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::Ready);
    gate.record_and_revoke(rec, |_| {}).expect("stored");

    let p = propose_deploy(
        &v,
        &ceremony,
        &gate,
        &stub_bytecode(),
        None,
        None,
        Some(300_000),
    )
    .expect("READY opens the ceremony");
    assert_eq!(
        p.gate.initcode_hash,
        initcode_hash(&initcode_of(&stub_bytecode())),
        "the review shows the hash of exactly the gated init code"
    );
    assert_eq!(p.gate.verdict, Verdict::Ready);
    assert_eq!(p.ceremony.origin, "local-user");
    assert_eq!(p.ceremony.chain_id, 40204);
    assert!(
        p.ceremony.decoded.action.to_lowercase().contains("contract"),
        "decoded as a contract creation: {:?}",
        p.ceremony.decoded.action
    );
    assert!(!p.ceremony.requires_raw_ack, "a creation is decodable");
    assert!(ceremony.status(&p.ceremony.id).is_some(), "PENDING, unsigned");
}

#[test]
fn a_not_ready_reevaluation_rejects_the_open_deploy_ceremony() {
    let v = vault("revoke", None);
    let ceremony = crate::ceremony::SignatureCeremony::new();
    let gate = GateStore::default();
    gate.record_and_revoke(
        evaluate(&ready_inputs(&stub_bytecode()), 1).expect("evaluates"),
        |_| {},
    )
    .expect("stored");
    let p = propose_deploy(&v, &ceremony, &gate, &stub_bytecode(), None, None, None)
        .expect("opens");
    // The same bytes are re-gated and a test now fails.
    let mut again = ready_inputs(&stub_bytecode());
    again.forge_tests = ran(
        r#"{"test/T.t.sol:T":{"test_results":{"test_cap()":{"status":"Failure"},"test_mint()":{"status":"Success"}}}}"#,
    );
    let rec = evaluate(&again, 2).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::NotReady);
    gate.record_and_revoke(rec, |id| {
        let _ = ceremony.reject(id);
    })
    .expect("stored");
    assert!(
        ceremony.status(&p.ceremony.id).is_none(),
        "the open deploy ceremony was rejected"
    );
}

/// The JSON `scripts/e2e-hello-mint.sh` writes (built here with the same field names) is the
/// typed `GateInputs` hand-over, including the "not installed" state the default verifier
/// configuration produces on a machine without aderyn and medusa.
#[test]
fn the_e2e_script_gate_inputs_shape_is_the_typed_hand_over() {
    let bc = stub_bytecode();
    let receipt = r#"{"status":"0x1","contractAddress":"0x5fbdb2315678afecb367f032d93f642f64180aa3","gasUsed":"0x5208"}"#;
    let json = serde_json::json!({
        "bytecodeHex": bc,
        "constructorArgsHex": null,
        "compiler": {"solcVersion": "0.8.36", "optimizer": true, "optimizerRuns": 200, "evmVersion": "cancun", "viaIr": false},
        "forgeTests": {"state": "ran", "output": r#"{"t:T":{"test_results":{"test_a()":{"status":"Success"}}}}"#, "durationMs": 10, "toolVersion": "forge 1.5.1"},
        "slither": {"state": "ran", "output": r#"{"success":true,"error":null,"results":{"detectors":[]}}"#, "durationMs": 10, "toolVersion": "0.11.6"},
        "aderyn": {"state": "notInstalled"},
        "medusa": {"run": {"state": "notInstalled"}, "callBudget": 50000},
        "forkDryRun": {"run": {"state": "ran", "output": receipt, "durationMs": 10, "toolVersion": "anvil 1.5.1"}, "txInputHex": bc, "citratePrecompiles": "none"},
    });
    let inputs: GateInputs = serde_json::from_value(json).expect("the script's shape parses");
    let rec = evaluate(&inputs, 1).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::NotReady);
    let failing: Vec<GateItemId> = rec.failing().map(|i| i.id).collect();
    assert_eq!(failing, vec![GateItemId::Aderyn, GateItemId::Medusa]);
    // An errored tool is the third state the script can write.
    let err: ToolRun =
        serde_json::from_value(serde_json::json!({"state": "error", "message": "exit 1"}))
            .expect("error state parses");
    assert!(matches!(err, ToolRun::Error { .. }));
}

/// US-6.1 AC2: the forge finding names the failing tests (at most three, bounded, no control
/// characters), so the member and Hermes see what broke, not only a count.
#[test]
fn a_forge_failure_reason_names_the_failing_tests() {
    let mut inputs = ready_inputs(&stub_bytecode());
    inputs.forge_tests = ran(
        r#"{"test/Token.t.sol:LemonDropsTest":{"test_results":{"test_mint_past_the_cap_reverts()":{"status":"Failure"},"test_mint()":{"status":"Success"}}}}"#,
    );
    let rec = evaluate(&inputs, 1).expect("evaluates");
    let forge = rec
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForgeTests)
        .expect("forge item");
    assert!(!forge.pass);
    assert_eq!(
        forge.reason,
        "1 failed (LemonDropsTest.test_mint_past_the_cap_reverts()), 1 passed"
    );

    let many: serde_json::Map<String, serde_json::Value> = (0..5)
        .map(|i| {
            (
                format!("t_{i}()"),
                serde_json::json!({"status": "Failure"}),
            )
        })
        .collect();
    let long = format!("{}\u{1b}[31m()", "x".repeat(200));
    let mut results = many.clone();
    results.insert(long, serde_json::json!({"status": "Failure"}));
    let report = serde_json::json!({"test/A.t.sol:A": {"test_results": results}});
    inputs.forge_tests = ran(&report.to_string());
    let rec = evaluate(&inputs, 1).expect("evaluates");
    let reason = &rec
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForgeTests)
        .expect("forge item")
        .reason;
    assert!(reason.starts_with("6 failed (A.t_0(), A.t_1(), A.t_2(), …)"), "{reason}");
    assert!(!reason.chars().any(|c| c.is_control()), "{reason:?}");

    // A single over-long name is cut, and its control characters are dropped.
    let mut only = serde_json::Map::new();
    only.insert(
        format!("{}\u{1b}()", "y".repeat(200)),
        serde_json::json!({"status": "Failure"}),
    );
    let report = serde_json::json!({"test/B.t.sol:B": {"test_results": only}});
    inputs.forge_tests = ran(&report.to_string());
    let rec = evaluate(&inputs, 1).expect("evaluates");
    let reason = &rec
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForgeTests)
        .expect("forge item")
        .reason;
    assert!(reason.contains('…') && reason.len() < 200, "{reason}");
    assert!(!reason.chars().any(|c| c.is_control()), "{reason:?}");
}

/// The test-only verifier configuration is never shipped: `scripts/` is not an app resource,
/// and the configuration says it is test-only.
#[test]
fn the_test_full_verifier_configuration_is_test_only_and_not_bundled() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    let resources = conf
        .pointer("/bundle/resources")
        .map(|r| r.to_string())
        .unwrap_or_default();
    assert!(!resources.contains("scripts"), "scripts/ must not be bundled: {resources}");
    let full: serde_json::Value =
        serde_json::from_str(include_str!("../../scripts/e2e/verifiers.test-full.json"))
            .expect("test-full config");
    assert_eq!(full["testOnly"], true);
    let default: serde_json::Value =
        serde_json::from_str(include_str!("../../scripts/e2e/verifiers.default.json"))
            .expect("default config");
    assert_eq!(default["testOnly"], false);
    assert_eq!(default["tools"]["aderyn"]["source"], "path");
    assert_eq!(default["tools"]["medusa"]["source"], "path");
}

// ------------------------------------------------------------------------ the e2e run

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn load_inputs(dir: &Path, rel: &str) -> GateInputs {
    serde_json::from_str(&read(dir, rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn selector(sig: &str) -> Vec<u8> {
    Keccak256::digest(sig.as_bytes())[..4].to_vec()
}

fn word(v: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&v.to_be_bytes());
    w
}

fn ret_hex(s: &str) -> Vec<u8> {
    hex::decode(s.trim_start_matches("0x")).expect("return data hex")
}

fn ret_u128(s: &str) -> u128 {
    let b = ret_hex(s);
    assert!(b.len() >= 32 && b[..16].iter().all(|x| *x == 0), "fits u128");
    u128::from_be_bytes(b[16..32].try_into().expect("16 bytes"))
}

fn ret_address(s: &str) -> String {
    let b = ret_hex(s);
    format!("0x{}", hex::encode(&b[12..32]))
}

fn ret_string(s: &str) -> String {
    let b = ret_hex(s);
    let len = u128::from_be_bytes(b[48..64].try_into().expect("len")) as usize;
    String::from_utf8(b[64..64 + len].to_vec()).expect("utf8")
}

fn gate_summary(rec: &crate::deploy_gate::GateRecord) -> serde_json::Value {
    serde_json::json!({
        "verdict": rec.verdict,
        "initcodeHash": rec.initcode_hash,
        "bindingHash": rec.binding_hash,
        "items": rec.items.iter().map(|i| serde_json::json!({
            "item": i.label, "pass": i.pass, "reason": i.reason,
            "outputSha256": i.evidence.output_sha256, "counts": i.evidence.counts,
            "toolVersion": i.evidence.tool_version,
        })).collect::<Vec<_>>(),
    })
}

fn print_step(n: &str, text: &str) {
    eprintln!("e2e hello-mint | {n} | {text}");
}

#[test]
fn e2e_hello_mint_on_an_anvil_fork() {
    let Ok(dir) = std::env::var("CITRATE_E2E_HM_DIR") else {
        eprintln!("skipped: CITRATE_E2E_HM_DIR is not set (run scripts/e2e-hello-mint.sh)");
        return;
    };
    let dir = PathBuf::from(dir);
    let expect_ready = match std::env::var("CITRATE_E2E_HM_EXPECT").as_deref() {
        Ok("ready") => true,
        Ok("not-ready") => false,
        other => panic!("CITRATE_E2E_HM_EXPECT must be ready or not-ready, got {other:?}"),
    };
    let rpc = std::env::var("CITRATE_E2E_RPC").expect("CITRATE_E2E_RPC");
    let target = crate::contract_reader::parse_target(Some(&rpc)).expect("loopback target");
    let client =
        crate::rpc::RpcClient::with_transport(crate::rpc::HttpTransport::new(target.rpc_url()));
    let mut evidence = serde_json::Map::new();

    // ---- Then the verdict card reads READY only if every gate item passes --------------------
    let good = load_inputs(&dir, "good/gate-inputs.json");
    let rec = evaluate(&good, 1).expect("the good build evaluates");
    for i in &rec.items {
        print_step(
            "gate(good)",
            &format!("{}: {} ({})", i.label, if i.pass { "PASS" } else { "FAIL" }, i.reason),
        );
    }
    evidence.insert("gateGood".into(), gate_summary(&rec));
    let fixed_items = [GateItemId::ForgeTests, GateItemId::Slither, GateItemId::ForkDryRun];
    for id in fixed_items {
        let item = rec.items.iter().find(|i| i.id == id).expect("item");
        assert!(item.pass, "the template build passes {}: {}", item.label, item.reason);
    }
    let gate = GateStore::default();
    let ceremony = crate::ceremony::SignatureCeremony::new();
    // The member's wallet: anvil's public development mnemonic (account 0 is the template owner
    // the script rendered), opened in a vault exactly as the app opens one. Built at runtime.
    let mut words = vec!["test"; 11];
    words.push("junk");
    let v = vault("e2e", Some(&words.join(" ")));
    let wallet = crate::wallet::address(&v).expect("address").address;
    let owner = read(&dir, "good/owner.txt").trim().to_lowercase();
    assert_eq!(wallet.to_lowercase(), owner, "the member's wallet is the rendered owner");
    gate.record_and_revoke(rec.clone(), |_| {}).expect("stored");

    if !expect_ready {
        // Default verifier configuration on a machine without aderyn and medusa: NOT READY,
        // and those two are exactly what fails.
        assert_eq!(rec.verdict, Verdict::NotReady);
        let failing: Vec<GateItemId> = rec.failing().map(|i| i.id).collect();
        assert_eq!(failing, vec![GateItemId::Aderyn, GateItemId::Medusa]);
        let before = probe_next_id(&ceremony);
        let err = propose_deploy(&v, &ceremony, &gate, &good.bytecode_hex, None, None, None)
            .expect_err("NOT READY refuses the deploy");
        print_step("deploy(good)", &err);
        assert!(err.contains("NOT READY") && err.contains("not installed"), "{err}");
        assert_eq!(probe_next_id(&ceremony), before + 1, "no ceremony was opened");
        evidence.insert("deployGood".into(), serde_json::json!({"refused": err}));
    } else {
        assert_eq!(rec.verdict, Verdict::Ready, "every gate item passes");

        // ---- When the Dev clicks Deploy: a SignatureCeremony for the gated bytecode -----------
        let dry_gas = rec
            .items
            .iter()
            .find(|i| i.id == GateItemId::ForkDryRun)
            .and_then(|i| i.evidence.counts.get("gas_used").copied())
            .expect("dry-run gas");
        let p = propose_deploy(
            &v,
            &ceremony,
            &gate,
            &good.bytecode_hex,
            None,
            None,
            Some(dry_gas + dry_gas / 5),
        )
        .expect("READY opens the deploy ceremony");
        assert_eq!(p.gate.initcode_hash, rec.initcode_hash, "bound to the gated hash");
        assert!(p.ceremony.decoded.action.to_lowercase().contains("contract"));
        print_step(
            "ceremony(deploy)",
            &format!("{} {} for {}", p.ceremony.id, p.ceremony.decoded.action, p.gate.initcode_hash),
        );

        // ---- And after approval the contract is deployed (to the fork) --------------------------
        let cfg = crate::ceremony::BroadcastConfig {
            chain_id: 40204,
            poll_attempts: 50,
            poll_interval: std::time::Duration::from_millis(200),
        };
        let sent = ceremony
            .approve_and_broadcast(&v, &client, &p.ceremony.id, false, cfg)
            .expect("the ceremony signs and broadcasts the creation");
        let receipt = crate::postdeploy::parse_receipt(
            &crate::contract_reader::rpc_raw(
                &client,
                "eth_getTransactionReceipt",
                serde_json::json!([sent.tx_hash]),
            )
            .expect("receipt"),
        )
        .expect("parses")
        .expect("mined");
        assert_eq!(receipt.status, Some(1), "the creation succeeded");
        let addr = receipt.contract_address.clone().expect("created address");
        print_step("deploy", &format!("{addr} tx {} block {}", sent.tx_hash, receipt.block_number));
        // The signed creation carried exactly the gated init code.
        let tx = crate::contract_reader::rpc_raw(
            &client,
            "eth_getTransactionByHash",
            serde_json::json!([sent.tx_hash]),
        )
        .expect("tx");
        let input = tx.get("input").and_then(|x| x.as_str()).expect("input");
        assert_eq!(
            initcode_hash(&ret_hex(input)),
            rec.initcode_hash,
            "the broadcast tx carries the gated bytes"
        );
        assert_eq!(
            tx.get("from").and_then(|x| x.as_str()).map(str::to_lowercase),
            Some(wallet.to_lowercase()),
            "signed by the member's vault wallet"
        );

        // ---- And verified: the deployed code is the compiled runtime code -----------------------
        let code = crate::contract_reader::rpc_raw(
            &client,
            "eth_getCode",
            serde_json::json!([addr, "latest"]),
        )
        .expect("code");
        let code = code.as_str().expect("hex").to_lowercase();
        let compiled = read(&dir, "good/deployed-bytecode.hex").trim().to_lowercase();
        assert_eq!(code, compiled, "on-chain runtime code equals the compiled artifact");
        let name = crate::contract_reader::view_call(&client, &addr, &selector("name()"))
            .expect("name()");
        let symbol = crate::contract_reader::view_call(&client, &addr, &selector("symbol()"))
            .expect("symbol()");
        let cap = crate::contract_reader::view_call(&client, &addr, &selector("MAX_SUPPLY()"))
            .expect("MAX_SUPPLY()");
        let price = ret_u128(
            &crate::contract_reader::view_call(&client, &addr, &selector("PRICE()"))
                .expect("PRICE()"),
        );
        let params: serde_json::Value =
            serde_json::from_str(&read(&dir, "good/params.json")).expect("params");
        assert_eq!(ret_string(&name), params["name"].as_str().expect("name"));
        assert_eq!(ret_string(&symbol), params["symbol"].as_str().expect("symbol"));
        assert_eq!(
            ret_u128(&cap).to_string(),
            params["supply"].as_str().expect("supply")
        );
        assert_eq!(price.to_string(), params["price"].as_str().expect("price"));
        let owner_on_chain = ret_address(
            &crate::contract_reader::view_call(&client, &addr, &selector("owner()"))
                .expect("owner()"),
        );
        assert_eq!(owner_on_chain, owner);
        print_step(
            "verify",
            &format!(
                "runtime code matches ({} bytes); name {:?}, cap {}, price {price} wei, owner {owner_on_chain}",
                (code.len() - 2) / 2,
                ret_string(&name),
                ret_u128(&cap)
            ),
        );

        // ---- And a test mint succeeds (through a second ceremony) -------------------------------
        let mut mint = selector("mint(uint256)");
        mint.extend_from_slice(&word(1));
        let gas = crate::contract_reader::estimate_write_gas(&client, &wallet, &addr, &mint, price)
            .expect("the node estimates mint(1)");
        let mv = ceremony.request(crate::contract_reader::write_intent(
            &wallet, &addr, &mint, price, gas,
        ));
        let minted = ceremony
            .approve_and_broadcast(&v, &client, &mv.id, false, cfg)
            .expect("the mint ceremony signs and broadcasts");
        let mint_receipt = crate::postdeploy::parse_receipt(
            &crate::contract_reader::rpc_raw(
                &client,
                "eth_getTransactionReceipt",
                serde_json::json!([minted.tx_hash]),
            )
            .expect("receipt"),
        )
        .expect("parses")
        .expect("mined");
        assert_eq!(mint_receipt.status, Some(1), "mint(1) succeeded");
        let total = ret_u128(
            &crate::contract_reader::view_call(&client, &addr, &selector("totalMinted()"))
                .expect("totalMinted()"),
        );
        let mut owner_of = selector("ownerOf(uint256)");
        owner_of.extend_from_slice(&word(1));
        let holder = ret_address(
            &crate::contract_reader::view_call(&client, &addr, &owner_of).expect("ownerOf(1)"),
        );
        assert_eq!(total, 1);
        assert_eq!(holder, wallet.to_lowercase());
        let bal = client.get_balance(&addr).expect("balance");
        assert_eq!(bal, price, "the contract holds exactly one mint payment");
        print_step(
            "mint",
            &format!("tx {} totalMinted {total}, token 1 held by {holder}", minted.tx_hash),
        );

        // ---- AC3: the Vercel export is produced from the project ------------------------------
        if let Ok(project) = std::env::var("CITRATE_E2E_HM_PROJECT") {
            let proj = crate::postdeploy::open_project(Path::new(&project)).expect("project");
            let export = crate::postdeploy::vercel_export(&proj).expect("vercel export");
            print_step("vercel-export", &format!("{export:?}"));
            evidence.insert("vercelExport".into(), serde_json::json!(format!("{export:?}")));
        }

        evidence.insert(
            "deployGood".into(),
            serde_json::json!({
                "ceremonyId": p.ceremony.id, "decodedAction": p.ceremony.decoded.action,
                "txHash": sent.tx_hash, "blockNumber": receipt.block_number, "address": addr,
                "runtimeCodeBytes": (code.len() - 2) / 2, "mintTx": minted.tx_hash,
                "totalMinted": total.to_string(), "token1Holder": holder,
                "contractBalanceWei": bal.to_string(),
            }),
        );
    }

    // ---- AC2: the same run with an injected bug yields NOT READY with the finding -------------
    let bug = load_inputs(&dir, "bug/gate-inputs.json");
    let bug_rec = evaluate(&bug, 2).expect("the bug build evaluates");
    for i in &bug_rec.items {
        print_step(
            "gate(bug)",
            &format!("{}: {} ({})", i.label, if i.pass { "PASS" } else { "FAIL" }, i.reason),
        );
    }
    evidence.insert("gateBug".into(), gate_summary(&bug_rec));
    assert_ne!(bug_rec.initcode_hash, rec.initcode_hash, "the bug changes the bytes");
    assert_eq!(bug_rec.verdict, Verdict::NotReady);
    let forge = bug_rec
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForgeTests)
        .expect("forge item");
    assert!(!forge.pass, "the unbounded mint fails the template's own tests: {}", forge.reason);
    if expect_ready {
        let medusa = bug_rec
            .items
            .iter()
            .find(|i| i.id == GateItemId::Medusa)
            .expect("medusa item");
        assert!(!medusa.pass, "medusa breaks the supply invariant: {}", medusa.reason);
    }
    gate.record_and_revoke(bug_rec, |id| {
        let _ = ceremony.reject(id);
    })
    .expect("stored");
    let before = probe_next_id(&ceremony);
    let err = propose_deploy(&v, &ceremony, &gate, &bug.bytecode_hex, None, None, None)
        .expect_err("US-6.2: the gate refuses the bug build");
    print_step("deploy(bug)", &err);
    assert!(err.contains("NOT READY") && err.contains("Forge tests:"), "{err}");
    assert_eq!(probe_next_id(&ceremony), before + 1, "no ceremony was opened");
    evidence.insert("deployBug".into(), serde_json::json!({"refused": err}));

    let out = dir.join(if expect_ready {
        "evidence-ready.json"
    } else {
        "evidence-not-ready.json"
    });
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&serde_json::Value::Object(evidence)).expect("json"),
    )
    .expect("evidence written");
    print_step("done", &format!("evidence at {}", out.display()));
}
