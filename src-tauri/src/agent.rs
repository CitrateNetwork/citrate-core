//! citrate-core — node-agent sidecar + signature-request bridge (CORE-C1.2).
//! @rule8 · bearer-authed supervision surface + a sidecar that requests signatures.
//!
//! This wires the **node-agent** (`citrate-node-agent`) under the C1.0
//! [`crate::supervisor::Supervisor`] and bridges its UNSIGNED chain-write
//! requests into the B1.2/B1.4 [`crate::ceremony::SignatureCeremony`]. Two
//! @rule8 properties are the whole point of this module:
//!
//! 1. **Bearer-authed supervision (WP1).** The node-agent serves a loopback-only
//!    HTTP control surface (`127.0.0.1:19600`, [`SUPERVISION_ADDR`]) whose every
//!    endpoint except `/health` requires `Authorization: Bearer <token>`
//!    ([grounded](#grounded-node-agent-facts)). citrate-core **mints** a fresh
//!    256-bit token per session with `OsRng`, hands it to the child through the
//!    node-agent's own file-based IPC channel ([`TOKEN_FILE_ENV`] → a `0600`
//!    token file the child `load_or_create`s and reads back), and presents the
//!    SAME token on every supervision request. The token is held only as a
//!    [`Zeroizing`] string; it never appears in `Debug`, in an error, or in a
//!    log, and it is dropped/zeroized when the manager stops.
//!
//! 2. **No direct signing (WP2, the ADV-7 property).** The node-agent holds NO
//!    keys. It emits unsigned [`PendingSignatureRequest`]s on
//!    `GET /signature-requests`; this module fetches a pending one, wraps it in a
//!    [`crate::ceremony::SignatureIntent`] with `origin = "agent:node-agent"`,
//!    and routes it through the ceremony's request→approve→sign→broadcast path.
//!    The node-agent can NEVER obtain a signature or key without an explicit
//!    human ceremony approval — the gated signer is `pub(crate)` and reachable
//!    only from `SignatureCeremony::approve[_and_broadcast]` (asserted by the
//!    B1.2/B1.4 source-scan test, extended here for the agent origin). After a
//!    broadcast we report the tx hash back to the node-agent via
//!    `POST /signature-requests/{id}/observed` so it stops re-emitting the write.
//!
//! ## Grounded node-agent facts (read from citrate-node-agent @ 09c8627)
//! - Binary: `node-agent`, daemon mode via `--daemon` (or env
//!   `CITRATE_NODE_AGENT_DAEMON=1`), positional `<compute.json>` (+ optional
//!   job-id) — `crates/node-agent/src/main.rs:87` / `:246`.
//! - Supervision surface: `crates/supervision/src/server.rs` — `GET /health`
//!   (open), and bearer-gated `GET /status`, `POST /pause`, `POST /resume`,
//!   `GET /signature-requests`, `POST /signature-requests/:id/observed`. Bind
//!   loopback-only, env `CITRATE_NODE_AGENT_ADDR` default `127.0.0.1:19600`
//!   (`server.rs:47`).
//! - Bearer: `Authorization: Bearer <hex64>`, per-instance token
//!   `SupervisionAuth::generate` (32 bytes → 64 hex) persisted `0600` at a file
//!   whose path is env `CITRATE_NODE_AGENT_TOKEN_FILE` (`auth.rs:40`, `:84`).
//!   The file IS the inter-process channel (auth.rs module doc): the daemon
//!   `load_or_create`s it — an EXISTING non-empty token is reused — so if we
//!   write our minted token there first, the child adopts it and we both present
//!   the same bytes.
//! - SignatureRequest wire shape = `PendingSignatureRequest` JSON
//!   (`crates/supervision/src/state.rs:87`): `{ id:u64, intent:String,
//!   to:"0x..", calldata:"0x..", value_wei:decimal-string, chain_id:u64,
//!   context:String, expires_block:decimal-string, status:"pending"|"submitted",
//!   tx_hash?:String }`. `intent` is the Solidity fn name (`claimRewards`,
//!   `bidOnJob`, `heartbeat`, `startExecution`, …); `claimRewards` is the
//!   earnings sweep (`crates/earnings/src/lib.rs`).
//!
//! ## Lean tree (WP0)
//! node-agent is a SPAWNED binary, NOT a Cargo dependency — there is no
//! `node-agent` / `supervision` / `lifecycle` / `earnings` crate in this crate's
//! `cargo tree`. The wire contract is decoded from JSON here; the child is
//! bundled as a Tauri `externalBin` overlay exactly like the C1.1 node.
//!
//! ## Same-user exposure (honest note, folds in C1.1-3 / #71-F2)
//! The token is passed via a `0600` file (not argv → not in `ps`) and the child
//! reads it via env-named path. A same-UID process can still read the child's
//! `/proc/<pid>/environ` (which carries the file PATH, not the token) and, being
//! the same user, could read the `0600` token file itself — this is the exact
//! threat model the node-agent's `auth.rs` documents and accepts: the token
//! defends against OTHER local users and browsers, not a same-user compromise
//! (which already owns the keyring). We do not widen that boundary here.

