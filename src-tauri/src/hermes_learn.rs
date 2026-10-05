//! HUP-S3.4 (core half) — Hermes learns only what is proven (US-3.4).
//!
//! The runtime half lives in the Hermes sidecar (`citrate-agent-runtime`: `agent-learn` plus the
//! sidecar's learn routes). A proposal can only be made from a workflow run whose verifiers all
//! passed; the member accepts or rejects it here, and every decision is written to the sidecar's
//! HIC decision log before anything is saved.
//!
//! What core does:
//!
//! - **Commands** over the sidecar routes: run a declarative workflow in a session and read its
//!   verdicts; propose, list, accept, reject. Ids are validated before they reach a URL. The member
//!   id recorded with a decision is the member's wallet address (or `local-member` when no wallet is
//!   unlocked); the webview never supplies it.
//! - **Skills.** An accepted skill is written by the sidecar to the member's skills folder
//!   (`<app data>/hermes/skills/<name>/SKILL.md`). Core passes that folder to the sidecar as
//!   `CITRATE_HERMES_SKILLS` as well, and the sidecar reloads its skills library on accept, so the
//!   skill is offered to the next session without a restart (`skillsReloaded` in the answer).
//! - **Memories.** An accepted memory comes back as a typed record. Core keeps it in the
//!   learned-memory ledger (`<app data>/hermes/learned-memories.json`, keyed by proposal id, so a
//!   second accept of the same proposal is not a duplicate) and stores it in the member's memory
//!   graph (`memory.assert` into the `personal` tenant). A contradiction is Belnap `both`: the new
//!   memory AND the one it contradicts are marked `both` (unresolved) in the ledger, and the two
//!   graph nodes are linked by a quarantined `contradicts` edge. Nothing is merged or overwritten.
//!   Until the member resolves it, recall and search leave both sides out ([`RecallHide`], read
//!   by `memory.rs` and the Hermes memory bridge on every call).
//!   When the memory daemon is not running, the memory waits in the ledger as `pending` and is
//!   stored by `hermes_learn_store_pending`.
//! - **Resolving a contradiction** is the member's call (HIC-1, recorded by the sidecar first):
//!   keep one memory, retract the other. The ledger marks the retracted one Belnap `false` (kept
//!   for the record) and drops it from every other memory's contradictions. Only the KEPT memory
//!   becomes `true` again, and only when nothing else contradicts it; it is stored again as
//!   settled and its new graph node supersedes the old nodes (a confirmed `supersedes` edge).
//!   If the route's answer is lost, the next sync with the sidecar's proposal list applies it
//!   (a retracted proposal names the one kept), and the sync settles a memory left `both` with
//!   nothing to contradict only when the sidecar shows it standing. Model:
//!   citrate-agent-runtime `agent-learn/formal/ContradictionResolve.tla`. One side may be a memory
//!   core held before (`memory:<id>`, a sidecar known memory): keeping the learned one sets it
//!   aside (the sidecar lists it in the proposal's `set_aside`), keeping it retracts the learned one.
//! - **Publishing** an accepted skill to the on-chain SkillRegistry is an HIC-1 action: the sidecar
//!   records the decision and builds calldata only, core checks the payload (target, owner, chain,
//!   selector, exact calldata, and the registry's `skillHashOf` id) and opens a PENDING
//!   SignatureCeremony; the member signs and
//!   sends there (Rule 3). Before that, core pins the accepted `SKILL.md` to the local IPFS node,
//!   reads it back, and passes its CID as the registry's `manifestCID` (the payload must carry
//!   it). It is OFF ([`SKILL_PUBLISH_ENABLED`]) pending owner sign-off, and the UI says so.
//!
//! Rule 1: a memory that could not be stored says so (`pending` / `failed` with the reason); the
//! publish button says why it is disabled. Rule 8: no unwrap/expect.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::hermes::{manager, ControlResp, HermesManager};

/// The learn data folder (sidecar env).
pub const LEARN_DIR_ENV: &str = "CITRATE_HERMES_LEARN_DIR";
/// The member's skills folder (sidecar env).
pub const LEARN_SKILLS_DIR_ENV: &str = "CITRATE_HERMES_LEARN_SKILLS_DIR";
/// The instruction-skill folders the sidecar offers in sessions (sidecar env, HUP-S3.2).
pub const SKILLS_ENV: &str = "CITRATE_HERMES_SKILLS";

/// Publishing learned skills to the SkillRegistry on 40204. **Pending owner sign-off.** The
/// recommended default (fan-out 7) is to enable it once the redeployed SkillRegistry
/// (citrate-chain PR #272, `abi.encode` skill ids, which this module and the runtime now follow)
/// is live on 40204 and in the address book. While this is `false` the publish button is
/// disabled with that note; nothing else changes.
pub const SKILL_PUBLISH_ENABLED: bool = false;

/// Explicit gas for `registerSkill` (strings plus a tags array written to storage). A calldata tx
/// must carry explicit gas. Conservative placeholder, pending a measured value on 40204.
const REGISTER_SKILL_GAS: u64 = 1_200_000;
/// `registerSkill(string,string,string,string,string[])`.
const REGISTER_SKILL_SELECTOR: &str = "2a996145";
/// The memory tenant learned memories go to.
const LEARN_TENANT: &str = crate::memory::PERSONAL_TENANT;
/// Most learned memories kept in the ledger.
pub const MAX_LEDGER: usize = 10_000;
/// The ledger's schema tag.
const LEDGER_SCHEMA: &str = "citrate.core.learned-memories.v1";
/// The sidecar's resolution record schema.
const RESOLUTION_SCHEMA: &str = "citrate.learn.resolve.v1";
/// Longest CID accepted from the IPFS node for a skill pin.
const MAX_CID_LEN: usize = 128;

/// HUP-S3.2: the `skills.lock` shipped beside the reviewed third-party skills (sidecar env).
pub const SKILLS_LOCK_ENV: &str = "CITRATE_HERMES_SKILLS_LOCK";
/// HUP-S3.2: the staged reviewed third-party skills, `<root>/<source>/<path>/` (sidecar env).
pub const SKILLS_THIRD_PARTY_ENV: &str = "CITRATE_HERMES_SKILLS_THIRD_PARTY";

/// HUP-S3.2 (US-3.2 AC2): every place the sidecar's one SKILL.md loader reads skills from, besides
/// the learned-skills folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSources {
    /// The member's saved skills (`skill_write`, the Agent surface): `skills_local.rs`.
    pub authored: PathBuf,
    /// Citrate's own bundled `SKILL.md` skills (resource `skills/`).
    pub first_party: Option<PathBuf>,
    /// The reviewed third-party skills: (`skills.lock`, staged tree) (resource `skills-bundle/`).
    pub third_party: Option<(PathBuf, PathBuf)>,
}

