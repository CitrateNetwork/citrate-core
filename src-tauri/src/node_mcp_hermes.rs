//! HUP-S1.1 (g1-loop) — Hermes sessions over the citrate-node MCP server.
//!
//! The agent loop runs in the Hermes sidecar, and its sessions (the app's chat included) live in
//! the sidecar's session store. These tools let an MCP client reach the same sessions the app and
//! the `citrate-agent hermes` CLI use:
//!
//! - `hermes_session_list` (read): the open sessions (id, model, busy, last sequence number).
//! - `hermes_session_events` (read): a session's events after a sequence number, with a bounded
//!   long-poll. Tool arguments and tool results are withheld over MCP (pending owner sign-off):
//!   they can carry what the member's own tools returned, including personal memory, which this
//!   server does not expose. Event types, tool names and statuses, streamed text, final answers
//!   and verifier verdicts are included.
//! - `hermes_session_send` / `hermes_session_stop` (write): queued in the approval inbox like every
//!   other MCP write. Nothing reaches the session until the member approves it in the app.
//!
//! Every answer comes from the sidecar's control API through core's bearer-authed Hermes client
//! (`hermes.rs`); nothing is cached or invented here.

use crate::node_mcp_approvals::McpAction;
use crate::node_mcp_tools::{self as tools, ToolDef, ToolKind};
use serde_json::{json, Value};

/// Said when the backend has no Hermes sidecar to ask.
pub const HERMES_UNAVAILABLE: &str =
    "Hermes sessions are not available: the Hermes sidecar is not running in Citrate Core";

/// Longest long-poll an MCP call may ask for (below the transport's 15 s socket timeout).
pub const MAX_WAIT_MS: u64 = 8_000;
/// Most events one `hermes_session_events` answer carries; the client continues from `nextAfter`.
pub const MAX_EVENTS: usize = 100;
/// Longest message `hermes_session_send` accepts (characters).
pub const MAX_TEXT_CHARS: usize = 4_000;
/// Longest text shown per streamed or final event (characters); the rest is cut with a marker.
pub const MAX_EVENT_TEXT_CHARS: usize = 8_000;
/// How much of a message the approval card shows (characters).
const SUMMARY_TEXT_CHARS: usize = 240;

/// Put in place of tool arguments and tool results over MCP.
pub const WITHHELD: &str = "withheld over MCP";

fn obj(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn list_schema() -> Value {
    obj(json!({}), &[])
}

fn events_schema() -> Value {
    obj(
        json!({
            "session": {"type": "string", "description": "session id from hermes_session_list"},
            "since_seq": {"type": "integer", "minimum": 0, "description": "return events after this sequence number (0 = from the start of the kept log)"},
            "wait_ms": {"type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS, "description": "wait up to this long for a new event when there is none (default 0)"},
        }),
        &["session"],
    )
}

fn send_schema() -> Value {
    obj(
        json!({
            "session": {"type": "string", "description": "session id from hermes_session_list"},
            "text": {"type": "string", "description": "the message to send, 1 to 4000 characters"},
        }),
        &["session", "text"],
    )
}

fn stop_schema() -> Value {
    obj(
        json!({"session": {"type": "string", "description": "session id from hermes_session_list"}}),
        &["session"],
    )
}

/// The Hermes session tools, appended to the node catalog by `node_mcp_tools::all_tools`.
pub const HERMES_TOOLS: &[ToolDef] = &[
    ToolDef { name: "hermes_session_list", title: "Hermes sessions", kind: ToolKind::Read, input_schema: list_schema,
        description: "The open Hermes agent sessions on this node, the member's app chat included: id, model, whether a turn is running, and the last event sequence number." },
    ToolDef { name: "hermes_session_events", title: "Hermes session events", kind: ToolKind::Read, input_schema: events_schema,
        description: "A Hermes session's events after since_seq (streamed text, tool calls by name, verifier verdicts, final answers). Waits up to wait_ms (at most 8000) for a new event. Continue with nextAfter. Tool arguments and results are withheld over MCP." },
    ToolDef { name: "hermes_session_send", title: "Send to a Hermes session", kind: ToolKind::Action, input_schema: send_schema,
        description: "Ask to send a message to a Hermes session, which starts a turn. Runs only after the member approves it in Citrate Core. Returns a request id for request_status; follow the turn with hermes_session_events." },
    ToolDef { name: "hermes_session_stop", title: "Stop a Hermes session's turn", kind: ToolKind::Action, input_schema: stop_schema,
        description: "Ask to stop the turn a Hermes session is running. Runs only after the member approves it in Citrate Core." },
];

/// A session id as the sidecar mints it: 1..=64 ASCII letters, digits and `-`.
pub fn parse_session(s: &str) -> Result<String, String> {
    if s.is_empty() || s.len() > 64 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("session must be a session id from hermes_session_list".to_string());
    }
    Ok(s.to_string())
}

/// A message: 1..=4000 characters after trimming; no control characters other than newline and
/// tab, and no bidirectional overrides (the approval card shows it).
pub fn parse_text(s: &str) -> Result<String, String> {
    let t = s.trim();
    let bad = t
        .chars()
        .any(|c| c != '\n' && c != '\t' && tools::is_unsafe_display_char(c));
    if t.is_empty() || t.chars().count() > MAX_TEXT_CHARS || bad {
        return Err(format!(
            "text must be 1 to {MAX_TEXT_CHARS} characters with no control characters"
        ));
    }
    Ok(t.to_string())
}

fn opt_u64(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("argument `{key}` must be a non-negative integer")),
    }
}

