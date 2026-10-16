// HUP-S6.4 — D-4 deploy gate tests. Pure: no chain, no signing, no tool processes.
//
// The forge / slither / anvil fixtures are REAL outputs captured from forge 1.5.1,
// slither 0.11.6 and anvil 1.5.1 on a two-contract hello-mint project (absolute paths
// rewritten to /work). Aderyn and Medusa are not installed on the build machine, so their
// fixtures are hand-written in the documented report shapes and are named `*.handwritten.*`
// (see tests/fixtures/deploygate/README.md).
use super::*;

const FORGE_PASS: &str = include_str!("../tests/fixtures/deploygate/forge-pass.json");
const FORGE_FAIL: &str = include_str!("../tests/fixtures/deploygate/forge-fail.json");
const SLITHER_CLEAN: &str = include_str!("../tests/fixtures/deploygate/slither-clean.json");
const SLITHER_HIGH: &str = include_str!("../tests/fixtures/deploygate/slither-high.json");
const SLITHER_CLEAN_SARIF: &str = include_str!("../tests/fixtures/deploygate/slither-clean.sarif");
const SLITHER_HIGH_SARIF: &str = include_str!("../tests/fixtures/deploygate/slither-high.sarif");
const ADERYN_CLEAN: &str =
    include_str!("../tests/fixtures/deploygate/aderyn-clean.handwritten.json");
const ADERYN_HIGH: &str = include_str!("../tests/fixtures/deploygate/aderyn-high.handwritten.json");
const MEDUSA_PASS: &str = include_str!("../tests/fixtures/deploygate/medusa-pass.handwritten.txt");
const MEDUSA_FAIL: &str = include_str!("../tests/fixtures/deploygate/medusa-fail.handwritten.txt");
const MEDUSA_SHORT: &str =
    include_str!("../tests/fixtures/deploygate/medusa-short.handwritten.txt");
// HUP-S6 g3-gate (fan-out 6): REAL captures from the measured macOS arm64 component archives
// (aderyn 0.6.8, medusa 1.5.1) on the rendered hello-mint template ("Lemon Drops", T1 budget).
// `medusa-fail.real.txt` is the same project with the payment check removed.
const ADERYN_CLEAN_REAL: &str = include_str!("../tests/fixtures/deploygate/aderyn-clean.real.json");
const MEDUSA_PASS_REAL: &str = include_str!("../tests/fixtures/deploygate/medusa-pass.real.txt");
const MEDUSA_FAIL_REAL: &str = include_str!("../tests/fixtures/deploygate/medusa-fail.real.txt");
const ANVIL_RECEIPT: &str = include_str!("../tests/fixtures/deploygate/anvil-receipt.json");
const ANVIL_TX: &str = include_str!("../tests/fixtures/deploygate/anvil-tx.json");
const INITCODE: &str = include_str!("../tests/fixtures/deploygate/initcode.hex");

/// The hello-mint creation bytecode: the fixture initcode minus its one 32-byte constructor arg.
fn split_fixture() -> (String, String) {
    let ic = INITCODE.trim().trim_start_matches("0x");
    let (code, args) = ic.split_at(ic.len() - 64);
    (format!("0x{code}"), format!("0x{args}"))
}

fn ran(output: &str) -> ToolRun {
    ToolRun::Ran {
        output: output.to_string(),
        duration_ms: 1234,
        tool_version: Some("test".into()),
    }
}

fn compiler() -> CompilerSettings {
    CompilerSettings {
        solc_version: "0.8.28".into(),
        optimizer: true,
        optimizer_runs: 200,
        evm_version: "cancun".into(),
        via_ir: false,
    }
}

fn anvil_tx_input() -> String {
    let v: serde_json::Value = serde_json::from_str(ANVIL_TX).expect("fixture json");
    v["input"].as_str().expect("input").to_string()
}

/// Every gate item green, on the real hello-mint bytecode.
fn green_inputs() -> GateInputs {
    let (code, args) = split_fixture();
    GateInputs {
        bytecode_hex: code,
        constructor_args_hex: Some(args),
        compiler: compiler(),
        forge_tests: ran(FORGE_PASS),
        slither: ran(SLITHER_CLEAN),
        aderyn: ran(ADERYN_CLEAN),
        medusa: MedusaInput {
            run: ran(MEDUSA_PASS),
            call_budget: 50_000,
        },
        fork_dry_run: Some(ForkDryRunInput {
            run: ran(ANVIL_RECEIPT),
            tx_input_hex: anvil_tx_input(),
            citrate_precompiles: PrecompileUse::None,
        }),
        fork_in_core: None,
    }
}

/// The caller-supplied fork input of `inp` (green_inputs always has one).
fn fork_mut(inp: &mut GateInputs) -> &mut ForkDryRunInput {
    inp.fork_dry_run.as_mut().expect("fork input")
}

