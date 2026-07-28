//! citrate-core — the `llama-server` sidecar (BC-3.2). @rule8-adjacent · spawns
//! an OS process serving OpenAI-compatible inference on loopback.
//!
//! BC-3.2 runs the locally-downloaded + verified Gemma GGUF (BC-3.1) via a
//! bundled `llama-server` (llama.cpp) under the [`crate::supervisor::Supervisor`].
//! `llama-server` serves an OpenAI-compatible `/v1/chat/completions`, so the local
//! model is simply an ai.rs provider whose `baseURL = http://127.0.0.1:<port>/v1`
//! with NO api key — the existing `ai.rs` inference path calls it unchanged.
//!
//! ## Grounded runtime facts
//! - CLI: `llama-server -m <gguf> --host 127.0.0.1 --port <p> --ctx-size <n>`
//!   (llama.cpp). It exposes `/health` and `/v1/models` for a liveness probe.
//! - Loopback-only bind (`127.0.0.1`) — the sidecar is LOCAL; nothing remote.
//! - `llama-server` is a SPAWNED binary, NOT a cargo dep. No llama.cpp bindings /
//!   heavy crates enter the src-tauri tree (SCOPE.md lean-tree gate).
//!
//! ## Fail-closed (Rule 1)
//! `start_if_ready` refuses to spawn unless the model is verified-`Ready`
//! (BC-3.1) AND the `llama-server` binary is actually bundled. A missing model or
//! a not-yet-bundled binary yields an HONEST error — never a silent no-op that
//! the UI could misread as a working local model.
//!
//! ## Honest gap (documented, not faked)
//! The `llama-server` platform binaries are a WO-2/S7 packaging deliverable (like
//! the node binary — gitignored, CI cross-build out of scope here). This module
//! builds the resolve + spawn + health + route LOGIC and unit-tests it against a
//! stub; the real binary + real inference are a manual/integration proof.

// BC-3.2: the production resolve/spawn surface only runs in a real Tauri build
// with the bundled binary; the stub drives CI. Some surface is reached only by
// the onboarding wiring + tests until then, mirroring node.rs/memory.rs.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::supervisor::{
    BackoffPolicy, HealthCheck, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// The default loopback port the local `llama-server` binds. Kept distinct from
/// the node RPC (8545) and the node-agent supervision port (19600).
pub const DEFAULT_LLAMA_PORT: u16 = 18080;

/// The default context window (`--ctx-size`). A modest window keeps memory
/// bounded on member hardware; the real value can be tuned in a later WP.
const DEFAULT_CTX_SIZE: u32 = 8192;

/// How often the liveness probe checks the server's `/health` while Running.
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);

/// A llama.cpp cold-start (mmap a ~5 GB GGUF + build the graph) can take tens of
/// seconds, so the sustained-healthy window is longer than the supervisor default
/// (mirrors node.rs's NODE_HEALTHY_AFTER rationale).
const LLAMA_HEALTHY_AFTER: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A llama-server sidecar error, surfaced to the bridge as a string.
#[derive(Debug)]
pub enum ServeError {
    /// The local model is not verified-`Ready` (BC-3.1) — refuse to spawn.
    ModelNotReady,
    /// The bundled `llama-server` binary could not be located (WO-2 gap).
    BinaryNotFound(String),
    /// The supervisor refused to start the sidecar (thread/spawn failure).
    Spawn(String),
    /// The sidecar is already running (idempotent-start guard).
    AlreadyRunning,
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeError::ModelNotReady => {
                write!(f, "local model is not ready (download + verify first)")
            }
            ServeError::BinaryNotFound(m) => write!(f, "llama-server binary not bundled: {m}"),
            ServeError::Spawn(m) => write!(f, "llama-server spawn error: {m}"),
            ServeError::AlreadyRunning => write!(f, "llama-server already running"),
        }
    }
}

impl std::error::Error for ServeError {}

type Result<T> = std::result::Result<T, ServeError>;

