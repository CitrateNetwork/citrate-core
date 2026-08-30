//! CX-S3 (lane s3) — groups: secure comms + admin RBAC host commands (C-19).
//!
//! ## S3.2 — spawn the `comms-member-daemon` and route Groups over its UDS socket
//! Commons runs the citrate-comms **member daemon** (comms-member-daemon: the wallet-owned OpenMLS
//! member + an in-process server-blind relay) as a LOCAL supervised sidecar, and speaks a light
//! loopback Unix-socket JSON IPC to it. All the heavy MLS/relay/tokio weight lives in that sidecar;
//! this crate links NO comms crate (lean-tree) — it spawns a binary and speaks JSON. The daemon
//! subsumes the S3.1 bare relay for the local single-node case (D-C1/D-C2 reconciliation).
//!
//! ENV config to the daemon (never argv — argv leaks to `ps`): `CITRATE_MEMBER_SOCKET` (the UDS the
//! daemon binds + we connect to), `CITRATE_MEMBER_BEARER_FILE` (a 0600 token file, the IPC gate),
//! `CITRATE_MEMBER_SEED_FILE` (a 0600 file holding the signing seed — never inline), `CITRATE_MEMBER_DOMAIN`.
//!
//! ## Identity — Option A: device-sealed comms key (confirmed 2026-08-27)
//! The daemon needs a signing seed but Rule 3 forbids exporting the main custody wallet. Decision: a
//! FRESH, scoped secp256k1 comms key, sealed in the OS keyring (account `comms-member-key`, legacy
//! custody service) and NEVER exported — the same custody pattern as the node storage key + mem-mcp
//! store key. It holds NO value (not funds, not the SBT): a per-device identity, so
//! `member address = comms key`. Loss → re-mint; deliberately no backup (don't spread a non-value
//! key). [`provision_comms_seed`] mints-or-loads it; [`CommsMemberManager::start`] writes it to a
//! 0600 file and passes the PATH (`CITRATE_MEMBER_SEED_FILE`), so the secret never crosses env/argv.
//! An unreachable keyring fails CLOSED (never a fake/absent identity — Rule 1).
//!
//! A wallet<->comms off-chain attestation (the "roster == wallet" property, via `wallet_link`) is a
//! deferred follow-on: nothing consumes it yet (the relay keys the roster on the comms address via
//! SIWE). On-chain anchoring is deferred further. The whole comms-identity path is reroll-insensitive
//! — SIWE / sign_binding sign over {domain, address, nonce, chain_id} and never touch chain state.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::supervisor::{
    BackoffPolicy, HealthCheck, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// Env override for the bundled `comms-member-daemon` binary path (dev/tests).
pub const COMMS_MEMBER_BIN_ENV: &str = "CITRATE_MEMBER_BIN";
/// The relay/SIWE domain the member authenticates under.
pub const COMMS_DOMAIN: &str = "relay.citrate.internal";

// The daemon's env knobs (comms-member-daemon/src/main.rs).
const ENV_SOCKET: &str = "CITRATE_MEMBER_SOCKET";
const ENV_BEARER_FILE: &str = "CITRATE_MEMBER_BEARER_FILE";
/// The seed crosses as a 0600 FILE PATH, never inline (env/argv leak to `ps`).
const ENV_SEED_FILE: &str = "CITRATE_MEMBER_SEED_FILE";
const ENV_DOMAIN: &str = "CITRATE_MEMBER_DOMAIN";

/// OS keyring account for the device-sealed comms member key (Option A). A distinct account under
/// the legacy custody service — the same namespacing the mem-mcp store key uses.
const KEYRING_COMMS_ACCOUNT: &str = "comms-member-key";
/// secp256k1 secret length.
const COMMS_SEED_LEN: usize = 32;

/// Supervision bearer length (256-bit).
const TOKEN_LEN: usize = 32;
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);
const COMMS_HEALTHY_AFTER: Duration = Duration::from_secs(30);
const COMMS_START_GRACE: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------
// Errors — coarse, secret-free (never carry the bearer or the seed).
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CommsError {
    /// The comms identity (seed) is not present — the manager was constructed with an empty seed,
    /// or the IPC was attempted before start. Provisioning itself surfaces as `Keyring`.
    NotConfigured,
    /// The bundled `comms-member-daemon` binary could not be located (packaging gap).
    BinaryNotFound(String),
    /// The supervisor refused to start the sidecar.
    Spawn(String),
    /// The sidecar is already running (idempotent-start guard).
    AlreadyRunning,
    /// A loopback IPC failure (socket, bearer, framing) — no secret carried.
    Ipc(String),
    /// The OS keyring is unreachable / a stored comms key is corrupt — fail closed (never mint or
    /// run under a random/absent identity). No key material carried.
    Keyring(String),
}

