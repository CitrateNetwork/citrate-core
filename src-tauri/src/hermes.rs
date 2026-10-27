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
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::ceremony::{CeremonyView, IntentKind, SignatureCeremony, SignatureIntent};
use crate::custody::CustodyVault;
use crate::supervisor::{
    BackoffPolicy, HealthCheck, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// The origin stamped on every intent bridged from Hermes; the ceremony DISPLAYS it verbatim so a
/// human approving a chain effect sees it came from the agent, not the local user.
const HERMES_ORIGIN: &str = "agent:hermes";
/// The Citrate chain id (40204). A Hermes chain effect carries no chain id; the bridge stamps this.
const CITRATE_CHAIN_ID: u64 = 40204;

// CX-S6.4 — the code-task HIC routing surface. Declared as a submodule of this (s6-owned) file so the
// new module needs no `mod` line in the (s0-owned) lib.rs; the file is still `agent_tools.rs`.
#[path = "agent_tools.rs"]
pub mod agent_tools;
/// HUP-S2.9: undo for agent file changes (the sidecar's `/checkpoints` routes).
#[path = "hermes_undo.rs"]
pub mod undo;
// HUP-S7.3/S7.5: the sidecar's metering + anchor routes (core side).
#[path = "hermes_chain.rs"]
pub mod chain;

// HUP-S1.9 — the sidecar's worker processes (GET /workers), read for the Activity monitor.
#[path = "hermes_workers.rs"]
pub mod workers;

// HUP-S5.3 — the decide() slot's per-backend metering (GET /decide/stats), for the Activity monitor.
#[path = "hermes_decide.rs"]
pub mod decide;

/// The Hermes harness loopback control bind. Distinct from node RPC (8545), llama (18080),
/// node-agent (19600), and comms (8787/8788).
pub const HERMES_CONTROL_ADDR: &str = "127.0.0.1:19700";
/// Env the Hermes child reads its control bind from.
const HERMES_ADDR_ENV: &str = "CITRATE_HERMES_ADDR";
/// Env the Hermes child reads its bearer-token FILE PATH from (the file is the IPC channel).
const HERMES_TOKEN_FILE_ENV: &str = "CITRATE_HERMES_TOKEN_FILE";
/// Env the Hermes child reads its capsule (skill) directory from. Without it the child defaults to
/// `./capsules` relative to its cwd — which is empty — so the agent boots with zero skills and can run
/// nothing. citrate-core points it at the per-session capsule dir it seeds from the bundled starters.
const HERMES_CAPSULES_ENV: &str = "CITRATE_HERMES_CAPSULES";
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
/// How long a restart waits for the old child's control port to be released.
const RESTART_PORT_WAIT: Duration = Duration::from_secs(5);

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
    /// The ceremony bridge failed (e.g. the vault is locked/absent so `from` can't be read). Fail
    /// closed — never bridge without the real signer's address.
    Ceremony(String),
    /// PBA-L7b-009: another process already holds the control port; refusing to start.
    PortInUse(u16),
    /// PBA-L7b-003: the sidecar's head is no longer the action the member reviewed (a timeout
    /// eviction or a newer effect changed it). Nothing was resolved; the member must re-review.
    Stale,
    /// HUP-S1.4: the sidecar refused a brief request (422): no track fits the goal, an answer is not
    /// one of its choices, or an edit dropped a required gate. Carries the sidecar's reason verbatim
    /// so the interview card can show it inline.
    BriefRefused(String),
    /// K5a: Hermes restarted since a Store daemon's tools were checked; the session was not
    /// opened (the new sidecar may offer other host tool sets).
    Restarted,
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
            HermesError::Ceremony(m) => write!(f, "hermes ceremony bridge error: {m}"),
            HermesError::PortInUse(p) => write!(
                f,
                "hermes control port {p} is already held by another process; refusing to start"
            ),
            HermesError::Stale => write!(
                f,
                "STALE_APPROVAL: the agent's pending action is no longer the one you reviewed (it expired or was replaced), so the agent did not receive this decision"
            ),
            HermesError::BriefRefused(m) => write!(f, "BRIEF_REFUSED: {m}"),
            HermesError::Restarted => write!(
                f,
                "Not started: Hermes restarted after this run's tools were checked; nothing was opened"
            ),
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
    /// HUP-S10.3: `DELETE` (closing a session). Transports that predate it refuse honestly.
    fn delete(&self, url: &str, bearer: &str) -> Result<ControlResp> {
        let _ = (url, bearer);
        Err(HermesError::Transport(
            "this control transport cannot close sessions".into(),
        ))
    }

    /// HUP-S1.5: a POST whose answer may take longer than [`HERMES_CONTROL_TIMEOUT`] (one escalation
    /// to a remote model). Transports without their own deadline handling use `post`.
    fn post_with_timeout(
        &self,
        url: &str,
        bearer: &str,
        body: &str,
        timeout: Duration,
    ) -> Result<ControlResp> {
        let _ = timeout;
        self.post(url, bearer, body)
    }
}

/// Production control transport over blocking `ureq`. A non-2xx is returned as a normal response
/// (`http_status_as_error(false)`) so its status AND body reach the caller, which maps it to a typed
/// error; a transport failure (refused/timeout) maps to [`HermesError::Transport`] and never carries
/// the bearer.
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

/// HUP-S0.1 — deadline for one loopback control call to the Hermes sidecar. The sidecar answers
/// control requests immediately (skills run in the background), so a call that exceeds this is a
/// wedged sidecar: fail the call instead of hanging the command.
pub(crate) const HERMES_CONTROL_TIMEOUT: Duration = Duration::from_secs(30);

/// HUP-S1.5 — deadline for `POST /escalations` (the sidecar allows 120 s for the remote model plus
/// up to 10 s waiting for a slot).
pub(crate) const HERMES_ESCALATION_TIMEOUT: Duration = Duration::from_secs(150);

