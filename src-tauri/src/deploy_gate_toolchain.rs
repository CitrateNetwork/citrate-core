//! HUP-S6.3 → HUP-S6.4 (retro A27) — Hermes's toolchain runs into the D-4 deploy gate.
//!
//! **One source of truth.** The deploy gate ([`crate::deploy_gate`]) is the only place a deploy
//! verdict is decided. The sidecar's toolchain tools (forge_test, slither_scan, aderyn_scan,
//! medusa_fuzz) judge their own output only to move a workflow step along; for a deploy, this
//! module fetches each run's **raw report** from the sidecar and hands it to
//! [`crate::deploy_gate::evaluate`], which parses it again with the gate's parsers. Nothing the
//! sidecar concluded is taken over.
//!
//! **Binding.** A gate record is bound to one init code. This module ties the four reports to it:
//!
//! 1. the bytecode is read from the forge artifact the member names (`out/<File>.sol/<Name>.json`
//!    inside a folder they granted for reading), and must be exactly what the forge_test run
//!    built (the sidecar recorded the artifact's bytecode digest after that run);
//! 2. every report must come from the same state of the project's sources (the sidecar took a
//!    digest before and after each run; a run whose sources changed while it ran has none);
//! 3. the compiler settings come from the artifact's own metadata.
//!
//! A report that is missing, came from other sources, or describes a tool that is not installed
//! becomes a failing item, so the record is NOT READY and names why. It is never skipped.
//!
//! **Tier budgets (HUP-S6.9).** The Medusa item is held to this machine's tier budget from the
//! bundled `templates/medusa-budgets.json` (T0/T1/T2 = 10k/50k/200k calls, starting values pending
//! owner sign-off), not to whatever budget the run was started with: the campaign must have run
//! at least that many calls, and its line coverage of the project's `src/` files (from medusa's
//! own lcov report) must reach the tier's minimum.
//!
//! **Fork dry run.** This module does not produce the fork dry run (lane CH-fork does). The
//! caller passes its result; without one, that item fails.
//!
//! Rule 3: nothing here signs or holds a key.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use citrate_templates::MedusaBudget;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::deploy_gate::{
    CompilerSettings, ForkDryRunInput, GateInputs, GateRecord, MedusaInput, ToolRun,
};

/// The four sidecar tool names, in gate order.
pub const FORGE_TEST: &str = "forge_test";
pub const SLITHER_SCAN: &str = "slither_scan";
pub const ADERYN_SCAN: &str = "aderyn_scan";
pub const MEDUSA_FUZZ: &str = "medusa_fuzz";
/// The largest artifact JSON read.
const MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;

/// The raw facts of one run, as the sidecar keeps them (`toolchain_reports::GateReport`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GateReport {
    pub project: String,
    pub output: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub sources_sha256: Option<String>,
    #[serde(default)]
    pub test_limit: Option<u64>,
    #[serde(default)]
    pub coverage_lcov: Option<String>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, String>,
}

/// One kept report (`GET /sessions/:id/toolchain/reports`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StoredReport {
    pub seq: u64,
    pub tool: String,
    pub project: String,
    /// `completed`, `not_installed`, `timed_out`, `refused` or `failed`.
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub gate: Option<GateReport>,
}

#[derive(Debug, Deserialize)]
struct ReportsBody {
    reports: Vec<StoredReport>,
}

/// What the member (or the hello-mint flow) asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolchainGateRequest {
    /// The Hermes session whose toolchain runs to use.
    pub session_id: String,
    /// The Foundry project folder (absolute).
    pub project: String,
    /// The artifact under `out/`, e.g. `Token.sol/LemonDrops.json`.
    pub artifact: String,
    #[serde(default)]
    pub constructor_args_hex: Option<String>,
    /// The fork dry run for this init code, when one was produced (lane CH-fork).
    #[serde(default)]
    pub fork_dry_run: Option<ForkDryRunInput>,
}

/// How the Medusa item was held to the tier budget.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MedusaBudgetCheck {
    pub tier: String,
    /// The calls the campaign had to reach (the tier's test limit).
    pub required_calls: u64,
    pub min_coverage_pct: u8,
    /// The budget the run was started with.
    pub run_test_limit: Option<u64>,
    pub lines_hit: Option<u64>,
    pub lines_total: Option<u64>,
    pub coverage_pct: Option<f64>,
    /// Why the run was not accepted against the budget (empty when it was).
    pub problems: Vec<String>,
}

/// What `deploy_gate_submit_toolchain` returns.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainGateResult {
    pub record: GateRecord,
    pub artifact: String,
    /// The source digest the four reports share, when they do.
    pub sources_sha256: Option<String>,
    pub medusa: MedusaBudgetCheck,
}

// ------------------------------------------------------------------------ pure parts