/// The sidecar env for every skill source, in precedence order (first wins): the learned skills,
/// the member's saved skills, Citrate's bundled skills, then the reviewed third-party skills (a
/// separate locked source the sidecar checks file by file against `skills.lock`). A bundled
/// source that is not staged in this build is left out rather than pointing at nothing.
pub fn skills_env(learned: Option<&Path>, s: &SkillSources) -> Vec<(String, String)> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(l) = learned {
        dirs.push(l.to_path_buf());
    }
    dirs.push(s.authored.clone());
    if let Some(fp) = s.first_party.as_ref().filter(|p| p.is_dir()) {
        dirs.push(fp.clone());
    }
    // A path the platform list cannot carry (it holds the separator) is left out.
    dirs.retain(|d| std::env::join_paths([d]).is_ok());
    let mut env = Vec::new();
    if let Ok(joined) = std::env::join_paths(&dirs) {
        env.push((SKILLS_ENV.to_string(), joined.to_string_lossy().to_string()));
    }
    if let Some((lock, root)) = &s.third_party {
        if lock.is_file() && root.is_dir() {
            env.push((
                SKILLS_LOCK_ENV.to_string(),
                lock.to_string_lossy().to_string(),
            ));
            env.push((
                SKILLS_THIRD_PARTY_ENV.to_string(),
                root.to_string_lossy().to_string(),
            ));
        }
    }
    env
}

/// The sidecar env for a learn folder and a skills folder.
pub fn learn_env(learn_dir: &Path, skills_dir: &Path) -> Vec<(String, String)> {
    let skills = skills_dir.to_string_lossy().to_string();
    vec![
        (
            LEARN_DIR_ENV.to_string(),
            learn_dir.to_string_lossy().to_string(),
        ),
        (LEARN_SKILLS_DIR_ENV.to_string(), skills.clone()),
        (SKILLS_ENV.to_string(), skills),
    ]
}

// ---------------------------------------------------------------------------
// Ids and control calls
// ---------------------------------------------------------------------------

/// A learn proposal id: `lp-` + 24 lowercase hex.
pub fn valid_proposal_id(id: &str) -> Result<(), String> {
    let ok = id.len() == 27
        && id.starts_with("lp-")
        && id[3..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if ok {
        Ok(())
    } else {
        Err("invalid proposal id".into())
    }
}

/// The prefix of a memory core held before Hermes learned anything about it (a sidecar
/// `known_memories` id). A learned memory can contradict one; the member resolves it like a
/// contradiction between two learned memories.
pub const KNOWN_MEMORY_PREFIX: &str = "memory:";

/// `memory:<id>`: 1..=128 printable ASCII bytes after the prefix, no spaces.
pub fn valid_known_ref(r: &str) -> bool {
    r.strip_prefix(KNOWN_MEMORY_PREFIX).is_some_and(|id| {
        !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_graphic())
    })
}

/// One side of a resolution: a learned proposal id or a known memory (`memory:<id>`).
fn valid_memory_ref(r: &str) -> Result<(), String> {
    if valid_known_ref(r) {
        Ok(())
    } else {
        valid_proposal_id(r)
    }
}

/// A workflow run id: `wr-` + digits.
pub fn valid_run_id(id: &str) -> Result<(), String> {
    let digits = id.strip_prefix("wr-").unwrap_or("");
    if !digits.is_empty() && digits.len() <= 20 && digits.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err("invalid workflow run id".into())
    }
}

/// A 2xx body, or `LEARN_REFUSED: <the sidecar's reason>`.
fn learn_check(resp: ControlResp) -> Result<Value, String> {
    if (200..300).contains(&resp.status) {
        return serde_json::from_str(&resp.body)
            .map_err(|e| format!("hermes learn: bad response: {e}"));
    }
    let reason = serde_json::from_str::<Value>(&resp.body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| format!("the sidecar answered {}", resp.status));
    Err(format!(
        "LEARN_REFUSED: {}",
        reason.chars().take(300).collect::<String>()
    ))
}

fn get(m: &HermesManager, path: &str) -> Result<Value, String> {
    learn_check(m.control_get(path).map_err(|e| e.to_string())?)
}

fn post(m: &HermesManager, path: &str, body: &Value) -> Result<Value, String> {
    learn_check(
        m.control_post(path, &body.to_string())
            .map_err(|e| e.to_string())?,
    )
}

/// Start a declarative workflow in a session; returns the run id.
pub fn workflow_run(
    m: &HermesManager,
    session_id: &str,
    spec_json: &str,
) -> Result<String, String> {
    crate::hermes::valid_session_id(session_id)?;
    let spec: Value =
        serde_json::from_str(spec_json).map_err(|_| "the workflow must be JSON".to_string())?;
    let v = post(m, &format!("/sessions/{session_id}/workflows"), &spec)?;
    let run = v
        .get("run_id")
        .and_then(|r| r.as_str())
        .unwrap_or_default()
        .to_string();
    valid_run_id(&run)?;
    Ok(run)
}

/// A workflow run's state, verdicts and evidence.
pub fn workflow_status(m: &HermesManager, session_id: &str, run_id: &str) -> Result<Value, String> {
    crate::hermes::valid_session_id(session_id)?;
    valid_run_id(run_id)?;
    get(m, &format!("/sessions/{session_id}/workflows/{run_id}"))
}

/// The sidecar's learn status (`{enabled, pending, ...}`).
pub fn learn_status_raw(m: &HermesManager) -> Result<Value, String> {
    get(m, "/learn/status")
}

/// Proposals waiting for the member (or every kept one with `all`).
pub fn learn_list(m: &HermesManager, all: bool) -> Result<Value, String> {
    get(
        m,
        if all {
            "/learn/proposals?all=true"
        } else {
            "/learn/proposals"
        },
    )
}

/// Propose from a verified run of a session.
pub fn learn_propose(
    m: &HermesManager,
    session_id: &str,
    run_id: &str,
    content: &Value,
) -> Result<Value, String> {
    crate::hermes::valid_session_id(session_id)?;
    valid_run_id(run_id)?;
    post(
        m,
        "/learn/proposals",
        &json!({ "session_id": session_id, "run_id": run_id, "content": content }),
    )
}

/// The member accepts (with the conflicts they acknowledged). Returns the sidecar's answer.
pub fn learn_accept_raw(
    m: &HermesManager,
    id: &str,
    member: &str,
    acknowledged: &[String],
) -> Result<Value, String> {
    valid_proposal_id(id)?;
    post(
        m,
        &format!("/learn/proposals/{id}/accept"),
        &json!({ "member": member, "acknowledged_conflicts": acknowledged }),
    )
}

/// The member resolves a contradiction: keep `keep`, retract `retract` (HIC-1, recorded by the
/// sidecar before anything changes). Returns the sidecar's resolution record.
pub fn learn_resolve(
    m: &HermesManager,
    keep: &str,
    retract: &str,
    member: &str,
) -> Result<Value, String> {
    valid_memory_ref(keep)?;
    valid_memory_ref(retract)?;
    if keep == retract {
        return Err("a memory cannot be kept and retracted at once".into());
    }
    if valid_known_ref(keep) && valid_known_ref(retract) {
        return Err("one side of a resolution must be a learned memory".into());
    }
    let v = post(
        m,
        "/learn/memories/resolve",
        &json!({ "member": member, "keep": keep, "retract": retract }),
    )?;
    v.get("resolution")
        .cloned()
        .ok_or_else(|| "hermes learn: the resolve answer has no resolution".to_string())
}