impl std::fmt::Display for CommsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommsError::NotConfigured => write!(f, "comms identity not available (daemon not started)"),
            CommsError::BinaryNotFound(m) => write!(f, "comms-member-daemon binary not bundled: {m}"),
            CommsError::Spawn(m) => write!(f, "comms-member-daemon spawn error: {m}"),
            CommsError::AlreadyRunning => write!(f, "comms-member-daemon already running"),
            CommsError::Ipc(m) => write!(f, "comms ipc error: {m}"),
            CommsError::Keyring(m) => write!(f, "comms keyring error: {m}"),
        }
    }
}
impl std::error::Error for CommsError {}

type Result<T> = std::result::Result<T, CommsError>;

/// The bridge status shape for the member-daemon sidecar (no secret). Manager API consumed by
/// tests today; a future `groups_status` command (S3.4 admin panel) will surface it to the UI.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommsStatus {
    pub state: String,
    pub socket_path: String,
    pub healthy: bool,
}

#[allow(dead_code)]
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
// The manager: supervise the comms-member-daemon binary; speak its UDS socket.
// ---------------------------------------------------------------------------

/// Supervises the `comms-member-daemon` sidecar (resolve binary → spawn ENV-configured → UDS-socket
/// liveness → bounded-backoff restart) and holds the session bearer for the IPC.
pub struct CommsMemberManager {
    bin: PathBuf,
    socket_path: PathBuf,
    bearer_path: PathBuf,
    data_dir: PathBuf,
    /// The member's signing seed (hex). Empty until provisioned (the identity decision). ENV, never argv.
    seed_hex: Zeroizing<String>,
    domain: String,
    crash_record_path: PathBuf,
    health_interval: Duration,
    #[cfg(test)]
    spawn_args_override: Option<Vec<String>>,
    token: Mutex<Option<Zeroizing<String>>>,
    sup: Mutex<Option<Supervisor>>,
}

