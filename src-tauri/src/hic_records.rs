//! HUP-S2.6 (US-7.2 AC1), core's half: core's HIC-1/2 events into the decision records the
//! nightly anchor batches.
//!
//! Some HIC events are decided in core, not in the agent sidecar: folder-grant changes and the
//! full-access confirmation (`agent_grants`), escalation spend (`escalation`), and the member's
//! answer on an approval card (a ceremony or an agent tool call, decided in the webview's
//! ceremony sheet). Each becomes one record in core's **outbox**, a hash-chained, append-only
//! JSONL file under `<app_local_data>/hermes/hic-outbox/` (the agent's default-deny list covers
//! Citrate Core's app data), and is then copied to the sidecar's `POST /records/core`, which writes
//! it into the one records directory the anchor batches (`CITRATE_HERMES_RECORDS_DIR`). Budgeted
//! SIWE keeps its own route (`/records/web-signing`, `web_signin::export_records`).
//!
//! Delivery is at least once: the export cursor advances only after the sidecar accepted a batch,
//! so a crash in between can repeat a record; the core record id and hash in each copy identify
//! repeats. Hermes off: the records wait in the outbox for the next export (after the next event,
//! or the nightly anchor pass).
//!
//! **Owner decisions (conservative placeholders, pending owner sign-off).**
//! - Fail closed: a grant change whose record cannot be written is undone and refused; an
//!   escalation does not start when the outbox cannot take a record; an outbox whose chain does
//!   not verify takes nothing and exports nothing.
//! - Bound: at most [`MAX_UNEXPORTED`] records wait for export; past that, new events are refused
//!   (fail closed) until Hermes runs and takes them.
//! - Retention: exported records beyond the newest [`RETAIN_EXPORTED`] are dropped from the
//!   outbox (the sidecar's records directory and the anchor keep them); the rewritten file starts
//!   with a base line naming the last dropped record, so the chain still verifies.
//! - No device-key MAC: records are hash-chained only. Nothing here signs or holds a key (Rule 3).
//!
//! Residual: an approval card's answer reaches this module from the webview (the same place the
//! member clicked); core checks its shape and kind, not that a click happened.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::web_signin::SidecarLink;

/// The outbox folder under core's `<app_local_data>/hermes`.
pub const OUTBOX_DIR: &str = "hic-outbox";
const OUTBOX_FILE: &str = "outbox.jsonl";
const CURSOR_FILE: &str = "exported";
/// Hash domain of a core HIC record.
const DOMAIN: &[u8] = b"citrate-core/hic-record/v1\n";
/// The chain's first `prevHash`.
pub const GENESIS: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";
/// Records sent per export call (the sidecar's limit is 100).
pub const EXPORT_BATCH: usize = 100;
/// At most this many records wait for export. Pending owner sign-off.
pub const MAX_UNEXPORTED: u64 = 10_000;
/// Exported records kept in the outbox. Pending owner sign-off.
pub const RETAIN_EXPORTED: u64 = 5_000;

/// One evidence reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HicEvidence {
    pub kind: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// One HIC event as core decided it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HicEvent {
    /// `grant.folder_added`, `grant.revoked`, `grant.reset`, `grant.full_access_confirmed`,
    /// `escalation.spend`, `ceremony.approval`, `agent.tool_approval`.
    pub kind: String,
    /// `approved`, `denied`, `auto_within_budget`.
    pub decision: String,
    pub subject: String,
    pub reason: String,
    /// `completed`, `failed`, `outcome_unknown`; `None` for a denial or when core does not know.
    pub outcome: Option<String>,
    pub outcome_detail: Option<String>,
    pub evidence: Vec<HicEvidence>,
}

/// One outbox record: the event, its id and time, and its place in the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HicRecord {
    pub record_id: u64,
    pub at_ms: u64,
    pub prev_hash: String,
    pub event: HicEvent,
    pub hash: String,
}

/// The first line of a compacted outbox: the last dropped record, which the chain continues from.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BaseLine {
    base_record_id: u64,
    base_hash: String,
}

/// The hashed body (everything but `hash`), in a fixed field order.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    record_id: u64,
    at_ms: u64,
    prev_hash: &'a str,
    event: &'a HicEvent,
}

