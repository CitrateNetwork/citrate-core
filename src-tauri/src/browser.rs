//! HUP-S5.1 + S5.6 — citrate-core's side of Hermes's browser.
//!
//! The browser itself runs in the Hermes sidecar (citrate-agent-browser, enabled with
//! `CITRATE_HERMES_BROWSER=1`; off by default). These commands are the member's controls behind
//! the Browser pop-out, all for the main window only:
//!
//! - `hermes_browser_status` / `hermes_browser_frame`: what the pop-out shows (status, the latest
//!   screencast frame, the action waiting for a decision, an origin waiting for consent).
//! - `hermes_browser_stop` / `_resume`: the member's Stop latches until resumed.
//! - `hermes_browser_attach` / `_detach`: attach-to-Chrome needs the member's explicit consent for
//!   this session and an unprivileged loopback port; detach forgets every consent.
//! - `hermes_browser_origin`: consent to or revoke one origin. Banking, email and health origins
//!   are excluded by default (the sidecar's denylist); including one needs `include_sensitive`.
//! - `hermes_browser_decide`: allow or deny the browser action that is waiting.
//!
//! Every input is checked here before it reaches the sidecar, and the sidecar's refusal reason is
//! shown to the member as it is. Nothing here signs or holds a key.

use serde_json::json;

use crate::hermes::{ControlResp, HermesError, HermesManager};

/// Ports below this are never contacted for attach.
pub(crate) const MIN_ATTACH_PORT: u16 = 1024;
/// Longest origin (or URL) a consent call may carry.
pub(crate) const MAX_ORIGIN_LEN: usize = 2048;

/// The simple member controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserControl {
    Stop,
    Resume,
    Detach,
}

impl BrowserControl {
    fn path(self) -> &'static str {
        match self {
            BrowserControl::Stop => "/browser/stop",
            BrowserControl::Resume => "/browser/resume",
            BrowserControl::Detach => "/browser/detach",
        }
    }
}

fn not_running(e: &HermesError) -> bool {
    matches!(e, HermesError::NotRunning)
}

/// A control error the member can read: the sidecar's `{error}` reason when it gave one.
fn reason(resp: &ControlResp) -> String {
    serde_json::from_str::<serde_json::Value>(&resp.body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("the browser control returned {}", resp.status))
}

fn transport(e: HermesError) -> String {
    if not_running(&e) {
        "Hermes is not running; start it first".to_string()
    } else {
        e.to_string()
    }
}

fn post(
    m: &HermesManager,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = m
        .control_post_path(path, &body.to_string())
        .map_err(transport)?;
    if !(200..300).contains(&resp.status) {
        return Err(reason(&resp));
    }
    Ok(serde_json::from_str(&resp.body).unwrap_or(serde_json::Value::Null))
}

/// The sidecar's browser status, or `{enabled: false}` when the browser is off, the sidecar is an
/// older one without the routes, or Hermes is not running (`running: false`).
pub(crate) fn browser_status(m: &HermesManager) -> Result<serde_json::Value, String> {
    let resp = match m.control_get_path("/browser/status") {
        Ok(r) => r,
        Err(e) if not_running(&e) => return Ok(json!({ "enabled": false, "running": false })),
        Err(e) => return Err(e.to_string()),
    };
    match resp.status {
        200..=299 => serde_json::from_str(&resp.body).map_err(|e| e.to_string()),
        404 => Ok(json!({ "enabled": false, "running": true })),
        _ => Err(reason(&resp)),
    }
}