impl HermesControl for UreqControl {
    fn post_with_timeout(
        &self,
        url: &str,
        bearer: &str,
        body: &str,
        timeout: Duration,
    ) -> Result<ControlResp> {
        match ureq::post(url)
            .config()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
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

    fn get(&self, url: &str, bearer: &str) -> Result<ControlResp> {
        match ureq::get(url)
            .config()
            .timeout_global(Some(HERMES_CONTROL_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .header("Authorization", &format!("Bearer {bearer}"))
            .call()
        {
            Ok(resp) => Self::read(resp),
            // Defensive: with http_status_as_error(false) ureq returns non-2xx as Ok; keep the status if not.
            Err(ureq::Error::StatusCode(code)) => Ok(ControlResp {
                status: code,
                body: String::new(),
            }),
            Err(e) => Err(HermesError::Transport(e.to_string())),
        }
    }

    fn post(&self, url: &str, bearer: &str, body: &str) -> Result<ControlResp> {
        match ureq::post(url)
            .config()
            .timeout_global(Some(HERMES_CONTROL_TIMEOUT))
            .http_status_as_error(false)
            .build()
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

    fn delete(&self, url: &str, bearer: &str) -> Result<ControlResp> {
        match ureq::delete(url)
            .config()
            .timeout_global(Some(HERMES_CONTROL_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .header("Authorization", &format!("Bearer {bearer}"))
            .call()
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
    /// citrate-agent-runtime#87: what the session API supports. Absent on an older sidecar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<SessionCapabilities>,
}

/// `GET /status` → `capabilities` (citrate-agent-runtime#87).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionCapabilities {
    /// The host tool-set names a session's `toolSets` allowlist accepts (core's permission names).
    #[serde(default)]
    pub session_tool_sets: Vec<String>,
    /// `promptSnapshot: false` is honoured (the sidecar adds nothing to core's system prompt).
    #[serde(default)]
    pub session_prompt_snapshot: bool,
}

/// One installed skill (a capsule).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

/// One pending chain/skill effect awaiting human approval. A chain effect carries the raw
/// `to`/`data` (the calldata the ceremony signs); code/shell effects leave them `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingApproval {
    pub id: String,
    pub kind: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
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
    /// The per-session capsule (skill) directory passed to the child as `CITRATE_HERMES_CAPSULES`.
    /// `None` (tests / no resource dir) → the env is not set and the child keeps its default; prod
    /// seeds this from the bundled starter capsules so the agent boots with runnable skills.
    capsules_dir: Option<PathBuf>,
    /// HUP-S4.3: the MCP allowlist file (`hermes_mcp.rs`). Passed as `CITRATE_HERMES_MCP` only while
    /// the file exists, so with no MCP server enabled the child runs no MCP at all.
    mcp_config_path: Option<PathBuf>,
    /// HUP-S2.9: the undo checkpoint store passed to the child as `CITRATE_HERMES_CHECKPOINTS`.
    checkpoints_dir: Option<PathBuf>,
    /// HUP-S3.4: the learn data folder and the member's skills folder, passed to the child as
    /// `CITRATE_HERMES_LEARN_DIR` / `CITRATE_HERMES_LEARN_SKILLS_DIR` (and the skills folder as
    /// `CITRATE_HERMES_SKILLS`, so accepted skills load in later sessions). `None` = learning off.
    learn_dirs: Option<(PathBuf, PathBuf)>,
    /// HUP-S3.2: the other skill sources for the sidecar's one SKILL.md loader (the member's saved
    /// skills, Citrate's bundled skills, the reviewed third-party skills). `None` = learned only.
    skill_sources: Option<crate::hermes_learn::SkillSources>,
    /// HUP-S4.4 + RC52 P-9a: the member's MCP servers (Settings > MCP servers; env values in the
    /// keychain). Their allowlist is written for the child only at spawn (see [`Self::build_spec`]).
    mcp_servers: Option<crate::mcp_servers::McpStore>,
    /// HUP-S7.3/S7.5: the base folder for the sidecar's metering log, decision records and anchor
    /// ledger (see `hermes_chain`). `None` (tests) leaves those env vars unset.
    chain_data_dir: Option<PathBuf>,
    /// HUP-S5.2/S5.3: extra child environment from the member's web opt-ins, computed at each
    /// start. Only keys on `hermes_web::SIDECAR_ENV_KEYS` pass. `None` = nothing extra.
    env_source: Option<crate::hermes_web::EnvSource>,
    /// HUP-S1.2 / US-1.4: the loopback BGE embedding server the sidecar ranks tools and skills
    /// with. Started before the sidecar; its URL and key FILE are passed only while it runs.
    /// `None` (tests, or no bundled model) = sessions rank lexically and report it.
    embed: Option<crate::embed_serve::EmbedServer>,
    health_interval: Duration,
    /// SCL-S0.3: where core records the sidecar's managed browsers (see `owned_browser`). A
    /// core-only folder that is never passed to the sidecar; `None` = next to the crash log.
    browser_record: Option<PathBuf>,
    #[cfg(test)]
    spawn_args_override: Option<Vec<String>>,
    /// The current session bearer (minted on start; wiped on drop). Held for control calls (S6.2+).
    token: Mutex<Option<Zeroizing<String>>>,
    sup: Mutex<Option<Supervisor>>,
    /// K5a: the environment the running child was started with (paths and flags; never a token,
    /// which the child reads from a file). The daemon book reads which host tool sets are on.
    started_env: Mutex<Option<Vec<(String, String)>>>,
    /// The bearer-authed control transport (production ureq; tests inject a mock).
    control: Box<dyn HermesControl>,
    /// S6.3 dedup: a chain effect's content key (hash of to+data) → the ceremony id minted for it.
    /// While an effect is the sidecar head it maps to AT MOST ONE ceremony: `bridge_pending` returns
    /// the SAME ceremony while pending, and once that ceremony is consumed (approved OR rejected) it
    /// returns None rather than minting a second — so an approved effect can never be re-bridged into a
    /// second broadcast (H-1). The entry is cleared only by `resolve_head` (the head has moved) or on
    /// stop, at which point a genuinely new effect may bridge fresh.
    bridged: Mutex<HashMap<String, String>>,
    /// Sidecar call id → the content key it was bridged under, so a resolve that the sidecar
    /// answers 409 (the call is no longer its head, e.g. the submitter timed out) forgets exactly
    /// that effect; otherwise a later identical effect would find the stale decided entry and never
    /// bridge again.
    bridged_by_call: Mutex<HashMap<String, String>>,
    /// HUP-S2.1: agent sessions opened with the member's folder-grant document; each change to the
    /// document is sent to every one of them (`POST /sessions/:id/grants`).
    grant_sessions: Mutex<std::collections::BTreeSet<String>>,
    /// K5a: which sidecar a check was made against. A Store daemon's tool check and its session
    /// open must be about the same running sidecar (see [`Self::session_open_in`]).
    generation: std::sync::atomic::AtomicU64,
    /// Held for writing by [`Self::start`] and [`Self::stop`], for reading by
    /// [`Self::session_open_in`] across its request: a checked open never straddles a restart.
    open_gate: std::sync::RwLock<()>,
}

/// HUP-S2.1: how sending a changed grant document to the open agent sessions went.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantsPushOutcome {
    /// Sessions that now use the new document.
    pub updated: usize,
    /// One line per session that refused it or could not be reached.
    pub failed: Vec<String>,
}

impl HermesManager {
    /// This service's supervisor slot, for the services registry's read-only snapshot (K1a-2).
    pub(crate) fn supervisor_slot(&self) -> &Mutex<Option<Supervisor>> {
        &self.sup
    }

    /// Build a manager over an explicit binary + token file + crash path (default control addr).
    pub fn new(bin: PathBuf, token_path: PathBuf, crash_record_path: PathBuf) -> Self {
        HermesManager {
            bin,
            control_addr: HERMES_CONTROL_ADDR.to_string(),
            token_path,
            crash_record_path,
            capsules_dir: None,
            mcp_config_path: None,
            checkpoints_dir: None,
            learn_dirs: None,
            skill_sources: None,
            mcp_servers: None,
            chain_data_dir: None,
            env_source: None,
            embed: None,
            health_interval: HEALTH_INTERVAL,
            browser_record: None,
            #[cfg(test)]
            spawn_args_override: None,
            token: Mutex::new(None),
            sup: Mutex::new(None),
            started_env: Mutex::new(None),
            control: Box::new(UreqControl),
            bridged: Mutex::new(HashMap::new()),
            bridged_by_call: Mutex::new(HashMap::new()),
            grant_sessions: Mutex::new(std::collections::BTreeSet::new()),
            generation: std::sync::atomic::AtomicU64::new(0),
            open_gate: std::sync::RwLock::new(()),
        }
    }

    /// SCL-S0.3: keep the managed-browser record at `path` (a core-only folder the sidecar is never
    /// told about) instead of next to the crash log.
    pub fn with_browser_record(mut self, path: PathBuf) -> Self {
        self.browser_record = Some(path);
        self
    }

    /// SCL-S0.3: the managed-browser record file core keeps for this sidecar.
    fn browser_record_path(&self) -> PathBuf {
        self.browser_record.clone().unwrap_or_else(|| {
            self.crash_record_path
                .with_file_name(crate::owned_browser::RECORD_FILE)
        })
    }

    /// Point the child at a capsule (skill) directory (`CITRATE_HERMES_CAPSULES`). Prod calls this
    /// with the seeded per-session dir so the agent starts with runnable skills instead of an empty
    /// catalog. Absent → the env is not set (unchanged default behavior).
    pub fn with_capsules_dir(mut self, dir: PathBuf) -> Self {
        self.capsules_dir = Some(dir);
        self
    }

    /// HUP-S4.3: name the MCP allowlist file. The env is set at spawn only if the file exists then.
    pub fn with_mcp_config_path(mut self, path: PathBuf) -> Self {
        self.mcp_config_path = Some(path);
        self
    }

    /// HUP-S3.4: turn on verified self-learning in the child (see `learn_dirs`).
    pub fn with_learn_dirs(mut self, learn_dir: PathBuf, skills_dir: PathBuf) -> Self {
        self.learn_dirs = Some((learn_dir, skills_dir));
        self
    }

    /// HUP-S3.2: offer these skill sources to the child (see `skill_sources`).
    pub fn with_skill_sources(mut self, sources: crate::hermes_learn::SkillSources) -> Self {
        self.skill_sources = Some(sources);
        self
    }

    /// HUP-S3.4: a bearer-authed `GET` on the control plane, for `hermes_learn`. `path` starts with
    /// `/` and is built from validated ids only.
    pub(crate) fn control_get(&self, path: &str) -> Result<ControlResp> {
        let bearer = self.bearer()?;
        self.control
            .get(&format!("{}{path}", self.control_url()), &bearer)
    }

    /// HUP-S3.4: a bearer-authed `POST` on the control plane, for `hermes_learn`.
    pub(crate) fn control_post(&self, path: &str, body: &str) -> Result<ControlResp> {
        let bearer = self.bearer()?;
        self.control
            .post(&format!("{}{path}", self.control_url()), &bearer, body)
    }

    /// HUP-S4.4: the member's MCP servers (joined with the built-in ones at spawn, see
    /// [`crate::hermes_mcp::effective_allowlist`]).
    pub fn with_mcp_servers(mut self, store: crate::mcp_servers::McpStore) -> Self {
        self.mcp_servers = Some(store);
        self
    }

    /// The embedding server this sidecar starts, if it has one (the services registry, K1a-2).
    pub(crate) fn embed_server(&self) -> Option<&crate::embed_serve::EmbedServer> {
        self.embed.as_ref()
    }

    /// HUP-S1.2 / US-1.4: start this embedding server with the sidecar and pass its URL and key
    /// file (`CITRATE_HERMES_EMBED_URL`, `CITRATE_HERMES_EMBED_KEY_FILE`) while it runs.
    pub fn with_embed(mut self, embed: crate::embed_serve::EmbedServer) -> Self {
        self.embed = Some(embed);
        self
    }

    /// HUP-S5.2/S5.3: add the member's web opt-ins to the child environment at each start.
    pub fn with_env_source(mut self, src: crate::hermes_web::EnvSource) -> Self {
        self.env_source = Some(src);
        self
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

    /// Test hook: bind the control surface on another loopback address.
    #[cfg(test)]
    pub fn with_control_addr(mut self, addr: &str) -> Self {
        self.control_addr = addr.to_string();
        self
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
        // Point the child at the seeded capsule dir so it boots with skills, not an empty catalog.
        if let Some(dir) = &self.capsules_dir {
            spec.env.push((
                HERMES_CAPSULES_ENV.to_string(),
                dir.to_string_lossy().to_string(),
            ));
        }
        // HUP-S4.3 + S4.4: hand the child one MCP allowlist (built-in servers and the member's
        // reviewed servers, joined when both are on), and only when there is one (default: none).
        // RC52 P-9a: with member servers it is a spawn-time file holding their env values from the
        // keychain; the child watch below removes it once the child has loaded it.
        let effective = crate::hermes_mcp::effective_allowlist(
            self.mcp_config_path.as_deref(),
            self.mcp_servers.as_ref(),
        );
        if let Some(path) = &effective {
            spec.env.push((
                crate::hermes_mcp::MCP_CONFIG_ENV.to_string(),
                path.to_string_lossy().to_string(),
            ));
        }
        // HUP-S4.4: the probe file, so the sidecar's dry-run probe starts only the entry this app
        // is checking. It exists only during a check; otherwise the probe refuses.
        if let Some(store) = &self.mcp_servers {
            spec.env.push((
                crate::mcp_servers::MCP_REGISTRY_ENV.to_string(),
                store.probe_path().to_string_lossy().to_string(),
            ));
        }
        if let Some(dir) = &self.checkpoints_dir {
            spec.env.push((
                undo::HERMES_CHECKPOINTS_ENV.to_string(),
                dir.to_string_lossy().to_string(),
            ));
        }
        if let Some((learn, skills)) = &self.learn_dirs {
            spec.env
                .extend(crate::hermes_learn::learn_env(learn, skills));
        }
        if let Some(sources) = &self.skill_sources {
            // HUP-S3.2: one CITRATE_HERMES_SKILLS listing every source, replacing the learned-only one.
            let learned = self.learn_dirs.as_ref().map(|(_, s)| s.as_path());
            let extra = crate::hermes_learn::skills_env(learned, sources);
            spec.env.retain(|(k, _)| !extra.iter().any(|(e, _)| e == k));
            spec.env.extend(extra);
        }
        if let Some(base) = &self.chain_data_dir {
            for (k, dir) in chain::data_dirs(base) {
                spec.env
                    .push((k.to_string(), dir.to_string_lossy().to_string()));
            }
        }
        // HUP-S1.2 / US-1.4: embedding retrieval, only while the embedding server runs (otherwise
        // the sidecar tries its own chat server and reports lexical ranking with the reason).
        if let Some(embed) = self.embed.as_ref().filter(|e| e.is_started()) {
            spec.env.extend(embed.sidecar_env());
        }
        if let Some(src) = &self.env_source {
            spec.env.extend(src().into_iter().filter(|(k, _)| {
                crate::hermes_web::SIDECAR_ENV_KEYS.contains(&k.as_str())
                    || crate::forge_toolchain::SIDECAR_ENV_KEYS.contains(&k.as_str())
                    || crate::fl_trajectories::SIDECAR_ENV_KEYS.contains(&k.as_str())
                    || crate::hermes_terminal::SIDECAR_ENV_KEYS.contains(&k.as_str())
            }));
        }
        let health_url = format!("http://{}/health", self.control_addr);
        spec.health_check = Some(HealthCheck {
            interval: self.health_interval,
            grace: HERMES_START_GRACE,
            probe: std::sync::Arc::new(move || http_health_ok(&health_url)),
        });
        // SCL-S0.3: record the sidecar's managed browsers while it runs, clean them up after it
        // ends (including core's hard kill after the stop grace).
        let browser = crate::owned_browser::watch(self.browser_record_path());
        // RC52 P-9a: a spawn-time allowlist with member env values lives only until the child has
        // loaded it, and is written again for a supervisor restart.
        spec.child_watch = match (effective, &self.mcp_servers) {
            (Some(path), Some(store)) if crate::hermes_mcp::is_spawn_file(&path, store) => {
                let health_url = format!("http://{}/health", self.control_addr);
                Some(crate::hermes_mcp::spawn_file_watch(
                    browser,
                    path,
                    self.mcp_config_path.clone(),
                    store.clone(),
                    std::sync::Arc::new(move || http_health_ok(&health_url)),
                ))
            }
            _ => browser,
        };
        spec
    }

    /// Test hook: the embedding server, to start it in the wiring proof.
    #[cfg(test)]
    pub fn embed_for_test(&self) -> Option<&crate::embed_serve::EmbedServer> {
        self.embed.as_ref()
    }

    /// Test hook: the sidecar child's pid while the supervisor holds one (SCL-S0.5 coverage).
    #[cfg(test)]
    pub fn sidecar_pid_for_test(&self) -> Option<u32> {
        self.sup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|s| s.status().pid)
    }

    /// Test hook: expose the spec env for the wiring proof (addr + token-file path, never a token).
    #[cfg(test)]
    pub fn spec_env_for_test(&self) -> Vec<(String, String)> {
        self.build_spec().env
    }

    /// Test hook: the whole spec (for the child-watch hooks).
    #[cfg(test)]
    pub fn spec_for_test(&self) -> SidecarSpec {
        self.build_spec()
    }

    /// RC52 P-9a: remove the files that hold member MCP env values only around a spawn or a probe.
    fn sweep_mcp_runtime_files(&self) {
        if let Some(store) = &self.mcp_servers {
            crate::mcp_servers::sweep_runtime_files(store.dir());
        }
    }

    /// Start the sidecar: mint a fresh session bearer, persist it `0600` (the child adopts it), and
    /// spawn under the supervisor. Fails CLOSED if the token can't be minted or the binary is
    /// missing; idempotent (`AlreadyRunning`).
    pub fn start(&self) -> Result<()> {
        // A start is a new generation: no checked open is in flight across it.
        let _gate = self.open_gate.write().unwrap_or_else(|e| e.into_inner());
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(HermesError::AlreadyRunning);
        }
        if !self.bin.exists() {
            return Err(HermesError::BinaryNotFound(self.bin.display().to_string()));
        }
        // SCL-S0.3: a managed browser an earlier sidecar left behind (it ended while this app was
        // not running, or this app was killed) is stopped and its profile removed first.
        crate::owned_browser::apply(&self.browser_record_path());
        // PBA-L7b-009: every control call carries the session bearer to the fixed loopback port. If
        // another process already holds it, refuse to start instead of handing it the bearer.
        if let Some(port) = self
            .control_addr
            .rsplit_once(':')
            .and_then(|(_, p)| p.parse::<u16>().ok())
        {
            if !crate::serve::loopback_port_is_free(port) {
                return Err(HermesError::PortInUse(port));
            }
        }
        // HUP-S1.2 / US-1.4: best effort. Without it the sessions rank lexically and say why.
        if let Some(embed) = &self.embed {
            if let Err(e) = embed.ensure_started() {
                eprintln!("hermes: embedding retrieval stays off: {e}");
            }
        }
        let token = mint_bearer();
        let mut env_used: Option<Vec<(String, String)>> = None;
        let started = persist_bearer(&self.token_path, &token).and_then(|()| {
            let spec = self.build_spec();
            // What the child actually sees: the app's own environment (inherited by the spawn)
            // overlaid with core's values, so a CITRATE_HERMES_* flag in the launch environment is
            // checked too.
            let inherited = std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)));
            env_used = Some(effective_child_env(inherited, &spec.env));
            let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
            config.backoff = BackoffPolicy::new();
            config.healthy_after = HERMES_HEALTHY_AFTER;
            Supervisor::start(config).map_err(|e| HermesError::Spawn(e.to_string()))
        });
        let sup = match started {
            Ok(sup) => sup,
            Err(e) => {
                self.sweep_mcp_runtime_files();
                // SCL-S0.5: the embedding server lives and dies with Hermes. A Hermes that did not
                // start must not leave it running until the app quits.
                if let Some(embed) = &self.embed {
                    embed.stop();
                }
                return Err(e);
            }
        };
        *guard = Some(sup);
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = Some(token);
        *self.started_env.lock().unwrap_or_else(|e| e.into_inner()) = env_used;
        Ok(())
    }

