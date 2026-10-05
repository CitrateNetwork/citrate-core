//! HUP-S6 US-6.1 (local half), gates g3-gate + g3-e2e (local), HUP-S11.1 (macOS part):
//! **hello mint end to end on this machine**, driven by the Gherkin in
//! `src-tauri/e2e/hello_mint_local.feature`.
//!
//! The run is real on every step it claims: the template renderer, `forge test`, Slither,
//! Aderyn, Medusa, an anvil fork dry run, the D-4 gate (`deploy_gate::evaluate` + the store),
//! `contract_deploy_sync` (the same body the `contract_deploy` command runs), the
//! SignatureCeremony signing a real EIP-155 transaction from a fresh test vault, and the
//! post-deploy steps. The chain is a throwaway anvil with chain id 40204; nothing touches the
//! live chain.
//!
//! **Verifier configs.** "The verifier config of this machine" finds each tool on `PATH`
//! (and forge in `~/.foundry/bin`). "The test-only verifier config" adds Aderyn and Medusa from
//! `CITRATE_E2E_HM_TEST_ADERYN_BIN` / `CITRATE_E2E_HM_TEST_MEDUSA_BIN`, which only
//! `scripts/e2e-hello-mint.sh --with-bundle-tools` sets, after checking the archives against the
//! SHA-256 measured in `components/toolchain-bundle.json`. The test-only config exists only in
//! this `#[cfg(test)]` file and never ships; it changes where two binaries are found, never how
//! their output is judged.
//!
//! Without `CITRATE_E2E_HM_CHAIN_RPC` the scenarios are skipped (the script sets it). The
//! feature parser and the step table are always checked.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sha3::Digest as _;

use crate::ceremony::{BroadcastConfig, IntentKind, SignatureCeremony, SignatureIntent};
use crate::custody::{CustodyError, CustodyVault, Keyring};
use crate::deploy_gate::{
    CompilerSettings, ForkDryRunInput, GateInputs, GateItemId, GateRecord, GateStore, MedusaInput,
    PrecompileUse, ToolRun, Verdict,
};

const FEATURE: &str = include_str!("../e2e/hello_mint_local.feature");
const DEPS_LOCK: &str = include_str!("../../templates/deps.lock.json");
const BUNDLE: &str = include_str!("../../components/toolchain-bundle.json");

/// 5 SALT in wei, the interview's price.
const PRICE_WEI: u128 = 5_000_000_000_000_000_000;
/// What the local chain credits the member wallet with (stands in for the faucet, HUP-S6.5).
const FUNDING_WEI: u128 = 100_000_000_000_000_000_000;
const CHAIN_ID: u64 = 40204;

// ------------------------------------------------------------------------ the Gherkin

#[derive(Debug, Clone, PartialEq, Eq)]
struct Scenario {
    name: String,
    /// Background steps first, then the scenario's own, keyword stripped.
    steps: Vec<String>,
}

/// A small Gherkin reader for this one feature: `Background:` and `Scenario:` blocks of
/// `Given/When/Then/And/But` steps; `#` comments and blank lines are skipped. Anything else
/// inside a block is an error, so a typo cannot silently drop a step.
fn parse_feature(src: &str) -> Result<Vec<Scenario>, String> {
    let mut background: Vec<String> = Vec::new();
    let mut scenarios: Vec<Scenario> = Vec::new();
    let mut in_background = false;
    let mut seen_feature = false;
    for (n, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("Feature:") {
            seen_feature = true;
            continue;
        }
        if line == "Background:" {
            in_background = true;
            continue;
        }
        if let Some(name) = line.strip_prefix("Scenario:") {
            in_background = false;
            scenarios.push(Scenario {
                name: name.trim().to_string(),
                steps: background.clone(),
            });
            continue;
        }
        let step = ["Given ", "When ", "Then ", "And ", "But "]
            .iter()
            .find_map(|k| line.strip_prefix(k))
            .ok_or_else(|| format!("line {}: not a step: {line}", n + 1))?
            .trim()
            .to_string();
        if in_background {
            background.push(step);
        } else {
            scenarios
                .last_mut()
                .ok_or_else(|| format!("line {}: a step outside any scenario", n + 1))?
                .steps
                .push(step);
        }
    }
    if !seen_feature {
        return Err("no Feature: line".into());
    }
    if scenarios.is_empty() {
        return Err("no scenarios".into());
    }
    Ok(scenarios)
}

type Step = fn(&mut World) -> Result<(), String>;