/// The latest screencast view when newer than `after`.
pub(crate) fn browser_frame(
    m: &HermesManager,
    after: u64,
) -> Result<Option<serde_json::Value>, String> {
    let resp = match m.control_get_path(&format!("/browser/frame?after={after}")) {
        Ok(r) => r,
        Err(e) if not_running(&e) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    match resp.status {
        204 | 404 => Ok(None),
        200..=299 => serde_json::from_str(&resp.body)
            .map(Some)
            .map_err(|e| e.to_string()),
        _ => Err(reason(&resp)),
    }
}

pub(crate) fn browser_simple(m: &HermesManager, c: BrowserControl) -> Result<(), String> {
    post(m, c.path(), json!({})).map(|_| ())
}

pub(crate) fn browser_attach(m: &HermesManager, port: u16, consent: bool) -> Result<(), String> {
    if !consent {
        return Err("attaching to your Chrome needs your consent for this session".to_string());
    }
    if port < MIN_ATTACH_PORT {
        return Err(format!(
            "the remote debugging port must be {MIN_ATTACH_PORT} or higher"
        ));
    }
    post(
        m,
        "/browser/attach",
        json!({ "port": port, "consent": true }),
    )
    .map(|_| ())
}

/// An http(s) URL or origin with a host, of sane length.
pub(crate) fn valid_origin(origin: &str) -> Result<(), String> {
    if origin.len() > MAX_ORIGIN_LEN {
        return Err("that address is too long".to_string());
    }
    let u = url::Url::parse(origin.trim()).map_err(|_| "that is not a web address".to_string())?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().unwrap_or_default().is_empty() {
        return Err("only http and https sites can be allowed".to_string());
    }
    Ok(())
}

/// Consent to (`allow`) or revoke one origin. Returns the origin as the sidecar normalised it.
pub(crate) fn browser_origin(
    m: &HermesManager,
    origin: &str,
    allow: bool,
    include_sensitive: bool,
) -> Result<String, String> {
    valid_origin(origin)?;
    let v = post(
        m,
        "/browser/origins",
        json!({ "origin": origin, "allow": allow, "includeSensitive": include_sensitive }),
    )?;
    Ok(v["origin"].as_str().unwrap_or(origin).to_string())
}

/// A waiting action's id as the sidecar mints it (`b` + digits).
pub(crate) fn valid_action_id(id: &str) -> Result<(), String> {
    let digits = id.strip_prefix('b').unwrap_or_default();
    if digits.is_empty() || digits.len() > 18 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err("that is not a browser action id".to_string());
    }
    Ok(())
}

pub(crate) fn browser_decide(m: &HermesManager, id: &str, allow: bool) -> Result<(), String> {
    valid_action_id(id)?;
    post(
        m,
        "/browser/actions/decide",
        json!({ "id": id, "allow": allow }),
    )
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// Commands (main window only; see permissions/main-window.toml)
// ---------------------------------------------------------------------------

/// **hermes_browser_status** — what the Browser pop-out and the main window's controls show.
#[tauri::command]
pub async fn hermes_browser_status(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    crate::blocking::off_main(move || browser_status(crate::hermes::manager_for(&app)?)).await
}

/// **hermes_browser_frame** — the latest screencast view when newer than `after`, else null.
#[tauri::command]
pub async fn hermes_browser_frame(
    app: tauri::AppHandle,
    after: u64,
) -> Result<Option<serde_json::Value>, String> {
    crate::blocking::off_main(move || browser_frame(crate::hermes::manager_for(&app)?, after)).await
}

/// **hermes_browser_stop** — the member's Stop (latches until resume).
#[tauri::command]
pub async fn hermes_browser_stop(app: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || {
        browser_simple(crate::hermes::manager_for(&app)?, BrowserControl::Stop)
    })
    .await
}

/// **hermes_browser_resume**
#[tauri::command]
pub async fn hermes_browser_resume(app: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || {
        browser_simple(crate::hermes::manager_for(&app)?, BrowserControl::Resume)
    })
    .await
}

/// **hermes_browser_attach** — attach to the member's Chrome on loopback `port`; `consent` is the
/// member's explicit consent for this session.
#[tauri::command]
pub async fn hermes_browser_attach(
    app: tauri::AppHandle,
    port: u16,
    consent: bool,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        browser_attach(crate::hermes::manager_for(&app)?, port, consent)
    })
    .await
}

/// **hermes_browser_detach** — close Hermes's tab in the member's Chrome and forget every consent.
#[tauri::command]
pub async fn hermes_browser_detach(app: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || {
        browser_simple(crate::hermes::manager_for(&app)?, BrowserControl::Detach)
    })
    .await
}

/// **hermes_browser_origin** — consent to (`allow`) or revoke one origin for this attach session.
#[tauri::command]
pub async fn hermes_browser_origin(
    app: tauri::AppHandle,
    origin: String,
    allow: bool,
    include_sensitive: bool,
) -> Result<String, String> {
    crate::blocking::off_main(move || {
        browser_origin(
            crate::hermes::manager_for(&app)?,
            &origin,
            allow,
            include_sensitive,
        )
    })
    .await
}

/// **hermes_browser_decide** — allow or deny the browser action waiting for the member.
#[tauri::command]
pub async fn hermes_browser_decide(
    app: tauri::AppHandle,
    id: String,
    allow: bool,
) -> Result<(), String> {
    crate::blocking::off_main(move || browser_decide(crate::hermes::manager_for(&app)?, &id, allow))
        .await
}

#[cfg(test)]
mod tests {
    include!("browser_tests.rs");
}