    /// Stop the sidecar (SIGTERM → grace → SIGKILL, no orphan) and drop the session bearer. Idempotent.
    pub fn stop(&self) {
        // Waits for a checked open in flight; any check made before this stop is now stale.
        let _gate = self.open_gate.write().unwrap_or_else(|e| e.into_inner());
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        *self.started_env.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if let Some(sup) = sup {
            sup.stop();
            drop(sup);
        }
        // After the stop (its last child-watch call may have rewritten the spawn-time allowlist).
        self.sweep_mcp_runtime_files();
        if let Some(embed) = &self.embed {
            embed.stop();
        }
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
        // A new session starts with a clean dedup map.
        self.bridged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.bridged_by_call
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.grant_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
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

    /// Whether the sidecar was started and not stopped (running, starting or restarting).
    pub fn is_started(&self) -> bool {
        self.sup.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// [`Self::start`] right after [`Self::stop`]: the old child's control port can take a moment
    /// to be released, so a port still held is retried for up to [`RESTART_PORT_WAIT`].
    pub fn start_after_restart(&self) -> Result<()> {
        let deadline = std::time::Instant::now() + RESTART_PORT_WAIT;
        loop {
            match self.start() {
                Err(HermesError::PortInUse(_)) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                other => return other,
            }
        }
    }

    /// K5a: the environment the running sidecar was started with, or `None` while it is not
    /// running. Read for which sidecar-hosted tool sets every session is offered.
    pub fn started_env(&self) -> Option<Vec<(String, String)>> {
        if !self.is_running() {
            return None;
        }
        self.started_env
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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

    /// HUP-S5.1: a raw bearer-authed GET of `path` (e.g. `/browser/status`). The caller maps the
    /// status (see `browser.rs`).
    pub(crate) fn control_get_path(&self, path: &str) -> Result<ControlResp> {
        let bearer = self.bearer()?;
        self.control
            .get(&format!("{}{path}", self.control_url()), &bearer)
    }

    /// HUP-S5.1: a raw bearer-authed POST of `body` to `path`.
    pub(crate) fn control_post_path(&self, path: &str, body: &str) -> Result<ControlResp> {
        let bearer = self.bearer()?;
        self.control
            .post(&format!("{}{path}", self.control_url()), &bearer, body)
    }

    /// K5a + citrate-agent-runtime#87: the host tool sets this sidecar can scope per session, read
    /// fresh from `GET /status`. `None` while not running, on any error, for an older sidecar
    /// without `capabilities`, or when it cannot honour `promptSnapshot: false` (both are needed
    /// to open a Store daemon's session exactly as core sends it).
    pub fn session_tool_sets(&self) -> Option<Vec<String>> {
        if !self.is_running() {
            return None;
        }
        scoping_tool_sets(&self.remote_status().ok()?)
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

    // --- HUP-S4.4 user-added MCP servers ----------------------------------------------------------

    /// `POST /mcp/probe` — the sidecar's dry-run check of one server entry (the runtime's
    /// `[[servers]]` shape). 422 = the sidecar's field errors; 200 = the probe report (which may
    /// itself say the server could not be reached). Registers nothing.
    /// `entry` is the entry's JSON with its env values ([`crate::mcp_servers::runtime_entry_json`]).
    pub fn mcp_probe(&self, entry: &str) -> Result<crate::mcp_servers::ProbeOutcome> {
        let bearer = self.bearer()?;
        let resp =
            self.control
                .post(&format!("{}/mcp/probe", self.control_url()), &bearer, entry)?;
        if resp.status == 422 {
            #[derive(Deserialize)]
            struct Invalid {
                #[serde(default)]
                errors: Vec<crate::mcp_servers::FieldError>,
            }
            let inv: Invalid =
                serde_json::from_str(&resp.body).map_err(|e| HermesError::Decode(e.to_string()))?;
            return Ok(crate::mcp_servers::ProbeOutcome::Invalid(inv.errors));
        }
        Ok(crate::mcp_servers::ProbeOutcome::Report(Self::decode(
            resp,
        )?))
    }

    /// `GET /mcp/servers` — the servers the running sidecar loaded (never URLs or env).
    pub fn mcp_servers_status(&self) -> Result<serde_json::Value> {
        let bearer = self.bearer()?;
        let resp = self
            .control
            .get(&format!("{}/mcp/servers", self.control_url()), &bearer)?;
        Self::decode(resp)
    }

    // --- HUP-S1.1c agent sessions (ADR loop-in-sidecar) -----------------------------------------

    fn check(resp: ControlResp) -> Result<ControlResp> {
        if !(200..300).contains(&resp.status) {
            return Err(HermesError::Control {
                status: resp.status,
                msg: resp.body.chars().take(200).collect(),
            });
        }
        Ok(resp)
    }

    /// K5a: the running sidecar's generation. Read it BEFORE the facts a check uses (its
    /// environment), then open with [`Self::session_open_in`].
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// [`Self::session_open`] into the sidecar generation `generation` was read from.
    /// Refused (nothing is sent) when Hermes stopped or started since `generation` was read; a
    /// stop or start waits until this open has returned.
    pub fn session_open_in(&self, generation: u64, body: &str) -> Result<String> {
        self.in_generation(generation, |m| m.session_open(body))
    }

    /// Run `f` (a session open) in sidecar `generation`: refused when Hermes stopped or started
    /// since it was read, and no stop or start runs until `f` has returned.
    pub fn in_generation<T>(
        &self,
        generation: u64,
        f: impl FnOnce(&Self) -> Result<T>,
    ) -> Result<T> {
        let _gate = self.open_gate.read().unwrap_or_else(|e| e.into_inner());
        if self.generation() != generation {
            return Err(HermesError::Restarted);
        }
        f(self)
    }

    /// `POST /sessions` with a body from [`build_session_body`]; returns the session id.
    pub fn session_open(&self, body: &str) -> Result<String> {
        let bearer = self.bearer()?;
        let resp = Self::check(self.control.post(
            &format!("{}/sessions", self.control_url()),
            &bearer,
            body,
        )?)?;
        let v: serde_json::Value =
            serde_json::from_str(&resp.body).map_err(|e| HermesError::Decode(e.to_string()))?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        valid_session_id(&id).map_err(HermesError::Decode)?;
        let with_grants = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .is_some_and(|b| b.get("grants").is_some());
        if with_grants {
            self.grant_sessions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.clone());
        }
        Ok(id)
    }

    /// K5a + citrate-agent-runtime#87: open a STORE daemon's session and verify the sidecar applied
    /// the scoping. The 201 response must echo `toolSets` equal (as a set) to `sent_sets` and
    /// `promptSnapshot: false`. A sidecar that ignored either (an older one, or a dropped field)
    /// fails open, so its session is closed and the run refused.
    pub fn session_open_store(&self, body: &str, sent_sets: &[String]) -> Result<String> {
        let bearer = self.bearer()?;
        let resp = Self::check(self.control.post(
            &format!("{}/sessions", self.control_url()),
            &bearer,
            body,
        )?)?;
        let v: serde_json::Value =
            serde_json::from_str(&resp.body).map_err(|e| HermesError::Decode(e.to_string()))?;
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        valid_session_id(&id).map_err(HermesError::Decode)?;
        if let Err(why) = verify_store_session_echo(&v, sent_sets) {
            let _ = self.session_close(&id);
            return Err(HermesError::Decode(why));
        }
        Ok(id)
    }

    /// HUP-S2.1: send `doc` (the whole grant document) to every session opened with grants. A
    /// session the sidecar no longer has (404) is forgotten; any other refusal is reported. With no
    /// sidecar running there is nothing to send to.
    pub fn push_grants(&self, doc: &crate::agent_grants::GrantState) -> GrantsPushOutcome {
        let mut out = GrantsPushOutcome::default();
        let ids: Vec<String> = self
            .grant_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect();
        if ids.is_empty() {
            return out;
        }
        let Ok(bearer) = self.bearer() else {
            self.grant_sessions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            return out;
        };
        let body = match serde_json::to_string(doc) {
            Ok(b) => b,
            Err(e) => {
                out.failed.push(format!("could not encode the grants: {e}"));
                return out;
            }
        };
        for id in ids {
            let url = format!("{}/sessions/{id}/grants", self.control_url());
            match self.control.post(&url, &bearer, &body) {
                Ok(r) if (200..300).contains(&r.status) => out.updated += 1,
                Ok(r) if r.status == 404 => {
                    self.grant_sessions
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&id);
                }
                Ok(r) => {
                    let msg: String = serde_json::from_str::<serde_json::Value>(&r.body)
                        .ok()
                        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                        .unwrap_or_else(|| format!("status {}", r.status))
                        .chars()
                        .take(200)
                        .collect();
                    out.failed.push(format!(
                        "a Hermes conversation refused the new grants ({msg}), so it has no folder access now"
                    ));
                }
                Err(e) => out
                    .failed
                    .push(format!("a Hermes conversation could not be reached ({e})")),
            }
        }
        out
    }

    /// HUP-S1.5 — `POST /escalations` with a body from `escalation::sidecar_body` (it carries the
    /// endpoint key for this one request; it is never logged). Returns the raw status + body: the
    /// caller reads the sidecar's `sent` flag to settle the spend reservation.
    pub fn escalate(&self, body: &str) -> Result<ControlResp> {
        let bearer = self.bearer()?;
        self.control.post_with_timeout(
            &format!("{}/escalations", self.control_url()),
            &bearer,
            body,
            HERMES_ESCALATION_TIMEOUT,
        )
    }

    /// `POST /sessions/:id/messages` — start one turn (the sidecar answers 409 while busy).
    pub fn session_send(&self, id: &str, text: &str) -> Result<()> {
        valid_session_id(id).map_err(|m| HermesError::Control {
            status: 400,
            msg: m,
        })?;
        let bearer = self.bearer()?;
        let body = serde_json::json!({ "text": text }).to_string();
        Self::check(self.control.post(
            &format!("{}/sessions/{id}/messages", self.control_url()),
            &bearer,
            &body,
        )?)?;
        Ok(())
    }

    /// `GET /sessions/:id/events?after=N&wait_ms=M` — long-poll; `wait_ms` is capped below the
    /// control-call deadline so the poll itself can never time out.
    pub fn session_events(&self, id: &str, after: u64, wait_ms: u64) -> Result<serde_json::Value> {
        valid_session_id(id).map_err(|m| HermesError::Control {
            status: 400,
            msg: m,
        })?;
        let bearer = self.bearer()?;
        let cap = HERMES_CONTROL_TIMEOUT.as_millis() as u64 * 2 / 3;
        let wait = wait_ms.min(cap);
        let resp = self.control.get(
            &format!(
                "{}/sessions/{id}/events?after={after}&wait_ms={wait}",
                self.control_url()
            ),
            &bearer,
        )?;
        Self::decode(resp)
    }

    /// `POST /sessions/:id/tool_results` — hand back a core-hosted tool's gated result.
    pub fn session_tool_result(
        &self,
        id: &str,
        call_id: &str,
        status: &str,
        content: &str,
    ) -> Result<()> {
        valid_session_id(id).map_err(|m| HermesError::Control {
            status: 400,
            msg: m,
        })?;
        if !matches!(status, "ok" | "denied" | "error") {
            return Err(HermesError::Control {
                status: 400,
                msg: format!("unknown tool status {status:?}"),
            });
        }
        let bearer = self.bearer()?;
        let body = serde_json::json!({ "callId": call_id, "status": status, "content": content })
            .to_string();
        Self::check(self.control.post(
            &format!("{}/sessions/{id}/tool_results", self.control_url()),
            &bearer,
            &body,
        )?)?;
        Ok(())
    }

    /// `POST /sessions/:id/stop`.
    pub fn session_stop(&self, id: &str) -> Result<()> {
        valid_session_id(id).map_err(|m| HermesError::Control {
            status: 400,
            msg: m,
        })?;
        let bearer = self.bearer()?;
        Self::check(self.control.post(
            &format!("{}/sessions/{id}/stop", self.control_url()),
            &bearer,
            "{}",
        )?)?;
        Ok(())
    }

    /// HUP-S10.3: `DELETE /sessions/:id` — close a session (daemon runs close theirs when done).
    pub fn session_close(&self, id: &str) -> Result<()> {
        valid_session_id(id).map_err(|m| HermesError::Control {
            status: 400,
            msg: m,
        })?;
        let bearer = self.bearer()?;
        Self::check(
            self.control
                .delete(&format!("{}/sessions/{id}", self.control_url()), &bearer)?,
        )?;
        Ok(())
    }

    // --- HUP-S1.4 interviewer: tracks + briefs (runtime agent-loop::interview) -------------------

    /// Map a brief-route response: 2xx → decoded body; 422 → [`HermesError::BriefRefused`] with the
    /// sidecar's `{error}` reason; anything else → the usual typed `Control` error.
    fn decode_brief<T: serde::de::DeserializeOwned>(resp: ControlResp) -> Result<T> {
        if resp.status == 422 {
            let reason = serde_json::from_str::<serde_json::Value>(&resp.body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| "the sidecar refused this brief".to_string());
            return Err(HermesError::BriefRefused(
                reason.chars().take(300).collect(),
            ));
        }
        Self::decode(resp)
    }

    /// `GET /tracks` — the bundled interview tracks (question sets, persona, skills, workflow, gates).
    pub fn tracks(&self) -> Result<Vec<Track>> {
        let bearer = self.bearer()?;
        let resp = self
            .control
            .get(&format!("{}/tracks", self.control_url()), &bearer)?;
        Self::decode(resp)
    }

    /// `POST /briefs` — answers → brief. Unanswered questions take their defaults (an empty map is
    /// "just use defaults"); with no `track` the sidecar suggests one from the goal (422 if none fits).
    pub fn brief_create(
        &self,
        track: Option<&str>,
        goal: &str,
        answers: &BTreeMap<String, String>,
    ) -> Result<BriefDraft> {
        let bad = |msg: String| HermesError::Control { status: 400, msg };
        if let Some(t) = track {
            valid_slug(t, "track id").map_err(bad)?;
        }
        valid_goal(goal).map_err(bad)?;
        if answers.len() > MAX_BRIEF_ANSWERS {
            return Err(bad(format!("at most {MAX_BRIEF_ANSWERS} answers")));
        }
        for (id, a) in answers {
            valid_slug(id, "question id").map_err(bad)?;
            valid_len(a, MAX_BRIEF_ANSWER_CHARS, "an answer").map_err(bad)?;
        }
        let bearer = self.bearer()?;
        let mut body = serde_json::json!({ "goal": goal, "answers": answers });
        if let Some(t) = track {
            body["track"] = serde_json::Value::String(t.to_string());
        }
        let resp = self.control.post(
            &format!("{}/briefs", self.control_url()),
            &bearer,
            &body.to_string(),
        )?;
        Self::decode_brief(resp)
    }

    /// `POST /briefs/check` — validate a member-edited brief against its track. The sidecar owns
    /// the rules (gates and workflow can't be edited away); core only bounds the sizes first.
    pub fn brief_check(&self, brief: &Brief) -> Result<BriefChecked> {
        validate_brief_shape(brief).map_err(|msg| HermesError::Control { status: 400, msg })?;
        let bearer = self.bearer()?;
        let body = serde_json::json!({ "brief": brief }).to_string();
        let resp = self.control.post(
            &format!("{}/briefs/check", self.control_url()),
            &bearer,
            &body,
        )?;
        Self::decode_brief(resp)
    }

    // --- S6.3 ceremony bridge -------------------------------------------------------------------

    /// Bridge the sidecar's HEAD pending chain effect into a PENDING ceremony (Rule 3): build the
    /// `SignatureIntent{origin:"agent:hermes"}` for its raw (to, data) and `request` it into the
    /// SignatureCeremony. This SIGNS NOTHING and touches no key — it only creates a ceremony the
    /// HUMAN must approve; the ceremony's own approve path (`sign_and_broadcast`) does the single
    /// signature + broadcast. The agent can never obtain a signature from here.
    ///
    /// Idempotent (dedup): the effect's content key (hash of to+data) maps to at most one ceremony
    /// while it is the head — re-polling returns the SAME pending ceremony, and once decided returns
    /// None (no second mint, no second broadcast) until `resolve_head` advances the head. Returns
    /// `None` when there is nothing pending or the head is a non-chain (code/shell) effect.
    ///
    /// `vault` supplies `from` (the real signer's PUBLIC address, never the key); `rpc` estimates the
    /// call's gas (never a fabricated number — Rule 1). A gas blip omits gas and the ceremony refuses
    /// to finalize rather than signing a guessed gas (fail closed).
    pub fn bridge_pending<T: crate::rpc::RpcTransport>(
        &self,
        ceremony: &SignatureCeremony,
        vault: &CustodyVault,
        rpc: &crate::rpc::RpcClient<T>,
        expected_id: &str,
    ) -> Result<Option<CeremonyView>> {
        let Some(head) = self.pending_approvals()?.into_iter().next() else {
            return Ok(None);
        };
        // PBA-L7b-003: only bridge the item the member is reviewing. If the head changed (timeout
        // eviction / a newer effect), refuse — never mint a signing ceremony for an unseen effect.
        if expected_id.is_empty() || head.id != expected_id {
            return Err(HermesError::Stale);
        }
        // Only chain effects (carrying to+data) bridge to a signing ceremony; code/shell effects have
        // their own HIC-1 control decision (S6.4), not a chain signature.
        let (Some(to), Some(data)) = (head.to.clone(), head.data.clone()) else {
            return Ok(None);
        };
        let key = content_key(&to, &data);

        // FAST PATH: this effect already has a bridged ceremony.
        //   - still pending → reuse it (no mint, no RPC).
        //   - CONSUMED (approved OR rejected) → the human already DECIDED this exact effect, but the
        //     sidecar head is still blocked on it (only `resolve_head` advances it). We must NOT mint
        //     a second ceremony for it: an approve→re-poll window would otherwise mint C2 for the same
        //     (to, data), and approving C2 signs a REAL second tx (fresh nonce) = double-broadcast.
        //     Return None ("decided, awaiting resolve"); the entry is cleared only by `resolve_head`,
        //     when the sidecar head actually moves off this effect. THIS is what makes "at most one
        //     broadcast per effect" a structural property, not a UI-ordering hope (fixes H-1).
        //   - EXPIRED unapproved → it signed nothing, so a fresh ceremony cannot double-broadcast:
        //     drop the entry and mint a fresh request below.
        {
            let mut map = self.bridged.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = map.get(&key) {
                if ceremony.is_expired(existing) {
                    map.remove(&key);
                } else {
                    return Ok(ceremony.status(existing)); // Some(pending view) or None (decided → no re-mint)
                }
            }
        }

        // The real signer is THIS vault's wallet — stamp its address as `from` (public identity only).
        let from = crate::wallet::address_auto_unlocked(vault)
            .map(|w| w.address)
            .map_err(|e| HermesError::Ceremony(e.to_string()))?;
        // Real gas estimate (computed off the dedup lock; a concurrent winner makes it moot, discarded).
        let gas = rpc.estimate_gas(gas_call(&to, &data)).ok();

        // ATOMIC check-and-mint under a single hold of the dedup lock: two concurrent bridges of the
        // same effect cannot each mint a ceremony (the loser re-checks and reuses the winner's), and a
        // consumed entry is NOT re-minted (H-1). We only mint when there is NO entry for this effect.
        // `ceremony.request`/`status` lock only the ceremony's own mutex (disjoint), so no deadlock.
        let mut map = self.bridged.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = map.get(&key) {
            if !ceremony.is_expired(existing) {
                return Ok(ceremony.status(existing)); // reuse pending, or None if decided (never re-mint)
            }
            map.remove(&key);
        }
        let view = ceremony.request_watched(hermes_intent(&to, &data, &from, gas));
        map.insert(key.clone(), view.id.clone());
        self.bridged_by_call
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(head.id.clone(), key);
        Ok(Some(view))
    }

    /// Resolve the sidecar's HEAD approval after the human decided its ceremony: `approve` → the
    /// capsule proceeds (the ceremony already signed + broadcast); otherwise → the capsule aborts.
    /// PBA-L7b-003: the POST body is `{"id": <reviewed call id>}` so the sidecar (AR-B-023) resolves
    /// ONLY that call and answers 409 if its head is a different one → [`HermesError::Stale`].
    /// The sidecar's queue is head-resolved and the head is BLOCKED until this call, so it targets the
    /// same effect that was bridged.
    ///
    /// On success this CLEARS the dedup map: the sidecar head now advances off the just-resolved
    /// effect, so a genuinely new effect (even one with the same `(to, data)`) is allowed to bridge
    /// fresh. Because the head is blocked until here, at most one effect is ever bridged at a time, so
    /// clearing the whole map is exactly "forget the resolved effect" (H-1: an approved effect is NOT
    /// re-bridgeable until its head is resolved here).
    pub fn resolve_head(&self, approve: bool, id: &str) -> Result<()> {
        // PBA-L7b-003: an empty id would select the sidecar's legacy "resolve whatever is at the
        // head" path (AR-B-023) — refuse it; every resolve is bound to the reviewed call id.
        if id.is_empty() {
            return Err(HermesError::Stale);
        }
        let bearer = self.bearer()?;
        let path = if approve {
            "/approvals/approve"
        } else {
            "/approvals/reject"
        };
        let body = serde_json::json!({ "id": id }).to_string();
        let resp = self
            .control
            .post(&format!("{}{}", self.control_url(), path), &bearer, &body)?;
        // 409 = the sidecar no longer has this call at its head (it expired or was replaced), so the
        // decision was not delivered. A chain effect's ceremony may ALREADY have signed and
        // broadcast; the UI says so. Forget this call's dedup entry either way: the sidecar has moved
        // off it, and a stale entry would stop a later identical effect from ever bridging.
        if resp.status == 409 {
            self.forget_call(id);
            return Err(HermesError::Stale);
        }
        if !(200..300).contains(&resp.status) {
            return Err(HermesError::Control {
                status: resp.status,
                msg: resp.body.chars().take(200).collect(),
            });
        }
        // The head moved off the resolved effect; forget it so the NEXT head can bridge fresh.
        self.bridged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.bridged_by_call
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        Ok(())
    }

    /// Drop the dedup entry recorded for sidecar call `id` (if it was bridged).
    fn forget_call(&self, id: &str) {
        let key = self
            .bridged_by_call
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(key) = key {
            self.bridged
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
        }
    }
}

/// The dedup content key for a chain effect: a hex SHA3 of `to|data`, so identical calldata collides
/// on one ceremony (deny a double-broadcast from rapid re-polling).
///
/// H-2: this covers `to` + `data` only. `hermes_intent` currently stamps a CONSTANT `value` ("0x0")
/// and `chain_id` (40204), so two effects with the same `to|data` are genuinely the same action and
/// collapsing them is correct. If Hermes ever emits VALUE-bearing effects or targets multiple chains,
/// `value` and `chain_id` MUST be folded into this key (else two different-value calls to the same
/// `to|data` would alias onto one ceremony). Keep this in lockstep with `hermes_intent`.
fn content_key(to: &str, data: &str) -> String {
    use sha3::{Digest, Keccak256};
    let mut h = Keccak256::new();
    h.update(to.as_bytes());
    h.update(b"|");
    h.update(data.as_bytes());
    hex::encode(&h.finalize()[..16])
}

/// The `eth_estimateGas` call object for a chain effect: `{to, value:0x0, data}`.
fn gas_call(to: &str, data: &str) -> serde_json::Value {
    serde_json::json!({ "to": to, "value": "0x0", "data": data })
}

/// Build the ceremony intent for a Hermes chain effect: origin `agent:hermes`, an
/// [`IntentKind::Transaction`] whose `raw` is the `{from, to, value, data, chainId, gas}` tx JSON the
/// ceremony's B1.4 decoder consumes (so the human sees the real action + it signs a REAL tx). No key.
fn hermes_intent(to: &str, data: &str, from: &str, gas: Option<u64>) -> SignatureIntent {
    let mut obj = serde_json::json!({
        "from": from,
        "to": to,
        "value": "0x0",
        "data": data,
        "chainId": format!("0x{CITRATE_CHAIN_ID:x}"),
    });
    if let Some(g) = gas {
        obj["gas"] = serde_json::json!(format!("0x{g:x}"));
    }
    SignatureIntent {
        origin: HERMES_ORIGIN.to_string(),
        kind: IntentKind::Transaction,
        chain_id: CITRATE_CHAIN_ID,
        raw: obj.to_string(),
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

/// Seed the per-session capsule dir from the bundled starter capsules. HUP-S2.5: only a capsule whose
/// `.cps` matches the digest pinned in this build is installed (`capsule_pins`), a seeded copy that no
/// longer matches is replaced by the verified one, and the member's own capsules are never touched. The
/// sidecar then verifies each `.cps` signature and content hash again at load. Best-effort: a refused
/// or failed copy is logged with the reason, never fatal; the agent still starts, honestly reporting
/// however many skills it actually has (Rule 1). Returns the number of skill dirs present in `dest`
/// afterwards (0 is a legitimate, honest outcome).
fn seed_starter_capsules(bundled: &Path, dest: &Path) -> usize {
    let report = crate::capsule_pins::seed_verified(bundled, dest);
    for (name, why) in &report.refused {
        eprintln!("[hermes] starter capsule {name} not installed: {why}");
    }
    for name in &report.repaired {
        eprintln!("[hermes] starter capsule {name} did not match its pin; replaced with the verified copy");
    }
    report.present
}

/// Resolve the bundled `hermes` binary (env override → resource dir), honest error if absent.
pub fn resolve_hermes_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var(HERMES_BIN_ENV) {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "{HERMES_BIN_ENV} set but not found: {}",
            path.display()
        ));
    }
    // externalBin lives next to the main executable (Contents/MacOS/hermes), not the resource dir.
    crate::supervisor::resolve_external_bin(app, "hermes")
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
///
/// CORE-B-001: routed through the shared [`citrate_core_kit::fsutil`] writer, which
/// creates the file `0600` in the `open(2)` call itself — the bearer is never
/// world-readable in the window a `fs::write`-then-`chmod` left open.
fn persist_bearer(path: &Path, token: &str) -> Result<()> {
    citrate_core_kit::fsutil::write_secret_file(path, token.as_bytes())
        .map_err(|e| HermesError::Token(e.kind().to_string()))
}

// ---------------------------------------------------------------------------
// Tauri command surface (S6.2) — a process-wide lazy singleton drives the sidecar over the bearer
// control transport. No state wiring in the (s0-owned) lib.rs; Lane D stays self-contained.
// ---------------------------------------------------------------------------

use std::sync::OnceLock;

/// The process-wide Hermes manager. Built lazily on first command from the app (resolve the bundled
/// binary + the 0600 bearer/crash paths); one instance for the process lifetime.
static HERMES: OnceLock<HermesManager> = OnceLock::new();

/// The manager if this session built one, without building it (the services registry, K1a-2).
pub(crate) fn existing_manager() -> Option<&'static HermesManager> {
    HERMES.get()
}

/// HUP-S4.3: whether this session's Hermes sidecar is running (false if never started).
pub fn sidecar_running() -> bool {
    HERMES.get().is_some_and(|m| m.is_running())
}

/// HUP-S1.1 (US-1.1 AC3): the manager, only while its sidecar is running (the headless dispatcher).
pub(crate) fn running_manager() -> Option<&'static HermesManager> {
    HERMES.get().filter(|m| m.is_running())
}

