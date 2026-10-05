//! HUP-S9.4 (US-9.2, US-9.1 AC5) — Hermes plans, explains and starts federated rounds, and a
//! round's LoRA adapter is loaded only after it passes the eval gate.
//!
//! Data sources (Rule 7):
//! - **Coordinator:** citrate-compute-pool's training-coordinator HTTP API, `GET /v1/status`
//!   (`{"counts":{pending,leased,done,quarantined,workers},"settlement":"shadow"}`). The base URL
//!   comes from config: the `CITRATE_FL_COORDINATOR_URL` env override, else the member's setting
//!   in `fl_rounds.json`. There is no default coordinator. Without one the plan says so and start
//!   is refused. Only typed numbers and one of three fixed settlement words leave the parser, so
//!   a coordinator cannot put free text into what Hermes reads.
//! - **Device fit:** the local tier probe (`tier::probe`), no network.
//! - **Eval gate:** the JSON scorecards `scripts/eval-tools.mjs` and `scripts/eval-qa.mjs` write
//!   (deterministic scorers in `src/agent/eval/`). The candidate run is stamped with the adapter's
//!   SHA-256 (`--adapter-sha256`), so a scorecard cannot be reused for a different file.
//!
//! What "start" means today: the member's HIC-1 decision (an explicit approve in the UI, no
//! signature, no wallet) authorizes this device to join the round described by one exact plan
//! hash. Core re-reads the coordinator at that moment and refuses if anything the member saw has
//! changed. When the plan names a round (`roundId`, the `round_id` of citrate-chain
//! `docs/fl/FL_ROUND_V1.md` section 2), the approval is also written as that round's per-round
//! consent (D-29) to `<app data>/fl_consent.json`, in the exact `{"rounds":["0x…"]}` shape the
//! compute-pool device worker reads through `CITRATE_FL_CONSENT_FILE`. The member can withdraw it.
//! This build does not bundle the device training worker, so no training runs from the app; the
//! receipt says so. Nothing here holds a key or signs (Rule 3).
//!
//! Round results (the local-devnet flow, FL_ROUND_V1 sections 6 and 9): the gate can bind an
//! adapter to the round that produced it. Core reads the round result the operator's round tool
//! wrote (`scripts/fl/devnet-round-e2e.sh` writes one per round) and accepts it only if the round
//! is Accepted, the chain, bundle and independent-replay record digests agree, the replay found no
//! mismatch and checked the merged adapter, and at least three devices took part. The adapter file
//! must hash to the round's merged adapter. Core does not read the ledger itself: on 40204 the
//! ledger is not deployed, and a devnet is throwaway.
//!
//! Loading an adapter: `fl_adapter_gate` hashes the file, checks it is GGUF, compares the base
//! and candidate scorecards and records ACCEPT or REJECT bound to that hash. `fl_adapter_load`
//! requires the latest record for that hash to be ACCEPT and the served base model to be the one
//! the scorecards measured, copies the file into `<app data>/adapters/<sha256>.gguf` and re-hashes
//! the copy (TOCTOU), then restarts llama-server with `--lora` pointing at the copy. The copy
//! matters because the supervisor re-reads `--lora` on every crash restart. Switching the base
//! model drops the adapter. A loaded adapter is remembered (`active` in `fl_rounds.json`) and put
//! back when the app next starts the local model on the same base, through the same checks as a
//! load (latest gate record ACCEPT for that base, copy re-hashed). Unload, a later REJECT, an
//! ACCEPT on another base, or switching the base while it is loaded ends that. Formal model:
//! `formal/FlRoundGate.tla`.
//!
//! Gate thresholds are conservative placeholders, **pending owner sign-off**: no metric may get
//! worse at all, and the mean of the shared metrics must strictly improve.

use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The env override for the coordinator base URL.
pub const COORDINATOR_ENV: &str = "CITRATE_FL_COORDINATOR_URL";
/// The app-data file holding the coordinator setting, start authorizations and gate records.
pub const STORE_FILE: &str = "fl_rounds.json";
/// The app-data file holding per-round consent for the device worker (`CITRATE_FL_CONSENT_FILE`).
pub const CONSENT_FILE: &str = "fl_consent.json";
/// The env variable the compute-pool device worker reads the consent file path from.
pub const WORKER_CONSENT_ENV: &str = "CITRATE_FL_CONSENT_FILE";
/// The largest round result file read for the gate. The devnet receipt is about 3 KB.
pub const MAX_ROUND_RESULT_BYTES: usize = 64 * 1024;
/// A round result names at least this many devices (US-9.1; FL_ROUND_V1 `minParticipants`).
pub const MIN_ROUND_PARTICIPANTS: u64 = 3;
/// The largest `/v1/status` body accepted. The real one is about 100 bytes.
pub const MAX_STATUS_BYTES: usize = 16 * 1024;
/// How long a plan stays startable. After that the member plans again (the coordinator moves).
pub const PLAN_TTL_MS: u64 = 15 * 60 * 1000;
/// Plans core remembers for a later start, oldest evicted first.
pub const MAX_PLANS: usize = 16;
/// Upper bound on trajectories one round may use from this device.
pub const MAX_TRAJECTORIES: u32 = 10_000;
/// Largest scorecard file read for the gate.
const MAX_SCORECARD_BYTES: u64 = 8 * 1024 * 1024;
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Gate placeholder, pending owner sign-off: the most any single metric may drop.
pub const REGRESSION_TOLERANCE: f64 = 0.0;
/// Gate placeholder, pending owner sign-off: the composite must rise by MORE than this.
pub const MIN_COMPOSITE_IMPROVEMENT: f64 = 0.0;

// ---------------------------------------------------------------------------
// Coordinator URL
// ---------------------------------------------------------------------------

fn is_loopback_host(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// Validate a coordinator base URL: `https` anywhere, `http` only on loopback (a local
/// coordinator during development). No credentials, query or fragment. Returns the URL without a
/// trailing slash.
pub fn normalize_coordinator_url(raw: &str) -> Result<String, String> {
    let u = url::Url::parse(raw.trim()).map_err(|_| format!("not a URL: {raw:?}"))?;
    let host = u
        .host()
        .ok_or_else(|| "the coordinator URL has no host".to_string())?;
    match u.scheme() {
        "https" => {}
        "http" if is_loopback_host(&host) => {}
        "http" => return Err("plain http is allowed only for a coordinator on this machine (127.0.0.1 or localhost); use https".into()),
        s => return Err(format!("unsupported scheme {s:?}; use https")),
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("the coordinator URL must not carry credentials".into());
    }
    if u.query().is_some() || u.fragment().is_some() {
        return Err("the coordinator URL must not carry a query or fragment".into());
    }
    Ok(u.as_str().trim_end_matches('/').to_string())
}

// ---------------------------------------------------------------------------
// /v1/status
// ---------------------------------------------------------------------------

/// The coordinator's settlement mode, reduced to three fixed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettlementMode {
    /// Contributions are measured, nothing is paid (the coordinator's current mode).
    Shadow,
    Live,
    /// Any other word. Never passed through.
    Unknown,
}

/// Where the coordinator's job pool stands, from its counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PoolPhase {
    /// No jobs at all.
    NoWork,
    /// Jobs waiting, none leased.
    Open,
    /// At least one job leased.
    Running,
    /// Nothing waiting or leased, at least one done.
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorStatus {
    pub pending: u64,
    pub leased: u64,
    pub done: u64,
    pub quarantined: u64,
    pub workers: u64,
    pub settlement: SettlementMode,
    pub phase: PoolPhase,
}

#[derive(Deserialize)]
struct WireCounts {
    pending: u64,
    leased: u64,
    done: u64,
    quarantined: u64,
    workers: u64,
}

#[derive(Deserialize)]
struct WireStatus {
    counts: WireCounts,
    settlement: String,
}

/// Parse a `/v1/status` body strictly. Any missing or mistyped field is an error, never zeroes.
pub fn parse_status(body: &str) -> Result<CoordinatorStatus, String> {
    if body.len() > MAX_STATUS_BYTES {
        return Err("the coordinator status reply is too large".into());
    }
    let w: WireStatus = serde_json::from_str(body)
        .map_err(|_| "the coordinator status reply is not the expected shape".to_string())?;
    let settlement = match w.settlement.as_str() {
        "shadow" => SettlementMode::Shadow,
        "live" => SettlementMode::Live,
        _ => SettlementMode::Unknown,
    };
    let c = w.counts;
    let phase = if c.leased > 0 {
        PoolPhase::Running
    } else if c.pending > 0 {
        PoolPhase::Open
    } else if c.done > 0 {
        PoolPhase::Complete
    } else {
        PoolPhase::NoWork
    };
    Ok(CoordinatorStatus {
        pending: c.pending,
        leased: c.leased,
        done: c.done,
        quarantined: c.quarantined,
        workers: c.workers,
        settlement,
        phase,
    })
}