/// The one table from step text to handler.
fn step_fn(text: &str) -> Option<Step> {
    let f: Step = match text {
        "a local anvil chain with chain id 40204 and an anvil fork of it" => given_local_chain,
        "a fresh member wallet in a test vault, funded on the local chain" => given_member_wallet,
        "the Dev answers the hello-mint interview with Lemon Drops, LEMON, 500 supply and 5 SALT each" => {
            given_interview_answers
        }
        "the hello-mint template is rendered for tier T1 with the pinned dependencies" => {
            when_rendered
        }
        "the project has a vite + wagmi page and an ERC-721 contract" => then_project_shape,
        "the verifier config of this machine, which has no aderyn or medusa" => {
            given_machine_config
        }
        "the test-only verifier config that adds aderyn and medusa from the measured bundle archives" => {
            given_test_only_config
        }
        "the supply cap check is removed from the contract" => given_injected_unbounded_mint,
        "the deploy gate runs forge test, slither, aderyn, medusa and a fork dry run" => {
            when_gate_runs
        }
        "the verdict is NOT READY" => then_not_ready,
        "the verdict is READY" => then_ready,
        "the only failing items are Aderyn and Medusa campaign, each because the tool is not installed" => {
            then_only_missing_tools_fail
        }
        "contract_deploy refuses with the failing items and no ceremony is opened" => {
            then_deploy_refused
        }
        "the member clicks Deploy" => when_member_clicks_deploy,
        "a SignatureCeremony shows a contract creation whose bytecode hash matches the gated artifact" => {
            then_ceremony_matches_gate
        }
        "the member approves the ceremony" => when_member_approves,
        "the contract is deployed on the local chain" => then_deployed,
        "the deployed code matches the compiled artifact and the verifier input is produced" => {
            then_code_matches
        }
        "the site switches to the deployed contract" => then_site_switched,
        "a test mint of 1 token at 5 SALT succeeds through a ceremony that shows mint(quantity=1) without the raw-data acknowledgement" => then_test_mint,
        "the Vercel export is written" => then_vercel_export,
        "the page builds and is pinned to IPFS when the page build is enabled" => then_page_pinned,
        "the Forge tests item fails naming test_mint_stops_at_the_cap" => then_forge_names_cap,
        // The sidecar-driven half (hello_mint_e2e_sidecar.rs).
        "a Hermes sidecar session with the toolchain on and the hello-mint project granted" => {
            given_sidecar_session
        }
        "the Dev sends one prompt that runs the hello-mint workflow" => when_dev_runs_workflow,
        "the Dev asks Hermes to run the checks" => when_dev_asks_checks,
        "the session kept a raw report of forge_test, slither_scan, aderyn_scan and medusa_fuzz" => {
            then_reports_kept
        }
        "core gates the artifact from the session's toolchain reports with forkInCore" => {
            when_core_gates_from_sidecar
        }
        "the fork dry run ran in core on the Citrate-aware fork with the template's test mint" => {
            then_fork_in_core
        }
        "READY took at most 2 prompts beyond the interview answers" => then_prompt_budget,
        "the Dev tells Hermes to deploy it anyway" => when_deploy_anyway,
        "Hermes refuses, names test_mint_stops_at_the_cap and proposes the SoldOut patch" => {
            then_refusal_with_fix
        }
        "no SignatureCeremony was created" => then_no_ceremony_created,
        _ => return None,
    };
    Some(f)
}

#[test]
fn hello_mint_feature_parses_and_every_step_has_a_handler() {
    let scenarios = parse_feature(FEATURE).expect("the feature parses");
    assert_eq!(scenarios.len(), 5);
    for s in &scenarios {
        assert!(s.steps.len() >= 8, "{}: {} steps", s.name, s.steps.len());
        for step in &s.steps {
            assert!(step_fn(step).is_some(), "no handler for step: {step:?}");
        }
    }
    // The READY scenario deploys, verifies and mints; the other two stop at the refusal.
    let ready = &scenarios[1];
    assert!(ready
        .steps
        .iter()
        .any(|s| s == "the member approves the ceremony"));
    for s in [&scenarios[0], &scenarios[2]] {
        assert!(s
            .steps
            .iter()
            .any(|x| x.starts_with("contract_deploy refuses")));
        assert!(!s
            .steps
            .iter()
            .any(|x| x == "the member approves the ceremony"));
    }
}

#[test]
fn the_feature_reader_rejects_stray_lines_and_steps_outside_a_scenario() {
    assert!(parse_feature("Feature: x\n  Scenario: a\n    Given y\n    oops\n").is_err());
    assert!(parse_feature("Feature: x\n  Given y\n").is_err());
    assert!(parse_feature("Scenario: a\n  Given y\n").is_err());
    let ok =
        parse_feature("Feature: x\nBackground:\n Given b\nScenario: a\n When c\n").expect("parses");
    assert_eq!(ok[0].steps, vec!["b".to_string(), "c".to_string()]);
}

#[test]
fn an_unknown_step_has_no_handler() {
    assert!(step_fn("the verdict is probably READY").is_none());
}

#[test]
fn the_test_only_tools_are_the_versions_the_bundle_measured() {
    // The script checks the archives' SHA-256 against these same entries; the test checks the
    // binaries report the bundle's versions.
    assert_eq!(bundle_version("aderyn").as_deref(), Some("0.6.8"));
    assert_eq!(bundle_version("medusa").as_deref(), Some("1.5.1"));
}

#[test]
fn precompile_scan_flags_citrate_precompile_literals_only() {
    assert_eq!(
        declared_precompile_use("x = address(0x0100); y = address(0x1003);"),
        PrecompileUse::Used
    );
    assert_eq!(
        declared_precompile_use("IERC721(address(0x5FbDB2315678afecb367f032d93F642f64180aa3))"),
        PrecompileUse::None
    );
    assert_eq!(
        declared_precompile_use("uint256 x = 0x0100;"),
        PrecompileUse::None
    );
}

/// The local end-to-end run. Skipped unless `scripts/e2e-hello-mint.sh` set the environment.
#[test]
fn e2e_hello_mint_feature_runs_on_a_local_chain() {
    let Some(env) = E2eEnv::from_env() else {
        eprintln!("skipped: CITRATE_E2E_HM_CHAIN_RPC is not set (run scripts/e2e-hello-mint.sh)");
        return;
    };
    let scenarios = parse_feature(FEATURE).expect("the feature parses");
    let mut report: Vec<String> = Vec::new();
    for (i, s) in scenarios.iter().enumerate() {
        let mut w = World::new(env.clone(), i).expect("scenario world");
        eprintln!("\nScenario: {}", s.name);
        for step in &s.steps {
            let f = step_fn(step).expect("every step has a handler");
            let t = Instant::now();
            match f(&mut w) {
                Ok(()) => eprintln!("  ok   {step} ({} ms)", t.elapsed().as_millis()),
                Err(e) => panic!("Scenario {:?}\n  FAILED step: {step}\n  {e}", s.name),
            }
        }
        report.push(format!("passed: {}", s.name));
    }
    eprintln!("\n{}", report.join("\n"));
}

// ------------------------------------------------------------------------ the world

