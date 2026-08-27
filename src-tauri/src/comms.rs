//! CX-S3 (lane s3) — groups: secure comms + admin RBAC host commands (C-19).
//!
//! ## S3.1 — the `comms-relay` sidecar lifecycle
//! Commons runs the citrate-comms **relay as a LOCAL supervised sidecar** (the mem-mcp /
//! llama-server pattern), and the app is a member-client to it over loopback. The relay is
//! server-blind (it stores only ciphertext; MLS handles E2E), and it is owned by the SIGNED-IN
//! wallet — Groups you own are hosted on your node's relay. Reaching *other* members' relays is
//! the P2P job of Lane E (cluster); S3.1 establishes the local relay lifecycle only.
//!
//! Architecture decision (S3.1, recorded here + in the sprint file): **local relay sidecar, owner
//! = the signed-in wallet.** Rationale: it reuses the exact supervised-sidecar seam the node and
//! mem-mcp already use (resolve binary → spawn with env config → admin `/health` liveness →
//! bounded-backoff restart), needs no new src-tauri crate dependency (the relay is a SPAWNED
//! binary, declared in the bundle overlays by S0.5), and keeps key material on-device.
//!
//! The relay is configured entirely by ENV (never argv — argv leaks to `ps`): `CITRATE_COMMS_BIND`
//! (ws transport), `CITRATE_COMMS_ADMIN_BIND` (loopback admin `GET /health`), `CITRATE_COMMS_DATA`,
//! `CITRATE_COMMS_OWNER` (the wallet), `CITRATE_COMMS_MASTER_KEY` (the store-wrapping key).
//!
//! S3.1 is the LIFECYCLE primitive (tested with an injected stub binary, no real relay in CI). The
//! `groups_*` commands (roster/RBAC/messages) are wired in S3.2/S3.3 over the member-client; they
//! stay honest `not wired` stubs until then. This module owns the relay manager as a process-wide
//! singleton, so no managed-state wiring is needed in the (s0-owned) `lib.rs` — Lane C stays
//! race-free.
//
// NOTE (remove in S3.2): S3.1 builds + TESTS the relay lifecycle primitive; its production
// consumer — the lazy-start singleton + the `groups_*` commands driven over the member-client —
// lands in S3.2 (the next serial WP in this lane), and `lib.rs` is off-limits to lane s3. Until
// that consumer exists the manager is exercised only by `comms_tests`, so the primitive reads as
// dead code for exactly one WP. Allow it here rather than wire a premature/fake caller just to
// satisfy the linter (Rule 1). S3.2 instantiates it and this attribute comes off.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::supervisor::{
    BackoffPolicy, HealthCheck, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// The relay's WebSocket transport bind (loopback). Distinct from node RPC (8545), llama (18080),
/// mem-mcp, and node-agent (19600).
pub const DEFAULT_COMMS_WS_BIND: &str = "127.0.0.1:8787";
/// The relay's loopback admin bind (`GET /health|/status`, `POST /pause|/resume`).
pub const DEFAULT_COMMS_ADMIN_BIND: &str = "127.0.0.1:8788";
/// Env override for the bundled `comms-relay` binary path (dev/tests).
pub const COMMS_RELAY_BIN_ENV: &str = "CITRATE_COMMS_RELAY_BIN";

// The relay's own env knobs (mirrors comms-relay/src/main.rs).
const ENV_WS_BIND: &str = "CITRATE_COMMS_BIND";
const ENV_ADMIN_BIND: &str = "CITRATE_COMMS_ADMIN_BIND";
const ENV_DATA: &str = "CITRATE_COMMS_DATA";
const ENV_OWNER: &str = "CITRATE_COMMS_OWNER";
const ENV_MASTER_KEY: &str = "CITRATE_COMMS_MASTER_KEY";

/// Liveness probe cadence while Running.
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);
/// Backoff crash counter resets after this long healthy (mirrors serve.rs rationale).
const COMMS_HEALTHY_AFTER: Duration = Duration::from_secs(30);
/// Startup grace: RocksDB open + WS bind. A failing probe before this elapses is not a crash.
const COMMS_START_GRACE: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Errors — coarse, secret-free.
// ---------------------------------------------------------------------------

/// A comms-relay sidecar error, surfaced to the bridge as a string.
#[derive(Debug)]
pub enum CommsError {
    /// The relay owner/master-key is not configured (no signed-in wallet / key not provisioned).
    NotConfigured,
    /// The bundled `comms-relay` binary could not be located (S0.5 packaging gap).
    BinaryNotFound(String),
    /// The supervisor refused to start the sidecar.
    Spawn(String),
    /// The sidecar is already running (idempotent-start guard).
    AlreadyRunning,
}