/// The member rejects.
pub fn learn_reject(m: &HermesManager, id: &str, member: &str, reason: &str) -> Result<(), String> {
    valid_proposal_id(id)?;
    post(
        m,
        &format!("/learn/proposals/{id}/reject"),
        &json!({ "member": member, "reason": reason }),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The learned-memory ledger and the memory graph
// ---------------------------------------------------------------------------

/// The memory graph a learned memory is stored in (production: the mem-mcp daemon).
pub trait MemoryGraph {
    fn running(&self) -> bool;
    /// Assert a claim; returns the daemon's reply text (which names the new node).
    fn assert_claim(&self, tenant: &str, content: &str) -> Result<String, String>;
    /// Propose a quarantined `contradicts` edge `from -> to`.
    fn propose_contradicts(&self, from: &str, to: &str, evidence: &str) -> Result<String, String>;
    /// Record that `from` supersedes `to` and confirm it (the daemon retires `to`).
    fn supersede(&self, from: &str, to: &str, evidence: &str) -> Result<String, String>;
}

impl MemoryGraph for crate::memory::MemoryManager {
    fn running(&self) -> bool {
        self.is_running()
    }
    fn assert_claim(&self, tenant: &str, content: &str) -> Result<String, String> {
        self.assert(tenant, content, "claim")
            .map_err(|e| e.to_string())
    }
    fn propose_contradicts(&self, from: &str, to: &str, evidence: &str) -> Result<String, String> {
        self.propose_edge(from, to, "contradicts", evidence)
            .map_err(|e| e.to_string())
    }
    fn supersede(&self, from: &str, to: &str, evidence: &str) -> Result<String, String> {
        self.propose_edge(from, to, "supersedes", evidence)
            .map_err(|e| e.to_string())?;
        self.confirm_edge(from, to, "supersedes")
            .map_err(|e| e.to_string())
    }
}

/// Where a learned memory is in the member's memory graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphState {
    /// "stored" | "pending" (the memory daemon was not running) | "failed" | "retracted" (set
    /// aside by the member before it reached the graph; never stored).
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One accepted memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedMemory {
    pub proposal_id: String,
    pub key: String,
    pub value: String,
    /// "true" | "both" (contradicted and unresolved; both memories are kept) | "false" (the
    /// member retracted it when resolving a contradiction; kept for the record).
    pub belnap: String,
    /// What this memory contradicts and is unresolved against: proposal ids of learned memories
    /// (both directions), and `memory:<id>` for a memory core held before (not learned here).
    pub contradicts: Vec<String>,
    pub content_sha256: String,
    pub workflow_id: String,
    pub accepted_by: String,
    pub accepted_at_ms: u64,
    /// `seq` of the HIC-1 decision in the sidecar's decision log.
    pub decision_seq: u64,
    pub graph: GraphState,
    /// For a retracted memory: the proposal id the member kept instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retracted_for: Option<String>,
    /// For a retracted memory: `seq` of the resolve decision (absent when applied from a sync).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_seq: Option<u64>,
    /// Graph nodes this memory's node still has to supersede (after a resolution).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersede_nodes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct LedgerFile {
    schema: String,
    entries: Vec<LearnedMemory>,
}

/// The learned-memory ledger.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Ledger {
    pub entries: Vec<LearnedMemory>,
}

/// The node id in a `memory.assert` reply (`asserted <12 hex> [...]`).
pub fn node_id_from_assert(text: &str) -> Option<String> {
    let id = text.strip_prefix("asserted ")?.split_whitespace().next()?;
    (id.len() >= 10 && id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| id.to_string())
}

/// How every learned memory's text starts in the memory graph ([`memory_text`]).
const LEARNED_TEXT_PREFIX: &str = "Hermes learned from a verified workflow";
/// The phrase the text of an unresolved learned memory carries ([`memory_text`]).
const UNRESOLVED_TEXT: &str = "It contradicts an earlier memory and is unresolved";
/// How the daemon prints the title of an unresolved learned memory: recall and search cut a
/// title at 71 characters, which ends inside [`UNRESOLVED_TEXT`], so the full phrase never shows
/// in a hit line. Every unresolved memory's text starts with this.
const UNRESOLVED_HEAD: &str =
    "Hermes learned from a verified workflow, accepted by you. It contradict";

/// What recall and search hide (fan-out 7, L02): a learned memory with an unresolved Belnap
/// `both` contradiction is not offered to Hermes or shown as a recall hit until the member
/// resolves it. The memory stays in the graph and in the ledger; only reads leave it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecallHide {
    /// The graph nodes of learned memories that are `both` right now.
    Nodes(Vec<String>),
    /// The ledger could not be read: every learned memory is left out (fail closed).
    AllLearned,
}

impl RecallHide {
    pub fn from_ledger(l: &Ledger) -> Self {
        RecallHide::Nodes(
            l.entries
                .iter()
                .filter(|e| e.belnap == "both")
                .filter_map(|e| e.graph.node_id.clone())
                .collect(),
        )
    }

    /// Read the ledger at `path` (a missing ledger hides nothing; a damaged one hides every
    /// learned memory).
    pub fn load(path: &Path) -> Self {
        match Ledger::load(path) {
            Ok(l) => Self::from_ledger(&l),
            Err(_) => RecallHide::AllLearned,
        }
    }

    /// Whether a recall hit (the daemon's node id prefix and its title) is left out.
    pub fn hides(&self, hit_id: &str, title: &str) -> bool {
        let learned = title.starts_with(LEARNED_TEXT_PREFIX);
        // Text stored for an unresolved memory is never offered, whatever the ledger says.
        if learned && (title.contains(UNRESOLVED_TEXT) || title.starts_with(UNRESOLVED_HEAD)) {
            return true;
        }
        match self {
            RecallHide::AllLearned => learned,
            RecallHide::Nodes(ids) => {
                hit_id.len() >= 6
                    && ids
                        .iter()
                        .any(|n| n.starts_with(hit_id) || hit_id.starts_with(n.as_str()))
            }
        }
    }

    /// Leave the hidden hits out of a parsed recall or search. Returns how many were left out.
    pub fn filter(&self, r: &mut crate::memory::MemoryResult) -> usize {
        let before = r.hits.len();
        r.hits.retain(|h| !self.hides(&h.id, &h.title));
        before - r.hits.len()
    }

    /// Leave the hidden hits out of the daemon's tool text (the hit line and its `cite:` and `>`
    /// passage lines). Every other line is kept as it was.
    pub fn filter_text(&self, text: &str) -> String {
        let mut out: Vec<&str> = Vec::new();
        let mut skipping = false;
        let mut removed = false;
        for line in text.lines() {
            let continuation = line.starts_with("    cite: ") || line.starts_with("    >");
            if skipping && continuation {
                continue;
            }
            skipping = false;
            if let Some(hit) = crate::memory::parse_hit_line(line.trim_end()) {
                if self.hides(&hit.id, &hit.title) {
                    skipping = true;
                    removed = true;
                    continue;
                }
            } else if let Some(title) = neighbor_title(line) {
                // `memory.neighbors` prints the full title with no node id: only the text rules
                // (unresolved text, or every learned memory when the ledger is unreadable) apply.
                if self.hides("", title) {
                    removed = true;
                    continue;
                }
            }
            out.push(line);
        }
        if !removed {
            return text.to_string();
        }
        let mut s = out.join("\n");
        if text.ends_with('\n') {
            s.push('\n');
        }
        s
    }
}

/// The title of a `memory.neighbors` line: `  -> [<edge>]< @repo> <title>` (or `<-`).
fn neighbor_title(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("  -> [")
        .or_else(|| line.strip_prefix("  <- ["))?;
    let after = &rest[rest.find(']')? + 1..];
    let after = match after.strip_prefix(" @") {
        Some(cross) => &cross[cross.find(' ')?..],
        None => after,
    };
    after.strip_prefix(' ')
}