fn item(rec: &GateRecord, id: GateItemId) -> &GateItem {
    rec.items.iter().find(|i| i.id == id).expect("item present")
}

fn failing(rec: &GateRecord) -> Vec<GateItemId> {
    rec.items.iter().filter(|i| !i.pass).map(|i| i.id).collect()
}

// ---------------------------------------------------------------- hashing / binding

#[test]
fn initcode_hash_is_keccak256_of_bytecode_then_args() {
    // `cast keccak <initcode>` on the fixture printed this value when it was captured.
    let ic = hex::decode(INITCODE.trim().trim_start_matches("0x")).expect("hex");
    assert_eq!(
        initcode_hash(&ic),
        "0xaa5f2337da9c808fb2ad5e1f956315f065d21e8369c061776c3bb6933ce0a1c6"
    );
}

#[test]
fn binding_hash_changes_with_compiler_settings_and_bytecode() {
    let ic = hex::decode(INITCODE.trim().trim_start_matches("0x")).expect("hex");
    let base = binding_hash(&ic, &compiler()).expect("binds");
    let mut c2 = compiler();
    c2.optimizer_runs = 201;
    assert_ne!(base, binding_hash(&ic, &c2).expect("binds"));
    let mut c3 = compiler();
    c3.evm_version = "paris".into();
    assert_ne!(base, binding_hash(&ic, &c3).expect("binds"));
    let mut c4 = compiler();
    c4.via_ir = true;
    assert_ne!(base, binding_hash(&ic, &c4).expect("binds"));
    let mut ic2 = ic.clone();
    let last = ic2.len() - 1;
    ic2[last] ^= 1;
    assert_ne!(base, binding_hash(&ic2, &compiler()).expect("binds"));
    // Deterministic.
    assert_eq!(base, binding_hash(&ic, &compiler()).expect("binds"));
}

#[test]
fn compiler_settings_reject_values_that_could_alias_the_canonical_form() {
    let ic = [0x60u8, 0x00];
    for bad in ["", "0.8.28;evm=x", "v0.8", "0.8.28 "] {
        let mut c = compiler();
        c.solc_version = bad.into();
        assert!(
            binding_hash(&ic, &c).is_err(),
            "solc version {bad:?} must be rejected"
        );
    }
    for bad in ["", "cancun;x", "Cancun!"] {
        let mut c = compiler();
        c.evm_version = bad.into();
        assert!(
            binding_hash(&ic, &c).is_err(),
            "evm version {bad:?} must be rejected"
        );
    }
}

// ---------------------------------------------------------------- the all-green case

#[test]
fn all_green_real_outputs_give_ready_with_evidence() {
    let rec = evaluate(&green_inputs(), 42).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::Ready, "failing: {:?}", failing(&rec));
    assert_eq!(rec.items.len(), 5);
    assert_eq!(rec.evaluated_at_ms, 42);
    assert_eq!(
        rec.initcode_hash,
        "0xaa5f2337da9c808fb2ad5e1f956315f065d21e8369c061776c3bb6933ce0a1c6"
    );
    // Every item carries evidence: a digest of the raw output and the run time.
    for it in &rec.items {
        assert!(
            it.evidence
                .output_sha256
                .as_deref()
                .is_some_and(|d| d.len() == 64),
            "{:?}",
            it.id
        );
        assert_eq!(it.evidence.duration_ms, Some(1234));
    }
    let forge = item(&rec, GateItemId::ForgeTests);
    assert_eq!(forge.evidence.counts.get("passed"), Some(&2));
    assert_eq!(forge.evidence.counts.get("failed"), Some(&0));
    let sl = item(&rec, GateItemId::Slither);
    assert_eq!(sl.evidence.counts.get("high"), Some(&0));
    assert_eq!(sl.evidence.counts.get("low"), Some(&1));
    let md = item(&rec, GateItemId::Medusa);
    assert_eq!(md.evidence.counts.get("calls"), Some(&50_000));
    assert_eq!(md.evidence.counts.get("call_budget"), Some(&50_000));
    let fork = item(&rec, GateItemId::ForkDryRun);
    assert!(
        fork.reason
            .contains("0x5fbdb2315678afecb367f032d93f642f64180aa3"),
        "{}",
        fork.reason
    );
}

#[test]
fn record_digest_is_sha256_of_the_raw_output() {
    use sha2::Digest;
    let rec = evaluate(&green_inputs(), 0).expect("evaluates");
    let want = hex::encode(sha2::Sha256::digest(SLITHER_CLEAN.as_bytes()));
    assert_eq!(
        item(&rec, GateItemId::Slither)
            .evidence
            .output_sha256
            .as_deref(),
        Some(want.as_str())
    );
}

// ---------------------------------------------------------------- "not installed" is a FAIL