/// `Token.sol/LemonDrops.json`: exactly two plain parts, a `.sol` folder and a `.json` file.
pub fn check_artifact_name(artifact: &str) -> Result<(), String> {
    let p = Path::new(artifact);
    let parts: Vec<_> = p.components().collect();
    let plain = parts.iter().all(|c| matches!(c, Component::Normal(_)));
    let ok = plain
        && parts.len() == 2
        && artifact.ends_with(".json")
        && artifact
            .split('/')
            .next()
            .is_some_and(|d| d.ends_with(".sol"));
    if ok {
        Ok(())
    } else {
        Err("the artifact must look like File.sol/Contract.json (a file under out/)".into())
    }
}

/// The SHA-256 of a creation bytecode, the way the sidecar records it: over the lower-case hex
/// without `0x`.
pub fn bytecode_digest(object: &str) -> Option<String> {
    let t = object.trim();
    let t = t.strip_prefix("0x").unwrap_or(t);
    if t.is_empty() || !t.len().is_multiple_of(2) || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex::encode(sha2::Sha256::digest(
        t.to_ascii_lowercase().as_bytes(),
    )))
}

/// The bytecode and compiler settings in a forge artifact.
pub fn read_artifact(json: &str) -> Result<(String, CompilerSettings), String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "the artifact is not forge JSON".to_string())?;
    let object = v
        .pointer("/bytecode/object")
        .and_then(|o| o.as_str())
        .ok_or("the artifact has no creation bytecode")?;
    if bytecode_digest(object).is_none() {
        return Err(
            "the artifact's creation bytecode is empty (an interface or abstract contract?)".into(),
        );
    }
    let m = v
        .get("metadata")
        .ok_or("the artifact has no compiler metadata")?;
    let version = m
        .pointer("/compiler/version")
        .and_then(|s| s.as_str())
        .ok_or("the artifact names no compiler version")?;
    let solc_version = version.split('+').next().unwrap_or(version).to_string();
    let settings = m
        .get("settings")
        .ok_or("the artifact has no compiler settings")?;
    let optimizer = settings
        .pointer("/optimizer/enabled")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let optimizer_runs = settings
        .pointer("/optimizer/runs")
        .and_then(|r| r.as_u64())
        .and_then(|r| u32::try_from(r).ok())
        .unwrap_or(200);
    let evm_version = settings
        .get("evmVersion")
        .and_then(|e| e.as_str())
        .ok_or("the artifact names no EVM version")?
        .to_string();
    let via_ir = settings
        .get("viaIR")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    Ok((
        object.to_string(),
        CompilerSettings {
            solc_version,
            optimizer,
            optimizer_runs,
            evm_version,
            via_ir,
        },
    ))
}

/// Line coverage of `project/src/` from an lcov report: (lines hit, lines with code).
pub fn src_line_coverage(lcov: &str, project: &str) -> (u64, u64) {
    let src = format!("{}/src/", project.trim_end_matches('/'));
    let (mut hit, mut total) = (0u64, 0u64);
    let mut in_src = false;
    for line in lcov.lines() {
        if let Some(f) = line.strip_prefix("SF:") {
            in_src = f.starts_with(&src);
        } else if line == "end_of_record" {
            in_src = false;
        } else if let Some(rest) = line.strip_prefix("DA:") {
            if !in_src {
                continue;
            }
            let count = rest
                .split(',')
                .nth(1)
                .and_then(|c| c.trim().parse::<u64>().ok());
            if let Some(c) = count {
                total += 1;
                if c > 0 {
                    hit += 1;
                }
            }
        }
    }
    (hit, total)
}

/// The latest report of `tool`.
fn latest<'a>(reports: &'a [StoredReport], tool: &str) -> Option<&'a StoredReport> {
    reports
        .iter()
        .filter(|r| r.tool == tool)
        .max_by_key(|r| r.seq)
}

/// A report as the gate's tool input: the raw output when the program ran, honest otherwise.
fn tool_run(rep: Option<&StoredReport>, tool: &str) -> ToolRun {
    match rep {
        None => ToolRun::Error {
            message: format!("{tool} has not run on this project in this Hermes session"),
        },
        Some(r) if r.status == "not_installed" => ToolRun::NotInstalled,
        Some(r) => match (&r.gate, r.status.as_str()) {
            (Some(g), "completed") => ToolRun::Ran {
                output: g.output.clone(),
                duration_ms: g.duration_ms,
                tool_version: None,
            },
            _ => ToolRun::Error {
                message: format!("{tool} {}: {}", r.status.replace('_', " "), r.summary),
            },
        },
    }
}

/// Everything [`gate_inputs`] needs.
pub struct GateBuild<'a> {
    pub reports: &'a [StoredReport],
    /// The canonical project folder.
    pub project: &'a str,
    pub artifact: &'a str,
    pub artifact_json: &'a str,
    pub constructor_args_hex: Option<&'a str>,
    pub tier: &'a str,
    pub budget: &'a MedusaBudget,
    pub fork_dry_run: Option<ForkDryRunInput>,
}

