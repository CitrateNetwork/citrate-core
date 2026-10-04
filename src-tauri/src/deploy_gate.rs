//! HUP-S6.4 — the **D-4 deploy gate**.
//!
//! A contract deploy is refused unless a READY gate record exists for exactly the init code
//! the creation transaction will carry. A record is READY only when every item passes:
//!
//! | item | passes when |
//! |---|---|
//! | Forge tests | `forge test --json` parsed; at least one test ran; zero failed |
//! | Slither | a successful Slither JSON report (or Slither SARIF) with zero High findings |
//! | Aderyn | an Aderyn JSON report with zero High issues (its two High counts agree) |
//! | Medusa campaign | the summary reports zero failed and at least one passed property test, and the campaign reached its call budget (budget ≥ [`MIN_MEDUSA_CALL_BUDGET`]; the planset names 50,000 calls for template invariants) |
//! | Fork dry run | the dry-run receipt succeeded with a contract address, the dry run deployed exactly this init code, and the contract does not use Citrate precompiles (an anvil fork cannot simulate them) |
//!
//! A tool that is not installed, or that errored, is a FAIL for its item, never a pass.
//! Each item carries evidence: counts, the SHA-256 of the raw tool output, run time and the
//! tool version the runner reported.
//!
//! **Binding.** `initcode_hash = keccak256(initcode)` where `initcode = creation bytecode ‖
//! ABI-encoded constructor args` (the exact `data` of the creation tx, so `cast keccak` on it
//! reproduces the value). The record also carries `binding_hash = keccak256(domain ‖
//! initcode_hash ‖ keccak256(canonical compiler settings))` (planset red-team correction 8:
//! solc version, optimizer, runs, EVM version, via-IR). Records are keyed by `initcode_hash`
//! and the latest evaluation for a hash wins, so a later NOT READY revokes an earlier READY.
//!
//! **Inputs.** The verifiers that run the tools live in the agent runtime (HUP-S6.3). This
//! module defines the typed input they hand over ([`GateInputs`]): the raw outputs, never a
//! pre-computed verdict. Every verdict here comes from a deterministic parser (planset
//! red-team correction 4). Records live in memory only: a restart forgets them and the gate
//! must run again.
//!
//! Rule 3: nothing here signs or holds a key. `contract_deploy` consults [`GateStore`] and
//! only then opens a PENDING SignatureCeremony.
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// The smallest Medusa call budget the gate accepts. Lower budgets are a FAIL even when met.
pub const MIN_MEDUSA_CALL_BUDGET: u64 = 10_000;
/// How many gate records are kept (oldest evicted first).
pub const MAX_GATE_RECORDS: usize = 64;
/// Domain separator of [`binding_hash`].
const BINDING_DOMAIN: &[u8] = b"citrate.deploygate.v1";
/// Raw tool output larger than this is refused as evidence (keeps the store bounded).
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Verifier-supplied free text (an error message, a tool version) kept in a record, in chars.
const MAX_TOOL_TEXT_CHARS: usize = 300;

/// The compiler settings the bytecode was built with. Part of the binding hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilerSettings {
    /// e.g. `0.8.28` (digits and dots only).
    pub solc_version: String,
    pub optimizer: bool,
    pub optimizer_runs: u32,
    /// e.g. `cancun` (lowercase letters and digits only).
    pub evm_version: String,
    #[serde(default)]
    pub via_ir: bool,
}

/// What a verifier reports for one tool run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ToolRun {
    /// The tool ran; `output` is its raw report (JSON, SARIF or log text).
    Ran {
        output: String,
        #[serde(rename = "durationMs")]
        duration_ms: u64,
        #[serde(default, rename = "toolVersion")]
        tool_version: Option<String>,
    },
    /// The tool is not installed on this machine. Always a FAIL.
    NotInstalled,
    /// The tool could not be run, or exited without a report. Always a FAIL.
    Error { message: String },
}

/// The Medusa campaign: its log and the call budget it was configured with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MedusaInput {
    pub run: ToolRun,
    pub call_budget: u64,
}

/// Whether the contract calls Citrate precompiles (declared by the verifier from the source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrecompileUse {
    None,
    Used,
    Unknown,
}

/// The fork dry run: the creation receipt (`cast send --create … --json`), the `input` of the
/// dry-run creation tx (`cast tx <hash> --json`), and the declared precompile use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkDryRunInput {
    pub run: ToolRun,
    pub tx_input_hex: String,
    pub citrate_precompiles: PrecompileUse,
}

