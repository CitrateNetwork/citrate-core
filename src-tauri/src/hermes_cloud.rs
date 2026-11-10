//! D-11 (RC52) — the Hermes agent loop on a member-selected cloud model (ADR P-5, P-6).
//!
//! The sidecar stays keyless. Core mints a proxy route bound to one provider, model and session
//! (`ModelProxy::mint_route`) and hands the sidecar an ordinary `LlmEndpoint
//! { baseUrl: "http://127.0.0.1:<port>/v1", bearer: <route token> }`. Every model request of the
//! loop then goes through the core model proxy: the pinned host, the member's caps and anomaly
//! halt, the signed usage log, and the key read from the keychain by core only. The session body
//! is the same one a local session gets (`build_session_body`): the same tools, the same HIC
//! awareness, so the sidecar's taint downgrade and approvals apply unchanged.
//!
//! - **Local model only** refuses the open, and every model request of a running cloud session
//!   (the route is gated on the chat-route setting, checked per call), so turning it on stops the
//!   next request of a turn already running.
//! - **Close**: the route is revoked when the session closes, when the sidecar stops, and when
//!   the provider's key is removed (`ModelProxy::remove_key`).
//! - **Tripwire**: a body, environment value or argument that crosses into the sidecar is refused
//!   when it carries anything shaped like a vendor key ([`ensure_keyless`]).
//!
//! Data sources (Rule 7): the provider and model are the member's choice in Hermes (the webview
//! passes ids; the provider must be connected in Settings › Models); the route is minted by the
//! in-process proxy; the session id is the sidecar's `POST /sessions` answer.

use std::collections::HashMap;
use std::sync::Mutex;

use super::HermesManager;
use crate::model_provider::ProviderId;
use crate::model_proxy::{ModelProxy, ProxyGrant, ProxyRoute};

/// Session labels of cloud sessions' routes (the usage log's `session` field).
pub const CLOUD_ROUTE_PREFIX: &str = "hs-";
/// The context window the sidecar plans a cloud session's prompts for. Cloud models' windows are
/// larger; this bounds what one request can carry (and so what it can cost). Pending owner sign-off.
pub const CLOUD_CONTEXT_TOKENS: u32 = 32_768;

/// The cloud sessions of one sidecar: session id → its route label (held by [`HermesManager`]).
#[derive(Default)]
pub struct CloudSessions(Mutex<HashMap<String, String>>);

impl CloudSessions {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn remember(&self, session_id: &str, route_label: &str) {
        self.map()
            .insert(session_id.to_string(), route_label.to_string());
    }

    /// The route label of a cloud session (and forget it).
    pub(crate) fn forget(&self, session_id: &str) -> Option<String> {
        self.map().remove(session_id)
    }

    pub(crate) fn is_cloud(&self, session_id: &str) -> bool {
        self.map().contains_key(session_id)
    }

    /// Forget every cloud session (the sidecar stopped: its sessions are gone).
    pub(crate) fn forget_all(&self) {
        self.map().clear();
    }
}

/// A fresh route label for one cloud session.
pub(crate) fn new_route_label() -> String {
    format!("{CLOUD_ROUTE_PREFIX}{:016x}", rand::random::<u64>())
}

/// The D-11 tripwire for everything core hands the sidecar: refused when it carries anything
/// shaped like a vendor key (`telemetry::has_vendor_key_shape`; the proxy's own route token is
/// not one). `what` names the crossing in the error.
pub fn ensure_keyless(what: &str, text: &str) -> Result<(), String> {
    if crate::telemetry::has_vendor_key_shape(text) {
        return Err(format!(
            "refused: the {what} for the Hermes agent carries something shaped like an API key, and keys never leave core"
        ));
    }
    Ok(())
}

/// Environment variables the model vendors' own SDKs read keys from. The sidecar never inherits
/// them from the app's launch environment.
pub const VENDOR_KEY_ENV: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "XAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "LLAMA_API_KEY",
    "META_API_KEY",
    "DEVIN_API_KEY",
    "COGNITION_API_KEY",
];

/// The inherited variables the sidecar must not see: a vendor SDK's key variable, or any value
/// shaped like a vendor key.
pub fn inherited_key_vars(inherited: impl Iterator<Item = (String, String)>) -> Vec<String> {
    inherited
        .filter(|(k, v)| {
            VENDOR_KEY_ENV.iter().any(|n| n.eq_ignore_ascii_case(k))
                || crate::telemetry::has_vendor_key_shape(v)
        })
        .map(|(k, _)| k)
        .collect()
}