// C1.2 delivers the agent manager as a Tauri-managed seam consumed by the
// AgentDomain bridge + the ceremony bridge; some constructor/status surface is
// only reached by that wiring + the tests, mirroring node.rs's staged consumers.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::ceremony::{
    BroadcastConfig, BroadcastResult, CeremonyError, IntentKind, SignatureCeremony, SignatureIntent,
};
use crate::custody::CustodyVault;
use crate::supervisor::{
    BackoffPolicy, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// The origin string stamped on every intent bridged from the node-agent. The
/// ceremony DISPLAYS this verbatim (B1.2-ADV-5) so a human approving a claim sees
/// exactly who asked. Never trusted to be benign — it is a label, not authority.
pub const AGENT_ORIGIN: &str = "agent:node-agent";

/// The node-agent supervision surface (loopback-only, grounded server.rs:47).
/// We pin the default and pass it to the child via [`ADDR_ENV`] so both sides
/// agree; a caller may override the port for tests via [`AgentManager::new`].
pub const SUPERVISION_ADDR: &str = "127.0.0.1:19600";

/// Env var the node-agent reads its bind address from (grounded server.rs:50).
pub const ADDR_ENV: &str = "CITRATE_NODE_AGENT_ADDR";

/// Env var the node-agent reads its bearer-token FILE PATH from (grounded
/// auth.rs:40). We write our minted token to this path `0600` before spawn; the
/// child `load_or_create`s it and adopts our token (the file IS the IPC channel).
pub const TOKEN_FILE_ENV: &str = "CITRATE_NODE_AGENT_TOKEN_FILE";

/// Env var that flips the node-agent into daemon mode (grounded main.rs:94).
pub const DAEMON_ENV: &str = "CITRATE_NODE_AGENT_DAEMON";

/// Length of the supervision bearer token in bytes (256-bit, matches
/// node-agent `SupervisionAuth::generate`). Rendered as 64 lowercase hex chars.
const TOKEN_LEN: usize = 32;

// ---------------------------------------------------------------------------
// The wire shape the node-agent serves (decoded, NOT a cargo dep of node-agent).
// ---------------------------------------------------------------------------

/// One unsigned chain write the node-agent needs signed, decoded from
/// `GET /signature-requests`. Field-for-field the node-agent's
/// `PendingSignatureRequest` (grounded state.rs:87). `value_wei`/`expires_block`
/// are decimal STRINGS on the wire (u128, precision-safe) — we keep them as
/// strings and only surface them; the ceremony signs the raw payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSignatureRequest {
    /// Stable id for the observe callback.
    pub id: u64,
    /// The Solidity function name (`claimRewards`, `bidOnJob`, `heartbeat`, …).
    pub intent: String,
    /// Target contract, `0x`-hex.
    pub to: String,
    /// ABI calldata, `0x`-hex.
    pub calldata: String,
    /// Wei to send, decimal string ("0" for the SELL writes).
    pub value_wei: String,
    /// Chain id the tx must be signed for.
    pub chain_id: u64,
    /// Human-readable description for the approval UI.
    pub context: String,
    /// Advisory block height past which signing is pointless (decimal string).
    pub expires_block: String,
    /// `"pending"` (awaiting signing) or `"submitted"` (broadcast + observed).
    pub status: String,
    /// The broadcast tx hash once observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
}

impl AgentSignatureRequest {
    /// Is this request awaiting a signature (vs already submitted)?
    pub fn is_pending(&self) -> bool {
        self.status == "pending"
    }
}

// ---------------------------------------------------------------------------
// The supervision transport seam (loopback HTTP), injectable for tests.
// ---------------------------------------------------------------------------

/// A single non-2xx-carrying HTTP response from the supervision surface: the
/// status code + the (small JSON) body. We keep the status so a 401 is
/// observable rather than collapsed into a transport error (the negative
/// control the WP1 acceptance requires).
#[derive(Debug, Clone)]
pub struct SupervisionResponse {
    pub status: u16,
    pub body: String,
}