/// The typed hand-over from the runtime verifiers (HUP-S6.3) to the core gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateInputs {
    pub bytecode_hex: String,
    #[serde(default)]
    pub constructor_args_hex: Option<String>,
    pub compiler: CompilerSettings,
    pub forge_tests: ToolRun,
    pub slither: ToolRun,
    pub aderyn: ToolRun,
    pub medusa: MedusaInput,
    pub fork_dry_run: ForkDryRunInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateItemId {
    ForgeTests,
    Slither,
    Aderyn,
    Medusa,
    ForkDryRun,
}

impl GateItemId {
    pub fn label(self) -> &'static str {
        match self {
            GateItemId::ForgeTests => "Forge tests",
            GateItemId::Slither => "Slither",
            GateItemId::Aderyn => "Aderyn",
            GateItemId::Medusa => "Medusa campaign",
            GateItemId::ForkDryRun => "Fork dry run",
        }
    }
}

/// The evidence behind one item.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub counts: BTreeMap<String, u64>,
    /// SHA-256 (hex) of the raw tool output; `None` when the tool did not run.
    pub output_sha256: Option<String>,
    pub duration_ms: Option<u64>,
    pub tool_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateItem {
    pub id: GateItemId,
    pub label: String,
    pub pass: bool,
    /// Plain-language reason: what passed, or why it failed.
    pub reason: String,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Ready,
    NotReady,
}

/// One gate evaluation, bound to one init code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateRecord {
    /// `0x` + keccak256(initcode).
    pub initcode_hash: String,
    /// `0x` + keccak256(domain ‖ initcode hash ‖ keccak256(compiler settings)).
    pub binding_hash: String,
    pub compiler: CompilerSettings,
    pub verdict: Verdict,
    pub items: Vec<GateItem>,
    pub evaluated_at_ms: u64,
}

impl GateRecord {
    pub fn failing(&self) -> impl Iterator<Item = &GateItem> {
        self.items.iter().filter(|i| !i.pass)
    }
}

/// What `deploy_gate_lookup` returns: the hash the deploy would carry and its record, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployGateLookup {
    pub initcode_hash: String,
    pub record: Option<GateRecord>,
}

// ------------------------------------------------------------------------ hashing

fn keccak(bytes: &[u8]) -> [u8; 32] {
    let mut h = sha3::Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

/// `0x` + keccak256(initcode).
pub fn initcode_hash(initcode: &[u8]) -> String {
    format!("0x{}", hex::encode(keccak(initcode)))
}

fn canonical_compiler(c: &CompilerSettings) -> Result<String, String> {
    let solc_ok = !c.solc_version.is_empty()
        && c.solc_version
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '.')
        && c.solc_version
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit());
    if !solc_ok {
        return Err(format!(
            "compiler: solc version {:?} is not a version like 0.8.28",
            c.solc_version
        ));
    }
    let evm_ok = !c.evm_version.is_empty()
        && c.evm_version
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit());
    if !evm_ok {
        return Err(format!(
            "compiler: EVM version {:?} is not a name like cancun",
            c.evm_version
        ));
    }
    Ok(format!(
        "solc={};optimizer={};runs={};evm={};viaIR={}",
        c.solc_version, c.optimizer, c.optimizer_runs, c.evm_version, c.via_ir
    ))
}

/// `0x` + keccak256(domain ‖ keccak256(initcode) ‖ keccak256(canonical compiler settings)).
pub fn binding_hash(initcode: &[u8], compiler: &CompilerSettings) -> Result<String, String> {
    let settings = canonical_compiler(compiler)?;
    let mut buf = Vec::with_capacity(BINDING_DOMAIN.len() + 64);
    buf.extend_from_slice(BINDING_DOMAIN);
    buf.extend_from_slice(&keccak(initcode));
    buf.extend_from_slice(&keccak(settings.as_bytes()));
    Ok(format!("0x{}", hex::encode(keccak(&buf))))
}

fn parse_hex(s: &str, what: &str) -> Result<Vec<u8>, String> {
    let t = s.trim();
    let t = t.strip_prefix("0x").unwrap_or(t);
    hex::decode(t).map_err(|e| format!("{what}: not valid hex ({e})"))
}

/// The init code a deploy of (`bytecode_hex`, `constructor_args_hex`) carries.
pub fn initcode_from_hex(
    bytecode_hex: &str,
    constructor_args_hex: Option<&str>,
) -> Result<Vec<u8>, String> {
    let code = parse_hex(bytecode_hex, "bytecode")?;
    if code.is_empty() {
        return Err("contract bytecode is required".into());
    }
    let args = match constructor_args_hex {
        Some(a) => parse_hex(a, "constructor args")?,
        None => Vec::new(),
    };
    Ok(crate::contract_deploy::deploy_initcode(&code, &args))
}