/// The one HTTP read this module makes. Abstracted so the decision logic is testable; the tests
/// still exercise the production client against a loopback fixture.
pub trait CoordinatorHttp {
    /// `GET {base}/v1/status`, returning the body of a 2xx reply.
    fn get_status(&self, base: &str) -> Result<String, String>;
}

/// Production client: blocking ureq, no redirects, short timeouts, capped body.
#[derive(Default)]
pub struct UreqCoordinatorHttp;

impl CoordinatorHttp for UreqCoordinatorHttp {
    fn get_status(&self, base: &str) -> Result<String, String> {
        let url = format!("{base}/v1/status");
        let mut resp = ureq::get(&url)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(STATUS_TIMEOUT))
            .build()
            .call()
            .map_err(|e| format!("could not reach the coordinator ({e})"))?;
        let code = resp.status().as_u16();
        if !resp.status().is_success() {
            return Err(format!("the coordinator answered HTTP {code}"));
        }
        resp.body_mut()
            .with_config()
            .limit(MAX_STATUS_BYTES as u64 + 1)
            .read_to_string()
            .map_err(|e| format!("could not read the coordinator reply ({e})"))
    }
}

/// What the app knows about the coordinator right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CoordinatorView {
    NotConfigured,
    Unreachable {
        url: String,
        reason: String,
    },
    Live {
        url: String,
        status: CoordinatorStatus,
    },
}

/// Read the coordinator at `url` (already normalized). `None` makes no network call.
pub fn read_coordinator(url: Option<&str>, http: &dyn CoordinatorHttp) -> CoordinatorView {
    let Some(url) = url else {
        return CoordinatorView::NotConfigured;
    };
    match http.get_status(url).and_then(|b| parse_status(&b)) {
        Ok(status) => CoordinatorView::Live {
            url: url.to_string(),
            status,
        },
        Err(reason) => CoordinatorView::Unreachable {
            url: url.to_string(),
            reason,
        },
    }
}

// ---------------------------------------------------------------------------
// Proposal, device fit, plan
// ---------------------------------------------------------------------------

/// The coordinator's capability ladder (`citrate_training_worker::coordinator_protocol::Capability`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Probe,
    Federated,
    H01,
}

/// What Hermes proposes for this device's part in a round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundProposal {
    pub requires: Capability,
    pub lora_rank: u32,
    pub max_trajectories: u32,
    pub lease_hours: u32,
    /// The round to join (`0x` + 64 hex, FL_ROUND_V1 `round_id`), as the operator published it.
    /// With one, an approved start also writes the device worker's consent for that round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<String>,
}

impl Default for RoundProposal {
    /// Conservative defaults, pending owner sign-off: a small adapter, a bounded sample, and a
    /// six-hour lease (the coordinator's shortest ladder rung).
    fn default() -> Self {
        RoundProposal {
            requires: Capability::Federated,
            lora_rank: 8,
            max_trajectories: 500,
            lease_hours: 6,
            round_id: None,
        }
    }
}

/// `0x` followed by 64 hex digits (a keccak or round id).
fn is_b32_hex(s: &str) -> bool {
    s.len() == 66 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// `0x` followed by 40 hex digits.
fn is_address_hex(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

impl RoundProposal {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=64).contains(&self.lora_rank) || !self.lora_rank.is_power_of_two() {
            return Err("the LoRA rank must be a power of two from 1 to 64".into());
        }
        if !(1..=MAX_TRAJECTORIES).contains(&self.max_trajectories) {
            return Err(format!(
                "the trajectory cap must be from 1 to {MAX_TRAJECTORIES}"
            ));
        }
        if !(1..=48).contains(&self.lease_hours) {
            return Err("the lease must be from 1 to 48 hours".into());
        }
        if let Some(r) = &self.round_id {
            if !is_b32_hex(r.trim()) {
                return Err("the round id must be 0x followed by 64 hex digits".into());
            }
        }
        Ok(())
    }
}

/// This machine, as far as the local probe can tell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFit {
    pub tier: Option<String>,
    /// `Some(true)`: Apple Silicon unified memory or a probed GPU. `None`: unknown.
    pub accelerator: Option<bool>,
}

impl DeviceFit {
    pub fn from_facts(facts: &crate::tier::HardwareFacts, tier: Option<&str>) -> DeviceFit {
        let accelerator = match (facts.unified_memory, facts.gpu_vram_bytes) {
            (Some(true), _) => Some(true),
            (_, Some(v)) if v > 0 => Some(true),
            (Some(false), None) => Some(false),
            _ => None,
        };
        DeviceFit {
            tier: tier.map(str::to_string),
            accelerator,
        }
    }
}

/// The plan in plain words: one paragraph per question a member asks before joining.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundExplanation {
    pub data: String,
    pub compute: String,
    pub reward: String,
    pub privacy: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundPlan {
    /// SHA-256 over what the member approves: proposal, coordinator URL and status, base model.
    pub plan_hash: String,
    pub created_at_ms: u64,
    pub coordinator: CoordinatorView,
    pub proposal: RoundProposal,
    pub base_model: String,
    pub device: DeviceFit,
    pub explain: RoundExplanation,
    pub can_start: bool,
    pub blockers: Vec<String>,
}

#[derive(Serialize)]
struct HashedPlan<'a> {
    v: u32,
    proposal: &'a RoundProposal,
    coordinator: &'a CoordinatorView,
    base_model: &'a str,
}

fn plan_hash(proposal: &RoundProposal, view: &CoordinatorView, base_model: &str) -> String {
    let body = serde_json::to_vec(&HashedPlan {
        v: 1,
        proposal,
        coordinator: view,
        base_model,
    })
    .unwrap_or_default();
    hex::encode(Sha256::digest(&body))
}