#[derive(Debug, Clone)]
struct E2eEnv {
    chain_rpc: String,
    fork_rpc: String,
    work: PathBuf,
    deps_cache: PathBuf,
    /// The sidecar-driven scenarios: the sidecar binary, the citrate-fork binary, an optional real
    /// model (base URL + name), the solc forge uses there, and the sidecar's sandbox mode.
    sidecar_bin: Option<PathBuf>,
    fork_bin: Option<PathBuf>,
    llm_url: Option<String>,
    llm_model: Option<String>,
    solc: Option<PathBuf>,
    sandbox: Option<String>,
    test_aderyn: Option<PathBuf>,
    test_medusa: Option<PathBuf>,
    kubo_api: Option<String>,
    page_build: bool,
}

impl E2eEnv {
    fn from_env() -> Option<E2eEnv> {
        let path = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Some(E2eEnv {
            chain_rpc: std::env::var("CITRATE_E2E_HM_CHAIN_RPC").ok()?,
            fork_rpc: std::env::var("CITRATE_E2E_HM_FORK_RPC").ok()?,
            work: path("CITRATE_E2E_HM_WORK")?,
            deps_cache: path("CITRATE_E2E_HM_DEPS")?,
            sidecar_bin: path("CITRATE_E2E_HM_SIDECAR_BIN"),
            fork_bin: path("CITRATE_E2E_HM_FORK_BIN"),
            llm_url: std::env::var("CITRATE_E2E_HM_LLM_URL").ok(),
            llm_model: std::env::var("CITRATE_E2E_HM_LLM_MODEL").ok(),
            solc: path("CITRATE_E2E_HM_SOLC"),
            sandbox: std::env::var("CITRATE_E2E_HM_SANDBOX").ok(),
            test_aderyn: path("CITRATE_E2E_HM_TEST_ADERYN_BIN"),
            test_medusa: path("CITRATE_E2E_HM_TEST_MEDUSA_BIN"),
            kubo_api: std::env::var("CITRATE_E2E_HM_KUBO_API").ok(),
            page_build: std::env::var("CITRATE_E2E_HM_PAGE_BUILD").as_deref() == Ok("1"),
        })
    }
}

/// Where each verifier tool was found; `None` = not installed (a FAIL item, never a pass).
#[derive(Debug, Clone)]
struct Tools {
    forge: Option<PathBuf>,
    slither: Option<PathBuf>,
    aderyn: Option<PathBuf>,
    medusa: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct Artifact {
    bytecode_hex: String,
    deployed_hex: String,
    compiler: CompilerSettings,
}

struct World {
    env: E2eEnv,
    dir: PathBuf,
    vault: CustodyVault,
    member: String,
    ceremony: SignatureCeremony,
    gate: GateStore,
    params: BTreeMap<String, String>,
    project: Option<PathBuf>,
    tools: Option<Tools>,
    artifact: Option<Artifact>,
    record: Option<GateRecord>,
    ceremony_id: Option<String>,
    deploy_tx: Option<String>,
    address: Option<String>,
    side: SidecarRun,
}

#[derive(Default)]
struct MemKeyring(std::sync::Mutex<BTreeMap<String, Vec<u8>>>);

impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| CustodyError::Io("poisoned".into()))?
            .get(account)
            .cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), CustodyError> {
        self.0
            .lock()
            .map_err(|_| CustodyError::Io("poisoned".into()))?
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), CustodyError> {
        self.0
            .lock()
            .map_err(|_| CustodyError::Io("poisoned".into()))?
            .remove(account);
        Ok(())
    }
}

