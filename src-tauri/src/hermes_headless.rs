//! HUP-S1.1 (US-1.1 AC3) — core-hosted tool calls answered from the Tauri side.
//!
//! The agent loop runs in the Hermes sidecar. A tool marked `host: "core"` parks the loop until
//! citrate-core posts its result. Until now only the chat view answered those calls, so a turn
//! started from the node MCP server (`hermes_session_send`) or the `citrate hermes` CLI while the
//! view was idle, or after the webview was closed, waited out the sidecar's 300 s deadline.
//!
//! This module owns that duty on the Tauri side, for every session:
//!
//! - **View lease.** Each time the chat view reads a session's events or posts a tool result
//!   (`hermes_session_events` / `hermes_session_tool_result`), that session's lease is renewed.
//!   A call the view has been shown becomes view-owned and is never taken from it, so a member's
//!   open approval card keeps its call however long the decision takes.
//! - **Dispatcher.** A background thread looks at the sidecar's open sessions. A core call that is
//!   pending on a session the view is not watching (no lease renewal for [`VIEW_LEASE`]) is
//!   claimed and answered here at once:
//!   - a read-only tool core can serve without the view ([`CoreToolHost`]) runs here and its real
//!     result is posted;
//!   - everything else is answered `denied` with a plain reason. Nothing runs, nothing is approved
//!     on the member's behalf: an effectful call needs the member's decision in the app, and a call
//!     the sidecar marked `hic: "required"` is never run here, whatever the tool.
//! - **No double run.** Claim and lease share one lock. When the view reads a page that holds a
//!   call this module claimed, that call's `host` is rewritten to [`HEADLESS_HOST`], and the view
//!   runs only `host: "core"` calls.
//!
//! The answers come from the same sources the app uses ([`crate::node_mcp_live::LiveBackend`]);
//! nothing here is cached or invented.

use crate::hermes::{HermesError, HermesManager};
use crate::node_mcp_protocol::NodeBackend;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// How long after the view last read a session's events (or posted a result) the session still
/// counts as watched. Above the view's longest long-poll (20 s, see `HERMES_CONTROL_TIMEOUT`)
/// with room for the round trip, so a view that is driving a turn never loses its calls.
pub const VIEW_LEASE: Duration = Duration::from_secs(30);
/// How often the dispatcher looks at the sidecar's sessions.
pub const TICK: Duration = Duration::from_millis(500);
/// The `host` a claimed call carries in pages the view reads, so the view never runs it too.
pub const HEADLESS_HOST: &str = "core-headless";
/// Most claims remembered per session (older ones are dropped; their calls are long answered).
const MAX_CLAIMS_PER_SESSION: usize = 256;
/// Longest tool result posted from here (characters).
const MAX_RESULT_CHARS: usize = 16_000;

/// The answer for a call this side cannot run because it needs the member at the app.
pub fn needs_member(tool: &str) -> String {
    format!(
        "{tool} was not run: it needs the member's decision in the Citrate Core chat, which is not open on this conversation. Nothing was done. Ask the member to open Citrate Core and try again."
    )
}

/// The answer for a call the sidecar marked as needing the member's explicit approval.
pub fn needs_hic(tool: &str) -> String {
    format!(
        "{tool} was not run: this conversation read untrusted content, so this call needs the member's explicit approval in the Citrate Core chat, which is not open on this conversation. Nothing was done."
    )
}

/// Core-hosted tools that can be answered without the view (read-only; no approval involved).
pub trait CoreToolHost: Send + Sync {
    /// `None` when `name` is not served here (the call then goes to the member through the view).
    fn run(&self, name: &str, args: &Value) -> Option<Result<String, String>>;
}

/// The read-only chat tools core serves from its own state, through the node backend the
/// citrate-node MCP server uses (the same sources as the app's surfaces).
pub struct NodeBackendTools(pub Arc<dyn NodeBackend>);

/// The chat tools [`NodeBackendTools`] answers. Each is in `READ_ONLY_AGENT_TOOLS`
/// (`src/agent/harness.ts`): no approval gate is skipped by answering it here.
pub const HEADLESS_TOOLS: &[&str] = &["node_status", "groups_list"];

impl CoreToolHost for NodeBackendTools {
    fn run(&self, name: &str, _args: &Value) -> Option<Result<String, String>> {
        if !HEADLESS_TOOLS.contains(&name) {
            return None;
        }
        let r = match name {
            "node_status" => self.0.node_status(),
            "groups_list" => self.0.groups(),
            _ => return None,
        };
        Some(r.map(|v| v.to_string()))
    }
}