/// The session body for a cloud model: the local body with the proxy's endpoint, plus
/// `llm.proxied: true` (a sidecar that knows it uses neither the endpoint's tokenizer nor its
/// embeddings, which a loopback endpoint would otherwise suggest; an older sidecar ignores it and
/// falls back to estimated tokens and lexical ranking when the proxy answers those paths 404).
pub fn build_cloud_session_body(
    system_prompt: &str,
    tools_json: &str,
    grant: &ProxyGrant,
    model: &str,
) -> Result<String, String> {
    let body = super::build_session_body(
        system_prompt,
        tools_json,
        &grant.base_url,
        grant.bearer.expose(),
        model,
        CLOUD_CONTEXT_TOKENS,
    )?;
    let mut v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if let Some(llm) = v.get_mut("llm").and_then(|l| l.as_object_mut()) {
        llm.insert("proxied".into(), serde_json::Value::Bool(true));
    }
    let out = v.to_string();
    ensure_keyless("session", &out)?;
    Ok(out)
}

/// Mint the route for a cloud session. The provider must be a chat provider (never the Cognition
/// delegate) and the model the member chose is approved for the anomaly halt (the member named it
/// here).
pub fn mint_cloud_route(
    proxy: &ModelProxy,
    provider_id: &str,
    model: &str,
    route_dir: std::path::PathBuf,
    label: &str,
) -> Result<ProxyGrant, String> {
    let provider = ProviderId::parse(provider_id).ok_or("unknown provider")?;
    if provider.is_delegate() {
        return Err("Cognition runs delegated tasks; choose a chat model for Hermes".into());
    }
    let model = model.trim();
    if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err("choose the model Hermes should use".into());
    }
    if !proxy.key_configured(&provider) {
        return Err(
            "that provider is not connected yet. Connect it in Settings › Models, then try again."
                .into(),
        );
    }
    proxy
        .approve_model(&provider, model)
        .map_err(|e| e.to_string())?;
    proxy
        .mint_route(
            ProxyRoute {
                provider,
                model: model.to_string(),
                session_id: label.to_string(),
            },
            route_dir,
        )
        .map_err(|e| e.to_string())
}

/// Open a cloud session: mint the route, build the body (`finish` adds the persona and grants and
/// opens it), remember the session, and revoke the route if the open fails.
#[allow(clippy::too_many_arguments)]
pub fn open_cloud_session(
    proxy: &ModelProxy,
    manager: &HermesManager,
    provider_id: &str,
    model: &str,
    route_dir: std::path::PathBuf,
    system_prompt: &str,
    tools_json: &str,
    finish: impl FnOnce(&str) -> Result<String, String>,
) -> Result<String, String> {
    crate::chat_route::remote_chat_allowed(&route_dir)?;
    let label = new_route_label();
    let grant = mint_cloud_route(proxy, provider_id, model, route_dir, &label)?;
    let opened = build_cloud_session_body(system_prompt, tools_json, &grant, model.trim())
        .and_then(|body| finish(&body));
    drop(grant);
    match opened {
        Ok(sid) => {
            manager.cloud.remember(&sid, &label);
            Ok(sid)
        }
        Err(e) => {
            proxy.revoke_session(&label);
            Err(e)
        }
    }
}

/// A turn may start in a cloud session only while remote sends are allowed.
pub fn check_send(
    manager: &HermesManager,
    session_id: &str,
    route_dir: &std::path::Path,
) -> Result<(), String> {
    if manager.cloud.is_cloud(session_id) {
        crate::chat_route::remote_chat_allowed(route_dir)?;
    }
    Ok(())
}

/// The session closed: revoke its route.
pub fn on_close(manager: &HermesManager, proxy: Option<&ModelProxy>, session_id: &str) {
    if let Some(label) = manager.cloud.forget(session_id) {
        if let Some(p) = proxy {
            p.revoke_session(&label);
        }
    }
}

/// The sidecar stopped (`HermesManager::stop` forgot its cloud sessions): every cloud route is
/// revoked.
pub fn on_stop(proxy: Option<&ModelProxy>) {
    if let Some(p) = proxy {
        p.revoke_sessions_with_prefix(CLOUD_ROUTE_PREFIX);
    }
}

#[cfg(test)]
mod tests {
    include!("hermes_cloud_tests.rs");
}
