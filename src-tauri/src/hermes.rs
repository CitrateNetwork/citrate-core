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
    /// A control call was made with no live session bearer (the sidecar isn't started). Fail closed.
    NotRunning,
    /// The control transport failed (connection refused, timeout). NEVER carries the bearer.
    Transport(String),
    /// The control surface answered non-2xx (e.g. 401 wrong/absent bearer, 404 unknown skill, 503
    /// e-stopped). Carries the status + a short message, never the bearer.
    Control { status: u16, msg: String },
    /// A control response body could not be decoded to the expected shape.
    Decode(String),
}

impl std::fmt::Display for HermesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HermesError::BinaryNotFound(m) => write!(f, "hermes binary not bundled: {m}"),
            HermesError::Token(m) => write!(f, "hermes bearer-token error: {m}"),
            HermesError::Spawn(m) => write!(f, "hermes spawn error: {m}"),
            HermesError::AlreadyRunning => write!(f, "hermes already running"),
            HermesError::NotRunning => write!(f, "hermes is not running (no session bearer)"),
            HermesError::Transport(m) => write!(f, "hermes control transport error: {m}"),
            HermesError::Control { status, msg } => {
                write!(f, "hermes control returned {status}: {msg}")
            }
            HermesError::Decode(m) => write!(f, "hermes control decode error: {m}"),
        }
    }
}
impl std::error::Error for HermesError {}

type Result<T> = std::result::Result<T, HermesError>;

// ---------------------------------------------------------------------------
// Control transport — the bearer-authed loopback calls to the sidecar (S6.2).
// ---------------------------------------------------------------------------

/// A bearer-authed control response: the raw status + body. Deliberately dumb — parsing lives in the
/// manager methods so the transport can be mocked in tests without a real HTTP sidecar.
#[derive(Debug, Clone)]
pub struct ControlResp {
    pub status: u16,
    pub body: String,
}

/// The Hermes control transport. Every call presents the bearer as `Authorization: Bearer <token>`.
/// Production is [`UreqControl`] (blocking ureq, already in the tree); tests inject a mock so the
/// command wiring is verified without spawning a real sidecar. The bearer is passed per-call and
/// never held by the transport.
pub trait HermesControl: Send + Sync {
    fn get(&self, url: &str, bearer: &str) -> Result<ControlResp>;
    fn post(&self, url: &str, bearer: &str, body: &str) -> Result<ControlResp>;
}

/// Production control transport over blocking `ureq`. On a non-2xx ureq surfaces the response (we map
/// it to [`HermesError::Control`]); a transport failure (refused/timeout) maps to
/// [`HermesError::Transport`] and never carries the bearer.
pub struct UreqControl;

impl UreqControl {
    fn read(resp: ureq::http::Response<ureq::Body>) -> Result<ControlResp> {
        let status = resp.status().as_u16();
        let body = resp
            .into_body()
            .read_to_string()
            .map_err(|e| HermesError::Transport(e.to_string()))?;
        Ok(ControlResp { status, body })
    }
}

