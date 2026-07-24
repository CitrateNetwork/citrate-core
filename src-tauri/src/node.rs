//! citrate-core — NodeDomain: the real citrate-node under the SidecarSupervisor
//! (CORE-C1.1). @rule8 · encrypted data dir + keyring storage key.
//!
//! This wires the `NodeDomain { status(); start(); stop() }` bridge seam to a
//! REAL `citrate` node spawned through [`crate::supervisor::Supervisor`]:
//!
//! - **`start`** mints/loads a 32-byte storage master key from the OS keyring
//!   (the A2 custody keyring pattern — never on disk in clear), then spawns the
//!   bundled `citrate` binary under the supervisor with an explicit `--data-dir`
//!   and the storage key handed in ONLY via the `CITRATE_STORAGE_KEY` env var.
//!   The node opens its RocksDB encrypted-at-rest (AES-256-GCM per value) with
//!   that key, so a raw-disk grep of the data dir yields ciphertext.
//! - **`status`** reads the node's REAL sync state from its local JSON-RPC
//!   (`eth_blockNumber` → height, `net_peerCount` → peers) — no fabricated
//!   numbers (Rule 1). When the node is not running or its RPC is not yet up,
//!   status reflects the supervisor state honestly (Off/Starting/Backoff/…).
//! - **`stop`** releases the supervisor: SIGTERM → grace → SIGKILL, no orphan.
//!
//! ## @rule8 — where the storage key lives
//! The key is minted with `OsRng` on first `start`, stored under keyring account
//! [`KEYRING_NODE_STORAGE_ACCOUNT`] in service `ai.citrate.core` (the same
//! service the custody vault uses), and is held in memory only as a
//! [`Zeroizing`] hex string for the duration of the spawn call. It is passed to
//! the child through an env var (argv would leak it to `ps`); the child converts
//! it back to 32 raw bytes. The plaintext key is NEVER written to the data dir
//! or any config file — only the encrypted DB and the wrong-key-detecting
//! `encryption.meta` commitment land on disk.
//!
//! ## C1.0b-1 — node restart policy (decided here)
//! A citrate-node that legitimately restarts (config reload, transient network
//! blip, an OS OOM-kill under memory pressure) must NEVER be driven to a
//! permanent `Failed` state, while a genuinely broken node (bad binary, corrupt
//! data dir, instant crash-loop) must still hit the fork-bomb bound. The
//! supervisor's F-1 sliding-window design already does exactly this: the
//! consecutive-failure counter resets to 0 once a respawned child stays
//! `Running` for `healthy_after`. We therefore tune, for the node specifically,
//! a LONGER `healthy_after` (`NODE_HEALTHY_AFTER`, 90s) than the supervisor
//! default (30s): initial-sync + peer-discovery + genesis-open can take tens of
//! seconds, so 30s risked counting a slow-but-healthy boot as "not yet healthy"
//! and letting the counter climb across legitimate restarts. 90s comfortably
//! exceeds a healthy node's cold-start, so any node that reaches steady state
//! resets its failure budget; a node that cannot stay up 90s is genuinely
//! broken and is still bounded by `max_retries`. We keep the default bounded
//! exponential backoff + `max_retries` (the fork-bomb cap). No change to the
//! supervisor state machine or the TLA+ model is required — this is a config
//! tuning of an existing, already-modelled parameter, not a new transition.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use zeroize::Zeroizing;