impl SupervisionResponse {
    fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Transport errors from the supervision surface — never carry the bearer token.
#[derive(Debug, PartialEq, Eq)]
pub enum AgentError {
    /// The loopback surface was unreachable (child not up / RPC blip). No token.
    Transport(String),
    /// The surface answered with a non-2xx status (e.g. 401 wrong/absent bearer,
    /// 404 unknown id). Carries the PUBLIC status code only — never the token.
    Status(u16),
    /// The `GET /signature-requests` body could not be decoded to the grounded
    /// wire shape.
    Decode(String),
    /// There is no pending request to bridge (honest: nothing to sign this poll).
    NoPending,
    /// The supervisor refused to start the agent (thread/spawn failure). No token.
    Spawn(String),
    /// The bundled node-agent binary could not be located.
    BinaryNotFound(String),
    /// Minting/persisting the bearer token file failed (fail closed — never spawn
    /// an UNGATED agent surface). Carries the io error kind only, never the token.
    Token(String),
    /// The agent is already running (idempotent-start guard).
    AlreadyRunning,
    /// The ceremony refused/failed the bridged request. Carries the ceremony's
    /// (key-free) error rendering.
    Ceremony(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::Transport(m) => write!(f, "agent supervision transport error: {m}"),
            AgentError::Status(c) => write!(f, "agent supervision status {c}"),
            AgentError::Decode(m) => write!(f, "agent signature-request decode error: {m}"),
            AgentError::NoPending => write!(f, "no pending signature request"),
            AgentError::Spawn(m) => write!(f, "agent spawn error: {m}"),
            AgentError::BinaryNotFound(m) => write!(f, "node-agent binary not found: {m}"),
            AgentError::Token(m) => write!(f, "agent bearer-token error: {m}"),
            AgentError::AlreadyRunning => write!(f, "node-agent already running"),
            AgentError::Ceremony(m) => write!(f, "agent ceremony bridge error: {m}"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<CeremonyError> for AgentError {
    fn from(e: CeremonyError) -> Self {
        AgentError::Ceremony(e.to_string())
    }
}

/// The bearer-authed supervision transport. Every call presents the bearer;
/// implementors NEVER log/store it beyond the request. Production is
/// [`UreqSupervisionTransport`]; tests inject a mock or a real loopback stub.
///
/// The `bearer` is passed per-call (not held) so the transport itself owns no
/// long-lived copy of the secret — the manager holds the single [`Zeroizing`]
/// copy and lends it for the duration of one request.
pub trait SupervisionTransport: Send + Sync {
    /// `GET {base}{path}` with `Authorization: Bearer {bearer}`. Returns the
    /// status + body WITHOUT treating a non-2xx as a transport error (so a 401 is
    /// observable). A genuine transport failure is [`AgentError::Transport`].
    fn get(&self, base: &str, path: &str, bearer: &str) -> Result<SupervisionResponse, AgentError>;

    /// `POST {base}{path}` with the bearer + a JSON body. Same status semantics.
    fn post_json(
        &self,
        base: &str,
        path: &str,
        bearer: &str,
        body: &str,
    ) -> Result<SupervisionResponse, AgentError>;
}

/// Production supervision transport over blocking `ureq` (already in the tree; no
/// tokio). Non-2xx is surfaced as a response (not an error) so a 401 negative
/// control is observable. The bearer is set on the request header and dropped
/// with the request — never retained.
pub struct UreqSupervisionTransport;

impl SupervisionTransport for UreqSupervisionTransport {
    fn get(&self, base: &str, path: &str, bearer: &str) -> Result<SupervisionResponse, AgentError> {
        let url = format!("{base}{path}");
        let resp = ureq::get(&url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| AgentError::Transport(e.to_string()))?;
        read_response(resp)
    }

    fn post_json(
        &self,
        base: &str,
        path: &str,
        bearer: &str,
        body: &str,
    ) -> Result<SupervisionResponse, AgentError> {
        let url = format!("{base}{path}");
        let resp = ureq::post(&url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .header("Content-Type", "application/json")
            .config()
            .http_status_as_error(false)
            .build()
            .send(body)
            .map_err(|e| AgentError::Transport(e.to_string()))?;
        read_response(resp)
    }
}

/// Read a ureq response into a [`SupervisionResponse`] (status + body string).
fn read_response(
    mut resp: ureq::http::Response<ureq::Body>,
) -> Result<SupervisionResponse, AgentError> {
    let status = resp.status().as_u16();
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| AgentError::Transport(e.to_string()))?;
    Ok(SupervisionResponse { status, body })
}

// ---------------------------------------------------------------------------
// Bearer token — minted per session with OsRng, persisted 0600, zeroized.
// ---------------------------------------------------------------------------

/// Mint a fresh 256-bit supervision bearer token (64 lowercase hex chars) using
/// the OS CSPRNG, matching node-agent `SupervisionAuth::generate`. Held in a
/// [`Zeroizing`] so the string is wiped on drop; never `Debug`-printed/logged.
fn mint_bearer() -> Zeroizing<String> {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = Zeroizing::new([0u8; TOKEN_LEN]);
    OsRng.fill_bytes(bytes.as_mut());
    // hex-encode into a Zeroizing<String>; the raw byte buffer zeroizes on drop.
    Zeroizing::new(hex::encode(bytes.as_ref()))
}

/// Write the bearer `token` to `path` with `0600` perms (parent `0700`), the same
/// inode hardening the node-agent's `auth.rs` enforces. The file IS the IPC
/// channel: the child `load_or_create`s it and adopts our token. We (re)write it
/// on every start so a stale/loosened token file is replaced with THIS session's.
fn persist_bearer(path: &Path, token: &str) -> Result<(), AgentError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AgentError::Token(e.kind().to_string()))?;
        harden_dir_perms(parent).map_err(|e| AgentError::Token(e.kind().to_string()))?;
    }
    std::fs::write(path, token.as_bytes()).map_err(|e| AgentError::Token(e.kind().to_string()))?;
    harden_perms(path).map_err(|e| AgentError::Token(e.kind().to_string()))?;
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
// The bridge: node-agent SignatureRequest -> SignatureIntent -> ceremony.
// ---------------------------------------------------------------------------

/// Build the ceremony [`SignatureIntent`] for a node-agent request. The origin is
/// stamped [`AGENT_ORIGIN`] (displayed verbatim); the payload is the request's
/// `calldata` (the raw bytes the human approves). It is an
/// [`IntentKind::Transaction`] bound to the request's `chain_id` — so the
/// ceremony's B1.4 approve+broadcast path signs a REAL legacy tx to the target
/// contract. A node-agent request NEVER carries a key or a signature; it is a
/// pure unsigned intent.
///
/// `gas` is the gas limit to bind. A node-agent request carries calldata but NO
/// gas, and B1.4's `finalize` refuses to GUESS execution gas for a contract call
/// (safety) — so the bridge supplies a real `eth_estimateGas` value here (Rule 1:
/// never a fabricated number). `None` omits it (only valid for an empty-calldata
/// value transfer, which the node-agent never emits).
///
/// `from` is THIS vault's wallet address. The node-agent request does NOT name a
/// sender (it holds no keys); the SIGNER is citrate-core's vault, so the bridge
/// stamps `from` = the vault address. The ceremony (B1.4/B1.5 F-2) then asserts
/// the signed tx's sender IS the vault key and fetches the pending nonce for it —
/// a node-agent can never redirect a signature to a different sender.
pub fn intent_from_request(
    req: &AgentSignatureRequest,
    from: &str,
    gas: Option<u64>,
) -> SignatureIntent {
    SignatureIntent {
        origin: AGENT_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: req.chain_id,
        // The raw payload the ceremony surfaces + signs. B1.4's tx decoder reads a
        // JSON tx object; we present the calldata + destination so the ceremony can
        // decode `{to, value, calldata}` and the human sees the real action.
        raw: encode_tx_json(req, from, gas),
    }
}

/// Encode a node-agent request as the JSON tx object shape B1.4's
/// `txdecode::decode_transaction` consumes: `{ from, to, value, data, gas }`
/// (+ chainId). `value_wei` is a decimal string on the wire; converted to a
/// `0x`-hex quantity. `from` is the vault wallet (the real signer); `gas` (from
/// `eth_estimateGas`) lets `finalize` build signable fields for a contract call
/// (no guessed gas — Rule 1). No key material.
fn encode_tx_json(req: &AgentSignatureRequest, from: &str, gas: Option<u64>) -> String {
    // Convert decimal value_wei -> 0x-hex quantity for the tx object (0 -> "0x0").
    let value_hex = req
        .value_wei
        .parse::<u128>()
        .map(|v| format!("0x{v:x}"))
        .unwrap_or_else(|_| "0x0".to_string());
    let mut obj = serde_json::json!({
        "from": from,
        "to": req.to,
        "value": value_hex,
        "data": req.calldata,
        "chainId": format!("0x{:x}", req.chain_id),
    });
    if let Some(g) = gas {
        obj["gas"] = serde_json::json!(format!("0x{g:x}"));
    }
    obj.to_string()
}

/// The `eth_estimateGas` call object for a node-agent request: `{to, value, data}`.
/// Built here so the RPC client stays transport-only.
fn estimate_gas_call(req: &AgentSignatureRequest) -> serde_json::Value {
    let value_hex = req
        .value_wei
        .parse::<u128>()
        .map(|v| format!("0x{v:x}"))
        .unwrap_or_else(|_| "0x0".to_string());
    serde_json::json!({
        "to": req.to,
        "value": value_hex,
        "data": req.calldata,
    })
}

// ---------------------------------------------------------------------------
// The node-agent manager: supervisor + supervision client + ceremony bridge.
// ---------------------------------------------------------------------------

/// The node-agent manager. Owns the resolved binary, the compute-config path, a
/// temp bearer-token file path, the loopback supervision base URL, the injected
/// transport, and (while running) a live [`Supervisor`] + the session bearer.
/// Managed as Tauri state; `start`/`status`/`stop` drive the child, and
/// `poll_and_bridge` routes one pending signature request through the ceremony.
pub struct AgentManager {
    /// The bundled node-agent binary path (resolved from the Tauri resource dir
    /// or a dev/stub override).
    bin: PathBuf,
    /// The `compute.json` the node-agent reads (positional arg).
    config_path: PathBuf,
    /// Where the per-session bearer token file is written (`0600`).
    token_path: PathBuf,
    /// Where crash records are appended.
    crash_record_path: PathBuf,
    /// The loopback supervision base URL, e.g. `http://127.0.0.1:19600`.
    base_url: String,
    /// The bind address handed to the child via [`ADDR_ENV`] (host:port).
    bind_addr: String,
    /// The supervision transport (production ureq; tests inject a mock/stub).
    transport: Box<dyn SupervisionTransport>,
    /// The live supervisor, present only while the agent is running.
    sup: Mutex<Option<Supervisor>>,
    /// The session bearer token, minted on `start`, held ONLY here as Zeroizing,
    /// lent per-request, wiped on `stop`/drop. Never logged / Debug'd / in errors.
    bearer: Mutex<Option<Zeroizing<String>>>,
    /// C1.2-F-1 dedup map: node-agent request `id` → the ceremony `id` already
    /// minted for it. A still-`pending` request maps to AT MOST ONE ceremony, so
    /// two `bridge_one_pending` calls for the same request return the SAME
    /// ceremony (never a second one → never a second broadcast). The entry is
    /// CLEARED once the request is observed/broadcast (in `approve_bridged_and_
    /// report`) so a later legitimate re-accrual under the same id can bridge
    /// afresh. Cleared wholesale on `stop` (a new session starts clean).
    bridged: Mutex<HashMap<u64, String>>,
}

/// The bridge status shape surfaced to the AgentDomain seam. Carries only PUBLIC
/// facts — supervisor state + pending-request count. NEVER the bearer token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentStatus {
    /// "stopped" | "starting" | "running" | "restarting" | "failed".
    pub state: String,
    /// Whether a live bearer session exists (the supervision surface is gated).
    pub authed: bool,
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

impl AgentManager {
    /// Build a manager over explicit paths + a supervision transport. Production
    /// uses [`build_agent_state`]; tests inject a stub binary + temp paths + a
    /// mock/stub transport.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bin: PathBuf,
        config_path: PathBuf,
        token_path: PathBuf,
        crash_record_path: PathBuf,
        base_url: impl Into<String>,
        bind_addr: impl Into<String>,
        transport: Box<dyn SupervisionTransport>,
    ) -> Self {
        AgentManager {
            bin,
            config_path,
            token_path,
            crash_record_path,
            base_url: base_url.into(),
            bind_addr: bind_addr.into(),
            transport,
            sup: Mutex::new(None),
            bearer: Mutex::new(None),
            bridged: Mutex::new(HashMap::new()),
        }
    }