// ------------------------------------------------------------------------ evaluation

/// Evaluate the gate for `inputs`. `Err` only for malformed bytecode or compiler settings;
/// every tool problem becomes a failing item in a NOT READY record.
pub fn evaluate(inputs: &GateInputs, now_ms: u64) -> Result<GateRecord, String> {
    let initcode = initcode_from_hex(&inputs.bytecode_hex, inputs.constructor_args_hex.as_deref())?;
    let binding = binding_hash(&initcode, &inputs.compiler)?;
    let items = vec![
        eval_tool(
            GateItemId::ForgeTests,
            &inputs.forge_tests,
            "forge",
            parse_forge,
        ),
        eval_tool(
            GateItemId::Slither,
            &inputs.slither,
            "slither",
            parse_slither,
        ),
        eval_tool(GateItemId::Aderyn, &inputs.aderyn, "aderyn", parse_aderyn),
        eval_tool(GateItemId::Medusa, &inputs.medusa.run, "medusa", |out| {
            parse_medusa(out, inputs.medusa.call_budget)
        }),
        eval_tool(
            GateItemId::ForkDryRun,
            &inputs.fork_dry_run.run,
            "anvil",
            |out| parse_fork(out, &inputs.fork_dry_run, &initcode),
        ),
    ];
    let verdict = if items.iter().all(|i| i.pass) {
        Verdict::Ready
    } else {
        Verdict::NotReady
    };
    Ok(GateRecord {
        initcode_hash: initcode_hash(&initcode),
        binding_hash: binding,
        compiler: inputs.compiler.clone(),
        verdict,
        items,
        evaluated_at_ms: now_ms,
    })
}

/// A parser's result: pass/fail, a reason, and the counts it found.
struct Parsed {
    pass: bool,
    reason: String,
    counts: BTreeMap<String, u64>,
}

fn fail(reason: impl Into<String>) -> Parsed {
    Parsed {
        pass: false,
        reason: reason.into(),
        counts: BTreeMap::new(),
    }
}

/// `s` cut to [`MAX_TOOL_TEXT_CHARS`] chars, with `…` marking a cut.
fn bounded(s: &str) -> String {
    if s.chars().count() <= MAX_TOOL_TEXT_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX_TOOL_TEXT_CHARS).collect();
    out.push('…');
    out
}

fn eval_tool(
    id: GateItemId,
    run: &ToolRun,
    tool: &str,
    parse: impl FnOnce(&str) -> Parsed,
) -> GateItem {
    let (parsed, evidence) = match run {
        ToolRun::NotInstalled => (
            fail(format!(
                "{tool} is not installed (a missing tool is a fail, never a pass)"
            )),
            Evidence::default(),
        ),
        ToolRun::Error { message } => (
            fail(format!(
                "{tool} did not produce a report: {}",
                bounded(message)
            )),
            Evidence::default(),
        ),
        ToolRun::Ran {
            output,
            duration_ms,
            tool_version,
        } => {
            let digest = hex::encode(sha2::Sha256::digest(output.as_bytes()));
            let parsed = if output.len() > MAX_OUTPUT_BYTES {
                fail(format!(
                    "{tool} output is larger than {MAX_OUTPUT_BYTES} bytes"
                ))
            } else {
                parse(output)
            };
            let ev = Evidence {
                counts: parsed.counts.clone(),
                output_sha256: Some(digest),
                duration_ms: Some(*duration_ms),
                tool_version: tool_version.as_deref().map(bounded),
            };
            (parsed, ev)
        }
    };
    GateItem {
        id,
        label: id.label().to_string(),
        pass: parsed.pass,
        reason: parsed.reason,
        evidence,
    }
}

/// How many failing tests a reason names; the rest are counted.
const MAX_NAMED_FAILURES: usize = 3;
/// Each named failure (test name plus its reason) is cut to this many chars.
const MAX_NAMED_FAILURE_CHARS: usize = 160;

/// `" (a; b; c and N more)"` naming up to [`MAX_NAMED_FAILURES`] failures, each bounded, so the
/// card and the refusal cite the finding itself and not only a count. Empty when none.
fn named_failures(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let cut = |s: &str| {
        if s.chars().count() <= MAX_NAMED_FAILURE_CHARS {
            s.to_string()
        } else {
            let mut o: String = s.chars().take(MAX_NAMED_FAILURE_CHARS).collect();
            o.push('…');
            o
        }
    };
    let shown: Vec<String> = names
        .iter()
        .take(MAX_NAMED_FAILURES)
        .map(|n| cut(n))
        .collect();
    let rest = names.len().saturating_sub(MAX_NAMED_FAILURES);
    let more = if rest > 0 {
        format!(" and {rest} more")
    } else {
        String::new()
    };
    format!(" ({}{more})", shown.join("; "))
}