impl CommsMemberManager {
    /// Build a manager over an explicit binary + socket/bearer/data paths + seed + domain.
    pub fn new(
        bin: PathBuf,
        socket_path: PathBuf,
        bearer_path: PathBuf,
        data_dir: PathBuf,
        seed_hex: impl Into<String>,
        domain: impl Into<String>,
        crash_record_path: PathBuf,
    ) -> Self {
        CommsMemberManager {
            bin,
            socket_path,
            bearer_path,
            data_dir,
            seed_hex: Zeroizing::new(seed_hex.into()),
            domain: domain.into(),
            crash_record_path,
            health_interval: HEALTH_INTERVAL,
            #[cfg(test)]
            spawn_args_override: None,
            token: Mutex::new(None),
            sup: Mutex::new(None),
        }
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

    /// Whether the daemon can start — the comms identity (seed) is provisioned.
    pub fn is_configured(&self) -> bool {
        !self.seed_hex.is_empty()
    }

    /// The 0600 file the sealed comms seed is written to at start (the daemon reads the PATH, so the
    /// secret never crosses env/argv). Derived from `data_dir` — the seed itself lives in the keyring.
    fn seed_path(&self) -> PathBuf {
        self.data_dir.join("member.seed")
    }

    #[cfg(test)]
    fn effective_spawn_args(&self) -> Vec<String> {
        self.spawn_args_override.clone().unwrap_or_default()
    }
    #[cfg(not(test))]
    fn effective_spawn_args(&self) -> Vec<String> {
        Vec::new()
    }

    /// Build the [`SidecarSpec`]: ENV carries the socket + bearer-file PATH + seed + domain (never a
    /// token/seed in argv). Liveness = the UDS socket accepts a connection (the daemon has no HTTP).
    fn build_spec(&self) -> SidecarSpec {
        let mut spec =
            SidecarSpec::new("comms-member-daemon", self.bin.clone(), self.effective_spawn_args());
        spec.env = vec![
            (ENV_SOCKET.to_string(), self.socket_path.to_string_lossy().to_string()),
            (ENV_BEARER_FILE.to_string(), self.bearer_path.to_string_lossy().to_string()),
            // The seed crosses as a 0600 FILE PATH (written in `start`), never inline.
            (ENV_SEED_FILE.to_string(), self.seed_path().to_string_lossy().to_string()),
            (ENV_DOMAIN.to_string(), self.domain.clone()),
        ];
        let sock = self.socket_path.clone();
        spec.health_check = Some(HealthCheck {
            interval: self.health_interval,
            grace: COMMS_START_GRACE,
            probe: std::sync::Arc::new(move || UnixStream::connect(&sock).is_ok()),
        });
        spec
    }

    #[cfg(test)]
    pub fn spec_env_for_test(&self) -> Vec<(String, String)> {
        self.build_spec().env
    }

    /// Start the sidecar IFF the identity is provisioned AND the binary exists. Mints a fresh session
    /// bearer, persists it 0600 (the daemon adopts it), spawns under the supervisor. Fails CLOSED.
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
        let token = mint_bearer();
        persist_secret_0600(&self.bearer_path, &token)?;
        // Write the sealed comms seed to its 0600 file — the daemon reads the PATH, so the secret
        // never crosses env/argv. Removed on stop (best-effort).
        persist_secret_0600(&self.seed_path(), &self.seed_hex)?;
        let spec = self.build_spec();
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = COMMS_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| CommsError::Spawn(e.to_string()))?;
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
        // Best-effort: don't leave the seed/bearer secrets on disk after teardown.
        let _ = std::fs::remove_file(self.seed_path());
        let _ = std::fs::remove_file(&self.bearer_path);
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    #[allow(dead_code)] // consumed by tests + future groups_status (S3.4)
    pub fn status(&self) -> CommsStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        CommsStatus {
            state: state.to_string(),
            socket_path: self.socket_path.to_string_lossy().to_string(),
            healthy: matches!(sup_state, Some(SupervisorState::Running)),
        }
    }

    pub fn is_running(&self) -> bool {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        matches!(
            guard.as_ref().map(|s| s.status().state),
            Some(SupervisorState::Running)
        )
    }

    /// The session bearer (for the IPC). None until started.
    fn bearer(&self) -> Option<Zeroizing<String>> {
        self.token.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Send one request to the daemon over its UDS socket and return the response.
    fn ipc(&self, req: &Request) -> Result<Response> {
        let bearer = self.bearer().ok_or(CommsError::NotConfigured)?;
        member_ipc(&self.socket_path, &bearer, req)
    }
}

// ---- bearer token (mirrors agent.rs / hermes.rs) ----

fn mint_bearer() -> Zeroizing<String> {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = Zeroizing::new([0u8; TOKEN_LEN]);
    OsRng.fill_bytes(bytes.as_mut());
    Zeroizing::new(hex::encode(bytes.as_ref()))
}

/// Write a secret (bearer token or seed hex) to a 0600 file under a 0700 dir. Fail closed.
fn persist_secret_0600(path: &Path, secret: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CommsError::Ipc(e.kind().to_string()))?;
        harden_dir_perms(parent).map_err(|e| CommsError::Ipc(e.kind().to_string()))?;
    }
    std::fs::write(path, secret.as_bytes()).map_err(|e| CommsError::Ipc(e.kind().to_string()))?;
    harden_perms(path).map_err(|e| CommsError::Ipc(e.kind().to_string()))?;
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

/// Resolve the bundled `comms-member-daemon` binary (env override → resource dir).
pub fn resolve_comms_member_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var(COMMS_MEMBER_BIN_ENV) {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("{COMMS_MEMBER_BIN_ENV} set but not found: {}", path.display()));
    }
    // Bundled externalBin: installed NEXT TO THE MAIN EXECUTABLE (Contents/MacOS/<name>), NOT the
    // resource dir. Using resource_dir() here made a packaged app report "binary not bundled" even
    // though the daemon shipped — mirror node.rs/ipfs.rs/mem-mcp via the shared resolver.
    crate::supervisor::resolve_external_bin(app, "comms-member-daemon")
}

