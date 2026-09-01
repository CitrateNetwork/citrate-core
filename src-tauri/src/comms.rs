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
//! ## Identity — CONNECT-S5: WALLET-DERIVED comms key (supersedes "Option A", 2026-08-31)
//! The daemon needs a signing seed but Rule 3 forbids exporting the main custody wallet. Original
//! decision (Option A, 2026-08-27) was a FRESH RANDOM per-device key — but that made identity
//! non-portable: reinstalling on another Mac minted a different comms address, so the member fell out
//! of every group they were in (rosters key on the comms address). CONNECT-S5 fixes that: the comms
//! key is now DERIVED DETERMINISTICALLY from the custody wallet via `wallet::derive_scoped_secret`
//! (HKDF over the sealed BIP39 entropy, domain `COMMS_IDENTITY_INFO`). The SAME wallet yields the
//! SAME comms address on every device, so membership follows the human across installs — "sign in on
//! any Mac and you're in your groups." The derivation is NOT a signature (one-way KDF), so it needs
//! no interactive ceremony and can run on the lazy daemon start; the wallet key never leaves the
//! vault — only this scoped, one-way-derived key does, into a 0600 file (`CITRATE_MEMBER_SEED_FILE`).
//! It still holds NO value (not funds, not the SBT). [`provision_comms_seed`] loads-or-derives it,
//! cached in the OS keyring under `comms-member-key-v2` (the legacy random `comms-member-key` is no
//! longer read — a versioned, observable migration; existing installs get a new address once and
//! need a single re-invite). Fails CLOSED: unreachable keyring → hard fault; locked/absent wallet →
//! `WalletNotReady` (never a random throwaway identity — Rule 1).
//!
//! A separate wallet<->comms on-chain/attestation anchor (via `wallet_link`) remains a deferred
//! follow-on. The comms-identity path is reroll-insensitive — SIWE / sign_binding sign over
//! {domain, address, nonce, chain_id} and never touch chain state.

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
/// The in-process relay/SIWE domain (local, single-machine — no networked relay).
pub const COMMS_DOMAIN: &str = "relay.citrate.internal";

/// GROW-S2 — the shared server-blind rendezvous relay (deployed on DO, Caddy TLS). Setting
/// `CITRATE_MEMBER_RELAY_URL` to this lets two machines form a cluster over the relay's claims-inbox +
/// KeyPackage directory + MLS delivery, with no p2p (the libp2p mesh stays soak/@rule8-gated).
/// Clustering is OPT-IN: unset ⇒ the local in-process relay. The relay VERIFIES the SIWE `domain`, so
/// [`CLUSTER_RELAY_DOMAIN`] equals the relay's own `CITRATE_COMMS_DOMAIN` (and its host).
///
/// The canonical value the app's connect/cluster flow sets as `CITRATE_MEMBER_RELAY_URL` (and the
/// tests pin). Not consumed by the lib itself — the transport is env-selected — so it reads as
/// "unused" in a non-test build until that flow lands.
#[allow(dead_code)]
pub const CLUSTER_RELAY_URL: &str = "wss://comms.citrate.ai";
pub const CLUSTER_RELAY_DOMAIN: &str = "comms.citrate.ai";

// The daemon's env knobs (comms-member-daemon/src/main.rs).
const ENV_SOCKET: &str = "CITRATE_MEMBER_SOCKET";
const ENV_BEARER_FILE: &str = "CITRATE_MEMBER_BEARER_FILE";
/// The seed crosses as a 0600 FILE PATH, never inline (env/argv leak to `ps`).
const ENV_SEED_FILE: &str = "CITRATE_MEMBER_SEED_FILE";
const ENV_DOMAIN: &str = "CITRATE_MEMBER_DOMAIN";
/// Set → the daemon uses a networked `WsRelay` at this `wss://`|`ws://` URL; empty/unset → in-process.
const ENV_RELAY_URL: &str = "CITRATE_MEMBER_RELAY_URL";

