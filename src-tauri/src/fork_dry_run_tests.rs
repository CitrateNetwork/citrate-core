// HUP-S6.10 — the deploy gate's fork step on the Citrate-aware fork.
//
// The citrate-fork reports are REAL outputs of citrate-chain `crates/citrate-fork` (see
// tests/fixtures/deploygate/README.md). The process tests run a small shell script in place of
// the binary to exercise the plumbing (exit codes, timeout, stdout capture); the e2e test runs
// the real binary against anvil when scripts/e2e-postdeploy-reader.sh provides one.
use super::*;
use crate::deploy_gate::{
    evaluate, CompilerSettings, GateInputs, GateItemId, GateRecord, MedusaInput, Verdict,
};

const BELNAP: &str = include_str!("../tests/fixtures/deploygate/citrate-fork-belnap.json");
const INFERENCE: &str = include_str!("../tests/fixtures/deploygate/citrate-fork-inference.json");
const REVERT: &str = include_str!("../tests/fixtures/deploygate/citrate-fork-revert.json");
const ANVIL_RECEIPT: &str = include_str!("../tests/fixtures/deploygate/anvil-receipt.json");
const FORGE_PASS: &str = include_str!("../tests/fixtures/deploygate/forge-pass.json");
const SLITHER_CLEAN: &str = include_str!("../tests/fixtures/deploygate/slither-clean.json");
const ADERYN_CLEAN: &str =
    include_str!("../tests/fixtures/deploygate/aderyn-clean.handwritten.json");
const MEDUSA_PASS: &str = include_str!("../tests/fixtures/deploygate/medusa-pass.handwritten.txt");

/// (initcode hex, report JSON text) of a fixture.
fn fixture(text: &str) -> (String, String) {
    let v: Value = serde_json::from_str(text).expect("fixture json");
    (
        v["initcode"].as_str().expect("initcode").to_string(),
        v["report"].to_string(),
    )
}

fn ran(output: &str) -> ToolRun {
    ToolRun::Ran {
        output: output.to_string(),
        duration_ms: 42,
        tool_version: Some("citrate-fork 0.4.0".into()),
    }
}

fn initcode_bytes(hex_s: &str) -> Vec<u8> {
    hex::decode(hex_s.trim_start_matches("0x")).expect("hex")
}

/// Every other gate item green; the fork item is what the test supplies.
fn gate(bytecode_hex: &str, fork: ForkDryRunInput) -> GateRecord {
    let inputs = GateInputs {
        bytecode_hex: bytecode_hex.to_string(),
        constructor_args_hex: None,
        compiler: CompilerSettings {
            solc_version: "0.8.36".into(),
            optimizer: true,
            optimizer_runs: 200,
            evm_version: "cancun".into(),
            via_ir: false,
        },
        forge_tests: ran(FORGE_PASS),
        slither: ran(SLITHER_CLEAN),
        aderyn: ran(ADERYN_CLEAN),
        medusa: MedusaInput {
            run: ran(MEDUSA_PASS),
            call_budget: 50_000,
        },
        fork_dry_run: fork,
    };
    evaluate(&inputs, 1).expect("evaluates")
}

fn fork_item(rec: &GateRecord) -> (bool, String) {
    let i = rec
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForkDryRun)
        .expect("fork item");
    (i.pass, i.reason.clone())
}

// ------------------------------------------------------------------------------ the plan