// ---------------------------------------------------------------------------
// The loopback UDS JSON IPC client (mirrors comms-member-daemon/src/server.rs framing).
// ---------------------------------------------------------------------------

/// A request to the daemon. `op`-tagged; mirrors comms-member-daemon::ipc::Request.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Request {
    CreateGroup { name: String },
    ListGroups,
    JoinGroup { group: String },
    /// Owner-invite: add a member who has published a key package to the shared relay. The daemon
    /// produces + publishes the MLS welcome; the invitee then joins. (Post-S0 amendment, CX-S3.5.)
    AddMember { group: String, member: String },
    Roster { group: String },
    AssignRole { group: String, member: String, role: String },
    Offboard { group: String, member: String },
    Send { group: String, text: String },
    Poll { group: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupView {
    id: String,
    name: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MsgView {
    sender: String,
    body: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RosterEntry {
    address: String,
    role: String,
}

/// A response from the daemon. `type`-tagged; mirrors comms-member-daemon::ipc::Response.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Response {
    Ok,
    GroupCreated { id: String },
    Groups { groups: Vec<GroupView> },
    /// The welcome material the daemon produced for the invitee (published to the relay too); the
    /// owner-client only needs to know the add succeeded, so the fields are informational.
    Added {
        #[allow(dead_code)]
        member: String,
    },
    Messages { messages: Vec<MsgView> },
    Roster { members: Vec<RosterEntry> },
    Error { message: String },
}

/// Connect to the member daemon's UDS, retrying briefly. The daemon spawns and binds `member.sock`
/// a beat after the app launches, so a `Connection refused` on the first request is a startup RACE,
/// not a fault — surfacing it makes a healthy first-open look broken. Retry with backoff for a short
/// window before giving up (any real, persistent failure still surfaces honestly, Rule 1).
fn connect_with_retry(socket_path: &Path) -> Result<UnixStream> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut delay = Duration::from_millis(40);
    loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                if Instant::now() >= deadline {
                    return Err(CommsError::Ipc(format!("connect {}: {e}", socket_path.display())));
                }
                std::thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_millis(400));
            }
        }
    }
}

/// Connect to the daemon's UDS, authenticate with the bearer, send one request, read one response.
fn member_ipc(socket_path: &Path, bearer: &str, req: &Request) -> Result<Response> {
    let stream = connect_with_retry(socket_path)?;
    let mut writer = stream
        .try_clone()
        .map_err(|e| CommsError::Ipc(e.to_string()))?;
    let mut reader = BufReader::new(stream);

    // bearer handshake
    writeln!(writer, "{{\"type\":\"auth\",\"token\":\"{bearer}\"}}")
        .map_err(|e| CommsError::Ipc(e.to_string()))?;
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| CommsError::Ipc(e.to_string()))?;
    if !line.contains("ready") {
        return Err(CommsError::Ipc(format!("handshake rejected: {}", line.trim())));
    }

    // request → response
    let body = serde_json::to_string(req).map_err(|e| CommsError::Ipc(e.to_string()))?;
    writeln!(writer, "{body}").map_err(|e| CommsError::Ipc(e.to_string()))?;
    line.clear();
    reader
        .read_line(&mut line)
        .map_err(|e| CommsError::Ipc(e.to_string()))?;
    serde_json::from_str::<Response>(line.trim())
        .map_err(|e| CommsError::Ipc(format!("bad response: {e}: {}", line.trim())))
}

// ---------------------------------------------------------------------------
// The lazy-start singleton + the comms-seed provisioning (GATED on the DGX decision).
// ---------------------------------------------------------------------------

static MANAGER: OnceLock<CommsMemberManager> = OnceLock::new();