impl World {
    fn new(env: E2eEnv, index: usize) -> Result<World, String> {
        let dir = env.work.join(format!("scenario-{index}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("work dir: {e}"))?;
        // A fresh vault with a freshly generated wallet: no fixed key exists anywhere.
        let vault = CustodyVault::new(Box::new(MemKeyring::default()), dir.join("vault.enc"), 0);
        let pass = || b"hello-mint e2e vault passphrase".to_vec();
        vault.init(&mut pass()).map_err(|e| e.to_string())?;
        vault.unlock(&mut pass()).map_err(|e| e.to_string())?;
        let created = crate::wallet::create(&vault).map_err(|e| e.to_string())?;
        Ok(World {
            env,
            dir,
            vault,
            member: created.address.to_ascii_lowercase(),
            ceremony: SignatureCeremony::new(),
            gate: GateStore::default(),
            params: BTreeMap::new(),
            project: None,
            tools: None,
            artifact: None,
            record: None,
            ceremony_id: None,
            deploy_tx: None,
            address: None,
            side: SidecarRun::default(),
        })
    }

    fn chain(&self) -> crate::rpc::RpcClient<crate::rpc::HttpTransport> {
        crate::rpc::RpcClient::with_transport(crate::rpc::HttpTransport::new(
            self.env.chain_rpc.clone(),
        ))
    }

    fn fork(&self) -> crate::rpc::RpcClient<crate::rpc::HttpTransport> {
        crate::rpc::RpcClient::with_transport(crate::rpc::HttpTransport::new(
            self.env.fork_rpc.clone(),
        ))
    }

    fn project(&self) -> Result<&Path, String> {
        self.project
            .as_deref()
            .ok_or_else(|| "no project rendered yet".to_string())
    }

    fn contracts(&self) -> Result<PathBuf, String> {
        Ok(self.project()?.join("contracts"))
    }

    fn record(&self) -> Result<&GateRecord, String> {
        self.record
            .as_ref()
            .ok_or_else(|| "the gate has not run".to_string())
    }

    fn artifact(&self) -> Result<&Artifact, String> {
        self.artifact
            .as_ref()
            .ok_or_else(|| "no compiled artifact".to_string())
    }

    fn address(&self) -> Result<&str, String> {
        self.address
            .as_deref()
            .ok_or_else(|| "nothing deployed yet".to_string())
    }
}

fn rpc(
    c: &crate::rpc::RpcClient<crate::rpc::HttpTransport>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    crate::contract_reader::rpc_raw(c, method, params).map_err(|e| format!("{method}: {e}"))
}

fn selector(sig: &str) -> Vec<u8> {
    sha3::Keccak256::digest(sig.as_bytes())[..4].to_vec()
}

fn word_u256(hex_out: &str) -> Result<u128, String> {
    let h = hex_out.trim_start_matches("0x");
    if h.len() < 64 {
        return Err(format!("short return data {hex_out}"));
    }
    u128::from_str_radix(&h[32..64], 16).map_err(|e| e.to_string())
}

fn word_address(hex_out: &str) -> Result<String, String> {
    let h = hex_out.trim_start_matches("0x");
    if h.len() < 64 {
        return Err(format!("short return data {hex_out}"));
    }
    Ok(format!("0x{}", &h[24..64]))
}

fn arg_word(v: u128) -> Vec<u8> {
    let mut w = vec![0u8; 16];
    w.extend_from_slice(&v.to_be_bytes());
    w
}

fn arg_address(a: &str) -> Result<Vec<u8>, String> {
    let b = hex::decode(a.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    let mut w = vec![0u8; 12];
    w.extend_from_slice(&b);
    Ok(w)
}

fn bundle_version(tool: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(BUNDLE).ok()?;
    v["tools"]
        .as_array()?
        .iter()
        .find(|t| t["name"] == tool)?
        .get("version")?
        .as_str()
        .map(str::to_string)
}

/// The precompile use a verifier declares from the source: `Used` when the Solidity names an
/// address literal in a Citrate precompile range (`address(0x0100)`..). The gate's bytecode scan
/// adds its own check on top.
fn declared_precompile_use(source: &str) -> PrecompileUse {
    let mut rest = source;
    while let Some(at) = rest.find("address(0x") {
        let tail = &rest[at + "address(0x".len()..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if digits.len() <= 8 {
            if let Ok(v) = u64::from_str_radix(&digits, 16) {
                if (0x0100..=0x013F).contains(&v)
                    || (0x0200..=0x0209).contains(&v)
                    || matches!(v, 0x1000 | 0x1002 | 0x1003)
                {
                    return PrecompileUse::Used;
                }
            }
        }
        rest = &tail[digits.len()..];
    }
    PrecompileUse::None
}

/// `name` on `PATH`, else `extra` (forge's default install).
fn on_path(name: &str, extra: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let c = dir.join(name);
            if c.is_file() {
                return Some(c);
            }
        }
    }
    extra.filter(|p| p.is_file())
}

fn first_line_of(bin: &Path, arg: &str) -> Option<String> {
    let out = Command::new(bin).arg(arg).output().ok()?;
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    text.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

/// Run a verifier tool. A missing tool is `NotInstalled`; a tool that printed nothing on stdout
/// (or `report` is absent) is `Error`. A non-zero exit with a report is still `Ran`: forge exits 1
/// when tests fail and slither exits 255 when it has findings; the parser judges the report.
fn run_tool(
    bin: Option<&Path>,
    args: &[&str],
    cwd: &Path,
    path_prefix: Option<&Path>,
    report: Option<&Path>,
) -> ToolRun {
    let Some(bin) = bin else {
        return ToolRun::NotInstalled;
    };
    let version = first_line_of(bin, "--version");
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(cwd);
    if let Some(prefix) = path_prefix {
        let mut paths = vec![prefix.to_path_buf()];
        if let Some(p) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&p));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }
    let t = Instant::now();
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return ToolRun::Error {
                message: format!("could not run {}: {e}", bin.display()),
            }
        }
    };
    let duration_ms = t.elapsed().as_millis() as u64;
    let output = match report {
        Some(p) => std::fs::read_to_string(p).unwrap_or_default(),
        None => String::from_utf8_lossy(&out.stdout).to_string(),
    };
    if output.trim().is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return ToolRun::Error {
            message: format!(
                "exit {:?}, no report: {}",
                out.status.code(),
                err.lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
            ),
        };
    }
    ToolRun::Ran {
        output,
        duration_ms,
        tool_version: version,
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    let st = Command::new("cp")
        .arg("-R")
        .arg(from)
        .arg(to)
        .status()
        .map_err(|e| format!("cp: {e}"))?;
    st.success()
        .then_some(())
        .ok_or_else(|| format!("cp -R {} failed", from.display()))
}

fn wait_receipt(
    c: &crate::rpc::RpcClient<crate::rpc::HttpTransport>,
    tx: &str,
) -> Result<serde_json::Value, String> {
    for _ in 0..100 {
        let r = rpc(c, "eth_getTransactionReceipt", serde_json::json!([tx]))?;
        if !r.is_null() {
            return Ok(r);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("no receipt for {tx}"))
}

// ------------------------------------------------------------------------ steps: background

fn given_local_chain(w: &mut World) -> Result<(), String> {
    for (name, c) in [("chain", w.chain()), ("fork", w.fork())] {
        let id = rpc(&c, "eth_chainId", serde_json::json!([]))?;
        if id.as_str() != Some("0x9d0c") {
            return Err(format!("the {name} RPC reports chain id {id}, not 40204"));
        }
        let client = rpc(&c, "web3_clientVersion", serde_json::json!([]))?;
        if !client.as_str().unwrap_or("").starts_with("anvil") {
            return Err(format!(
                "the {name} RPC is not anvil ({client}); refusing to run"
            ));
        }
    }
    Ok(())
}

fn given_member_wallet(w: &mut World) -> Result<(), String> {
    // The faucet (HUP-S6.5) is not built: the local chain credits the wallet directly.
    rpc(
        &w.chain(),
        "anvil_setBalance",
        serde_json::json!([w.member, format!("0x{FUNDING_WEI:x}")]),
    )?;
    let bal = w
        .chain()
        .get_balance(&w.member)
        .map_err(|e| e.to_string())?;
    (bal == FUNDING_WEI)
        .then_some(())
        .ok_or_else(|| format!("member balance is {bal}, expected {FUNDING_WEI}"))
}

fn given_interview_answers(w: &mut World) -> Result<(), String> {
    // The interview's answers as template parameters. The owner is the member's own wallet.
    w.params = BTreeMap::from([
        ("name".to_string(), "Lemon Drops".to_string()),
        ("symbol".to_string(), "LEMON".to_string()),
        ("supply".to_string(), "500".to_string()),
        ("price".to_string(), PRICE_WEI.to_string()),
        ("owner".to_string(), w.member.clone()),
    ]);
    Ok(())
}

fn when_rendered(w: &mut World) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates");
    let set = citrate_templates::TemplateSet::open(&root).map_err(|e| e.to_string())?;
    let out = w.dir.join("project");
    set.render("hello-mint", &w.params, citrate_templates::Tier::T1, &out)
        .map_err(|e| e.to_string())?;
    // Pinned Solidity dependencies, each checked against its locked commit.
    let lock: serde_json::Value = serde_json::from_str(DEPS_LOCK).map_err(|e| e.to_string())?;
    let deps = lock["deps"]
        .as_object()
        .ok_or_else(|| "deps.lock.json has no deps".to_string())?;
    let lib = out.join("contracts/lib");
    std::fs::create_dir_all(&lib).map_err(|e| e.to_string())?;
    for (name, d) in deps {
        let src = w.env.deps_cache.join(name);
        let head = Command::new("git")
            .arg("-C")
            .arg(&src)
            .args(["rev-parse", "HEAD"])
            .output()
            .map_err(|e| format!("git: {e}"))?;
        let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        if Some(head.as_str()) != d["commit"].as_str() {
            return Err(format!(
                "{name} in the deps cache is at {head:?}, not the locked commit"
            ));
        }
        copy_dir(&src, &lib.join(name))?;
    }
    w.project = Some(out);
    Ok(())
}