    /// Build the [`SidecarSpec`] for the node-agent: explicit binary, `--daemon`,
    /// the positional `compute.json`, and the supervision addr + token-file path +
    /// daemon flag in the ENV (never argv — argv would leak the token FILE PATH to
    /// `ps`; the token itself is never on argv either). The token is NOT an env
    /// value — only its FILE PATH is, and the file is `0600` (the grounded
    /// node-agent IPC channel).
    fn build_spec(&self) -> SidecarSpec {
        let mut spec = SidecarSpec::new(
            "node-agent",
            self.bin.clone(),
            vec![
                "--daemon".to_string(),
                self.config_path.to_string_lossy().to_string(),
            ],
        );
        spec.env = vec![
            (DAEMON_ENV.to_string(), "1".to_string()),
            (ADDR_ENV.to_string(), self.bind_addr.clone()),
            (
                TOKEN_FILE_ENV.to_string(),
                self.token_path.to_string_lossy().to_string(),
            ),
        ];
        spec
    }

    /// Start the node-agent under the supervisor. @rule8: mint a fresh session
    /// bearer with `OsRng`, persist it `0600` to the token file the child adopts,
    /// hold the single copy here as [`Zeroizing`], and spawn. Idempotent-ish:
    /// `AlreadyRunning` if a supervisor is already live. Fails CLOSED if the
    /// binary is missing or the token cannot be persisted (never spawn an ungated
    /// agent surface).
    pub fn start(&self) -> Result<(), AgentError> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(AgentError::AlreadyRunning);
        }
        if !self.bin.exists() {
            return Err(AgentError::BinaryNotFound(self.bin.display().to_string()));
        }
        // Mint + persist the per-session bearer BEFORE spawn so the child adopts
        // it via load_or_create (the file is the IPC channel).
        let bearer = mint_bearer();
        persist_bearer(&self.token_path, &bearer)?;