/// Stop the hermes sidecar if this session started it (called on graceful app teardown). Idempotent
/// and a no-op if it was never started.
pub fn shutdown() {
    if let Some(m) = HERMES.get() {
        m.stop();
    }
}

/// Restart Hermes so a changed spawn setting applies (the terminal-commands switch). A Hermes that
/// is not started stays stopped (`Ok(false)`) and picks the setting up when it starts. Open
/// conversations end with the old sidecar; the chat opens a fresh one.
pub(crate) fn restart_if_running<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<bool, String> {
    let Some(m) = HERMES.get() else {
        return Ok(false);
    };
    if !m.is_started() {
        return Ok(false);
    }
    m.stop();
    if let Err(e) = crate::hermes_mcp::sync_for_app(app) {
        eprintln!("hermes: MCP allowlist not updated: {e}");
    }
    m.start_after_restart().map_err(|e| e.to_string())?;
    Ok(true)
}

/// The embedding server's model file while it runs (the Models screen refuses to delete it).
pub(crate) fn embed_model_in_use() -> Option<PathBuf> {
    HERMES
        .get()
        .and_then(|m| m.embed.as_ref())
        .filter(|e| e.is_started())
        .map(|e| e.model_path().to_path_buf())
}

/// HUP-S2.1: send the grant document to every open agent session, if the manager exists.
pub(crate) fn push_grants_to_sessions(doc: &crate::agent_grants::GrantState) -> GrantsPushOutcome {
    match HERMES.get() {
        Some(m) => m.push_grants(doc),
        None => GrantsPushOutcome::default(),
    }
}

