//! CX-S4 (lane s4) — group private P2P cluster (C-20), daemon-backed.
//!
//! ## S4/CL-S2 — spawn the `cluster-daemon` and route the cluster over its UDS socket
//! A Group's cluster is a private P2P mesh among its members. The mesh + the admission engine live in
//! the standalone **citrate-cluster** daemon (Noise + gossipsub + the `cluster-core` RBAC gate — too
//! heavy to link into this lean tree, exactly like the comms member-daemon). citrate-core runs it as
//! a LOCAL supervised sidecar and speaks a light loopback Unix-socket JSON IPC to it. The admission
//! logic that lived here in S4.1–S4.3 has MOVED to its canonical home, `cluster-core` (ADR-2026-08-28
//! cluster-daemon-extraction) — this module is now a thin client, mirroring `comms.rs`.
//!
//! The cluster never re-derives group membership: citrate-core feeds it the roster (from the comms
//! member-daemon via `comms::groups_roster`) with `setRoster`, and the daemon reconciles the mesh +
//! enforces the RBAC boundary. ENV config (never argv — leaks to `ps`): `CITRATE_CLUSTER_SOCKET`,
//! `CITRATE_CLUSTER_BEARER_FILE` (0600 token file), `CITRATE_CLUSTER_SELF_ADDR` (this member's addr).
//!
//! ## Identity — cluster id = comms id (supersedes CL-2's `= wallet` clause)
//! The mesh admits by **address ∈ roster**, and the roster (from the comms member-daemon) keys on the
//! device-sealed **comms** address (Option A, comms.rs) — NOT the custody wallet address. citrate-cluster's
//! locked decision CL-2 (`WalletAddress = comms id = cluster Noise id`) predates that split and is stale:
//! comms mints a fresh, non-value per-device key, so wallet ≠ comms. So this node's cluster identity is
//! its **comms** identity — self_addr = the comms address, and (for libp2p) the Noise seed = the comms
//! key — which keeps Rule 3 intact (a non-value device key, never the wallet) and makes the address it
//! announces on the wire match the roster. See `docs/adr/ADR-2026-08-30-cluster-identity-and-transport.md`.
//! (The comms↔wallet on-chain binding is the deferred `wallet_link` attestation; nothing consumes it yet.)
//!
//! ## Transport — in-process by default, libp2p behind a soak-gated env flip (CL-S3)
//! In-process transport by default (single node — real admission, no cross-machine fan-out). When
//! citrate-core's own env carries `CITRATE_CLUSTER_LISTEN` (a listen multiaddr), we ALSO write the comms
//! seed to a 0600 file and forward `CITRATE_CLUSTER_SEED_FILE` + optional `CITRATE_CLUSTER_BOOTSTRAP`, so
//! the daemon runs the real cross-machine libp2p mesh. This is OFF by default and stays operator-only
//! until the two-machine soak + the Rule-8 transport sign-off (citrate-cluster Rule 8) — it must not be
//! flipped on for real partner traffic pre-audit. NOTE: the CL-S1 libp2p transport is one-group-per-daemon
//! (a per-group swarm reading the same listen addr), so use an ephemeral `/tcp/0` or one group per soak run;
//! multi-group cross-machine fan-out from this single daemon is a documented follow-on.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