fn parse_forge(out: &str) -> Parsed {
    let Ok(serde_json::Value::Object(suites)) = serde_json::from_str::<serde_json::Value>(out)
    else {
        return fail("forge output is not a `forge test --json` report");
    };
    let (mut passed, mut failed, mut skipped, mut unknown) = (0u64, 0u64, 0u64, 0u64);
    let mut failures: Vec<String> = Vec::new();
    for (suite_id, suite) in &suites {
        let Some(results) = suite.get("test_results").and_then(|r| r.as_object()) else {
            return fail(
                "forge output is not a `forge test --json` report (a suite has no test_results)",
            );
        };
        // `test/Token.t.sol:LemonDropsTest` → `LemonDropsTest`.
        let contract = suite_id.rsplit(':').next().unwrap_or(suite_id);
        for (test, t) in results {
            match t.get("status").and_then(|s| s.as_str()) {
                Some("Success") => passed += 1,
                Some("Failure") => {
                    failed += 1;
                    let why = t
                        .get("reason")
                        .and_then(|r| r.as_str())
                        .filter(|r| !r.trim().is_empty());
                    failures.push(match why {
                        Some(r) => format!("{contract}.{test}: {}", r.trim()),
                        None => format!("{contract}.{test}"),
                    });
                }
                Some("Skipped") => skipped += 1,
                _ => unknown += 1,
            }
        }
    }
    let counts = BTreeMap::from([
        ("suites".to_string(), suites.len() as u64),
        ("passed".to_string(), passed),
        ("failed".to_string(), failed),
        ("skipped".to_string(), skipped),
        ("unrecognized".to_string(), unknown),
    ]);
    let (pass, reason) = if failed > 0 {
        (
            false,
            format!(
                "{failed} failed{}, {passed} passed",
                named_failures(&failures)
            ),
        )
    } else if unknown > 0 {
        (
            false,
            format!("{unknown} test(s) with an unrecognized status"),
        )
    } else if passed == 0 {
        (false, "no tests ran".to_string())
    } else {
        (
            true,
            format!("{passed} passed, 0 failed, {skipped} skipped"),
        )
    };
    Parsed {
        pass,
        reason,
        counts,
    }
}

const SLITHER_IMPACTS: [&str; 5] = ["High", "Medium", "Low", "Informational", "Optimization"];

fn slither_counts(by_impact: [u64; 5]) -> BTreeMap<String, u64> {
    SLITHER_IMPACTS
        .iter()
        .zip(by_impact)
        .map(|(k, v)| (k.to_lowercase(), v))
        .collect()
}

fn slither_verdict(by_impact: [u64; 5], format: &str) -> Parsed {
    let high = by_impact[0];
    let counts = slither_counts(by_impact);
    if high > 0 {
        Parsed {
            pass: false,
            reason: format!("{high} High finding(s) ({format})"),
            counts,
        }
    } else {
        Parsed {
            pass: true,
            reason: format!(
                "0 High, {} Medium, {} Low ({format})",
                by_impact[1], by_impact[2]
            ),
            counts,
        }
    }
}

fn parse_slither(out: &str) -> Parsed {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(out) else {
        return fail("slither output is not JSON or SARIF");
    };
    if v.get("runs").is_some() {
        return parse_slither_sarif(&v);
    }
    match v.get("success").and_then(|s| s.as_bool()) {
        Some(true) => {}
        Some(false) => {
            let err = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("no error message");
            return fail(format!("slither reported an unsuccessful run: {err}"));
        }
        None => return fail("slither output is not a Slither JSON report"),
    }
    let mut by = [0u64; 5];
    let detectors = v.pointer("/results/detectors").and_then(|d| d.as_array());
    for d in detectors.map(|a| a.as_slice()).unwrap_or(&[]) {
        let impact = d.get("impact").and_then(|i| i.as_str()).unwrap_or("");
        match SLITHER_IMPACTS.iter().position(|k| *k == impact) {
            Some(ix) => by[ix] += 1,
            // An impact we cannot classify is treated as High (fail closed).
            None => by[0] += 1,
        }
    }
    slither_verdict(by, "JSON")
}

