//! D-11 (RC52, ADR P-5) — the member's preferred cloud model for Hermes: one provider and model,
//! or none (the local model).
//!
//! **Data source (Rule 7):** `<app data>/providers/preferred.json`, written only by
//! `providers_preferred_set` (owner-only, replaced atomically). Read by:
//! - `hermes_session_open_cloud`: the agent loop's cloud session takes its provider and model from
//!   here, never from the webview's arguments;
//! - `ai_chat` / `ai_chat_tools` with the provider id [`PREFERRED_ID`]: the app's own loop sends a
//!   message the member chose to send to the cloud (the K2 consent still applies per message).
//!
//! A missing file is "no preference". An unreadable or invalid file is also "no preference" and is
//! reported (`loadError`): a read error never sends anything to a cloud model. Setting a model
//! approves it for the anomaly halt (the member named it), and the provider must be one that can
//! take calls now (connected, with an adapter; a custom endpoint confirmed). Cognition is a
//! delegate, never a chat model.

use crate::model_provider::ProviderId;
use crate::model_proxy::{ModelProxy, ProxyRoute};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The settings file inside `<app data>/providers`.
pub const FILE: &str = "preferred.json";
/// The chat provider id that means "the preferred cloud model".
pub const PREFERRED_ID: &str = "preferred";

/// The member's choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreferredModel {
    pub provider_id: String,
    pub model: String,
}

/// What the app reads back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferredView {
    pub preferred: Option<PreferredModel>,
    pub load_error: Option<String>,
}

static WRITE: Mutex<()> = Mutex::new(());

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// A provider and model a chat call may be bound to.
pub fn validate(p: &PreferredModel) -> Result<ProviderId, String> {
    let pid = ProviderId::parse(p.provider_id.trim()).ok_or("unknown provider")?;
    if pid.is_delegate() {
        return Err("Cognition runs delegated tasks; choose a chat model for Hermes".into());
    }
    let m = p.model.trim();
    if m.is_empty() || m.len() > 200 || m.chars().any(char::is_control) || m != p.model {
        return Err("choose the model Hermes should use".into());
    }
    Ok(pid)
}

/// Read the stored choice. Missing = none; unreadable or invalid = none, with the reason.
pub fn load(dir: &Path) -> PreferredView {
    match std::fs::read_to_string(path(dir)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => PreferredView {
            preferred: None,
            load_error: None,
        },
        Err(_) => PreferredView {
            preferred: None,
            load_error: Some(
                "the preferred model setting could not be read; Hermes uses the local model".into(),
            ),
        },
        Ok(text) => match serde_json::from_str::<Option<PreferredModel>>(&text) {
            Ok(Some(p)) if validate(&p).is_ok() => PreferredView {
                preferred: Some(p),
                load_error: None,
            },
            Ok(None) => PreferredView {
                preferred: None,
                load_error: None,
            },
            _ => PreferredView {
                preferred: None,
                load_error: Some(
                    "the preferred model setting is not valid; Hermes uses the local model until you choose again"
                        .into(),
                ),
            },
        },
    }
}

/// The member's choice, checked against the proxy (the provider can take calls now) and saved;
/// its model is approved for the anomaly halt. `None` clears it.
pub fn set(
    dir: &Path,
    proxy: &ModelProxy,
    choice: Option<PreferredModel>,
) -> Result<PreferredView, String> {
    let _w = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = &choice {
        let pid = validate(p)?;
        if !proxy.resolves(&pid) {
            return Err(
                "that provider cannot take calls now (not available in this build, or a custom endpoint not confirmed yet)"
                    .into(),
            );
        }
        if !proxy.key_configured(&pid) {
            return Err(
                "that provider is not connected yet. Connect it in Settings › Models, then choose it here."
                    .into(),
            );
        }
        proxy
            .approve_model(&pid, &p.model)
            .map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec(&choice).map_err(|e| e.to_string())?;
    citrate_core_kit::fsutil::ensure_private_dir(dir).map_err(|e| e.kind().to_string())?;
    crate::atomic_file::replace(&path(dir), &bytes).map_err(|e| e.kind().to_string())?;
    Ok(load(dir))
}

/// The proxy route for the preferred model (`session_id` names the caller in the usage log).
pub fn route(dir: &Path, session_id: &str) -> Result<ProxyRoute, String> {
    let v = load(dir);
    let p = v.preferred.ok_or_else(|| {
        v.load_error.unwrap_or_else(|| {
            "no cloud model is chosen for Hermes; choose one in Settings › App › Hermes".into()
        })
    })?;
    let provider = validate(&p)?;
    Ok(ProxyRoute {
        provider,
        model: p.model,
        session_id: session_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    include!("provider_preferred_tests.rs");
}