#[test]
fn not_installed_is_a_fail_for_every_item_never_a_pass() {
    for id in [
        GateItemId::ForgeTests,
        GateItemId::Slither,
        GateItemId::Aderyn,
        GateItemId::Medusa,
        GateItemId::ForkDryRun,
    ] {
        let mut inp = green_inputs();
        match id {
            GateItemId::ForgeTests => inp.forge_tests = ToolRun::NotInstalled,
            GateItemId::Slither => inp.slither = ToolRun::NotInstalled,
            GateItemId::Aderyn => inp.aderyn = ToolRun::NotInstalled,
            GateItemId::Medusa => inp.medusa.run = ToolRun::NotInstalled,
            GateItemId::ForkDryRun => fork_mut(&mut inp).run = ToolRun::NotInstalled,
        }
        let rec = evaluate(&inp, 0).expect("evaluates");
        assert_eq!(
            rec.verdict,
            Verdict::NotReady,
            "{id:?} not installed must not be READY"
        );
        assert_eq!(failing(&rec), vec![id]);
        let it = item(&rec, id);
        assert!(it.reason.contains("not installed"), "{}", it.reason);
        assert_eq!(it.evidence.output_sha256, None);
    }
}

#[test]
fn a_tool_error_is_a_fail_with_its_message() {
    let mut inp = green_inputs();
    inp.aderyn = ToolRun::Error {
        message: "exit status 101".into(),
    };
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::NotReady);
    assert!(item(&rec, GateItemId::Aderyn)
        .reason
        .contains("exit status 101"));
}

// ---------------------------------------------------------------- per-tool parsers

#[test]
fn forge_failure_is_not_ready_and_names_the_count() {
    let mut inp = green_inputs();
    inp.forge_tests = ran(FORGE_FAIL);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::ForgeTests]);
    let f = item(&rec, GateItemId::ForgeTests);
    assert_eq!(f.evidence.counts.get("failed"), Some(&1));
    assert_eq!(f.evidence.counts.get("passed"), Some(&2));
    assert!(f.reason.contains("1 failed"), "{}", f.reason);
}

#[test]
fn forge_with_no_tests_or_junk_output_fails() {
    for out in [
        "{}",
        "not json",
        "[]",
        r#"{"a":{"test_results":{"t()":{"status":"Weird"}}}}"#,
    ] {
        let mut inp = green_inputs();
        inp.forge_tests = ran(out);
        let rec = evaluate(&inp, 0).expect("evaluates");
        assert_eq!(
            failing(&rec),
            vec![GateItemId::ForgeTests],
            "output {out:?}"
        );
    }
}

#[test]
fn slither_high_fails_in_json_and_sarif() {
    let mut inp = green_inputs();
    inp.slither = ran(SLITHER_HIGH);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Slither]);
    assert_eq!(
        item(&rec, GateItemId::Slither).evidence.counts.get("high"),
        Some(&1)
    );
    assert!(item(&rec, GateItemId::Slither).reason.contains("1 High"));

    let mut inp = green_inputs();
    inp.slither = ran(SLITHER_HIGH_SARIF);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Slither]);
    assert_eq!(
        item(&rec, GateItemId::Slither).evidence.counts.get("high"),
        Some(&1)
    );

    let mut inp = green_inputs();
    inp.slither = ran(SLITHER_CLEAN_SARIF);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(
        rec.verdict,
        Verdict::Ready,
        "clean SARIF passes: {:?}",
        failing(&rec)
    );
}

#[test]
fn slither_unsuccessful_run_or_foreign_sarif_fails() {
    let mut inp = green_inputs();
    inp.slither = ran(r#"{"success": false, "error": "compilation failed", "results": {}}"#);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Slither]);
    assert!(item(&rec, GateItemId::Slither)
        .reason
        .contains("compilation failed"));

    let foreign = SLITHER_CLEAN_SARIF.replace("\"Slither\"", "\"SomethingElse\"");
    let mut inp = green_inputs();
    inp.slither = ran(&foreign);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Slither]);
}

#[test]
fn aderyn_high_fails_and_inconsistent_counts_fail() {
    let mut inp = green_inputs();
    inp.aderyn = ran(ADERYN_HIGH);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Aderyn]);
    assert_eq!(
        item(&rec, GateItemId::Aderyn).evidence.counts.get("high"),
        Some(&1)
    );

    // issue_count says 0 but a High issue is listed: refuse to trust the report.
    let lying = ADERYN_HIGH.replace("\"high\": 1", "\"high\": 0");
    let mut inp = green_inputs();
    inp.aderyn = ran(&lying);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Aderyn]);

    let mut inp = green_inputs();
    inp.aderyn = ran(r#"{"files_summary": {}}"#);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Aderyn]);
}