fn then_project_shape(w: &mut World) -> Result<(), String> {
    let p = w.project()?;
    let pkg = std::fs::read_to_string(p.join("app/package.json")).map_err(|e| e.to_string())?;
    for dep in ["\"vite\"", "\"wagmi\"", "\"viem\""] {
        if !pkg.contains(dep) {
            return Err(format!("app/package.json has no {dep}"));
        }
    }
    let sol =
        std::fs::read_to_string(p.join("contracts/src/Token.sol")).map_err(|e| e.to_string())?;
    if !sol.contains("contract LemonDrops is ERC721") {
        return Err("contracts/src/Token.sol is not the LemonDrops ERC-721".into());
    }
    let proj = crate::postdeploy::open_project(p)?;
    (proj.contract_name == "LemonDrops")
        .then_some(())
        .ok_or_else(|| format!("contract name {}", proj.contract_name))
}

// ------------------------------------------------------------------------ steps: verifier configs

fn machine_tools() -> Tools {
    let foundry = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".foundry/bin/forge"));
    Tools {
        forge: on_path("forge", foundry),
        slither: on_path("slither", None),
        aderyn: on_path("aderyn", None),
        medusa: on_path("medusa", None),
    }
}

fn given_machine_config(w: &mut World) -> Result<(), String> {
    let t = machine_tools();
    if t.aderyn.is_some() || t.medusa.is_some() {
        return Err(
            "aderyn or medusa is on PATH; this scenario proves the missing-tool path, so run it \
             through scripts/e2e-hello-mint.sh, which sets a PATH without them"
                .into(),
        );
    }
    w.tools = Some(t);
    Ok(())
}

fn given_test_only_config(w: &mut World) -> Result<(), String> {
    let mut t = machine_tools();
    let (Some(aderyn), Some(medusa)) = (w.env.test_aderyn.clone(), w.env.test_medusa.clone())
    else {
        return Err(
            "the test-only verifier config needs CITRATE_E2E_HM_TEST_ADERYN_BIN and \
             CITRATE_E2E_HM_TEST_MEDUSA_BIN (scripts/e2e-hello-mint.sh --with-bundle-tools)"
                .into(),
        );
    };
    for (tool, bin) in [("aderyn", &aderyn), ("medusa", &medusa)] {
        let want = bundle_version(tool).ok_or_else(|| format!("{tool} not in the bundle"))?;
        let got = first_line_of(bin, "--version").unwrap_or_default();
        if !got.contains(&want) {
            return Err(format!(
                "{tool} reports {got:?}, the bundle measured {want}"
            ));
        }
    }
    t.aderyn = Some(aderyn);
    t.medusa = Some(medusa);
    w.tools = Some(t);
    Ok(())
}