fn memory_text(e: &LearnedMemory) -> String {
    if e.belnap == "both" {
        format!(
            "{LEARNED_TEXT_PREFIX}, accepted by you. {UNRESOLVED_TEXT}. {}: {}",
            e.key, e.value
        )
    } else {
        format!(
            "{LEARNED_TEXT_PREFIX}, accepted by you. {}: {}",
            e.key, e.value
        )
    }
}

fn str_field<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v.get(k)
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("the memory record has no {k}"))
}

impl Ledger {
    /// Read the ledger. A missing file is an empty ledger; a damaged one is an error (never
    /// silently emptied).
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Ledger::default()),
            Err(e) => return Err(format!("learned memories: cannot read: {}", e.kind())),
        };
        let f: LedgerFile = serde_json::from_slice(&bytes)
            .map_err(|e| format!("learned memories: damaged file: {e}"))?;
        if f.schema != LEDGER_SCHEMA {
            return Err(format!("learned memories: unknown schema {:?}", f.schema));
        }
        Ok(Ledger { entries: f.entries })
    }

    /// Write the ledger atomically (owner-only temporary file, flush, rename).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let body = serde_json::to_vec_pretty(&LedgerFile {
            schema: LEDGER_SCHEMA.into(),
            entries: self.entries.clone(),
        })
        .map_err(|e| e.to_string())?;
        let dir = path
            .parent()
            .ok_or_else(|| "learned memories: no folder".to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| format!("learned memories: {}", e.kind()))?;
        let tmp = dir.join(".learned-memories.json.tmp");
        let res = (|| -> std::io::Result<()> {
            use std::io::Write as _;
            // Owner-only from creation: the ledger holds the member's learned memories.
            // A leftover temporary file could carry a looser mode; start from a new one.
            let _ = std::fs::remove_file(&tmp);
            let mut f = citrate_core_kit::fsutil::create_secret_file(&tmp)?;
            f.write_all(&body)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if let Err(e) = res {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("learned memories: write: {}", e.kind()));
        }
        Ok(())
    }

    fn find_mut(&mut self, pid: &str) -> Option<&mut LearnedMemory> {
        self.entries.iter_mut().find(|e| e.proposal_id == pid)
    }

    /// Store one entry in the graph (if it is not stored yet), link its contradictions, and
    /// supersede the nodes it replaces. A retracted memory is never stored.
    fn store_one(&mut self, idx: usize, g: &dyn MemoryGraph) {
        let Some(e) = self.entries.get(idx).cloned() else {
            return;
        };
        if e.belnap == "false" {
            if e.graph.state != "stored" {
                if let Some(x) = self.entries.get_mut(idx) {
                    x.graph = retracted_graph();
                }
            }
            return;
        }
        if e.graph.state != "stored" {
            if !g.running() {
                if let Some(x) = self.entries.get_mut(idx) {
                    x.graph = GraphState {
                        state: "pending".into(),
                        node_id: None,
                        detail: Some(
                            "the memory store is not running; it is stored when the store is started"
                                .into(),
                        ),
                    };
                }
                return;
            }
            let state = match g.assert_claim(LEARN_TENANT, &memory_text(&e)) {
                Ok(reply) => match node_id_from_assert(&reply) {
                    Some(id) => GraphState {
                        state: "stored".into(),
                        node_id: Some(id),
                        detail: None,
                    },
                    None => GraphState {
                        state: "failed".into(),
                        node_id: None,
                        detail: Some(reply.chars().take(300).collect()),
                    },
                },
                Err(m) => GraphState {
                    state: "failed".into(),
                    node_id: None,
                    detail: Some(m.chars().take(300).collect()),
                },
            };
            let new_node = state.node_id.clone();
            if let Some(x) = self.entries.get_mut(idx) {
                x.graph = state;
            }
            // Link the contradiction both ways in meaning (one quarantined edge, new -> old).
            if let Some(from) = new_node {
                for other in &e.contradicts {
                    let to = self
                        .entries
                        .iter()
                        .find(|x| &x.proposal_id == other)
                        .and_then(|x| x.graph.node_id.clone());
                    if let Some(to) = to {
                        let evidence = format!(
                            "the member acknowledged this contradiction when accepting learn proposal {}",
                            e.proposal_id
                        );
                        if let Err(m) = g.propose_contradicts(&from, &to, &evidence) {
                            if let Some(x) = self.entries.get_mut(idx) {
                                x.graph.detail = Some(format!(
                                    "stored; the contradiction link to {other} failed: {}",
                                    m.chars().take(200).collect::<String>()
                                ));
                            }
                        }
                    }
                }
            }
        }
        self.link_supersedes(idx, g);
    }

    /// Supersede the nodes a stored memory replaces (after a resolution). A node that could not be
    /// linked stays listed for the next try, and the entry says why.
    fn link_supersedes(&mut self, idx: usize, g: &dyn MemoryGraph) {
        let Some(e) = self.entries.get(idx).cloned() else {
            return;
        };
        if e.supersede_nodes.is_empty() || e.graph.state != "stored" || !g.running() {
            return;
        }
        let Some(from) = e.graph.node_id.clone() else {
            return;
        };
        let evidence = format!(
            "the member resolved a contradiction and kept learn proposal {}",
            e.proposal_id
        );
        let mut left = Vec::new();
        let mut failure = None;
        for to in &e.supersede_nodes {
            if to == &from {
                continue;
            }
            if let Err(m) = g.supersede(&from, to, &evidence) {
                left.push(to.clone());
                failure = Some(m);
            }
        }
        if let Some(x) = self.entries.get_mut(idx) {
            x.supersede_nodes = left;
            if let Some(m) = failure {
                x.graph.detail = Some(format!(
                    "stored; marking the memory it replaces as superseded failed: {}",
                    m.chars().take(200).collect::<String>()
                ));
            }
        }
    }

    /// Keep an accepted memory record from the sidecar and store it in the graph. A record for a
    /// proposal already in the ledger is not stored again.
    pub fn accept_record(
        &mut self,
        rec: &Value,
        g: &dyn MemoryGraph,
    ) -> Result<LearnedMemory, String> {
        if str_field(rec, "schema")? != "citrate.learn.memory.v1" {
            return Err("not a learned memory record".into());
        }
        let pid = str_field(rec, "proposal_id")?.to_string();
        valid_proposal_id(&pid)?;
        let belnap = str_field(rec, "belnap")?.to_string();
        if belnap != "true" && belnap != "both" {
            return Err(format!("unknown Belnap value {belnap:?}"));
        }
        if let Some(existing) = self.entries.iter().position(|e| e.proposal_id == pid) {
            self.store_one(existing, g);
            return self
                .entries
                .get(existing)
                .cloned()
                .ok_or_else(|| "internal: ledger entry vanished".to_string());
        }
        if self.entries.len() >= MAX_LEDGER {
            return Err(format!(
                "the learned-memory ledger is full ({MAX_LEDGER}); nothing was stored"
            ));
        }
        let contradicts: Vec<String> = rec
            .get("contradicts")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str())
                    .filter_map(|s| match s.strip_prefix("proposal:") {
                        // A learned memory, held in this ledger by its proposal id.
                        Some(pid) => Some(pid.to_string()),
                        // The sidecar names a known memory by its bare id.
                        None => {
                            let r = format!("{KNOWN_MEMORY_PREFIX}{s}");
                            valid_known_ref(&r).then_some(r)
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let entry = LearnedMemory {
            proposal_id: pid.clone(),
            key: str_field(rec, "key")?.to_string(),
            value: str_field(rec, "value")?.to_string(),
            belnap: belnap.clone(),
            contradicts: contradicts.clone(),
            content_sha256: str_field(rec, "content_sha256")?.to_string(),
            workflow_id: rec
                .pointer("/evidence/workflow_id")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
            accepted_by: str_field(rec, "accepted_by")?.to_string(),
            accepted_at_ms: rec
                .get("accepted_at_ms")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            decision_seq: rec
                .get("decision_seq")
                .and_then(|x| x.as_u64())
                .unwrap_or(0),
            graph: GraphState {
                state: "pending".into(),
                node_id: None,
                detail: None,
            },
            retracted_for: None,
            resolved_seq: None,
            supersede_nodes: Vec::new(),
        };
        // Belnap `both` on both sides: the memory it contradicts is marked unresolved too.
        if belnap == "both" {
            for other in &contradicts {
                if let Some(o) = self.find_mut(other) {
                    o.belnap = "both".into();
                    if !o.contradicts.contains(&pid) {
                        o.contradicts.push(pid.clone());
                    }
                }
            }
        }
        self.entries.push(entry);
        let idx = self.entries.len() - 1;
        self.store_one(idx, g);
        self.entries
            .get(idx)
            .cloned()
            .ok_or_else(|| "internal: ledger entry vanished".to_string())
    }

    /// Store every memory still waiting for the graph, and finish any supersede links a
    /// resolution left. Returns how many memories are newly stored.
    pub fn store_pending(&mut self, g: &dyn MemoryGraph) -> usize {
        let mut n = 0;
        for i in 0..self.entries.len() {
            let Some(e) = self.entries.get(i) else {
                continue;
            };
            let was_stored = e.graph.state == "stored";
            if e.belnap == "false" || (was_stored && e.supersede_nodes.is_empty()) {
                continue;
            }
            self.store_one(i, g);
            if !was_stored && self.entries.get(i).map(|e| e.graph.state.as_str()) == Some("stored")
            {
                n += 1;
            }
        }
        n
    }

    fn position(&self, pid: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.proposal_id == pid)
    }

    /// Mark an entry settled (`true`). If it was stored with its unresolved text, it is stored
    /// again and the new node supersedes the old one.
    fn settle(&mut self, idx: usize) {
        if let Some(k) = self.entries.get_mut(idx) {
            k.belnap = "true".into();
            if k.graph.state == "stored" {
                if let Some(old) = k.graph.node_id.take() {
                    if !k.supersede_nodes.contains(&old) {
                        k.supersede_nodes.push(old);
                    }
                }
                k.graph = GraphState {
                    state: "pending".into(),
                    node_id: None,
                    detail: Some("resolved; stored again as settled".into()),
                };
            }
        }
    }

    /// Retract `retracted` in favour of `kept` (the sidecar has recorded it). Returns whether
    /// the ledger changed. Only the kept memory may be settled here (formal model,
    /// `ContradictionResolve.tla`: another memory may itself be retracted by a resolution whose
    /// answer has not arrived yet).
    fn apply_retraction(
        &mut self,
        kept: &str,
        retracted: &str,
        seq: Option<u64>,
        g: &dyn MemoryGraph,
    ) -> bool {
        let Some(di) = self.position(retracted) else {
            return false;
        };
        let Some(d) = self.entries.get_mut(di) else {
            return false;
        };
        if d.belnap == "false" {
            return false;
        }
        let d_node = if d.graph.state == "stored" {
            d.graph.node_id.clone()
        } else {
            None
        };
        d.belnap = "false".into();
        d.retracted_for = Some(kept.to_string());
        d.resolved_seq = seq;
        d.contradicts.clear();
        d.supersede_nodes.clear();
        if d.graph.state != "stored" {
            d.graph = retracted_graph();
        }
        for e in self.entries.iter_mut() {
            e.contradicts.retain(|c| c != retracted);
        }
        if let Some(ki) = self.position(kept) {
            let settle = match self.entries.get_mut(ki) {
                Some(k) if k.belnap != "false" => {
                    if let Some(n) = d_node {
                        if !k.supersede_nodes.contains(&n) {
                            k.supersede_nodes.push(n);
                        }
                    }
                    Some(k.belnap == "both" && k.contradicts.is_empty())
                }
                _ => None,
            };
            if let Some(settle) = settle {
                if settle {
                    self.settle(ki);
                }
                self.store_one(ki, g);
            }
        }
        true
    }

    /// Apply the sidecar's resolution record. Refuses a record that is not one; a record for
    /// memories this ledger never received changes nothing. Returns whether the ledger changed.
    pub fn apply_resolution(&mut self, res: &Value, g: &dyn MemoryGraph) -> Result<bool, String> {
        if str_field(res, "schema")? != RESOLUTION_SCHEMA {
            return Err("not a learn resolution record".into());
        }
        let kept = str_field(res, "kept")?;
        let retracted = str_field(res, "retracted")?;
        valid_memory_ref(kept)?;
        valid_memory_ref(retracted)?;
        if kept == retracted {
            return Err("a memory cannot be kept and retracted at once".into());
        }
        if valid_known_ref(kept) && valid_known_ref(retracted) {
            return Err("one side of a resolution must be a learned memory".into());
        }
        if valid_known_ref(retracted) {
            return Ok(self.apply_set_aside(kept, retracted, g));
        }
        let seq = res.get("decision_seq").and_then(|x| x.as_u64());
        Ok(self.apply_retraction(kept, retracted, seq, g))
    }

    /// The member kept the learned memory `kept` over the known memory `known` (`memory:<id>`):
    /// the known one no longer counts against it, and when nothing else contradicts it the kept
    /// memory settles (`true`) and is stored again as settled. When the known id is a memory
    /// graph node, the kept memory's node supersedes it. Returns whether the ledger changed.
    fn apply_set_aside(&mut self, kept: &str, known: &str, g: &dyn MemoryGraph) -> bool {
        let Some(ki) = self.position(kept) else {
            return false;
        };
        let settle = match self.entries.get_mut(ki) {
            Some(k) if k.belnap != "false" && k.contradicts.iter().any(|c| c == known) => {
                k.contradicts.retain(|c| c != known);
                if let Some(node) = known.strip_prefix(KNOWN_MEMORY_PREFIX) {
                    if node_id_from_assert(&format!("asserted {node}")).is_some()
                        && !k.supersede_nodes.iter().any(|n| n == node)
                    {
                        k.supersede_nodes.push(node.to_string());
                    }
                }
                k.belnap == "both" && k.contradicts.is_empty()
            }
            _ => return false,
        };
        if settle {
            self.settle(ki);
        }
        self.store_one(ki, g);
        true
    }

    /// Bring the ledger in line with the sidecar's proposal list (`/learn/proposals?all=true`):
    /// apply every retraction the ledger has not applied yet (a resolve whose answer was lost),
    /// then settle a memory still `both` with nothing left to contradict, but only one the
    /// sidecar shows standing (`persisted`, or `persist_failed` after a lost save). Returns how
    /// many entries were changed by a retraction or a settle.
    pub fn sync_with_sidecar(&mut self, list: &Value, g: &dyn MemoryGraph) -> usize {
        let Some(arr) = list.get("proposals").and_then(|p| p.as_array()) else {
            return 0;
        };
        let memory = |p: &&Value| p.get("kind").and_then(|k| k.as_str()) == Some("memory");
        let state_of = |p: &Value| {
            p.pointer("/state/state")
                .and_then(|s| s.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let mut n = 0;
        for p in arr.iter().filter(memory) {
            if state_of(p) != "retracted" {
                continue;
            }
            let id = p.get("id").and_then(|x| x.as_str()).unwrap_or_default();
            let kept = p
                .pointer("/state/kept")
                .and_then(|x| x.as_str())
                .unwrap_or_default();
            if valid_proposal_id(id).is_err() || valid_memory_ref(kept).is_err() || id == kept {
                continue;
            }
            if self.apply_retraction(kept, id, None, g) {
                n += 1;
            }
        }
        // Known memories the member set aside in favour of a learned one (a resolve whose answer
        // was lost).
        for p in arr.iter().filter(memory) {
            let id = p.get("id").and_then(|x| x.as_str()).unwrap_or_default();
            if valid_proposal_id(id).is_err() {
                continue;
            }
            let asides = p
                .get("set_aside")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default();
            for known in asides.iter().filter_map(|x| x.as_str()) {
                if valid_known_ref(known) && self.apply_set_aside(id, known, g) {
                    n += 1;
                }
            }
        }
        let standing: Vec<String> = arr
            .iter()
            .filter(memory)
            .filter(|p| matches!(state_of(p).as_str(), "persisted" | "persist_failed"))
            .filter_map(|p| p.get("id").and_then(|x| x.as_str()).map(str::to_string))
            .collect();
        for i in 0..self.entries.len() {
            let settle = self.entries.get(i).is_some_and(|e| {
                e.belnap == "both" && e.contradicts.is_empty() && standing.contains(&e.proposal_id)
            });
            if settle {
                self.settle(i);
                self.store_one(i, g);
                n += 1;
            }
        }
        n
    }
}

fn retracted_graph() -> GraphState {
    GraphState {
        state: "retracted".into(),
        node_id: None,
        detail: Some("set aside when you resolved a contradiction; not stored".into()),
    }
}

// ---------------------------------------------------------------------------
// Pinning a skill (before a publish)
// ---------------------------------------------------------------------------

fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(b))
}