/// Load the device-sealed comms key from the OS keyring, minting a fresh secp256k1 key on first use
/// (Option A). Returned as hex [`Zeroizing`] ready to write to the daemon's 0600 seed file. The key
/// is a scoped, non-value per-device identity (NOT the custody wallet); it leaves this process only
/// into that 0600 file. Stable across restarts (persistent member identity). An unreachable keyring,
/// or a stored key that is the wrong length or not a valid secp256k1 scalar, is a HARD FAULT — we
/// never run the daemon under a random/absent identity (fail closed).
fn load_or_mint_comms_seed(keyring: &dyn crate::custody::Keyring) -> Result<Zeroizing<String>> {
    match keyring
        .get(KEYRING_COMMS_ACCOUNT)
        .map_err(|e| CommsError::Keyring(e.to_string()))?
    {
        Some(bytes) => {
            if bytes.len() != COMMS_SEED_LEN {
                return Err(CommsError::Keyring(format!(
                    "stored comms key has wrong length {}",
                    bytes.len()
                )));
            }
            // Validate it is a real secp256k1 scalar before we hand it to the daemon (the daemon's
            // EthWallet::from_secret_key would reject 0 / >= n).
            k256::ecdsa::SigningKey::from_slice(&bytes).map_err(|_| {
                CommsError::Keyring("stored comms key is not a valid secp256k1 scalar".into())
            })?;
            let hex = Zeroizing::new(hex::encode(&bytes));
            let mut bytes = bytes;
            use zeroize::Zeroize;
            bytes.zeroize();
            Ok(hex)
        }
        None => {
            // Mint a fresh valid secp256k1 secret via rejection sampling (OsRng bytes → validate),
            // so we depend only on rand's OsRng + k256 validation, not their rng-trait versions.
            use rand::rngs::OsRng;
            use rand::RngCore;
            let mut raw = Zeroizing::new([0u8; COMMS_SEED_LEN]);
            loop {
                OsRng.fill_bytes(raw.as_mut());
                if k256::ecdsa::SigningKey::from_slice(raw.as_ref()).is_ok() {
                    break;
                }
            }
            keyring
                .set(KEYRING_COMMS_ACCOUNT, raw.as_ref())
                .map_err(|e| CommsError::Keyring(e.to_string()))?;
            Ok(Zeroizing::new(hex::encode(raw.as_ref())))
        }
    }
}

/// Provision the member's signing seed (Option A — device-sealed comms key, confirmed 2026-08-27).
/// Mints-or-loads a fresh scoped secp256k1 key sealed in the OS keyring (never exported), the same
/// custody pattern as the node/mem storage keys. Reroll-insensitive. Fails CLOSED on an unreachable
/// keyring — never a fake identity (Rule 1).
fn provision_comms_seed<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<Zeroizing<String>> {
    let keyring = crate::custody::OsKeyring::legacy();
    load_or_mint_comms_seed(&keyring)
}

/// Derive the EVM address (lowercase hex, **no** `0x`) of a secp256k1 secret hex — the member's
/// device identity. This is the standard derivation `keccak256(uncompressed_pubkey[1..])[12..]`, the
/// same one the comms member-daemon and the cluster libp2p transport use (`derive_address_from_secp256k1`
/// / `address_from_public_key`), so the address this returns matches the one that appears in the group
/// roster. Kept here as the ONE source of truth for the device identity (Rule 9).
pub(crate) fn address_from_secret_hex(seed_hex: &str) -> std::result::Result<String, String> {
    use sha3::{Digest, Keccak256};
    let bytes = hex::decode(seed_hex.trim()).map_err(|_| "comms seed is not hex".to_string())?;
    let sk = k256::ecdsa::SigningKey::from_slice(&bytes)
        .map_err(|_| "comms seed is not a valid secp256k1 scalar".to_string())?;
    let point = sk.verifying_key().to_encoded_point(false); // uncompressed: 0x04 || X(32) || Y(32)
    let pub_bytes = &point.as_bytes()[1..]; // drop the 0x04 tag → 64 bytes
    let digest = Keccak256::digest(pub_bytes);
    Ok(hex::encode(&digest[12..]))
}

/// The device-sealed comms identity: the seed (ready to write to a daemon's 0600 seed file) **and**
/// its derived address. This is the identity a member is known by in the group roster, so both the
/// comms member-daemon and the cluster-daemon must use THIS key — the cluster's Noise/peer id and the
/// address it announces on the mesh have to match the roster entries (which key on the comms address).
/// One source of truth (Rule 9) — the cluster module consumes this rather than re-reading the keyring.
pub(crate) struct DeviceIdentity {
    pub seed_hex: Zeroizing<String>,
    pub address: String,
}

/// Provision the comms device identity (seed + address). Fails CLOSED on an unreachable keyring.
pub(crate) fn device_identity<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<DeviceIdentity, String> {
    let seed_hex = provision_comms_seed(app).map_err(|e| e.to_string())?;
    let address = address_from_secret_hex(&seed_hex)?;
    Ok(DeviceIdentity { seed_hex, address })
}