impl HermesControl for UreqControl {
    fn get(&self, url: &str, bearer: &str) -> Result<ControlResp> {
        match ureq::get(url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .call()
        {
            Ok(resp) => Self::read(resp),
            // ureq returns Err on non-2xx; recover the status/body rather than losing it.
            Err(ureq::Error::StatusCode(code)) => Ok(ControlResp {
                status: code,
                body: String::new(),
            }),
            Err(e) => Err(HermesError::Transport(e.to_string())),
        }
    }

    fn post(&self, url: &str, bearer: &str, body: &str) -> Result<ControlResp> {
        match ureq::post(url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .header("Content-Type", "application/json")
            .send(body)
        {
            Ok(resp) => Self::read(resp),
            Err(ureq::Error::StatusCode(code)) => Ok(ControlResp {
                status: code,
                body: String::new(),
            }),
            Err(e) => Err(HermesError::Transport(e.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// The AgentHarnessDomain DTOs the commands return (mirror the sidecar's wire shapes).
// ---------------------------------------------------------------------------

/// `GET /status` — the sidecar's running/skills/pending snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub running: bool,
    pub skills: usize,
    pub pending_approvals: usize,
}

/// One installed skill (a capsule).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

/// One pending chain/skill effect awaiting human approval (surfaced to the ceremony bridge in S6.3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingApproval {
    pub id: String,
    pub kind: String,
    pub summary: String,
}

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
    /// The bearer-authed control transport (production ureq; tests inject a mock).
    control: Box<dyn HermesControl>,
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
            control: Box::new(UreqControl),
        }
    }

    /// Test hook: inject a mock control transport so the command wiring is verified without a real
    /// HTTP sidecar.
    #[cfg(test)]
    pub fn with_control(mut self, control: Box<dyn HermesControl>) -> Self {
        self.control = control;
        self
    }

    /// Test hook: set the session bearer directly (prod sets it only in `start`), so control methods
    /// can be exercised against the mock transport without spawning the sidecar.
    #[cfg(test)]
    pub fn set_token_for_test(&self, token: &str) {
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Zeroizing::new(token.to_string()));
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

    // --- S6.2 bearer-authed control calls -------------------------------------------------------

    /// Clone the live session bearer for a single call, or fail closed if none (not started).
    fn bearer(&self) -> Result<Zeroizing<String>> {
        self.token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|t| Zeroizing::new(t.to_string()))
            .ok_or(HermesError::NotRunning)
    }

    /// Map a control response to the expected JSON shape; a non-2xx becomes a typed `Control` error
    /// (fail closed — never parse an error body as success).
    fn decode<T: serde::de::DeserializeOwned>(resp: ControlResp) -> Result<T> {
        if !(200..300).contains(&resp.status) {
            return Err(HermesError::Control {
                status: resp.status,
                msg: resp.body.chars().take(200).collect(),
            });
        }
        serde_json::from_str(&resp.body).map_err(|e| HermesError::Decode(e.to_string()))
    }

    /// `GET /status` — the sidecar's running/skills/pending snapshot.
    pub fn remote_status(&self) -> Result<RemoteStatus> {
        let bearer = self.bearer()?;
        let resp = self
            .control
            .get(&format!("{}/status", self.control_url()), &bearer)?;
        Self::decode(resp)
    }

    /// `GET /skills` — the installed skill catalog.
    pub fn list_skills(&self) -> Result<Vec<SkillMeta>> {
        let bearer = self.bearer()?;
        let resp = self
            .control
            .get(&format!("{}/skills", self.control_url()), &bearer)?;
        Self::decode(resp)
    }

    /// `POST /run_skill` — accept a skill for execution (its chain effects surface as approvals). The
    /// body is `{ "name", "args" }`; the sidecar returns `{ ok }` = accepted.
    pub fn run_skill(&self, name: &str, args: &serde_json::Value) -> Result<()> {
        let bearer = self.bearer()?;
        let body = serde_json::json!({ "name": name, "args": args }).to_string();
        let resp =
            self.control
                .post(&format!("{}/run_skill", self.control_url()), &bearer, &body)?;
        if !(200..300).contains(&resp.status) {
            return Err(HermesError::Control {
                status: resp.status,
                msg: resp.body.chars().take(200).collect(),
            });
        }
        Ok(())
    }

    /// `GET /approvals` — the pending chain/skill effects awaiting human approval.
    pub fn pending_approvals(&self) -> Result<Vec<PendingApproval>> {
        let bearer = self.bearer()?;
        let resp = self
            .control
            .get(&format!("{}/approvals", self.control_url()), &bearer)?;
        Self::decode(resp)
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
// Tauri command surface (S6.2) — a process-wide lazy singleton drives the sidecar over the bearer
// control transport. No state wiring in the (s0-owned) lib.rs; Lane D stays self-contained.
// ---------------------------------------------------------------------------

use std::sync::OnceLock;

/// The process-wide Hermes manager. Built lazily on first command from the app (resolve the bundled
/// binary + the 0600 bearer/crash paths); one instance for the process lifetime.
static HERMES: OnceLock<HermesManager> = OnceLock::new();

/// Lazily build/borrow the manager. A resolve failure (an ENV override set-but-missing, or no
/// resource dir) is returned every call until fixed — never a half-inited global. A missing bundled
/// binary is NOT an error here; `start` reports `BinaryNotFound` (honest, Rule 1).
fn manager<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<&'static HermesManager, String> {
    if let Some(h) = HERMES.get() {
        return Ok(h);
    }
    use tauri::Manager;
    let bin = resolve_hermes_bin(app)?;
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes");
    let token_path = base.join("bearer.token");
    let crash_path = base.join("crashes.log");
    // If another thread won the race, `set` fails and we return the stored winner — same instance.
    let _ = HERMES.set(HermesManager::new(bin, token_path, crash_path));
    Ok(HERMES.get().expect("manager just set"))
}

/// Start the sidecar (idempotent). Returns the local lifecycle status.
#[tauri::command]
pub fn hermes_start(app: tauri::AppHandle) -> std::result::Result<HermesStatus, String> {
    let m = manager(&app)?;
    m.start().map_err(|e| e.to_string())?;
    Ok(m.status())
}

/// The AgentHarnessDomain status snapshot: running + skill/pending counts. A not-started sidecar is a
/// clean stopped snapshot, not an error.
#[tauri::command]
pub fn hermes_status(app: tauri::AppHandle) -> std::result::Result<RemoteStatus, String> {
    let m = manager(&app)?;
    if !m.is_running() {
        return Ok(RemoteStatus {
            running: false,
            skills: 0,
            pending_approvals: 0,
        });
    }
    m.remote_status().map_err(|e| e.to_string())
}

/// The installed skill catalog.
#[tauri::command]
pub fn hermes_skills(app: tauri::AppHandle) -> std::result::Result<Vec<SkillMeta>, String> {
    manager(&app)?.list_skills().map_err(|e| e.to_string())
}

/// Accept a skill for execution; its chain effects surface as pending approvals (the ceremony bridge,
/// S6.3). Returns `{ ok: true }` = accepted.
#[tauri::command]
pub fn hermes_run_skill(
    app: tauri::AppHandle,
    name: String,
    args: serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    manager(&app)?
        .run_skill(&name, &args)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true }))
}

/// The pending chain/skill effects awaiting human approval.
#[tauri::command]
pub fn hermes_pending_approvals(
    app: tauri::AppHandle,
) -> std::result::Result<Vec<PendingApproval>, String> {
    manager(&app)?.pending_approvals().map_err(|e| e.to_string())
}

/// Stop the sidecar (SIGTERM → grace → SIGKILL; the session bearer is wiped). Idempotent.
#[tauri::command]
pub fn hermes_stop(app: tauri::AppHandle) -> std::result::Result<(), String> {
    manager(&app)?.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("hermes_tests.rs");
}