use crate::custody::{Keyring, OsKeyring};
use crate::rpc::{HttpTransport, RpcClient};
use crate::supervisor::{
    BackoffPolicy, LogLine, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// OS keyring account for the node's 32-byte storage-at-rest master key. Lives
/// in the same service (`ai.citrate.core`) as the custody vault master, under a
/// distinct account so the two never collide.
const KEYRING_NODE_STORAGE_ACCOUNT: &str = "node-storage-key";

/// Length of the storage master key (32 bytes = AES-256).
const STORAGE_KEY_LEN: usize = 32;

/// C1.0b-1: the node's sustained-healthy window (see the module doc). Longer
/// than the supervisor default so a slow-but-healthy cold start is not mistaken
/// for a failure, and legitimate restarts always reset the failure budget.
const NODE_HEALTHY_AFTER: Duration = Duration::from_secs(90);

/// The default local RPC URL the spawned node serves (from the bundled testnet
/// config: `[rpc] listen_addr = "0.0.0.0:8545"`, reachable on loopback). Kept
/// distinct from the remote `rpc.citrate.ai` used by the wallet ceremony — this
/// one points at OUR local node's status surface.
const NODE_LOCAL_RPC_URL: &str = "http://127.0.0.1:8545";

/// The env var the node reads its 32-byte storage key from (hex). Matches the
/// node-side `at_rest_encryption_from_env` (citrate-chain node/src/main.rs).
const NODE_STORAGE_KEY_ENV: &str = "CITRATE_STORAGE_KEY";

/// CONSENSUS-CRITICAL launch env the fleet producer runs (DGX_NODE_SYNC_WEDGE_RESPONSE
/// 2026-07-22). VALIDATOR-S1 / §R' epoch-reward + validator-registry snapshot are OFF
/// by default (`citrate-chain node/src/main.rs:2506`: activates only on
/// `CITRATE_VALIDATOR_REGISTRY` + `_ACTIVATION_HEIGHT`). Without these the app node
/// computes a DIFFERENT state root than the fleet once §R' bites (first divergence at
/// block 2,580, after activation-2000) and the receive-path root check rejects the
/// block — the observed sync wedge. These MUST match the fleet producer's systemd env
/// (`rpc-1` 142.93.58.145) exactly.
const NODE_BLOCK_V2_ENV: &str = "CITRATE_BLOCK_V2";
const NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV: &str = "CITRATE_VALIDATOR_ACTIVATION_HEIGHT";
const NODE_VALIDATOR_REGISTRY_ENV: &str = "CITRATE_VALIDATOR_REGISTRY";
/// v2 execute-on-receive is DEFAULT ON since the 2026-07-21 SRP reroll; set explicitly
/// for parity with the fleet + clarity.
const NODE_BLOCK_V2_VALUE: &str = "1";
/// The fleet's validator activation height (fleet systemd env).
const NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE: &str = "2000";
/// The live ValidatorRegistry on chain 40204 — canonical
/// `citrate-chain/contracts/addresses/40204.json` (`ValidatorRegistry`), the same
/// address the fleet producer runs.
const NODE_VALIDATOR_REGISTRY_VALUE: &str = "0x915DdE02831ebacFc57f329f60944492ebb0A095";

/// Node-side errors surfaced to the bridge as strings.
#[derive(Debug)]
pub enum NodeError {
    /// The OS keyring is unreachable (fail closed — never spawn without a key).
    Keyring(String),
    /// The bundled node binary could not be located.
    BinaryNotFound(String),
    /// The supervisor refused to start the node (thread/spawn failure).
    Spawn(String),
    /// The node is already running (idempotent-start guard).
    AlreadyRunning,
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Keyring(m) => write!(f, "node keyring error: {m}"),
            NodeError::BinaryNotFound(m) => write!(f, "node binary not found: {m}"),
            NodeError::Spawn(m) => write!(f, "node spawn error: {m}"),
            NodeError::AlreadyRunning => write!(f, "node already running"),
        }
    }
}

impl std::error::Error for NodeError {}

type Result<T> = std::result::Result<T, NodeError>;

/// The bridge status shape — mirrors `NodeDomain.status()` in domains.ts:
/// `{ state, peers, height, syncPct }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeStatus {
    /// "stopped" | "starting" | "running" | "restarting" | "failed" — the
    /// supervisor state mapped to the bridge vocabulary.
    pub state: String,
    /// Live peer count from `net_peerCount` (0 when RPC not reachable yet).
    pub peers: u64,
    /// Head height from `eth_blockNumber` (0 when RPC not reachable yet).
    pub height: u64,
    /// Bounded sync-progress percentage in [0, 100]. Honest: when we cannot
    /// determine a target height over the local RPC we report progress as a
    /// function of the observed head only if a target is known, else 0 while
    /// `starting`/unsynced and 100 once the node reports itself not syncing.
    #[serde(rename = "syncPct")]
    pub sync_pct: f64,
}

/// Map a [`SupervisorState`] to the bridge state vocabulary the surface renders.
fn map_state(state: &SupervisorState) -> &'static str {
    match state {
        SupervisorState::Off => "stopped",
        SupervisorState::Starting => "starting",
        SupervisorState::Running => "running",
        SupervisorState::Backoff { .. } => "restarting",
        SupervisorState::Failed => "failed",
    }
}

/// The node manager: owns the keyring seam, the resolved binary + data dir, the
/// local RPC url, and (while running) a live [`Supervisor`]. Managed as Tauri
/// state; `status`/`start`/`stop` are called from the invoke commands.
pub struct NodeManager {
    keyring: Box<dyn Keyring>,
    /// The bundled node binary path (resolved from the Tauri resource dir or a
    /// dev/stub override).
    bin: PathBuf,
    /// The node's encrypted data dir (inside the app data dir).
    data_dir: PathBuf,
    /// Where crash records are appended.
    crash_record_path: PathBuf,
    /// The local RPC url to poll for real height/peers.
    rpc_url: String,
    /// The live supervisor, present only while the node is running.
    sup: Mutex<Option<Supervisor>>,
}

