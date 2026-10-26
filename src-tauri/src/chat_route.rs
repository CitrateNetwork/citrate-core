//! K2 (0.5.2): the member's chat route, "Local model only" in Hermes › Controls.
//!
//! Data sources (Rule 7):
//! - **The switch:** `<app data>/escalation/route.json`, next to the escalation endpoints and
//!   spend ledger (`escalation.rs`), written only by [`chat_route_set`].
//! - **Where it applies:** [`select_route`] turns the honest inference state
//!   (`ai::select_inference_state`) into what `model_inference_state` reports, and no remote-send
//!   permit (`chat_consent::authorize` / `authorize_once`, which every remote chat and image send
//!   needs) is handed out while it is `local_only`.
//!
//! Two values (owner decision 2026-10-19):
//! - **`ask_first` (default):** today's behaviour (ask before a message goes to the gateway when the
//!   local model is stopped), and the silent case is closed: with a gateway key and no local model
//!   running, chat asks before each message instead of routing there.
//! - **`local_only`:** chat never goes to the gateway. A stopped local model offers only a restart;
//!   with no local model the message is not sent.
//!
//! A missing file is the default. An unreadable or corrupt file counts as `local_only`: a read
//! error never widens what may leave the device. Nothing here signs or holds a key (Rule 3).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ai::{select_inference_state, InferenceState, ProviderInputs};

/// The settings file inside the escalation data folder.
pub const ROUTE_FILE: &str = "route.json";

/// Where chat may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChatRoute {
    /// Never to the gateway.
    LocalOnly,
    /// Ask before each message goes to the gateway (the default).
    #[default]
    AskFirst,
}

/// The stored choice. A file that exists must name its `route`: only an ABSENT file gets the
/// default ([`load`]), so a damaged file (`{}`, a timestamp alone) never reads as `ask_first`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RouteSettings {
    pub route: ChatRoute,
    #[serde(default)]
    pub changed_at_ms: Option<u64>,
}

/// What the app reads back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteStatus {
    pub route: ChatRoute,
    pub load_error: Option<String>,
}

/// Read the settings. Missing = the default (ask first). Unreadable or corrupt is an error; every
/// path that acts on it then treats it as local only.
pub fn load(dir: &Path) -> Result<RouteSettings, String> {
    match std::fs::read_to_string(dir.join(ROUTE_FILE)) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| {
            "the Local model only setting is not valid; chat stays on this device until you set it again"
                .to_string()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RouteSettings::default()),
        Err(_) => Err("the Local model only setting could not be read; chat stays on this device".into()),
    }
}

/// The route that applies (a read error counts as local only).
pub fn effective(dir: &Path) -> ChatRoute {
    load(dir).map(|s| s.route).unwrap_or(ChatRoute::LocalOnly)
}

/// The status the app shows.
pub fn status(dir: &Path) -> RouteStatus {
    match load(dir) {
        Ok(s) => RouteStatus {
            route: s.route,
            load_error: None,
        },
        Err(e) => RouteStatus {
            route: ChatRoute::LocalOnly,
            load_error: Some(e),
        },
    }
}

/// Store the member's choice (written to a temp file, then renamed).
pub fn set(dir: &Path, route: ChatRoute, now_ms: u64) -> Result<RouteStatus, String> {
    std::fs::create_dir_all(dir).map_err(|_| "cannot create the settings folder".to_string())?;
    let s = RouteSettings {
        route,
        changed_at_ms: Some(now_ms),
    };
    let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{ROUTE_FILE}.tmp"));
    std::fs::write(&tmp, text)
        .map_err(|_| "cannot write the Local model only setting".to_string())?;
    std::fs::rename(&tmp, dir.join(ROUTE_FILE))
        .map_err(|_| "cannot write the Local model only setting".to_string())?;
    Ok(RouteStatus {
        route,
        load_error: None,
    })
}

/// The inference state the app routes by, for this route.
///
/// - `local_only`: `Ready` stays local; a local model that is set up but stopped is
///   `LocalOnlyStopped` (offer only a restart); anything else (no model, a download in flight,
///   with or without a remote key) is `LocalOnly`: the message is not sent and the app says how
///   to start the local model. Never the demo agent, never a remote route.
/// - `ask_first`: the plain state, except that a remote route with no local model running
///   (`GatewayOnly`, or `Downloading` with a key) becomes `AskGateway`: asked, never routed.
pub fn select_route(i: ProviderInputs, route: ChatRoute) -> InferenceState {
    let plain = select_inference_state(i);
    match route {
        ChatRoute::LocalOnly => {
            if plain == InferenceState::Ready {
                InferenceState::Ready
            } else if i.model_ready {
                InferenceState::LocalOnlyStopped
            } else {
                InferenceState::LocalOnly
            }
        }
        ChatRoute::AskFirst => match plain {
            InferenceState::GatewayOnly => InferenceState::AskGateway,
            InferenceState::Downloading if i.gateway_key_configured => InferenceState::AskGateway,
            other => other,
        },
    }
}

/// Whether a message may be sent to a remote provider now. Under `local_only` (or an unreadable
/// setting) no remote-send permit is handed out, so nothing is sent.
pub fn remote_chat_allowed(dir: &Path) -> Result<(), String> {
    match effective(dir) {
        ChatRoute::AskFirst => Ok(()),
        ChatRoute::LocalOnly => Err(
            "Local only is on, so nothing was sent off this device. Turn off Local model only in Hermes › Controls to use the gateway."
                .into(),
        ),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The settings folder (the escalation data folder).
pub fn route_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(crate::escalation::DIR_NAME))
}

/// **chat_route_get**: the stored route (main window only).
#[tauri::command]
pub async fn chat_route_get(app: tauri::AppHandle) -> Result<RouteStatus, String> {
    crate::blocking::off_main(move || Ok(status(&route_dir(&app)?))).await
}

/// **chat_route_set**: store the member's route (main window only). Takes only the route value.
#[tauri::command]
pub async fn chat_route_set(
    app: tauri::AppHandle,
    route: ChatRoute,
) -> Result<RouteStatus, String> {
    crate::blocking::off_main(move || set(&route_dir(&app)?, route, now_ms())).await
}

#[cfg(test)]
mod tests {
    include!("chat_route_tests.rs");
}
