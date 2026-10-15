//! HGW-W1 (core half): the member's decision on an action card the sidecar holds.
//!
//! Owner decision 2026-10-15 (HGW (2)): once a session read untrusted content, an effectful
//! sidecar-hosted call (`fs_*`, `file_write`, `sheet_write`, the toolchain tools, `project_create`,
//! capsules) waits for the member instead of being refused for the rest of the conversation. The
//! runtime (`citrate-agent-runtime` agent-sidecar, `action_approvals`) holds it per session:
//!
//! - `GET /sessions/:id/approvals/pending`: the tool, the exact canonical arguments (`subject`),
//!   a one-line summary, why it is gated (`hicReason`) and every taint source (`taintSources`);
//! - `POST /sessions/:id/approvals/:aid/decide {allow, subject}`: the decision, bound to the
//!   subject shown. Core re-reads what is waiting before deciding, so a decision is never sent for
//!   arguments the member was not shown. An allow releases that one call; the session stays tainted.
//!
//! `shell_run` and MCP keep their own cards ([`super::shell`], [`super::mcp_cards`]). Nothing here
//! runs a tool, signs, spends or holds a key.

use super::{manager, HermesError, HermesManager, Result as HResult};

/// Longest subject a decision may carry (the sidecar's tool arguments are bounded well below).
pub(crate) const MAX_SUBJECT_BYTES: usize = 64 * 1024;

/// A card id as the sidecar mints it (`act-` + digits).
pub(crate) fn valid_card_id(id: &str) -> std::result::Result<(), String> {
    let digits = id.strip_prefix("act-").unwrap_or_default();
    if digits.is_empty() || digits.len() > 18 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err("that is not an action approval id".to_string());
    }
    Ok(())
}

fn reason(status: u16, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| format!("the sidecar answered {status}"))
        .chars()
        .take(300)
        .collect()
}

fn transport(e: HermesError) -> String {
    match e {
        HermesError::NotRunning => "Hermes is not running; start it first".to_string(),
        other => other.to_string(),
    }
}

/// `GET /sessions/:id/approvals/pending`: the action cards waiting for the member in that session.
pub(crate) fn action_pending(
    m: &HermesManager,
    session_id: &str,
) -> std::result::Result<serde_json::Value, String> {
    super::valid_session_id(session_id)?;
    let run = || -> HResult<super::ControlResp> {
        let bearer = m.bearer()?;
        m.control.get(
            &format!(
                "{}/sessions/{session_id}/approvals/pending",
                m.control_url()
            ),
            &bearer,
        )
    };
    let resp = run().map_err(transport)?;
    if !(200..300).contains(&resp.status) {
        return Err(reason(resp.status, &resp.body));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.body)
        .map_err(|_| "the sidecar's waiting actions could not be read".to_string())?;
    match v.get("pending") {
        Some(list @ serde_json::Value::Array(_)) => Ok(list.clone()),
        _ => Err("the sidecar's waiting actions could not be read".to_string()),
    }
}

/// `POST /sessions/:id/approvals/:aid/decide`: the member's decision, bound to the subject shown.
pub(crate) fn action_decide(
    m: &HermesManager,
    session_id: &str,
    id: &str,
    allow: bool,
    subject: &str,
) -> std::result::Result<(), String> {
    super::valid_session_id(session_id)?;
    valid_card_id(id)?;
    if subject.len() > MAX_SUBJECT_BYTES || subject.contains('\0') {
        return Err("the decision's subject is too long or contains a NUL byte".to_string());
    }
    let waiting = action_pending(m, session_id)?;
    let card = waiting
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(id))
        })
        .cloned()
        .ok_or_else(|| "ACTION_DECISION_REFUSED: that action is no longer waiting".to_string())?;
    if card.get("subject").and_then(|v| v.as_str()) != Some(subject) {
        return Err(
            "ACTION_DECISION_REFUSED: the decision does not match what is waiting".to_string(),
        );
    }
    let body = serde_json::json!({ "allow": allow, "subject": subject }).to_string();
    let run = || -> HResult<super::ControlResp> {
        let bearer = m.bearer()?;
        m.control.post(
            &format!(
                "{}/sessions/{session_id}/approvals/{id}/decide",
                m.control_url()
            ),
            &bearer,
            &body,
        )
    };
    let resp = run().map_err(transport)?;
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "ACTION_DECISION_REFUSED: {}",
            reason(resp.status, &resp.body)
        ));
    }
    Ok(())
}

/// **hermes_action_pending**: the actions Hermes is waiting on in a session (main window only).
#[tauri::command]
pub async fn hermes_action_pending(
    app: tauri::AppHandle,
    session_id: String,
) -> std::result::Result<serde_json::Value, String> {
    crate::blocking::off_main(move || action_pending(manager(&app)?, &session_id)).await
}

/// **hermes_action_decide**: allow or decline one waiting action, bound to the subject shown
/// (main window only).
#[tauri::command]
pub async fn hermes_action_decide(
    app: tauri::AppHandle,
    session_id: String,
    id: String,
    allow: bool,
    subject: String,
) -> std::result::Result<(), String> {
    crate::blocking::off_main(move || {
        action_decide(manager(&app)?, &session_id, &id, allow, &subject)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_action_cards_tests.rs");
}