/// OS keyring account for the LEGACY random device-sealed comms key (Option A, pre CONNECT-S5). No
/// longer read — kept only as a name so the migration is observable/reversible. See `_V2` below.
#[allow(dead_code)]
const KEYRING_COMMS_ACCOUNT: &str = "comms-member-key";
/// OS keyring account for the CONNECT-S5 wallet-derived comms key. A distinct account so the
/// migration from the old random key is observable and reversible (we never overwrite the old slot
/// in place). First start under v2 derives-and-caches; every later start is a cheap keyring read.
const KEYRING_COMMS_ACCOUNT_V2: &str = "comms-member-key-v2";
/// HKDF `info` (domain) that scopes the comms identity out of the wallet entropy. Fixed forever —
/// changing it changes every member's comms address. Paired with `wallet::SCOPED_SECRET_SALT`.
const COMMS_IDENTITY_INFO: &[u8] = b"citrate-comms-member-identity-v1";
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
    /// CONNECT-S5: the comms identity is derived from the wallet, but the wallet isn't ready yet
    /// (custody vault locked, or no wallet created) — fail closed with an honest, actionable signal
    /// (the caller/UI says "finish sign-in first") rather than minting a throwaway identity. No key
    /// material carried.
    WalletNotReady(String),
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
            CommsError::WalletNotReady(m) => write!(f, "comms identity needs your wallet: {m}"),
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
    /// Flag-A — the networked relay-link health, distinct from process liveness (`healthy`). One of
    /// `"n/a"` (in-process relay), `"connected"`, `"degraded"` (relay down — ops failing), `"unknown"`
    /// (not running / daemon didn't answer). Reported for honesty; NOT a restart trigger.
    pub relay: String,
}

/// Flag-A — the relay-link health of the member daemon, distinct from PROCESS liveness (which the
/// supervisor's socket probe already covers AND recovers via restart). This is REPORTED, never acted
/// on: a relay drop must not restart the daemon — a flapping relay would restart-loop it to terminal
/// `Failed` and fight the daemon's own WsRelay reconnect (citrate-comms) — so the app surfaces
/// "degraded" while the daemon reconnects, and the socket probe keeps recovering a dead PROCESS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayHealth {
    /// No networked relay configured — the in-process relay is always local-fine.
    NotApplicable,
    /// The daemon reports its relay link up.
    Connected,
    /// Relay configured but the daemon reports the link DOWN — relayed ops will fail until it
    /// reconnects. This is exactly the state the old socket-only probe mislabelled "healthy".
    Degraded,
    /// Can't tell right now: daemon not running, didn't answer within the probe timeout, or is an
    /// older build with no `relayStatus` op. Never reported as a hard failure (fail-open on ambiguity).
    Unknown,
}

/// Pure mapping from a `relayStatus` IPC result to [`RelayHealth`]. Split out so the classification is
/// unit-testable without a live supervisor/daemon. Fails OPEN to `Unknown` on an `Error` (an older
/// daemon that doesn't know the op), any unexpected shape, or an IPC error — never a false "degraded".
fn classify_relay(resp: Result<Response>) -> RelayHealth {
    match resp {
        Ok(Response::RelayStatus { connected: true }) => RelayHealth::Connected,
        Ok(Response::RelayStatus { connected: false }) => RelayHealth::Degraded,
        _ => RelayHealth::Unknown,
    }
}