/// SCL-S0.3: the managed-browser record core keeps for the Hermes sidecar, in a core-only folder
/// of the app's data that is never passed to the sidecar.
fn browser_record_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("core-lifecycle")
        .join(crate::owned_browser::RECORD_FILE))
}

/// SCL-S0.3: at app launch, stop a managed browser an earlier Hermes sidecar left behind and
/// remove its profile (the sidecar or this app ended without cleaning it up).
pub(crate) fn clean_leftover_browser<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Ok(path) = browser_record_path(app) {
        crate::owned_browser::apply(&path);
    }
}

/// HUP-S5.1 + S4.4: the Hermes manager for sibling modules (`browser.rs`, `mcp_servers.rs`).
pub(crate) fn manager_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<&'static HermesManager, String> {
    manager(app)
}

/// Lazily build/borrow the manager. A resolve failure (an ENV override set-but-missing, or no
/// resource dir) is returned every call until fixed — never a half-inited global. A missing bundled
/// binary is NOT an error here; `start` reports `BinaryNotFound` (honest, Rule 1).
pub(crate) fn manager<R: tauri::Runtime>(
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
    // Seed the capsule (skill) dir from the bundled starters so the agent boots with runnable skills
    // instead of an empty catalog (the "running but does nothing" bug). Best-effort; a resolve/copy
    // failure just means fewer skills, honestly reported — never a start failure.
    let capsules_dir = base.join("capsules");
    let resources = app.path().resource_dir().ok();
    if let Some(res) = &resources {
        let _ = seed_starter_capsules(&res.join("capsules"), &capsules_dir);
    }
    // HUP-S3.2: the sidecar's one SKILL.md loader reads the member's saved skills, Citrate's own
    // bundled skills and the reviewed third-party skills (checked against skills.lock).
    let skill_sources = crate::hermes_learn::SkillSources {
        authored: crate::skills_local::authored_skills_dir(
            &app.path().app_local_data_dir().map_err(|e| e.to_string())?,
        ),
        first_party: resources.as_ref().map(|r| r.join("skills")),
        third_party: resources.as_ref().map(|r| {
            (
                r.join("skills-bundle").join("skills.lock"),
                r.join("skills-bundle"),
            )
        }),
    };
    // HUP-S3.4: verified self-learning. Proposals and the HIC decision log live under
    // `hermes/learn`, accepted skills under `hermes/skills`. Nothing is learned unless the member
    // accepts a proposal backed by a verified workflow run.
    let mgr = HermesManager::new(bin, token_path, crash_path)
        .with_browser_record(browser_record_path(app)?)
        .with_capsules_dir(capsules_dir)
        .with_mcp_config_path(crate::hermes_mcp::config_path(&base))
        .with_checkpoints_dir(base.join("checkpoints"))
        .with_learn_dirs(base.join("learn"), base.join("skills"))
        .with_skill_sources(skill_sources)
        .with_mcp_servers(crate::mcp_servers::os_store(base.clone()))
        .with_chain_data_dir(base.clone())
        .with_env_source({
            // HUP-S5.2/S5.3 web opt-ins and HUP-S6.3 the toolchain switch, read at each start.
            let web = crate::hermes_web::file_env_source(
                base.clone(),
                crate::components::components_root(app).ok(),
            );
            let toolchain = crate::forge_toolchain::file_env_source(
                base.clone(),
                crate::forge_toolchain::places_for(app)?,
            );
            // HUP-S9.3: verified-trajectory recording, only when the member turned it on.
            let trajectories = crate::fl_trajectories::file_env_source(base.clone());
            // Terminal commands (shell_run): `1` when the member's switch is on (the default),
            // pinned empty when off.
            let terminal = crate::hermes_terminal::file_env_source(base.clone());
            std::sync::Arc::new(move || {
                let mut env = web();
                env.extend(toolchain());
                env.extend(trajectories());
                env.extend(terminal());
                env
            })
        });
    // HUP-S1.2 / US-1.4: the bundled BGE model on a second, embedding-only llama-server. Only
    // when both the model and the llama-server binary are present; the pin is checked at start.
    let mgr = match (
        crate::embed_serve::resolve_model(resources.as_deref()),
        crate::serve::resolve_llama_bin(app).ok(),
    ) {
        (Some(model), Some(llama)) if model.exists() && llama.exists() => {
            mgr.with_embed(crate::embed_serve::EmbedServer::new(
                llama,
                model,
                crate::embed_serve::EMBED_PORT,
                base.join("embed.key"),
                base.join("embed-crashes.log"),
            ))
        }
        _ => mgr,
    };
    // If another thread won the race, `set` fails and we return the stored winner — same instance.
    let _ = HERMES.set(mgr);
    Ok(HERMES.get().expect("manager just set"))
}