fn hash_of(
    record_id: u64,
    at_ms: u64,
    prev_hash: &str,
    event: &HicEvent,
) -> Result<String, String> {
    let body = serde_json::to_vec(&Body {
        record_id,
        at_ms,
        prev_hash,
        event,
    })
    .map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(&body);
    Ok(format!("0x{}", hex::encode(h.finalize())))
}

fn is_hash(s: &str) -> bool {
    s.len() == 66 && s.starts_with("0x") && s.as_bytes()[2..].iter().all(|b| b.is_ascii_hexdigit())
}

fn clip(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(n).collect()
}

/// The same rules the sidecar's `/records/core` applies, checked before anything is stored.
pub fn validate(ev: &HicEvent) -> Result<(), String> {
    let allowed: &[&str] = match ev.kind.as_str() {
        "grant.folder_added" | "grant.revoked" | "grant.reset" | "grant.full_access_confirmed" => {
            &["approved"]
        }
        "ceremony.approval" | "agent.tool_approval" => &["approved", "denied"],
        "escalation.spend" => &["approved", "auto_within_budget"],
        other => return Err(format!("unknown decision kind {other:?}")),
    };
    if !allowed.contains(&ev.decision.as_str()) {
        return Err(format!("a {} cannot be {:?}", ev.kind, ev.decision));
    }
    if ev.subject.trim().is_empty() || ev.subject.len() > 300 {
        return Err("the subject is missing or too long".into());
    }
    if ev.reason.len() > 400 {
        return Err("the reason is too long".into());
    }
    match (ev.decision.as_str(), ev.outcome.as_deref()) {
        ("denied", Some(_)) => return Err("a denial has no outcome".into()),
        (_, None | Some("completed" | "failed" | "outcome_unknown")) => {}
        (_, Some(o)) => return Err(format!("unknown outcome {o:?}")),
    }
    if ev.outcome_detail.as_ref().is_some_and(|d| d.len() > 300) {
        return Err("the outcome detail is too long".into());
    }
    if ev.evidence.len() > 4 {
        return Err("at most 4 evidence references".into());
    }
    for e in &ev.evidence {
        if e.kind.is_empty() || e.kind.len() > 64 || e.uri.is_empty() || e.uri.len() > 300 {
            return Err("an evidence reference is malformed".into());
        }
        if e.digest.as_deref().is_some_and(|d| !is_hash(d)) {
            return Err("an evidence digest is malformed".into());
        }
    }
    Ok(())
}

/// What an export pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportResult {
    Exported(usize),
    /// The sidecar has no records folder (anchoring is off), or predates the route.
    NotConfigured,
}

/// Core's HIC outbox (one per folder; one writer at a time inside the process).
pub struct HicOutbox {
    dir: PathBuf,
    max_unexported: u64,
    retain_exported: u64,
    lock: Mutex<()>,
}