impl std::fmt::Display for CommsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommsError::NotConfigured => write!(
                f,
                "comms relay not configured: sign in first (the relay is owned by your wallet)"
            ),
            CommsError::BinaryNotFound(m) => write!(f, "comms-relay binary not bundled: {m}"),
            CommsError::Spawn(m) => write!(f, "comms-relay spawn error: {m}"),
            CommsError::AlreadyRunning => write!(f, "comms-relay already running"),
        }
    }
}
impl std::error::Error for CommsError {}

type Result<T> = std::result::Result<T, CommsError>;

/// The bridge status shape for the relay sidecar. Public facts only (no key / no owner secret).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommsRelayStatus {
    /// "stopped" | "starting" | "running" | "restarting" | "failed".
    pub state: String,
    /// The loopback member-client URL (`ws://<ws_bind>`).
    pub ws_url: String,
    /// Whether the supervisor currently reports Running (coarse health).
    pub healthy: bool,
}

fn map_state(state: &SupervisorState) -> &'static str {
    match state {
        SupervisorState::Off => "stopped",
        SupervisorState::Starting => "starting",
        SupervisorState::Running => "running",
        SupervisorState::Backoff { .. } => "restarting",
        SupervisorState::Failed => "failed",
    }
}

// ---------------------------------------------------------------------------
// The manager: the resolved binary + relay config + (while running) a supervisor.
// ---------------------------------------------------------------------------

/// The comms-relay sidecar manager. Owns the bundled binary, the relay's env config (data dir,
/// binds, the owner wallet + master key), and — while running — a live [`Supervisor`].
pub struct CommsRelayManager {
    bin: PathBuf,
    data_dir: PathBuf,
    ws_bind: String,
    admin_bind: String,
    /// The signed-in wallet that owns this relay (20-byte hex). Empty until configured.
    owner_hex: String,
    /// The store-wrapping master key (hex). Empty until provisioned. Passed via ENV, never argv.
    master_key_hex: String,
    crash_record_path: PathBuf,
    health_interval: Duration,
    #[cfg(test)]
    spawn_args_override: Option<Vec<String>>,
    sup: Mutex<Option<Supervisor>>,
}

impl CommsRelayManager {
    /// Build a manager over an explicit binary + data dir + relay config. Production uses
    /// [`build_relay_manager`]; tests inject a stub binary + dummy owner/key.
    pub fn new(
        bin: PathBuf,
        data_dir: PathBuf,
        owner_hex: impl Into<String>,
        master_key_hex: impl Into<String>,
        crash_record_path: PathBuf,
    ) -> Self {
        CommsRelayManager {
            bin,
            data_dir,
            ws_bind: DEFAULT_COMMS_WS_BIND.to_string(),
            admin_bind: DEFAULT_COMMS_ADMIN_BIND.to_string(),
            owner_hex: owner_hex.into(),
            master_key_hex: master_key_hex.into(),
            crash_record_path,
            health_interval: HEALTH_INTERVAL,
            #[cfg(test)]
            spawn_args_override: None,
            sup: Mutex::new(None),
        }
    }

    /// Test hook: override the `/health` probe cadence (avoids the startup-race flake, as serve.rs).
    #[cfg(test)]
    pub fn with_health_interval(mut self, interval: Duration) -> Self {
        self.health_interval = interval;
        self
    }

    /// Test hook: override the spawned argv so a `sleep`-style stub is a long-lived child.
    #[cfg(test)]
    pub fn with_spawn_args(mut self, args: Vec<String>) -> Self {
        self.spawn_args_override = Some(args);
        self
    }

    /// The loopback member-client URL (`ws://<ws_bind>`). Rust-owned so the webview never supplies
    /// the relay endpoint.
    pub fn ws_url(&self) -> String {
        format!("ws://{}", self.ws_bind)
    }

    /// Whether the relay is configured to start (a signed-in owner + a provisioned master key).
    pub fn is_configured(&self) -> bool {
        !self.owner_hex.is_empty() && !self.master_key_hex.is_empty()
    }

    #[cfg(test)]
    fn effective_spawn_args(&self) -> Vec<String> {
        if let Some(args) = &self.spawn_args_override {
            return args.clone();
        }
        Vec::new()
    }
    #[cfg(not(test))]
    fn effective_spawn_args(&self) -> Vec<String> {
        // The relay is env-configured; no positional argv.
        Vec::new()
    }