/// The one-line form of a message for the approval card.
pub fn summary_text(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !tools::is_unsafe_display_char(*c))
        .collect();
    if flat.chars().count() > SUMMARY_TEXT_CHARS {
        let cut: String = flat.chars().take(SUMMARY_TEXT_CHARS).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

fn clip(s: &str) -> Value {
    if s.chars().count() > MAX_EVENT_TEXT_CHARS {
        let cut: String = s.chars().take(MAX_EVENT_TEXT_CHARS).collect();
        json!(format!("{cut}… (cut over MCP)"))
    } else {
        json!(s)
    }
}

/// One sidecar event as an MCP client sees it: tool arguments and results withheld, long text cut.
pub fn redact_event(ev: &Value) -> Value {
    let mut out = ev.clone();
    let Some(o) = out.as_object_mut() else {
        return json!({"type": "unknown"});
    };
    match o.get("type").and_then(Value::as_str).unwrap_or("") {
        "tool_call" => {
            if let Some(call) = o.get_mut("call").and_then(Value::as_object_mut) {
                if call.contains_key("arguments") {
                    call.insert("arguments".into(), json!(WITHHELD));
                }
            }
        }
        "tool_result" => {
            o.insert("content".into(), json!(WITHHELD));
        }
        "assistant_delta" => {
            let t = o
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            o.insert("text".into(), clip(&t));
        }
        "final" => {
            let t = o
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            o.insert("content".into(), clip(&t));
        }
        _ => {}
    }
    out
}

/// The `hermes_session_events` answer from one sidecar events page: at most [`MAX_EVENTS`]
/// events after `since`, redacted, with where to continue.
pub fn events_view(session: &str, since: u64, page: &Value) -> Value {
    let all: Vec<&Value> = page
        .get("events")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|e| {
                    e.get("seq")
                        .and_then(Value::as_u64)
                        .is_some_and(|s| s > since)
                })
                .collect()
        })
        .unwrap_or_default();
    let more = all.len() > MAX_EVENTS;
    let shown: Vec<Value> = all
        .iter()
        .take(MAX_EVENTS)
        .map(|e| {
            json!({
                "seq": e.get("seq").cloned().unwrap_or(Value::Null),
                "event": redact_event(e.get("event").unwrap_or(&Value::Null)),
            })
        })
        .collect();
    let last_seq = page.get("lastSeq").and_then(Value::as_u64).unwrap_or(since);
    let next_after = shown
        .last()
        .and_then(|e| e.get("seq").and_then(Value::as_u64))
        .unwrap_or_else(|| last_seq.max(since));
    json!({
        "session": session,
        "events": shown,
        "nextAfter": next_after,
        "lastSeq": last_seq,
        "more": more,
        "busy": page.get("busy").and_then(Value::as_bool).unwrap_or(false),
        "waitingOnAppTools": page.get("pendingCoreCalls").and_then(Value::as_array).map_or(0, Vec::len),
    })
}

/// Where the sessions come from (the app's Hermes client in production, a fixture in tests).
pub trait HermesSessions {
    /// The sidecar's `GET /sessions` body.
    fn list(&self) -> Result<Value, String>;
    /// The sidecar's `GET /sessions/:id/events` body.
    fn events(&self, session: &str, after: u64, wait_ms: u64) -> Result<Value, String>;
}