/// Slither SARIF encodes impact in the rule id prefix (`<impact>-<confidence>-<check>`, 0 =
/// High) and as `security-severity` on the rule. Either signal of High counts as High.
fn parse_slither_sarif(v: &serde_json::Value) -> Parsed {
    let Some(runs) = v.get("runs").and_then(|r| r.as_array()) else {
        return fail("slither SARIF has no runs");
    };
    if runs.is_empty() {
        return fail("slither SARIF has no runs");
    }
    let mut by = [0u64; 5];
    for run in runs {
        let driver = run.pointer("/tool/driver");
        let name = driver
            .and_then(|d| d.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        if !name.eq_ignore_ascii_case("slither") {
            return fail(format!("SARIF report is from {name:?}, not Slither"));
        }
        let mut severity: HashMap<&str, f64> = HashMap::new();
        for rule in driver
            .and_then(|d| d.get("rules"))
            .and_then(|r| r.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            let id = rule.get("id").and_then(|i| i.as_str()).unwrap_or("");
            let sev = rule.pointer("/properties/security-severity").and_then(|s| {
                s.as_str()
                    .and_then(|x| x.parse::<f64>().ok())
                    .or_else(|| s.as_f64())
            });
            if let Some(sev) = sev {
                severity.insert(id, sev);
            }
        }
        for res in run
            .get("results")
            .and_then(|r| r.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            let rule_id = res.get("ruleId").and_then(|r| r.as_str()).unwrap_or("");
            let prefix = rule_id
                .split('-')
                .next()
                .and_then(|p| p.parse::<usize>().ok());
            let high_by_severity = severity.get(rule_id).is_some_and(|s| *s >= 7.0);
            let ix = match prefix {
                Some(_) if high_by_severity => 0,
                Some(p) if p < 5 => p,
                // Unclassifiable results count as High (fail closed).
                _ => 0,
            };
            by[ix] += 1;
        }
    }
    slither_verdict(by, "SARIF")
}

fn parse_aderyn(out: &str) -> Parsed {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(out) else {
        return fail("aderyn output is not JSON");
    };
    let count_high = v.pointer("/issue_count/high").and_then(|h| h.as_u64());
    let listed_high = v
        .pointer("/high_issues/issues")
        .and_then(|i| i.as_array())
        .map(|a| a.len() as u64);
    let high = match (count_high, listed_high) {
        (None, None) => {
            return fail("aderyn output is not an Aderyn JSON report (no High section)")
        }
        (Some(a), Some(b)) if a != b => {
            return fail(format!(
            "aderyn report is inconsistent: issue_count.high = {a} but {b} High issue(s) listed"
        ))
        }
        (Some(a), _) => a,
        (None, Some(b)) => b,
    };
    let low = v
        .pointer("/issue_count/low")
        .and_then(|l| l.as_u64())
        .or_else(|| {
            v.pointer("/low_issues/issues")
                .and_then(|i| i.as_array())
                .map(|a| a.len() as u64)
        })
        .unwrap_or(0);
    let counts = BTreeMap::from([("high".to_string(), high), ("low".to_string(), low)]);
    if high > 0 {
        Parsed {
            pass: false,
            reason: format!("{high} High issue(s)"),
            counts,
        }
    } else {
        Parsed {
            pass: true,
            reason: format!("0 High, {low} Low"),
            counts,
        }
    }
}

/// Remove ANSI escape sequences (`ESC [ … letter`) from terminal output.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The unsigned integer right after `key` in `line` (digits only), if any.
fn number_after(line: &str, key: &str) -> Option<u64> {
    let at = line.find(key)? + key.len();
    let digits: String = line[at..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Seconds in a Go-style duration like `9s`, `1m30s`, `1h2m3s`.
fn go_duration_secs(s: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut num = String::new();
    let mut any = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u64 = num.parse().ok()?;
        num.clear();
        total = total.checked_add(match c {
            'h' => n.checked_mul(3600)?,
            'm' => n.checked_mul(60)?,
            's' => n,
            _ => return None,
        })?;
        any = true;
    }
    (any && num.is_empty()).then_some(total)
}

fn parse_medusa(out: &str, budget: u64) -> Parsed {
    let text = strip_ansi(out);
    let mut calls: Option<u64> = None;
    let mut elapsed: Option<u64> = None;
    let mut summary: Option<(u64, u64)> = None;
    let mut failures: Vec<String> = Vec::new();
    // Names already collected: a set, so up to MAX_OUTPUT_BYTES of log stays linear.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for line in text.lines() {
        // `⇾ [FAILED] Property Test: LemonDropsProperties.property_x()` names the finding.
        if let Some(rest) = line.split("[FAILED]").nth(1) {
            let name = rest
                .split_once("Test:")
                .map(|(_, n)| n)
                .unwrap_or(rest)
                .trim();
            if !name.is_empty() && seen.insert(name) {
                failures.push(name.to_string());
            }
        }
        if line.contains("fuzz:") {
            if let Some(c) = number_after(line, "calls:") {
                calls = Some(c);
            }
            if let Some(rest) = line.split("elapsed:").nth(1) {
                let tok: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| *c != ',')
                    .collect();
                elapsed = go_duration_secs(tok.trim()).or(elapsed);
            }
        }
        if line.contains("Test summary:") {
            let passed = number_after(line, "Test summary:");
            let failed = line
                .split("passed,")
                .nth(1)
                .and_then(|r| number_after(r, ""));
            if let (Some(p), Some(f)) = (passed, failed) {
                summary = Some((p, f));
            }
        }
    }
    let mut counts = BTreeMap::from([("call_budget".to_string(), budget)]);
    if let Some(c) = calls {
        counts.insert("calls".into(), c);
    }
    if let Some(e) = elapsed {
        counts.insert("elapsed_s".into(), e);
    }
    let Some((passed, failed)) = summary else {
        return Parsed {
            pass: false,
            reason: "medusa log has no test summary (campaign did not finish)".into(),
            counts,
        };
    };
    counts.insert("passed".into(), passed);
    counts.insert("failed".into(), failed);
    let calls_done = calls.unwrap_or(0);
    let reason = if budget < MIN_MEDUSA_CALL_BUDGET {
        Some(format!(
            "call budget {budget} is below the minimum of {MIN_MEDUSA_CALL_BUDGET}"
        ))
    } else if failed > 0 {
        Some(format!(
            "{failed} failed{}, {passed} passed property test(s)",
            named_failures(&failures)
        ))
    } else if passed == 0 {
        Some("no property tests ran".to_string())
    } else if calls_done < budget {
        Some(format!(
            "campaign stopped at {calls_done} of {budget} calls"
        ))
    } else {
        None
    };
    match reason {
        Some(r) => Parsed {
            pass: false,
            reason: r,
            counts,
        },
        None => Parsed {
            pass: true,
            reason: format!("{passed} passed, 0 failed, {calls_done} calls (budget {budget})"),
            counts,
        },
    }
}

