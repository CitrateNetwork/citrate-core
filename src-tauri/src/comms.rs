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
//! `CITRATE_MEMBER_SEED` (the member's signing identity), `CITRATE_MEMBER_DOMAIN`.
//!
//! ## Identity (GATED — pending the DGX keys/devops decision)
//! The daemon needs a signing seed but Rule 3 forbids exporting the main custody wallet. The chosen
//! identity model (device-sealed comms key vs. ceremony-signed wallet identity vs. derived sub-key)
//! is being confirmed on the DGX. Until then [`provision_comms_seed`] returns an honest
//! `NotConfigured` (Rule 1 — never a fake identity), so `groups_*` surface "comms identity not
//! provisioned" rather than run under a wrong/insecure key. EVERYTHING ELSE — spawn/supervise, the
//! UDS client, the `groups_*` routing — is identity-agnostic and lives here, wired and tested.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

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
const ENV_SEED: &str = "CITRATE_MEMBER_SEED";
const ENV_DOMAIN: &str = "CITRATE_MEMBER_DOMAIN";

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
    /// The comms identity (seed) is not provisioned — GATED on the DGX identity decision.
    NotConfigured,
    /// The bundled `comms-member-daemon` binary could not be located (packaging gap).
    BinaryNotFound(String),
    /// The supervisor refused to start the sidecar.
    Spawn(String),
    /// The sidecar is already running (idempotent-start guard).
    AlreadyRunning,
    /// A loopback IPC failure (socket, bearer, framing) — no secret carried.
    Ipc(String),
}

impl std::fmt::Display for CommsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommsError::NotConfigured => write!(
                f,
                "comms identity not provisioned yet (pending the keys/devops identity decision)"
            ),
            CommsError::BinaryNotFound(m) => write!(f, "comms-member-daemon binary not bundled: {m}"),
            CommsError::Spawn(m) => write!(f, "comms-member-daemon spawn error: {m}"),
            CommsError::AlreadyRunning => write!(f, "comms-member-daemon already running"),
            CommsError::Ipc(m) => write!(f, "comms ipc error: {m}"),
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
            (ENV_SEED.to_string(), self.seed_hex.to_string()),
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
        persist_bearer(&self.bearer_path, &token)?;
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

fn persist_bearer(path: &Path, token: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CommsError::Ipc(e.kind().to_string()))?;
        harden_dir_perms(parent).map_err(|e| CommsError::Ipc(e.kind().to_string()))?;
    }
    std::fs::write(path, token.as_bytes()).map_err(|e| CommsError::Ipc(e.kind().to_string()))?;
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
    use tauri::Manager;
    if let Ok(p) = std::env::var(COMMS_MEMBER_BIN_ENV) {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("{COMMS_MEMBER_BIN_ENV} set but not found: {}", path.display()));
    }
    let resource = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("comms-member-daemon");
    Ok(resource)
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
    // NOTE: the daemon exposes AddMember (owner invites a member → welcome material), but the
    // frozen GroupsDomain has no `addMember` command, so the client cannot issue it yet. Exposing
    // the invite flow needs an S0 domain amendment (a `groups_add_member` command + DTO); tracked
    // as a Lane-C follow-on. Until then the client speaks only the frozen surface.
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
    Messages { messages: Vec<MsgView> },
    Roster { members: Vec<RosterEntry> },
    Error { message: String },
}

/// Connect to the daemon's UDS, authenticate with the bearer, send one request, read one response.
fn member_ipc(socket_path: &Path, bearer: &str, req: &Request) -> Result<Response> {
    let stream = UnixStream::connect(socket_path)
        .map_err(|e| CommsError::Ipc(format!("connect {}: {e}", socket_path.display())))?;
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

/// Provision the member's signing seed. GATED (Rule 1): the identity model (device-sealed comms key
/// vs. ceremony-signed wallet vs. derived sub-key) is being confirmed on the DGX keys/devops side.
/// Until then this returns `NotConfigured` — the app never runs the daemon under a wrong/insecure
/// key. When the decision lands, this becomes: generate/load the sealed comms key from the OS keyring
/// (Option A) or drive the ceremony (Option B) / derive it (Option C).
fn provision_comms_seed<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<Zeroizing<String>> {
    Err(CommsError::NotConfigured)
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
