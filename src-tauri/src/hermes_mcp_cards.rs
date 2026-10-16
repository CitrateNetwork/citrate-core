//! HUP-S4.1 (US-4.1 AC2, core half): the member's decision on an MCP card the sidecar holds.
//!
//! The runtime (`citrate-agent-runtime` agent-sidecar) holds two kinds of MCP request for the
//! member, per session (`GET /sessions/:id/mcp/pending`):
//!
//! - `tool_call`: an effectful MCP call after the session read untrusted content. The card shows
//!   the server, the tool, the exact arguments that will be sent and the server's hints.
//! - `open_url`: a URL-mode elicitation (MCP 2026-07-28). The card shows the full URL and its host
//!   with any warnings. On an allow, core (never the sidecar, never the model) opens it in the
//!   system browser.
//!
//! The decision goes back with the subject the member was shown (`POST .../mcp/decide`). Core
//! re-reads what is waiting before deciding, so the URL it opens is the one the sidecar holds, not
//! one the webview supplied. Nothing here signs, spends or holds a key.

use super::{manager, HermesError, HermesManager, Result as HResult};

/// Prefix of the error when the member allowed a page that the system could not open (the
/// decision itself reached the sidecar).
pub(crate) const PAGE_NOT_OPENED: &str = "MCP_PAGE_NOT_OPENED";

/// Longest subject a decision may carry (the sidecar caps arguments at 8 KiB and URLs at 2048).
pub(crate) const MAX_SUBJECT_BYTES: usize = 9 * 1024;

/// A card id as the sidecar mints it (`mcp-` + digits).
pub(crate) fn valid_card_id(id: &str) -> std::result::Result<(), String> {
    let digits = id.strip_prefix("mcp-").unwrap_or_default();
    if digits.is_empty() || digits.len() > 18 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err("that is not an MCP approval id".to_string());
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

/// Only an http(s) address with a host and no credentials is ever handed to the opener.
pub(crate) fn openable(url: &str) -> bool {
    match url::Url::parse(url) {
        Ok(u) => {
            matches!(u.scheme(), "https" | "http")
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
        }
        Err(_) => false,
    }
}

/// `GET /sessions/:id/mcp/pending`: the MCP cards waiting for the member in that session.
pub(crate) fn mcp_pending(
    m: &HermesManager,
    session_id: &str,
) -> std::result::Result<serde_json::Value, String> {
    super::valid_session_id(session_id)?;
    let run = || -> HResult<super::ControlResp> {
        let bearer = m.bearer()?;
        m.control.get(
            &format!("{}/sessions/{session_id}/mcp/pending", m.control_url()),
            &bearer,
        )
    };
    let resp = run().map_err(transport)?;
    if !(200..300).contains(&resp.status) {
        return Err(reason(resp.status, &resp.body));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.body)
        .map_err(|_| "the sidecar's MCP requests could not be read".to_string())?;
    match v.get("pending") {
        Some(list @ serde_json::Value::Array(_)) => Ok(list.clone()),
        _ => Err("the sidecar's MCP requests could not be read".to_string()),
    }
}

/// `POST /sessions/:id/mcp/decide`: the member's decision, bound to the subject shown. Returns the
/// URL to open when the member allowed an `open_url` card (taken from what the sidecar holds).
pub(crate) fn mcp_decide(
    m: &HermesManager,
    session_id: &str,
    id: &str,
    allow: bool,
    subject: &str,
) -> std::result::Result<Option<String>, String> {
    super::valid_session_id(session_id)?;
    valid_card_id(id)?;
    if subject.len() > MAX_SUBJECT_BYTES || subject.contains('\0') {
        return Err("the decision's subject is too long or contains a NUL byte".to_string());
    }
    let waiting = mcp_pending(m, session_id)?;
    let card = waiting
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(id))
        })
        .cloned()
        .ok_or_else(|| "MCP_DECISION_REFUSED: that request is no longer waiting".to_string())?;
    if card.get("subject").and_then(|v| v.as_str()) != Some(subject) {
        return Err(
            "MCP_DECISION_REFUSED: the decision does not match what is waiting".to_string(),
        );
    }
    let open = match card.get("kind").and_then(|v| v.as_str()) {
        Some("open_url") if allow => {
            let url = card.get("url").and_then(|v| v.as_str()).unwrap_or_default();
            if url != subject || !openable(url) {
                return Err(
                    "MCP_DECISION_REFUSED: that address cannot be opened from the app".to_string(),
                );
            }
            Some(url.to_string())
        }
        _ => None,
    };
    let body = serde_json::json!({ "id": id, "allow": allow, "subject": subject }).to_string();
    let run = || -> HResult<super::ControlResp> {
        let bearer = m.bearer()?;
        m.control.post(
            &format!("{}/sessions/{session_id}/mcp/decide", m.control_url()),
            &bearer,
            &body,
        )
    };
    let resp = run().map_err(transport)?;
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "MCP_DECISION_REFUSED: {}",
            reason(resp.status, &resp.body)
        ));
    }
    Ok(open)
}

/// **hermes_mcp_pending**: the MCP requests Hermes is waiting on in a session (main window only).
#[tauri::command]
pub async fn hermes_mcp_pending(
    app: tauri::AppHandle,
    session_id: String,
) -> std::result::Result<serde_json::Value, String> {
    crate::blocking::off_main(move || mcp_pending(manager(&app)?, &session_id)).await
}

/// **hermes_mcp_decide**: allow or decline one MCP request, bound to the subject shown. For an
/// allowed `open_url` request, core opens the address in the system browser (main window only).
#[tauri::command]
pub async fn hermes_mcp_decide(
    app: tauri::AppHandle,
    session_id: String,
    id: String,
    allow: bool,
    subject: String,
) -> std::result::Result<(), String> {
    let app2 = app.clone();
    let open = crate::blocking::off_main(move || {
        mcp_decide(manager(&app2)?, &session_id, &id, allow, &subject)
    })
    .await?;
    if let Some(url) = open {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("{PAGE_NOT_OPENED}: the page could not be opened ({e})"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("hermes_mcp_cards_tests.rs");
}