/// HUP-S1.5 — run one escalation through the sidecar. `Err(false)`: nothing reached the sidecar
/// (it is not running), so nothing was sent; `Err(true)`: the call failed in flight, so the request
/// may have reached the provider.
pub(crate) fn sidecar_escalate<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    body: &str,
) -> std::result::Result<(u16, String), bool> {
    let mgr = manager(app).map_err(|_| false)?;
    match mgr.escalate(body) {
        Ok(r) => Ok((r.status, r.body)),
        Err(HermesError::NotRunning) => Err(false),
        Err(_) => Err(true),
    }
}

/// Start the sidecar (idempotent). Returns the local lifecycle status.
#[tauri::command]
pub async fn hermes_start(app_h: tauri::AppHandle) -> std::result::Result<HermesStatus, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_start_sync(app_h.clone())).await
}

/// Blocking body of [`hermes_start`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_start_sync(app: tauri::AppHandle) -> std::result::Result<HermesStatus, String> {
    let m = manager(&app)?;
    // HUP-S4.3: bring the MCP allowlist in line with the member's settings before the child reads
    // it. A write failure means no MCP this session (logged), never a failed start.
    if let Err(e) = crate::hermes_mcp::sync_for_app(&app) {
        eprintln!("hermes: MCP allowlist not updated: {e}");
    }
    m.start().map_err(|e| e.to_string())?;
    Ok(m.status())
}

/// The AgentHarnessDomain status snapshot: running + skill/pending counts. A not-started sidecar is a
/// clean stopped snapshot, not an error.
#[tauri::command]
pub async fn hermes_status(app_h: tauri::AppHandle) -> std::result::Result<RemoteStatus, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_status_sync(app_h.clone())).await
}

/// Blocking body of [`hermes_status`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_status_sync(app: tauri::AppHandle) -> std::result::Result<RemoteStatus, String> {
    let m = manager(&app)?;
    if !m.is_running() {
        return Ok(RemoteStatus {
            running: false,
            skills: 0,
            pending_approvals: 0,
            capabilities: None,
        });
    }
    m.remote_status().map_err(|e| e.to_string())
}

/// The installed skill catalog.
#[tauri::command]
pub async fn hermes_skills(app_h: tauri::AppHandle) -> std::result::Result<Vec<SkillMeta>, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_skills_sync(app_h.clone())).await
}

/// Blocking body of [`hermes_skills`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_skills_sync(app: tauri::AppHandle) -> std::result::Result<Vec<SkillMeta>, String> {
    manager(&app)?.list_skills().map_err(|e| e.to_string())
}

/// Accept a skill for execution; its chain effects surface as pending approvals (the ceremony bridge,
/// S6.3). Returns `{ ok: true }` = accepted.
#[tauri::command]
pub async fn hermes_run_skill(
    app_h: tauri::AppHandle,
    name: String,
    args: serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_run_skill_sync(app_h.clone(), name, args)).await
}

/// Blocking body of [`hermes_run_skill`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_run_skill_sync(
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
pub async fn hermes_pending_approvals(
    app_h: tauri::AppHandle,
) -> std::result::Result<Vec<PendingApproval>, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_pending_approvals_sync(app_h.clone())).await
}

/// Blocking body of [`hermes_pending_approvals`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_pending_approvals_sync(
    app: tauri::AppHandle,
) -> std::result::Result<Vec<PendingApproval>, String> {
    let mut approvals = manager(&app)?
        .pending_approvals()
        .map_err(|e| e.to_string())?;
    // S6.4 — normalize the sidecar's coarse risk level to the AgentHarnessDomain kind
    // (chain|code|shell) so the UI shows the right HIC-1 control (signature vs approve/reject).
    agent_tools::normalize_kinds(&mut approvals);
    Ok(approvals)
}

/// Stop the sidecar (SIGTERM → grace → SIGKILL; the session bearer is wiped). Idempotent.
#[tauri::command]
pub async fn hermes_stop(app_h: tauri::AppHandle) -> std::result::Result<(), String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_stop_sync(app_h.clone())).await
}

/// Blocking body of [`hermes_stop`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_stop_sync(app: tauri::AppHandle) -> std::result::Result<(), String> {
    manager(&app)?.stop();
    Ok(())
}

/// S6.3 — bridge the sidecar's head pending CHAIN effect into a PENDING ceremony the human approves
/// (Rule 3: signs nothing here). Returns the `CeremonyView` to display, or `null` when there is
/// nothing pending / the head is a non-chain effect. The user then approves it via the normal
/// ceremony path (`sign_and_broadcast`) and calls `hermes_resolve(true)` to let the capsule proceed.
#[tauri::command]
pub async fn hermes_bridge_pending(
    app_h: tauri::AppHandle,
    id: String,
) -> std::result::Result<Option<CeremonyView>, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st1 = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st2 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        hermes_bridge_pending_sync(app_h.clone(), st1, st2, id)
    })
    .await
}

/// Blocking body of [`hermes_bridge_pending`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_bridge_pending_sync(
    app: tauri::AppHandle,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    id: String,
) -> std::result::Result<Option<CeremonyView>, String> {
    let rpc = crate::rpc::RpcClient::citrate();
    manager(&app)?
        .bridge_pending(&ceremony.0, &custody.0, &rpc, &id)
        .map_err(|e| e.to_string())
}

/// S6.3 — resolve the sidecar's head effect after the human decided its ceremony: `approve=true`
/// lets the (already-signed-and-broadcast) effect proceed; `false` aborts it. The head is blocked
/// until this call, so it targets the effect that was bridged.
#[tauri::command]
pub async fn hermes_resolve(
    app_h: tauri::AppHandle,
    approve: bool,
    id: String,
) -> std::result::Result<(), String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || hermes_resolve_sync(app_h.clone(), approve, id)).await
}

/// Blocking body of [`hermes_resolve`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_resolve_sync(
    app: tauri::AppHandle,
    approve: bool,
    id: String,
) -> std::result::Result<(), String> {
    manager(&app)?
        .resolve_head(approve, &id)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// HUP-S1.4 — interviewer DTOs, input bounds and commands (runtime agent-loop::interview)
// ---------------------------------------------------------------------------
//
// These mirror the sidecar's wire shapes exactly (snake_case, as `agent-loop::interview` serializes
// them) so a brief round-trips webview → core → sidecar unchanged. Core bounds sizes; the sidecar
// owns the semantics (choices, required gates, fixed workflow).

const MAX_BRIEF_GOAL_CHARS: usize = 2000;
const MAX_BRIEF_ANSWER_CHARS: usize = 500;
const MAX_BRIEF_ANSWERS: usize = 16;
const MAX_BRIEF_LIST: usize = 32;
const MAX_BRIEF_ITEM_CHARS: usize = 200;

/// One interview question. `choices` empty means free text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackQuestion {
    pub id: String,
    pub ask: String,
    #[serde(default)]
    pub choices: Vec<String>,
    pub default: String,
}

/// An interview track (a goal family that selects a workflow).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub persona: String,
    pub skills: Vec<String>,
    pub workflow: String,
    /// False until the workflow ships; the card says so instead of pretending (Rule 1).
    pub workflow_available: bool,
    #[serde(default)]
    pub ships_in: Option<String>,
    pub gates: Vec<String>,
    pub questions: Vec<TrackQuestion>,
}

/// One answered question in a brief.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefConstraint {
    pub id: String,
    pub ask: String,
    pub answer: String,
    pub from_default: bool,
}