#[test]
fn medusa_failed_property_or_short_campaign_fails() {
    let mut inp = green_inputs();
    inp.medusa.run = ran(MEDUSA_FAIL);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Medusa]);
    assert!(item(&rec, GateItemId::Medusa).reason.contains("1 failed"));

    // Stopped at 17,114 calls of a 50,000-call budget: not a finished campaign.
    let mut inp = green_inputs();
    inp.medusa.run = ran(MEDUSA_SHORT);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Medusa]);
    assert_eq!(
        item(&rec, GateItemId::Medusa).evidence.counts.get("calls"),
        Some(&17_114)
    );

    // No summary line at all.
    let mut inp = green_inputs();
    inp.medusa.run = ran("⇾ fuzz: elapsed: 9s, calls: 50000 (5104/sec)\n");
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Medusa]);
}

#[test]
fn medusa_budget_below_the_floor_fails_even_if_met() {
    let mut inp = green_inputs();
    inp.medusa.call_budget = MIN_MEDUSA_CALL_BUDGET - 1;
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Medusa]);
}

#[test]
fn medusa_parser_strips_terminal_colour_codes() {
    let coloured = MEDUSA_PASS
        .replace("[PASSED]", "\u{1b}[32m[PASSED]\u{1b}[0m")
        .replace(
            "Test summary: 1 test(s) passed",
            "Test summary: \u{1b}[1m1\u{1b}[0m test(s) passed",
        );
    let mut inp = green_inputs();
    inp.medusa.run = ran(&coloured);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::Ready, "{:?}", failing(&rec));
}

#[test]
fn fork_dry_run_must_deploy_exactly_this_initcode() {
    let mut inp = green_inputs();
    // The dry run deployed a different constructor argument.
    let other = anvil_tx_input().replace("00000000aa", "00000000bb");
    fork_mut(&mut inp).tx_input_hex = other;
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::ForkDryRun]);
    assert!(item(&rec, GateItemId::ForkDryRun)
        .reason
        .contains("different bytecode"));
}

#[test]
fn fork_dry_run_reverted_receipt_fails() {
    let mut inp = green_inputs();
    fork_mut(&mut inp).run = ran(&ANVIL_RECEIPT.replace("\"status\":\"0x1\"", "\"status\":\"0x0\""));
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::ForkDryRun]);
    let mut inp = green_inputs();
    fork_mut(&mut inp).run = ran(&ANVIL_RECEIPT.replace(
        "\"contractAddress\":\"0x5fbdb2315678afecb367f032d93f642f64180aa3\"",
        "\"contractAddress\":null",
    ));
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::ForkDryRun]);
}

#[test]
fn citrate_precompiles_used_or_unknown_cannot_be_ready_on_an_anvil_fork() {
    for p in [PrecompileUse::Used, PrecompileUse::Unknown] {
        let mut inp = green_inputs();
        fork_mut(&mut inp).citrate_precompiles = p;
        let rec = evaluate(&inp, 0).expect("evaluates");
        assert_eq!(failing(&rec), vec![GateItemId::ForkDryRun], "{p:?}");
        assert!(item(&rec, GateItemId::ForkDryRun)
            .reason
            .contains("precompile"));
    }
}

#[test]
fn precompile_call_site_in_bytecode_fails_even_when_declared_none() {
    // PUSH2 0x0107 GAS STATICCALL — a call into the Citrate tensor-commit precompile.
    let code = hex::decode("6101075afa").expect("hex");
    assert_eq!(precompile_call_sites(&code), vec!["0x0107".to_string()]);
    // The same bytes as PUSH data (inside a PUSH3) are not an instruction.
    assert!(precompile_call_sites(&hex::decode("6261010700fa").expect("hex")).is_empty());
    // 0x0120 used as a memory offset (MLOAD) is not a call site.
    assert!(precompile_call_sites(&hex::decode("61012051").expect("hex")).is_empty());
    // PUSH20 of the 0x1000 model precompile then GAS CALL.
    let mut p20 = vec![0x73];
    p20.extend_from_slice(&[0u8; 18]);
    p20.extend_from_slice(&[0x10, 0x00]);
    p20.extend_from_slice(&[0x5a, 0xf1]);
    assert_eq!(precompile_call_sites(&p20), vec!["0x1000".to_string()]);

    // Wire it into a gate: declared None, but the bytecode carries a call site.
    let mut inp = green_inputs();
    let (code_hex, _) = split_fixture();
    let tainted = format!("{code_hex}6101075afa");
    inp.bytecode_hex = tainted.clone();
    fork_mut(&mut inp).tx_input_hex = format!(
        "{}{}",
        tainted,
        inp.constructor_args_hex
            .clone()
            .unwrap_or_default()
            .trim_start_matches("0x")
    );
    let rec = evaluate(&inp, 0).expect("evaluates");
    let fork = item(&rec, GateItemId::ForkDryRun);
    assert!(!fork.pass);
    assert!(fork.reason.contains("0x0107"), "{}", fork.reason);
}