fn capability_words(c: Capability) -> &'static str {
    match c {
        Capability::Probe => "probe (a short measurement run any machine can do)",
        Capability::Federated => "federated (LoRA training that needs a GPU or Apple Silicon)",
        Capability::H01 => "H-01 (the ablation ladder, bf16 CUDA on operator-vetted machines)",
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Build the plan and its explanation. Pure: the caller supplies the coordinator view and device.
pub fn build_plan(
    proposal: RoundProposal,
    coordinator: CoordinatorView,
    base_model: &str,
    device: &DeviceFit,
    now_ms: u64,
) -> Result<RoundPlan, String> {
    proposal.validate()?;
    let mut proposal = proposal;
    proposal.round_id = proposal.round_id.map(|r| r.trim().to_ascii_lowercase());
    let mut blockers = Vec::new();

    let data = format!(
        "Trains only on your verified Hermes conversations, at most {} of them, redacted on this device before use. Conversations that read untrusted pages are left out. They are recorded only while \"Train on my verified conversations\" is on (off by default); with it off this device has no training set to offer.",
        proposal.max_trajectories
    );

    let pool = match &coordinator {
        CoordinatorView::Live { status, .. } => format!(
            "The coordinator reports {} registered, {} waiting, {} running and {} done.",
            plural(status.workers, "machine", "machines"),
            plural(status.pending, "job", "jobs"),
            status.leased,
            status.done
        ),
        _ => "The coordinator's job counts are not available.".to_string(),
    };
    let round_words = match &proposal.round_id {
        Some(r) => format!(" The round is {r}. Joining writes your consent for this round only, which the device training worker checks before it takes a job; you can withdraw it."),
        None => " No round is named, so joining records your approval and writes no consent for a device worker.".to_string(),
    };
    let compute = format!(
        "{pool} Your device would take one job at a time, each leased for up to {} hours, at the {} tier. It trains a LoRA adapter of rank {} on {}.{round_words}",
        proposal.lease_hours,
        capability_words(proposal.requires),
        proposal.lora_rank,
        base_model
    );

    let reward = match &coordinator {
        CoordinatorView::Live { status, .. } => match status.settlement {
            SettlementMode::Shadow => "The coordinator runs settlement in shadow mode: contributions are measured and nothing is paid for this round.".to_string(),
            SettlementMode::Live => "The coordinator reports live settlement: a round settles on chain through its settlement contract after the challenge window. This app credits nothing itself, and any reward follows that contract's terms.".to_string(),
            SettlementMode::Unknown => "The coordinator did not say how this round settles, so assume nothing is paid.".to_string(),
        },
        _ => "Unknown until a coordinator answers. Assume nothing is paid.".to_string(),
    };

    let privacy = "Your conversations never leave this device. Only the trained adapter update (numbers, not text) is submitted, signed by the device training worker. The coordinator sees that worker's address, your IP address and when you work.".to_string();

    let status_line = match &coordinator {
        CoordinatorView::NotConfigured => {
            let b = "No training coordinator is configured. Live rounds need one (set it in Settings, or the operator sets CITRATE_FL_COORDINATOR_URL).".to_string();
            blockers.push(b.clone());
            b
        }
        CoordinatorView::Unreachable { url, reason } => {
            let b = format!("The coordinator at {url} could not be read: {reason}.");
            blockers.push(b.clone());
            b
        }
        CoordinatorView::Live { url, status } => match status.phase {
            PoolPhase::NoWork => {
                let b = format!("The coordinator at {url} has no open work right now.");
                blockers.push(b.clone());
                b
            }
            PoolPhase::Complete => {
                let b = format!("The coordinator at {url} has no open work: this round's jobs are done. When its adapter is published, run the eval gate before loading it.");
                blockers.push(format!("The coordinator at {url} has no open work."));
                b
            }
            PoolPhase::Open | PoolPhase::Running => {
                format!("The coordinator at {url} has work open.")
            }
        },
    };

    match proposal.requires {
        Capability::H01 => blockers.push(
            "H-01 ladder work is granted only to operator-vetted workers. Plan a federated or probe round instead.".into(),
        ),
        Capability::Federated => match device.accelerator {
            Some(true) => {}
            Some(false) => blockers.push(
                "Federated training needs a GPU or Apple Silicon accelerator, and this device has neither. A probe round still fits.".into(),
            ),
            None => blockers.push(
                "The app could not tell whether this device has a GPU, so it will not commit it to federated training. A probe round still fits.".into(),
            ),
        },
        Capability::Probe => {}
    }

    let hash = plan_hash(&proposal, &coordinator, base_model);
    Ok(RoundPlan {
        plan_hash: hash,
        created_at_ms: now_ms,
        can_start: blockers.is_empty(),
        blockers,
        coordinator,
        proposal,
        base_model: base_model.to_string(),
        device: device.clone(),
        explain: RoundExplanation {
            data,
            compute,
            reward,
            privacy,
            status: status_line,
        },
    })
}

// ---------------------------------------------------------------------------
// Persistent store + in-memory plans
// ---------------------------------------------------------------------------

/// One HIC-1 authorization: the member approved joining the round of exactly this plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRecord {
    pub plan_hash: String,
    pub coordinator_url: String,
    pub requires: Capability,
    pub authorized_at_ms: u64,
    /// The round this approval consented to, when the plan named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<String>,
    /// When the member withdrew that consent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartReceipt {
    pub plan_hash: String,
    pub coordinator_url: String,
    pub authorized_at_ms: u64,
    /// Always false in this build: no device training worker is bundled.
    pub training_started: bool,
    pub note: String,
    /// The round consented to, when the plan named one.
    pub round_id: Option<String>,
    /// Where that consent was written (pass it to the worker as `CITRATE_FL_CONSENT_FILE`).
    pub consent_file: Option<String>,
}

/// The adapter the member loaded, remembered so it is put back after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAdapter {
    pub sha256: String,
    /// The served model file it was loaded on.
    pub base_model: String,
}

/// The device worker's consent file: exactly `{"rounds":["0x…"]}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ConsentFile {
    rounds: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreFile {
    #[serde(default)]
    coordinator_url: Option<String>,
    #[serde(default)]
    starts: Vec<StartRecord>,
    #[serde(default)]
    gates: BTreeMap<String, AdapterGateRecord>,
    #[serde(default)]
    active: Option<ActiveAdapter>,
}

#[derive(Default)]
struct Inner {
    file: StoreFile,
    plans: VecDeque<RoundPlan>,
    /// Why the last re-apply after a restart did not happen, if it failed.
    restore_error: Option<String>,
    /// Set when the store file exists but could not be read. Writes are then refused so the
    /// member's data is not overwritten with an empty file.
    load_error: Option<String>,
}

/// Managed state: the persisted store plus the recent plans a start may name.
#[derive(Default)]
pub struct FlRounds {
    inner: Mutex<Inner>,
    path: Option<PathBuf>,
    /// Serializes starts and consent withdrawals, so the consent file and the start records change
    /// together (a start's rollback never removes another start's consent).
    consent_lock: Mutex<()>,
}

