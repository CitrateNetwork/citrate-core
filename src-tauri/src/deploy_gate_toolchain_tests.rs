// HUP-S6.3 → S6.4 (retro A27): Hermes's toolchain reports into the D-4 deploy gate.
//
// The slither, aderyn and medusa outputs are REAL captures (slither 0.11.6, aderyn 0.6.8,
// medusa 1.5.1) of the exact argv the sidecar's toolchain tools run, on a rendered erc20 template
// and on the same contract with an injected `selfdestruct` anyone can call (2026-10-04; scratch
// paths rewritten to /work/erc20). The same bytes are in citrate-agent-runtime
// `agent-loop/tests/fixtures/toolchain/captured/`, where the runtime's step verifiers must agree.
// The forge report is the real capture already used by the gate tests; the artifact is the real
// forge 1.5.1 artifact of the rendered erc20, trimmed to its ABI, bytecode and metadata.
use super::*;
use crate::deploy_gate::{evaluate, initcode_hash, GateItemId, PrecompileUse, Verdict};

const SLITHER_CLEAN: &str =
    include_str!("../tests/fixtures/toolchain-captured/slither-erc20-clean.sarif");
const SLITHER_SUICIDAL: &str =
    include_str!("../tests/fixtures/toolchain-captured/slither-erc20-suicidal.sarif");
const ADERYN_CLEAN: &str =
    include_str!("../tests/fixtures/toolchain-captured/aderyn-erc20-clean.txt");
const ADERYN_SELFDESTRUCT: &str =
    include_str!("../tests/fixtures/toolchain-captured/aderyn-erc20-selfdestruct.txt");
const MEDUSA_T0: &str = include_str!("../tests/fixtures/toolchain-captured/medusa-erc20-T0.txt");
const MEDUSA_LCOV: &str = include_str!("../tests/fixtures/toolchain-captured/medusa-erc20-T0.lcov");
const ARTIFACT: &str =
    include_str!("../tests/fixtures/toolchain-captured/erc20-LemonDrops.artifact.json");
const FORGE_PASS: &str = include_str!("../tests/fixtures/deploygate/forge-pass.json");
const FORGE_FAIL: &str = include_str!("../tests/fixtures/deploygate/forge-fail.json");
const ANVIL_RECEIPT: &str = include_str!("../tests/fixtures/deploygate/anvil-receipt.json");

const PROJECT: &str = "/work/erc20";
const ART: &str = "Token.sol/LemonDrops.json";
const SOURCES: &str = "5ea1";

fn budget(test_limit: u64, min_coverage_pct: u8) -> MedusaBudget {
    MedusaBudget {
        test_limit,
        workers: 2,
        call_sequence_length: 50,
        timeout_secs: 1200,
        coverage_plateau_calls: 2500,
        min_coverage_pct,
    }
}

fn t0() -> MedusaBudget {
    budget(10_000, 60)
}

fn artifact_bytecode() -> String {
    let v: serde_json::Value = serde_json::from_str(ARTIFACT).expect("artifact json");
    v["bytecode"]["object"]
        .as_str()
        .expect("bytecode")
        .to_string()
}

fn report(seq: u64, tool: &str, output: &str) -> StoredReport {
    let mut artifacts = BTreeMap::new();
    if tool == FORGE_TEST {
        artifacts.insert(
            ART.to_string(),
            bytecode_digest(&artifact_bytecode()).expect("digest"),
        );
    }
    StoredReport {
        seq,
        tool: tool.to_string(),
        project: PROJECT.to_string(),
        status: "completed".into(),
        summary: "ran".into(),
        gate: Some(GateReport {
            project: PROJECT.to_string(),
            output: output.to_string(),
            duration_ms: 1000,
            sources_sha256: Some(SOURCES.into()),
            test_limit: (tool == MEDUSA_FUZZ).then_some(10_000),
            coverage_lcov: (tool == MEDUSA_FUZZ).then(|| MEDUSA_LCOV.to_string()),
            artifacts,
        }),
    }
}

/// Four clean, bound reports for the rendered erc20.
fn clean_reports() -> Vec<StoredReport> {
    vec![
        report(1, FORGE_TEST, FORGE_PASS),
        report(2, SLITHER_SCAN, SLITHER_CLEAN),
        report(3, ADERYN_SCAN, ADERYN_CLEAN),
        report(4, MEDUSA_FUZZ, MEDUSA_T0),
    ]
}