#[test]
fn malformed_bytecode_is_an_error_not_a_record() {
    let mut inp = green_inputs();
    inp.bytecode_hex = "0xzz".into();
    assert!(evaluate(&inp, 0).is_err());
    let mut inp = green_inputs();
    inp.bytecode_hex = "".into();
    assert!(evaluate(&inp, 0).is_err());
}

// ---------------------------------------------------------------- the store + deploy refusal

fn initcode_of(inp: &GateInputs) -> Vec<u8> {
    let code = hex::decode(inp.bytecode_hex.trim_start_matches("0x")).expect("hex");
    let args = hex::decode(
        inp.constructor_args_hex
            .as_deref()
            .unwrap_or("")
            .trim_start_matches("0x"),
    )
    .expect("hex");
    crate::contract_deploy::deploy_initcode(&code, &args)
}

#[test]
fn store_refuses_without_a_record_and_names_the_hash() {
    let store = GateStore::default();
    let ic = initcode_of(&green_inputs());
    let err = store.require_ready(&ic).expect_err("no record → refused");
    assert!(
        err.contains("0xaa5f2337da9c808fb2ad5e1f956315f065d21e8369c061776c3bb6933ce0a1c6"),
        "{err}"
    );
    assert!(err.to_lowercase().contains("deploy gate"), "{err}");
}

#[test]
fn store_ready_record_allows_exactly_that_initcode() {
    let store = GateStore::default();
    let inp = green_inputs();
    let rec = evaluate(&inp, 1).expect("evaluates");
    store.record(rec.clone()).expect("records");
    let ic = initcode_of(&inp);
    let got = store.require_ready(&ic).expect("READY for this initcode");
    assert_eq!(got.binding_hash, rec.binding_hash);

    // Any change of bytecode (one byte, or a different constructor arg) has no READY.
    let mut ic2 = ic.clone();
    let last = ic2.len() - 1;
    ic2[last] ^= 0xff;
    assert!(store.require_ready(&ic2).is_err());
    let mut ic3 = ic.clone();
    ic3.push(0);
    assert!(store.require_ready(&ic3).is_err());
}

#[test]
fn store_not_ready_refusal_names_every_failing_item() {
    let store = GateStore::default();
    let mut inp = green_inputs();
    inp.aderyn = ToolRun::NotInstalled;
    inp.slither = ran(SLITHER_HIGH);
    store
        .record(evaluate(&inp, 1).expect("evaluates"))
        .expect("records");
    let err = store
        .require_ready(&initcode_of(&inp))
        .expect_err("NOT READY never deploys");
    assert!(err.contains("NOT READY"), "{err}");
    assert!(err.contains("Slither"), "{err}");
    assert!(err.contains("Aderyn"), "{err}");
    assert!(err.contains("not installed"), "{err}");
    assert!(
        !err.contains("Forge"),
        "passing items are not named as failing: {err}"
    );
}

#[test]
fn a_later_not_ready_record_revokes_an_earlier_ready_for_the_same_hash() {
    let store = GateStore::default();
    let inp = green_inputs();
    store
        .record(evaluate(&inp, 1).expect("evaluates"))
        .expect("records");
    assert!(store.require_ready(&initcode_of(&inp)).is_ok());
    let mut bad = green_inputs();
    bad.forge_tests = ran(FORGE_FAIL);
    store
        .record(evaluate(&bad, 2).expect("evaluates"))
        .expect("records");
    assert!(
        store.require_ready(&initcode_of(&inp)).is_err(),
        "latest evaluation wins"
    );
}

#[test]
fn store_is_bounded_and_evicts_the_oldest() {
    let store = GateStore::default();
    for i in 0..(MAX_GATE_RECORDS as u64 + 5) {
        let mut rec = evaluate(&green_inputs(), i).expect("evaluates");
        rec.initcode_hash = format!("0x{i:064x}");
        store.record(rec).expect("records");
    }
    assert_eq!(store.len(), MAX_GATE_RECORDS);
    assert!(
        store.get(&format!("0x{:064x}", 0u64)).is_none(),
        "oldest evicted"
    );
    assert!(store
        .get(&format!("0x{:064x}", MAX_GATE_RECORDS as u64 + 4))
        .is_some());
}

#[test]
fn gate_record_serializes_camel_case_for_the_ui() {
    let rec = evaluate(&green_inputs(), 7).expect("evaluates");
    let v = serde_json::to_value(&rec).expect("serializes");
    assert_eq!(v["verdict"], "READY");
    assert!(v["initcodeHash"].is_string());
    assert!(v["bindingHash"].is_string());
    assert_eq!(v["compiler"]["solcVersion"], "0.8.28");
    assert_eq!(v["items"][0]["id"], "forge_tests");
    assert!(v["items"][0]["evidence"]["outputSha256"].is_string());
    let mut inp = green_inputs();
    inp.medusa.run = ToolRun::NotInstalled;
    let v = serde_json::to_value(evaluate(&inp, 7).expect("evaluates")).expect("serializes");
    assert_eq!(v["verdict"], "NOT_READY");
}