/// The bridge status shape for the llama-server sidecar. Public facts only —
/// supervisor state + the local baseURL + whether it is health-probed alive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServeStatus {
    /// "stopped" | "starting" | "running" | "restarting" | "failed".
    pub state: String,
    /// The local OpenAI-compatible baseURL (`http://127.0.0.1:<port>/v1`).
    #[serde(rename = "baseURL")]
    pub base_url: String,
    /// Whether the supervisor currently reports the child Running (a coarse
    /// health signal; the periodic `/health` probe restarts a wedged server).
    pub healthy: bool,
}

/// Map a [`SupervisorState`] to the bridge vocabulary (same mapping as node.rs).
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
// The manager: the resolved binary + the model path + the port + (while running)
// a live Supervisor. Mirrors node.rs::NodeManager.
// ---------------------------------------------------------------------------

/// The llama-server sidecar manager. Owns the resolved binary, the model path
/// (BC-3.1's verified GGUF), the loopback port, and (while running) a live
/// [`Supervisor`]. Managed as Tauri state.
pub struct LlamaServerManager {
    /// The bundled `llama-server` binary path (Tauri resource dir or override).
    bin: PathBuf,
    /// The verified model GGUF path (the `-m` arg).
    model_path: PathBuf,
    /// Where crash records are appended.
    crash_record_path: PathBuf,
    /// The loopback port the server binds.
    port: u16,
    /// The `/health` probe cadence. Production uses [`HEALTH_INTERVAL`]; the spawn-
    /// wiring test overrides it to a long interval so the 5s bind race against a
    /// never-answering stub port cannot flip Running→Backoff mid-poll (F-2). The
    /// health-driven restart itself keeps its own dedicated coverage.
    health_interval: Duration,
    /// Test-only argv override. Production is `None` → the grounded llama-server
    /// argv ([`Self::spawn_args`]). The spawn-wiring test sets this to a genuinely
    /// long-lived stub argv (e.g. `sleep 3600`) so the child actually SITS in
    /// Running rather than crash-looping (F-2): the real llama-server argv fed to a
    /// `sleep` stub is rejected as bad usage and exits instantly, which is what made
    /// the test race the transient Running window. This never affects production.
    #[cfg(test)]
    spawn_args_override: Option<Vec<String>>,
    /// The live supervisor, present only while the sidecar is running.
    sup: Mutex<Option<Supervisor>>,
}

impl LlamaServerManager {
    /// Build a manager over an explicit binary, model path, crash path, and port.
    /// Production uses [`build_serve_state`]; tests inject a stub binary + a temp
    /// model + a temp crash path.
    pub fn new(
        bin: PathBuf,
        model_path: PathBuf,
        crash_record_path: PathBuf,
        port: u16,
    ) -> Self {
        LlamaServerManager {
            bin,
            model_path,
            crash_record_path,
            port,
            health_interval: HEALTH_INTERVAL,
            #[cfg(test)]
            spawn_args_override: None,
            sup: Mutex::new(None),
        }
    }

    /// Test hook: override the `/health` probe cadence. Used by the spawn-wiring
    /// test to disable the 5s race that made it flaky (F-2). The health-driven
    /// restart path has its own dedicated coverage and is unaffected.
    #[cfg(test)]
    pub fn with_health_interval(mut self, interval: Duration) -> Self {
        self.health_interval = interval;
        self
    }

    /// Test hook: override the spawned argv so a `sleep`-style stub is a genuinely
    /// long-lived child (a valid duration arg) rather than crash-looping on the
    /// rejected llama-server argv. Makes the spawn-wiring test deterministic (F-2).
    #[cfg(test)]
    pub fn with_spawn_args(mut self, args: Vec<String>) -> Self {
        self.spawn_args_override = Some(args);
        self
    }

