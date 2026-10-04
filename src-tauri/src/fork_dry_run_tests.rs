// HUP-S6.10 — the deploy gate's fork step on the Citrate-aware fork.
//
// The citrate-fork reports are REAL outputs of citrate-chain `crates/citrate-fork` (see
// tests/fixtures/deploygate/README.md). The process tests run a small shell script in place of
// the binary to exercise the plumbing (exit codes, timeout, stdout capture); the e2e test runs
// the real binary against anvil when scripts/e2e-postdeploy-reader.sh provides one.
use super::*;
use crate::deploy_gate::{
    evaluate, evaluate_submission, evaluate_with_fork, CompilerSettings, ForkProvenance,
    GateInputs, GateItemId, GateRecord, MedusaInput, Verdict,
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

/// Every other gate item green; no fork input yet.
fn inputs_for(bytecode_hex: &str) -> GateInputs {
    GateInputs {
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
        fork_dry_run: None,
        fork_in_core: None,
    }
}

/// The gate with a fork run core made itself (what `forkInCore` produces).
fn gate(bytecode_hex: &str, fork: ForkDryRunInput) -> GateRecord {
    evaluate_with_fork(&inputs_for(bytecode_hex), Some(&fork), ForkProvenance::Core, 1)
        .expect("evaluates")
}

/// The gate with the same fork run handed in by a caller (`forkDryRun`).
fn gate_from_caller(bytecode_hex: &str, fork: ForkDryRunInput) -> GateRecord {
    let mut inputs = inputs_for(bytecode_hex);
    inputs.fork_dry_run = Some(fork);
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
            "if [ \"$1\" = --version ]; then sleep 30; exit 0; fi\n\
             [ \"$1\" = run ] && [ \"$4\" = --rpc ] || exit 9\ncat >/dev/null\ncat '{}'",
            report_file.display()
        ),
    );
    let t = Instant::now();
    match run_fork(&bin, &json!({}), "http://127.0.0.1:1", Duration::from_secs(10)) {
        ToolRun::Ran {
            output,
            tool_version,
            ..
        } => {
            // The version comes from the report itself; a second `--version` run (which
            // this script would hang on) is never made.
            assert!(t.elapsed() < Duration::from_secs(8), "no --version run");
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
        state_rpc: Some(rpc.clone()),
        from: None,
        test_mint: Some(TestMint {
            quantity: 2,
            price_wei: price.clone(),
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

    // The wired path: deploy_gate_submit with forkInCore runs the same fork step in core.
    let mut inputs = inputs_for(&initcode);
    inputs.fork_in_core = Some(ForkInCore {
        state_rpc: Some(rpc),
        from: None,
        test_mint: Some(TestMint {
            quantity: 2,
            price_wei: price,
        }),
    });
    let rec = evaluate_submission(&inputs, Some(Path::new(&bin)), FORK_TIMEOUT, 1)
        .expect("submission evaluates");
    let (pass, reason) = fork_item(&rec);
    assert!(pass, "forkInCore: {reason}");
    assert!(reason.contains("Citrate-aware fork"), "{reason}");
}

#[cfg(unix)]
#[test]
fn a_binary_that_never_reads_its_plan_is_still_bounded_by_the_timeout() {
    // A plan far larger than a pipe buffer, and a binary that never reads stdin: writing the
    // plan must not block past the time bound.
    let bin = script("noread", "sleep 5");
    let big = json!({ "steps": [{ "kind": "create", "data": format!("0x{}", "60".repeat(512 * 1024)) }] });
    let t = Instant::now();
    match run_fork(&bin, &big, "http://127.0.0.1:1", Duration::from_millis(300)) {
        ToolRun::Error { message } => assert!(message.contains("did not finish"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(t.elapsed() < Duration::from_secs(4), "bounded by the timeout");
}

// ------------------------------------------------------------- provenance (forkInCore)

#[test]
fn a_caller_supplied_report_cannot_vouch_for_precompiles() {
    // The same real report that passes when core ran the fork fails when a caller hands it
    // in: its coverage list is the caller's word.
    let (ic, report) = fixture(BELNAP);
    let input = fork_input(ran(&report), &initcode_bytes(&ic));
    let rec = gate_from_caller(&ic, input);
    let (pass, reason) = fork_item(&rec);
    assert!(!pass);
    assert!(reason.contains("forkInCore"), "{reason}");
    assert_eq!(rec.verdict, Verdict::NotReady);
}

#[test]
fn a_caller_supplied_report_that_claims_every_precompile_is_real_still_fails() {
    // A forged report listing an unrunnable call site as real.
    let (ic, report) = fixture(BELNAP);
    let mut v: Value = serde_json::from_str(&report).expect("json");
    let code = format!("{ic}6101005afa"); // PUSH2 0x0100 GAS STATICCALL (inference family)
    v["steps"][0]["input"] = json!(code.clone());
    v["precompiles"]["real"] = json!(["0x0100", "0x0110"]);
    v["precompiles"]["touched"] = json!([]);
    let input = fork_input(ran(&v.to_string()), &initcode_bytes(&code));
    let (pass, reason) = fork_item(&gate_from_caller(&code, input));
    assert!(!pass);
    assert!(reason.contains("forkInCore"), "{reason}");
}

#[test]
fn no_fork_input_at_all_fails_the_fork_item() {
    let (ic, _) = fixture(BELNAP);
    let rec = evaluate(&inputs_for(&ic), 1).expect("evaluates");
    let (pass, reason) = fork_item(&rec);
    assert!(!pass);
    assert!(reason.contains("no fork dry run was supplied"), "{reason}");
}

#[test]
fn fork_in_core_and_a_caller_fork_together_are_refused() {
    let (ic, report) = fixture(BELNAP);
    let mut inputs = inputs_for(&ic);
    inputs.fork_dry_run = Some(fork_input(ran(&report), &initcode_bytes(&ic)));
    inputs.fork_in_core = Some(ForkInCore {
        state_rpc: None,
        from: None,
        test_mint: None,
    });
    let e = evaluate_submission(&inputs, None, FORK_TIMEOUT, 1).expect_err("refused");
    assert!(e.contains("not both"), "{e}");
}

#[test]
fn fork_in_core_without_the_binary_is_not_installed() {
    let (ic, _) = fixture(BELNAP);
    let mut inputs = inputs_for(&ic);
    inputs.fork_in_core = Some(ForkInCore {
        state_rpc: None,
        from: None,
        test_mint: None,
    });
    let rec = evaluate_submission(&inputs, None, FORK_TIMEOUT, 1).expect("evaluates");
    let (pass, reason) = fork_item(&rec);
    assert!(!pass);
    assert!(reason.contains("citrate-fork is not installed"), "{reason}");
}

#[test]
fn fork_in_core_refuses_a_state_rpc_that_is_not_40204_or_loopback() {
    let (ic, _) = fixture(BELNAP);
    let mut inputs = inputs_for(&ic);
    inputs.fork_in_core = Some(ForkInCore {
        state_rpc: Some("https://evil.example".into()),
        from: None,
        test_mint: None,
    });
    assert!(evaluate_submission(&inputs, None, FORK_TIMEOUT, 1).is_err());
}

#[cfg(unix)]
#[test]
fn fork_in_core_runs_the_binary_on_the_gated_init_code_and_can_be_ready() {
    let (ic, report) = fixture(BELNAP);
    let dir = std::env::temp_dir().join(format!("citrate-fork-core-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let report_file = dir.join("report.json");
    let plan_file = dir.join("plan.json");
    std::fs::write(&report_file, &report).expect("write");
    let bin = script(
        "core",
        &format!(
            "[ \"$1\" = run ] || exit 9\ncat > '{}'\ncat '{}'",
            plan_file.display(),
            report_file.display()
        ),
    );
    let mut inputs = inputs_for(&ic);
    inputs.fork_in_core = Some(ForkInCore {
        state_rpc: Some("http://127.0.0.1:18545".into()),
        from: None,
        test_mint: None,
    });
    let rec =
        evaluate_submission(&inputs, Some(&bin), Duration::from_secs(10), 1).expect("evaluates");
    let (pass, reason) = fork_item(&rec);
    assert!(pass, "{reason}");
    assert_eq!(rec.verdict, Verdict::Ready);
    // Core built the plan from the submitted init code, not from anything the caller sent.
    let plan: Value =
        serde_json::from_str(&std::fs::read_to_string(&plan_file).expect("plan")).expect("json");
    assert_eq!(plan["steps"][0]["data"], json!(ic));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fork_in_core_deserializes_from_the_wire_shape() {
    let j = json!({
        "bytecodeHex": "0x6000",
        "compiler": { "solcVersion": "0.8.36", "optimizer": true, "optimizerRuns": 200, "evmVersion": "cancun" },
        "forgeTests": { "state": "notInstalled" },
        "slither": { "state": "notInstalled" },
        "aderyn": { "state": "notInstalled" },
        "medusa": { "run": { "state": "notInstalled" }, "callBudget": 50000 },
        "forkInCore": { "stateRpc": "citrate", "testMint": { "quantity": 2, "priceWei": "5" } }
    });
    let inp: GateInputs = serde_json::from_value(j).expect("deserializes");
    assert_eq!(inp.fork_dry_run, None);
    let f = inp.fork_in_core.expect("forkInCore");
    assert_eq!(f.state_rpc.as_deref(), Some("citrate"));
    assert_eq!(f.test_mint.map(|m| m.quantity), Some(2));
    let bad = json!({ "stateRpc": "citrate", "bytecodeHex": "0x60" });
    assert!(serde_json::from_value::<ForkInCore>(bad).is_err(), "bytecode comes from the gate inputs only");
}