impl RelayHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            RelayHealth::NotApplicable => "n/a",
            RelayHealth::Connected => "connected",
            RelayHealth::Degraded => "degraded",
            RelayHealth::Unknown => "unknown",
        }
    }
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
    /// If set, the daemon connects a networked `WsRelay` at this URL instead of the in-process relay
    /// (GROW-S2 cluster rendezvous). Its host's SIWE domain MUST equal `domain`.
    relay_url: Option<String>,
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
            relay_url: None,
            crash_record_path,
            health_interval: HEALTH_INTERVAL,
            #[cfg(test)]
            spawn_args_override: None,
            token: Mutex::new(None),
            sup: Mutex::new(None),
        }
    }

    /// Point the daemon at a networked relay (`wss://…`, or `ws://` loopback for dev) instead of the
    /// in-process one — the GROW-S2 cluster rendezvous. An empty URL is ignored (stays in-process).
    /// The `domain` passed to [`Self::new`] MUST equal that relay's SIWE domain or the login is
    /// rejected (`DomainMismatch`).
    pub fn with_relay_url(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        self.relay_url = if url.trim().is_empty() { None } else { Some(url) };
        self
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
        // GROW-S2: when a networked relay is configured, the daemon selects the WsRelay transport.
        // Public data only (a URL) — no secret. Unset ⇒ the daemon runs its in-process relay.
        if let Some(url) = self.relay_url.as_deref().filter(|u| !u.is_empty()) {
            spec.env.push((ENV_RELAY_URL.to_string(), url.to_string()));
        }
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
            // Flag-A: also report the RELAY link, not just the process. `relay_status` is bounded +
            // non-retrying (and a no-op fast path unless a networked relay is configured AND running),
            // so it never reintroduces the unbounded-IPC UI stall.
            relay: self.relay_status().as_str().to_string(),
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

    /// True when a NETWORKED relay transport is configured (vs the local in-process relay). Only then
    /// is relay-link health a meaningful question.
    fn relay_configured(&self) -> bool {
        self.relay_url
            .as_deref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false)
    }

    /// Flag-A — the daemon's networked relay-link health (see [`RelayHealth`]). Bounded + non-retrying
    /// (sub-second worst case), so it is safe to call alongside [`Self::status`] without risking the
    /// UI-thread stall that unbounded IPC caused before. Reported for honesty; NEVER a restart trigger
    /// (the supervisor's socket probe recovers a dead PROCESS; a relay drop is left to the daemon's own
    /// WsRelay reconnect). Fails OPEN to `Unknown` on any ambiguity so it can't false-alarm.
    pub fn relay_status(&self) -> RelayHealth {
        if !self.relay_configured() {
            return RelayHealth::NotApplicable;
        }
        if !self.is_running() {
            return RelayHealth::Unknown;
        }
        let Some(bearer) = self.bearer() else {
            return RelayHealth::Unknown;
        };
        classify_relay(member_ipc_quick(&self.socket_path, &bearer, &Request::RelayStatus))
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
    /// CONNECT-S1 — submit a sealed claim to the relay's server-blind claims-inbox (invitee side).
    SubmitClaim { token_hash: String, ciphertext: String },
    /// CONNECT-S1 — poll the claims-inbox by invite token hash (owner side).
    PollClaims { token_hash: String },
    /// Flag-A — ask the daemon whether its networked relay link is currently up. Cheap in-memory
    /// read on the daemon side; used by [`CommsMemberManager::relay_status`] so the app can report a
    /// relay DROP instead of showing "healthy" (the UDS socket stays up while every relayed op fails).
    RelayStatus,
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
    /// CONNECT-S1 — polled claim ciphertexts (hex; opaque). The owner opens them with the invite key.
    Claims { ciphertexts: Vec<String> },
    /// Flag-A — the daemon's networked relay-link state (answer to [`Request::RelayStatus`]).
    /// `connected: false` = configured but the link is down (ops will fail until it reconnects).
    RelayStatus { connected: bool },
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

/// Read/write deadline on the member-daemon socket. BOUNDS every IPC so a wedged daemon (accepts the
/// connection but never replies — e.g. a stalled MLS store) can NEVER block the caller forever. The
/// UI-thread pinwheel came from an unbounded `read_line` here; on timeout the call returns an honest
/// `Ipc` error and the surface falls to its empty/error state instead of beachballing.
const COMMS_IPC_TIMEOUT: Duration = Duration::from_secs(5);

/// Flag-A — short, single-attempt timeout for the relay-health probe. It must never add UI latency,
/// so it does NOT use `connect_with_retry` (that window is for first-open races) and caps the whole
/// round-trip well under a second. A wedged daemon is already the supervisor's job; this probe only
/// asks "is the relay link up right now?" and fails-open (→ `Unknown`) on any ambiguity.
const RELAY_PROBE_TIMEOUT: Duration = Duration::from_millis(750);

/// Connect to the daemon's UDS (retrying briefly through the first-open race), authenticate, send one
/// request, read one response — the normal command path.
fn member_ipc(socket_path: &Path, bearer: &str, req: &Request) -> Result<Response> {
    let stream = connect_with_retry(socket_path)?;
    ipc_round_trip(stream, COMMS_IPC_TIMEOUT, bearer, req)
}

/// Flag-A — a bounded, NON-retrying round-trip for the relay-health probe: one connect attempt and a
/// sub-second timeout so a health check can never stall a caller. Used only by `relay_status`.
fn member_ipc_quick(socket_path: &Path, bearer: &str, req: &Request) -> Result<Response> {
    let stream = UnixStream::connect(socket_path).map_err(|e| CommsError::Ipc(e.to_string()))?;
    ipc_round_trip(stream, RELAY_PROBE_TIMEOUT, bearer, req)
}

/// The shared post-connect half of the IPC: bound both directions by `timeout`, do the bearer
/// handshake, then one request → one response. Factored out so the normal (retry-connect) path and
/// the relay-health probe (single-connect, short timeout) share identical framing.
fn ipc_round_trip(
    stream: UnixStream,
    timeout: Duration,
    bearer: &str,
    req: &Request,
) -> Result<Response> {
    // Bound both directions before any read/write (SO_RCVTIMEO/SO_SNDTIMEO). Applied to the clone too
    // so neither the read nor the write side can hang indefinitely.
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| CommsError::Ipc(e.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| CommsError::Ipc(e.to_string()))?;
    let mut writer = stream
        .try_clone()
        .map_err(|e| CommsError::Ipc(e.to_string()))?;
    let _ = writer.set_write_timeout(Some(timeout));
    let _ = writer.set_read_timeout(Some(timeout));
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

/// Stop the comms member-daemon if this session started it (called on graceful app teardown). Releases
/// the MLS store LOCK so the next launch reopens cleanly instead of racing an orphan. Idempotent and a
/// no-op if the daemon was never started.
pub fn shutdown() {
    if let Some(m) = MANAGER.get() {
        m.stop();
    }
}

/// Load the wallet-derived comms key from the OS keyring (account `_V2`), deriving it on first use
/// via `derive` (CONNECT-S5). Returned as hex [`Zeroizing`] ready to write to the daemon's 0600 seed
/// file. The key is a scoped identity (NOT the custody wallet, and never carries value); it leaves
/// this process only into that 0600 file. It is DETERMINISTIC in the wallet, so the same wallet
/// produces the same comms address on every device — membership follows the human across installs.
///
/// Fail-closed contract (Rule 1) — we NEVER run the daemon under a random/absent identity:
///   - an unreachable keyring, or a cached key of the wrong length / not a valid secp256k1 scalar,
///     is a `Keyring` hard fault;
///   - if the key isn't cached and `derive` can't produce it (wallet locked / not created), that is
///     a `WalletNotReady` fault — no throwaway identity is minted.
///
/// `derive` is injected (not called inline) so this stays a pure, keyring-only unit under test.
fn load_or_derive_comms_seed(
    keyring: &dyn crate::custody::Keyring,
    derive: impl FnOnce() -> std::result::Result<Zeroizing<[u8; COMMS_SEED_LEN]>, String>,
) -> Result<Zeroizing<String>> {
    match keyring
        .get(KEYRING_COMMS_ACCOUNT_V2)
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
            // First start under v2: derive from the wallet (deterministic, portable) and cache it.
            let raw = derive().map_err(CommsError::WalletNotReady)?;
            // `derive_scoped_secret` already guarantees a valid scalar; re-validate as defense in
            // depth before it ever reaches the daemon.
            k256::ecdsa::SigningKey::from_slice(raw.as_ref()).map_err(|_| {
                CommsError::Keyring("derived comms key is not a valid secp256k1 scalar".into())
            })?;
            keyring
                .set(KEYRING_COMMS_ACCOUNT_V2, raw.as_ref())
                .map_err(|e| CommsError::Keyring(e.to_string()))?;
            Ok(Zeroizing::new(hex::encode(raw.as_ref())))
        }
    }
}