/// Pin an accepted `SKILL.md` to the local IPFS node and read it back; returns its CID for the
/// registry's `manifestCID`. Refuses bytes that are not the accepted content, a CID that is not a
/// bare CID, and a pin that does not read back the same bytes.
pub fn pin_skill(
    t: &dyn crate::storage::KuboTransport,
    skill_md: &str,
    expected_sha256: &str,
) -> Result<String, String> {
    if sha256_hex(skill_md.as_bytes()) != expected_sha256 {
        return Err("the skill does not match its accepted content".into());
    }
    let out = t
        .add("SKILL.md", skill_md.as_bytes())
        .map_err(|e| format!("IPFS: could not add the skill: {e}"))?;
    let cid = out.cid;
    if cid.is_empty() || cid.len() > MAX_CID_LEN || !cid.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err("IPFS: the node answered with something that is not a CID".into());
    }
    t.pin_add(&cid)
        .map_err(|e| format!("IPFS: could not pin {cid}: {e}"))?;
    let back = t
        .cat(&cid)
        .map_err(|e| format!("IPFS: could not read {cid} back: {e}"))?;
    if back != skill_md.as_bytes() {
        return Err(format!(
            "IPFS: {cid} did not read back as the accepted skill"
        ));
    }
    Ok(cid)
}