impl HicOutbox {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        HicOutbox {
            dir: dir.into(),
            max_unexported: MAX_UNEXPORTED,
            retain_exported: RETAIN_EXPORTED,
            lock: Mutex::new(()),
        }
    }

    /// Other bounds (tests).
    #[cfg(test)]
    pub fn with_bounds(mut self, max_unexported: u64, retain_exported: u64) -> Self {
        self.max_unexported = max_unexported;
        self.retain_exported = retain_exported;
        self
    }

    #[cfg(test)]
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn read_u64(&self, name: &str) -> Result<u64, String> {
        match std::fs::read_to_string(self.dir.join(name)) {
            Ok(t) => t
                .trim()
                .parse()
                .map_err(|_| format!("the outbox {name} file is unreadable")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(format!(
                "the outbox {name} file is unreadable: {}",
                e.kind()
            )),
        }
    }

    fn write_atomic(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let tmp = self.dir.join(format!("{name}.tmp"));
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("outbox: {}", e.kind()))?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| format!("outbox: {}", e.kind()))?;
        std::fs::rename(&tmp, self.dir.join(name)).map_err(|e| format!("outbox: {}", e.kind()))
    }

    /// Every retained record, oldest first, with the chain verified. Any break is an error.
    #[cfg(test)]
    pub fn records(&self) -> Result<Vec<HicRecord>, String> {
        let _g = self.guard();
        self.records_locked()
    }

    fn records_locked(&self) -> Result<Vec<HicRecord>, String> {
        Ok(self.chain_locked()?.1)
    }

    /// The base the retained chain continues from (`(0, GENESIS)` before any compaction) and
    /// every retained record, oldest first, with the chain verified.
    fn chain_locked(&self) -> Result<((u64, String), Vec<HicRecord>), String> {
        let text = match std::fs::read_to_string(self.dir.join(OUTBOX_FILE)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(((0, GENESIS.to_string()), Vec::new()))
            }
            Err(e) => return Err(format!("the outbox is unreadable: {}", e.kind())),
        };
        let mut lines = text.lines().enumerate().peekable();
        let mut base = (0, GENESIS.to_string());
        if let Some((_, first)) = lines.peek() {
            if let Ok(b) = serde_json::from_str::<BaseLine>(first) {
                if !is_hash(&b.base_hash) {
                    return Err("the outbox base line is malformed".into());
                }
                base = (b.base_record_id, b.base_hash);
                lines.next();
            }
        }
        let (mut last_id, mut last_hash) = base.clone();
        let mut out = Vec::new();
        for (n, line) in lines {
            let r: HicRecord = serde_json::from_str(line)
                .map_err(|_| format!("outbox line {} is not a record", n + 1))?;
            let expect = hash_of(r.record_id, r.at_ms, &r.prev_hash, &r.event)?;
            if r.record_id != last_id + 1 || r.prev_hash != last_hash || r.hash != expect {
                return Err(format!(
                    "the outbox chain is broken at record {}",
                    r.record_id
                ));
            }
            last_id = r.record_id;
            last_hash = r.hash.clone();
            out.push(r);
        }
        Ok((base, out))
    }

    /// The id of the last record the sidecar accepted.
    #[cfg(test)]
    pub fn exported(&self) -> Result<u64, String> {
        self.read_u64(CURSOR_FILE)
    }

    /// Append one event (checked, chained, `fsync`ed). Refused when the chain does not verify or
    /// too many records wait for export.
    pub fn append(&self, ev: HicEvent, at_ms: u64) -> Result<HicRecord, String> {
        validate(&ev)?;
        let _g = self.guard();
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("outbox: {}", e.kind()))?;
        let (base, records) = self.chain_locked()?;
        let (last_id, last_hash) = records
            .last()
            .map(|r| (r.record_id, r.hash.clone()))
            .unwrap_or(base);
        let exported = self.read_u64(CURSOR_FILE)?;
        if last_id.saturating_sub(exported) >= self.max_unexported {
            return Err(format!(
                "{} decision records are waiting for the Hermes agent; start Hermes so they can be recorded",
                last_id.saturating_sub(exported)
            ));
        }
        let ev = HicEvent {
            subject: clip(&ev.subject, 300),
            reason: clip(&ev.reason, 400),
            outcome_detail: ev.outcome_detail.as_deref().map(|d| clip(d, 300)),
            ..ev
        };
        let record_id = last_id + 1;
        let hash = hash_of(record_id, at_ms, &last_hash, &ev)?;
        let r = HicRecord {
            record_id,
            at_ms,
            prev_hash: last_hash,
            event: ev,
            hash,
        };
        let mut line = serde_json::to_vec(&r).map_err(|e| e.to_string())?;
        line.push(b'\n');
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(OUTBOX_FILE))
            .map_err(|e| format!("outbox: {}", e.kind()))?;
        f.write_all(&line)
            .and_then(|_| f.sync_all())
            .map_err(|e| format!("outbox: {}", e.kind()))?;
        Ok(r)
    }

    /// Whether an event could be appended now (an escalation checks this before it starts).
    pub fn check_writable(&self) -> Result<(), String> {
        let _g = self.guard();
        let (base, records) = self.chain_locked()?;
        let last = records.last().map(|r| r.record_id).unwrap_or(base.0);
        if last.saturating_sub(self.read_u64(CURSOR_FILE)?) >= self.max_unexported {
            return Err("too many decision records are waiting for the Hermes agent".into());
        }
        Ok(())
    }

    /// Copy every record not yet accepted, oldest first, advancing the cursor after each batch,
    /// then drop exported records beyond the retention bound.
    pub fn export(&self, link: &dyn SidecarLink) -> Result<ExportResult, String> {
        let _g = self.guard();
        let records = self.records_locked()?;
        let mut cursor = self.read_u64(CURSOR_FILE)?;
        let mut total = 0usize;
        loop {
            let batch: Vec<&HicRecord> = records
                .iter()
                .filter(|r| r.record_id > cursor)
                .take(EXPORT_BATCH)
                .collect();
            let Some(last) = batch.last().map(|r| r.record_id) else {
                break;
            };
            let body =
                serde_json::json!({ "records": batch.iter().map(|r| wire(r)).collect::<Vec<_>>() });
            let (status, text) = link.post("/records/core", &body.to_string())?;
            match status {
                200..=299 => {}
                404 => return Ok(ExportResult::NotConfigured),
                _ => return Err(reason_of(&text, status)),
            }
            self.write_atomic(CURSOR_FILE, last.to_string().as_bytes())?;
            cursor = last;
            total += batch.len();
        }
        self.compact_locked(&records, cursor)?;
        Ok(ExportResult::Exported(total))
    }

    fn compact_locked(&self, records: &[HicRecord], cursor: u64) -> Result<(), String> {
        let exported = records.iter().filter(|r| r.record_id <= cursor).count() as u64;
        if exported <= self.retain_exported {
            return Ok(());
        }
        let drop = (exported - self.retain_exported) as usize;
        let Some(last_dropped) = records.get(drop - 1) else {
            return Ok(());
        };
        let mut kept = serde_json::to_vec(&BaseLine {
            base_record_id: last_dropped.record_id,
            base_hash: last_dropped.hash.clone(),
        })
        .map_err(|e| e.to_string())?;
        kept.push(b'\n');
        for r in &records[drop..] {
            kept.extend(serde_json::to_vec(r).map_err(|e| e.to_string())?);
            kept.push(b'\n');
        }
        // One atomic rename: the base line and the kept records replace the file together.
        self.write_atomic(OUTBOX_FILE, &kept)
    }
}