fn given_injected_unbounded_mint(w: &mut World) -> Result<(), String> {
    let path = w.contracts()?.join("src/Token.sol");
    let sol = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let line = "        if (quantity > remaining) revert SoldOut(quantity, remaining);\n";
    if !sol.contains(line) {
        return Err("the supply cap check is not in the rendered contract".into());
    }
    std::fs::write(&path, sol.replace(line, "")).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------------ steps: the gate

fn compile(w: &mut World) -> Result<Artifact, String> {
    let tools = w.tools.as_ref().ok_or("no verifier config")?;
    let forge = tools.forge.as_ref().ok_or("forge is needed to compile")?;
    let c = w.contracts()?;
    let st = Command::new(forge)
        .args(["build", "--offline"])
        .current_dir(&c)
        .output()
        .map_err(|e| format!("forge build: {e}"))?;
    if !st.status.success() {
        return Err(format!(
            "forge build failed: {}",
            String::from_utf8_lossy(&st.stderr)
        ));
    }
    let raw = std::fs::read_to_string(c.join("out/Token.sol/LemonDrops.json"))
        .map_err(|e| format!("artifact: {e}"))?;
    let a: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let s = |p: &str| a.pointer(p).and_then(|v| v.as_str()).map(str::to_string);
    let version = s("/metadata/compiler/version").ok_or("artifact has no compiler version")?;
    let compiler = CompilerSettings {
        solc_version: version.split('+').next().unwrap_or("").to_string(),
        optimizer: a
            .pointer("/metadata/settings/optimizer/enabled")
            .and_then(|v| v.as_bool())
            .ok_or("artifact has no optimizer setting")?,
        optimizer_runs: a
            .pointer("/metadata/settings/optimizer/runs")
            .and_then(|v| v.as_u64())
            .ok_or("artifact has no optimizer runs")? as u32,
        evm_version: s("/metadata/settings/evmVersion").ok_or("artifact has no EVM version")?,
        via_ir: a
            .pointer("/metadata/settings/viaIR")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    };
    let immutables = a
        .pointer("/deployedBytecode/immutableReferences")
        .and_then(|v| v.as_object())
        .map(|m| m.len())
        .unwrap_or(0);
    if immutables != 0 {
        return Err("the contract has immutables; the code comparison below assumes none".into());
    }
    Ok(Artifact {
        bytecode_hex: s("/bytecode/object").ok_or("artifact has no bytecode")?,
        deployed_hex: s("/deployedBytecode/object").ok_or("artifact has no deployed bytecode")?,
        compiler,
    })
}

/// The fork dry run: the creation tx from an unlocked anvil dev account on the fork, its receipt
/// and its `input`, plus the declared precompile use.
fn fork_dry_run(w: &World, initcode_hex: &str) -> Result<ForkDryRunInput, String> {
    let fork = w.fork();
    let accounts = rpc(&fork, "eth_accounts", serde_json::json!([]))?;
    let from = accounts[0]
        .as_str()
        .ok_or("the fork has no unlocked dev account")?
        .to_string();
    let version = rpc(&fork, "web3_clientVersion", serde_json::json!([]))?
        .as_str()
        .map(str::to_string);
    let t = Instant::now();
    let tx = rpc(
        &fork,
        "eth_sendTransaction",
        serde_json::json!([{ "from": from, "data": initcode_hex, "gas": "0x4c4b40" }]),
    )?;
    let tx = tx.as_str().ok_or("no tx hash from the fork")?.to_string();
    let receipt = wait_receipt(&fork, &tx)?;
    let duration_ms = t.elapsed().as_millis() as u64;
    let txv = rpc(&fork, "eth_getTransactionByHash", serde_json::json!([tx]))?;
    let input = txv["input"].as_str().ok_or("the dry-run tx has no input")?;
    let src =
        std::fs::read_to_string(w.contracts()?.join("src/Token.sol")).map_err(|e| e.to_string())?;
    Ok(ForkDryRunInput {
        run: ToolRun::Ran {
            output: receipt.to_string(),
            duration_ms,
            tool_version: version,
        },
        tx_input_hex: input.to_string(),
        citrate_precompiles: declared_precompile_use(&src),
    })
}

fn when_gate_runs(w: &mut World) -> Result<(), String> {
    let artifact = compile(w)?;
    let tools = w.tools.clone().ok_or("no verifier config")?;
    let c = w.contracts()?;
    let forge_tests = run_tool(tools.forge.as_deref(), &["test", "--json"], &c, None, None);
    // The same arguments the runtime's slither_scan tool passes (SARIF on stdout).
    let slither = run_tool(
        tools.slither.as_deref(),
        &[
            ".",
            "--sarif",
            "-",
            "--exclude-dependencies",
            "--disable-color",
            "--compile-force-framework",
            "foundry",
        ],
        &c,
        None,
        None,
    );
    // The same arguments the runtime's aderyn_scan tool passes: SARIF on stdout, which is what
    // the gate parses from a Hermes session too (one format on both paths).
    let aderyn = run_tool(
        tools.aderyn.as_deref(),
        &[
            ".",
            "--output",
            "aderyn-report.sarif",
            "--stdout",
            "--skip-update-check",
        ],
        &c,
        None,
        None,
    );
    // Medusa compiles through crytic-compile, which lives beside slither.
    let crytic_dir = tools
        .slither
        .as_ref()
        .and_then(|s| std::fs::canonicalize(s).ok())
        .and_then(|s| s.parent().map(Path::to_path_buf));
    let medusa_json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(c.join("medusa.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let call_budget = medusa_json
        .pointer("/fuzzing/testLimit")
        .and_then(|v| v.as_u64())
        .ok_or("medusa.json has no testLimit")?;
    let medusa = run_tool(
        tools.medusa.as_deref(),
        &["fuzz", "--no-color"],
        &c,
        crytic_dir.as_deref(),
        None,
    );
    let initcode_hex = artifact.bytecode_hex.clone();
    let dry = fork_dry_run(w, &initcode_hex)?;
    let inputs = GateInputs {
        bytecode_hex: artifact.bytecode_hex.clone(),
        constructor_args_hex: None,
        compiler: artifact.compiler.clone(),
        forge_tests,
        slither,
        aderyn,
        medusa: MedusaInput {
            run: medusa,
            call_budget,
        },
        // The e2e's own anvil fork run, handed over (it calls no Citrate precompile).
        fork_dry_run: Some(dry),
        fork_in_core: None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let rec = crate::deploy_gate::evaluate(&inputs, now)?;
    let cer = &w.ceremony;
    w.gate.record_and_revoke(rec.clone(), |id| {
        let _ = cer.reject(id);
    })?;
    for it in &rec.items {
        eprintln!(
            "       {} {}: {}",
            if it.pass { "PASS" } else { "FAIL" },
            it.label,
            it.reason
        );
    }
    w.artifact = Some(artifact);
    w.record = Some(rec);
    Ok(())
}

fn failing_ids(rec: &GateRecord) -> Vec<GateItemId> {
    rec.failing().map(|i| i.id).collect()
}

fn then_not_ready(w: &mut World) -> Result<(), String> {
    let rec = w.record()?;
    (rec.verdict == Verdict::NotReady)
        .then_some(())
        .ok_or_else(|| "the verdict is READY".to_string())
}

fn then_ready(w: &mut World) -> Result<(), String> {
    let rec = w.record()?;
    if rec.verdict == Verdict::Ready {
        return Ok(());
    }
    let why: Vec<String> = rec
        .failing()
        .map(|i| format!("{}: {}", i.label, i.reason))
        .collect();
    Err(format!("NOT READY: {}", why.join("; ")))
}

fn then_only_missing_tools_fail(w: &mut World) -> Result<(), String> {
    let rec = w.record()?;
    let ids = failing_ids(rec);
    if ids != vec![GateItemId::Aderyn, GateItemId::Medusa] {
        return Err(format!("failing items are {ids:?}"));
    }
    for it in rec.failing() {
        if !it.reason.contains("is not installed") {
            return Err(format!("{}: {}", it.label, it.reason));
        }
    }
    Ok(())
}

fn then_forge_names_cap(w: &mut World) -> Result<(), String> {
    let rec = w.record()?;
    let it = rec
        .items
        .iter()
        .find(|i| i.id == GateItemId::ForgeTests)
        .ok_or("no forge item")?;
    if it.pass {
        return Err("the forge item passed".into());
    }
    it.reason
        .contains("LemonDropsTest.test_mint_stops_at_the_cap()")
        .then_some(())
        .ok_or_else(|| format!("the finding is not named: {}", it.reason))
}

fn deploy(w: &World) -> Result<crate::contract_deploy::DeployProposal, String> {
    let a = w.artifact()?;
    crate::contract_deploy::contract_deploy_sync(
        &w.vault,
        &w.ceremony,
        &w.gate,
        a.bytecode_hex.clone(),
        None,
        None,
        Some(5_000_000),
    )
}

fn then_deploy_refused(w: &mut World) -> Result<(), String> {
    let err = match deploy(w) {
        Ok(p) => return Err(format!("contract_deploy opened ceremony {}", p.ceremony.id)),
        Err(e) => e,
    };
    if !err.contains("NOT READY") {
        return Err(format!("unexpected refusal: {err}"));
    }
    for it in w.record()?.failing() {
        if !err.contains(&it.label) {
            return Err(format!("the refusal does not name {}: {err}", it.label));
        }
    }
    // No ceremony was minted: ids are monotonic from 1, so the next request still gets id 1.
    let probe = w.ceremony.request(SignatureIntent {
        origin: "e2e-probe".into(),
        kind: IntentKind::PersonalSign,
        chain_id: CHAIN_ID,
        raw: "0x00".into(),
    });
    let _ = w.ceremony.reject(&probe.id);
    (probe.id == "1").then_some(()).ok_or_else(|| {
        format!(
            "a ceremony was opened before the refusal (next id {})",
            probe.id
        )
    })
}

// ------------------------------------------------------------------------ steps: deploy + after

fn when_member_clicks_deploy(w: &mut World) -> Result<(), String> {
    let p = deploy(w)?;
    w.ceremony_id = Some(p.ceremony.id.clone());
    let rec = w.record()?;
    if p.gate.initcode_hash != rec.initcode_hash || p.gate.verdict != Verdict::Ready {
        return Err("the proposal carries a different gate record".into());
    }
    Ok(())
}

fn then_ceremony_matches_gate(w: &mut World) -> Result<(), String> {
    let id = w.ceremony_id.as_deref().ok_or("no ceremony")?;
    let view = w.ceremony.status(id).ok_or("the ceremony is not pending")?;
    if view.chain_id != CHAIN_ID || view.requires_raw_ack {
        return Err(format!("unexpected ceremony view {view:?}"));
    }
    if !view.decoded.action.to_lowercase().contains("contract") {
        return Err(format!("decoded action {:?}", view.decoded.action));
    }
    let initcode = hex::decode(w.artifact()?.bytecode_hex.trim_start_matches("0x"))
        .map_err(|e| e.to_string())?;
    let h = crate::deploy_gate::initcode_hash(&initcode);
    (h == w.record()?.initcode_hash)
        .then_some(())
        .ok_or_else(|| {
            format!(
                "compiled init code {h} is not the gated {}",
                w.record()
                    .map(|r| r.initcode_hash.clone())
                    .unwrap_or_default()
            )
        })
}

fn approve(w: &World, id: &str, raw_ack: bool) -> Result<String, String> {
    let r = w
        .ceremony
        .approve_and_broadcast(
            &w.vault,
            &w.chain(),
            id,
            raw_ack,
            BroadcastConfig {
                chain_id: CHAIN_ID,
                poll_attempts: 100,
                poll_interval: Duration::from_millis(100),
            },
        )
        .map_err(|e| format!("ceremony: {e}"))?;
    Ok(r.tx_hash)
}

fn when_member_approves(w: &mut World) -> Result<(), String> {
    let id = w.ceremony_id.clone().ok_or("no ceremony")?;
    w.deploy_tx = Some(approve(w, &id, false)?);
    Ok(())
}

fn then_deployed(w: &mut World) -> Result<(), String> {
    let tx = w.deploy_tx.clone().ok_or("no deploy tx")?;
    let chain = w.chain();
    let receipt = crate::postdeploy::parse_receipt(&wait_receipt(&chain, &tx)?)?
        .ok_or("the deploy is not mined")?;
    let addr = receipt
        .contract_address
        .ok_or("the creation reverted or deployed nothing")?;
    let from = rpc(&chain, "eth_getTransactionByHash", serde_json::json!([tx]))?["from"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    if from != w.member {
        return Err(format!("deployed from {from}, not the member wallet"));
    }
    if crate::contract_reader::code_size(&chain, &addr)? == 0 {
        return Err("no code at the deployed address".into());
    }
    eprintln!("       deployed LemonDrops at {addr} (tx {tx})");
    w.address = Some(addr);
    Ok(())
}

fn then_code_matches(w: &mut World) -> Result<(), String> {
    let addr = w.address()?.to_string();
    let code = rpc(
        &w.chain(),
        "eth_getCode",
        serde_json::json!([addr, "latest"]),
    )?;
    let code = code.as_str().unwrap_or("").to_ascii_lowercase();
    if code != w.artifact()?.deployed_hex.to_ascii_lowercase() {
        return Err("the deployed runtime code differs from the compiled artifact".into());
    }
    // The standard-JSON input CitrateScan's verifier takes (submitted only on the live chain).
    let p = crate::postdeploy::open_project(w.project()?)?;
    let std_json = crate::postdeploy::forge_standard_json(&p, &addr)?;
    let v: serde_json::Value = serde_json::from_str(&std_json).map_err(|e| e.to_string())?;
    (v["language"] == "Solidity")
        .then_some(())
        .ok_or_else(|| "the verifier input is not Solidity standard JSON".to_string())
}

fn then_site_switched(w: &mut World) -> Result<(), String> {
    let addr = w.address()?.to_string();
    let p = crate::postdeploy::open_project(w.project()?)?;
    crate::postdeploy::switch_site_checked(&w.chain(), &p, &addr)?;
    // What postdeploy_switch_site does next: let the ceremony decode the page's calls to this
    // contract from its ABI (its code is the READY-gated build's).
    crate::postdeploy::register_gated_abi(&w.chain(), &p, &addr, &w.gate, &w.ceremony)?;
    let site = crate::postdeploy::site_contract(&p)?;
    (site.as_deref() == Some(addr.as_str()))
        .then_some(())
        .ok_or_else(|| format!("the site points at {site:?}"))
}

fn then_test_mint(w: &mut World) -> Result<(), String> {
    let addr = w.address()?.to_string();
    let chain = w.chain();
    let mut data = selector("mint(uint256)");
    data.extend(arg_word(1));
    let raw = serde_json::json!({
        "from": w.member,
        "to": addr,
        "value": format!("0x{PRICE_WEI:x}"),
        "data": format!("0x{}", hex::encode(&data)),
        "gas": "0x7a120",
        "chainId": format!("0x{CHAIN_ID:x}"),
    })
    .to_string();
    let intent = SignatureIntent {
        origin: "hello-mint page (local)".into(),
        kind: IntentKind::Transaction,
        chain_id: CHAIN_ID,
        raw,
    };
    // The mutant: a ceremony without the gated ABI shows the same call as raw data.
    let bare = SignatureCeremony::new();
    let raw_view = bare.request(intent.clone());
    let _ = bare.reject(&raw_view.id);
    if !raw_view.requires_raw_ack {
        return Err("without the ABI the mint should need the raw-data acknowledgement".into());
    }
    // With the ABI the site switch registered, the member sees mint(quantity=1) and approves it
    // with no raw-data acknowledgement.
    let view = w.ceremony.request(intent);
    if view.requires_raw_ack || !view.decoded.action.contains("mint(quantity=1)") {
        return Err(format!("expected a decoded mint, got {:?}", view.decoded));
    }
    eprintln!("       ceremony shows: {}", view.decoded.action);
    let tx = approve(w, &view.id, false)?;
    let r = wait_receipt(&chain, &tx)?;
    if r["status"].as_str() != Some("0x1") {
        return Err(format!("the mint reverted: {r}"));
    }
    let call = |sig: &str, args: Vec<u8>| {
        let mut d = selector(sig);
        d.extend(args);
        crate::contract_reader::view_call(&chain, &addr, &d)
    };
    let minted = word_u256(&call("totalMinted()", vec![])?)?;
    let bal = word_u256(&call("balanceOf(address)", arg_address(&w.member)?)?)?;
    let owner = word_address(&call("ownerOf(uint256)", arg_word(1))?)?;
    let held = chain.get_balance(&addr).map_err(|e| e.to_string())?;
    if minted != 1 || bal != 1 || owner != w.member || held != PRICE_WEI {
        return Err(format!(
            "after the mint: totalMinted {minted}, balanceOf {bal}, ownerOf(1) {owner}, contract holds {held} wei"
        ));
    }
    Ok(())
}

fn then_vercel_export(w: &mut World) -> Result<(), String> {
    let p = crate::postdeploy::open_project(w.project()?)?;
    let exp = crate::postdeploy::vercel_export(&p)?;
    let dir = PathBuf::from(&exp.dir);
    for f in ["vercel.json", "package.json", ".env.production"] {
        if !dir.join(f).is_file() {
            return Err(format!("the export has no {f}"));
        }
    }
    let env = std::fs::read_to_string(dir.join(".env.production")).map_err(|e| e.to_string())?;
    (env.to_ascii_lowercase()
        .contains(&w.address()?.to_ascii_lowercase()))
    .then_some(())
    .ok_or_else(|| "the export does not carry the deployed address".to_string())
}

fn npm(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("npm")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("npm: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "npm {} failed in {}: {}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .join(" | ")
    ))
}

fn then_page_pinned(w: &mut World) -> Result<(), String> {
    if !w.env.page_build {
        eprintln!(
            "       not run: the page build is off (scripts/e2e-hello-mint.sh --with-page-build)"
        );
        return Ok(());
    }
    let project = w.project()?.to_path_buf();
    // AC3: the Vercel export folder is a project that builds on its own.
    let export = project.join(crate::postdeploy::EXPORT_DIR);
    npm(&export, &["install", "--no-audit", "--no-fund"])?;
    npm(&export, &["run", "build"])?;
    if !export.join("dist/index.html").is_file() {
        return Err("the export build wrote no dist/index.html".into());
    }
    // The page itself, built against the deployed contract, then pinned.
    let app = project.join("app");
    npm(&app, &["install", "--no-audit", "--no-fund"])?;
    npm(&app, &["run", "build"])?;
    let built = std::fs::read_dir(app.join("dist/assets"))
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .any(|js| {
            js.to_ascii_lowercase()
                .contains(&w.address().unwrap_or_default().to_ascii_lowercase())
        });
    if !built {
        return Err("the built page does not carry the deployed contract address".into());
    }
    let Some(api) = w.env.kubo_api.clone() else {
        eprintln!("       not pinned: no local IPFS API (ipfs is not installed)");
        return Ok(());
    };
    let p = crate::postdeploy::open_project(&project)?;
    let pin = crate::postdeploy::pin_site(&p, &api)?;
    eprintln!("       pinned the site: CID {}", pin.cid);
    pin.cid
        .starts_with("bafy")
        .then_some(())
        .ok_or_else(|| format!("unexpected CID {}", pin.cid))
}

include!("hello_mint_e2e_sidecar.rs");
