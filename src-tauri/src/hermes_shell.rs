//! HUP-S2.2 (core half): the member's decision on a command Hermes wants to run.
//!
//! The runtime (`citrate-agent-runtime` agent-sidecar, `shell_run`, off by default) holds every
//! `shell_run` call until the member decides (HIC required, never automatic). Core reads what is
//! waiting (`GET /sessions/:id/shell/pending`: the exact argv, the canonical folder, the resolved
//! program, the timeout and the OS sandbox summary), shows it on an approval card, and sends the
//! decision back (`POST /sessions/:id/shell/decide`). The decision carries the exact argv and
//! folder the member was shown; the sidecar refuses it when they differ from what is waiting, so
//! an approval can never run a different command. Core bounds the input before it leaves the app.
//! Nothing here runs a command, signs, spends or holds a key.

use super::{manager, HermesError, HermesManager, Result as HResult};

/// Most arguments a decision may carry.
pub(crate) const MAX_ARGS: usize = 256;
/// Longest single argument.
pub(crate) const MAX_ARG_CHARS: usize = 4096;
/// Longest folder path.
pub(crate) const MAX_CWD_CHARS: usize = 4096;

/// A waiting command's id as the sidecar mints it (`sh-` + digits).
pub(crate) fn valid_approval_id(id: &str) -> std::result::Result<(), String> {
    let digits = id.strip_prefix("sh-").unwrap_or_default();
    if digits.is_empty() || digits.len() > 18 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err("that is not a command approval id".to_string());
    }
    Ok(())
}

fn valid_argv(argv: &[String]) -> std::result::Result<(), String> {
    if argv.is_empty() {
        return Err("the command has no program".to_string());
    }
    if argv.len() > MAX_ARGS {
        return Err(format!("the command has more than {MAX_ARGS} arguments"));
    }
    if argv[0].is_empty() {
        return Err("the command has an empty program name".to_string());
    }
    for a in argv {
        if a.chars().count() > MAX_ARG_CHARS {
            return Err(format!(
                "an argument is longer than {MAX_ARG_CHARS} characters"
            ));
        }
        if a.contains('\0') {
            return Err("an argument contains a NUL byte".to_string());
        }
    }
    Ok(())
}

fn valid_cwd(cwd: &str) -> std::result::Result<(), String> {
    if !cwd.starts_with('/') {
        return Err("the folder must be an absolute path".to_string());
    }
    if cwd.chars().count() > MAX_CWD_CHARS {
        return Err(format!(
            "the folder is longer than {MAX_CWD_CHARS} characters"
        ));
    }
    if cwd.contains('\0') {
        return Err("the folder contains a NUL byte".to_string());
    }
    Ok(())
}

/// The sidecar's `{error}` reason, or its status.
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

/// `GET /sessions/:id/shell/pending`: the commands waiting for the member in that session.
pub(crate) fn shell_pending(
    m: &HermesManager,
    session_id: &str,
) -> std::result::Result<serde_json::Value, String> {
    super::valid_session_id(session_id)?;
    let run = || -> HResult<super::ControlResp> {
        let bearer = m.bearer()?;
        m.control.get(
            &format!("{}/sessions/{session_id}/shell/pending", m.control_url()),
            &bearer,
        )
    };
    let resp = run().map_err(transport)?;
    if !(200..300).contains(&resp.status) {
        return Err(reason(resp.status, &resp.body));
    }
    let v: serde_json::Value = serde_json::from_str(&resp.body)
        .map_err(|_| "the sidecar's pending commands could not be read".to_string())?;
    match v.get("pending") {
        Some(list @ serde_json::Value::Array(_)) => Ok(list.clone()),
        _ => Err("the sidecar's pending commands could not be read".to_string()),
    }
}

/// `POST /sessions/:id/shell/decide`: the member's decision, bound to the argv and folder shown.
pub(crate) fn shell_decide(
    m: &HermesManager,
    session_id: &str,
    id: &str,
    allow: bool,
    argv: &[String],
    cwd: &str,
) -> std::result::Result<(), String> {
    super::valid_session_id(session_id)?;
    valid_approval_id(id)?;
    valid_argv(argv)?;
    valid_cwd(cwd)?;
    let body =
        serde_json::json!({ "id": id, "allow": allow, "argv": argv, "cwd": cwd }).to_string();
    let run = || -> HResult<super::ControlResp> {
        let bearer = m.bearer()?;
        m.control.post(
            &format!("{}/sessions/{session_id}/shell/decide", m.control_url()),
            &bearer,
            &body,
        )
    };
    let resp = run().map_err(transport)?;
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "SHELL_DECISION_REFUSED: {}",
            reason(resp.status, &resp.body)
        ));
    }
    Ok(())
}

/// **hermes_shell_pending**: the commands Hermes is waiting to run in a session (main window only).
#[tauri::command]
pub async fn hermes_shell_pending(
    app: tauri::AppHandle,
    session_id: String,
) -> std::result::Result<serde_json::Value, String> {
    crate::blocking::off_main(move || shell_pending(manager(&app)?, &session_id)).await
}

/// **hermes_shell_decide**: allow or decline one waiting command. The decision carries the exact
/// argv and folder the member was shown (main window only).
#[tauri::command]
pub async fn hermes_shell_decide(
    app: tauri::AppHandle,
    session_id: String,
    id: String,
    allow: bool,
    argv: Vec<String>,
    cwd: String,
) -> std::result::Result<(), String> {
    crate::blocking::off_main(move || {
        shell_decide(manager(&app)?, &session_id, &id, allow, &argv, &cwd)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_shell_tests.rs");
}