/// Build the gate's input from the sidecar's raw reports. `Err` only when the artifact itself is
/// unusable; every tool problem becomes a failing item.
pub fn gate_inputs(
    b: GateBuild<'_>,
) -> Result<(GateInputs, Option<String>, MedusaBudgetCheck), String> {
    check_artifact_name(b.artifact)?;
    let (bytecode, compiler) = read_artifact(b.artifact_json)?;
    let reports: Vec<StoredReport> = b
        .reports
        .iter()
        .filter(|r| r.project == b.project)
        .cloned()
        .collect();
    let forge = latest(&reports, FORGE_TEST);
    let reference = forge
        .and_then(|r| r.gate.as_ref())
        .and_then(|g| g.sources_sha256.clone());

    let mut forge_run = tool_run(forge, FORGE_TEST);
    if let (ToolRun::Ran { .. }, Some(f)) = (&forge_run, forge) {
        let built = f.gate.as_ref().and_then(|g| g.artifacts.get(b.artifact));
        let now = bytecode_digest(&bytecode);
        forge_run = if reference.is_none() {
            ToolRun::Error {
                message: "the project's sources changed while forge test ran (or could not be read), so its result is not bound to them".into(),
            }
        } else if built.is_none() {
            ToolRun::Error {
                message: format!("forge test did not build {}", b.artifact),
            }
        } else if built != now.as_ref() {
            ToolRun::Error {
                message: format!(
                    "the bytecode in {} is not the bytecode forge test built",
                    b.artifact
                ),
            }
        } else {
            forge_run
        };
    }

    // Every other report must come from the sources forge tested.
    let bound = |tool: &str| -> ToolRun {
        let rep = latest(&reports, tool);
        let run = tool_run(rep, tool);
        if !matches!(run, ToolRun::Ran { .. }) {
            return run;
        }
        let theirs = rep
            .and_then(|r| r.gate.as_ref())
            .and_then(|g| g.sources_sha256.clone());
        match (&reference, theirs) {
            (Some(a), Some(t)) if *a == t => run,
            (None, _) => ToolRun::Error {
                message: format!(
                    "{tool} cannot be bound: forge test has no source digest for this project"
                ),
            },
            _ => ToolRun::Error {
                message: format!("{tool} ran on different sources than forge test; run it again"),
            },
        }
    };
    let slither = bound(SLITHER_SCAN);
    let aderyn = bound(ADERYN_SCAN);
    let mut medusa_run = bound(MEDUSA_FUZZ);

    // HUP-S6.9: hold the campaign to this machine's tier budget.
    let medusa_rep = latest(&reports, MEDUSA_FUZZ).and_then(|r| r.gate.as_ref());
    let mut check = MedusaBudgetCheck {
        tier: b.tier.to_string(),
        required_calls: b.budget.test_limit,
        min_coverage_pct: b.budget.min_coverage_pct,
        run_test_limit: medusa_rep.and_then(|g| g.test_limit),
        lines_hit: None,
        lines_total: None,
        coverage_pct: None,
        problems: Vec::new(),
    };
    if matches!(medusa_run, ToolRun::Ran { .. }) {
        match medusa_rep.and_then(|g| g.coverage_lcov.as_deref()) {
            None => check
                .problems
                .push("the campaign wrote no coverage report".to_string()),
            Some(lcov) => {
                let (hit, total) = src_line_coverage(lcov, b.project);
                check.lines_hit = Some(hit);
                check.lines_total = Some(total);
                if total == 0 {
                    check
                        .problems
                        .push("the coverage report covers no line of src/".to_string());
                } else {
                    let pct = (hit as f64) * 100.0 / (total as f64);
                    check.coverage_pct = Some((pct * 10.0).round() / 10.0);
                    if pct < f64::from(b.budget.min_coverage_pct) {
                        check.problems.push(format!(
                            "line coverage of src/ is {:.1}% ({hit} of {total} lines), below the {} minimum of {}%",
                            pct, b.tier, b.budget.min_coverage_pct
                        ));
                    }
                }
            }
        }
        if !check.problems.is_empty() {
            medusa_run = ToolRun::Error {
                message: check.problems.join("; "),
            };
        }
    }

    let fork_dry_run = b.fork_dry_run.unwrap_or(ForkDryRunInput {
        run: ToolRun::Error {
            message: "no fork dry run was produced for this bytecode yet".into(),
        },
        tx_input_hex: String::new(),
        citrate_precompiles: crate::deploy_gate::PrecompileUse::Unknown,
    });

    let inputs = GateInputs {
        bytecode_hex: bytecode,
        constructor_args_hex: b.constructor_args_hex.map(str::to_string),
        compiler,
        forge_tests: forge_run,
        slither,
        aderyn,
        // The gate checks the calls against the tier budget, not the run's own budget.
        medusa: MedusaInput {
            run: medusa_run,
            call_budget: b.budget.test_limit,
        },
        fork_dry_run,
    };
    Ok((inputs, reference, check))
}