/// The publish payload must register the CID core pinned.
pub fn check_manifest_cid(payload: &Value, cid: &str) -> Result<(), String> {
    if cid.is_empty() || str_field(payload, "manifest_cid")? != cid {
        return Err("the publish payload does not carry the pinned skill's CID".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Publishing (HIC-1 ceremony; off pending owner sign-off)
// ---------------------------------------------------------------------------

/// Whether the publish button is live, and why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishAvailability {
    pub enabled: bool,
    pub note: String,
}

/// `enabled` only when the flag is on AND the registry's code answers `registerSkill`.
pub fn publish_availability(flag: bool, registry_code: Option<&str>) -> PublishAvailability {
    if !flag {
        return PublishAvailability {
            enabled: false,
            note: "Publishing learned skills to the on-chain SkillRegistry is not turned on yet (pending owner sign-off on the registry deployment after the network reset). Your accepted skills are saved on this device.".into(),
        };
    }
    let code = registry_code.unwrap_or("0x").trim_start_matches("0x");
    if code.is_empty() {
        return PublishAvailability {
            enabled: false,
            note: "The SkillRegistry is not deployed on chain 40204 yet, so there is nothing to publish to. Your accepted skills are saved on this device.".into(),
        };
    }
    if !code
        .to_ascii_lowercase()
        .contains(&format!("63{REGISTER_SKILL_SELECTOR}"))
    {
        return PublishAvailability {
            enabled: false,
            note: "The contract at the SkillRegistry address does not offer registerSkill, so publishing stays off.".into(),
        };
    }
    PublishAvailability {
        enabled: true,
        note: "Publishing opens the Signature Ceremony; you review and sign there.".into(),
    }
}

fn lower_addr(v: &Value, k: &str) -> Result<String, String> {
    let s = str_field(v, k)?.to_ascii_lowercase();
    if s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(s)
    } else {
        Err(format!("the publish payload's {k} is not an address"))
    }
}

/// Longest skill name, version, manifest CID or description accepted for publishing (bytes).
const MAX_PUBLISH_FIELD: usize = 2_000;
/// Most tags accepted for publishing.
const MAX_PUBLISH_TAGS: usize = 16;

fn abi_word(n: usize) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&(n as u64).to_be_bytes());
    w
}

fn abi_string(s: &str) -> Vec<u8> {
    let mut out = abi_word(s.len()).to_vec();
    out.extend_from_slice(s.as_bytes());
    out.resize(32 + s.len().div_ceil(32) * 32, 0);
    out
}

/// The canonical ABI encoding of `registerSkill(name, version, manifestCID, description, tags)`,
/// selector included.
pub fn encode_register_skill(
    name: &str,
    version: &str,
    manifest_cid: &str,
    description: &str,
    tags: &[String],
) -> Vec<u8> {
    let mut arr = abi_word(tags.len()).to_vec();
    let encoded: Vec<Vec<u8>> = tags.iter().map(|t| abi_string(t)).collect();
    let mut off = tags.len() * 32;
    for e in &encoded {
        arr.extend_from_slice(&abi_word(off));
        off += e.len();
    }
    for e in &encoded {
        arr.extend_from_slice(e);
    }
    let parts = [
        abi_string(name),
        abi_string(version),
        abi_string(manifest_cid),
        abi_string(description),
        arr,
    ];
    let mut out = hex::decode(REGISTER_SKILL_SELECTOR).unwrap_or_default();
    let mut off = parts.len() * 32;
    for p in &parts {
        out.extend_from_slice(&abi_word(off));
        off += p.len();
    }
    for p in &parts {
        out.extend_from_slice(p);
    }
    out
}

