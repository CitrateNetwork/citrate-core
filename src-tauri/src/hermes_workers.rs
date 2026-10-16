//! HUP-S1.9 (core half) — the agent sidecar's worker processes, as core reads them.
//!
//! The sidecar runs its toolchain tools in a separate child process it supervises (restart
//! policy, health checks, clean shutdown); the browser worker is reserved for HUP-S5.1. Core
//! reads the sidecar's `GET /workers` report (bearer-authed, loopback) so the Activity monitor can
//! show each worker's state and restart history. Read-only: nothing here starts, stops or signs.
//!
//! Data source (Rule 7): the sidecar's own supervisor (`citrate-agent-workers`), via
//! `GET /workers`.

use serde::{Deserialize, Serialize};

use super::{HermesManager, Result};

/// The most rows core passes on (the sidecar reports one per worker kind; this bounds a bad reply).
pub const MAX_WORKER_ROWS: usize = 8;

/// One worker process as the sidecar reports it. Fields the sidecar did not send stay `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerReport {
    /// `toolchain` | `browser`.
    pub kind: String,
    /// `starting` | `running` | `restarting` | `failed` | `stopped` | `off` | `not_built`.
    pub state: String,
    #[serde(default)]
    pub healthy: Option<bool>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub restarts: Option<u32>,
    #[serde(default, alias = "last_exit")]
    pub last_exit: Option<String>,
    #[serde(default, alias = "last_error")]
    pub last_error: Option<String>,
    #[serde(default, alias = "running_since_ms")]
    pub running_since_ms: Option<u64>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Deserialize)]
struct WorkersBody {
    workers: Vec<WorkerReport>,
}

/// Bound a free-text field from the sidecar before it reaches the webview.
fn clip(s: Option<String>) -> Option<String> {
    s.map(|v| v.chars().take(300).collect())
}

impl HermesManager {
    /// `GET /workers` — the sidecar's worker processes and their health.
    pub fn workers(&self) -> Result<Vec<WorkerReport>> {
        let bearer = self.bearer()?;
        let resp = self
            .control
            .get(&format!("{}/workers", self.control_url()), &bearer)?;
        let body: WorkersBody = Self::decode(resp)?;
        Ok(body
            .workers
            .into_iter()
            .take(MAX_WORKER_ROWS)
            .map(|mut w| {
                w.kind = w.kind.chars().take(32).collect();
                w.state = w.state.chars().take(32).collect();
                w.last_exit = clip(w.last_exit);
                w.last_error = clip(w.last_error);
                w.detail = clip(w.detail);
                w
            })
            .collect())
    }
}

/// **hermes_workers** — the agent sidecar's worker processes (state, health, restarts, last exit).
/// A sidecar that is not running has no workers: an empty list, not an error.
#[tauri::command]
pub async fn hermes_workers(
    app: tauri::AppHandle,
) -> std::result::Result<Vec<WorkerReport>, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_workers_sync(app.clone())).await
}

/// Blocking body of [`hermes_workers`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_workers_sync(
    app: tauri::AppHandle,
) -> std::result::Result<Vec<WorkerReport>, String> {
    let m = super::manager(&app)?;
    if !m.is_running() {
        return Ok(Vec::new());
    }
    m.workers().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("hermes_workers_tests.rs");
}