/// Parse the sidecar's answer.
pub fn parse_reports(status: u16, body: &str) -> Result<Vec<StoredReport>, String> {
    match status {
        200 => serde_json::from_str::<ReportsBody>(body)
            .map(|b| b.reports)
            .map_err(|_| "Hermes answered with something that is not a report list".to_string()),
        404 => Err("that Hermes session is gone or has no toolchain (turn the toolchain on in Settings, then restart Hermes)".to_string()),
        s => Err(format!("Hermes refused the report request ({s})")),
    }
}

/// A session id as the sidecar mints it (letters, digits, `-`).
fn check_session_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then_some(())
        .ok_or_else(|| "not a Hermes session id".to_string())
}

/// `project` percent-encoded for a query string.
fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The whole hand-over over a sidecar link: fetch, bind, build. `project` must already be
/// canonical and granted.
pub fn build_from_sidecar(
    link: &dyn crate::web_signin::SidecarLink,
    req: &ToolchainGateRequest,
    project: &Path,
    tier: &str,
    budget: &MedusaBudget,
) -> Result<(GateInputs, Option<String>, MedusaBudgetCheck), String> {
    check_session_id(&req.session_id)?;
    check_artifact_name(&req.artifact)?;
    let project_s = project.to_string_lossy().into_owned();
    let path = format!(
        "/sessions/{}/toolchain/reports?project={}",
        req.session_id,
        query_escape(&project_s)
    );
    let (status, body) = link.get(&path)?;
    let reports = parse_reports(status, &body)?;
    let artifact_path: PathBuf = project.join("out").join(&req.artifact);
    let meta = std::fs::symlink_metadata(&artifact_path)
        .map_err(|_| format!("{} was not found; run forge test first", req.artifact))?;
    if !meta.is_file() || meta.len() > MAX_ARTIFACT_BYTES {
        return Err(format!("{} is not a forge artifact file", req.artifact));
    }
    let artifact_json = std::fs::read_to_string(&artifact_path)
        .map_err(|e| format!("cannot read {}: {}", req.artifact, e.kind()))?;
    gate_inputs(GateBuild {
        reports: &reports,
        project: &project_s,
        artifact: &req.artifact,
        artifact_json: &artifact_json,
        constructor_args_hex: req.constructor_args_hex.as_deref(),
        tier,
        budget,
        fork_dry_run: req.fork_dry_run.clone(),
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn now_secs() -> u64 {
    now_ms() / 1000
}

/// **Command — deploy_gate_submit_toolchain.** Run the deploy gate on a Hermes session's
/// toolchain reports for one forge artifact, store the record (replacing any earlier record for
/// the same init code; a NOT READY one rejects any deploy ceremony still open for it) and return
/// it with the tier-budget check.
#[tauri::command]
pub async fn deploy_gate_submit_toolchain(
    app_h: tauri::AppHandle,
    request: ToolchainGateRequest,
) -> Result<ToolchainGateResult, String> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        let store = crate::agent_grants::GrantStore::for_app(&app_h)?;
        let project = crate::template_forge::require_grant(
            &store,
            Path::new(&request.project),
            crate::agent_grants::Access::Read,
            now_secs(),
        )?;
        let tier = crate::template_forge::renderer_tier(
            crate::tier::tier_recommend_sync(app_h.clone())?.effective,
        );
        let root =
            crate::template_forge::templates_root(app_h.path().resource_dir().ok().as_deref())?;
        let budgets = citrate_templates::MedusaBudgets::load(&root)?;
        let budget = budgets.for_tier(tier).clone();
        let mgr = crate::hermes::manager(&app_h)?;
        let (inputs, sources, medusa) =
            build_from_sidecar(mgr, &request, &project, tier.id(), &budget)?;
        let st = app_h
            .try_state::<crate::deploy_gate::DeployGateState>()
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let cer = app_h
            .try_state::<crate::ceremony::CeremonyState>()
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let rec = crate::deploy_gate::evaluate(&inputs, now_ms())?;
        // An already-decided ceremony cannot be rejected again; that error is expected and moot.
        st.0.record_and_revoke(rec.clone(), |id| {
            let _ = cer.0.reject(id);
        })?;
        Ok(ToolchainGateResult {
            record: rec,
            artifact: request.artifact.clone(),
            sources_sha256: sources,
            medusa,
        })
    })
    .await
}

#[cfg(test)]
#[path = "deploy_gate_toolchain_tests.rs"]
mod tests;