        let spec = self.build_spec();
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        let sup = Supervisor::start(config).map_err(|e| AgentError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        *self.bearer.lock().unwrap_or_else(|e| e.into_inner()) = Some(bearer);
        Ok(())
    }

    /// Stop the node-agent: graceful supervisor release (SIGTERM → grace →
    /// SIGKILL, no orphan) AND wipe the session bearer (Zeroizing drop) + best-
    /// effort remove the token file so no session credential lingers. Idempotent.
    pub fn stop(&self) {
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(sup) = sup {
            sup.stop();
            drop(sup);
        }
        // Drop the in-memory bearer (Zeroizing wipes it) and remove the file.
        *self.bearer.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _ = std::fs::remove_file(&self.token_path);
        // C1.2-F-1: a new session starts with a clean dedup map (request ids are
        // per-session on the node-agent side).
        self.bridged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// The current supervisor state + whether a bearer session exists. PUBLIC
    /// facts only — never the token.
    pub fn status(&self) -> AgentStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let authed = self
            .bearer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some();
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        AgentStatus {
            state: state.to_string(),
            authed,
        }
    }

    /// Lend the session bearer to `f` for the duration of one request. Returns
    /// [`AgentError::Transport`] ("no session") if the agent is not running —
    /// callers can never reach the supervision surface without a live bearer.
    fn with_bearer<T>(
        &self,
        f: impl FnOnce(&str) -> Result<T, AgentError>,
    ) -> Result<T, AgentError> {
        let guard = self.bearer.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(tok) => f(tok),
            None => Err(AgentError::Transport("no bearer session".into())),
        }
    }

    /// `GET /status` — the coarse agent lifecycle string, bearer-authed. Proves
    /// the supervision handshake (bearer round-trip) end-to-end.
    pub fn supervision_status(&self) -> Result<String, AgentError> {
        self.with_bearer(|bearer| {
            let resp = self.transport.get(&self.base_url, "/status", bearer)?;
            if !resp.is_ok() {
                return Err(AgentError::Status(resp.status));
            }
            // The body is a bare JSON string, e.g. "idle".
            serde_json::from_str::<String>(&resp.body)
                .map_err(|e| AgentError::Decode(e.to_string()))
        })
    }

    /// `GET /signature-requests` — the node-agent's unsigned chain writes,
    /// bearer-authed. Decodes the grounded wire shape.
    pub fn fetch_signature_requests(&self) -> Result<Vec<AgentSignatureRequest>, AgentError> {
        self.with_bearer(|bearer| {
            let resp = self
                .transport
                .get(&self.base_url, "/signature-requests", bearer)?;
            if !resp.is_ok() {
                return Err(AgentError::Status(resp.status));
            }
            serde_json::from_str::<Vec<AgentSignatureRequest>>(&resp.body)
                .map_err(|e| AgentError::Decode(e.to_string()))
        })
    }

    /// `POST /signature-requests/{id}/observed` — report a broadcast tx hash back
    /// so the node-agent stops re-emitting the write. Bearer-authed.
    pub fn report_observed(&self, id: u64, tx_hash: &str) -> Result<(), AgentError> {
        self.with_bearer(|bearer| {
            let body = serde_json::json!({ "tx_hash": tx_hash }).to_string();
            let resp = self.transport.post_json(
                &self.base_url,
                &format!("/signature-requests/{id}/observed"),
                bearer,
                &body,
            )?;
            if !resp.is_ok() {
                return Err(AgentError::Status(resp.status));
            }
            Ok(())
        })
    }

    /// The full C1.2 bridge for ONE pending request (WP2, the ADV-7 path):
    ///   1. fetch the node-agent's pending signature requests (bearer-authed),
    ///   2. take the first `pending` one (else [`AgentError::NoPending`]),
    ///   3. **C1.2-F-1 dedup:** if this request `id` ALREADY has a still-pending
    ///      ceremony, return THAT ceremony (no second ceremony, no second
    ///      broadcast). Only mint a new ceremony if there is none, or the prior
    ///      one was already consumed (approved/rejected).
    ///   4. estimate the call's gas from the live RPC (`eth_estimateGas`) — the
    ///      node-agent request has no gas and B1.4 won't guess one (Rule 1),
    ///   5. wrap it in a `SignatureIntent{origin:"agent:node-agent", …}` and
    ///      `request` it into the ceremony (creates a PENDING ceremony — NO key),
    ///      record `req.id → ceremony.id` in the dedup map,
    ///   6. return the ceremony id + the request so the HUMAN can approve it.
    ///
    /// This is the split the ADV-7 property demands: bridging NEVER signs. A
    /// signature is produced ONLY when the human later approves the returned
    /// ceremony id via the ceremony's own path (proven by
    /// [`Self::approve_bridged_and_report`]). The node-agent cannot obtain a
    /// signature or key from this method.
    ///
    /// `vault` supplies the `from` (the real signer's address — reads the vault
    /// wallet's public identity, NOT the key). `rpc` is injected (Rule 1: tests
    /// mock the transport). A gas-estimate blip falls back to omitting gas — the
    /// ceremony then refuses to finalize rather than signing with a guessed gas
    /// (fail closed). A locked/absent vault surfaces as a ceremony error at
    /// approve time (the bridge still builds the pending intent for display).
    pub fn bridge_one_pending<T: crate::rpc::RpcTransport>(
        &self,
        ceremony: &SignatureCeremony,
        vault: &CustodyVault,
        rpc: &crate::rpc::RpcClient<T>,
    ) -> Result<(String, AgentSignatureRequest), AgentError> {
        let requests = self.fetch_signature_requests()?;
        let req = requests
            .into_iter()
            .find(|r| r.is_pending())
            .ok_or(AgentError::NoPending)?;
        let id = self.bridge_request(ceremony, vault, rpc, &req)?;
        Ok((id, req))
    }

    /// **C2-F-1 — the USER Claim path.** Build the user's unsigned `claimRewards()`
    /// request (via [`crate::earnings::user_claim_request`], carrying the REAL
    /// claimable in its context — the value the caller just read on-chain), and
    /// bridge it into a PENDING ceremony through the SAME
    /// [`Self::bridge_request`] path the node-agent sweep uses. Returns the
    /// ceremony id the HUMAN must approve via the ceremony's own
    /// `sign_and_broadcast` (B1.4) — this method signs NOTHING and touches no key.
    ///
    /// The user claim carries the DISJOINT [`crate::earnings::USER_CLAIM_ID`], so it
    /// can never alias a node-agent request in the shared dedup map (C2-F-3). If the
    /// caller has nothing to claim (`claimable_wei == 0`) we return
    /// [`AgentError::NoPending`] — an HONEST "nothing to claim", never a fabricated
    /// settlement (Rule 1). The claim's on-chain effect is REAL: the ceremony signs
    /// the SAME `claimRewards()` 4 bytes the contract expects and broadcasts to
    /// 40204; there is no local balance mutation presented as a chain claim.
    pub fn bridge_user_claim<T: crate::rpc::RpcTransport>(
        &self,
        ceremony: &SignatureCeremony,
        vault: &CustodyVault,
        rpc: &crate::rpc::RpcClient<T>,
        claimable_wei: u128,
        to: &str,
    ) -> Result<(String, AgentSignatureRequest), AgentError> {
        // Honest zero: nothing to claim → no ceremony, no tx (Rule 1 / I-3).
        if claimable_wei == 0 {
            return Err(AgentError::NoPending);
        }
        let req = crate::earnings::user_claim_request_to(claimable_wei, to);
        let id = self.bridge_request(ceremony, vault, rpc, &req)?;
        Ok((id, req))
    }

    /// The shared, ATOMIC check-and-mint that turns ONE unsigned request (from the
    /// node-agent sweep OR the user Claim button) into AT MOST ONE pending ceremony.
    /// Signs nothing; produces no key material.
    ///
    /// C1.2-F-1 (dedup) + C2-F-2 (atomicity): a request id maps to at most one
    /// still-pending ceremony. A FAST PATH returns an existing pending ceremony
    /// without estimating gas or minting. Otherwise the check-and-mint is performed
    /// under a SINGLE hold of the `bridged` lock (a re-check + `ceremony.request` +
    /// insert), so two concurrent bridges of the same id cannot each mint a
    /// ceremony — the loser re-checks under the lock and reuses the winner's.
    fn bridge_request<T: crate::rpc::RpcTransport>(
        &self,
        ceremony: &SignatureCeremony,
        vault: &CustodyVault,
        rpc: &crate::rpc::RpcClient<T>,
        req: &AgentSignatureRequest,
    ) -> Result<String, AgentError> {
        // FAST PATH: under the map lock, if this id already maps to a STILL-pending
        // ceremony, return it verbatim WITHOUT estimating gas or minting. A stale
        // entry (the ceremony was already approved/rejected) is dropped so a
        // legitimate re-accrual can mint a fresh one. Releasing the lock here is
        // safe because the SLOW path below re-checks under the lock before minting
        // (the C2-F-2 double-check).
        {
            let mut map = self.bridged.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = map.get(&req.id) {
                if ceremony.status(existing).is_some() {
                    return Ok(existing.clone());
                }
                map.remove(&req.id);
            }
        }

        // The real signer is THIS vault's wallet — stamp its address as `from` so
        // the ceremony fetches the pending nonce for it and binds the sender.
        let from = crate::wallet::address(vault)
            .map(|w| w.address)
            .map_err(|e| AgentError::Ceremony(e.to_string()))?;
        // Real gas estimate for the contract call (never a fabricated number).
        // Computed BEFORE the mint lock so the (potentially blocking) RPC does not
        // hold the dedup map; a concurrent winner may make this estimate moot, in
        // which case its ceremony is discarded below (idempotent read, no harm).
        let gas = rpc.estimate_gas(estimate_gas_call(req)).ok();

        // C2-F-2 (LOW): the check-and-mint MUST be atomic. Two concurrent bridges
        // of the SAME still-pending req.id previously raced between the fast-path
        // check (lock released) and the insert (lock re-acquired) — both saw an
        // empty slot, both minted a ceremony, and the second insert overwrote the
        // first, leaving TWO approvable ceremonies for one request (a double-claim
        // surface). We now hold the map lock across the RE-CHECK + mint + insert:
        // whichever thread wins the lock mints exactly one ceremony and records it;
        // the loser re-checks under the same lock, finds the winner's still-pending
        // ceremony, and returns THAT (discarding its own would-be mint). Exactly one
        // ceremony per request id. `ceremony.request`/`status` lock only the
        // ceremony's OWN mutex (disjoint from `bridged`), so holding `bridged`
        // across them cannot deadlock.
        let mut map = self.bridged.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = map.get(&req.id) {
            if ceremony.status(existing).is_some() {
                // A concurrent bridge already minted for this id — reuse it and do
                // NOT mint a second ceremony (the atomicity that closes C2-F-2; the
                // estimate above is simply discarded).
                return Ok(existing.clone());
            }
            map.remove(&req.id);
        }
        let intent = intent_from_request(req, &from, gas);
        let view = ceremony.request(intent);
        map.insert(req.id, view.id.clone());
        Ok(view.id)
    }

    /// The human-approval leg (WP2): approve a bridged ceremony `id` (bound to the
    /// original request `req`), signing + broadcasting via the B1.4 path, then
    /// report the tx hash back to the node-agent. The ceremony enforces every
    /// B1.2/B1.4 invariant (consume-first single-use, raw-ack gate, fail-closed
    /// when locked). The node-agent origin has NO special path — it goes through
    /// the SAME `approve_and_broadcast` a user tx does.
    ///
    /// `rpc` is injected so tests mock the transport (Rule 1). On a successful
    /// broadcast we `report_observed` so the node-agent stops re-emitting.
    #[allow(clippy::too_many_arguments)] // distinct ceremony/vault/rpc/req params
    pub fn approve_bridged_and_report<T: crate::rpc::RpcTransport>(
        &self,
        ceremony: &SignatureCeremony,
        vault: &CustodyVault,
        rpc: &crate::rpc::RpcClient<T>,
        id: &str,
        raw_ack: bool,
        req: &AgentSignatureRequest,
        cfg: BroadcastConfig,
    ) -> Result<BroadcastResult, AgentError> {
        let result = ceremony.approve_and_broadcast(vault, rpc, id, raw_ack, cfg)?;
        // C1.2-F-1: the ceremony is now consumed (single-use) and the tx is
        // broadcast. Drop the request→ceremony dedup entry so the map does not
        // leak and a future legitimate re-accrual under the same id can bridge
        // afresh. The ceremony's own consume-first guarantees THIS ceremony can
        // never broadcast twice; clearing the entry here keeps the two invariants
        // aligned (one request → one ceremony → one broadcast).
        {
            let mut map = self.bridged.lock().unwrap_or_else(|e| e.into_inner());
            if map.get(&req.id).map(|c| c == id).unwrap_or(false) {
                map.remove(&req.id);
            }
        }
        // Best-effort observe: the tx is already broadcast; a failed report just
        // means the node-agent may re-emit (dedup-by-calldata on its side makes
        // that safe). Do NOT fail the whole bridge on a report blip.
        let _ = self.report_observed(req.id, &result.tx_hash);
        Ok(result)
    }
}