/// Citrate precompile addresses (chain `precompiles/mod.rs` + the executor's model/artifact/
/// governance precompiles): 0x0100–0x013F, 0x0200–0x0209, 0x1000, 0x1002, 0x1003.
fn is_citrate_precompile(v: u64) -> bool {
    (0x0100..=0x013F).contains(&v)
        || (0x0200..=0x0209).contains(&v)
        || matches!(v, 0x1000 | 0x1002 | 0x1003)
}

/// Best-effort scan of init code for the call-site pattern `PUSHn <citrate precompile> GAS
/// CALL|CALLCODE|DELEGATECALL|STATICCALL`. Returns the addresses found (`0x%04x`). It only ever
/// adds a failure; a clean scan is not proof the contract avoids precompiles (the verifier's
/// declaration covers the rest).
pub fn precompile_call_sites(code: &[u8]) -> Vec<String> {
    // (opcode, pushed value when it is a PUSH whose value fits in a u64)
    let mut ins: Vec<(u8, Option<u64>)> = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let op = code[pc];
        if (0x60..=0x7f).contains(&op) {
            let n = (op - 0x5f) as usize;
            let end = (pc + 1 + n).min(code.len());
            let data = &code[pc + 1..end];
            let lead = data.len().saturating_sub(8);
            let value = if data[..lead].iter().all(|b| *b == 0) {
                Some(
                    data[lead..]
                        .iter()
                        .fold(0u64, |acc, b| (acc << 8) | u64::from(*b)),
                )
            } else {
                None
            };
            ins.push((op, value));
            pc = end;
        } else {
            ins.push((op, None));
            pc += 1;
        }
    }
    let mut found: Vec<String> = Vec::new();
    for w in ins.windows(3) {
        let (push, gas, call) = (w[0], w[1], w[2]);
        if let Some(v) = push.1 {
            if (0x60..=0x7f).contains(&push.0)
                && is_citrate_precompile(v)
                && gas.0 == 0x5a
                && matches!(call.0, 0xf1 | 0xf2 | 0xf4 | 0xfa)
            {
                let a = format!("0x{v:04x}");
                if !found.contains(&a) {
                    found.push(a);
                }
            }
        }
    }
    found
}

fn hex_u64(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => {
            let t = s.strip_prefix("0x").unwrap_or(s);
            u64::from_str_radix(t, 16).ok()
        }
        _ => None,
    }
}