/// The wire form `POST /records/core` takes for one record.
pub fn wire(r: &HicRecord) -> serde_json::Value {
    serde_json::json!({
        "recordId": r.record_id,
        "kind": r.event.kind,
        "decision": r.event.decision,
        "subject": r.event.subject,
        "reason": r.event.reason,
        "outcome": r.event.outcome,
        "outcomeDetail": r.event.outcome_detail,
        "evidence": r.event.evidence,
        "atMs": r.at_ms,
        "hash": r.hash,
    })
}

fn reason_of(text: &str, status: u16) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| format!("the agent answered {status}"))
}

// ---------------------------------------------------------------------------------------------
// The events core records
// ---------------------------------------------------------------------------------------------

fn member_event(kind: &str, subject: String, reason: &str) -> HicEvent {
    HicEvent {
        kind: kind.to_string(),
        decision: "approved".to_string(),
        subject,
        reason: reason.to_string(),
        outcome: Some("completed".to_string()),
        outcome_detail: Some("saved to the grant document".to_string()),
        evidence: Vec::new(),
    }
}

/// Folder grants added (one per access).
pub fn grant_added(root: &str, ids: &[String]) -> HicEvent {
    member_event(
        "grant.folder_added",
        format!("{} ({})", root, ids.join(", ")),
        "the member granted a folder in Settings",
    )
}

pub fn grant_revoked(id: &str) -> HicEvent {
    member_event(
        "grant.revoked",
        id.to_string(),
        "the member revoked a grant in Settings",
    )
}

pub fn grant_reset() -> HicEvent {
    member_event(
        "grant.reset",
        "all folder grants".to_string(),
        "the member set an unreadable grant document aside and started with none",
    )
}