    /// The local OpenAI-compatible baseURL (`http://127.0.0.1:<port>/v1`, no key).
    /// This is the ai.rs LOCAL provider endpoint.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    /// The loopback port the local `llama-server` binds. `ai_chat_local` reads this
    /// (Rust-owned) so the LOCAL inference URL is derived in Rust, never supplied
    /// by the webview (F-1).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The grounded `llama-server` argv: `-m <model> --host 127.0.0.1 --port <p>
    /// --ctx-size <n>`. Loopback-only bind.
    fn spawn_args(&self) -> Vec<String> {
        vec![
            "-m".to_string(),
            self.model_path.to_string_lossy().to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            self.port.to_string(),
            "--ctx-size".to_string(),
            DEFAULT_CTX_SIZE.to_string(),
        ]
    }

    /// The argv actually handed to the supervisor. Production is always the
    /// grounded llama-server argv; a test may override it (long-lived stub).
    fn effective_spawn_args(&self) -> Vec<String> {
        #[cfg(test)]
        if let Some(args) = &self.spawn_args_override {
            return args.clone();
        }
        self.spawn_args()
    }

    /// Test hook: expose the argv for the structural CLI proof.
    #[cfg(test)]
    pub fn spawn_args_for_test(&self) -> Vec<String> {
        self.spawn_args()
    }

    /// Build the [`SidecarSpec`] + a `/health` liveness probe. A failing probe is
    /// treated as a crash by the supervisor (restart under the bounded backoff),
    /// so a wedged server recovers. No env / no secret (the local model needs no
    /// key).
    fn build_spec(&self) -> SidecarSpec {
        let mut spec =
            SidecarSpec::new("llama-server", self.bin.clone(), self.effective_spawn_args());
        let health_url = format!("http://127.0.0.1:{}/health", self.port);
        spec.health_check = Some(HealthCheck {
            interval: self.health_interval,
            probe: std::sync::Arc::new(move || http_health_ok(&health_url)),
        });
        spec
    }

    /// Start the sidecar IFF the model is `Ready` (BC-3.1) AND the binary exists.
    /// `model_ready` is the caller-supplied gate (from `ModelManager::is_ready`),
    /// injected so this stays a pure spawn primitive testable without BC-3.1's
    /// filesystem. Fails CLOSED on either gate; idempotent (`AlreadyRunning`).
    pub fn start_if_ready(&self, model_ready: bool) -> Result<()> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(ServeError::AlreadyRunning);
        }
        if !model_ready {
            return Err(ServeError::ModelNotReady);
        }
        if !self.bin.exists() {
            return Err(ServeError::BinaryNotFound(self.bin.display().to_string()));
        }
        let spec = self.build_spec();
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = LLAMA_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| ServeError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        Ok(())
    }

    /// Stop the sidecar: graceful supervisor release (SIGTERM → grace → SIGKILL,
    /// no orphan). Idempotent.
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

    /// The current status: supervisor state + the local baseURL + a coarse
    /// healthy flag (Running).
    pub fn status(&self) -> ServeStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        let healthy = matches!(sup_state, Some(SupervisorState::Running));
        ServeStatus {
            state: state.to_string(),
            base_url: self.base_url(),
            healthy,
        }
    }

    /// Whether the sidecar is currently Running (the ai.rs LOCAL-provider gate).
    pub fn is_running(&self) -> bool {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        matches!(
            guard.as_ref().map(|s| s.status().state),
            Some(SupervisorState::Running)
        )
    }
}