fn parse_fork(out: &str, input: &ForkDryRunInput, initcode: &[u8]) -> Parsed {
    let Ok(receipt) = serde_json::from_str::<serde_json::Value>(out) else {
        return fail("fork dry-run output is not a receipt JSON");
    };
    let mut counts = BTreeMap::new();
    let mut problems: Vec<String> = Vec::new();
    let status = receipt.get("status").and_then(hex_u64);
    if status != Some(1) {
        problems.push("the dry-run creation reverted or has no status".into());
    }
    let address = receipt
        .get("contractAddress")
        .and_then(|a| a.as_str())
        .map(str::to_string);
    if address.is_none() {
        problems.push("the dry-run receipt has no contract address".into());
    }
    if let Some(g) = receipt.get("gasUsed").and_then(hex_u64) {
        counts.insert("gas_used".to_string(), g);
    }
    match parse_hex(&input.tx_input_hex, "dry-run tx input") {
        Ok(sim) if keccak(&sim) == keccak(initcode) => {}
        Ok(sim) => problems.push(format!(
            "the dry run deployed different bytecode ({} instead of {})",
            initcode_hash(&sim),
            initcode_hash(initcode)
        )),
        Err(e) => problems.push(e),
    }
    match input.citrate_precompiles {
        PrecompileUse::None => {}
        PrecompileUse::Used => problems.push(
            "the contract uses Citrate precompiles, which an anvil fork cannot simulate (needs the Citrate-aware fork, HUP-S6.10)".into(),
        ),
        PrecompileUse::Unknown => problems.push(
            "Citrate precompile use is unknown, and an anvil fork cannot simulate precompiles".into(),
        ),
    }
    let sites = precompile_call_sites(initcode);
    counts.insert("precompile_call_sites".to_string(), sites.len() as u64);
    if !sites.is_empty() {
        problems.push(format!(
            "the bytecode calls Citrate precompile(s) {}, which an anvil fork cannot simulate",
            sites.join(", ")
        ));
    }
    if problems.is_empty() {
        let gas = counts
            .get("gas_used")
            .map(|g| format!(", gas used {g}"))
            .unwrap_or_default();
        Parsed {
            pass: true,
            reason: format!(
                "deployed at {} on an anvil fork{gas}",
                address.unwrap_or_default()
            ),
            counts,
        }
    } else {
        Parsed {
            pass: false,
            reason: problems.join("; "),
            counts,
        }
    }
}

// ------------------------------------------------------------------------ the store

#[derive(Default)]
struct StoreInner {
    records: HashMap<String, GateRecord>,
    order: VecDeque<String>,
    /// Ceremony ids opened (through [`GateStore::open_ceremony`]) per init-code hash.
    open: HashMap<String, VecDeque<String>>,
}

/// Open ceremonies remembered per hash. Past this bound the oldest is revoked (rejected), never
/// silently forgotten: a forgotten pending ceremony would escape a later NOT READY.
const MAX_OPEN_PER_HASH: usize = 16;

fn refusal(h: &str, rec: Option<&GateRecord>) -> Option<String> {
    match rec {
        None => Some(format!(
            "Deploy refused: no D-4 deploy gate record exists for this exact bytecode (keccak256 {h}). \
             Run the deploy gate on it first: forge tests, Slither, Aderyn, a Medusa campaign and a fork dry run. \
             Any change to the bytecode or constructor arguments needs a new gate run."
        )),
        Some(rec) if rec.verdict == Verdict::Ready => None,
        Some(rec) => {
            let reasons: Vec<String> = rec.failing().map(|i| format!("{}: {}", i.label, i.reason)).collect();
            Some(format!(
                "Deploy refused: the D-4 deploy gate is NOT READY for bytecode {h}. Failing: {}.",
                reasons.join("; ")
            ))
        }
    }
}

/// Gate records keyed by init-code hash. The latest evaluation for a hash wins, and a NOT READY
/// evaluation rejects every ceremony still open for that hash.
///
/// Lock order: the store lock is taken before the ceremony's own lock (in `open_ceremony` and
/// `record_and_revoke`); nothing takes them the other way round.
#[derive(Default)]
pub struct GateStore {
    inner: Mutex<StoreInner>,
}

impl GateStore {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, StoreInner>, String> {
        self.inner
            .lock()
            .map_err(|_| "internal: deploy gate store unavailable".to_string())
    }

