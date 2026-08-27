//! CX-S6 (lane s6) — Hermes agent harness host commands (C-22).
//!
//! ## S6.1 — the `hermes` sidecar lifecycle
//! Commons runs the keyless **Hermes** agent harness as a LOCAL supervised sidecar (the node-agent
//! / mem-mcp pattern), bearer-authed over loopback. Distinct from the legacy `agent` module (the
//! node-agent GPU market). Every chain effect the harness wants routes through the SignatureCeremony
//! (Rule 3 / D-18) — the harness holds NO key and signs nothing; that wiring is S6.3.
//!
//! S6.1 is the LIFECYCLE + bearer primitive, mirroring `serve.rs`/`comms.rs` (resolve binary →
//! spawn → loopback `/health` liveness → bounded-backoff restart) plus `agent.rs`'s bearer scheme:
//! a fresh 256-bit token is minted per start, written to a `0600` file (the file IS the IPC channel
//! — the child adopts it), and the file PATH is handed to the child via ENV (never the token in
//! argv/env — argv/env leak to `ps`). The control surface + skills are S6.2–S6.4; the `hermes_*`
//! commands stay honest `not wired` until then. Managed as a process-wide singleton in this module,
//! so no state wiring in the (s0-owned) `lib.rs` — Lane D stays race-free.
//
// The manager's production consumer (lazy-start singleton + the `hermes_*` control commands) lands
// in S6.2/S6.3, and `lib.rs` is off-limits to lane s6. Until then the primitive is exercised only by
// `hermes_tests`, so it reads as dead code for one WP. Allow it rather than fake a caller (Rule 1);
// S6.2 removes this attribute.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::supervisor::{
    BackoffPolicy, HealthCheck, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// The Hermes harness loopback control bind. Distinct from node RPC (8545), llama (18080),
/// node-agent (19600), and comms (8787/8788).
pub const HERMES_CONTROL_ADDR: &str = "127.0.0.1:19700";
/// Env the Hermes child reads its control bind from.
const HERMES_ADDR_ENV: &str = "CITRATE_HERMES_ADDR";
/// Env the Hermes child reads its bearer-token FILE PATH from (the file is the IPC channel).
const HERMES_TOKEN_FILE_ENV: &str = "CITRATE_HERMES_TOKEN_FILE";
/// Env override for the bundled `hermes` binary path (dev/tests).
pub const HERMES_BIN_ENV: &str = "CITRATE_HERMES_BIN";

/// Supervision bearer length (256-bit), matching the node-agent scheme.
const TOKEN_LEN: usize = 32;
/// Liveness probe cadence while Running.
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);
/// Backoff crash counter resets after this long healthy.
const HERMES_HEALTHY_AFTER: Duration = Duration::from_secs(30);
/// Startup grace before a failing probe counts as a crash.
const HERMES_START_GRACE: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Errors — coarse, secret-free (NEVER carry the bearer token).
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum HermesError {
    /// The bundled `hermes` binary could not be located (S0.5 packaging gap).
    BinaryNotFound(String),
    /// Minting/persisting the bearer-token file failed (fail closed — never spawn without it).
    Token(String),
    /// The supervisor refused to start the sidecar.
    Spawn(String),
    /// The sidecar is already running (idempotent-start guard).
    AlreadyRunning,
}

impl std::fmt::Display for HermesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HermesError::BinaryNotFound(m) => write!(f, "hermes binary not bundled: {m}"),
            HermesError::Token(m) => write!(f, "hermes bearer-token error: {m}"),
            HermesError::Spawn(m) => write!(f, "hermes spawn error: {m}"),
            HermesError::AlreadyRunning => write!(f, "hermes already running"),
        }
    }
}
impl std::error::Error for HermesError {}

type Result<T> = std::result::Result<T, HermesError>;

/// The bridge status shape for the Hermes sidecar. Public facts only (never the bearer).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HermesStatus {
    /// "stopped" | "starting" | "running" | "restarting" | "failed".
    pub state: String,
    /// The loopback control base URL (`http://<control_addr>`). No token.
    pub control_url: String,
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
// The manager.
// ---------------------------------------------------------------------------

/// The Hermes sidecar manager. Owns the bundled binary, the loopback control bind, the bearer-token
/// file path, and — while running — a live [`Supervisor`] + the minted session bearer.
pub struct HermesManager {
    bin: PathBuf,
    control_addr: String,
    token_path: PathBuf,
    crash_record_path: PathBuf,
    health_interval: Duration,
    #[cfg(test)]
    spawn_args_override: Option<Vec<String>>,
    /// The current session bearer (minted on start; wiped on drop). Held for control calls (S6.2+).
    token: Mutex<Option<Zeroizing<String>>>,
    sup: Mutex<Option<Supervisor>>,
}

impl HermesManager {
    /// Build a manager over an explicit binary + token file + crash path (default control addr).
    pub fn new(bin: PathBuf, token_path: PathBuf, crash_record_path: PathBuf) -> Self {
        HermesManager {
            bin,
            control_addr: HERMES_CONTROL_ADDR.to_string(),
            token_path,
            crash_record_path,
            health_interval: HEALTH_INTERVAL,
            #[cfg(test)]
            spawn_args_override: None,
            token: Mutex::new(None),
            sup: Mutex::new(None),
        }
    }

    /// Test hook: override the `/health` probe cadence (avoids the startup-race flake).
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

    /// The loopback control base URL (`http://<control_addr>`). No token.
    pub fn control_url(&self) -> String {
        format!("http://{}", self.control_addr)
    }

