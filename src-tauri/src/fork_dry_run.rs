//! HUP-S6.10 — the deploy gate's **fork dry run on the Citrate-aware fork**.
//!
//! The D-4 gate's fork item ([`crate::deploy_gate`]) used to accept only an anvil receipt, and
//! refused any contract that touches a Citrate precompile, because an anvil fork reaches an
//! empty account there and answers success with no data. `citrate-fork` (citrate-chain
//! `crates/citrate-fork`) runs the node's own EVM configuration and Citrate precompile bridge
//! over 40204 state, and reports every precompile it touched with its coverage (`real` = the
//! node's implementation, `unavailable` = the fork cannot reproduce what 40204 would do).
//!
//! This module:
//! * builds the dry-run plan: create the gated init code, then optionally a test mint
//!   (`mint(uint256)` at `PRICE * quantity`, the hello-mint / erc721 template's mint, which is
//!   the S6.6 after-deploy test run on the fork first);
//! * runs the `citrate-fork` binary (from `CITRATE_FORK_BIN`, else the installed
//!   `citrate-fork` component) with the plan on stdin, bounded in time and output;
//! * turns its report into the gate's [`ForkDryRunInput`], binding `txInputHex` to the init
//!   code the fork actually executed;
//! * reads a report back as evidence for the gate parser ([`citrate_fork_evidence`]).
//!
//! State source: chain 40204's public RPC, or the member's local anvil fork of it (http
//! loopback only, the same rule as the contract reader). `citrate-fork` only calls read
//! methods; nothing here holds a key, signs, or sends a transaction (Rule 3). The fork-only
//! balance given to the dry-run sender exists in the fork's memory and nowhere else.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::deploy_gate::{ForkDryRunInput, PrecompileUse, ToolRun};

/// Developer override: the path of a `citrate-fork` binary.
pub const FORK_BIN_ENV: &str = "CITRATE_FORK_BIN";
/// The component name (and entrypoint) of the fork in the component store.
pub const FORK_COMPONENT: &str = "citrate-fork";
/// The `engine` value a citrate-fork report carries.
pub const ENGINE: &str = "citrate-fork";
/// How long one dry run may take before it is killed and reported as an error.
/// Conservative default, pending owner sign-off.
pub const FORK_TIMEOUT: Duration = Duration::from_secs(120);
/// The largest report accepted (the gate refuses larger evidence too).
const MAX_REPORT_BYTES: usize = 8 * 1024 * 1024;
/// Stderr kept for an error message, in chars.
const MAX_STDERR_CHARS: usize = 300;
/// The chain the gate deploys to.
pub const DEPLOY_CHAIN_ID: u64 = 40204;
/// The default dry-run sender: an address nobody holds a key for. The fork funds it in memory.
pub const DRY_RUN_SENDER: &str = "0x00000000000000000000000000000000000d7a11";
/// `mint(uint256)` (the erc721 / hello-mint template).
const MINT_SELECTOR: [u8; 4] = [0xa0, 0x71, 0x2d, 0x68];
/// The template's `MAX_PER_TX`.
pub const MAX_TEST_MINT: u32 = 10;
/// Headroom on top of the test-mint value given to the dry-run sender (1 SALT). Gas is not
/// charged in the dry run (the chain's executor charges it outside the EVM).
const SENDER_HEADROOM_WEI: u128 = 1_000_000_000_000_000_000;

/// The test mint run after the create: `mint(quantity)` paying `priceWei * quantity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestMint {
    pub quantity: u32,
    /// Decimal wei per token (the template's `PRICE`).
    pub price_wei: String,
}

/// What the app asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkDryRunRequest {
    pub bytecode_hex: String,
    #[serde(default)]
    pub constructor_args_hex: Option<String>,
    /// `None` / `"citrate"` / `"40204"` = chain 40204; otherwise an http loopback anvil fork.
    #[serde(default)]
    pub state_rpc: Option<String>,
    /// The sender to simulate (the member's address gives the real create address); defaults
    /// to [`DRY_RUN_SENDER`].
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub test_mint: Option<TestMint>,
}

fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// The dry-run plan for `initcode` (create, then the optional test mint).
pub fn build_plan(initcode: &[u8], from: &str, mint: Option<&TestMint>) -> Result<Value, String> {
    let from = crate::contract_reader::normalize_address(from)?;
    if initcode.is_empty() {
        return Err("the init code is empty".into());
    }
    let mut steps = vec![json!({ "kind": "create", "data": hex0x(initcode) })];
    let mut value_total: u128 = 0;
    if let Some(m) = mint {
        if m.quantity == 0 || m.quantity > MAX_TEST_MINT {
            return Err(format!(
                "the test mint quantity must be 1 to {MAX_TEST_MINT}"
            ));
        }
        let price = crate::contract_reader::parse_value_wei(Some(&m.price_wei))?;
        value_total = price
            .checked_mul(u128::from(m.quantity))
            .ok_or_else(|| "the test mint value is too large".to_string())?;
        let mut data = MINT_SELECTOR.to_vec();
        let mut word = [0u8; 32];
        word[28..].copy_from_slice(&m.quantity.to_be_bytes());
        data.extend_from_slice(&word);
        steps.push(json!({
            "kind": "call",
            "to": "created:0",
            "data": hex0x(&data),
            "value": value_total.to_string(),
        }));
    }
    let balance = value_total
        .checked_add(SENDER_HEADROOM_WEI)
        .ok_or_else(|| "the test mint value is too large".to_string())?;
    Ok(json!({
        "from": from,
        "balances": { from.clone(): balance.to_string() },
        "steps": steps,
    }))
}

/// Where the `citrate-fork` binary is: `CITRATE_FORK_BIN` when it names a file, else the
/// installed component's entrypoint. `None` when neither exists (the gate item then FAILs as
/// not installed).
pub fn resolve_fork_bin(
    env_value: Option<&str>,
    components_root: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(p) = env_value.map(str::trim).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let root = components_root?;
    if !root.join("state.json").exists() {
        return None;
    }
    let dir = citrate_components::install::Store::open(root)
        .ok()?
        .current_dir(FORK_COMPONENT)
        .ok()??;
    let exe = if cfg!(windows) {
        format!("{FORK_COMPONENT}.exe")
    } else {
        FORK_COMPONENT.to_string()
    };
    let p = dir.join(exe);
    p.is_file().then_some(p)
}

/// Runs `citrate-fork run --plan - --rpc <rpc_url>` with `plan` on stdin. A report on stdout is
/// [`ToolRun::Ran`] (whatever it says); a failure to start, a non-zero exit, a timeout or an
/// oversized report is [`ToolRun::Error`].
pub fn run_fork(bin: &Path, plan: &Value, rpc_url: &str, timeout: Duration) -> ToolRun {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let version = Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let started = Instant::now();
    let mut child = match Command::new(bin)
        .args(["run", "--plan", "-", "--rpc", rpc_url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return ToolRun::Error {
                message: format!("citrate-fork could not start: {e}"),
            }
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A write error shows up as the child's own failure below.
        let _ = stdin.write_all(plan.to_string().as_bytes());
    }
    fn drain<R: std::io::Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let _ = std::io::Read::read_to_end(
                    &mut std::io::Read::take(r, (MAX_REPORT_BYTES + 1) as u64),
                    &mut buf,
                );
            }
            buf
        })
    }
    let out_h = drain(child.stdout.take());
    let err_h = drain(child.stderr.take());
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let Some(status) = status else {
        // Killed. The drain threads end when the pipes close; they are not waited for, since a
        // grandchild could still hold a pipe open.
        return ToolRun::Error {
            message: format!(
                "citrate-fork did not finish within {} ms",
                timeout.as_millis()
            ),
        };
    };
    let out = out_h.join().unwrap_or_default();
    let err = err_h.join().unwrap_or_default();
    let duration_ms = started.elapsed().as_millis() as u64;
    let stderr: String = String::from_utf8_lossy(&err)
        .trim()
        .chars()
        .take(MAX_STDERR_CHARS)
        .collect();
    if !status.success() {
        return ToolRun::Error {
            message: if stderr.is_empty() {
                format!("citrate-fork exited with {status}")
            } else {
                stderr
            },
        };
    }
    match out.len() {
        n if n > MAX_REPORT_BYTES => ToolRun::Error {
            message: "the citrate-fork report is larger than 8 MiB".into(),
        },
        _ => ToolRun::Ran {
            output: String::from_utf8_lossy(&out).into_owned(),
            duration_ms,
            tool_version: version,
        },
    }
}

/// The gate input for a fork run. `txInputHex` is the init code the fork executed (step 0),
/// so the gate's binding check compares what really ran. Precompile use is what the fork saw
/// (any Citrate precompile touched) or what the bytecode scan finds.
pub fn fork_input(run: ToolRun, initcode: &[u8]) -> ForkDryRunInput {
    let scanned = !crate::deploy_gate::precompile_call_sites(initcode).is_empty();
    let (tx_input_hex, touched) = match &run {
        ToolRun::Ran { output, .. } => match serde_json::from_str::<Value>(output) {
            Ok(v) => (
                v["steps"][0]["input"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                v["precompiles"]["touched"]
                    .as_array()
                    .is_some_and(|t| !t.is_empty()),
            ),
            Err(_) => (String::new(), false),
        },
        _ => (hex0x(initcode), false),
    };
    let citrate_precompiles = match &run {
        ToolRun::Ran { .. } if touched || scanned => PrecompileUse::Used,
        ToolRun::Ran { .. } => PrecompileUse::None,
        _ => PrecompileUse::Unknown,
    };
    ForkDryRunInput {
        run,
        tx_input_hex,
        citrate_precompiles,
    }
}

/// What the gate reads from a citrate-fork report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitrateForkEvidence {
    pub chain_id: u64,
    pub fork_block: u64,
    /// `true` when step 0 is a create.
    pub first_step_is_create: bool,
    /// Steps after the first that did not succeed, as `step i (kind): error`.
    pub failed_later_steps: Vec<String>,
    /// Addresses the fork runs with the node's own code.
    pub real: Vec<String>,
    /// Every touched Citrate precompile.
    pub touched: Vec<String>,
    /// Touched addresses the fork cannot reproduce.
    pub unavailable_touched: Vec<String>,
}