/// The id the SkillRegistry assigns (HUP-S7.1 redeploy, citrate-chain PR #272):
/// `skillHashOf(owner, name, version) = keccak256(abi.encode(owner, name, version))`, as `0x` hex.
pub fn skill_hash_of(owner: &str, name: &str, version: &str) -> Result<String, String> {
    use sha3::Digest as _;
    let hexpart = owner
        .strip_prefix("0x")
        .filter(|h| h.len() == 40)
        .ok_or_else(|| "the owner is not an address".to_string())?;
    let addr = hex::decode(hexpart).map_err(|_| "the owner is not an address".to_string())?;
    let (name_tail, version_tail) = (abi_string(name), abi_string(version));
    let mut owner_word = [0u8; 32];
    owner_word[12..].copy_from_slice(&addr);
    let mut h = sha3::Keccak256::new();
    h.update(owner_word);
    h.update(abi_word(96));
    h.update(abi_word(96 + name_tail.len()));
    h.update(&name_tail);
    h.update(&version_tail);
    Ok(format!("0x{}", hex::encode(h.finalize())))
}

/// Check the sidecar's publish payload and turn it into a PENDING ceremony intent. Refuses a
/// payload for another target, owner or chain, one that moves value, asks to broadcast, is not
/// a `registerSkill` call, or projects a skill id other than the registry's `skillHashOf`.
pub fn publish_intent(
    payload: &Value,
    registry: &str,
    wallet: &str,
) -> Result<crate::ceremony::SignatureIntent, String> {
    let to = lower_addr(payload, "to")?;
    let owner = lower_addr(payload, "owner")?;
    if to != registry.to_ascii_lowercase() {
        return Err("the publish payload targets another contract".into());
    }
    if owner != wallet.to_ascii_lowercase() {
        return Err("the publish payload is for another owner".into());
    }
    if payload.get("chain_id").and_then(|c| c.as_u64()) != Some(40204) {
        return Err("the publish payload is for another chain".into());
    }
    if payload.get("broadcast").and_then(|b| b.as_bool()) != Some(false) {
        return Err("the publish payload must not broadcast".into());
    }
    if str_field(payload, "value")? != "0x0" {
        return Err("publishing a skill moves no value".into());
    }
    let data = str_field(payload, "data")?.to_ascii_lowercase();
    if !data.starts_with(&format!("0x{REGISTER_SKILL_SELECTOR}"))
        || !data[2..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("the publish payload is not a registerSkill call".into());
    }
    // The calldata must be exactly the canonical encoding of the fields the member is shown: no
    // other strings, no extra bytes, no unusual offsets.
    let field = |k: &str| -> Result<&str, String> {
        let v = payload
            .get(k)
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("the publish payload has no {k}"))?;
        if v.len() > MAX_PUBLISH_FIELD {
            return Err(format!("the publish payload's {k} is too long"));
        }
        Ok(v)
    };
    let name = field("name")?;
    if name.is_empty() {
        return Err("the publish payload has no skill name".into());
    }
    let tags: Vec<String> = payload
        .get("tags")
        .and_then(|t| t.as_array())
        .ok_or("the publish payload has no tags")?
        .iter()
        .map(|t| {
            t.as_str()
                .filter(|s| s.len() <= MAX_PUBLISH_FIELD)
                .map(str::to_string)
                .ok_or_else(|| "the publish payload has a tag that is not text".to_string())
        })
        .collect::<Result<_, _>>()?;
    if tags.len() > MAX_PUBLISH_TAGS {
        return Err("the publish payload has too many tags".into());
    }
    let expected = encode_register_skill(
        name,
        field("version")?,
        field("manifest_cid")?,
        field("description")?,
        &tags,
    );
    if data[2..] != hex::encode(expected) {
        return Err(
            "the publish calldata does not encode the skill shown; nothing was prepared".into(),
        );
    }
    // The id the member is shown must be the one the registry will assign.
    let id = skill_hash_of(&owner, name, field("version")?)?;
    if field("expected_skill_hash")?.to_ascii_lowercase() != id {
        return Err(
            "the publish payload's skill id is not the registry's id for this owner, name and version; nothing was prepared"
                .into(),
        );
    }
    let raw = json!({
        "from": owner,
        "to": to,
        "value": "0x0",
        "data": data,
        "gas": format!("0x{REGISTER_SKILL_GAS:x}"),
        "chainId": format!("0x{:x}", 40204u64),
    })
    .to_string();
    Ok(crate::ceremony::SignatureIntent {
        origin: "agent:hermes".to_string(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw,
    })
}

// ---------------------------------------------------------------------------
// App wiring
// ---------------------------------------------------------------------------

static LEDGER_LOCK: Mutex<()> = Mutex::new(());

/// The learned-memory ledger file (`<app local data>/hermes/learned-memories.json`).
pub(crate) fn ledger_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes")
        .join("learned-memories.json"))
}

/// The member id recorded with a decision: the wallet address, or `local-member`.
fn member_id(app: &tauri::AppHandle) -> String {
    tauri::Manager::try_state::<crate::custody::CustodyState>(app)
        .and_then(|c| crate::wallet::address_auto_unlocked(&c.0).ok())
        .map(|w| w.address.to_ascii_lowercase())
        .unwrap_or_else(|| "local-member".to_string())
}

/// Run `f` over the ledger and the memory graph, then save the ledger.
fn with_ledger<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&mut Ledger, &dyn MemoryGraph) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = LEDGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = ledger_path(app)?;
    let mut ledger = Ledger::load(&path)?;
    let mem = tauri::Manager::try_state::<crate::memory::MemoryState>(app)
        .ok_or("internal: memory state unavailable")?;
    let out = f(&mut ledger, &mem.0)?;
    ledger.save(&path)?;
    Ok(out)
}

/// **hermes_workflow_run** — run a declarative, verifier-judged workflow in a session.
#[tauri::command]
pub async fn hermes_workflow_run(
    app: tauri::AppHandle,
    session_id: String,
    workflow_json: String,
) -> Result<String, String> {
    crate::blocking::off_main(move || workflow_run(manager(&app)?, &session_id, &workflow_json))
        .await
}

/// **hermes_workflow_status** — a workflow run's state and evidence.
#[tauri::command]
pub async fn hermes_workflow_status(
    app: tauri::AppHandle,
    session_id: String,
    run_id: String,
) -> Result<Value, String> {
    crate::blocking::off_main(move || workflow_status(manager(&app)?, &session_id, &run_id)).await
}

/// **hermes_learn_status** — whether learning is on in the sidecar, and whether publishing is.
#[tauri::command]
pub async fn hermes_learn_status(app: tauri::AppHandle) -> Result<Value, String> {
    crate::blocking::off_main(move || {
        let m = manager(&app)?;
        let sidecar = if m.is_running() {
            learn_status_raw(m).unwrap_or_else(|e| json!({ "enabled": false, "error": e }))
        } else {
            json!({ "enabled": false, "error": "Hermes is not running" })
        };
        // The registry probe is a network read; it only runs once publishing is turned on.
        let publish = if SKILL_PUBLISH_ENABLED {
            let rpc = crate::rpc::RpcClient::citrate();
            let body = rpc.build_request(
                "eth_getCode",
                json!([crate::addresses::skill_registry(), "latest"]),
            );
            let code = crate::rpc::RpcTransport::call(rpc.transport(), body)
                .ok()
                .and_then(|v| v.get("result").and_then(|r| r.as_str()).map(str::to_string));
            publish_availability(true, code.as_deref())
        } else {
            publish_availability(false, None)
        };
        Ok(json!({ "sidecar": sidecar, "publish": publish }))
    })
    .await
}