#[test]
fn gate_inputs_deserialize_from_the_documented_json_shape() {
    let j = serde_json::json!({
        "bytecodeHex": "0x6000",
        "compiler": { "solcVersion": "0.8.28", "optimizer": true, "optimizerRuns": 200, "evmVersion": "cancun" },
        "forgeTests": { "state": "ran", "output": "{}", "durationMs": 5 },
        "slither": { "state": "notInstalled" },
        "aderyn": { "state": "error", "message": "boom" },
        "medusa": { "run": { "state": "notInstalled" }, "callBudget": 50000 },
        "forkDryRun": { "run": { "state": "notInstalled" }, "txInputHex": "0x6000", "citratePrecompiles": "unknown" }
    });
    let inp: GateInputs = serde_json::from_value(j).expect("deserializes");
    assert_eq!(inp.constructor_args_hex, None);
    assert!(!inp.compiler.via_ir);
    assert_eq!(inp.slither, ToolRun::NotInstalled);
    assert_eq!(
        inp.fork_dry_run.as_ref().map(|f| f.citrate_precompiles),
        Some(PrecompileUse::Unknown)
    );
    assert_eq!(inp.fork_in_core, None);
}

#[test]
fn slither_sarif_security_severity_alone_marks_high() {
    // A rule id whose prefix says Medium but whose security-severity is High still counts as High.
    let bumped = SLITHER_CLEAN_SARIF.replace(
        "\"security-severity\": \"3.0\"",
        "\"security-severity\": \"8.0\"",
    );
    assert_ne!(
        bumped, SLITHER_CLEAN_SARIF,
        "fixture carries a security-severity"
    );
    let mut inp = green_inputs();
    inp.slither = ran(&bumped);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Slither]);
}

// ---------------------------------------------------------------- open ceremonies + revocation

