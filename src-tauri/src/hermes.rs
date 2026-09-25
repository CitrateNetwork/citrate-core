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
use std::collections::HashMap;
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
                "STALE_APPROVAL: the agent's pending action changed since you reviewed it; nothing was resolved, re-review it"
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
    health_interval: Duration,
    #[cfg(test)]
    spawn_args_override: Option<Vec<String>>,
    /// The current session bearer (minted on start; wiped on drop). Held for control calls (S6.2+).
    token: Mutex<Option<Zeroizing<String>>>,
    sup: Mutex<Option<Supervisor>>,
    /// The bearer-authed control transport (production ureq; tests inject a mock).
    control: Box<dyn HermesControl>,
    /// S6.3 dedup: a chain effect's content key (hash of to+data) → the ceremony id minted for it.
    /// While an effect is the sidecar head it maps to AT MOST ONE ceremony: `bridge_pending` returns
    /// the SAME ceremony while pending, and once that ceremony is consumed (approved OR rejected) it
    /// returns None rather than minting a second — so an approved effect can never be re-bridged into a
    /// second broadcast (H-1). The entry is cleared only by `resolve_head` (the head has moved) or on
    /// stop, at which point a genuinely new effect may bridge fresh.
    bridged: Mutex<HashMap<String, String>>,
}

impl HermesManager {
    /// Build a manager over an explicit binary + token file + crash path (default control addr).
    pub fn new(bin: PathBuf, token_path: PathBuf, crash_record_path: PathBuf) -> Self {
        HermesManager {
            bin,
            control_addr: HERMES_CONTROL_ADDR.to_string(),
            token_path,
            crash_record_path,
            capsules_dir: None,
            health_interval: HEALTH_INTERVAL,
            #[cfg(test)]
            spawn_args_override: None,
            token: Mutex::new(None),
            sup: Mutex::new(None),
            control: Box::new(UreqControl),
            bridged: Mutex::new(HashMap::new()),
        }
    }

    /// Point the child at a capsule (skill) directory (`CITRATE_HERMES_CAPSULES`). Prod calls this
    /// with the seeded per-session dir so the agent starts with runnable skills instead of an empty
    /// catalog. Absent → the env is not set (unchanged default behavior).
    pub fn with_capsules_dir(mut self, dir: PathBuf) -> Self {
        self.capsules_dir = Some(dir);
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
        // A new session starts with a clean dedup map.
        self.bridged
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
        {
            let map = self.bridged.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = map.get(&key) {
                return Ok(ceremony.status(existing)); // Some(pending view) or None (decided → no re-mint)
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
            return Ok(ceremony.status(existing)); // reuse pending, or None if decided (never re-mint)
        }
        let view = ceremony.request(hermes_intent(&to, &data, &from, gas));
        map.insert(key, view.id.clone());
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
        // 409 = the head is not the call the member reviewed; nothing was resolved (re-review).
        if resp.status == 409 {
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
        Ok(())
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

/// Seed the per-session capsule dir from the bundled starter capsules (first run only). Each bundled
/// skill dir is copied into `dest` ONLY if a skill of that name is absent — a user's own capsules are
/// never clobbered. Best-effort: a missing bundled dir or a copy error is skipped, never fatal — the
/// agent still starts, honestly reporting however many skills it actually has (Rule 1). Returns the
/// number of skill dirs present in `dest` afterwards (0 is a legitimate, honest outcome).
fn seed_starter_capsules(bundled: &Path, dest: &Path) -> usize {
    let _ = std::fs::create_dir_all(dest);
    if let Ok(entries) = std::fs::read_dir(bundled) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let target = dest.join(entry.file_name());
            if target.exists() {
                continue; // never overwrite an existing (possibly user-added) skill
            }
            if std::fs::create_dir_all(&target).is_ok() {
                if let Ok(files) = std::fs::read_dir(entry.path()) {
                    for f in files.flatten() {
                        if f.path().is_file() {
                            let _ = std::fs::copy(f.path(), target.join(f.file_name()));
                        }
                    }
                }
            }
        }
    }
    std::fs::read_dir(dest)
        .map(|e| e.flatten().filter(|x| x.path().is_dir()).count())
        .unwrap_or(0)
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

/// Stop the hermes sidecar if this session started it (called on graceful app teardown). Idempotent
/// and a no-op if it was never started.
pub fn shutdown() {
    if let Some(m) = HERMES.get() {
        m.stop();
    }
}

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
    // Seed the capsule (skill) dir from the bundled starters so the agent boots with runnable skills
    // instead of an empty catalog (the "running but does nothing" bug). Best-effort; a resolve/copy
    // failure just means fewer skills, honestly reported — never a start failure.
    let capsules_dir = base.join("capsules");
    if let Ok(res) = app.path().resource_dir() {
        let _ = seed_starter_capsules(&res.join("capsules"), &capsules_dir);
    }
    let mgr = HermesManager::new(bin, token_path, crash_path).with_capsules_dir(capsules_dir);
    // If another thread won the race, `set` fails and we return the stored winner — same instance.
    let _ = HERMES.set(mgr);
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
pub fn hermes_stop(app: tauri::AppHandle) -> std::result::Result<(), String> {
    manager(&app)?.stop();
    Ok(())
}

/// S6.3 — bridge the sidecar's head pending CHAIN effect into a PENDING ceremony the human approves
/// (Rule 3: signs nothing here). Returns the `CeremonyView` to display, or `null` when there is
/// nothing pending / the head is a non-chain effect. The user then approves it via the normal
/// ceremony path (`sign_and_broadcast`) and calls `hermes_resolve(true)` to let the capsule proceed.
#[tauri::command]
pub fn hermes_bridge_pending(
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
pub fn hermes_resolve(
    app: tauri::AppHandle,
    approve: bool,
    id: String,
) -> std::result::Result<(), String> {
    manager(&app)?
        .resolve_head(approve, &id)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("hermes_tests.rs");
}