impl FlRounds {
    pub fn with_store(path: PathBuf) -> FlRounds {
        let mut inner = Inner::default();
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<StoreFile>(&bytes) {
                Ok(f) => inner.file = f,
                Err(e) => inner.load_error = Some(format!("{STORE_FILE} is unreadable ({e})")),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => inner.load_error = Some(format!("{STORE_FILE} could not be read ({e})")),
        }
        FlRounds {
            inner: Mutex::new(inner),
            path: Some(path),
            consent_lock: Mutex::new(()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(test)]
    pub fn store_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn load_error(&self) -> Option<String> {
        self.lock().load_error.clone()
    }

    /// Apply `f` to a copy of the file, persist it, and only then make it live.
    fn mutate<T>(&self, f: impl FnOnce(&mut StoreFile) -> Result<T, String>) -> Result<T, String> {
        let mut g = self.lock();
        if let Some(e) = &g.load_error {
            return Err(format!("{e}; fix or remove it before saving"));
        }
        let mut next = g.file.clone();
        let out = f(&mut next)?;
        if let Some(path) = &self.path {
            let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
            write_atomic(path, &bytes)?;
        }
        g.file = next;
        Ok(out)
    }

    pub fn coordinator_setting(&self) -> Option<String> {
        self.lock().file.coordinator_url.clone()
    }

    /// Persist the member's coordinator URL (`None` clears it). An invalid value keeps the old one.
    pub fn set_coordinator_setting(&self, url: Option<&str>) -> Result<Option<String>, String> {
        let normalized = match url.map(str::trim).filter(|s| !s.is_empty()) {
            Some(u) => Some(normalize_coordinator_url(u)?),
            None => None,
        };
        self.mutate(|f| {
            f.coordinator_url = normalized.clone();
            Ok(normalized)
        })
    }

    pub fn remember_plan(&self, plan: RoundPlan) {
        let mut g = self.lock();
        g.plans.retain(|p| p.plan_hash != plan.plan_hash);
        g.plans.push_back(plan);
        while g.plans.len() > MAX_PLANS {
            g.plans.pop_front();
        }
    }

    pub fn lookup_plan(&self, hash: &str) -> Option<RoundPlan> {
        self.lock()
            .plans
            .iter()
            .find(|p| p.plan_hash == hash)
            .cloned()
    }

    pub fn starts(&self) -> Vec<StartRecord> {
        self.lock().file.starts.clone()
    }

    /// Record a gate decision. The latest decision for a hash replaces the earlier one, so a
    /// later REJECT revokes an earlier ACCEPT.
    pub fn record_gate(&self, rec: AdapterGateRecord) -> Result<(), String> {
        self.mutate(|f| {
            // A record that no longer allows the remembered adapter on the base it was loaded on
            // ends the re-apply; a later ACCEPT does not restore it without a new load.
            if let Some(a) = &f.active {
                if a.sha256 == rec.adapter_sha256
                    && (rec.decision.verdict != GateVerdict::Accept
                        || model_stem(&rec.base_model) != model_stem(&a.base_model))
                {
                    f.active = None;
                }
            }
            f.gates.insert(rec.adapter_sha256.clone(), rec);
            Ok(())
        })
    }

    /// The adapter to put back after a restart, if any.
    pub fn active(&self) -> Option<ActiveAdapter> {
        self.lock().file.active.clone()
    }

    /// Remember a loaded adapter. Refused unless its latest gate record is ACCEPT for that base.
    pub fn set_active(&self, sha: &str, base_model_file: &str) -> Result<(), String> {
        let sha = sha.trim().to_ascii_lowercase();
        self.mutate(|f| {
            match f.gates.get(&sha) {
                Some(r)
                    if r.decision.verdict == GateVerdict::Accept
                        && model_stem(&r.base_model) == model_stem(base_model_file) => {}
                _ => return Err(
                    "only an adapter the eval gate accepted for this base model can be remembered"
                        .to_string(),
                ),
            }
            f.active = Some(ActiveAdapter {
                sha256: sha.clone(),
                base_model: base_model_file.to_string(),
            });
            Ok(())
        })
    }

    /// Forget the remembered adapter (unload, or a base switch while it was loaded).
    pub fn forget_active(&self) -> Result<(), String> {
        self.mutate(|f| {
            f.active = None;
            Ok(())
        })
    }

    pub fn restore_error(&self) -> Option<String> {
        self.lock().restore_error.clone()
    }

    fn note_restore(&self, e: Option<String>) {
        self.lock().restore_error = e;
    }

    /// `<app data>/fl_consent.json`, next to the store. None without an app data folder.
    pub fn consent_path(&self) -> Option<PathBuf> {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .map(|d| d.join(CONSENT_FILE))
    }

    fn read_consent(&self) -> Result<ConsentFile, String> {
        let Some(path) = self.consent_path() else {
            return Ok(ConsentFile::default());
        };
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<ConsentFile>(&bytes).map_err(|e| {
                format!("{CONSENT_FILE} is unreadable ({e}); fix or remove it before saving")
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConsentFile::default()),
            Err(e) => Err(format!("{CONSENT_FILE} could not be read ({e})")),
        }
    }

    fn write_consent(&self, c: &ConsentFile) -> Result<PathBuf, String> {
        let path = self
            .consent_path()
            .ok_or_else(|| "no app data folder to write the round consent into".to_string())?;
        let bytes = serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?;
        write_atomic(&path, &bytes)?;
        Ok(path)
    }

    /// The rounds this device consented to, as the worker would read them.
    pub fn consented_rounds(&self) -> Result<Vec<String>, String> {
        Ok(self.read_consent()?.rounds)
    }

    /// Withdraw consent for a round: removed from the worker's file first (it takes effect at the
    /// worker's next job), then recorded on the start.
    pub fn revoke_consent(&self, round_id: &str, now_ms: u64) -> Result<(), String> {
        let _serial = self.consent_lock.lock().unwrap_or_else(|e| e.into_inner());
        let rid = round_id.trim().to_ascii_lowercase();
        let live = |s: &StartRecord| {
            s.round_id.as_deref() == Some(rid.as_str()) && s.revoked_at_ms.is_none()
        };
        let mut c = self.read_consent()?;
        let before = c.rounds.len();
        c.rounds.retain(|r| !r.trim().eq_ignore_ascii_case(&rid));
        let in_file = c.rounds.len() != before;
        // A round in the worker file without a live start (the app stopped between writing the
        // consent and recording the start, or a hand edit) can still be withdrawn.
        if !in_file && !self.starts().iter().any(live) {
            return Err(format!("there is no consent for round {rid} to withdraw"));
        }
        if in_file {
            self.write_consent(&c)?;
        }
        self.mutate(|f| {
            for s in f.starts.iter_mut().filter(|s| live(s)) {
                s.revoked_at_ms = Some(now_ms);
            }
            Ok(())
        })
    }

    pub fn gate(&self, sha: &str) -> Option<AdapterGateRecord> {
        self.lock().file.gates.get(sha).cloned()
    }

    pub fn gates(&self) -> Vec<AdapterGateRecord> {
        self.lock().file.gates.values().cloned().collect()
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Where the coordinator URL in effect came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CoordinatorSource {
    Env,
    Settings,
    /// The env override is set but invalid: nothing is used (fail closed).
    Invalid,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorConfig {
    pub url: Option<String>,
    pub source: CoordinatorSource,
    pub settings_url: Option<String>,
    pub note: Option<String>,
}

/// The URL in effect: a valid env override, else the member's setting.
pub fn resolve_coordinator(fl: &FlRounds, env: Option<&str>) -> CoordinatorConfig {
    let settings_url = fl.coordinator_setting();
    if let Some(raw) = env.map(str::trim).filter(|s| !s.is_empty()) {
        return match normalize_coordinator_url(raw) {
            Ok(u) => CoordinatorConfig {
                url: Some(u),
                source: CoordinatorSource::Env,
                settings_url,
                note: Some(format!("{COORDINATOR_ENV} overrides the setting")),
            },
            Err(e) => CoordinatorConfig {
                url: None,
                source: CoordinatorSource::Invalid,
                settings_url,
                note: Some(format!("{COORDINATOR_ENV} is set but invalid: {e}")),
            },
        };
    }
    CoordinatorConfig {
        source: if settings_url.is_some() {
            CoordinatorSource::Settings
        } else {
            CoordinatorSource::None
        },
        url: settings_url.clone(),
        settings_url,
        note: None,
    }
}

// ---------------------------------------------------------------------------
// HIC-1 start
// ---------------------------------------------------------------------------

/// Called only after the member approved this exact plan (HIC-1). Re-reads the coordinator and
/// refuses if what they approved no longer holds, then records the authorization.
pub fn start_round(
    fl: &FlRounds,
    plan_hash: &str,
    http: &dyn CoordinatorHttp,
    now_ms: u64,
) -> Result<StartReceipt, String> {
    let _serial = fl.consent_lock.lock().unwrap_or_else(|e| e.into_inner());
    let plan = fl
        .lookup_plan(plan_hash)
        .ok_or_else(|| "no plan with that hash on this device; plan the round first".to_string())?;
    if !plan.can_start {
        return Err(plan.blockers.join(" "));
    }
    if now_ms.saturating_sub(plan.created_at_ms) > PLAN_TTL_MS {
        return Err(
            "this plan is more than 15 minutes old; plan again so you approve current numbers"
                .into(),
        );
    }
    if fl.starts().iter().any(|s| s.plan_hash == plan_hash) {
        return Err("you already authorized this plan".into());
    }
    let (url, planned) = match &plan.coordinator {
        CoordinatorView::Live { url, status } => (url.clone(), status.clone()),
        _ => return Err("the plan has no live coordinator".into()),
    };
    let now = read_coordinator(Some(&url), http);
    match &now {
        CoordinatorView::Live { status, .. }
            if matches!(status.phase, PoolPhase::Open | PoolPhase::Running)
                && status.settlement == planned.settlement => {}
        CoordinatorView::Live { status, .. } => {
            return Err(format!(
                "the coordinator changed since you approved (now {} waiting, {} running, settlement {:?}); plan again",
                status.pending, status.leased, status.settlement
            ))
        }
        _ => return Err("the coordinator changed since you approved: it cannot be read now; plan again".into()),
    }
    let round_id = plan.proposal.round_id.clone();
    let rec = StartRecord {
        plan_hash: plan_hash.to_string(),
        coordinator_url: url.clone(),
        requires: plan.proposal.requires,
        authorized_at_ms: now_ms,
        round_id: round_id.clone(),
        revoked_at_ms: None,
    };
    // Per-round consent (D-29) goes to the worker's file first; if recording the start then fails,
    // the consent is taken back, so the file never holds a round without a recorded approval.
    let mut consent_file = None;
    let mut previous_consent = None;
    if let Some(rid) = &round_id {
        if fl
            .starts()
            .iter()
            .any(|s| s.round_id.as_deref() == Some(rid.as_str()) && s.revoked_at_ms.is_none())
        {
            return Err(format!("you already consented to round {rid}"));
        }
        let c = fl.read_consent()?;
        let mut next = c.clone();
        if !next
            .rounds
            .iter()
            .any(|r| r.trim().eq_ignore_ascii_case(rid))
        {
            next.rounds.push(rid.clone());
        }
        consent_file = Some(fl.write_consent(&next)?);
        previous_consent = Some(c);
    }
    let recorded = fl.mutate(|f| {
        if f.starts.iter().any(|s| s.plan_hash == rec.plan_hash) {
            return Err("you already authorized this plan".to_string());
        }
        f.starts.push(rec.clone());
        Ok(())
    });
    if let Err(e) = recorded {
        if let Some(c) = previous_consent {
            if let Err(undo) = fl.write_consent(&c) {
                return Err(format!(
                    "{e}; the round consent could not be taken back ({undo}), remove it from {CONSENT_FILE}"
                ));
            }
        }
        return Err(e);
    }
    let note = match (&round_id, &consent_file) {
        (Some(rid), Some(path)) => format!(
            "Your approval for this exact plan is recorded, and your consent for round {rid} is written to {}. A device training worker started with {WORKER_CONSENT_ENV} set to that file takes part in this round only. This build does not bundle the worker, so no training has started from the app.",
            path.display()
        ),
        _ => "Your approval for this exact plan is recorded on this device. This build does not include the device training worker (HUP-S9.1/S9.2), so no training has started.".to_string(),
    };
    Ok(StartReceipt {
        plan_hash: rec.plan_hash,
        coordinator_url: url,
        authorized_at_ms: now_ms,
        training_started: false,
        note,
        round_id,
        consent_file: consent_file.map(|p| p.to_string_lossy().to_string()),
    })
}

// ---------------------------------------------------------------------------
// Eval scorecards + gate decision
// ---------------------------------------------------------------------------

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn rate(v: Option<f64>) -> Result<Option<f64>, String> {
    match v {
        None => Ok(None),
        Some(x) if x.is_finite() && (0.0..=1.0).contains(&x) => Ok(Some(x)),
        Some(_) => Err("a rate is outside 0..1".into()),
    }
}

/// A tool-call + injection scorecard (`scripts/eval-tools.mjs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsScore {
    pub model: String,
    pub dataset_version: String,
    pub n: u64,
    pub valid_tool_call_rate: Option<f64>,
    pub correct_tool_rate: Option<f64>,
    pub args_ok_rate: Option<f64>,
    pub injection_resist_rate: Option<f64>,
    #[serde(default)]
    pub adapter_sha256: Option<String>,
}

/// A Citrate QA scorecard (`scripts/eval-qa.mjs`, the `scorecard` member of its file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaScore {
    pub model: String,
    pub dataset_version: String,
    pub n: u64,
    pub pass_rate: f64,
    pub key_point_coverage: f64,
    pub citation_validity: Option<f64>,
    pub false_abstention_rate: Option<f64>,
    #[serde(default)]
    pub adapter_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireTools {
    model: String,
    dataset_version: String,
    n: u64,
    valid_tool_call_rate: Option<f64>,
    correct_tool_rate: Option<f64>,
    args_ok_rate: Option<f64>,
    injection_resist_rate: Option<f64>,
    failures: Vec<serde_json::Value>,
    #[serde(default)]
    adapter_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireQaInner {
    model: String,
    dataset_version: String,
    n: u64,
    pass_rate: f64,
    key_point_coverage: f64,
    citation_validity: Option<f64>,
    false_abstention_rate: Option<f64>,
    failures: Vec<serde_json::Value>,
    #[serde(default)]
    adapter_sha256: Option<String>,
}

#[derive(Deserialize)]
struct WireQa {
    scorecard: WireQaInner,
}

fn check_adapter_field(a: &Option<String>) -> Result<(), String> {
    match a {
        Some(s) if !is_sha256_hex(s) => Err("adapterSha256 is not a sha256 hex".into()),
        _ => Ok(()),
    }
}

pub fn parse_tools_scorecard(body: &str) -> Result<ToolsScore, String> {
    let w: WireTools = serde_json::from_str(body)
        .map_err(|_| "not a tool-call scorecard (scripts/eval-tools.mjs output)".to_string())?;
    // `failures` is required so an arbitrary JSON object with rates is not taken for a scorecard.
    let _ = w.failures.len();
    check_adapter_field(&w.adapter_sha256)?;
    Ok(ToolsScore {
        model: w.model,
        dataset_version: w.dataset_version,
        n: w.n,
        valid_tool_call_rate: rate(w.valid_tool_call_rate)?,
        correct_tool_rate: rate(w.correct_tool_rate)?,
        args_ok_rate: rate(w.args_ok_rate)?,
        injection_resist_rate: rate(w.injection_resist_rate)?,
        adapter_sha256: w.adapter_sha256.map(|s| s.to_ascii_lowercase()),
    })
}

pub fn parse_qa_scorecard(body: &str) -> Result<QaScore, String> {
    let w: WireQa = serde_json::from_str(body)
        .map_err(|_| "not a QA scorecard (scripts/eval-qa.mjs output)".to_string())?;
    let s = w.scorecard;
    // Required for the same reason as in the tool-call scorecard.
    let _ = s.failures.len();
    check_adapter_field(&s.adapter_sha256)?;
    Ok(QaScore {
        model: s.model,
        dataset_version: s.dataset_version,
        n: s.n,
        pass_rate: rate(Some(s.pass_rate))?.unwrap_or(0.0),
        key_point_coverage: rate(Some(s.key_point_coverage))?.unwrap_or(0.0),
        citation_validity: rate(s.citation_validity)?,
        false_abstention_rate: rate(s.false_abstention_rate)?,
        adapter_sha256: s.adapter_sha256.map(|s| s.to_ascii_lowercase()),
    })
}

/// The scorecards the gate compares: the base model alone, then with the adapter.
#[derive(Debug, Clone)]
pub struct EvalPair {
    pub base_tools: ToolsScore,
    pub candidate_tools: ToolsScore,
    pub base_qa: Option<QaScore>,
    pub candidate_qa: Option<QaScore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GateVerdict {
    Accept,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricDelta {
    pub metric: String,
    pub base: Option<f64>,
    pub candidate: Option<f64>,
    /// Signed so that positive is always better (false abstention is inverted).
    pub improvement: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateDecision {
    pub verdict: GateVerdict,
    pub reasons: Vec<String>,
    pub metrics: Vec<MetricDelta>,
    pub composite_base: f64,
    pub composite_candidate: f64,
}

/// (name, base, candidate, higher_is_better)
type MetricRow = (&'static str, Option<f64>, Option<f64>, bool);

fn metric_rows(p: &EvalPair) -> Vec<MetricRow> {
    let (b, c) = (&p.base_tools, &p.candidate_tools);
    let mut rows: Vec<MetricRow> = vec![
        (
            "validToolCallRate",
            b.valid_tool_call_rate,
            c.valid_tool_call_rate,
            true,
        ),
        (
            "correctToolRate",
            b.correct_tool_rate,
            c.correct_tool_rate,
            true,
        ),
        ("argsOkRate", b.args_ok_rate, c.args_ok_rate, true),
        (
            "injectionResistRate",
            b.injection_resist_rate,
            c.injection_resist_rate,
            true,
        ),
    ];
    if let (Some(bq), Some(cq)) = (&p.base_qa, &p.candidate_qa) {
        rows.push(("passRate", Some(bq.pass_rate), Some(cq.pass_rate), true));
        rows.push((
            "keyPointCoverage",
            Some(bq.key_point_coverage),
            Some(cq.key_point_coverage),
            true,
        ));
        rows.push((
            "citationValidity",
            bq.citation_validity,
            cq.citation_validity,
            true,
        ));
        rows.push((
            "falseAbstentionRate",
            bq.false_abstention_rate,
            cq.false_abstention_rate,
            false,
        ));
    }
    rows
}

fn comparable(
    reasons: &mut Vec<String>,
    what: &str,
    adapter_sha: &str,
    base: (&str, &str, u64, &Option<String>),
    cand: (&str, &str, u64, &Option<String>),
) {
    if base.0 != cand.0 {
        reasons.push(format!(
            "{what}: the base and candidate runs measured a different model ({} vs {})",
            base.0, cand.0
        ));
    }
    if base.1 != cand.1 || base.2 != cand.2 {
        reasons.push(format!(
            "{what}: the runs used different datasets ({} n={} vs {} n={})",
            base.1, base.2, cand.1, cand.2
        ));
    }
    if base.3.is_some() {
        reasons.push(format!(
            "{what}: the base run was made with an adapter loaded, so it is not a base run"
        ));
    }
    if cand.3.as_deref() != Some(adapter_sha) {
        reasons.push(format!(
            "{what}: the candidate run is not stamped with this adapter's sha256 (run the eval with --adapter-sha256)"
        ));
    }
}

/// Decide whether the adapter may be loaded. Accept only when the runs are comparable, nothing
/// got worse, and the mean of the shared metrics strictly improved.
pub fn decide_eval_gate(adapter_sha: &str, p: &EvalPair) -> GateDecision {
    let adapter_sha = adapter_sha.to_ascii_lowercase();
    let mut reasons = Vec::new();
    let (b, c) = (&p.base_tools, &p.candidate_tools);
    comparable(
        &mut reasons,
        "tool-call eval",
        &adapter_sha,
        (&b.model, &b.dataset_version, b.n, &b.adapter_sha256),
        (&c.model, &c.dataset_version, c.n, &c.adapter_sha256),
    );
    match (&p.base_qa, &p.candidate_qa) {
        (Some(bq), Some(cq)) => comparable(
            &mut reasons,
            "QA eval",
            &adapter_sha,
            (&bq.model, &bq.dataset_version, bq.n, &bq.adapter_sha256),
            (&cq.model, &cq.dataset_version, cq.n, &cq.adapter_sha256),
        ),
        (None, None) => {}
        _ => reasons
            .push("QA eval: give both the base and the candidate QA scorecard, or neither".into()),
    }

    let mut metrics = Vec::new();
    let (mut sum_b, mut sum_c, mut k) = (0.0f64, 0.0f64, 0u32);
    for (name, base, cand, higher) in metric_rows(p) {
        let orient = |v: f64| if higher { v } else { 1.0 - v };
        let improvement = match (base, cand) {
            (Some(bv), Some(cv)) => {
                let d = orient(cv) - orient(bv);
                sum_b += orient(bv);
                sum_c += orient(cv);
                k += 1;
                if d < -REGRESSION_TOLERANCE - 1e-12 {
                    reasons.push(format!("{name} got worse ({bv:.4} to {cv:.4})"));
                }
                Some(d)
            }
            (Some(_), None) => {
                reasons.push(format!("{name} is missing from the candidate run"));
                None
            }
            _ => None,
        };
        metrics.push(MetricDelta {
            metric: name.to_string(),
            base,
            candidate: cand,
            improvement,
        });
    }
    let (composite_base, composite_candidate) = if k == 0 {
        reasons.push("no metric is shared by both runs".into());
        (0.0, 0.0)
    } else {
        (sum_b / k as f64, sum_c / k as f64)
    };
    if composite_candidate - composite_base <= MIN_COMPOSITE_IMPROVEMENT + 1e-12 {
        reasons.push(format!(
            "the eval score did not improve ({composite_base:.4} to {composite_candidate:.4})"
        ));
    }
    GateDecision {
        verdict: if reasons.is_empty() {
            GateVerdict::Accept
        } else {
            GateVerdict::Reject
        },
        reasons,
        metrics,
        composite_base,
        composite_candidate,
    }
}

// ---------------------------------------------------------------------------
// Round results (local-devnet flow, citrate-chain docs/fl/FL_ROUND_V1.md)
// ---------------------------------------------------------------------------

/// The round an adapter came from, as its round result file states it. Every field was checked for
/// shape and for agreement between the chain, bundle and replay digests the file records. Core
/// does not read the ledger, so this is the file's own consistency, not an on-chain proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundBinding {
    pub round_id: String,
    pub record_digest: String,
    /// sha256 of the merged adapter file (FL_ROUND_V1 section 6 `adapter_hash`).
    pub adapter_sha256: String,
    pub chain_id: u64,
    pub ledger: String,
    pub participants: u64,
}

#[derive(Deserialize)]
struct WireRoundAggregate {
    adapter_sha256: String,
    round_id: String,
    record_digest: String,
    participants: u64,
}

#[derive(Deserialize)]
struct WireRoundReplay {
    round_id: String,
    record_digest: String,
    merged_adapter_checked: bool,
    mismatches: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct WireRoundDigests {
    chain: String,
    bundle: String,
    replay: String,
}

#[derive(Deserialize)]
struct WireRoundResult {
    chain_id: u64,
    ledger: String,
    round_id: String,
    aggregate: WireRoundAggregate,
    replay: WireRoundReplay,
    record_digest: WireRoundDigests,
    /// The round's ledger status. The devnet script writes it as `round0_status`.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    round0_status: Option<String>,
}

/// Read a round result strictly. Refused unless the round is Accepted, at least three devices
/// took part, the replay found no mismatch and checked the merged adapter, and every record
/// digest and round id in it agree.
pub fn parse_round_result(body: &str) -> Result<RoundBinding, String> {
    if body.len() > MAX_ROUND_RESULT_BYTES {
        return Err("the round result is too large".into());
    }
    let w: WireRoundResult = serde_json::from_str(body).map_err(|_| {
        "not a round result (the round tool's receipt: round_id, aggregate, replay, record_digest)"
            .to_string()
    })?;
    let rid = w.round_id.to_ascii_lowercase();
    if !is_b32_hex(&rid) {
        return Err("the round result's round id is not 0x followed by 64 hex digits".into());
    }
    if !is_address_hex(&w.ledger) {
        return Err("the round result's ledger is not an address".into());
    }
    let sha = w.aggregate.adapter_sha256.to_ascii_lowercase();
    if !is_sha256_hex(&sha) {
        return Err("the round result's adapter hash is not a sha256 hex".into());
    }
    if !w.aggregate.round_id.eq_ignore_ascii_case(&rid)
        || !w.replay.round_id.eq_ignore_ascii_case(&rid)
    {
        return Err("the round result names more than one round".into());
    }
    let digest = w.record_digest.chain.to_ascii_lowercase();
    if !is_b32_hex(&digest) {
        return Err("the round result's record digest is not 0x followed by 64 hex digits".into());
    }
    let all = [
        &w.record_digest.bundle,
        &w.record_digest.replay,
        &w.aggregate.record_digest,
        &w.replay.record_digest,
    ];
    if all.iter().any(|d| !d.eq_ignore_ascii_case(&digest)) {
        return Err(
            "the round's record digests disagree (chain, bundle and replay must match)".into(),
        );
    }
    if !w.replay.mismatches.is_empty() {
        return Err(format!(
            "the independent replay found {} mismatch(es) in this round",
            w.replay.mismatches.len()
        ));
    }
    if !w.replay.merged_adapter_checked {
        return Err("the independent replay did not check the merged adapter".into());
    }
    if w.aggregate.participants < MIN_ROUND_PARTICIPANTS {
        return Err(format!(
            "the round had {} devices; at least {MIN_ROUND_PARTICIPANTS} are required",
            w.aggregate.participants
        ));
    }
    match w.status.or(w.round0_status).as_deref() {
        Some("Accepted") => {}
        Some(other) => {
            return Err(format!(
                "the round is {}, not Accepted",
                if other.len() <= 32 {
                    other
                } else {
                    "in another state"
                }
            ))
        }
        None => return Err("the round result does not say the round was accepted".into()),
    }
    Ok(RoundBinding {
        round_id: rid,
        record_digest: digest,
        adapter_sha256: sha,
        chain_id: w.chain_id,
        ledger: w.ledger.to_ascii_lowercase(),
        participants: w.aggregate.participants,
    })
}

// ---------------------------------------------------------------------------
// Adapter file, gate record, load authorization
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterGateRequest {
    pub adapter_path: String,
    /// The hash the round published for its adapter.
    pub expected_sha256: String,
    pub base_tools_path: String,
    pub candidate_tools_path: String,
    #[serde(default)]
    pub base_qa_path: Option<String>,
    #[serde(default)]
    pub candidate_qa_path: Option<String>,
    /// The round result for the round that produced this adapter (optional). With one, the
    /// expected hash may be left empty: it is the round's merged adapter.
    #[serde(default)]
    pub round_result_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterGateRecord {
    pub adapter_sha256: String,
    pub adapter_path: String,
    /// The model name the scorecards measured; the served base must match it to load.
    pub base_model: String,
    pub decided_at_ms: u64,
    /// The round this adapter came from, when a round result was given.
    #[serde(default)]
    pub round: Option<RoundBinding>,
    pub decision: GateDecision,
}

const GGUF_MAGIC: &[u8; 4] = b"GGUF";

/// SHA-256 of a file, streamed, and whether it starts with the GGUF magic.
fn hash_file(path: &Path) -> Result<(String, bool), String> {
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut head = Vec::with_capacity(4);
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if head.len() < 4 {
            let take = (4 - head.len()).min(n);
            head.extend_from_slice(&buf[..take]);
        }
        h.update(&buf[..n]);
    }
    Ok((hex::encode(h.finalize()), head.as_slice() == GGUF_MAGIC))
}

fn read_capped(path: &str) -> Result<String, String> {
    read_capped_to(path, MAX_SCORECARD_BYTES, "scorecard")
}

fn read_capped_to(path: &str, cap: u64, what: &str) -> Result<String, String> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| format!("cannot read {path}: {e}"))?;
    if !meta.is_file() || meta.len() > cap {
        return Err(format!("{path} is not a {what} file of a sensible size"));
    }
    std::fs::read_to_string(p).map_err(|e| format!("cannot read {path}: {e}"))
}

/// Hash and check the adapter, read the four scorecards, decide.
pub fn evaluate_adapter(
    req: &AdapterGateRequest,
    now_ms: u64,
) -> Result<AdapterGateRecord, String> {
    let round = match req
        .round_result_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => Some(parse_round_result(&read_capped_to(
            p,
            MAX_ROUND_RESULT_BYTES as u64,
            "round result",
        )?)?),
        None => None,
    };
    let typed = req.expected_sha256.trim().to_ascii_lowercase();
    let expected = match &round {
        Some(r) if typed.is_empty() => r.adapter_sha256.clone(),
        Some(r) if typed != r.adapter_sha256 => {
            return Err(format!(
                "the round result names merged adapter {}, not {typed}",
                r.adapter_sha256
            ))
        }
        _ => typed,
    };
    if !is_sha256_hex(&expected) {
        return Err("the expected adapter hash must be a sha256 hex string (64 characters)".into());
    }
    let path = PathBuf::from(&req.adapter_path);
    let (actual, is_gguf) = hash_file(&path)?;
    if !is_gguf {
        return Err("the adapter is not a GGUF file".into());
    }
    if actual != expected {
        return Err(format!(
            "the adapter's sha256 {actual} does not match the expected {expected}"
        ));
    }
    let pair = EvalPair {
        base_tools: parse_tools_scorecard(&read_capped(&req.base_tools_path)?)?,
        candidate_tools: parse_tools_scorecard(&read_capped(&req.candidate_tools_path)?)?,
        base_qa: match &req.base_qa_path {
            Some(p) => Some(parse_qa_scorecard(&read_capped(p)?)?),
            None => None,
        },
        candidate_qa: match &req.candidate_qa_path {
            Some(p) => Some(parse_qa_scorecard(&read_capped(p)?)?),
            None => None,
        },
    };
    let decision = decide_eval_gate(&actual, &pair);
    Ok(AdapterGateRecord {
        adapter_sha256: actual,
        adapter_path: path.to_string_lossy().to_string(),
        base_model: pair.base_tools.model.clone(),
        decided_at_ms: now_ms,
        round,
        decision,
    })
}

fn model_stem(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    lower.strip_suffix(".gguf").unwrap_or(&lower).to_string()
}

/// Where an accepted adapter is served from: `<store>/<sha256>.gguf`, a copy the app owns.
pub fn adapter_store_path(store: &Path, sha: &str) -> PathBuf {
    store.join(format!("{}.gguf", sha.to_ascii_lowercase()))
}

/// The adapter path llama-server may load, or why not.
///
/// llama-server re-reads `--lora` on every (crash) restart, so it is never pointed at the file the
/// member picked: the verified bytes are copied into the app's adapter store under their hash and
/// served from there. A copy already in place is re-hashed before reuse; a bad copy is removed.
pub fn authorize_load(
    fl: &FlRounds,
    sha: &str,
    current_model_file: &str,
    store: &Path,
) -> Result<PathBuf, String> {
    let sha = sha.trim().to_ascii_lowercase();
    if !is_sha256_hex(&sha) {
        return Err("the adapter hash must be a sha256 hex string (64 characters)".into());
    }
    let rec = fl
        .gate(&sha)
        .ok_or_else(|| "this adapter has not been through the eval gate".to_string())?;
    if rec.decision.verdict != GateVerdict::Accept {
        return Err(format!(
            "the eval gate rejected this adapter: {}",
            rec.decision.reasons.join("; ")
        ));
    }
    if model_stem(&rec.base_model) != model_stem(current_model_file) {
        return Err(format!(
            "the adapter was measured on base model {} but {} is selected; select that base model first",
            rec.base_model, current_model_file
        ));
    }
    let dest = adapter_store_path(store, &sha);
    if dest.exists() {
        match hash_file(&dest) {
            Ok((h, true)) if h == sha => return Ok(dest),
            _ => {
                std::fs::remove_file(&dest).map_err(|e| {
                    format!("a stored adapter copy is damaged and could not be removed ({e})")
                })?;
            }
        }
    }
    std::fs::create_dir_all(store).map_err(|e| e.to_string())?;
    let part = store.join(format!("{sha}.gguf.part"));
    std::fs::copy(&rec.adapter_path, &part)
        .map_err(|e| format!("cannot copy the adapter into the app's store ({e})"))?;
    match hash_file(&part) {
        Ok((h, true)) if h == sha => {}
        _ => {
            let _ = std::fs::remove_file(&part);
            return Err("the adapter file changed since the eval gate; run the gate again".into());
        }
    }
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// After a restart: the remembered adapter's served path, if it may be put back now. `None` when
/// nothing is remembered or another base is served (it stays remembered for that base). Goes
/// through [`authorize_load`], so the latest gate record must still be ACCEPT for this base and
/// the copy is re-hashed.
pub fn restore_active(
    fl: &FlRounds,
    current_model_file: &str,
    store: &Path,
) -> Result<Option<PathBuf>, String> {
    let Some(a) = fl.active() else {
        return Ok(None);
    };
    if model_stem(&a.base_model) != model_stem(current_model_file) {
        return Ok(None);
    }
    authorize_load(fl, &a.sha256, current_model_file, store).map(Some)
}

/// After a new gate record for an adapter: must the served adapter be dropped? Yes when it is this
/// adapter and the new record no longer allows it on the served base (a REJECT, or an ACCEPT
/// measured on a different base). Found by TLC (`FlRoundGate.tla`, LoadedIsAccepted).
pub fn must_unload_after_gate(
    rec: &AdapterGateRecord,
    served_lora: Option<&Path>,
    store: &Path,
    current_model_file: &str,
) -> bool {
    if served_lora != Some(adapter_store_path(store, &rec.adapter_sha256).as_path()) {
        return false;
    }
    rec.decision.verdict != GateVerdict::Accept
        || model_stem(&rec.base_model) != model_stem(current_model_file)
}

/// `<app data>/adapters`.
fn adapter_store(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("adapters"))
}

// ---------------------------------------------------------------------------
// Tauri commands (async, off the main thread)
// ---------------------------------------------------------------------------

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn state(app: &tauri::AppHandle) -> Result<tauri::State<'_, FlRounds>, String> {
    tauri::Manager::try_state::<FlRounds>(app)
        .ok_or_else(|| "internal: managed state unavailable".to_string())
}

fn env_coordinator() -> Option<String> {
    std::env::var(COORDINATOR_ENV).ok()
}

/// Build the managed state over `<app data>/fl_rounds.json`.
pub fn build_state(app: &tauri::AppHandle) -> FlRounds {
    use tauri::Manager;
    match app.path().app_data_dir() {
        Ok(dir) => FlRounds::with_store(dir.join(STORE_FILE)),
        Err(_) => FlRounds::default(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlOverview {
    pub config: CoordinatorConfig,
    pub starts: Vec<StartRecord>,
    pub gates: Vec<AdapterGateRecord>,
    /// The adapter llama-server is (or will be) started with, if any.
    pub active_adapter: Option<String>,
    /// The adapter remembered for re-applying after a restart.
    pub remembered_adapter: Option<ActiveAdapter>,
    /// Why the last re-apply did not happen, if it failed.
    pub restore_error: Option<String>,
    /// The rounds this device consented to (the worker's file), and where that file is.
    pub consented_rounds: Vec<String>,
    pub consent_file: Option<String>,
    pub consent_error: Option<String>,
    pub store_error: Option<String>,
}

/// **Command — fl_overview.** The coordinator config, start authorizations, gate records and
/// the loaded adapter. Local only.
#[tauri::command]
pub async fn fl_overview(app_h: tauri::AppHandle) -> Result<FlOverview, String> {
    crate::blocking::off_main(move || {
        let fl = state(&app_h)?;
        let serve = tauri::Manager::try_state::<crate::serve::ServeState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let (consented_rounds, consent_error) = match fl.consented_rounds() {
            Ok(r) => (r, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        Ok(FlOverview {
            config: resolve_coordinator(&fl, env_coordinator().as_deref()),
            starts: fl.starts(),
            gates: fl.gates(),
            active_adapter: serve.0.lora().map(|p| p.to_string_lossy().to_string()),
            remembered_adapter: fl.active(),
            restore_error: fl.restore_error(),
            consented_rounds,
            consent_file: fl.consent_path().map(|p| p.to_string_lossy().to_string()),
            consent_error,
            store_error: fl.load_error(),
        })
    })
    .await
}

/// **Command — fl_coordinator_set.** Persist (or clear, with `null`) the coordinator base URL.
#[tauri::command]
pub async fn fl_coordinator_set(
    app_h: tauri::AppHandle,
    url: Option<String>,
) -> Result<CoordinatorConfig, String> {
    crate::blocking::off_main(move || {
        let fl = state(&app_h)?;
        fl.set_coordinator_setting(url.as_deref())?;
        Ok(resolve_coordinator(&fl, env_coordinator().as_deref()))
    })
    .await
}

/// **Command — fl_round_plan.** Read the coordinator, probe this device, and build the plan and
/// its plain-words explanation. Remembers the plan so a start can name it. Read-only.
#[tauri::command]
pub async fn fl_round_plan(
    app_h: tauri::AppHandle,
    proposal: Option<RoundProposal>,
) -> Result<RoundPlan, String> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        let fl = state(&app_h)?;
        let serve = app_h
            .try_state::<crate::serve::ServeState>()
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let cfg = resolve_coordinator(&fl, env_coordinator().as_deref());
        let view = read_coordinator(cfg.url.as_deref(), &UreqCoordinatorHttp);
        let device = match app_h.path().app_data_dir() {
            Ok(dir) => {
                let facts = crate::tier::probe(&dir);
                let tier = crate::tier::recommend_tier(&facts).tier;
                DeviceFit::from_facts(&facts, Some(tier.id()))
            }
            Err(_) => DeviceFit {
                tier: None,
                accelerator: None,
            },
        };
        let plan = build_plan(
            proposal.unwrap_or_default(),
            view,
            &serve.0.current_model_file(),
            &device,
            now_ms(),
        )?;
        fl.remember_plan(plan.clone());
        Ok(plan)
    })
    .await
}

/// **Command — fl_round_plan_lookup.** A plan core built earlier, by hash (for the approval card).
#[tauri::command]
pub async fn fl_round_plan_lookup(
    app_h: tauri::AppHandle,
    plan_hash: String,
) -> Result<RoundPlan, String> {
    crate::blocking::off_main(move || {
        let fl = state(&app_h)?;
        fl.lookup_plan(plan_hash.trim()).ok_or_else(|| {
            "no plan with that hash on this device; plan the round first".to_string()
        })
    })
    .await
}

/// **Command — fl_round_start.** Record the member's HIC-1 approval of one exact plan. The UI
/// calls this only after the member clicked Approve on the plan's card.
#[tauri::command]
pub async fn fl_round_start(
    app_h: tauri::AppHandle,
    plan_hash: String,
) -> Result<StartReceipt, String> {
    crate::blocking::off_main(move || {
        let fl = state(&app_h)?;
        start_round(&fl, plan_hash.trim(), &UreqCoordinatorHttp, now_ms())
    })
    .await
}

/// **Command — fl_round_consent_revoke.** Withdraw this device's consent for one round. Lowers
/// authority only, so it needs no approval card.
#[tauri::command]
pub async fn fl_round_consent_revoke(
    app_h: tauri::AppHandle,
    round_id: String,
) -> Result<Vec<String>, String> {
    crate::blocking::off_main(move || {
        let fl = state(&app_h)?;
        fl.revoke_consent(&round_id, now_ms())?;
        fl.consented_rounds()
    })
    .await
}

/// **Command — fl_adapter_gate.** Hash the adapter, compare the eval scorecards, record the
/// verdict bound to the hash. If that adapter is being served and the new record no longer allows
/// it on the served base, it is unloaded.
#[tauri::command]
pub async fn fl_adapter_gate(
    app_h: tauri::AppHandle,
    request: AdapterGateRequest,
) -> Result<AdapterGateRecord, String> {
    crate::blocking::off_main(move || {
        let fl = state(&app_h)?;
        let rec = evaluate_adapter(&request, now_ms())?;
        let _serial = adapter_lock();
        fl.record_gate(rec.clone())?;
        if let Some(serve) = tauri::Manager::try_state::<crate::serve::ServeState>(&app_h) {
            let store = adapter_store(&app_h)?;
            let lora = serve.0.lora();
            if must_unload_after_gate(&rec, lora.as_deref(), &store, &serve.0.current_model_file())
            {
                restart_with_lora(&app_h, &serve, LoraChoice::None)?;
            }
        }
        Ok(rec)
    })
    .await
}

/// Gate decisions, adapter loads and unloads run one at a time, so a reject recorded while a load
/// is in progress is seen by that load (and a load never lands after the reject that unloaded it).
fn adapter_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

/// What to serve: no adapter, or one authorized for the named base model file.
enum LoraChoice {
    None,
    For {
        path: PathBuf,
        base_model_file: String,
    },
}

fn restart_with_lora(
    app: &tauri::AppHandle,
    serve: &tauri::State<'_, crate::serve::ServeState>,
    lora: LoraChoice,
) -> Result<(), String> {
    let previous = serve.0.lora();
    match lora {
        LoraChoice::None => serve.0.set_lora(None),
        LoraChoice::For {
            path,
            base_model_file,
        } => serve.0.set_lora_for_model(path, &base_model_file)?,
    }
    if !serve.0.is_running() {
        return Ok(());
    }
    let path = serve.0.current_model_path();
    let ready = match (path.parent(), path.file_name()) {
        (Some(dir), Some(file)) => crate::model::is_file_ready(dir, &file.to_string_lossy()),
        _ => false,
    };
    let plan = crate::serve_plan::plan_or_unsized(crate::serve_plan::plan_for_model(app, &path));
    // Same base model path, so the restart keeps the adapter just set. A refused restart (model
    // not ready) stops nothing, and the previous adapter setting is put back.
    if let Err(e) = serve.0.select_model_planned(path, ready, plan) {
        serve.0.set_lora(previous);
        return Err(e.to_string());
    }
    Ok(())
}

/// **Command — fl_adapter_load.** Load an ACCEPTED adapter into llama-server (`--lora`),
/// restarting it if running. Refuses otherwise.
#[tauri::command]
pub async fn fl_adapter_load(app_h: tauri::AppHandle, sha256: String) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let fl = state(&app_h)?;
        let serve = tauri::Manager::try_state::<crate::serve::ServeState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let store = adapter_store(&app_h)?;
        let _serial = adapter_lock();
        let base = serve.0.current_model_file();
        let path = authorize_load(&fl, &sha256, &base, &store)?;
        restart_with_lora(
            &app_h,
            &serve,
            LoraChoice::For {
                path: path.clone(),
                base_model_file: base.clone(),
            },
        )?;
        fl.set_active(&sha256, &base)?;
        fl.note_restore(None);
        Ok(path.to_string_lossy().to_string())
    })
    .await
}

/// **Command — fl_adapter_unload.** Serve the base model alone again.
#[tauri::command]
pub async fn fl_adapter_unload(app_h: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let serve = tauri::Manager::try_state::<crate::serve::ServeState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let _serial = adapter_lock();
        restart_with_lora(&app_h, &serve, LoraChoice::None)?;
        let fl = state(&app_h)?;
        fl.note_restore(None);
        fl.forget_active()
    })
    .await
}

/// Before the local model server starts: put the remembered adapter back if it still passes the
/// load checks for the base about to be served. Never fails the start; a refusal is kept for
/// `fl_overview`. Runs only while the server is stopped and no adapter is set.
pub fn reapply_before_start(app: &tauri::AppHandle) {
    let (Some(fl), Some(serve)) = (
        tauri::Manager::try_state::<FlRounds>(app),
        tauri::Manager::try_state::<crate::serve::ServeState>(app),
    ) else {
        return;
    };
    if serve.0.is_running() || serve.0.lora().is_some() {
        return;
    }
    // One at a time with gate decisions, loads and unloads.
    let _serial = adapter_lock();
    match adapter_store(app) {
        Ok(store) => reapply_into(&fl, &serve.0, &store),
        Err(e) => fl.note_restore(Some(e)),
    }
}

/// The body of [`reapply_before_start`] over an explicit server manager and adapter store.
pub fn reapply_into(fl: &FlRounds, serve: &crate::serve::LlamaServerManager, store: &Path) {
    if serve.is_running() || serve.lora().is_some() {
        return;
    }
    let base = serve.current_model_file();
    match restore_active(fl, &base, store) {
        // Bound to the base it was checked for, as a load is.
        Ok(Some(path)) => match serve.set_lora_for_model(path, &base) {
            Ok(()) => fl.note_restore(None),
            Err(e) => fl.note_restore(Some(format!(
                "The adapter you loaded earlier was not put back: {e}"
            ))),
        },
        Ok(None) => fl.note_restore(None),
        Err(e) => {
            eprintln!("citrate-core: fl_rounds: the remembered adapter was not re-applied: {e}");
            fl.note_restore(Some(format!(
                "The adapter you loaded earlier was not put back: {e}"
            )));
        }
    }
}

/// Does selecting `next` drop the served adapter? Yes when one is set and the base changes.
pub fn base_switch_drops_adapter(lora: Option<&Path>, current: &Path, next: &Path) -> bool {
    lora.is_some() && current != next
}

/// After the member switched the base model while an adapter was loaded: the adapter belongs to
/// the old base, so it is no longer remembered.
pub fn forget_after_base_switch(app: &tauri::AppHandle) {
    if let Some(fl) = tauri::Manager::try_state::<FlRounds>(app) {
        if let Err(e) = fl.forget_active() {
            eprintln!(
                "citrate-core: fl_rounds: could not forget the adapter after a base switch: {e}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    include!("fl_rounds_tests.rs");
}
