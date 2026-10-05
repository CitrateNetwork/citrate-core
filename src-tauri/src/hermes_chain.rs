//! HUP-S7.3 + S7.5 (core): the sidecar's metering and anchor routes, seen from core.
//!
//! Data source (Rule 7): every call here is the Hermes sidecar's bearer-authed loopback control
//! plane (`citrate-agent-sidecar`, crates `citrate-agent-metering` and `citrate-agent-anchor` in
//! citrate-agent-runtime). Core passes the sidecar three data folders under its own app-data dir:
//! the metering log, the decision records, and the anchor ledger. Trajectory recording is not one
//! of them: it is turned on only by the member's own switch (`fl_trajectories.rs`, default off).
//!
//! The sidecar batches and builds calldata; it never signs. Core signs the anchor with its own
//! anchor key inside the anchor ceremony (`citrate_core_kit::ceremony::anchor`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{HermesError, HermesManager, Result};

/// Env names the sidecar reads (see its `main.rs`).
pub const METERING_DIR_ENV: &str = "CITRATE_HERMES_METERING_DIR";
pub const RECORDS_DIR_ENV: &str = "CITRATE_HERMES_RECORDS_DIR";
pub const ANCHOR_DIR_ENV: &str = "CITRATE_HERMES_ANCHOR_DIR";

/// The data folders under `base` (core's `<app_local_data>/hermes`).
pub fn data_dirs(base: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![
        (METERING_DIR_ENV, base.join("metering")),
        (RECORDS_DIR_ENV, base.join("records")),
        (ANCHOR_DIR_ENV, base.join("anchor")),
    ]
}

/// One sidecar-planned anchor (`POST /anchor/plan`), as core reads it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlannedAnchor {
    pub plan: String,
    pub day: u64,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub commitment: Option<String>,
    #[serde(default)]
    pub call: Option<PlannedCall>,
}

/// The unsigned call inside a `ready` plan (the runtime's `UnsignedAnchorCall`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PlannedCall {
    pub chain_id: u64,
    pub to: Option<String>,
    pub value: u64,
    pub kind: String,
    pub root: String,
    pub data: String,
}

/// A valid `YYYY-MM-DD` shape (the sidecar does the calendar check).
pub fn valid_day(day: &str) -> bool {
    let b = day.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

impl HermesManager {
    /// Point the sidecar at its metering, decision-record and anchor-ledger folders.
    pub fn with_chain_data_dir(mut self, base: PathBuf) -> Self {
        self.chain_data_dir = Some(base);
        self
    }

    /// `GET /metering/daily?day=` — the day's report as the sidecar serves it.
    pub fn metering_daily(&self, day: Option<&str>) -> Result<serde_json::Value> {
        let url = match day {
            Some(d) if valid_day(d) => format!("{}/metering/daily?day={d}", self.control_url()),
            Some(_) => {
                return Err(HermesError::Control {
                    status: 400,
                    msg: "day must be YYYY-MM-DD".into(),
                })
            }
            None => format!("{}/metering/daily", self.control_url()),
        };
        let bearer = self.bearer()?;
        Self::decode(self.control.get(&url, &bearer)?)
    }

    /// `GET /anchor/status`.
    pub fn anchor_status(&self) -> Result<serde_json::Value> {
        let bearer = self.bearer()?;
        Self::decode(
            self.control
                .get(&format!("{}/anchor/status", self.control_url()), &bearer)?,
        )
    }

    /// `POST /anchor/plan` for one closed day, with the pinned registry.
    pub fn anchor_plan(&self, day: u64, registry: &str) -> Result<PlannedAnchor> {
        let bearer = self.bearer()?;
        let body = serde_json::json!({ "day": day, "registry": registry }).to_string();
        Self::decode(self.control.post(
            &format!("{}/anchor/plan", self.control_url()),
            &bearer,
            &body,
        )?)
    }

    /// `POST /anchor/confirm` — only ever called with a receipt that confirms (status 1, mined).
    pub fn anchor_confirm(
        &self,
        day: u64,
        commitment: &str,
        tx_hash: &str,
        block_number: u64,
    ) -> Result<serde_json::Value> {
        let bearer = self.bearer()?;
        let body = serde_json::json!({
            "day": day,
            "commitment": commitment,
            "txHash": tx_hash,
            "blockNumber": block_number,
        })
        .to_string();
        Self::decode(self.control.post(
            &format!("{}/anchor/confirm", self.control_url()),
            &bearer,
            &body,
        )?)
    }

    /// `GET /anchor/proof?seq=N`: one record's inclusion proof as the sidecar builds it. Core
    /// checks it itself (`anchor_proof::verdict`); nothing the sidecar says is taken as verified.
    pub fn anchor_proof(&self, seq: u64) -> Result<crate::anchor_proof::SidecarProof> {
        let bearer = self.bearer()?;
        Self::decode(self.control.get(
            &format!("{}/anchor/proof?seq={seq}", self.control_url()),
            &bearer,
        )?)
    }

    /// `GET /anchor/records?before=&limit=`: retained decision records, newest first.
    pub fn anchor_records(
        &self,
        before: Option<u64>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value> {
        let mut q = Vec::new();
        if let Some(b) = before {
            q.push(format!("before={b}"));
        }
        if let Some(l) = limit {
            q.push(format!("limit={l}"));
        }
        let qs = if q.is_empty() {
            String::new()
        } else {
            format!("?{}", q.join("&"))
        };
        let bearer = self.bearer()?;
        Self::decode(self.control.get(
            &format!("{}/anchor/records{qs}", self.control_url()),
            &bearer,
        )?)
    }

    /// `POST /metering/benchmark`: the unsigned BenchmarkRegistry calls for one day's
    /// aggregates. Naming the agent id and the registry is the member's opt-in; core rechecks
    /// every call before any of it reaches a ceremony (`benchmark_share`).
    pub fn metering_benchmark(
        &self,
        day: &str,
        agent_id: u128,
        registry: &str,
    ) -> Result<serde_json::Value> {
        if !valid_day(day) {
            return Err(HermesError::Control {
                status: 400,
                msg: "day must be YYYY-MM-DD".into(),
            });
        }
        let bearer = self.bearer()?;
        let body = serde_json::json!({
            "day": day,
            "agentId": agent_id.to_string(),
            "registry": registry,
        })
        .to_string();
        Self::decode(self.control.post(
            &format!("{}/metering/benchmark", self.control_url()),
            &bearer,
            &body,
        )?)
    }
}

/// The app's Hermes manager (built lazily, like every `hermes_*` command uses it).
pub(crate) fn manager_for(
    app: &tauri::AppHandle,
) -> std::result::Result<&'static HermesManager, String> {
    super::manager(app)
}
