//! HUP-S7.5 / US-7.3 AC1 (D-27): SALT spent and gas for the Hermes daily report.
//!
//! Data source (Rule 7): the receipts of transactions core itself signed and sent, read from the
//! 40204 RPC by the ceremony that sent them (`eth_getTransactionReceipt`: status, `gasUsed`,
//! `effectiveGasPrice`) plus the value the signed transaction carried. Only Hermes's own
//! transactions are reported, recognised by the origin the ceremony displayed:
//!
//! - `agent:hermes` to the pinned InferenceRouter: a registry escalation (pays native SALT);
//! - `hermes:benchmark-share`: opt-in BenchmarkRegistry sharing;
//! - `hermes:nightly-anchor`: the nightly anchor (its own signer, see `ceremony::anchor`);
//! - any other `agent:hermes` or `hermes:` origin: another transaction Hermes proposed.
//!
//! A transaction is reported only once its receipt is mined and carries gas used and the gas
//! price; anything less stays unknown rather than guessed. Reporting goes to the sidecar's
//! `POST /metering/chain-receipt` (public facts only) and never changes what the member sees from
//! the ceremony; a sidecar that is down only means the day's spend is incomplete.

use citrate_core_kit::ceremony::anchor::{AnchorReceipt, ANCHOR_ORIGIN};
use citrate_core_kit::ceremony::BroadcastResult;

/// The origin the Hermes bridge and the registry route stamp on their intents.
pub const HERMES_AGENT_ORIGIN: &str = "agent:hermes";
/// The opt-in benchmark sharing origin (`benchmark_share::SHARE_ORIGIN`).
pub const BENCHMARK_ORIGIN: &str = crate::benchmark_share::SHARE_ORIGIN;

/// What a transaction from `origin` to `to` was for, or `None` when it is not Hermes's.
pub fn purpose_for(origin: &str, to: Option<&str>, router: Option<&str>) -> Option<&'static str> {
    if origin == ANCHOR_ORIGIN {
        return Some("anchor");
    }
    if origin == BENCHMARK_ORIGIN {
        return Some("benchmark");
    }
    let hermes = origin == HERMES_AGENT_ORIGIN
        || origin.starts_with("agent:hermes:")
        || origin.starts_with("hermes:");
    if !hermes {
        return None;
    }
    match (to, router) {
        (Some(t), Some(r)) if t.eq_ignore_ascii_case(r) => Some("registry_escalation"),
        _ => Some("agent"),
    }
}

fn body(
    tx_hash: &str,
    purpose: &str,
    status: Option<u64>,
    gas_used: Option<u64>,
    price: Option<&str>,
    value: &str,
) -> Option<serde_json::Value> {
    let status = u8::try_from(status?).ok().filter(|s| *s <= 1)?;
    Some(serde_json::json!({
        "txHash": tx_hash,
        "purpose": purpose,
        "status": status,
        "gasUsed": gas_used?,
        "effectiveGasPriceWei": price?,
        "valueWei": value,
    }))
}

/// The sidecar body for a ceremony broadcast, when it is Hermes's and its receipt is complete.
pub fn broadcast_body(r: &BroadcastResult, router: Option<&str>) -> Option<serde_json::Value> {
    let purpose = purpose_for(&r.origin, r.to.as_deref(), router)?;
    r.block_number?;
    body(
        &r.tx_hash,
        purpose,
        r.status,
        r.gas_used,
        r.effective_gas_price_wei.as_deref(),
        r.value_wei.as_deref().unwrap_or("0"),
    )
}

/// The sidecar body for a mined anchor (it moves no value; it pays gas).
pub fn anchor_body(r: &AnchorReceipt) -> Option<serde_json::Value> {
    r.block_number?;
    body(
        &r.tx_hash,
        "anchor",
        r.status,
        r.gas_used,
        r.effective_gas_price_wei.as_deref(),
        "0",
    )
}

/// Hand one receipt body to the running sidecar. Best effort: a sidecar that is not running or
/// refuses it is logged (without content) and the caller carries on.
pub fn report(app: &tauri::AppHandle, body: serde_json::Value) {
    let m = match crate::hermes::chain::manager_for(app) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("citrate-core: Hermes metering could not take a chain receipt: {e}");
            return;
        }
    };
    if !m.is_running() {
        eprintln!("citrate-core: Hermes is not running; a chain receipt was not metered");
        return;
    }
    if let Err(e) = m.metering_chain_receipt(&body) {
        eprintln!("citrate-core: Hermes metering refused a chain receipt: {e}");
    }
}

/// Install the ceremony's broadcast observer: each Hermes transaction the member approves is
/// metered once its receipt is mined. Called once at startup.
pub fn install(app: tauri::AppHandle) {
    let installed = citrate_core_kit::ceremony::set_broadcast_observer(Box::new(move |r| {
        if let Some(b) = broadcast_body(r, crate::addresses::inference_router()) {
            report(&app, b);
        }
    }));
    if !installed {
        eprintln!("citrate-core: the broadcast observer was already installed");
    }
}