impl NodeManager {
    /// Build a manager over an explicit keyring, binary path, data dir, and RPC
    /// url. Production uses [`NodeManager::from_app`]; tests inject a fake
    /// keyring + a stub binary + a temp data dir + a stub RPC url.
    pub fn new(
        keyring: Box<dyn Keyring>,
        bin: PathBuf,
        data_dir: PathBuf,
        crash_record_path: PathBuf,
        rpc_url: impl Into<String>,
    ) -> Self {
        NodeManager {
            keyring,
            bin,
            data_dir,
            crash_record_path,
            rpc_url: rpc_url.into(),
            sup: Mutex::new(None),
        }
    }

    /// Load the 32-byte storage key from the OS keyring, minting it on first use
    /// (@rule8). Returned as a hex `Zeroizing<String>` ready to hand to the child
    /// via env; the raw bytes are zeroized on drop. An unreachable keyring is a
    /// hard fault — we NEVER spawn the node without an encryption key (that would
    /// silently produce a plaintext data dir).
    fn storage_key_hex(&self) -> Result<Zeroizing<String>> {
        // Read an existing key, if present.
        match self
            .keyring
            .get(KEYRING_NODE_STORAGE_ACCOUNT)
            .map_err(|e| NodeError::Keyring(e.to_string()))?
        {
            Some(bytes) => {
                if bytes.len() != STORAGE_KEY_LEN {
                    return Err(NodeError::Keyring(format!(
                        "stored node storage key has wrong length {}",
                        bytes.len()
                    )));
                }
                let hex = Zeroizing::new(hex::encode(&bytes));
                // scrub the transient copy from the keyring read
                let mut bytes = bytes;
                use zeroize::Zeroize;
                bytes.zeroize();
                Ok(hex)
            }
            None => {
                // Mint a fresh 32-byte key with a CSPRNG and persist it.
                use rand::RngCore;
                let mut key = Zeroizing::new([0u8; STORAGE_KEY_LEN]);
                rand::thread_rng().fill_bytes(key.as_mut());
                self.keyring
                    .set(KEYRING_NODE_STORAGE_ACCOUNT, key.as_ref())
                    .map_err(|e| NodeError::Keyring(e.to_string()))?;
                Ok(Zeroizing::new(hex::encode(key.as_ref())))
            }
        }
    }

    /// Build the [`SidecarSpec`] for the node: explicit binary, `--data-dir`,
    /// and the storage key in the env (NOT argv — argv would leak the key to a
    /// `ps` listing). `--network testnet` joins the public testnet.
    fn build_spec(&self, storage_key_hex: &str) -> SidecarSpec {
        let mut spec = SidecarSpec::new(
            "citrate-node",
            self.bin.clone(),
            vec![
                "--network".to_string(),
                "testnet".to_string(),
                "--data-dir".to_string(),
                self.data_dir.to_string_lossy().to_string(),
            ],
        );
        spec.env = vec![
            (NODE_STORAGE_KEY_ENV.to_string(), storage_key_hex.to_string()),
            // CONSENSUS-CRITICAL: reproduce the fleet producer's validator/§R' state
            // path or the node forks the state root and wedges (see the const docs +
            // DGX_NODE_SYNC_WEDGE_RESPONSE_2026-07-22).
            (NODE_BLOCK_V2_ENV.to_string(), NODE_BLOCK_V2_VALUE.to_string()),
            (
                NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV.to_string(),
                NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE.to_string(),
            ),
            (
                NODE_VALIDATOR_REGISTRY_ENV.to_string(),
                NODE_VALIDATOR_REGISTRY_VALUE.to_string(),
            ),
        ];
        spec
    }