// Issue #46 — cross-platform local IPC. `Stream` supplies `set_recv_timeout`/
// `set_send_timeout` (-> `UnixStream::set_read_timeout`/`set_write_timeout` on unix)
// and `TryClone` supplies `try_clone` (-> `UnixStream::try_clone` on unix), so the
// UDS framing below stays byte-identical to the pre-port path.
use interprocess::local_socket::traits::Stream as _;
use interprocess::TryClone as _;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::supervisor::{
    BackoffPolicy, HealthCheck, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// Env override for the bundled `cluster-daemon` binary path (dev/tests).
pub const CLUSTER_DAEMON_BIN_ENV: &str = "CITRATE_CLUSTER_BIN";

// The daemon's env knobs (cluster-daemon/src/main.rs).
const ENV_SOCKET: &str = "CITRATE_CLUSTER_SOCKET";
const ENV_BEARER_FILE: &str = "CITRATE_CLUSTER_BEARER_FILE";
const ENV_SELF_ADDR: &str = "CITRATE_CLUSTER_SELF_ADDR";
// libp2p transport knobs — only set when the operator opts in (CL-S3, soak-gated).
const ENV_LISTEN: &str = "CITRATE_CLUSTER_LISTEN"; // listen multiaddr; its presence SELECTS libp2p
const ENV_SEED_FILE: &str = "CITRATE_CLUSTER_SEED_FILE"; // 0600 file: the comms secp256k1 seed (Noise id)
const ENV_BOOTSTRAP: &str = "CITRATE_CLUSTER_BOOTSTRAP"; // optional comma-sep peer multiaddrs to dial

const TOKEN_LEN: usize = 32;
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);
const CLUSTER_HEALTHY_AFTER: Duration = Duration::from_secs(30);
const CLUSTER_START_GRACE: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Errors — coarse, secret-free.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ClusterError {
    /// No member identity yet (no wallet address to be this node's cluster id).
    NotConfigured,
    /// The bundled `cluster-daemon` binary could not be located (packaging gap).
    BinaryNotFound(String),
    /// The supervisor refused to start the sidecar.
    Spawn(String),
    /// The sidecar is already running (idempotent-start guard).
    AlreadyRunning,
    /// A loopback IPC failure (socket, bearer, framing) — no secret carried.
    Ipc(String),
}

impl std::fmt::Display for ClusterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterError::NotConfigured => write!(f, "cluster identity not available (no wallet)"),
            ClusterError::BinaryNotFound(m) => write!(f, "cluster-daemon binary not bundled: {m}"),
            ClusterError::Spawn(m) => write!(f, "cluster-daemon spawn error: {m}"),
            ClusterError::AlreadyRunning => write!(f, "cluster-daemon already running"),
            ClusterError::Ipc(m) => write!(f, "cluster ipc error: {m}"),
        }
    }
}
impl std::error::Error for ClusterError {}

type Result<T> = std::result::Result<T, ClusterError>;

// ---------------------------------------------------------------------------
// The manager — supervises the cluster-daemon sidecar + holds the IPC bearer.
// ---------------------------------------------------------------------------

/// Supervises the `cluster-daemon` sidecar (resolve binary → spawn ENV-configured → UDS-socket
/// liveness → bounded-backoff restart) and holds the session bearer for the IPC.
pub struct ClusterDaemonManager {
    bin: PathBuf,
    socket_path: PathBuf,
    bearer_path: PathBuf,
    data_dir: PathBuf,
    /// This node's member address (hex, its cluster identity = its comms address). Empty until known.
    self_addr: String,
    crash_record_path: PathBuf,
    health_interval: Duration,
    /// Real cross-machine libp2p transport config, when the operator has opted in (CL-S3). `None` →
    /// the single-node in-process transport (the default). Its `seed_hex` is the comms secp256k1 key.
    libp2p: Option<Libp2pOpts>,
    #[cfg(test)]
    spawn_args_override: Option<Vec<String>>,
    token: Mutex<Option<Zeroizing<String>>>,
    sup: Mutex<Option<Supervisor>>,
}

/// Opt-in libp2p transport config for the cluster daemon (CL-S3). Present only when the operator set
/// `CITRATE_CLUSTER_LISTEN` — otherwise the daemon stays on the in-process transport.
#[derive(Clone)]
pub struct Libp2pOpts {
    /// Listen multiaddr, e.g. `/ip4/0.0.0.0/tcp/0`. Its presence is what selects libp2p in the daemon.
    pub listen: String,
    /// Optional comma-separated peer multiaddrs to dial on startup.
    pub bootstrap: Option<String>,
    /// The comms secp256k1 seed hex — the Noise/peer identity binds to this key (never the wallet).
    /// Written to a 0600 file at start; the daemon reads the PATH, so the secret never crosses env/argv.
    pub seed_hex: Zeroizing<String>,
}

impl ClusterDaemonManager {
    pub fn new(
        bin: PathBuf,
        socket_path: PathBuf,
        bearer_path: PathBuf,
        data_dir: PathBuf,
        self_addr: impl Into<String>,
        crash_record_path: PathBuf,
    ) -> Self {
        ClusterDaemonManager {
            bin,
            socket_path,
            bearer_path,
            data_dir,
            self_addr: self_addr.into(),
            crash_record_path,
            health_interval: HEALTH_INTERVAL,
            libp2p: None,
            #[cfg(test)]
            spawn_args_override: None,
            token: Mutex::new(None),
            sup: Mutex::new(None),
        }
    }

