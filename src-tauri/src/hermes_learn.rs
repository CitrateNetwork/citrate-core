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
//!   `CITRATE_HERMES_SKILLS` as well, so an accepted skill is offered in sessions after the next
//!   sidecar start.
//! - **Memories.** An accepted memory comes back as a typed record. Core keeps it in the
//!   learned-memory ledger (`<app data>/hermes/learned-memories.json`, keyed by proposal id, so a
//!   second accept of the same proposal is not a duplicate) and stores it in the member's memory
//!   graph (`memory.assert` into the `personal` tenant). A contradiction is Belnap `both`: the new
//!   memory AND the one it contradicts are marked `both` in the ledger (neither is relied on until
//!   the member resolves it) and the two graph nodes are linked by a quarantined `contradicts`
//!   edge. Nothing is merged or overwritten. When the memory daemon is not running, the memory waits
//!   in the ledger as `pending` and is stored by `hermes_learn_store_pending`.
//! - **Publishing** an accepted skill to the on-chain SkillRegistry is an HIC-1 action: the sidecar
//!   records the decision and builds calldata only, core checks the payload (target, owner, chain,
//!   selector, no value, no broadcast) and opens a PENDING SignatureCeremony; the member signs and
//!   sends there (Rule 3). It is OFF ([`SKILL_PUBLISH_ENABLED`]) pending owner sign-off, and the UI
//!   says so.
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

/// Publishing learned skills to the SkillRegistry on 40204. **Pending owner sign-off**: the
/// registry at the address-book address answers `registerSkill`, but whether that deployment is
/// the one members should publish to after the fresh-keys reroll is the owner's call. While this
/// is `false` the publish button is disabled with that note; nothing else changes.
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
}

/// Where a learned memory is in the member's memory graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphState {
    /// "stored" | "pending" (the memory daemon was not running) | "failed".
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
    /// "true" | "both" (contradicted, not relied on until the member resolves it).
    pub belnap: String,
    /// Proposal ids of the learned memories this one contradicts (both directions).
    pub contradicts: Vec<String>,
    pub content_sha256: String,
    pub workflow_id: String,
    pub accepted_by: String,
    pub accepted_at_ms: u64,
    /// `seq` of the HIC-1 decision in the sidecar's decision log.
    pub decision_seq: u64,
    pub graph: GraphState,
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

fn memory_text(e: &LearnedMemory) -> String {
    if e.belnap == "both" {
        format!(
            "Hermes learned from a verified workflow, accepted by you. It contradicts an earlier memory and is unresolved. {}: {}",
            e.key, e.value
        )
    } else {
        format!(
            "Hermes learned from a verified workflow, accepted by you. {}: {}",
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

    /// Write the ledger atomically (temporary file, flush, rename).
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
            let mut f = std::fs::File::create(&tmp)?;
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

    /// Store one entry in the graph (if it is not stored yet) and link its contradictions.
    fn store_one(&mut self, idx: usize, g: &dyn MemoryGraph) {
        let Some(e) = self.entries.get(idx).cloned() else {
            return;
        };
        if e.graph.state == "stored" {
            return;
        }
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
                    .map(|s| s.strip_prefix("proposal:").unwrap_or(s).to_string())
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
        };
        // Belnap `both` on both sides: the memory it contradicts is no longer relied on either.
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

    /// Store every memory still waiting for the graph. Returns how many are stored now.
    pub fn store_pending(&mut self, g: &dyn MemoryGraph) -> usize {
        let mut n = 0;
        for i in 0..self.entries.len() {
            let before = self.entries.get(i).map(|e| e.graph.state.clone());
            if before.as_deref() == Some("stored") {
                continue;
            }
            self.store_one(i, g);
            if self.entries.get(i).map(|e| e.graph.state.as_str()) == Some("stored") {
                n += 1;
            }
        }
        n
    }
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

/// Check the sidecar's publish payload and turn it into a PENDING ceremony intent. Refuses a
/// payload for another target, owner or chain, one that moves value, asks to broadcast, or is not
/// a `registerSkill` call.
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

fn ledger_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
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
        Ok(json!({ "persisted": persisted }))
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

/// **hermes_learn_memories** — the learned-memory ledger (with each memory's graph state).
#[tauri::command]
pub async fn hermes_learn_memories(app: tauri::AppHandle) -> Result<Vec<LearnedMemory>, String> {
    crate::blocking::off_main(move || {
        let _guard = LEDGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Ledger::load(&ledger_path(&app)?).map(|l| l.entries)
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
        let registry = crate::addresses::skill_registry().to_string();
        let payload = post(
            m,
            &format!("/learn/proposals/{id}/publish"),
            &json!({
                "approval": { "member": wallet, "proposal_id": id, "content_sha256": sha },
                "params": { "chain_id": 40204, "registry": registry, "owner": wallet, "version": version, "manifest_cid": null, "tags": [] },
            }),
        )?;
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