/// Managed Tauri state: the process-wide node-agent manager.
pub struct AgentState(pub AgentManager);

/// Resolve the bundled `node-agent` sidecar binary path (D-C1-1, same overlay
/// approach as the node): `CITRATE_NODE_AGENT_BIN` override first (dev/tests),
/// else the Tauri resource dir (`externalBin` strips the target-triple suffix to
/// `node-agent`).
fn resolve_agent_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CITRATE_NODE_AGENT_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "CITRATE_NODE_AGENT_BIN set but not found: {}",
            path.display()
        ));
    }
    // Bundled externalBin: installed next to the main executable (Contents/MacOS).
    crate::supervisor::resolve_external_bin(app, "node-agent")
}

/// Build the managed agent state from a live app handle: the bundled binary, the
/// `compute.json` in the app data dir, a per-session token file, a crash-record
/// path, and the production ureq supervision transport bound to the loopback
/// surface.
pub fn build_agent_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<AgentState, String> {
    use tauri::Manager;
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let agent_dir = data_root.join("node-agent");
    let config_path = agent_dir.join("compute.json");
    let token_path = agent_dir.join("supervision.token");
    let crash_record_path = agent_dir.join("crash-records.jsonl");
    let bin = resolve_agent_bin(app)?;
    Ok(AgentState(AgentManager::new(
        bin,
        config_path,
        token_path,
        crash_record_path,
        format!("http://{SUPERVISION_ADDR}"),
        SUPERVISION_ADDR,
        Box::new(UreqSupervisionTransport),
    )))
}