    /// Opt into the real cross-machine libp2p transport (CL-S3). Off unless called — the default is the
    /// single-node in-process transport. Soak-gated: only the operator (env-driven) turns this on.
    pub fn with_libp2p(mut self, opts: Libp2pOpts) -> Self {
        self.libp2p = Some(opts);
        self
    }

    /// The 0600 file the comms seed is written to at start (libp2p only). The daemon reads the PATH, so
    /// the secret never crosses env/argv; derived from `data_dir` (the seed itself lives in the keyring).
    fn seed_path(&self) -> PathBuf {
        self.data_dir.join("cluster.seed")
    }

    #[cfg(test)]
    pub fn with_health_interval(mut self, interval: Duration) -> Self {
        self.health_interval = interval;
        self
    }
    #[cfg(test)]
    pub fn with_spawn_args(mut self, args: Vec<String>) -> Self {
        self.spawn_args_override = Some(args);
        self
    }

    /// Whether the daemon can start — this node has a member identity (a wallet address).
    pub fn is_configured(&self) -> bool {
        !self.self_addr.is_empty()
    }

    #[cfg(test)]
    fn effective_spawn_args(&self) -> Vec<String> {
        self.spawn_args_override.clone().unwrap_or_default()
    }
    #[cfg(not(test))]
    fn effective_spawn_args(&self) -> Vec<String> {
        Vec::new()
    }

    /// Build the [`SidecarSpec`]: ENV carries the socket + bearer-file PATH + this node's address
    /// (never a token in argv). Liveness = the UDS socket accepts a connection (no HTTP).
    fn build_spec(&self) -> SidecarSpec {
        let mut spec =
            SidecarSpec::new("cluster-daemon", self.bin.clone(), self.effective_spawn_args());
        spec.env = vec![
            (ENV_SOCKET.to_string(), self.socket_path.to_string_lossy().to_string()),
            (ENV_BEARER_FILE.to_string(), self.bearer_path.to_string_lossy().to_string()),
            (ENV_SELF_ADDR.to_string(), self.self_addr.clone()),
        ];
        // libp2p (opt-in): the LISTEN addr selects the real transport; the seed crosses as a 0600 file
        // PATH (written in `start`), never inline; bootstrap is optional. Absent → in-process default.
        if let Some(opts) = &self.libp2p {
            spec.env.push((ENV_LISTEN.to_string(), opts.listen.clone()));
            spec.env.push((ENV_SEED_FILE.to_string(), self.seed_path().to_string_lossy().to_string()));
            if let Some(b) = &opts.bootstrap {
                spec.env.push((ENV_BOOTSTRAP.to_string(), b.clone()));
            }
        }
        let sock = self.socket_path.clone();
        spec.health_check = Some(HealthCheck {
            interval: self.health_interval,
            grace: CLUSTER_START_GRACE,
            probe: std::sync::Arc::new(move || {
                crate::ipc_name::connect(&sock.to_string_lossy()).is_ok()
            }),
        });
        spec
    }

    #[cfg(test)]
    pub fn spec_env_for_test(&self) -> Vec<(String, String)> {
        self.build_spec().env
    }

