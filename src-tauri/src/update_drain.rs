//! CORE-D6 (0.5.3) — drain the sidecars before an update is installed.
//!
//! Before 0.5.3 the update was installed while every sidecar was still running. Sidecars were
//! stopped only on the relaunch's exit request. On macOS and Linux that left a window in which the
//! supervisor could respawn a crashed sidecar from the freshly installed bundle: a new node binary
//! against a data dir the old app had not migrated (CORE-N3). On Windows the installer starts
//! while the sidecars still hold their files and RocksDB locks, because the plugin exits the
//! process without the app's exit event.
//!
//! Now the updater (`src/shell/updater.ts`) downloads first. Then, on the member's restart click,
//! it calls [`update_prepare`] **before** `install()`:
//! 1. the app-wide spawn gate ([`citrate_core_kit::supervisor::SpawnGate::global`]) is held, so no
//!    sidecar starts, restarts or is respawned from here on;
//! 2. every sidecar is stopped through its owner (`shutdown_all_sidecars`). On macOS and Linux
//!    that is SIGTERM, then SIGKILL after the stop grace; on Windows a hard kill. The node does not
//!    handle SIGTERM yet (it waits on SIGINT), so it ends without its own shutdown path; its
//!    RocksDB write-ahead log keeps the database consistent. A node SIGTERM handler is a chain
//!    follow-up (CHAIN-INTEG);
//! 3. the drain waits (up to [`EXIT_WAIT`]) until no supervisor under the gate has a live child.
//!    An empty owner slot is not an exit: a member Stop in flight, or a monitor detached past its
//!    join timeout, still has one. The children still running are reported as `stillExiting`;
//! 4. the services snapshot is read again, and the report says, per sidecar, what it was before
//!    and after. `allStopped` is true only when no sidecar is still supervised in a phase with a
//!    process and no child is still exiting.
//!
//! The webview installs only when `allStopped` is true. If it is false, or the install fails, it
//! calls [`update_prepare_cancel`], which releases the gate and reopens knowledge imports (the
//! drain closed them), so the memory daemon and imports can start again. The sidecars stay stopped
//! until they are started again (or the app restarts), and the member is told so.
use citrate_core_kit::supervisor::{Phase, SpawnGate};

use crate::services::{ObservationKind, ServiceEntry, ServiceId, ServicesSnapshot};

/// How long the drain waits for the stopped sidecars' children to exit.
pub const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// What a start refused during the drain reports.
pub const HOLD_REASON: &str =
    "an update is being installed; services start again when the app restarts";

/// One sidecar in the drain report.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrainedService {
    pub id: ServiceId,
    pub label: &'static str,
    /// What the snapshot said before the drain: `notSupervised`, `busy`, or the phase.
    pub before: String,
    /// What it says after.
    pub after: String,
    /// No process remains for it.
    pub stopped: bool,
}

/// What `update_prepare` returns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrainReport {
    pub all_stopped: bool,
    /// Sidecars whose child had not exited when the drain stopped waiting.
    pub still_exiting: Vec<String>,
    pub services: Vec<DrainedService>,
    pub took_ms: u64,
}

fn word(e: &ServiceEntry) -> String {
    match e.observation {
        ObservationKind::NotSupervised => "notSupervised".to_string(),
        ObservationKind::Busy => "busy".to_string(),
        ObservationKind::Observed => match e.phase {
            Some(p) => serde_json::to_value(p)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "unknown".to_string()),
            None => "unknown".to_string(),
        },
    }
}

/// Whether no process remains for the service. A busy read claims nothing, so it is not stopped.
fn is_stopped(e: &ServiceEntry) -> bool {
    match e.observation {
        ObservationKind::NotSupervised => true,
        ObservationKind::Busy => false,
        ObservationKind::Observed => matches!(e.phase, Some(Phase::Stopped | Phase::Failed)),
    }
}

/// Hold the gate, stop everything, wait for the children to exit, and report. `read` is the
/// services snapshot; `stop_all` the owners' stop paths; `wait_exit` the wait for real exits (the
/// names still running); `elapsed_ms` the time it took (injected so the tests are exact).
pub(crate) fn drain_with(
    gate: &SpawnGate,
    read: impl Fn() -> ServicesSnapshot,
    stop_all: impl FnOnce(),
    wait_exit: impl FnOnce() -> Vec<String>,
    elapsed_ms: impl FnOnce() -> u64,
) -> DrainReport {
    gate.hold(HOLD_REASON);
    let before = read();
    stop_all();
    let still_exiting = wait_exit();
    let after = read();
    let services: Vec<DrainedService> = after
        .services
        .iter()
        .map(|a| {
            let b = before.services.iter().find(|b| b.id == a.id);
            DrainedService {
                id: a.id,
                label: a.label,
                before: b.map_or_else(|| "unknown".to_string(), word),
                after: word(a),
                stopped: is_stopped(a),
            }
        })
        .collect();
    DrainReport {
        all_stopped: !services.is_empty()
            && services.iter().all(|s| s.stopped)
            && still_exiting.is_empty(),
        still_exiting,
        services,
        took_ms: elapsed_ms(),
    }
}

/// **update_prepare** — before the update is installed: hold every new sidecar start, stop every
/// sidecar through its owner, and report what each was before and after. Data sources: each
/// sidecar's kit supervisor, read through `services_snapshot`, and the owners' stop paths
/// (`shutdown_all_sidecars`). Nothing is installed here; the webview installs only when
/// `allStopped` is true.
#[tauri::command]
pub async fn update_prepare(app_h: tauri::AppHandle) -> std::result::Result<DrainReport, String> {
    crate::blocking::off_main(move || {
        let t0 = std::time::Instant::now();
        Ok(drain_with(
            &SpawnGate::global(),
            || crate::services::services_snapshot_sync(&app_h),
            || crate::shutdown_all_sidecars(&app_h),
            || SpawnGate::global().wait_until_no_children(EXIT_WAIT),
            || t0.elapsed().as_millis() as u64,
        ))
    })
    .await
}

/// Release the gate and reopen knowledge imports, which the drain's `shutdown_all_sidecars`
/// closed (#614 review): without this, imports and the memory daemon stay refused until restart.
pub(crate) fn cancel_with(gate: &SpawnGate, imports: &crate::knowledge_import::ImportRegistry) {
    imports.reopen();
    gate.release();
}

/// **update_prepare_cancel** — the update was not installed: release the spawn gate and reopen
/// knowledge imports so services can be started again. Nothing is restarted here. Data source:
/// the app-wide spawn gate and the knowledge-import registry.
#[tauri::command]
pub async fn update_prepare_cancel() -> std::result::Result<(), String> {
    cancel_with(
        &SpawnGate::global(),
        &crate::knowledge_import::ImportRegistry::global(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("update_drain_tests.rs");
}