fn real_ceremony_view(c: &crate::ceremony::SignatureCeremony) -> crate::ceremony::CeremonyView {
    c.request(crate::ceremony::SignatureIntent {
        origin: "local-user".into(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw: r#"{"from":"0x1111111111111111111111111111111111111111","value":"0x0","data":"0x6000","gas":"0x5208","chainId":"0x9d0c"}"#.into(),
    })
}

#[test]
fn open_ceremony_refuses_without_ready_and_never_calls_the_opener() {
    let store = GateStore::default();
    let ic = initcode_of(&green_inputs());
    let mut called = false;
    let r = store.open_ceremony(
        &ic,
        || {
            called = true;
            Err("must not be reached".into())
        },
        |_| {},
    );
    assert!(r.is_err());
    assert!(!called, "no ceremony is opened for an ungated bytecode");
}

#[test]
fn a_not_ready_record_rejects_the_open_ceremonies_for_that_hash() {
    let store = GateStore::default();
    let cer = crate::ceremony::SignatureCeremony::new();
    let inp = green_inputs();
    store
        .record_and_revoke(evaluate(&inp, 1).expect("evaluates"), |_| {
            panic!("READY revokes nothing")
        })
        .expect("records");
    let ic = initcode_of(&inp);
    let (rec, view) = store
        .open_ceremony(&ic, || Ok(real_ceremony_view(&cer)), |_| {})
        .expect("opens");
    assert_eq!(rec.verdict, Verdict::Ready);
    assert!(cer.status(&view.id).is_some(), "pending");

    // A READY re-run leaves the ceremony alone.
    store
        .record_and_revoke(evaluate(&inp, 2).expect("evaluates"), |_| {
            panic!("READY revokes nothing")
        })
        .expect("records");
    assert!(cer.status(&view.id).is_some(), "still pending");

    // A NOT READY re-run for the same hash rejects it: it can no longer be approved.
    let mut bad = green_inputs();
    bad.slither = ran(SLITHER_HIGH);
    let mut revoked = Vec::new();
    store
        .record_and_revoke(evaluate(&bad, 3).expect("evaluates"), |id| {
            revoked.push(id.to_string());
            let _ = cer.reject(id);
        })
        .expect("records");
    assert_eq!(revoked, vec![view.id.clone()]);
    assert!(
        cer.status(&view.id).is_none(),
        "rejected ceremonies are no longer pending"
    );
}

#[test]
fn a_not_ready_record_for_another_hash_revokes_nothing() {
    let store = GateStore::default();
    let cer = crate::ceremony::SignatureCeremony::new();
    let inp = green_inputs();
    store
        .record_and_revoke(evaluate(&inp, 1).expect("evaluates"), |_| {})
        .expect("records");
    let (_, view) = store
        .open_ceremony(&initcode_of(&inp), || Ok(real_ceremony_view(&cer)), |_| {})
        .expect("opens");
    let mut other = green_inputs();
    other.constructor_args_hex = Some(format!("0x{}", "00".repeat(31) + "bb"));
    other.forge_tests = ToolRun::NotInstalled;
    store
        .record_and_revoke(evaluate(&other, 2).expect("evaluates"), |_| {
            panic!("different hash")
        })
        .expect("records");
    assert!(cer.status(&view.id).is_some());
}

#[test]
fn eviction_revokes_the_evicted_hash_ceremonies() {
    let store = GateStore::default();
    let inp = green_inputs();
    store
        .record_and_revoke(evaluate(&inp, 0).expect("evaluates"), |_| {})
        .expect("records");
    let (_, _view) = store
        .open_ceremony(
            &initcode_of(&inp),
            || {
                Ok(real_ceremony_view(
                    &crate::ceremony::SignatureCeremony::new(),
                ))
            },
            |_| {},
        )
        .expect("opens");
    let mut revoked = 0;
    for i in 1..=(MAX_GATE_RECORDS as u64) {
        let mut rec = evaluate(&green_inputs(), i).expect("evaluates");
        rec.initcode_hash = format!("0x{i:064x}");
        store
            .record_and_revoke(rec, |_| revoked += 1)
            .expect("records");
    }
    assert_eq!(revoked, 1, "the evicted record's open ceremony is revoked");
}

#[test]
fn open_ceremony_refuses_a_not_ready_record_and_never_calls_the_opener() {
    let store = GateStore::default();
    let mut inp = green_inputs();
    inp.medusa.run = ToolRun::NotInstalled;
    store
        .record(evaluate(&inp, 1).expect("evaluates"))
        .expect("records");
    let mut called = false;
    let err = store
        .open_ceremony(
            &initcode_of(&inp),
            || {
                called = true;
                Err("must not be reached".into())
            },
            |_| {},
        )
        .expect_err("NOT READY never opens a ceremony");
    assert!(!called);
    assert!(err.contains("Medusa campaign"), "{err}");
}

#[test]
fn a_not_ready_record_rejects_every_open_ceremony_even_past_the_per_hash_bound() {
    // Opening more deploy ceremonies for one hash than the store tracks must not leave an
    // untracked one pending: a later NOT READY has to reach every ceremony for that hash.
    let store = GateStore::default();
    let cer = crate::ceremony::SignatureCeremony::new();
    let inp = green_inputs();
    store
        .record(evaluate(&inp, 1).expect("evaluates"))
        .expect("records");
    let ic = initcode_of(&inp);
    let mut ids = Vec::new();
    for _ in 0..(MAX_OPEN_PER_HASH + 3) {
        let (_, view) = store
            .open_ceremony(
                &ic,
                || Ok(real_ceremony_view(&cer)),
                |id| {
                    let _ = cer.reject(id);
                },
            )
            .expect("opens");
        ids.push(view.id);
    }
    let mut bad = green_inputs();
    bad.slither = ran(SLITHER_HIGH);
    store
        .record_and_revoke(evaluate(&bad, 2).expect("evaluates"), |id| {
            let _ = cer.reject(id);
        })
        .expect("records");
    for id in &ids {
        assert!(
            cer.status(id).is_none(),
            "ceremony {id} is still pending after NOT READY"
        );
    }
}

#[test]
fn slither_sarif_rule_id_prefix_alone_marks_high() {
    // With no security-severity on the rule, the `0-` rule-id prefix alone still means High.
    let stripped = SLITHER_HIGH_SARIF.replace(
        "\"security-severity\": \"8.0\"",
        "\"security-severity\": \"0.0\"",
    );
    assert_ne!(
        stripped, SLITHER_HIGH_SARIF,
        "fixture carries a High security-severity"
    );
    let mut inp = green_inputs();
    inp.slither = ran(&stripped);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Slither]);
    assert_eq!(
        item(&rec, GateItemId::Slither).evidence.counts.get("high"),
        Some(&1)
    );
}

#[test]
fn free_text_from_the_verifier_is_bounded_in_the_record() {
    // The error message and tool version are verifier-supplied text that is stored in the
    // record, shown on the card and quoted in the refusal: keep them short.
    let mut inp = green_inputs();
    inp.aderyn = ToolRun::Error {
        message: "é".repeat(5_000),
    };
    inp.slither = ToolRun::Ran {
        output: SLITHER_CLEAN.to_string(),
        duration_ms: 1,
        tool_version: Some("v".repeat(5_000)),
    };
    let rec = evaluate(&inp, 0).expect("evaluates");
    let reason = &item(&rec, GateItemId::Aderyn).reason;
    assert!(reason.chars().count() <= 400, "{}", reason.len());
    assert!(reason.ends_with('…'), "truncation is marked");
    let version = item(&rec, GateItemId::Slither)
        .evidence
        .tool_version
        .clone()
        .expect("version kept");
    assert!(version.chars().count() <= MAX_TOOL_TEXT_CHARS + 1);
}