    /// Start the sidecar IFF the identity is present AND the binary exists. Mints a fresh session
    /// bearer, persists it 0600 (the daemon adopts it), spawns under the supervisor. Fails CLOSED.
    pub fn start(&self) -> Result<()> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(ClusterError::AlreadyRunning);
        }
        if !self.is_configured() {
            return Err(ClusterError::NotConfigured);
        }
        if !self.bin.exists() {
            return Err(ClusterError::BinaryNotFound(self.bin.display().to_string()));
        }
        std::fs::create_dir_all(&self.data_dir).ok();
        let token = mint_bearer();
        persist_secret_0600(&self.bearer_path, &token)?;
        // libp2p only: write the comms seed to its 0600 file — the daemon reads the PATH, so the
        // secret never crosses env/argv. In-process mode needs no seed and writes nothing.
        if let Some(opts) = &self.libp2p {
            persist_secret_0600(&self.seed_path(), &opts.seed_hex)?;
        }
        let spec = self.build_spec();
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = CLUSTER_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| ClusterError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = Some(token);
        Ok(())
    }

    #[allow(dead_code)] // graceful teardown; used by tests + a future shutdown hook
    pub fn stop(&self) {
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(sup) = sup {
            sup.stop();
            drop(sup);
        }
        let _ = std::fs::remove_file(&self.bearer_path);
        // Don't leave the comms seed on disk after teardown (libp2p only; a no-op otherwise).
        let _ = std::fs::remove_file(self.seed_path());
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn is_running(&self) -> bool {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        matches!(
            guard.as_ref().map(|s| s.status().state),
            Some(SupervisorState::Running)
        )
    }

    fn bearer(&self) -> Option<Zeroizing<String>> {
        self.token.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Send one request to the daemon over its UDS socket and return the response.
    fn ipc(&self, req: &Request) -> Result<Response> {
        let bearer = self.bearer().ok_or(ClusterError::NotConfigured)?;
        cluster_ipc(&self.socket_path, &bearer, req)
    }
}

// ---- bearer token (mirrors comms.rs / hermes.rs) ----

fn mint_bearer() -> Zeroizing<String> {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = Zeroizing::new([0u8; TOKEN_LEN]);
    OsRng.fill_bytes(bytes.as_mut());
    Zeroizing::new(hex::encode(bytes.as_ref()))
}

// CORE-B-001: routed through the shared [`citrate_core_kit::fsutil`] writer, which
// creates the file `0600` in the `open(2)` call itself — the seed/bearer is never
// world-readable in the window a `fs::write`-then-`chmod` left open.
fn persist_secret_0600(path: &Path, secret: &str) -> Result<()> {
    citrate_core_kit::fsutil::write_secret_file(path, secret.as_bytes())
        .map_err(|e| ClusterError::Ipc(e.kind().to_string()))
}

/// Resolve the bundled `cluster-daemon` binary (env override → resource dir).
pub fn resolve_cluster_daemon_bin(app: &tauri::AppHandle) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var(CLUSTER_DAEMON_BIN_ENV) {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("{CLUSTER_DAEMON_BIN_ENV} set but not found: {}", path.display()));
    }
    // externalBin lives next to the main executable (Contents/MacOS/<name>), not the resource dir.
    crate::supervisor::resolve_external_bin(app, "cluster-daemon")
}

// ---------------------------------------------------------------------------
// The UDS JSON client — mirrors cluster-daemon::ipc.
// ---------------------------------------------------------------------------

/// A request to the daemon. `op`-tagged; mirrors cluster-daemon::ipc::Request.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Request {
    SetRoster { group: String, roster: Vec<(String, String)> },
    Join { group: String },
    Leave { group: String },
    Status { group: String },
    Peers { group: String },
    ShareFile { group: String, cid: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerView {
    address: String,
    online: bool,
}

/// A response from the daemon. `type`-tagged; mirrors cluster-daemon::ipc::Response.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Response {
    Ok,
    Reconciled {
        #[allow(dead_code)]
        evicted: Vec<String>,
    },
    Status {
        online: usize,
        total: usize,
        /// The group's co-pinned file set. `default` so this stays compatible with a daemon that
        /// predates the co-pin field (CL-S2 daemon side).
        #[serde(default, rename = "sharedFiles")]
        shared_files: Vec<String>,
    },
    Peers {
        peers: Vec<PeerView>,
    },
    Error {
        message: String,
    },
}