/// A best-effort HTTP GET liveness probe: `true` iff the URL answers 2xx quickly.
/// Bounded so a wedged server does not stall the probe (the supervisor's off-
/// thread probe runner also bounds it). Uses the existing `ureq` client.
fn http_health_ok(url: &str) -> bool {
    ureq::get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .call()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Managed Tauri state: the process-wide llama-server manager.
pub struct ServeState(pub LlamaServerManager);

/// Resolve the bundled `llama-server` sidecar binary path (same overlay approach
/// as node.rs/memory.rs): `CITRATE_LLAMA_SERVER_BIN` override first (dev/tests),
/// else the Tauri resource dir (`externalBin` strips the target-triple suffix to
/// `llama-server`). Honest error if neither is present (the WO-2 packaging gap).
fn resolve_llama_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    use tauri::Manager;
    if let Ok(p) = std::env::var("CITRATE_LLAMA_SERVER_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "CITRATE_LLAMA_SERVER_BIN set but not found: {}",
            path.display()
        ));
    }
    // Bundled as a `resources` DIRECTORY (`llama/`) — the binary sits alongside its
    // llama.cpp/ggml dylibs so its `@loader_path` rpath resolves them (a bare
    // `externalBin` can't carry the dylib tree). See tauri.conf.json `resources`.
    let resource = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("llama")
        .join("llama-server");
    Ok(resource)
}

/// Build the managed serve state from a live app handle: the bundled
/// `llama-server` binary, the BC-3.1 verified model path, a crash-record path,
/// and the default loopback port.
pub fn build_serve_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<ServeState, String> {
    use tauri::Manager;
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let model_path = data_root.join("models").join(crate::model::MODEL_FILE);
    let crash_record_path = data_root.join("models").join("llama-crash-records.jsonl");
    let bin = resolve_llama_bin(app)?;
    Ok(ServeState(LlamaServerManager::new(
        bin,
        model_path,
        crash_record_path,
        DEFAULT_LLAMA_PORT,
    )))
}

// ---------------------------------------------------------------------------
// Tauri commands — the model-serve bridge surface. Return status / () only. The
// `Ready` gate reads the BC-3.1 ModelManager (fail closed when the model is not
// verified-Ready).
// ---------------------------------------------------------------------------

use tauri::State;

/// **Command — model_serve_start.** Spawn `llama-server` on the local model IFF
/// it is verified-`Ready` (BC-3.1) and the binary is bundled. Fails closed +
/// honest error otherwise (never a silent no-op).
#[tauri::command]
pub fn model_serve_start(
    serve: State<'_, ServeState>,
    model: State<'_, crate::model::ModelState>,
) -> std::result::Result<(), String> {
    let ready = model.0.is_ready();
    serve.0.start_if_ready(ready).map_err(|e| e.to_string())
}

/// **Command — model_serve_stop.** Release the supervisor (SIGTERM→grace→SIGKILL,
/// no orphan). Idempotent.
#[tauri::command]
pub fn model_serve_stop(serve: State<'_, ServeState>) -> std::result::Result<(), String> {
    serve.0.stop();
    Ok(())
}

/// **Command — model_serve_status.** The supervisor state + the local baseURL +
/// a coarse healthy flag.
#[tauri::command]
pub fn model_serve_status(
    serve: State<'_, ServeState>,
) -> std::result::Result<ServeStatus, String> {
    Ok(serve.0.status())
}

/// **Command — model_inference_state.** The HONEST inference-routing state the
/// frontend renders: LOCAL (`ready`) → gateway (`gatewayOnly`/`localFallback`) →
/// downloading → demo. Computed from the real model status + serve health + the
/// gateway-key presence. `gateway_configured` is passed from the frontend's
/// providerStatus read (the gateway provider is configured iff a cgk_ key is
/// sealed); the model + serve facts are read here from the real managers.
#[tauri::command]
pub fn model_inference_state(
    serve: State<'_, ServeState>,
    model: State<'_, crate::model::ModelState>,
    gateway_configured: bool,
) -> std::result::Result<String, String> {
    use crate::model::ModelStatus;
    let model_status = model.0.status();
    let downloading = matches!(model_status, ModelStatus::Downloading { .. });
    let inputs = crate::ai::ProviderInputs {
        model_ready: matches!(model_status, ModelStatus::Ready),
        server_healthy: serve.0.is_running(),
        downloading,
        gateway_key_configured: gateway_configured,
    };
    Ok(crate::ai::select_inference_state(inputs).as_str().to_string())
}

#[cfg(test)]
mod tests {
    include!("serve_tests.rs");
}