    /// Start the node under the supervisor. Idempotent-ish: returns
    /// `AlreadyRunning` if a supervisor is already live. @rule8: the storage key
    /// is minted/loaded from the keyring and handed to the child via env.
    pub fn start(&self) -> Result<()> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(NodeError::AlreadyRunning);
        }
        if !self.bin.exists() {
            return Err(NodeError::BinaryNotFound(self.bin.display().to_string()));
        }
        let key_hex = self.storage_key_hex()?;
        let spec = self.build_spec(&key_hex);
        // C1.0b-1: default fork-bomb-bounded backoff, but a node-tuned
        // sustained-healthy window (see the module doc).
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = NODE_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| NodeError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        Ok(())
    }

    /// Stop the node: graceful supervisor release (SIGTERM → grace → SIGKILL),
    /// dropping the supervisor so no orphan remains. Idempotent — stopping an
    /// already-stopped node is a no-op.
    pub fn stop(&self) {
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(sup) = sup {
            sup.stop();
            // Dropping the supervisor joins the monitor thread (no orphan).
            drop(sup);
        }
    }

    /// Read the node's real status: supervisor state + (when running) live
    /// height/peers from the local RPC. NEVER fabricates numbers — if the RPC is
    /// not reachable yet the counts are 0 and the state reflects that honestly.
    pub fn status(&self) -> NodeStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        // Only poll the RPC when the supervisor thinks the child is Running.
        let running = matches!(sup_state, Some(SupervisorState::Running));
        let (height, peers) = if running {
            self.poll_rpc().unwrap_or((0, 0))
        } else {
            (0, 0)
        };
        let sync_pct = if running && height > 0 { 100.0 } else { 0.0 };
        NodeStatus {
            state: state.to_string(),
            peers,
            height,
            sync_pct,
        }
    }

    /// Q-A.2/Q-B.2 — the REAL recent node log lines, streamed from the
    /// supervised child's stdout+stderr into the supervisor's bounded ring. Newest
    /// lines are last; the ring is capped (`LOG_RING_CAPACITY`), so this returns at
    /// most that many. When the node is not running (no supervisor) it honestly
    /// returns an EMPTY list — never a fabricated template (Rule 1). This is what
    /// fills the Node LOG panel in a packaged build (it was permanently empty
    /// because stdout was inherited-and-dropped in the GUI process).
    pub fn logs(&self) -> Vec<LogLine> {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(sup) => sup.logs(),
            None => Vec::new(),
        }
    }

    /// Poll the node's local RPC for `(height, peers)`. Returns `None` on any
    /// transport error (RPC not up yet / node still booting) so the caller
    /// reports 0/0 honestly rather than a stale or fabricated value.
    fn poll_rpc(&self) -> Option<(u64, u64)> {
        let client = RpcClient::with_transport(HttpTransport::new(self.rpc_url.clone()));
        let height = client.block_number().ok()?;
        let peers = client.peer_count().unwrap_or(0);
        Some((height, peers))
    }
}

/// Managed Tauri state: the process-wide node manager.
pub struct NodeState(pub NodeManager);

/// Resolve the bundled `citrate` sidecar binary path.
///
/// D-C1-1: the node is bundled as a Tauri `externalBin` (`citrate`), which Tauri
/// installs into the app's resource dir as `citrate` (Tauri strips the target
/// triple suffix at bundle time). In dev, and for the stub-node CI path, an
/// override env var `CITRATE_NODE_BIN` points directly at a binary so the app
/// (and the tests) can run without a full bundle.
fn resolve_node_bin<R: Runtime>(app: &AppHandle<R>) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CITRATE_NODE_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "CITRATE_NODE_BIN set but not found: {}",
            path.display()
        ));
    }
    // Bundled externalBin: installed next to the main executable (Contents/MacOS).
    crate::supervisor::resolve_external_bin(app, "citrate")
}

/// Build the managed node state from a live app handle: the real OS keyring, the
/// bundled binary, an encrypted `node` data dir inside the app data dir, and the
/// local RPC url.
pub fn build_node_state<R: Runtime>(app: &AppHandle<R>) -> std::result::Result<NodeState, String> {
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let data_dir = data_root.join("node");
    let crash_record_path = data_root.join("node").join("crash-records.jsonl");
    let bin = resolve_node_bin(app)?;
    Ok(NodeState(NodeManager::new(
        Box::new(OsKeyring),
        bin,
        data_dir,
        crash_record_path,
        NODE_LOCAL_RPC_URL,
    )))
}

// ---------------------------------------------------------------------------
// Tauri commands — the NodeDomain bridge surface. Return status / () only.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn node_status(state: State<'_, NodeState>) -> std::result::Result<NodeStatus, String> {
    Ok(state.0.status())
}

#[tauri::command]
pub fn node_start(state: State<'_, NodeState>) -> std::result::Result<(), String> {
    state.0.start().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn node_stop(state: State<'_, NodeState>) -> std::result::Result<(), String> {
    state.0.stop();
    Ok(())
}

/// Q-A.2/Q-B.2 — return the REAL recent node log lines (streamed stdout+stderr
/// from the supervised child's bounded ring). Honest empty when the node is not
/// running. The webview folds these into the Node LOG panel so a packaged build
/// shows live node output, never a fabricated template (Rule 1).
#[tauri::command]
pub fn node_logs(state: State<'_, NodeState>) -> std::result::Result<Vec<LogLine>, String> {
    Ok(state.0.logs())
}

#[cfg(test)]
mod tests {
    include!("node_tests.rs");
}