    /// Build the [`SidecarSpec`]: the relay is configured entirely by ENV (never argv), with an
    /// admin `GET /health` liveness probe. A failing probe is treated as a crash (bounded-backoff
    /// restart), so a wedged relay recovers.
    fn build_spec(&self) -> SidecarSpec {
        let mut spec = SidecarSpec::new("comms-relay", self.bin.clone(), self.effective_spawn_args());
        spec.env = vec![
            (ENV_WS_BIND.to_string(), self.ws_bind.clone()),
            (ENV_ADMIN_BIND.to_string(), self.admin_bind.clone()),
            (ENV_DATA.to_string(), self.data_dir.to_string_lossy().to_string()),
            (ENV_OWNER.to_string(), self.owner_hex.clone()),
            (ENV_MASTER_KEY.to_string(), self.master_key_hex.clone()),
        ];
        let health_url = format!("http://{}/health", self.admin_bind);
        spec.health_check = Some(HealthCheck {
            interval: self.health_interval,
            grace: COMMS_START_GRACE,
            probe: std::sync::Arc::new(move || http_health_ok(&health_url)),
        });
        spec
    }

    /// Test hook: expose the spec's env for the wiring proof (owner/binds/key carried, not argv).
    #[cfg(test)]
    pub fn spec_env_for_test(&self) -> Vec<(String, String)> {
        self.build_spec().env
    }

    /// Start the relay sidecar IFF it is configured (owner + master key) AND the binary exists.
    /// Fails CLOSED on either gate; idempotent (`AlreadyRunning`).
    pub fn start(&self) -> Result<()> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(CommsError::AlreadyRunning);
        }
        if !self.is_configured() {
            return Err(CommsError::NotConfigured);
        }
        if !self.bin.exists() {
            return Err(CommsError::BinaryNotFound(self.bin.display().to_string()));
        }
        std::fs::create_dir_all(&self.data_dir).ok();
        let spec = self.build_spec();
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = COMMS_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| CommsError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        Ok(())
    }

    /// Stop the relay: graceful supervisor release (SIGTERM → grace → SIGKILL, no orphan). Idempotent.
    pub fn stop(&self) {
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(sup) = sup {
            sup.stop();
            drop(sup);
        }
    }

    /// The current status: supervisor state + the member-client URL + a coarse healthy flag.
    pub fn status(&self) -> CommsRelayStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        let healthy = matches!(sup_state, Some(SupervisorState::Running));
        CommsRelayStatus {
            state: state.to_string(),
            ws_url: self.ws_url(),
            healthy,
        }
    }

    /// Whether the relay is currently Running (the member-client gate for S3.2).
    pub fn is_running(&self) -> bool {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        matches!(
            guard.as_ref().map(|s| s.status().state),
            Some(SupervisorState::Running)
        )
    }
}

/// A best-effort HTTP GET liveness probe against the relay admin `/health`: `true` iff 2xx quickly.
fn http_health_ok(url: &str) -> bool {
    ureq::get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .call()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Resolve the bundled `comms-relay` binary path (same overlay approach as serve.rs/memory.rs):
/// the `CITRATE_COMMS_RELAY_BIN` override first (dev/tests), else the Tauri resource dir. Honest
/// error if neither is present (the S0.5 packaging gap).
pub fn resolve_comms_relay_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    use tauri::Manager;
    if let Ok(p) = std::env::var(COMMS_RELAY_BIN_ENV) {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "{COMMS_RELAY_BIN_ENV} set but not found: {}",
            path.display()
        ));
    }
    let resource = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("comms-relay");
    Ok(resource)
}

// ---------------------------------------------------------------------------
// Tauri command surface — groups: create/list/join/roster/RBAC/messages.
// Wired over the member-client in S3.2/S3.3; honest `not wired` until then (Rule 1).
// ---------------------------------------------------------------------------

fn not_wired(cmd: &str) -> Result<()> {
    Err(CommsError::Spawn(format!(
        "comms::{cmd} is not wired yet (CX-S3 scaffold — relay lifecycle is S3.1; groups are S3.2/S3.3)"
    )))
}

macro_rules! not_wired_cmd {
    ($fn:ident, $name:literal) => {
        #[tauri::command]
        pub fn $fn() -> std::result::Result<(), String> {
            not_wired($name).map_err(|e| e.to_string())
        }
    };
}

not_wired_cmd!(groups_create, "create");
not_wired_cmd!(groups_list, "list");
not_wired_cmd!(groups_join, "join");
not_wired_cmd!(groups_roster, "roster");
not_wired_cmd!(groups_assign_role, "assign_role");
not_wired_cmd!(groups_offboard, "offboard");
not_wired_cmd!(groups_send, "send");
not_wired_cmd!(groups_messages, "messages");

#[cfg(test)]
mod tests {
    include!("comms_tests.rs");
}