fn strings(v: &Value) -> Result<Vec<String>, String> {
    v.as_array()
        .ok_or_else(|| "expected a list".to_string())?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_string)
                .ok_or_else(|| "expected a list of strings".to_string())
        })
        .collect()
}

/// `None` when `report` is not a citrate-fork report (a plain anvil receipt); otherwise the
/// evidence, or why the report is malformed.
pub fn citrate_fork_evidence(report: &Value) -> Option<Result<CitrateForkEvidence, String>> {
    if report.get("engine").and_then(Value::as_str) != Some(ENGINE) {
        return None;
    }
    Some((|| {
        let bad = |w: &str| format!("the citrate-fork report has no valid {w}");
        let chain_id = report["chainId"].as_u64().ok_or_else(|| bad("chainId"))?;
        let fork_block = report["forkBlock"]
            .as_u64()
            .ok_or_else(|| bad("forkBlock"))?;
        let steps = report["steps"].as_array().ok_or_else(|| bad("steps"))?;
        let first_step_is_create = steps
            .first()
            .and_then(|s| s["kind"].as_str())
            .is_some_and(|k| k == "create");
        let failed_later_steps = steps
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, s)| s["status"].as_str() != Some("0x1"))
            .map(|(i, s)| {
                format!(
                    "step {i} ({}): {}",
                    s["kind"].as_str().unwrap_or("?"),
                    s["error"].as_str().unwrap_or("failed")
                )
            })
            .collect();
        let pre = &report["precompiles"];
        let touched = pre["touched"]
            .as_array()
            .ok_or_else(|| bad("precompiles.touched"))?
            .iter()
            .map(|t| {
                t["address"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad("precompiles.touched address"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CitrateForkEvidence {
            chain_id,
            fork_block,
            first_step_is_create,
            failed_later_steps,
            real: strings(&pre["real"]).map_err(|_| bad("precompiles.real"))?,
            touched,
            unavailable_touched: strings(&pre["unavailableTouched"])
                .map_err(|_| bad("precompiles.unavailableTouched"))?,
        })
    })())
}

/// The whole fork step for one request, given where the binary and the RPC are. No Tauri.
pub fn dry_run(
    req: &ForkDryRunRequest,
    bin: Option<&Path>,
    timeout: Duration,
) -> Result<ForkDryRunInput, String> {
    let initcode = crate::deploy_gate::initcode_from_hex(
        &req.bytecode_hex,
        req.constructor_args_hex.as_deref(),
    )?;
    let target = crate::contract_reader::parse_target(req.state_rpc.as_deref())?;
    let from = req.from.as_deref().unwrap_or(DRY_RUN_SENDER);
    let plan = build_plan(&initcode, from, req.test_mint.as_ref())?;
    let run = match bin {
        None => ToolRun::NotInstalled,
        Some(b) => run_fork(b, &plan, &target.rpc_url(), timeout),
    };
    Ok(fork_input(run, &initcode))
}

/// **Command — deploy_gate_fork_dry_run.** Runs the deploy gate's fork step on the
/// Citrate-aware fork and returns the gate input (`forkDryRun` of `deploy_gate_submit`).
/// Read-only (HIC-0): reads chain state, signs nothing, sends nothing.
#[tauri::command]
pub async fn deploy_gate_fork_dry_run(
    app_h: tauri::AppHandle,
    request: ForkDryRunRequest,
) -> Result<ForkDryRunInput, String> {
    crate::blocking::off_main(move || {
        use tauri::Manager as _;
        let components = app_h
            .path()
            .app_data_dir()
            .ok()
            .map(|d| d.join("components"));
        let env = std::env::var(FORK_BIN_ENV).ok();
        let bin = resolve_fork_bin(env.as_deref(), components.as_deref());
        dry_run(&request, bin.as_deref(), FORK_TIMEOUT)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("fork_dry_run_tests.rs");
}