    #[cfg(test)]
    fn effective_spawn_args(&self) -> Vec<String> {
        self.spawn_args_override.clone().unwrap_or_default()
    }
    #[cfg(not(test))]
    fn effective_spawn_args(&self) -> Vec<String> {
        Vec::new()
    }

    /// Build the [`SidecarSpec`]: env carries the control bind + the token-file PATH (never the
    /// token itself); a loopback `GET /health` (open, no bearer) drives liveness.
    fn build_spec(&self) -> SidecarSpec {
        let mut spec = SidecarSpec::new("hermes", self.bin.clone(), self.effective_spawn_args());
        spec.env = vec![
            (HERMES_ADDR_ENV.to_string(), self.control_addr.clone()),
            (
                HERMES_TOKEN_FILE_ENV.to_string(),
                self.token_path.to_string_lossy().to_string(),
            ),
        ];
        let health_url = format!("http://{}/health", self.control_addr);
        spec.health_check = Some(HealthCheck {
            interval: self.health_interval,
            grace: HERMES_START_GRACE,
            probe: std::sync::Arc::new(move || http_health_ok(&health_url)),
        });
        spec
    }

    /// Test hook: expose the spec env for the wiring proof (addr + token-file path, never a token).
    #[cfg(test)]
    pub fn spec_env_for_test(&self) -> Vec<(String, String)> {
        self.build_spec().env
    }

    /// Start the sidecar: mint a fresh session bearer, persist it `0600` (the child adopts it), and
    /// spawn under the supervisor. Fails CLOSED if the token can't be minted or the binary is
    /// missing; idempotent (`AlreadyRunning`).
    pub fn start(&self) -> Result<()> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(HermesError::AlreadyRunning);
        }
        if !self.bin.exists() {
            return Err(HermesError::BinaryNotFound(self.bin.display().to_string()));
        }
        let token = mint_bearer();
        persist_bearer(&self.token_path, &token)?;
        let spec = self.build_spec();
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = HERMES_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| HermesError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = Some(token);
        Ok(())
    }

    /// Stop the sidecar (SIGTERM → grace → SIGKILL, no orphan) and drop the session bearer. Idempotent.
    pub fn stop(&self) {
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(sup) = sup {
            sup.stop();
            drop(sup);
        }
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The current status: supervisor state + the control URL + a coarse healthy flag.
    pub fn status(&self) -> HermesStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        let healthy = matches!(sup_state, Some(SupervisorState::Running));
        HermesStatus {
            state: state.to_string(),
            control_url: self.control_url(),
            healthy,
        }
    }

    /// Whether the sidecar is currently Running (the control-call gate for S6.2+).
    pub fn is_running(&self) -> bool {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        matches!(
            guard.as_ref().map(|s| s.status().state),
            Some(SupervisorState::Running)
        )
    }
}

/// A best-effort HTTP GET liveness probe against the Hermes control `/health` (open, no bearer).
fn http_health_ok(url: &str) -> bool {
    ureq::get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .call()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Resolve the bundled `hermes` binary (env override → resource dir), honest error if absent.
pub fn resolve_hermes_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    use tauri::Manager;
    if let Ok(p) = std::env::var(HERMES_BIN_ENV) {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("{HERMES_BIN_ENV} set but not found: {}", path.display()));
    }
    let resource = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("hermes");
    Ok(resource)
}

// ---- bearer token (mirrors agent.rs's scheme) ----

/// Mint a fresh 256-bit bearer (64 lowercase hex) via the OS CSPRNG, in a [`Zeroizing`] so it wipes
/// on drop; never `Debug`-printed/logged.
fn mint_bearer() -> Zeroizing<String> {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = Zeroizing::new([0u8; TOKEN_LEN]);
    OsRng.fill_bytes(bytes.as_mut());
    Zeroizing::new(hex::encode(bytes.as_ref()))
}

/// Write the bearer to `path` `0600` (parent `0700`). Rewritten each start so a stale/loosened file
/// is replaced with THIS session's token.
fn persist_bearer(path: &Path, token: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HermesError::Token(e.kind().to_string()))?;
        harden_dir_perms(parent).map_err(|e| HermesError::Token(e.kind().to_string()))?;
    }
    std::fs::write(path, token.as_bytes()).map_err(|e| HermesError::Token(e.kind().to_string()))?;
    harden_perms(path).map_err(|e| HermesError::Token(e.kind().to_string()))?;
    Ok(())
}

#[cfg(unix)]
fn harden_perms(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(path, perms)
}
#[cfg(not(unix))]
fn harden_perms(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn harden_dir_perms(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    let mut perms = std::fs::metadata(dir)?.permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(dir, perms)
}
#[cfg(not(unix))]
fn harden_dir_perms(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri command surface — wired over the control transport in S6.2/S6.3. Honest until then.
// ---------------------------------------------------------------------------

fn not_wired(cmd: &str) -> std::result::Result<(), String> {
    Err(format!(
        "hermes::{cmd} is not wired yet (CX-S6 scaffold — sidecar lifecycle is S6.1; control is S6.2/S6.3)"
    ))
}

macro_rules! not_wired_cmd {
    ($fn:ident, $name:literal) => {
        #[tauri::command]
        pub fn $fn() -> std::result::Result<(), String> {
            not_wired($name)
        }
    };
}

not_wired_cmd!(hermes_start, "start");
not_wired_cmd!(hermes_status, "status");
not_wired_cmd!(hermes_skills, "skills");
not_wired_cmd!(hermes_run_skill, "run_skill");
not_wired_cmd!(hermes_pending_approvals, "pending_approvals");
not_wired_cmd!(hermes_stop, "stop");

#[cfg(test)]
mod tests {
    include!("hermes_tests.rs");
}
