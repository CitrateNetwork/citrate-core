//! `session/request_permission` answers (L1-WP03; the HIC mapping itself is L1-WP06 on L2's
//! `HicPolicy`). Core decides; the agent's options only say how to express the decision. An allow
//! is only ever `allow_once` (no standing grant from an agent's menu); a deny prefers `reject_once`.

use serde_json::{json, Value};

/// What the agent asked for, read from its `RequestPermissionRequest`.
#[derive(Debug, Clone)]
pub struct PermissionAsk {
    pub session_id: String,
    pub tool_call_id: String,
    /// ACP tool kind: read, edit, delete, move, search, execute, think, fetch, other.
    pub tool_kind: Option<String>,
    pub title: Option<String>,
    /// `(optionId, kind)` pairs as offered.
    pub options: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow(String),
    Deny(String),
}

/// The decision maker. Implementations must be fail-closed: anything they do not recognise is a
/// deny.
pub trait PermissionPolicy: Send + Sync {
    fn decide(&self, ask: &PermissionAsk) -> Verdict;
}

/// The default until L2's HIC policy and approval inbox land: every request is denied, writes and
/// reads alike, because no member decision can be collected yet.
pub struct DenyByDefault;

impl PermissionPolicy for DenyByDefault {
    fn decide(&self, ask: &PermissionAsk) -> Verdict {
        Verdict::Deny(format!(
            "no HIC policy is installed yet; {} denied",
            ask.tool_kind.as_deref().unwrap_or("tool")
        ))
    }
}

impl PermissionAsk {
    /// Reads the request's JSON form (`toolCall.{toolCallId,kind,title}`, `options[]`).
    pub fn from_request(v: &Value) -> PermissionAsk {
        let s = |p: &str| v.pointer(p).and_then(Value::as_str).map(String::from);
        let options = v["options"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| {
                        Some((
                            o["optionId"].as_str()?.to_string(),
                            o["kind"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        PermissionAsk {
            session_id: s("/sessionId").unwrap_or_default(),
            tool_call_id: s("/toolCall/toolCallId").unwrap_or_default(),
            tool_kind: s("/toolCall/kind"),
            title: s("/toolCall/title"),
            options,
        }
    }
}

/// The ACP outcome for a verdict: `{"outcome":"selected","optionId":..}` or `cancelled` when the
/// agent offered no option of the needed kind.
pub fn outcome(verdict: &Verdict, options: &[(String, String)]) -> Value {
    let pick = |kinds: &[&str]| {
        kinds.iter().find_map(|k| {
            options
                .iter()
                .find(|(_, kind)| kind == k)
                .map(|(id, _)| id.clone())
        })
    };
    let chosen = match verdict {
        Verdict::Allow(_) => pick(&["allow_once"]),
        Verdict::Deny(_) => pick(&["reject_once", "reject_always"]),
    };
    match chosen {
        Some(id) => json!({"outcome": "selected", "optionId": id}),
        None => json!({"outcome": "cancelled"}),
    }
}