#[derive(Default)]
struct SessionLease {
    /// When the view last read events or posted a result for this session.
    seen: Option<Instant>,
    /// Call ids the view was shown while pending; the view runs (or ran) them.
    view_owned: HashSet<String>,
    /// Sequence numbers of the `tool_call` events this side claimed.
    claimed: HashSet<u64>,
    /// Claim order, to bound `claimed`.
    claim_order: Vec<u64>,
}

/// The view leases and claims, shared by the view's commands and the dispatcher.
pub struct ViewLeases {
    ttl: Duration,
    inner: Mutex<HashMap<String, SessionLease>>,
}

/// The process-wide leases (the commands and the dispatcher thread share them).
pub static LEASES: LazyLock<ViewLeases> = LazyLock::new(|| ViewLeases::new(VIEW_LEASE));

/// One claimed call, ready to run.
#[derive(Debug, Clone, PartialEq)]
pub struct Claimed {
    pub session: String,
    pub seq: u64,
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    pub hic_required: bool,
}

fn pending_of(page: &Value) -> HashSet<String> {
    page.get("pendingCoreCalls")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The `(seq, event)` pairs of a sidecar events page.
fn envelopes(page: &Value) -> impl Iterator<Item = (u64, &Value)> {
    page.get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e.get("seq")?.as_u64()?, e.get("event")?)))
}

fn core_call(ev: &Value) -> Option<(&str, &Value)> {
    if ev.get("type").and_then(Value::as_str) != Some("tool_call")
        || ev.get("host").and_then(Value::as_str) != Some("core")
    {
        return None;
    }
    let call = ev.get("call")?;
    Some((call.get("id")?.as_str()?, call))
}