/// Read/write deadline on the cluster-daemon socket. BOUNDS every IPC so a daemon that accepts the
/// connection but stalls (e.g. a soak-gated libp2p transport that isn't ready) can never block the
/// caller forever — the UI-thread pinwheel on the Cluster tab came from an unbounded `read_line`
/// here. On timeout the call returns an honest `Ipc` error and the surface shows its empty state.
const CLUSTER_IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Connect to the daemon's UDS, authenticate with the bearer, send one request, read one response.
fn cluster_ipc(socket_path: &Path, bearer: &str, req: &Request) -> Result<Response> {
    let stream = crate::ipc_name::connect(&socket_path.to_string_lossy())
        .map_err(|e| ClusterError::Ipc(format!("connect {}: {e}", socket_path.display())))?;
    stream
        .set_recv_timeout(Some(CLUSTER_IPC_TIMEOUT))
        .map_err(|e| ClusterError::Ipc(e.to_string()))?;
    stream
        .set_send_timeout(Some(CLUSTER_IPC_TIMEOUT))
        .map_err(|e| ClusterError::Ipc(e.to_string()))?;
    let mut w = stream.try_clone().map_err(|e| ClusterError::Ipc(e.to_string()))?;
    let _ = w.set_send_timeout(Some(CLUSTER_IPC_TIMEOUT));
    let _ = w.set_recv_timeout(Some(CLUSTER_IPC_TIMEOUT));
    let mut r = BufReader::new(stream);

    // Auth handshake: {"token":"..."} → {"type":"ready"}.
    writeln!(w, "{{\"token\":\"{bearer}\"}}").map_err(|e| ClusterError::Ipc(e.to_string()))?;
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| ClusterError::Ipc(e.to_string()))?;
    if !line.contains("ready") {
        return Err(ClusterError::Ipc(format!("auth rejected: {}", line.trim())));
    }

    // One request line → one response line.
    let body = serde_json::to_string(req).map_err(|e| ClusterError::Ipc(e.to_string()))?;
    writeln!(w, "{body}").map_err(|e| ClusterError::Ipc(e.to_string()))?;
    line.clear();
    r.read_line(&mut line).map_err(|e| ClusterError::Ipc(e.to_string()))?;
    serde_json::from_str::<Response>(line.trim())
        .map_err(|e| ClusterError::Ipc(format!("decode: {e}: {}", line.trim())))
}

// ---------------------------------------------------------------------------
// The bridge DTOs — serialize to the frozen ClusterStatus / ClusterPeer shapes.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterStatusDto {
    group_id: String,
    online: usize,
    total: usize,
    shared_files: Vec<String>,
}

#[derive(Serialize)]
pub struct ClusterPeerDto {
    address: String,
    online: bool,
}

// ---------------------------------------------------------------------------
// The lazy-start singleton + roster feed from comms.
// ---------------------------------------------------------------------------

static MANAGER: OnceLock<ClusterDaemonManager> = OnceLock::new();
/// Serializes first-time manager init so `mgr.start()` (mint bearer → write the bearer file → spawn the
/// daemon) happens for EXACTLY ONE manager. Without it, two concurrent first calls (e.g. the cluster
/// tab opening + a roster/file load) each build + start their own manager, spawning two daemons and
/// overwriting `cluster.bearer`; `get_or_init` then commits one manager whose in-memory bearer no
/// longer matches the surviving daemon's file → the IPC handshake is rejected "unauthorized". Mirrors
/// the comms.rs fix (device identity + bearer scheme is the same).
static MANAGER_INIT: Mutex<()> = Mutex::new(());

/// Stop the cluster-daemon if this session started it (called on graceful app teardown). Removes the
/// seed file and drops the mesh cleanly. Idempotent and a no-op if the daemon was never started.
pub fn shutdown() {
    if let Some(m) = MANAGER.get() {
        m.stop();
    }
}

/// The opt-in libp2p transport config, read from citrate-core's OWN env, paired with the comms seed.
/// `CITRATE_CLUSTER_LISTEN` present → real cross-machine mesh (CL-S3, soak-gated); absent → `None` =
/// in-process. Kept env-driven (not a UI toggle) so it cannot be flipped on for partner traffic before
/// the two-machine soak + the Rule-8 transport sign-off.
fn libp2p_opts_from_env(seed_hex: Zeroizing<String>) -> Option<Libp2pOpts> {
    let listen = std::env::var(ENV_LISTEN).ok().filter(|s| !s.trim().is_empty())?;
    let bootstrap = std::env::var(ENV_BOOTSTRAP).ok().filter(|s| !s.trim().is_empty());
    Some(Libp2pOpts { listen, bootstrap, seed_hex })
}