// ---------------------------------------------------------------- real aderyn + medusa captures

#[test]
fn real_aderyn_and_medusa_captures_pass_the_gate() {
    let mut inp = green_inputs();
    inp.aderyn = ran(ADERYN_CLEAN_REAL);
    inp.medusa.run = ran(MEDUSA_PASS_REAL);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(rec.verdict, Verdict::Ready, "failing: {:?}", failing(&rec));
    let ad = item(&rec, GateItemId::Aderyn);
    assert_eq!(ad.evidence.counts.get("high"), Some(&0));
    assert_eq!(ad.evidence.counts.get("low"), Some(&7));
    let md = item(&rec, GateItemId::Medusa);
    // The last progress line before the T1 test limit (50,000 calls) halted the campaign.
    assert_eq!(md.evidence.counts.get("calls"), Some(&81_117));
    assert_eq!(md.evidence.counts.get("passed"), Some(&14));
    assert_eq!(md.evidence.counts.get("failed"), Some(&0));
}

#[test]
fn a_real_failed_medusa_campaign_names_the_failed_tests() {
    let mut inp = green_inputs();
    inp.medusa.run = ran(MEDUSA_FAIL_REAL);
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert_eq!(failing(&rec), vec![GateItemId::Medusa]);
    let r = &item(&rec, GateItemId::Medusa).reason;
    assert!(r.starts_with("2 failed"), "{r}");
    assert!(
        r.contains("LemonDropsProperties.property_payments_are_accounted()")
            && r.contains("LemonDropsProperties.property_wrong_payment_never_accepted()"),
        "the finding is named: {r}"
    );
}

#[test]
fn a_forge_failure_names_the_failing_test_and_its_reason() {
    let mut inp = green_inputs();
    inp.forge_tests = ran(FORGE_FAIL);
    let rec = evaluate(&inp, 0).expect("evaluates");
    let r = &item(&rec, GateItemId::ForgeTests).reason;
    assert!(r.starts_with("1 failed"), "{r}");
    assert!(r.contains("BrokenTest.test_wrongSupply()"), "{r}");
    assert!(r.contains("supply"), "the revert reason rides along: {r}");
}

#[test]
fn named_failures_are_bounded() {
    // 40 failing tests with long reasons: the reason names a few and counts the rest.
    let mut results = serde_json::Map::new();
    for i in 0..40 {
        results.insert(
            format!("test_{i}()"),
            serde_json::json!({"status": "Failure", "reason": "x".repeat(2_000)}),
        );
    }
    let out = serde_json::json!({"test/T.t.sol:T": {"test_results": results}}).to_string();
    let mut inp = green_inputs();
    inp.forge_tests = ran(&out);
    let rec = evaluate(&inp, 0).expect("evaluates");
    let r = &item(&rec, GateItemId::ForgeTests).reason;
    assert!(r.starts_with("40 failed"), "{r}");
    assert!(r.contains("and 37 more"), "{r}");
    assert!(r.chars().count() <= 1_200, "{}", r.chars().count());

    let mut log = String::from("fuzz: elapsed: 9s, calls: 50000 (5104/sec)\n");
    for i in 0..40 {
        log.push_str(&format!("[FAILED] Property Test: P.property_{i}_{}()\n", "y".repeat(500)));
    }
    log.push_str("Test summary: 0 test(s) passed, 40 test(s) failed\n");
    let mut inp = green_inputs();
    inp.medusa.run = ran(&log);
    let rec = evaluate(&inp, 0).expect("evaluates");
    let r = &item(&rec, GateItemId::Medusa).reason;
    assert!(r.starts_with("40 failed"), "{r}");
    assert!(r.contains("and 37 more"), "{r}");
    assert!(r.chars().count() <= 1_200, "{}", r.chars().count());
}

#[test]
fn many_distinct_failed_medusa_lines_parse_in_linear_time() {
    // Up to MAX_OUTPUT_BYTES of tool output reaches the parser; collecting the names of failed
    // properties must not compare every new name against every earlier one.
    let n = 80_000;
    let mut log = String::from("fuzz: elapsed: 9s, calls: 50000 (5104/sec)\n");
    for i in 0..n {
        log.push_str(&format!("[FAILED] Property Test: P.property_{i}()\n"));
    }
    log.push_str(&format!("Test summary: 0 test(s) passed, {n} test(s) failed\n"));
    assert!(log.len() < MAX_OUTPUT_BYTES);
    let mut inp = green_inputs();
    inp.medusa.run = ran(&log);
    let t = std::time::Instant::now();
    let rec = evaluate(&inp, 0).expect("evaluates");
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "parsing took {:?}",
        t.elapsed()
    );
    let r = &item(&rec, GateItemId::Medusa).reason;
    assert!(r.starts_with(&format!("{n} failed")), "{r}");
    assert!(r.contains(&format!("and {} more", n - 3)), "{r}");
}