/// The brief the member reads and edits before anything is built.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Brief {
    pub track: String,
    pub goal: String,
    pub constraints: Vec<BriefConstraint>,
    pub persona: String,
    pub skills: Vec<String>,
    pub workflow: String,
    pub workflow_available: bool,
    #[serde(default)]
    pub ships_in: Option<String>,
    pub gates: Vec<String>,
}

/// `POST /briefs` reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefDraft {
    pub brief: Brief,
    pub markdown: String,
}

/// `POST /briefs/check` reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefChecked {
    pub ok: bool,
    pub markdown: String,
}

/// A track or question id: `[a-z0-9_-]`, 1..=64 (lowercase slugs, as the bundled tracks use).
fn valid_slug(s: &str, what: &str) -> std::result::Result<(), String> {
    if !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        Ok(())
    } else {
        Err(format!("invalid {what}"))
    }
}

fn valid_len(s: &str, max: usize, what: &str) -> std::result::Result<(), String> {
    if s.chars().count() > max {
        Err(format!("{what} is longer than {max} characters"))
    } else {
        Ok(())
    }
}

fn valid_goal(goal: &str) -> std::result::Result<(), String> {
    if goal.trim().is_empty() {
        return Err("a brief needs a goal".into());
    }
    valid_len(goal, MAX_BRIEF_GOAL_CHARS, "the goal")
}

fn valid_list(items: &[String], what: &str) -> std::result::Result<(), String> {
    if items.len() > MAX_BRIEF_LIST {
        return Err(format!("at most {MAX_BRIEF_LIST} {what}"));
    }
    for i in items {
        valid_len(i, MAX_BRIEF_ITEM_CHARS, what)?;
    }
    Ok(())
}

/// Size/shape bounds on an edited brief before it goes to the sidecar for the real check.
fn validate_brief_shape(b: &Brief) -> std::result::Result<(), String> {
    valid_slug(&b.track, "track id")?;
    valid_goal(&b.goal)?;
    valid_len(&b.persona, MAX_BRIEF_ITEM_CHARS, "the persona")?;
    valid_len(&b.workflow, MAX_BRIEF_ITEM_CHARS, "the workflow")?;
    if let Some(v) = &b.ships_in {
        valid_len(v, MAX_BRIEF_ITEM_CHARS, "ships_in")?;
    }
    valid_list(&b.skills, "skills")?;
    valid_list(&b.gates, "gates")?;
    if b.constraints.len() > MAX_BRIEF_ANSWERS {
        return Err(format!("at most {MAX_BRIEF_ANSWERS} answers"));
    }
    for c in &b.constraints {
        valid_slug(&c.id, "question id")?;
        valid_len(&c.ask, MAX_BRIEF_ANSWER_CHARS, "a question")?;
        valid_len(&c.answer, MAX_BRIEF_ANSWER_CHARS, "an answer")?;
    }
    Ok(())
}

/// **hermes_tracks** — the interview tracks the sidecar serves.
#[tauri::command]
pub async fn hermes_tracks(app: tauri::AppHandle) -> std::result::Result<Vec<Track>, String> {
    crate::blocking::off_main(move || manager(&app)?.tracks().map_err(|e| e.to_string())).await
}

/// **hermes_brief_create** — answers → brief (unanswered questions take their defaults; no track
/// lets the sidecar suggest one). Builds nothing.
#[tauri::command]
pub async fn hermes_brief_create(
    app: tauri::AppHandle,
    track: Option<String>,
    goal: String,
    answers: BTreeMap<String, String>,
) -> std::result::Result<BriefDraft, String> {
    crate::blocking::off_main(move || {
        manager(&app)?
            .brief_create(track.as_deref(), &goal, &answers)
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_brief_check** — validate a member-edited brief (a refusal reads `BRIEF_REFUSED: …`).
#[tauri::command]
pub async fn hermes_brief_check(
    app: tauri::AppHandle,
    brief: Brief,
) -> std::result::Result<BriefChecked, String> {
    crate::blocking::off_main(move || {
        manager(&app)?
            .brief_check(&brief)
            .map_err(|e| e.to_string())
    })
    .await
}

// ---------------------------------------------------------------------------
// HUP-S1.1c — session body builder + commands (ADR loop-in-sidecar)
// ---------------------------------------------------------------------------

/// A sidecar session id: short, `[A-Za-z0-9-]` only (it is placed in a URL path).
pub fn valid_session_id(id: &str) -> std::result::Result<(), String> {
    if !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Ok(())
    } else {
        Err("invalid session id".into())
    }
}

/// HUP-S2.4 / A8 — the `effect` / `trust` annotations every core tool must carry into a sidecar
/// session (the runtime's `ToolAnnotations` shape). They drive the sidecar's taint downgrade, so a
/// tool without both is refused here rather than left to the runtime's safe default: the webview
/// keeps one reviewed annotation per tool. Only the two values plus the derived `read_only` hint are
/// forwarded; anything else in the object (a `host`, other hints) is dropped.
fn session_tool_annotations(
    tool: &serde_json::Value,
    function: &serde_json::Value,
    name: &str,
) -> std::result::Result<serde_json::Value, String> {
    let ann = tool
        .get("annotations")
        .or_else(|| function.get("annotations"))
        .and_then(|a| a.as_object())
        .ok_or_else(|| format!("tool {name} needs effect/trust annotations"))?;
    let effect = match ann.get("effect").and_then(|v| v.as_str()) {
        Some(e @ ("none" | "write" | "spend" | "sign")) => e,
        _ => {
            return Err(format!(
                "tool {name} annotation effect must be none, write, spend or sign"
            ))
        }
    };
    let trust = match ann.get("trust").and_then(|v| v.as_str()) {
        Some(t @ ("trusted" | "untrusted")) => t,
        _ => {
            return Err(format!(
                "tool {name} annotation trust must be trusted or untrusted"
            ))
        }
    };
    Ok(serde_json::json!({ "effect": effect, "trust": trust, "read_only": effect == "none" }))
}

/// Build the `POST /sessions` body. The webview supplies ONLY the system prompt and tool specs
/// (OpenAI `{type:"function",function:{…}}` wrappers or bare `{name,description,parameters}`); the
/// model endpoint, key and model name come from Rust-owned serve state; every tool is stamped
/// `host: "core"` so it executes through citrate-core's approval gates, never in the sidecar.
/// Each tool must carry `effect` / `trust` annotations (HUP-S2.4 / A8), forwarded as given. The body
/// sets `hicAware: true`: a `hic: "required"` call (an effectful call after the session read
/// untrusted content) resolves only through the member's explicit Approve/Decline click, with no
/// automatic or budget route (pinned by `src/shell/hicApproval.test.ts`). Any future budget path
/// (HUP-S2.3) must keep refusing hic-required calls, or this flag must come off.
/// `context_tokens` is the running llama-server's planned `--ctx-size` (HUP-S1.6).
pub fn build_session_body(
    system_prompt: &str,
    tools_json: &str,
    base_url: &str,
    bearer: &str,
    model: &str,
    context_tokens: u32,
) -> std::result::Result<String, String> {
    let raw: serde_json::Value =
        serde_json::from_str(tools_json).map_err(|_| "tools must be a JSON array".to_string())?;
    let arr = raw.as_array().ok_or("tools must be a JSON array")?;
    if arr.len() > 64 {
        return Err("at most 64 tools".into());
    }
    let mut tools = Vec::with_capacity(arr.len());
    for t in arr {
        let f = t.get("function").unwrap_or(t);
        let name = f
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("every tool needs a name")?;
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(format!("invalid tool name {name:?}"));
        }
        let description: String = f
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(2000)
            .collect();
        let parameters = f
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "object"}));
        if !parameters.is_object() {
            return Err(format!("tool {name} parameters must be a JSON object"));
        }
        let annotations = session_tool_annotations(t, f, name)?;
        tools.push(serde_json::json!({ "name": name, "description": description, "parameters": parameters, "host": "core", "annotations": annotations }));
    }
    Ok(serde_json::json!({
        "model": model,
        "systemPrompt": system_prompt,
        "llm": { "baseUrl": base_url, "bearer": bearer },
        "tools": tools,
        // HUP-S1.2: the sidecar offers only the relevant tools per request and keeps every prompt
        // inside the local model's real context window (llama-server --ctx-size).
        "maxToolsPerRequest": 8,
        "contextTokens": context_tokens,
        // HUP-S1.6: the same per-turn reply cap as direct chat, never more than a quarter of the
        // window so a small window still leaves room for the prompt.
        "maxTokens": crate::ai::AI_MAX_TOKENS.min(context_tokens / 4),
        "hicAware": true,
    })
    .to_string())
}

/// K5a: the environment a spawned child sees: `inherited` (the app's own, which the spawn passes
/// on) overlaid with `set` (core's explicit values, which win). Only `CITRATE_` keys are kept:
/// those are the ones that turn sidecar features on.
pub(crate) fn effective_child_env(
    inherited: impl Iterator<Item = (String, String)>,
    set: &[(String, String)],
) -> Vec<(String, String)> {
    effective_child_env_for(inherited, set, cfg!(windows))
}

/// [`effective_child_env`] with the platform's env-name rule made explicit (tests drive both).
pub(crate) fn effective_child_env_for(
    inherited: impl Iterator<Item = (String, String)>,
    set: &[(String, String)],
    case_insensitive: bool,
) -> Vec<(String, String)> {
    use crate::daemons::env_name_eq;
    let citrate = |k: &str| {
        k.get(..8)
            .is_some_and(|p| env_name_eq(p, "CITRATE_", case_insensitive))
    };
    let mut out: Vec<(String, String)> = inherited
        .filter(|(k, _)| {
            citrate(k) && !set.iter().any(|(s, _)| env_name_eq(s, k, case_insensitive))
        })
        .collect();
    out.extend(set.iter().cloned());
    out
}

/// HUP-S10.3 — the body for a DAEMON run's session: exactly the chat body plus `unattended: true`,
/// so the sidecar starts the session in the HIC downgrade (every effectful call needs the member's
/// explicit decision from the first step). A sidecar that predates the flag ignores it; core marks
/// every effectful daemon call HIC-required on its own as well (`src/daemons/runner.ts`).
pub fn build_daemon_session_body(
    system_prompt: &str,
    tools_json: &str,
    base_url: &str,
    bearer: &str,
    model: &str,
    context_tokens: u32,
) -> std::result::Result<String, String> {
    let body = build_session_body(
        system_prompt,
        tools_json,
        base_url,
        bearer,
        model,
        context_tokens,
    )?;
    let mut v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if let Some(o) = v.as_object_mut() {
        o.insert("unattended".into(), serde_json::Value::Bool(true));
    }
    Ok(v.to_string())
}