/// Provision the member's signing seed (CONNECT-S5 — wallet-derived comms identity). Loads the
/// cached key, or on first use derives it deterministically from the custody wallet via
/// `wallet::derive_scoped_secret` so the SAME wallet yields the SAME comms address on every device
/// (membership is portable across reinstalls). The wallet key itself never leaves the vault — only
/// this one-way-derived scoped key does, into the daemon's 0600 seed file. Fails CLOSED: an
/// unreachable keyring is a hard fault, and a locked/absent wallet surfaces `WalletNotReady` rather
/// than a throwaway identity (Rule 1).
fn provision_comms_seed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Zeroizing<String>> {
    use tauri::Manager;
    let keyring = crate::custody::OsKeyring::legacy();
    let custody = app.state::<crate::custody::CustodyState>();
    load_or_derive_comms_seed(&keyring, || {
        // Device-bound auto-unlock (no user passphrase in the beta model), then derive. A locked or
        // absent wallet surfaces as an honest error string → WalletNotReady.
        custody
            .0
            .ensure_auto_unlocked()
            .map_err(|e| format!("custody vault unavailable: {e}"))?;
        crate::wallet::derive_scoped_secret(&custody.0, COMMS_IDENTITY_INFO)
            .map_err(|e| e.to_string())
    })
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

/// Decide the daemon's relay transport + SIWE domain from env (pure — unit-tested). GROW-S2 is
/// OPT-IN: `None` relay ⇒ the local in-process relay under [`COMMS_DOMAIN`]. When a networked relay is
/// set, the relay verifies the SIWE `domain`, so the domain must equal THAT relay's — taken from
/// `CITRATE_MEMBER_DOMAIN` if given, else derived from the relay URL's host (correct whenever
/// host == the relay's domain, which the cluster relay satisfies), else [`CLUSTER_RELAY_DOMAIN`] as a
/// last resort. Downgrade to plaintext is refused later by `WsRelay::connect` (non-loopback `ws://`
/// is rejected).
/// Resolve the comms transport. **DEFAULT-ON for the alpha (owner 2026-09-01):** with no override the
/// daemon uses the shared rendezvous relay ([`CLUSTER_RELAY_URL`]) so two machines connect out of the
/// box — a partner who just opens the DMG is on the network, no env/config. `CITRATE_MEMBER_RELAY_URL`
/// overrides: an explicit `wss://`/`ws://` URL points elsewhere; an explicit OFF-switch
/// (`off`/`disabled`/`none`/`local`/`in-process`/`0`/`false`, case-insensitive) drops back to the
/// in-process relay (single node, no remote dependency — the escape hatch). Trade-off acknowledged:
/// default-on makes the relay a dependency for cluster comms (red-team F5), accepted for the soak-test
/// alpha with sharding/DDoS on the roadmap.
fn resolve_relay_transport(
    relay_env: Option<String>,
    domain_env: Option<String>,
) -> (Option<String>, String) {
    let raw = relay_env.map(|u| u.trim().to_string()).unwrap_or_default();
    let off = matches!(
        raw.to_ascii_lowercase().as_str(),
        "off" | "disabled" | "none" | "local" | "in-process" | "0" | "false"
    );
    if off {
        return (None, COMMS_DOMAIN.to_string()); // escape hatch → in-process, no remote box
    }
    let url = if raw.is_empty() { CLUSTER_RELAY_URL.to_string() } else { raw };
    let domain = domain_env
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .or_else(|| host_of(&url))
        .unwrap_or_else(|| CLUSTER_RELAY_DOMAIN.to_string());
    (Some(url), domain)
}

/// Extract the host from a `ws://`|`wss://` URL: `wss://comms.citrate.ai:443/ws` → `comms.citrate.ai`.
/// `None` if there is no `//authority`. (Domain hosts only — not intended for bracketed IPv6.)
fn host_of(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    let authority = after_scheme.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?; // strip any userinfo
    let host = host.split(':').next()?; // strip any port
    (!host.is_empty()).then(|| host.to_string())
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
    // GROW-S2 transport (DEFAULT-ON for the alpha): use the shared rendezvous relay (CLUSTER_RELAY_URL)
    // so a partner who just opens the DMG connects out of the box. CITRATE_MEMBER_RELAY_URL overrides
    // (another URL, or an off-switch → in-process). The default + domain rule live in
    // `resolve_relay_transport` (unit-tested).
    let (relay_url, domain) =
        resolve_relay_transport(std::env::var(ENV_RELAY_URL).ok(), std::env::var(ENV_DOMAIN).ok());
    let mut mgr = CommsMemberManager::new(
        bin,
        data_root.join("member.sock"),
        data_root.join("member.bearer"),
        data_root.clone(),
        seed.to_string(),
        domain,
        data_root.join("member-crash.jsonl"),
    );
    if let Some(url) = relay_url {
        mgr = mgr.with_relay_url(url);
    }
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
pub async fn groups_create(app: tauri::AppHandle, name: String) -> std::result::Result<String, String> {
    match route(&app, Request::CreateGroup { name })? {
        Response::GroupCreated { id } => Ok(id),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **groups_list** — the member's groups.
#[tauri::command]
pub async fn groups_list(app: tauri::AppHandle) -> std::result::Result<Vec<(String, String)>, String> {
    match route(&app, Request::ListGroups)? {
        Response::Groups { groups } => Ok(groups.into_iter().map(|g| (g.id, g.name)).collect()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// **groups_self_address** — THIS member's own comms address (the identity the rosters key on — the
/// device-sealed comms key, NOT the wallet). Read-only, non-secret (the address only). The People
/// directory uses it to exclude yourself from your own roster overlap; it also gives the CONNECT arc a
/// coherent "who am I" that the wallet-address heuristic (the `iCreated` seam) can't reliably provide.
#[tauri::command]
pub async fn groups_self_address(app: tauri::AppHandle) -> std::result::Result<String, String> {
    device_identity(&app).map(|d| d.address)
}

/// **groups_join** — join a group this member was added to on a shared relay.
#[tauri::command]
pub async fn groups_join(app: tauri::AppHandle, group: String) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::JoinGroup { group })?)
}

/// **groups_add_member** — owner-invite a member (who has published a key package to the relay).
/// The daemon produces + publishes the MLS welcome; the invitee then joins. Post-S0 amendment.
#[tauri::command]
pub async fn groups_add_member(
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
/// CONNECT-S1 — submit a sealed claim (hex ciphertext) to the relay's server-blind claims-inbox, keyed
/// by the invite `token_hash` (hex). Invitee side. `pub(crate)` — driven by the invites module.
pub(crate) fn submit_claim<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    token_hash: String,
    ciphertext: String,
) -> std::result::Result<(), String> {
    match route(app, Request::SubmitClaim { token_hash, ciphertext })? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

/// CONNECT-S1 — poll the relay's claims-inbox by invite `token_hash` (hex). Returns the opaque hex
/// ciphertexts; the owner opens them with the invite's private key. `pub(crate)`.
pub(crate) fn poll_claims<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    token_hash: String,
) -> std::result::Result<Vec<String>, String> {
    match route(app, Request::PollClaims { token_hash })? {
        Response::Claims { ciphertexts } => Ok(ciphertexts),
        Response::Error { message } => Err(message),
        other => Err(format!("unexpected response: {other:?}")),
    }
}

#[tauri::command]
pub async fn groups_roster(
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
pub async fn groups_assign_role(
    app: tauri::AppHandle,
    group: String,
    member: String,
    role: String,
) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::AssignRole { group, member, role })?)
}

/// **groups_offboard** — atomic offboard (MLS remove + relay roster/tree drop).
#[tauri::command]
pub async fn groups_offboard(
    app: tauri::AppHandle,
    group: String,
    member: String,
) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::Offboard { group, member })?)
}

/// **groups_send** — post an encrypted message to a group.
#[tauri::command]
pub async fn groups_send(
    app: tauri::AppHandle,
    group: String,
    text: String,
) -> std::result::Result<(), String> {
    parse_ok(route(&app, Request::Send { group, text })?)
}

/// **groups_messages** — drain + decrypt the member's mailbox for a group.
#[tauri::command]
pub async fn groups_messages(
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