#[test]
fn the_plan_creates_then_test_mints_with_the_exact_payment() {
    let plan = build_plan(
        &[0x60, 0x80],
        "0x00000000000000000000000000000000000d7a11",
        Some(&TestMint {
            quantity: 2,
            price_wei: "5000000000000000000".into(),
        }),
    )
    .expect("plan");
    let steps = plan["steps"].as_array().expect("steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0], json!({ "kind": "create", "data": "0x6080" }));
    assert_eq!(steps[1]["to"], "created:0");
    assert_eq!(
        steps[1]["data"],
        format!("0xa0712d68{}", format_args!("{:064x}", 2))
    );
    assert_eq!(steps[1]["value"], "10000000000000000000");
    let from = plan["from"].as_str().expect("from");
    // The fork-only balance covers the mint plus 1 SALT headroom.
    assert_eq!(plan["balances"][from], "11000000000000000000");
}

#[test]
fn the_plan_without_a_mint_only_creates() {
    let plan = build_plan(&[0x60], DRY_RUN_SENDER, None).expect("plan");
    assert_eq!(plan["steps"].as_array().map(Vec::len), Some(1));
}

#[test]
fn bad_plans_are_refused() {
    let m = |q: u32, p: &str| TestMint {
        quantity: q,
        price_wei: p.into(),
    };
    assert!(build_plan(&[], DRY_RUN_SENDER, None).is_err(), "empty init code");
    assert!(build_plan(&[0x60], "0x12", None).is_err(), "bad sender");
    assert!(build_plan(&[0x60], DRY_RUN_SENDER, Some(&m(0, "1"))).is_err());
    assert!(build_plan(&[0x60], DRY_RUN_SENDER, Some(&m(MAX_TEST_MINT + 1, "1"))).is_err());
    assert!(build_plan(&[0x60], DRY_RUN_SENDER, Some(&m(1, "-5"))).is_err());
    let huge = u128::MAX.to_string();
    assert!(build_plan(&[0x60], DRY_RUN_SENDER, Some(&m(2, &huge))).is_err(), "overflow");
}

// ------------------------------------------------------------------------- the binary

#[test]
fn the_binary_comes_from_the_env_override_or_the_component_store() {
    let dir = std::env::temp_dir().join(format!("citrate-fork-bin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("citrate-fork");
    std::fs::write(&file, b"x").expect("write");
    let f = file.to_string_lossy().to_string();
    assert_eq!(resolve_fork_bin(Some(&f), None), Some(file.clone()));
    let missing = dir.join("nope").to_string_lossy().to_string();
    assert_eq!(resolve_fork_bin(Some(&missing), Some(&dir)), None, "a named missing file is not replaced");
    assert_eq!(resolve_fork_bin(None, None), None);
    assert_eq!(resolve_fork_bin(Some("  "), Some(&dir)), None, "no store state");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_binary_is_not_installed_and_the_gate_fails() {
    let (ic, _) = fixture(BELNAP);
    let req = ForkDryRunRequest {
        bytecode_hex: ic.clone(),
        constructor_args_hex: None,
        state_rpc: None,
        from: None,
        test_mint: None,
    };
    let input = dry_run(&req, None, FORK_TIMEOUT).expect("input");
    assert_eq!(input.run, ToolRun::NotInstalled);
    let (pass, _) = fork_item(&gate(&ic, input));
    assert!(!pass);
}

#[test]
fn the_state_rpc_is_40204_or_a_loopback_fork() {
    let (ic, _) = fixture(BELNAP);
    let req = |rpc: &str| ForkDryRunRequest {
        bytecode_hex: ic.clone(),
        constructor_args_hex: None,
        state_rpc: Some(rpc.into()),
        from: None,
        test_mint: None,
    };
    assert!(dry_run(&req("https://evil.example"), None, FORK_TIMEOUT).is_err());
    assert!(dry_run(&req("http://127.0.0.1:8545"), None, FORK_TIMEOUT).is_ok());
    assert!(dry_run(&req("citrate"), None, FORK_TIMEOUT).is_ok());
}

#[cfg(unix)]
fn script(name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("citrate-fork-script-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let p = dir.join("citrate-fork");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("write");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    p
}

#[cfg(unix)]
#[test]
fn a_report_on_stdout_is_a_run_with_the_tool_version() {
    let (_, report) = fixture(BELNAP);
    let report_file = std::env::temp_dir().join(format!("citrate-fork-report-{}.json", std::process::id()));
    std::fs::write(&report_file, &report).expect("write");
    let bin = script(
        "ok",
        &format!(
            "if [ \"$1\" = --version ]; then echo 'citrate-fork 0.4.0'; exit 0; fi\n\
             [ \"$1\" = run ] && [ \"$4\" = --rpc ] || exit 9\ncat >/dev/null\ncat '{}'",
            report_file.display()
        ),
    );
    match run_fork(&bin, &json!({}), "http://127.0.0.1:1", Duration::from_secs(10)) {
        ToolRun::Ran {
            output,
            tool_version,
            ..
        } => {
            assert_eq!(tool_version.as_deref(), Some("citrate-fork 0.4.0"));
            let v: Value = serde_json::from_str(&output).expect("json");
            assert_eq!(v["engine"], "citrate-fork");
        }
        other => panic!("expected Ran, got {other:?}"),
    }
    let _ = std::fs::remove_file(&report_file);
}

#[cfg(unix)]
#[test]
fn a_non_zero_exit_is_an_error_with_the_reason() {
    let bin = script("fail", "[ \"$1\" = --version ] && exit 0\necho 'citrate-fork: the endpoint serves chain 1, not 40204' >&2\nexit 1");
    match run_fork(&bin, &json!({}), "http://127.0.0.1:1", Duration::from_secs(10)) {
        ToolRun::Error { message } => assert!(message.contains("chain 1"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn a_run_past_the_timeout_is_killed_and_reported() {
    let bin = script("slow", "[ \"$1\" = --version ] && exit 0\nsleep 5");
    let t = Instant::now();
    match run_fork(&bin, &json!({}), "http://127.0.0.1:1", Duration::from_millis(300)) {
        ToolRun::Error { message } => assert!(message.contains("did not finish"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(t.elapsed() < Duration::from_secs(4), "killed, not waited out");
}

#[test]
fn a_missing_binary_is_an_error() {
    let run = run_fork(
        Path::new("/nonexistent/citrate-fork"),
        &json!({}),
        "http://127.0.0.1:1",
        Duration::from_secs(1),
    );
    assert!(matches!(run, ToolRun::Error { .. }));
}

// ------------------------------------------------------------------- the gate verdicts

#[test]
fn a_contract_using_a_real_citrate_precompile_passes_on_the_citrate_fork() {
    let (ic, report) = fixture(BELNAP);
    let input = fork_input(ran(&report), &initcode_bytes(&ic));
    assert_eq!(input.citrate_precompiles, PrecompileUse::Used, "0x0110 was touched");
    assert_eq!(input.tx_input_hex, ic, "bound to the init code the fork ran");
    let rec = gate(&ic, input);
    let (pass, reason) = fork_item(&rec);
    assert!(pass, "{reason}");
    assert!(reason.contains("Citrate-aware fork (40204 block 100000)"), "{reason}");
    assert_eq!(rec.verdict, Verdict::Ready);
    let counts = &rec
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForkDryRun)
        .expect("item")
        .evidence
        .counts;
    assert_eq!(counts.get("citrate_precompiles_touched"), Some(&1));
}

#[test]
fn the_same_precompile_use_still_fails_on_a_plain_anvil_receipt() {
    let (ic, _) = fixture(BELNAP);
    let rec = gate(
        &ic,
        ForkDryRunInput {
            run: ran(ANVIL_RECEIPT),
            tx_input_hex: ic.clone(),
            citrate_precompiles: PrecompileUse::Used,
        },
    );
    let (pass, reason) = fork_item(&rec);
    assert!(!pass);
    assert!(reason.contains("anvil fork cannot simulate"), "{reason}");
}

#[test]
fn an_unavailable_precompile_fails_and_is_named() {
    let (ic, report) = fixture(INFERENCE);
    let rec = gate(&ic, fork_input(ran(&report), &initcode_bytes(&ic)));
    let (pass, reason) = fork_item(&rec);
    assert!(!pass);
    assert!(reason.contains("0x0100") && reason.contains("cannot reproduce"), "{reason}");
    assert_eq!(rec.verdict, Verdict::NotReady);
}

#[test]
fn a_failed_test_mint_fails_the_fork_item() {
    let (ic, report) = fixture(REVERT);
    let rec = gate(&ic, fork_input(ran(&report), &initcode_bytes(&ic)));
    let (pass, reason) = fork_item(&rec);
    assert!(!pass);
    assert!(reason.contains("step 1 (call): reverted"), "{reason}");
}

#[test]
fn a_report_for_other_bytecode_fails_the_binding() {
    let (ic, _) = fixture(BELNAP);
    let (_, other_report) = fixture(REVERT);
    let input = fork_input(ran(&other_report), &initcode_bytes(&ic));
    let (pass, reason) = fork_item(&gate(&ic, input));
    assert!(!pass);
    assert!(reason.contains("different bytecode"), "{reason}");
}

#[test]
fn a_report_on_another_chain_or_malformed_fails() {
    let (ic, report) = fixture(BELNAP);
    let mut v: Value = serde_json::from_str(&report).expect("json");
    v["chainId"] = json!(1337);
    let (pass, reason) = fork_item(&gate(&ic, fork_input(ran(&v.to_string()), &initcode_bytes(&ic))));
    assert!(!pass);
    assert!(reason.contains("chain 1337"), "{reason}");

    let mut m: Value = serde_json::from_str(&report).expect("json");
    m["precompiles"] = json!({});
    let (pass, reason) = fork_item(&gate(&ic, fork_input(ran(&m.to_string()), &initcode_bytes(&ic))));
    assert!(!pass);
    assert!(reason.contains("no valid precompiles"), "{reason}");

    let mut nc: Value = serde_json::from_str(&report).expect("json");
    nc["steps"][0]["kind"] = json!("call");
    let (pass, reason) = fork_item(&gate(&ic, fork_input(ran(&nc.to_string()), &initcode_bytes(&ic))));
    assert!(!pass);
    assert!(reason.contains("did not start with the contract creation"), "{reason}");
}

#[test]
fn a_bytecode_call_site_the_fork_cannot_run_fails_even_if_untouched() {
    let (ic, report) = fixture(BELNAP);
    let mut v: Value = serde_json::from_str(&report).expect("json");
    // Pretend the fork did not run 0x0107 (it does); append a `PUSH2 0x0107 GAS STATICCALL`.
    v["precompiles"]["real"] = json!(["0x0110"]);
    let code = format!("{ic}610107" );
    let code = format!("{code}5afa");
    v["steps"][0]["input"] = json!(code.clone());
    let (pass, reason) = fork_item(&gate(&code, fork_input(ran(&v.to_string()), &initcode_bytes(&code))));
    assert!(!pass);
    assert!(reason.contains("0x0107") && reason.contains("cannot reproduce"), "{reason}");
}

#[test]
fn evidence_is_none_for_a_plain_receipt() {
    let v: Value = serde_json::from_str(ANVIL_RECEIPT).expect("json");
    assert!(citrate_fork_evidence(&v).is_none());
}

// ----------------------------------------------------------------------------- e2e

/// Runs with scripts/e2e-postdeploy-reader.sh: the real citrate-fork binary against the
/// e2e anvil (chain id 40204), on the rendered hello-mint contract, with a test mint.
#[test]
fn e2e_citrate_fork_dry_run_of_hello_mint_with_a_test_mint() {
    let (Ok(bin), Ok(rpc), Ok(initcode), Ok(price)) = (
        std::env::var("CITRATE_E2E_FORK_BIN"),
        std::env::var("CITRATE_E2E_RPC"),
        std::env::var("CITRATE_E2E_INITCODE"),
        std::env::var("CITRATE_E2E_PRICE_WEI"),
    ) else {
        eprintln!("skipped: CITRATE_E2E_FORK_BIN is not set (run scripts/e2e-postdeploy-reader.sh)");
        return;
    };
    let req = ForkDryRunRequest {
        bytecode_hex: initcode.clone(),
        constructor_args_hex: None,
        state_rpc: Some(rpc),
        from: None,
        test_mint: Some(TestMint {
            quantity: 2,
            price_wei: price,
        }),
    };
    let input = dry_run(&req, Some(Path::new(&bin)), FORK_TIMEOUT).expect("dry run");
    let ToolRun::Ran { output, .. } = &input.run else {
        panic!("citrate-fork did not run: {:?}", input.run);
    };
    let v: Value = serde_json::from_str(output).expect("report");
    assert_eq!(v["allStepsSucceeded"], true, "{output}");
    assert_eq!(v["chainId"], 40204);
    let rec = gate(&initcode, input);
    let (pass, reason) = fork_item(&rec);
    assert!(pass, "{reason}");
}