/// Answer a Hermes read tool. `None` when `name` is not one of them.
pub fn read_tool(
    src: &dyn HermesSessions,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "hermes_session_list" => Some(src.list().map(
            |v| json!({ "sessions": v.get("sessions").cloned().unwrap_or_else(|| json!([])) }),
        )),
        "hermes_session_events" => Some((|| {
            let session = parse_session(tools::arg_str(args, "session")?)?;
            let since = opt_u64(args, "since_seq")?.unwrap_or(0);
            let wait = opt_u64(args, "wait_ms")?.unwrap_or(0);
            if wait > MAX_WAIT_MS {
                return Err(format!("wait_ms must be at most {MAX_WAIT_MS}"));
            }
            let page = src.events(&session, since, wait)?;
            Ok(events_view(&session, since, &page))
        })()),
        _ => None,
    }
}

/// The inbox action for a Hermes write tool. `None` when `name` is not one of them.
pub fn action_for(name: &str, args: &Value) -> Option<Result<McpAction, String>> {
    match name {
        "hermes_session_send" => Some((|| {
            Ok(McpAction::HermesSessionSend {
                session: parse_session(tools::arg_str(args, "session")?)?,
                text: parse_text(tools::arg_str(args, "text")?)?,
            })
        })()),
        "hermes_session_stop" => Some((|| {
            Ok(McpAction::HermesSessionStop {
                session: parse_session(tools::arg_str(args, "session")?)?,
            })
        })()),
        _ => None,
    }
}

/// A [`crate::node_mcp_protocol::NodeBackend`] as a session source.
pub struct ViaBackend<'a>(pub &'a dyn crate::node_mcp_protocol::NodeBackend);

impl HermesSessions for ViaBackend<'_> {
    fn list(&self) -> Result<Value, String> {
        self.0.hermes_sessions()
    }
    fn events(&self, session: &str, after: u64, wait_ms: u64) -> Result<Value, String> {
        self.0.hermes_events(session, after, wait_ms)
    }
}

/// A Hermes client error in the words an MCP client sees.
fn hermes_error(e: crate::hermes::HermesError, session: Option<&str>) -> String {
    let m = e.to_string();
    match (&e, session) {
        (crate::hermes::HermesError::NotRunning, _) => HERMES_UNAVAILABLE.to_string(),
        (crate::hermes::HermesError::Control { status: 404, .. }, Some(s)) => {
            format!("no Hermes session {s} (it may have ended when Hermes restarted)")
        }
        (crate::hermes::HermesError::Control { status: 409, .. }, Some(_)) => {
            "the session is already running a turn; try again when it finishes".to_string()
        }
        _ => m,
    }
}

/// The app's Hermes client as a session source.
pub struct ManagerSessions<'a>(pub &'a crate::hermes::HermesManager);

impl HermesSessions for ManagerSessions<'_> {
    fn list(&self) -> Result<Value, String> {
        let resp = self
            .0
            .control_get_path("/sessions")
            .map_err(|e| hermes_error(e, None))?;
        if !(200..300).contains(&resp.status) {
            return Err(format!("the Hermes sidecar answered {}", resp.status));
        }
        serde_json::from_str(&resp.body)
            .map_err(|e| format!("the Hermes sidecar sent an unreadable session list: {e}"))
    }

    fn events(&self, session: &str, after: u64, wait_ms: u64) -> Result<Value, String> {
        self.0
            .session_events(session, after, wait_ms.min(MAX_WAIT_MS))
            .map_err(|e| hermes_error(e, Some(session)))
    }
}

/// Run an approved Hermes action through the app's Hermes client.
pub fn run_action(mgr: &crate::hermes::HermesManager, action: &McpAction) -> Result<Value, String> {
    match action {
        McpAction::HermesSessionSend { session, text } => {
            mgr.session_send(session, text)
                .map_err(|e| hermes_error(e, Some(session)))?;
            Ok(
                json!({"sent": true, "session": session, "next": "Follow the turn with hermes_session_events."}),
            )
        }
        McpAction::HermesSessionStop { session } => {
            mgr.session_stop(session)
                .map_err(|e| hermes_error(e, Some(session)))?;
            Ok(json!({"stopped": true, "session": session}))
        }
        _ => Err("not a Hermes session action".to_string()),
    }
}

#[cfg(test)]
mod tests {
    include!("node_mcp_hermes_tests.rs");
}