/// Ensure the daemon is built + started; returns the process-wide manager. Lazy singleton.
fn ensure_started(app: &tauri::AppHandle) -> std::result::Result<&'static ClusterDaemonManager, String> {
    use tauri::Manager;
    if let Some(m) = MANAGER.get() {
        if !m.is_running() {
            m.start().map_err(|e| e.to_string())?;
        }
        return Ok(m);
    }
    // Serialize first-time init (double-checked): only ONE thread builds + starts + commits the
    // manager, so the surviving daemon's bearer always matches the committed manager's (no
    // "unauthorized"). A concurrent caller that lost the race re-reads the committed manager here.
    let _init = MANAGER_INIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = MANAGER.get() {
        if !m.is_running() {
            m.start().map_err(|e| e.to_string())?;
        }
        return Ok(m);
    }
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("cluster");
    // Identity = the device-sealed COMMS key (comms.rs), NOT the wallet — the roster keys on the comms
    // address, so self_addr and (for libp2p) the Noise seed must be the comms identity to match it.
    let identity = crate::comms::device_identity(app)?;
    let libp2p = libp2p_opts_from_env(identity.seed_hex);
    let bin = resolve_cluster_daemon_bin(app)?;
    let mut mgr = ClusterDaemonManager::new(
        bin,
        data_root.join("cluster.sock"),
        data_root.join("cluster.bearer"),
        data_root.clone(),
        identity.address,
        data_root.join("cluster-crash.jsonl"),
    );
    if let Some(opts) = libp2p {
        mgr = mgr.with_libp2p(opts);
    }
    mgr.start().map_err(|e| e.to_string())?;
    Ok(MANAGER.get_or_init(|| mgr))
}

/// Route one request to the running daemon.
fn route(app: &tauri::AppHandle, req: Request) -> std::result::Result<Response, String> {
    ensure_started(app)?.ipc(&req).map_err(|e| e.to_string())
}

/// Push the group's current roster (from the comms member-daemon) to the cluster daemon so it
/// reconciles the mesh. The cluster is a Group's cluster — no group/roster means no cluster.
async fn feed_roster(app: &tauri::AppHandle, group: &str) -> std::result::Result<(), String> {
    let roster = crate::comms::groups_roster(app.clone(), group.to_string()).await?;
    match route(app, Request::SetRoster { group: group.to_string(), roster })? {
        Response::Reconciled { .. } | Response::Ok => Ok(()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

fn parse_ok(r: Response) -> std::result::Result<(), String> {
    match r {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Tauri command surface — cluster: status / join / peers / shareFile / leave.
// ---------------------------------------------------------------------------

/// **cluster_status** — the group's cluster status (connected / authorized + shared files).
#[tauri::command]
pub async fn cluster_status(app: tauri::AppHandle, group: String) -> std::result::Result<ClusterStatusDto, String> {
    feed_roster(&app, &group).await?;
    match route(&app, Request::Status { group: group.clone() })? {
        Response::Status { online, total, shared_files } => {
            Ok(ClusterStatusDto { group_id: group, online, total, shared_files })
        }
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **cluster_peers** — the group's authorized peers with live connection state.
#[tauri::command]
pub async fn cluster_peers(app: tauri::AppHandle, group: String) -> std::result::Result<Vec<ClusterPeerDto>, String> {
    feed_roster(&app, &group).await?;
    match route(&app, Request::Peers { group })? {
        Response::Peers { peers } => {
            Ok(peers.into_iter().map(|p| ClusterPeerDto { address: p.address, online: p.online }).collect())
        }
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **cluster_join** — this node joins the group's mesh.
#[tauri::command]
pub async fn cluster_join(app: tauri::AppHandle, group: String) -> std::result::Result<(), String> {
    feed_roster(&app, &group).await?;
    parse_ok(route(&app, Request::Join { group })?)
}

/// **cluster_share_file** — announce a co-pinned CID to the group over the mesh.
#[tauri::command]
pub async fn cluster_share_file(app: tauri::AppHandle, group: String, cid: String) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::ShareFile { group, cid })?)
}

/// **cluster_leave** — this node leaves the group's mesh.
#[tauri::command]
pub async fn cluster_leave(app: tauri::AppHandle, group: String) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::Leave { group })?)
}

#[cfg(test)]
mod tests {
    include!("cluster_tests.rs");
}
