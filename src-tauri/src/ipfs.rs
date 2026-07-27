//! citrate-core — bundled IPFS (kubo) daemon supervisor.
//!
//! The `citrate` node uses IPFS at `http://127.0.0.1:5001` (its default; see
//! citrate-chain `node/src/artifact.rs` / `model_manager.rs`) for artifact + model
//! pin/add/ls. Block production does NOT need it, but the AI model/artifact
//! features do, so we bundle kubo and supervise `ipfs daemon` alongside the node.
//!
//! Lifecycle: on first start we `ipfs init --profile lowpower` (a repo tuned for a
//! desktop follower — reduced DHT/reprovider load) into a per-user repo, move the
//! HTTP gateway off the common :8080 to avoid conflicts, then supervise `ipfs
//! daemon` (which serves the API on the default 127.0.0.1:5001 the node expects).
//! No secret material; the repo is a plain kubo repo in the app data dir.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use crate::supervisor::{
    BackoffPolicy, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// The env var kubo reads for its repo location.
const IPFS_PATH_ENV: &str = "IPFS_PATH";
/// A non-conflicting local gateway port (kubo's default :8080 collides with common
/// dev servers; the node only talks to the API on :5001, not the gateway).
const GATEWAY_MULTIADDR: &str = "/ip4/127.0.0.1/tcp/48080";

#[derive(Debug)]
pub enum IpfsError {
    BinaryNotFound(String),
    Init(String),
    Spawn(String),
    AlreadyRunning,
}

impl std::fmt::Display for IpfsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpfsError::BinaryNotFound(m) => write!(f, "ipfs binary not found: {m}"),
            IpfsError::Init(m) => write!(f, "ipfs repo init failed: {m}"),
            IpfsError::Spawn(m) => write!(f, "ipfs daemon spawn error: {m}"),
            IpfsError::AlreadyRunning => write!(f, "ipfs daemon already running"),
        }
    }
}

/// Manages the bundled kubo daemon: resolved binary + per-user repo + supervisor.
pub struct IpfsManager {
    bin: PathBuf,
    repo_dir: PathBuf,
    crash_record_path: PathBuf,
    sup: Mutex<Option<Supervisor>>,
}

impl IpfsManager {
    pub fn new(bin: PathBuf, repo_dir: PathBuf, crash_record_path: PathBuf) -> Self {
        IpfsManager {
            bin,
            repo_dir,
            crash_record_path,
            sup: Mutex::new(None),
        }
    }

    /// Init the kubo repo on first use (idempotent — a repo with a `config` is left
    /// alone). Uses the `lowpower` profile (desktop-friendly) and moves the gateway
    /// off :8080. Blocking one-time setup before the daemon is supervised.
    fn ensure_init(&self) -> Result<(), IpfsError> {
        if self.repo_dir.join("config").is_file() {
            return Ok(()); // already initialized
        }
        std::fs::create_dir_all(&self.repo_dir)
            .map_err(|e| IpfsError::Init(format!("mkdir repo: {e}")))?;
        let out = Command::new(&self.bin)
            .env(IPFS_PATH_ENV, &self.repo_dir)
            .args(["init", "--profile", "lowpower"])
            .output()
            .map_err(|e| IpfsError::Init(format!("run ipfs init: {e}")))?;
        if !out.status.success() {
            return Err(IpfsError::Init(String::from_utf8_lossy(&out.stderr).to_string()));
        }
        // Best-effort: move the gateway off the common :8080 (the node only needs
        // the API on :5001). A failure here is non-fatal — the daemon still serves
        // the API; we just risk an :8080 clash.
        let _ = Command::new(&self.bin)
            .env(IPFS_PATH_ENV, &self.repo_dir)
            .args(["config", "Addresses.Gateway", GATEWAY_MULTIADDR])
            .output();
        Ok(())
    }

    /// The `ipfs daemon` spawn spec — repo via env (never argv), API on the default
    /// 127.0.0.1:5001 the node connects to.
    fn build_spec(&self) -> SidecarSpec {
        let mut spec = SidecarSpec::new(
            "ipfs",
            self.bin.clone(),
            vec!["daemon".to_string(), "--migrate=true".to_string()],
        );
        spec.env = vec![(IPFS_PATH_ENV.to_string(), self.repo_dir.to_string_lossy().to_string())];
        spec
    }

    /// Start the kubo daemon under the supervisor (idempotent-ish: `AlreadyRunning`
    /// if live). Inits the repo first. Fails closed if the binary is missing.
    pub fn start(&self) -> Result<(), IpfsError> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(IpfsError::AlreadyRunning);
        }
        if !self.bin.exists() {
            return Err(IpfsError::BinaryNotFound(self.bin.display().to_string()));
        }
        self.ensure_init()?;
        let mut config = SupervisorConfig::new(self.build_spec(), self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        let sup = Supervisor::start(config).map_err(|e| IpfsError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        Ok(())
    }

    /// Stop the daemon (graceful supervisor release; no orphan). Idempotent.
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

    /// Supervisor-reported running state (for a status surface).
    pub fn running(&self) -> bool {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        matches!(guard.as_ref().map(|s| s.status().state), Some(SupervisorState::Running))
    }
}

/// Managed Tauri state: the process-wide IPFS manager.
pub struct IpfsState(pub IpfsManager);

/// Resolve the bundled `ipfs` (kubo) binary: `CITRATE_IPFS_BIN` override first
/// (dev/tests), else the Tauri resource dir (externalBin, target-triple stripped).
fn resolve_ipfs_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CITRATE_IPFS_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("CITRATE_IPFS_BIN set but not found: {}", path.display()));
    }
    crate::supervisor::resolve_external_bin(app, "ipfs")
}

/// Build the managed IPFS state from a live app handle: bundled kubo binary + a
/// per-user repo (`ipfs/`) + crash-record path inside the app data dir.
pub fn build_ipfs_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<IpfsState, String> {
    use tauri::Manager;
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let repo_dir = data_root.join("ipfs");
    let crash_record_path = repo_dir.join("crash-records.jsonl");
    let bin = resolve_ipfs_bin(app)?;
    Ok(IpfsState(IpfsManager::new(bin, repo_dir, crash_record_path)))
}

/// Start the bundled IPFS daemon (idempotent). The node reaches it on 127.0.0.1:5001.
#[tauri::command]
pub fn ipfs_start(state: tauri::State<'_, IpfsState>) -> std::result::Result<(), String> {
    match state.0.start() {
        Ok(()) | Err(IpfsError::AlreadyRunning) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Stop the bundled IPFS daemon.
#[tauri::command]
pub fn ipfs_stop(state: tauri::State<'_, IpfsState>) -> std::result::Result<(), String> {
    state.0.stop();
    Ok(())
}

/// Whether the IPFS daemon is supervised-running (honest — never fabricated).
#[tauri::command]
pub fn ipfs_status(state: tauri::State<'_, IpfsState>) -> std::result::Result<bool, String> {
    Ok(state.0.running())
}