/// Ensure the daemon is built + started; returns the process-wide manager. Lazy singleton, so no
/// managed state in the (s0-owned) lib.rs.
fn ensure_started<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<&'static CommsMemberManager, String> {
    use tauri::Manager;
    if let Some(m) = MANAGER.get() {
        if !m.is_running() {
            m.start().map_err(|e| e.to_string())?;
        }
        return Ok(m);
    }
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("comms");
    let seed = provision_comms_seed(app).map_err(|e| e.to_string())?; // GATED — errors until the decision
    let bin = resolve_comms_member_bin(app)?;
    let mgr = CommsMemberManager::new(
        bin,
        data_root.join("member.sock"),
        data_root.join("member.bearer"),
        data_root.clone(),
        seed.to_string(),
        COMMS_DOMAIN,
        data_root.join("member-crash.jsonl"),
    );
    mgr.start().map_err(|e| e.to_string())?;
    Ok(MANAGER.get_or_init(|| mgr))
}

/// Route one request to the running daemon.
fn route<R: tauri::Runtime>(app: &tauri::AppHandle<R>, req: Request) -> std::result::Result<Response, String> {
    ensure_started(app)?.ipc(&req).map_err(|e| e.to_string())
}

fn parse_ok(r: Response) -> std::result::Result<(), String> {
    match r {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Tauri command surface — groups: create/list/join/roster/RBAC/messages over the daemon socket.
// ---------------------------------------------------------------------------

/// **groups_create** — create a Group; returns its id.
#[tauri::command]
pub fn groups_create(app: tauri::AppHandle, name: String) -> std::result::Result<String, String> {
    match route(&app, Request::CreateGroup { name })? {
        Response::GroupCreated { id } => Ok(id),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **groups_list** — the member's groups.
#[tauri::command]
pub fn groups_list(app: tauri::AppHandle) -> std::result::Result<Vec<(String, String)>, String> {
    match route(&app, Request::ListGroups)? {
        Response::Groups { groups } => Ok(groups.into_iter().map(|g| (g.id, g.name)).collect()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **groups_join** — join a group this member was added to on a shared relay.
#[tauri::command]
pub fn groups_join(app: tauri::AppHandle, group: String) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::JoinGroup { group })?)
}

/// **groups_add_member** — owner-invite a member (who has published a key package to the relay).
/// The daemon produces + publishes the MLS welcome; the invitee then joins. Post-S0 amendment.
#[tauri::command]
pub fn groups_add_member(
    app: tauri::AppHandle,
    group: String,
    member: String,
) -> std::result::Result<(), String> {
    match route(&app, Request::AddMember { group, member })? {
        Response::Added { .. } => Ok(()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **groups_roster** — the (address, role) roster.
#[tauri::command]
pub fn groups_roster(
    app: tauri::AppHandle,
    group: String,
) -> std::result::Result<Vec<(String, String)>, String> {
    match route(&app, Request::Roster { group })? {
        Response::Roster { members } => Ok(members.into_iter().map(|m| (m.address, m.role)).collect()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **groups_assign_role** — owner-signed role grant.
#[tauri::command]
pub fn groups_assign_role(
    app: tauri::AppHandle,
    group: String,
    member: String,
    role: String,
) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::AssignRole { group, member, role })?)
}

/// **groups_offboard** — atomic offboard (MLS remove + relay roster/tree drop).
#[tauri::command]
pub fn groups_offboard(
    app: tauri::AppHandle,
    group: String,
    member: String,
) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::Offboard { group, member })?)
}

/// **groups_send** — post an encrypted message to a group.
#[tauri::command]
pub fn groups_send(
    app: tauri::AppHandle,
    group: String,
    text: String,
) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::Send { group, text })?)
}

/// **groups_messages** — drain + decrypt the member's mailbox for a group.
#[tauri::command]
pub fn groups_messages(
    app: tauri::AppHandle,
    group: String,
) -> std::result::Result<Vec<(String, String)>, String> {
    match route(&app, Request::Poll { group })? {
        Response::Messages { messages } => {
            Ok(messages.into_iter().map(|m| (m.sender, m.body)).collect())
        }
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    include!("comms_tests.rs");
}