    /// Store `rec` (replacing any record for its hash). If it is NOT READY, call `revoke` with
    /// every ceremony id still open for that hash, while the store lock is held, so no ceremony
    /// for that hash can be opened or left open against a stale READY. Records evicted by the
    /// size bound have their open ceremonies revoked too.
    pub fn record_and_revoke(
        &self,
        rec: GateRecord,
        mut revoke: impl FnMut(&str),
    ) -> Result<(), String> {
        let mut g = self.lock()?;
        let key = rec.initcode_hash.clone();
        let not_ready = rec.verdict != Verdict::Ready;
        g.order.retain(|k| *k != key);
        g.order.push_back(key.clone());
        g.records.insert(key.clone(), rec);
        if not_ready {
            for id in g.open.remove(&key).unwrap_or_default() {
                revoke(&id);
            }
        }
        while g.order.len() > MAX_GATE_RECORDS {
            if let Some(old) = g.order.pop_front() {
                g.records.remove(&old);
                for id in g.open.remove(&old).unwrap_or_default() {
                    revoke(&id);
                }
            }
        }
        Ok(())
    }

    /// Store `rec` with nothing to revoke (tests and callers without a ceremony).
    #[cfg(test)]
    pub fn record(&self, rec: GateRecord) -> Result<(), String> {
        self.record_and_revoke(rec, |_| {})
    }

    pub fn get(&self, initcode_hash: &str) -> Option<GateRecord> {
        self.lock()
            .ok()
            .and_then(|g| g.records.get(initcode_hash).cloned())
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().map(|g| g.records.len()).unwrap_or(0)
    }

    /// The READY record for exactly this init code, or the honest refusal naming what fails.
    pub fn require_ready(&self, initcode: &[u8]) -> Result<GateRecord, String> {
        let h = initcode_hash(initcode);
        let rec = self.get(&h);
        match refusal(&h, rec.as_ref()) {
            Some(msg) => Err(msg),
            None => rec.ok_or_else(|| "internal: deploy gate record vanished".to_string()),
        }
    }

    /// Re-check READY for exactly this init code and, under the same lock, open the ceremony
    /// with `open` and remember its id for this hash. `open` is never called without READY.
    /// When more than [`MAX_OPEN_PER_HASH`] ceremonies are tracked for the hash, the oldest is
    /// passed to `revoke` (rejected) so every ceremony that can still be approved stays tracked.
    pub fn open_ceremony(
        &self,
        initcode: &[u8],
        open: impl FnOnce() -> Result<crate::ceremony::CeremonyView, String>,
        mut revoke: impl FnMut(&str),
    ) -> Result<(GateRecord, crate::ceremony::CeremonyView), String> {
        let h = initcode_hash(initcode);
        let mut g = self.lock()?;
        let rec = g.records.get(&h).cloned();
        if let Some(msg) = refusal(&h, rec.as_ref()) {
            return Err(msg);
        }
        let rec = rec.ok_or_else(|| "internal: deploy gate record vanished".to_string())?;
        let view = open()?;
        let ids = g.open.entry(h).or_default();
        ids.push_back(view.id.clone());
        while ids.len() > MAX_OPEN_PER_HASH {
            if let Some(old) = ids.pop_front() {
                revoke(&old);
            }
        }
        Ok((rec, view))
    }
}

/// Tauri managed state for the gate store.
#[derive(Default)]
pub struct DeployGateState(pub GateStore);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// **Command — deploy_gate_submit.** Evaluate verifier outputs for one bytecode and store the
/// record (replacing any earlier record for the same init-code hash). A NOT READY result rejects
/// any deploy ceremony still open for that hash. Returns the record.
#[tauri::command]
pub async fn deploy_gate_submit(
    app_h: tauri::AppHandle,
    inputs: GateInputs,
) -> Result<GateRecord, String> {
    crate::blocking::off_main(move || {
        let st = tauri::Manager::try_state::<DeployGateState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let cer = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let rec = evaluate(&inputs, now_ms())?;
        // An already-decided ceremony cannot be rejected again; that error is expected and moot.
        st.0.record_and_revoke(rec.clone(), |id| {
            let _ = cer.0.reject(id);
        })?;
        Ok(rec)
    })
    .await
}

/// **Command — deploy_gate_lookup.** The init-code hash a deploy of this bytecode would carry,
/// and its gate record if one exists. Read-only.
#[tauri::command]
pub async fn deploy_gate_lookup(
    app_h: tauri::AppHandle,
    bytecode_hex: String,
    constructor_args_hex: Option<String>,
) -> Result<DeployGateLookup, String> {
    crate::blocking::off_main(move || {
        let st = tauri::Manager::try_state::<DeployGateState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let initcode = initcode_from_hex(&bytecode_hex, constructor_args_hex.as_deref())?;
        let h = initcode_hash(&initcode);
        Ok(DeployGateLookup {
            record: st.0.get(&h),
            initcode_hash: h,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("deploy_gate_tests.rs");
}