fn fork_for_artifact() -> ForkDryRunInput {
    ForkDryRunInput {
        run: ToolRun::Ran {
            output: ANVIL_RECEIPT.to_string(),
            duration_ms: 10,
            tool_version: None,
        },
        tx_input_hex: artifact_bytecode(),
        citrate_precompiles: PrecompileUse::None,
    }
}

fn build(
    reports: &[StoredReport],
    fork: Option<ForkDryRunInput>,
    b: &MedusaBudget,
) -> (GateInputs, Option<String>, MedusaBudgetCheck) {
    gate_inputs(GateBuild {
        reports,
        project: PROJECT,
        artifact: ART,
        artifact_json: ARTIFACT,
        constructor_args_hex: None,
        tier: "T0",
        budget: b,
        fork_dry_run: fork,
    })
    .expect("gate inputs")
}

fn item(rec: &crate::deploy_gate::GateRecord, id: GateItemId) -> crate::deploy_gate::GateItem {
    rec.items
        .iter()
        .find(|i| i.id == id)
        .cloned()
        .expect("item")
}

// ------------------------------------------------------------------------ the whole path

#[test]
fn a_clean_rendered_erc20_with_a_fork_dry_run_is_ready_and_bound_to_its_bytecode() {
    let (inputs, sources, medusa) = build(&clean_reports(), Some(fork_for_artifact()), &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let failing: Vec<_> = rec
        .failing()
        .map(|i| format!("{}: {}", i.label, i.reason))
        .collect();
    assert_eq!(rec.verdict, Verdict::Ready, "{failing:?}");
    let code = hex::decode(artifact_bytecode().trim_start_matches("0x")).expect("hex");
    assert_eq!(rec.initcode_hash, initcode_hash(&code));
    assert_eq!(rec.compiler.solc_version, "0.8.36");
    assert_eq!(rec.compiler.evm_version, "cancun");
    assert!(rec.compiler.optimizer);
    assert_eq!(rec.compiler.optimizer_runs, 200);
    assert_eq!(sources.as_deref(), Some(SOURCES));
    assert_eq!(medusa.coverage_pct, Some(100.0));
    assert!(medusa.problems.is_empty());
    // The gate re-parsed the raw reports itself: its own counts, not the sidecar's.
    assert_eq!(
        item(&rec, GateItemId::Medusa).evidence.counts.get("calls"),
        Some(&16_099)
    );
    assert_eq!(
        item(&rec, GateItemId::Aderyn).evidence.counts.get("low"),
        Some(&3)
    );
}

#[test]
fn without_a_fork_dry_run_the_record_is_not_ready_and_says_so() {
    let (inputs, _, _) = build(&clean_reports(), None, &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert_eq!(rec.verdict, Verdict::NotReady);
    let failing: Vec<_> = rec.failing().collect();
    assert_eq!(failing.len(), 1);
    assert_eq!(failing[0].id, GateItemId::ForkDryRun);
    assert!(
        failing[0].reason.contains("no fork dry run"),
        "{}",
        failing[0].reason
    );
}

#[test]
fn an_injected_selfdestruct_is_not_ready_on_slither_and_aderyn() {
    let mut reps = clean_reports();
    reps[1] = report(2, SLITHER_SCAN, SLITHER_SUICIDAL);
    reps[2] = report(3, ADERYN_SCAN, ADERYN_SELFDESTRUCT);
    let (inputs, _, _) = build(&reps, Some(fork_for_artifact()), &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert_eq!(rec.verdict, Verdict::NotReady);
    let slither = item(&rec, GateItemId::Slither);
    assert!(!slither.pass);
    assert!(slither.reason.contains("1 High"), "{}", slither.reason);
    let aderyn = item(&rec, GateItemId::Aderyn);
    assert!(!aderyn.pass);
    assert!(aderyn.reason.contains("1 High"), "{}", aderyn.reason);
}

#[test]
fn a_failing_forge_run_is_not_ready() {
    let mut reps = clean_reports();
    reps[0] = report(1, FORGE_TEST, FORGE_FAIL);
    let (inputs, _, _) = build(&reps, Some(fork_for_artifact()), &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert!(!item(&rec, GateItemId::ForgeTests).pass);
}

// ------------------------------------------------------------------------ binding

#[test]
fn bytecode_forge_did_not_build_fails_the_forge_item() {
    let mut reps = clean_reports();
    if let Some(g) = reps[0].gate.as_mut() {
        g.artifacts.insert(ART.into(), "00".repeat(32));
    }
    let (inputs, _, _) = build(&reps, Some(fork_for_artifact()), &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let forge = item(&rec, GateItemId::ForgeTests);
    assert!(!forge.pass);
    assert!(
        forge.reason.contains("not the bytecode forge test built"),
        "{}",
        forge.reason
    );
}

#[test]
fn an_artifact_forge_never_built_fails_the_forge_item() {
    let mut reps = clean_reports();
    if let Some(g) = reps[0].gate.as_mut() {
        g.artifacts.clear();
    }
    let (inputs, _, _) = build(&reps, None, &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert!(item(&rec, GateItemId::ForgeTests)
        .reason
        .contains("did not build"));
}

#[test]
fn a_scan_of_other_sources_is_not_bound_to_the_forge_run() {
    let mut reps = clean_reports();
    if let Some(g) = reps[1].gate.as_mut() {
        g.sources_sha256 = Some("0ther".into());
    }
    let (inputs, _, _) = build(&reps, Some(fork_for_artifact()), &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let s = item(&rec, GateItemId::Slither);
    assert!(!s.pass);
    assert!(s.reason.contains("different sources"), "{}", s.reason);
}

#[test]
fn a_forge_run_whose_sources_moved_binds_nothing() {
    let mut reps = clean_reports();
    if let Some(g) = reps[0].gate.as_mut() {
        g.sources_sha256 = None;
    }
    let (inputs, sources, _) = build(&reps, Some(fork_for_artifact()), &t0());
    assert!(sources.is_none());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    for id in [
        GateItemId::ForgeTests,
        GateItemId::Slither,
        GateItemId::Aderyn,
        GateItemId::Medusa,
    ] {
        assert!(
            !item(&rec, id).pass,
            "{id:?} must fail without a source binding"
        );
    }
}

#[test]
fn reports_of_another_project_are_ignored() {
    let mut reps = clean_reports();
    for r in &mut reps {
        r.project = "/elsewhere".into();
    }
    let (inputs, _, _) = build(&reps, None, &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert!(item(&rec, GateItemId::ForgeTests)
        .reason
        .contains("has not run on this project"));
}

#[test]
fn the_latest_report_of_a_tool_wins() {
    let mut reps = clean_reports();
    reps.push(report(9, SLITHER_SCAN, SLITHER_SUICIDAL));
    let (inputs, _, _) = build(&reps, Some(fork_for_artifact()), &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert!(!item(&rec, GateItemId::Slither).pass);
}

#[test]
fn a_tool_that_is_not_installed_stays_a_fail() {
    let mut reps = clean_reports();
    reps[2] = StoredReport {
        seq: 3,
        tool: ADERYN_SCAN.into(),
        project: PROJECT.into(),
        status: "not_installed".into(),
        summary: "aderyn is not installed on this machine".into(),
        gate: None,
    };
    let (inputs, _, _) = build(&reps, Some(fork_for_artifact()), &t0());
    assert_eq!(inputs.aderyn, ToolRun::NotInstalled);
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert_eq!(rec.verdict, Verdict::NotReady);
    assert!(item(&rec, GateItemId::Aderyn)
        .reason
        .contains("not installed"));
}

#[test]
fn a_timed_out_run_is_an_error_item_with_its_summary() {
    let mut reps = clean_reports();
    reps[3] = StoredReport {
        seq: 4,
        tool: MEDUSA_FUZZ.into(),
        project: PROJECT.into(),
        status: "timed_out".into(),
        summary: "medusa did not finish within 660s and was stopped".into(),
        gate: None,
    };
    let (inputs, _, _) = build(&reps, Some(fork_for_artifact()), &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let m = item(&rec, GateItemId::Medusa);
    assert!(!m.pass);
    assert!(m.reason.contains("timed out"), "{}", m.reason);
}

// ------------------------------------------------------------------------ S6.9 tier budgets

#[test]
fn the_tier_call_budget_is_what_the_gate_checks() {
    // The captured T0 campaign ran 16,099 calls: enough for T0, not for T1's 50,000.
    let (inputs, _, check) = build(
        &clean_reports(),
        Some(fork_for_artifact()),
        &budget(50_000, 75),
    );
    assert_eq!(inputs.medusa.call_budget, 50_000);
    assert_eq!(check.required_calls, 50_000);
    assert_eq!(check.run_test_limit, Some(10_000));
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let m = item(&rec, GateItemId::Medusa);
    assert!(!m.pass);
    assert!(m.reason.contains("16099 of 50000"), "{}", m.reason);
}

#[test]
fn coverage_below_the_tier_minimum_fails_medusa() {
    let mut reps = clean_reports();
    if let Some(g) = reps[3].gate.as_mut() {
        // Only the OpenZeppelin ERC20 record: src/ is not covered at all.
        g.coverage_lcov =
            Some("SF:/work/erc20/src/Token.sol\nDA:11,1\nDA:12,0\nDA:15,0\nend_of_record\n".into());
    }
    let (inputs, _, check) = build(&reps, Some(fork_for_artifact()), &t0());
    assert_eq!(check.lines_hit, Some(1));
    assert_eq!(check.lines_total, Some(3));
    assert_eq!(check.coverage_pct, Some(33.3));
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let m = item(&rec, GateItemId::Medusa);
    assert!(!m.pass);
    assert!(
        m.reason.contains("below the T0 minimum of 60%"),
        "{}",
        m.reason
    );
}

#[test]
fn a_campaign_without_coverage_fails_medusa() {
    let mut reps = clean_reports();
    if let Some(g) = reps[3].gate.as_mut() {
        g.coverage_lcov = None;
    }
    let (inputs, _, check) = build(&reps, Some(fork_for_artifact()), &t0());
    assert!(!check.problems.is_empty());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert!(item(&rec, GateItemId::Medusa)
        .reason
        .contains("no coverage report"));
}

#[test]
fn coverage_counts_only_the_projects_src_lines() {
    // Library lines (the ERC20 record, 51 lines, mostly unhit) do not count.
    assert_eq!(src_line_coverage(MEDUSA_LCOV, PROJECT), (3, 3));
    assert_eq!(src_line_coverage(MEDUSA_LCOV, "/work/erc20/"), (3, 3));
    assert_eq!(src_line_coverage(MEDUSA_LCOV, "/work"), (0, 0));
    assert_eq!(src_line_coverage("DA:1,1\n", PROJECT), (0, 0));
}

// ------------------------------------------------------------------------ aderyn SARIF in the gate

#[test]
fn the_gate_reads_aderyns_sarif_stdout_with_its_banner() {
    let (inputs, _, _) = build(&clean_reports(), None, &t0());
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let a = item(&rec, GateItemId::Aderyn);
    assert!(a.pass, "{}", a.reason);
    assert_eq!(a.reason, "0 High, 3 Low (SARIF)");
}

#[test]
fn aderyn_sarif_from_another_tool_or_with_an_unknown_level_fails_closed() {
    let other = r#"{"runs":[{"tool":{"driver":{"name":"Slither"}},"results":[]}]}"#;
    let unknown = r#"{"runs":[{"tool":{"driver":{"name":"Aderyn"}},"results":[{"ruleId":"x"}]}]}"#;
    let pass_kind = r#"{"runs":[{"tool":{"driver":{"name":"Aderyn"}},"results":[{"ruleId":"x","kind":"pass","level":"warning"}]}]}"#;
    for (raw, pass) in [(other, false), (unknown, false), (pass_kind, true)] {
        let mut reps = clean_reports();
        reps[2] = report(3, ADERYN_SCAN, raw);
        let (inputs, _, _) = build(&reps, None, &t0());
        let rec = evaluate(&inputs, 1).expect("evaluate");
        assert_eq!(item(&rec, GateItemId::Aderyn).pass, pass, "{raw}");
    }
}

// ------------------------------------------------------------------------ inputs

#[test]
fn artifact_names_are_plain_out_paths() {
    assert!(check_artifact_name(ART).is_ok());
    for bad in [
        "../Token.sol/X.json",
        "/abs/Token.sol/X.json",
        "Token.sol/../../X.json",
        "Token.sol/X.txt",
        "Token/X.json",
        "a/Token.sol/X.json",
        "",
    ] {
        assert!(check_artifact_name(bad).is_err(), "{bad}");
    }
}

#[test]
fn an_artifact_without_bytecode_or_metadata_is_refused() {
    assert!(read_artifact(r#"{"bytecode":{"object":"0x"}}"#).is_err());
    assert!(read_artifact(r#"{"bytecode":{"object":"0x6080"}}"#)
        .unwrap_err()
        .contains("metadata"));
    assert!(read_artifact("not json").is_err());
    let (code, c) = read_artifact(ARTIFACT).expect("artifact");
    assert!(code.starts_with("0x6101"));
    assert!(!c.via_ir);
}

#[test]
fn the_sidecar_answer_is_parsed_honestly() {
    assert!(parse_reports(200, r#"{"reports":[]}"#).unwrap().is_empty());
    assert!(parse_reports(404, "{}")
        .unwrap_err()
        .contains("no toolchain"));
    assert!(parse_reports(500, "{}").unwrap_err().contains("500"));
    assert!(parse_reports(200, "nope").is_err());
}

/// The sidecar over its bearer channel, answering one GET from a table.
struct Link {
    answer: (u16, String),
    asked: std::sync::Mutex<Vec<String>>,
}

impl crate::web_signin::SidecarLink for Link {
    fn get(&self, path: &str) -> Result<(u16, String), String> {
        self.asked
            .lock()
            .map_err(|_| "poisoned".to_string())?
            .push(path.to_string());
        Ok(self.answer.clone())
    }
    fn post(&self, _path: &str, _body: &str) -> Result<(u16, String), String> {
        Err("no posts here".into())
    }
}

fn report_json(r: &StoredReport) -> serde_json::Value {
    let g = r.gate.as_ref().expect("gate");
    serde_json::json!({
        "seq": r.seq, "tool": r.tool, "project": r.project, "status": r.status,
        "summary": r.summary, "captured_at_ms": 1,
        "gate": {
            "project": g.project, "output": g.output, "duration_ms": g.duration_ms,
            "sources_sha256": g.sources_sha256, "test_limit": g.test_limit,
            "coverage_lcov": g.coverage_lcov, "artifacts": g.artifacts,
        }
    })
}

#[test]
fn build_from_sidecar_reads_the_granted_projects_artifact_and_asks_for_that_project() {
    let dir = std::env::temp_dir().join(format!("citrate-gate-tc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out/Token.sol")).expect("mkdir");
    std::fs::write(dir.join("out/Token.sol/LemonDrops.json"), ARTIFACT).expect("write");
    let project = dir.canonicalize().expect("canonical");
    let p = project.to_string_lossy().into_owned();
    let reps: Vec<serde_json::Value> = clean_reports()
        .into_iter()
        .map(|mut r| {
            r.project = p.clone();
            if let Some(g) = r.gate.as_mut() {
                g.project = p.clone();
                g.coverage_lcov = g
                    .coverage_lcov
                    .as_ref()
                    .map(|l| l.replace("/work/erc20", &p));
            }
            report_json(&r)
        })
        .collect();
    let link = Link {
        answer: (200, serde_json::json!({ "reports": reps }).to_string()),
        asked: std::sync::Mutex::new(vec![]),
    };
    let req = ToolchainGateRequest {
        session_id: "s1-ab".into(),
        project: p.clone(),
        artifact: ART.into(),
        constructor_args_hex: None,
        fork_dry_run: Some(fork_for_artifact()),
    };
    let (inputs, _, check) = build_from_sidecar(&link, &req, &project, "T0", &t0()).expect("build");
    let rec = evaluate(&inputs, 1).expect("evaluate");
    assert_eq!(rec.verdict, Verdict::Ready);
    assert_eq!(check.coverage_pct, Some(100.0));
    let asked = link.asked.lock().expect("lock").clone();
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].starts_with("/sessions/s1-ab/toolchain/reports?project=/"),
        "{}",
        asked[0]
    );
    // A bad session id or artifact name never reaches the sidecar.
    let mut bad = req.clone();
    bad.session_id = "../x".into();
    assert!(build_from_sidecar(&link, &bad, &project, "T0", &t0()).is_err());
    let mut bad = req.clone();
    bad.artifact = "../../etc/passwd".into();
    assert!(build_from_sidecar(&link, &bad, &project, "T0", &t0()).is_err());
    assert_eq!(link.asked.lock().expect("lock").len(), 1);
    // A missing artifact is a plain refusal.
    let mut missing = req;
    missing.artifact = "Other.sol/Other.json".into();
    assert!(build_from_sidecar(&link, &missing, &project, "T0", &t0())
        .unwrap_err()
        .contains("run forge test first"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn query_escape_keeps_paths_readable_and_escapes_the_rest() {
    assert_eq!(query_escape("/a b/c&d=e"), "/a%20b/c%26d%3De");
}

// ------------------------------------------------------------------------ recorded proof

/// The recorded proof run (scripts/forge-gate-proof.sh): real forge, slither, aderyn and medusa
/// outputs captured on a freshly rendered erc20, evaluated by the production bridge and gate.
/// Skips unless CITRATE_FORGE_PROOF_DIR names the run folder.
#[test]
fn recorded_proof_run_when_present() {
    let Ok(dir) = std::env::var("CITRATE_FORGE_PROOF_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    let project = read("project.txt").trim().to_string();
    let artifact = read("artifact.txt").trim().to_string();
    let sources = Some(read("sources.txt").trim().to_string());
    let art_json =
        std::fs::read_to_string(std::path::Path::new(&project).join("out").join(&artifact))
            .expect("artifact");
    let (code, _) = read_artifact(&art_json).expect("artifact");
    let mk = |seq: u64, tool: &str, file: &str| -> StoredReport {
        let mut artifacts = BTreeMap::new();
        if tool == FORGE_TEST {
            artifacts.insert(artifact.clone(), bytecode_digest(&code).expect("digest"));
        }
        StoredReport {
            seq,
            tool: tool.into(),
            project: project.clone(),
            status: "completed".into(),
            summary: "recorded".into(),
            gate: Some(GateReport {
                project: project.clone(),
                output: read(file),
                duration_ms: 0,
                sources_sha256: sources.clone(),
                test_limit: (tool == MEDUSA_FUZZ).then_some(10_000),
                coverage_lcov: (tool == MEDUSA_FUZZ).then(|| read("medusa.lcov")),
                artifacts,
            }),
        }
    };
    let reps = vec![
        mk(1, FORGE_TEST, "forge.out"),
        mk(2, SLITHER_SCAN, "slither.out"),
        mk(3, ADERYN_SCAN, "aderyn.out"),
        mk(4, MEDUSA_FUZZ, "medusa.out"),
    ];
    let fork = std::fs::read_to_string(dir.join("fork-receipt.json"))
        .ok()
        .map(|receipt| ForkDryRunInput {
            run: ToolRun::Ran {
                output: receipt,
                duration_ms: 0,
                tool_version: None,
            },
            tx_input_hex: read("fork-tx-input.txt").trim().to_string(),
            citrate_precompiles: PrecompileUse::None,
        });
    let (inputs, _, check) = gate_inputs(GateBuild {
        reports: &reps,
        project: &project,
        artifact: &artifact,
        artifact_json: &art_json,
        constructor_args_hex: None,
        tier: "T0",
        budget: &t0(),
        fork_dry_run: fork,
    })
    .expect("inputs");
    let rec = evaluate(&inputs, 1).expect("evaluate");
    let out = serde_json::json!({ "record": rec, "medusa": check });
    std::fs::write(
        dir.join("gate-record.json"),
        serde_json::to_string_pretty(&out).expect("json"),
    )
    .expect("write record");
    let expect = read("expect.txt");
    let want = if expect.trim() == "READY" {
        Verdict::Ready
    } else {
        Verdict::NotReady
    };
    assert_eq!(
        rec.verdict,
        want,
        "{}",
        serde_json::to_string_pretty(&rec.items).unwrap_or_default()
    );
}