pub fn full_access_confirmed(gid: &str, root: &str, expires_at: u64) -> HicEvent {
    member_event(
        "grant.full_access_confirmed",
        format!("{gid}: read-only full access under {root} until {expires_at}"),
        "the member confirmed the 24 h read-only full-access window (HIC-1)",
    )
}

/// One finished escalation.
pub fn escalation_spent(rec: &crate::escalation::SpendRecord) -> HicEvent {
    let (decision, reason) = match rec.mode {
        crate::escalation::Mode::Budget => (
            "auto_within_budget",
            "inside the member's daily escalation budget (HIC-2)",
        ),
        crate::escalation::Mode::Confirmed => {
            ("approved", "the member confirmed this escalation (HIC-1)")
        }
    };
    let outcome = match rec.outcome.as_str() {
        "answered" => "completed",
        "not_sent" => "failed",
        _ => "outcome_unknown",
    };
    HicEvent {
        kind: "escalation.spend".to_string(),
        decision: decision.to_string(),
        subject: format!("{} ({})", rec.destination, rec.escalation_id),
        reason: reason.to_string(),
        outcome: Some(outcome.to_string()),
        outcome_detail: Some(format!(
            "{}: charged {} micro-USD of a {} micro-USD quote",
            rec.outcome, rec.charged_micros, rec.quoted_micros
        )),
        evidence: Vec::new(),
    }
}

/// The member's answer on an approval card, as the webview reports it. Only the two card kinds;
/// a denial has no outcome, an approval's result is not part of the record.
pub fn card_event(
    kind: &str,
    decision: &str,
    subject: &str,
    reason: &str,
) -> Result<HicEvent, String> {
    if kind != "ceremony.approval" && kind != "agent.tool_approval" {
        return Err(format!("{kind:?} is not an approval card"));
    }
    let ev = HicEvent {
        kind: kind.to_string(),
        decision: decision.to_string(),
        subject: subject.to_string(),
        reason: if reason.trim().is_empty() {
            "the member's answer on the approval card".to_string()
        } else {
            reason.to_string()
        },
        outcome: None,
        outcome_detail: None,
        evidence: Vec::new(),
    };
    validate(&ev)?;
    Ok(ev)
}

// ---------------------------------------------------------------------------------------------
// Production wiring
// ---------------------------------------------------------------------------------------------

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The app's outbox (`<app_local_data>/hermes/hic-outbox`), one per process.
pub fn outbox_for_app<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<&'static HicOutbox, String> {
    static OUTBOX: OnceLock<HicOutbox> = OnceLock::new();
    if let Some(o) = OUTBOX.get() {
        return Ok(o);
    }
    use tauri::Manager;
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes")
        .join(OUTBOX_DIR);
    let _ = OUTBOX.set(HicOutbox::new(dir));
    OUTBOX
        .get()
        .ok_or_else(|| "internal: the decision outbox is unavailable".to_string())
}

/// Copy the outbox into the sidecar's records. Best effort: Hermes may not be running; the next
/// event or the nightly pass tries again.
pub fn export_for_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(outbox) = outbox_for_app(app) else {
        return;
    };
    let Ok(m) = crate::hermes::manager(app) else {
        return;
    };
    if !m.is_running() {
        return;
    }
    if let Err(e) = outbox.export(m) {
        eprintln!("citrate-core: decision records not exported yet: {e}");
    }
}

/// Record one event (fail closed: the error goes back to the caller), then try to export.
pub fn record_for_app<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    ev: HicEvent,
) -> Result<u64, String> {
    let r = outbox_for_app(app)?.append(ev, now_ms())?;
    export_for_app(app);
    Ok(r.record_id)
}

/// **hic_record_decision** — the member's answer on an approval card (a ceremony or an agent tool
/// call), into the decision records the nightly anchor covers.
#[tauri::command]
pub async fn hic_record_decision(
    app: tauri::AppHandle,
    kind: String,
    decision: String,
    subject: String,
    reason: String,
) -> Result<u64, String> {
    crate::blocking::off_main(move || {
        let ev = card_event(&kind, &decision, &subject, &reason)?;
        record_for_app(&app, ev)
    })
    .await
}

#[cfg(test)]
#[path = "hic_records_tests.rs"]
mod tests;