impl ViewLeases {
    pub fn new(ttl: Duration) -> Self {
        ViewLeases {
            ttl,
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionLease>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The view posted a tool result for `session` (`hermes_session_tool_result`): the lease is
    /// renewed and the call is the view's no longer.
    pub fn view_answered(&self, session: &str, call_id: &str, now: Instant) {
        let mut g = self.lock();
        let l = g.entry(session.to_string()).or_default();
        l.seen = Some(now);
        l.view_owned.remove(call_id);
    }

    /// The view read `page` for `session` (`hermes_session_events`). Renews the lease, marks the
    /// pending core calls it shows as view-owned, and rewrites the `host` of every call this side
    /// claimed to [`HEADLESS_HOST`] so the view does not run it again.
    pub fn view_page(&self, session: &str, mut page: Value, now: Instant) -> Value {
        let pending = pending_of(&page);
        let mut g = self.lock();
        let l = g.entry(session.to_string()).or_default();
        l.seen = Some(now);
        if let Some(events) = page.get_mut("events").and_then(Value::as_array_mut) {
            for env in events.iter_mut() {
                let Some(seq) = env.get("seq").and_then(Value::as_u64) else {
                    continue;
                };
                let Some(ev) = env.get_mut("event") else {
                    continue;
                };
                let Some((id, _)) = core_call(ev) else {
                    continue;
                };
                let id = id.to_string();
                if l.claimed.contains(&seq) {
                    if let Some(o) = ev.as_object_mut() {
                        o.insert("host".into(), json!(HEADLESS_HOST));
                    }
                } else if pending.contains(&id) {
                    l.view_owned.insert(id);
                }
            }
        }
        page
    }

    /// Whether the view is watching `session` at `now`.
    pub fn watched(&self, session: &str, now: Instant) -> bool {
        self.lock()
            .get(session)
            .and_then(|l| l.seen)
            .is_some_and(|t| now.saturating_duration_since(t) < self.ttl)
    }

    /// Claim the pending core calls in `page` (the session's events from the start of its kept
    /// log) that the view does not own, when the view is not watching `session`. A call already
    /// claimed is not claimed twice. Returns what to run.
    pub fn claim(&self, session: &str, page: &Value, now: Instant) -> Vec<Claimed> {
        let pending = pending_of(page);
        let mut g = self.lock();
        let l = g.entry(session.to_string()).or_default();
        // Calls no longer pending are answered: the view gives them up.
        l.view_owned.retain(|id| pending.contains(id));
        if pending.is_empty()
            || l.seen
                .is_some_and(|t| now.saturating_duration_since(t) < self.ttl)
        {
            return Vec::new();
        }
        // The latest tool_call event for each pending id (ids are reused across steps).
        let mut latest: HashMap<&str, (u64, &Value, &Value)> = HashMap::new();
        for (seq, ev) in envelopes(page) {
            if let Some((id, call)) = core_call(ev) {
                if pending.contains(id) {
                    latest.insert(id, (seq, call, ev));
                }
            }
        }
        let mut out = Vec::new();
        let mut ids: Vec<&str> = latest.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let (seq, call, ev) = latest[id];
            if l.view_owned.contains(id) || l.claimed.contains(&seq) {
                continue;
            }
            l.claimed.insert(seq);
            l.claim_order.push(seq);
            if l.claim_order.len() > MAX_CLAIMS_PER_SESSION {
                let old = l.claim_order.remove(0);
                l.claimed.remove(&old);
            }
            let arguments = match call.get("arguments") {
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
                Some(v) => v.clone(),
                None => json!({}),
            };
            out.push(Claimed {
                session: session.to_string(),
                seq,
                call_id: id.to_string(),
                name: call
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                arguments,
                hic_required: ev.get("hic").and_then(Value::as_str) == Some("required"),
            });
        }
        out
    }

    /// Give a claim back (its result could not be posted).
    pub fn unclaim(&self, session: &str, seq: u64) {
        if let Some(l) = self.lock().get_mut(session) {
            l.claimed.remove(&seq);
            l.claim_order.retain(|s| *s != seq);
        }
    }

    /// Forget sessions the sidecar no longer lists.
    pub fn retain_sessions(&self, open: &HashSet<String>) {
        self.lock().retain(|k, _| open.contains(k));
    }
}

/// What to post for one claimed call: `(status, content)`.
pub fn answer(host: &dyn CoreToolHost, c: &Claimed) -> (&'static str, String) {
    if c.hic_required {
        return ("denied", needs_hic(&c.name));
    }
    if !c.arguments.is_object() {
        return (
            "error",
            "tool arguments must be a JSON object; nothing was run".to_string(),
        );
    }
    match host.run(&c.name, &c.arguments) {
        Some(Ok(text)) => ("ok", text.chars().take(MAX_RESULT_CHARS).collect()),
        Some(Err(e)) => ("error", format!("{} unavailable: {e}", c.name)),
        None => ("denied", needs_member(&c.name)),
    }
}

/// One dispatcher pass over the sidecar's sessions. Returns how many calls were answered here.
/// A sidecar that is not running is not an error (nothing to answer).
pub fn tick(
    leases: &ViewLeases,
    mgr: &HermesManager,
    host: &dyn CoreToolHost,
    now: Instant,
) -> Result<usize, HermesError> {
    let resp = match mgr.control_get_path("/sessions") {
        Ok(r) => r,
        Err(HermesError::NotRunning) => return Ok(0),
        Err(e) => return Err(e),
    };
    if !(200..300).contains(&resp.status) {
        return Err(HermesError::Control {
            status: resp.status,
            msg: resp.body.chars().take(200).collect(),
        });
    }
    let list: Value =
        serde_json::from_str(&resp.body).map_err(|e| HermesError::Decode(e.to_string()))?;
    let sessions = list
        .get("sessions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut open = HashSet::new();
    let mut answered = 0;
    for s in &sessions {
        let Some(id) = s.get("id").and_then(Value::as_str) else {
            continue;
        };
        open.insert(id.to_string());
        let waiting = s
            .get("pendingCoreCalls")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        if !waiting || leases.watched(id, now) {
            continue;
        }
        let page = match mgr.session_events(id, 0, 0) {
            Ok(p) => p,
            // The session ended between the list and this read.
            Err(HermesError::Control { status: 404, .. }) => continue,
            Err(e) => return Err(e),
        };
        for c in leases.claim(id, &page, now) {
            let (status, content) = answer(host, &c);
            match mgr.session_tool_result(&c.session, &c.call_id, status, &content) {
                Ok(()) => answered += 1,
                // 404: the session ended; 409: the loop stopped waiting for this call meanwhile.
                Err(HermesError::Control {
                    status: 404 | 409, ..
                }) => {}
                Err(e) => {
                    // Not answered: let the next pass try this call again.
                    leases.unclaim(&c.session, c.seq);
                    return Err(e);
                }
            }
        }
    }
    leases.retain_sessions(&open);
    Ok(answered)
}

/// Start the dispatcher thread for the app (once). It sleeps while the sidecar is not running.
pub fn spawn(app: tauri::AppHandle) {
    let host = NodeBackendTools(Arc::new(crate::node_mcp_live::LiveBackend::new(app)));
    let spawned = std::thread::Builder::new()
        .name("hermes-headless".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            if let Some(mgr) = crate::hermes::running_manager() {
                // A failed pass (the sidecar restarting) is retried at the next tick.
                let _ = tick(&LEASES, mgr, &host, Instant::now());
            }
        });
    if let Err(e) = spawned {
        eprintln!("hermes: the headless tool dispatcher could not start: {e}");
    }
}

#[cfg(test)]
mod tests {
    include!("hermes_headless_tests.rs");
}