/// **hermes_learn_proposals** — proposals waiting for the member (`all`: every kept one).
#[tauri::command]
pub async fn hermes_learn_proposals(app: tauri::AppHandle, all: bool) -> Result<Value, String> {
    crate::blocking::off_main(move || learn_list(manager(&app)?, all)).await
}

/// **hermes_learn_propose** — propose a skill or memory from a verified run of a session.
#[tauri::command]
pub async fn hermes_learn_propose(
    app: tauri::AppHandle,
    session_id: String,
    run_id: String,
    content_json: String,
) -> Result<Value, String> {
    crate::blocking::off_main(move || {
        let content: Value = serde_json::from_str(&content_json)
            .map_err(|_| "the proposal content must be JSON".to_string())?;
        learn_propose(manager(&app)?, &session_id, &run_id, &content)
    })
    .await
}

/// **hermes_learn_accept** — the member accepts. A skill is written by the sidecar; a memory is
/// kept in the ledger and stored in the memory graph. Returns `{persisted, memory?}`.
#[tauri::command]
pub async fn hermes_learn_accept(
    app: tauri::AppHandle,
    id: String,
    acknowledged: Vec<String>,
) -> Result<Value, String> {
    crate::blocking::off_main(move || {
        if acknowledged.len() > 64 {
            return Err("too many acknowledged conflicts".into());
        }
        let member = member_id(&app);
        let out = learn_accept_raw(manager(&app)?, &id, &member, &acknowledged)?;
        let persisted = out.get("persisted").cloned().unwrap_or(Value::Null);
        if persisted.get("kind").and_then(|k| k.as_str()) == Some("memory") {
            let entry = with_ledger(&app, |l, g| l.accept_record(&persisted, g))?;
            return Ok(json!({ "persisted": persisted, "memory": entry }));
        }
        // A skill: whether the sidecar already offers it to the next session (no restart).
        let reloaded = out
            .get("skills_reloaded")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Ok(json!({ "persisted": persisted, "skillsReloaded": reloaded }))
    })
    .await
}

/// **hermes_learn_reject** — the member rejects (recorded in the decision log; final).
#[tauri::command]
pub async fn hermes_learn_reject(
    app: tauri::AppHandle,
    id: String,
    reason: String,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let member = member_id(&app);
        let reason: String = reason.chars().take(1000).collect();
        learn_reject(manager(&app)?, &id, &member, &reason)
    })
    .await
}

/// The sidecar's full proposal list, when Hermes is running (for [`Ledger::sync_with_sidecar`]).
fn sidecar_proposals(app: &tauri::AppHandle) -> Option<Value> {
    let m = manager(app).ok()?;
    if !m.is_running() {
        return None;
    }
    learn_list(m, true).ok()
}

/// **hermes_learn_memories** — the learned-memory ledger (with each memory's graph state). While
/// Hermes is running, the ledger is first brought in line with the sidecar (a resolution whose
/// answer was lost is applied).
#[tauri::command]
pub async fn hermes_learn_memories(app: tauri::AppHandle) -> Result<Vec<LearnedMemory>, String> {
    crate::blocking::off_main(move || match sidecar_proposals(&app) {
        Some(list) => with_ledger(&app, |l, g| {
            l.sync_with_sidecar(&list, g);
            Ok(l.entries.clone())
        }),
        None => {
            let _guard = LEDGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            Ledger::load(&ledger_path(&app)?).map(|l| l.entries)
        }
    })
    .await
}

/// **hermes_learn_resolve** — the member resolves a contradiction: keep one learned memory,
/// retract the other (HIC-1; the sidecar records the decision before anything changes). Core
/// applies it to the ledger and the memory graph and returns the ledger.
#[tauri::command]
pub async fn hermes_learn_resolve(
    app: tauri::AppHandle,
    keep: String,
    retract: String,
) -> Result<Vec<LearnedMemory>, String> {
    crate::blocking::off_main(move || {
        let member = member_id(&app);
        let m = manager(&app)?;
        let res = learn_resolve(m, &keep, &retract, &member)?;
        let list = learn_list(m, true).ok();
        with_ledger(&app, |l, g| {
            l.apply_resolution(&res, g)?;
            if let Some(list) = &list {
                l.sync_with_sidecar(list, g);
            }
            Ok(l.entries.clone())
        })
    })
    .await
}

/// **hermes_learn_store_pending** — store every learned memory still waiting for the graph.
#[tauri::command]
pub async fn hermes_learn_store_pending(
    app: tauri::AppHandle,
) -> Result<Vec<LearnedMemory>, String> {
    crate::blocking::off_main(move || {
        with_ledger(&app, |l, g| {
            l.store_pending(g);
            Ok(l.entries.clone())
        })
    })
    .await
}

/// **hermes_learn_publish** — publish an accepted skill to the SkillRegistry (HIC-1). Off pending
/// owner sign-off ([`SKILL_PUBLISH_ENABLED`]); when on, it opens a PENDING SignatureCeremony.
#[tauri::command]
pub async fn hermes_learn_publish(
    app: tauri::AppHandle,
    id: String,
    version: String,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        if !SKILL_PUBLISH_ENABLED {
            return Err(format!(
                "PUBLISH_DISABLED: {}",
                publish_availability(false, None).note
            ));
        }
        valid_proposal_id(&id)?;
        let m = manager(&app)?;
        let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(&app)
            .ok_or("internal: custody state unavailable")?;
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app)
            .ok_or("internal: ceremony state unavailable")?;
        let wallet = crate::wallet::address_auto_unlocked(&custody.0)
            .map_err(|e| e.to_string())?
            .address
            .to_ascii_lowercase();
        let proposal = get(m, &format!("/learn/proposals/{id}"))?;
        let sha = str_field(&proposal, "content_sha256")?.to_string();
        // Pin the accepted SKILL.md to the local IPFS node first, so the registry entry names
        // content anyone can fetch and check against the `sha256:` tag.
        let skill_md = proposal
            .pointer("/content/skill_md")
            .and_then(|v| v.as_str())
            .ok_or("only a saved skill can be published")?;
        let cid = pin_skill(
            &crate::storage::UreqKuboTransport::from_env(),
            skill_md,
            &sha,
        )
        .map_err(|e| format!("PIN_FAILED: {e}. Start Storage (IPFS) and try again."))?;
        let registry = crate::addresses::skill_registry().to_string();
        let payload = post(
            m,
            &format!("/learn/proposals/{id}/publish"),
            &json!({
                "approval": { "member": wallet, "proposal_id": id, "content_sha256": sha },
                "params": { "chain_id": 40204, "registry": registry, "owner": wallet, "version": version, "manifest_cid": cid, "tags": [] },
            }),
        )?;
        check_manifest_cid(&payload, &cid)?;
        let intent = publish_intent(&payload, &registry, &wallet)?;
        ceremony.0.request(intent);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_learn_tests.rs");
}