/// K5a + citrate-agent-runtime#87: the body for a STORE daemon run's session: the daemon body with
/// only the core tools the daemon was approved for, `toolSets` naming exactly the host tool sets
/// its approved permissions cover (empty = none; a sidecar without the field ignores it, which is
/// why core also gates the start on the sidecar's capability), and `promptSnapshot: false` so the
/// sidecar adds nothing to the prompt.
#[allow(clippy::too_many_arguments)]
pub fn build_store_daemon_session_body(
    system_prompt: &str,
    tools_json: &str,
    base_url: &str,
    bearer: &str,
    model: &str,
    context_tokens: u32,
    allowed: &[String],
) -> std::result::Result<String, String> {
    let body = build_daemon_session_body(
        system_prompt,
        tools_json,
        base_url,
        bearer,
        model,
        context_tokens,
    )?;
    let mut v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let o = v
        .as_object_mut()
        .ok_or_else(|| "internal: the session body is not an object".to_string())?;
    if let Some(serde_json::Value::Array(tools)) = o.get_mut("tools") {
        tools.retain(|t| {
            t.get("name")
                .and_then(|n| n.as_str())
                .is_some_and(|n| allowed.iter().any(|a| a == n))
        });
    }
    o.insert(
        "toolSets".into(),
        serde_json::json!(crate::daemons::store_tool_sets(allowed)),
    );
    o.insert("promptSnapshot".into(), serde_json::Value::Bool(false));
    Ok(v.to_string())
}

/// The host tool sets a sidecar's `GET /status` says a session can be scoped to, or `None` unless
/// it reports BOTH `sessionToolSets` and `sessionPromptSnapshot: true` (citrate-agent-runtime#87).
pub(crate) fn scoping_tool_sets(status: &RemoteStatus) -> Option<Vec<String>> {
    let caps = status.capabilities.as_ref()?;
    caps.session_prompt_snapshot
        .then(|| caps.session_tool_sets.clone())
}

/// The check on a Store daemon's session-create response: `toolSets` echoed as an array with
/// exactly the sent names (any order), and `promptSnapshot` echoed as `false`.
pub(crate) fn verify_store_session_echo(
    resp: &serde_json::Value,
    sent_sets: &[String],
) -> std::result::Result<(), String> {
    let refused = |what: &str| {
        format!("Not started: Hermes did not confirm {what} for this daemon's session, so it was closed. Hermes may need an update.")
    };
    let echoed = resp
        .get("toolSets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| refused("the toolSets allowlist"))?;
    let mut got: Vec<&str> = echoed.iter().filter_map(|v| v.as_str()).collect();
    if got.len() != echoed.len() {
        return Err(refused("the toolSets allowlist"));
    }
    let mut want: Vec<&str> = sent_sets.iter().map(String::as_str).collect();
    got.sort_unstable();
    got.dedup();
    want.sort_unstable();
    want.dedup();
    if got != want {
        return Err(refused("the toolSets allowlist"));
    }
    if resp.get("promptSnapshot") != Some(&serde_json::Value::Bool(false)) {
        return Err(refused("promptSnapshot: false"));
    }
    Ok(())
}

/// Open a daemon session on the local model with the body `build` makes from (system prompt,
/// tools JSON, base URL, bearer, model, context tokens).
fn open_local_session_with(
    app: &tauri::AppHandle,
    system_prompt: &str,
    tools_json: &str,
    build: impl FnOnce(&str, &str, &str, &str, &str, u32) -> std::result::Result<String, String>,
) -> std::result::Result<String, String> {
    open_local_session_via(app, system_prompt, tools_json, build, |m, body| {
        m.session_open(body)
    })
}

/// [`open_local_session_with`] with the call that opens the built body.
fn open_local_session_via(
    app: &tauri::AppHandle,
    system_prompt: &str,
    tools_json: &str,
    build: impl FnOnce(&str, &str, &str, &str, &str, u32) -> std::result::Result<String, String>,
    open: impl FnOnce(&HermesManager, &str) -> Result<String>,
) -> std::result::Result<String, String> {
    let serve = tauri::Manager::try_state::<crate::serve::ServeState>(app)
        .ok_or("internal: serve state unavailable")?;
    if !serve.0.is_running() {
        return Err(
            "the local model isn't running yet. Start it from Models, then try again.".into(),
        );
    }
    let key = serve.0.api_key();
    let body = build(
        system_prompt,
        tools_json,
        &serve.0.base_url(),
        key.as_str(),
        &serve.0.current_model_file(),
        serve.0.ctx_size(),
    )?;
    open(manager(app)?, &body).map_err(|e| e.to_string())
}

/// **hermes_session_open_unattended** — open a sidecar session for a scheduled daemon run, on the
/// LOCAL model only (a daemon's spend budget is 0).
#[tauri::command]
pub async fn hermes_session_open_unattended(
    app: tauri::AppHandle,
    system_prompt: String,
    tools_json: String,
    // K5a: the daemon this run belongs to. A Store daemon is re-checked against the sidecar's
    // CURRENT environment right before its session opens (an older runner sends none).
    daemon_id: Option<String>,
    // The claimed run this session is for; core checks it is in flight for `daemon_id`.
    run_id: Option<String>,
) -> std::result::Result<String, String> {
    crate::blocking::off_main(move || {
        // DGX R3: fail closed. A missing or unknown daemon id, or a run that is not this
        // daemon's run in flight, is refused inside the check. A Store daemon is opened in the
        // sidecar generation it was checked against (a restart in between refuses the open).
        let store_allowed =
            crate::daemons::check_store_daemon_open(&app, daemon_id.as_deref(), run_id.as_deref())?;
        match store_allowed {
            // A Store daemon: only its approved core tools, exactly its approved host tool sets,
            // and no prompt additions from the sidecar.
            // On a sidecar that scopes host tool sets, the response must confirm the scoping
            // (`session_open_store`); otherwise the env-hold fallback already found no uncovered
            // set on, and the session opens as is.
            Some(open) => {
                let allowed = open.allowed;
                let generation = open.generation;
                let sets = crate::daemons::store_tool_sets(&allowed);
                let build = |p: &str, t: &str, u: &str, k: &str, m: &str, c: u32| {
                    build_store_daemon_session_body(p, t, u, k, m, c, &allowed)
                };
                if open.scoped {
                    open_local_session_via(&app, &system_prompt, &tools_json, build, |m, body| {
                        m.in_generation(generation, |m| m.session_open_store(body, &sets))
                    })
                } else {
                    open_local_session_via(&app, &system_prompt, &tools_json, build, |m, body| {
                        m.session_open_in(generation, body)
                    })
                }
            }
            None => open_local_session_with(
                &app,
                &system_prompt,
                &tools_json,
                build_daemon_session_body,
            ),
        }
    })
    .await
}

/// **hermes_session_close** — close a sidecar session (a finished daemon run).
#[tauri::command]
pub async fn hermes_session_close(
    app: tauri::AppHandle,
    id: String,
) -> std::result::Result<(), String> {
    crate::blocking::off_main(move || manager(&app)?.session_close(&id).map_err(|e| e.to_string()))
        .await
}

/// **hermes_session_open** — open a sidecar agent session on the LOCAL model. The endpoint + key are
/// the running llama-server's (serve state); nothing about them comes from the webview.
#[tauri::command]
pub async fn hermes_session_open(
    app: tauri::AppHandle,
    system_prompt: String,
    tools_json: String,
    // HUP-S3.3: the member's persona (a shipped id, or a member-defined one). Absent = none, and
    // the body is unchanged.
    persona: Option<String>,
    custom_persona: Option<personas::CustomPersonaInput>,
) -> std::result::Result<String, String> {
    crate::blocking::off_main(move || {
        let serve = tauri::Manager::try_state::<crate::serve::ServeState>(&app)
            .ok_or("internal: serve state unavailable")?;
        if !serve.0.is_running() {
            return Err(
                "the local model isn't running yet — start it from Models, then try again".into(),
            );
        }
        let key = serve.0.api_key();
        let body = build_session_body(
            &system_prompt,
            &tools_json,
            &serve.0.base_url(),
            key.as_str(),
            &serve.0.current_model_file(),
            serve.0.ctx_size(),
        )?;
        let body =
            personas::with_session_persona(&body, persona.as_deref(), custom_persona.as_ref())?;
        // HUP-S2.1: the member's folder grants travel with the session (an unreadable grant file
        // sends an empty document: no folder access). Opened under the grant store's lock, so a
        // change saved meanwhile is still sent to this session.
        let store = crate::agent_grants::GrantStore::for_app(&app)?;
        let m = manager(&app)?;
        crate::agent_grants::open_with_grants(&store, &body, |b| {
            m.session_open(b).map_err(|e| e.to_string())
        })
    })
    .await
}

/// **hermes_session_send** — run one user turn in the session.
#[tauri::command]
pub async fn hermes_session_send(
    app: tauri::AppHandle,
    id: String,
    text: String,
) -> std::result::Result<(), String> {
    crate::blocking::off_main(move || {
        manager(&app)?
            .session_send(&id, &text)
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_session_events** — long-poll the session's event log after `after` (the chat view's
/// read: it renews the view's lease on the session, see `hermes_headless`).
#[tauri::command]
pub async fn hermes_session_events(
    app: tauri::AppHandle,
    id: String,
    after: u64,
    wait_ms: u64,
) -> std::result::Result<serde_json::Value, String> {
    crate::blocking::off_main(move || {
        let page = manager(&app)?
            .session_events(&id, after, wait_ms)
            .map_err(|e| e.to_string())?;
        // HUP-S1.1 (US-1.1 AC3): the view is watching this session; calls it is shown are its own,
        // and a call the headless dispatcher already answered is marked so the view skips it.
        Ok(crate::hermes_headless::LEASES.view_page(&id, page, std::time::Instant::now()))
    })
    .await
}

/// **hermes_session_tool_result** — hand back a core-hosted tool's result (already gated by core).
#[tauri::command]
pub async fn hermes_session_tool_result(
    app: tauri::AppHandle,
    id: String,
    call_id: String,
    status: String,
    content: String,
) -> std::result::Result<(), String> {
    crate::blocking::off_main(move || {
        // HUP-S1.1 (US-1.1 AC3): the view is still driving this session. Renewed BEFORE the post,
        // so the next call the result unblocks stays the view's (the lease is fresh when it lands).
        crate::hermes_headless::LEASES.view_answered(&id, &call_id, std::time::Instant::now());
        manager(&app)?
            .session_tool_result(&id, &call_id, &status, &content)
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_session_stop** — stop the session's current turn.
#[tauri::command]
pub async fn hermes_session_stop(
    app: tauri::AppHandle,
    id: String,
) -> std::result::Result<(), String> {
    crate::blocking::off_main(move || manager(&app)?.session_stop(&id).map_err(|e| e.to_string()))
        .await
}

// HUP-S3.3 + S3.7: personas + track workflows (child module: reuses the bearer-authed control).
#[path = "hermes_personas.rs"]
pub mod personas;

// HUP-S2.2: the member's decision on a `shell_run` command the sidecar holds (child module: reuses
// the bearer-authed control).
#[path = "hermes_shell.rs"]
pub mod shell;

// HUP-S4.1: the member's decision on an MCP card the sidecar holds (child module: reuses the
// bearer-authed control).
#[path = "hermes_mcp_cards.rs"]
pub mod mcp_cards;

#[cfg(test)]
mod brief_tests {
    include!("hermes_brief_tests.rs");
}

#[cfg(test)]
mod session_tests {
    include!("hermes_session_tests.rs");
}

#[cfg(test)]
mod tests {
    include!("hermes_tests.rs");
}

#[cfg(test)]
mod daemon_session_tests {
    include!("hermes_daemon_tests.rs");
}

#[cfg(all(test, unix))]
mod browser_cleanup_tests {
    include!("hermes_browser_tests.rs");
}