// ---------------------------------------------------------------------------
// Tauri commands — the AgentDomain bridge surface. Return status / () only;
// NEVER the bearer token or any key/signature-without-ceremony.
// ---------------------------------------------------------------------------

use tauri::State;

#[tauri::command]
pub fn agent_status(state: State<'_, AgentState>) -> std::result::Result<AgentStatus, String> {
    Ok(state.0.status())
}

#[tauri::command]
pub fn agent_start(state: State<'_, AgentState>) -> std::result::Result<(), String> {
    state.0.start().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_stop(state: State<'_, AgentState>) -> std::result::Result<(), String> {
    state.0.stop();
    Ok(())
}

/// **Command — user_claim (C2-F-1, @rule8).** The USER's Claim button. It does the
/// REAL claim, NEVER a sim mutation presented as a chain settlement:
///   1. read the vault wallet's REAL claimable from
///      `ContributionAccounting.claimable(addr)` via `eth_call` on the live 40204
///      RPC (the same read `agent_earnings` surfaces — Rule 1, no fabricated value),
///   2. if it is `0`, return an HONEST error ("nothing to claim") — no ceremony, no
///      tx, no faked balance change,
///   3. otherwise bridge the unsigned `claimRewards()` intent (carrying the real
///      claimable) into a PENDING [`crate::ceremony::SignatureCeremony`] and return
///      the [`crate::ceremony::CeremonyView`] (id + decoded action). Signs NOTHING.
///
/// The HUMAN then approves the returned ceremony id via `sign_and_broadcast`
/// (B1.4) — the SAME single human-in-the-loop path a node-agent sweep or a user tx
/// takes. There is NO local balance mutation and NO fabricated "claimed" hash: the
/// balance only changes when the real broadcast tx settles on-chain and the tab
/// re-reads `claimable`. Requires the vault UNLOCKED (to read the public address +
/// sign at approve); a locked/absent vault fails closed with a clear error.
#[tauri::command]
pub async fn user_claim(
    agent: State<'_, AgentState>,
    ceremony: State<'_, crate::ceremony::CeremonyState>,
    custody: State<'_, crate::custody::CustodyState>,
    node: State<'_, crate::node::NodeState>,
) -> std::result::Result<crate::ceremony::CeremonyView, String> {
    // Read the wallet's public address (never the key).
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let rpc = crate::rpc::RpcClient::citrate();

    // WHICH CLAIM IS THIS? The two are different contracts with different money.
    //
    // A VALIDATOR's rewards accrue in `ValidatorRegistry` against the proposer
    // pubkey, and only the registered staker — the member's MemberBond CLONE — may
    // call `ValidatorRegistry.claimRewards(pubkey)`. The member reaches them
    // through `MemberBond.claimRewards()`, which is `onlyMember` (so the EOA IS the
    // right signer) and forwards the proceeds on.
    //
    // Everything here previously went to `ContributionAccounting`, so a validator
    // pressing Claim signed a tx against a contract holding none of their rewards.
    // It became reachable the moment validator earnings were surfaced, because the
    // button enables on a non-zero claimable that this tx could never collect.
    //
    // Same 4 bytes either way — `claimRewards()` is 0x372500ab on BOTH — so only
    // the target changes. (`ValidatorRegistry.claimRewards(bytes32)` is 0x7790ddc6
    // and is NOT what we build: the clone calls that, not the member.)
    let grant = crate::grant_status::read_grant_status(&rpc, &wallet.address).ok();
    let is_validator = grant
        .as_ref()
        .map(|g| g.bond_deployed && g.has_validator)
        .unwrap_or(false);

    let (to, claimable_wei) = if is_validator {
        // MATURED validator rewards only — `rewardsOf(pubkey).claimableNow`. Rewards
        // inside the REWARD_RING evidence window are still slashable and are NOT
        // claimable; the contract reports them separately for exactly that reason.
        let pubkey_hex = node.0.proposer_pubkey()?;
        let raw = pubkey_hex.strip_prefix("0x").unwrap_or(&pubkey_hex);
        let bytes = hex::decode(raw).map_err(|e| format!("bad proposer pubkey hex: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!("proposer pubkey is {} bytes, expected 32", bytes.len()));
        }
        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&bytes);
        let (_total, claimable) = crate::validator::read_validator_rewards(
            &rpc,
            crate::addresses::validator_registry(),
            &pubkey,
        )?;
        let bond = grant.as_ref().map(|g| g.bond_address.clone()).unwrap_or_default();
        (bond, claimable)
    } else {
        let snap = crate::earnings::read_claimable(&rpc, &wallet.address).map_err(|e| e.to_string())?;
        let wei: u128 = snap
            .claimable_wei
            .parse()
            .map_err(|_| "earnings: claimable is not a u128 wei value".to_string())?;
        (crate::earnings::CONTRIBUTION_ACCOUNTING.to_string(), wei)
    };

    // Bridge the REAL claim into a pending ceremony (honest 0 → NoPending error).
    let (id, _req) = agent
        .0
        .bridge_user_claim(&ceremony.0, &custody.0, &rpc, claimable_wei, &to)
        .map_err(|e| e.to_string())?;
    // Return the pending ceremony view so the human approves it via the ceremony's
    // own sign_and_broadcast (B1.4) — this command never signs.
    ceremony
        .0
        .status(&id)
        .ok_or_else(|| "ceremony: pending claim not found after bridge".to_string())
}

/// A minimal writer helper for the token file used only by tests that need to
/// assert perms without going through `start`. Kept crate-visible for the test
/// module include.
#[cfg(test)]
pub(crate) fn test_persist_bearer(path: &Path, token: &str) -> Result<(), AgentError> {
    persist_bearer(path, token)
}

#[cfg(test)]
impl AgentManager {
    /// Inject a session bearer as if `start` had minted one, so the bearer-gated
    /// supervision calls can be exercised WITHOUT spawning a child.
    pub(crate) fn test_set_bearer(&self, token: &str) {
        *self.bearer.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Zeroizing::new(token.to_string()));
    }
}

#[cfg(test)]
mod tests {
    include!("agent_tests.rs");
}
