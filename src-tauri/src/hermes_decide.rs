//! HUP-S5.3 (core half): the `decide()` slot's per-backend metering, as core reads it for the
//! Activity monitor.
//!
//! The sidecar meters every decision its `decide()` slot makes (backend, latency, confidence,
//! errors, bytes sent off the machine) and every task outcome recorded through
//! `POST /decide/outcomes`, and reports them per backend at `GET /decide/stats`. Core reads that
//! report (bearer-authed, loopback) and passes a bounded, camelCase view to the webview.
//! Read-only: nothing here decides, records or signs.
//!
//! Data source (Rule 7): the sidecar's `DecideService` (citrate-agent-runtime `agent-sidecar`,
//! report built by `citrate-agent-metering::decisions::DecisionReport`), via `GET /decide/stats`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{HermesManager, Result};

/// The most backend rows core passes on (the sidecar has two, `local` and `jev`; this bounds a
/// bad reply).
pub const MAX_DECIDE_BACKENDS: usize = 8;

/// One backend's numbers, as the monitor shows them. Absent numbers stay `None`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideBackendRow {
    /// `local` | `jev`.
    pub backend: String,
    pub decisions: u32,
    pub errors: u32,
    /// Median and 95th percentile latency over successful decisions.
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub mean_confidence: Option<f64>,
    /// Bytes sent off the machine (Jev request bodies; 0 for local).
    pub egress_bytes: u64,
    pub tasks_attempted: u32,
    pub tasks_succeeded: u32,
    /// Succeeded / attempted in basis points; `None` before any task outcome was recorded.
    pub task_success_bps: Option<u32>,
}

/// What core passes to the Activity monitor.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideMetering {
    /// The member opted into Jev and a key file is configured.
    pub jev_enabled: bool,
    pub jev_origins: u32,
    pub jev_non_web: bool,
    /// The content-free decision log is being written.
    pub logging: bool,
    pub backends: Vec<DecideBackendRow>,
}

#[derive(Deserialize)]
struct LatencyBody {
    p50: u64,
    p95: u64,
}

#[derive(Deserialize)]
struct BackendBody {
    #[serde(default)]
    decisions: u32,
    #[serde(default)]
    errors: u32,
    #[serde(default)]
    latency_ms: Option<LatencyBody>,
    #[serde(default)]
    mean_confidence: Option<f64>,
    #[serde(default)]
    egress_bytes: u64,
    #[serde(default)]
    tasks_attempted: u32,
    #[serde(default)]
    tasks_succeeded: u32,
    #[serde(default)]
    task_success_bps: Option<u32>,
}

#[derive(Deserialize)]
struct ReportBody {
    #[serde(default)]
    backends: BTreeMap<String, BackendBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatsBody {
    #[serde(default)]
    jev_enabled: bool,
    #[serde(default)]
    jev_origins: u32,
    #[serde(default)]
    jev_non_web: bool,
    #[serde(default)]
    logging: bool,
    report: ReportBody,
}

impl From<StatsBody> for DecideMetering {
    fn from(b: StatsBody) -> Self {
        DecideMetering {
            jev_enabled: b.jev_enabled,
            jev_origins: b.jev_origins,
            jev_non_web: b.jev_non_web,
            logging: b.logging,
            backends: b
                .report
                .backends
                .into_iter()
                .take(MAX_DECIDE_BACKENDS)
                .map(|(name, s)| DecideBackendRow {
                    backend: name.chars().take(32).collect(),
                    decisions: s.decisions,
                    errors: s.errors,
                    p50_ms: s.latency_ms.as_ref().map(|l| l.p50),
                    p95_ms: s.latency_ms.as_ref().map(|l| l.p95),
                    mean_confidence: s.mean_confidence.filter(|c| c.is_finite()),
                    egress_bytes: s.egress_bytes,
                    tasks_attempted: s.tasks_attempted,
                    tasks_succeeded: s.tasks_succeeded.min(s.tasks_attempted),
                    task_success_bps: s.task_success_bps.map(|b| b.min(10_000)),
                })
                .collect(),
        }
    }
}

impl HermesManager {
    /// `GET /decide/stats`: the `decide()` slot's per-backend metering.
    pub fn decide_stats(&self) -> Result<DecideMetering> {
        let bearer = self.bearer()?;
        let resp = self
            .control
            .get(&format!("{}/decide/stats", self.control_url()), &bearer)?;
        let body: StatsBody = Self::decode(resp)?;
        Ok(body.into())
    }
}

/// **hermes_decide_stats**: the `decide()` slot's per-backend metering for the Activity monitor.
/// A sidecar that is not running has no metering: `None`, not an error.
#[tauri::command]
pub async fn hermes_decide_stats(
    app: tauri::AppHandle,
) -> std::result::Result<Option<DecideMetering>, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_decide_stats_sync(app.clone())).await
}

/// Blocking body of [`hermes_decide_stats`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_decide_stats_sync(
    app: tauri::AppHandle,
) -> std::result::Result<Option<DecideMetering>, String> {
    let m = super::manager(&app)?;
    if !m.is_running() {
        return Ok(None);
    }
    m.decide_stats().map(Some).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("hermes_decide_tests.rs");
}
