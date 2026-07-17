//! citrate-core — real AI inference (CORE-AI1). @rule8 · provider API-key custody
//! + secret network egress (an exfiltration surface).
//!
//! Today the Dashboard chat runs a scripted demo provider and the Settings BYO-key
//! input is an honest stub (the key is masked + discarded). AI1 makes chat REAL
//! against any OpenAI-compatible endpoint (OpenAI, the Citrate gateway
//! `infer.citrate.ai/v1`, or any generic `/v1` endpoint) whose credential is
//! custodied in Rust: the key is sealed in the OS keyring, and the network call to
//! `/v1/chat/completions` originates HERE, never from the webview.
//!
//! ## Data source (Rule 1)
//! The completion is a REAL model response from the user's configured
//! OpenAI-compatible endpoint (`POST {baseURL}/chat/completions`, parse
//! `choices[0].message.content`). Nothing is fabricated; a failed call surfaces an
//! honest, secret-free error.
//!
//! ## Security invariants (@rule8 — the review attacks these)
//! 1. **No key egress to the webview (I-2).** NO `#[tauri::command]` returns the
//!    API key or the `Authorization` header. `ai_provider_status` returns only
//!    non-secret metadata (`id`, `baseURL`, `model`, `configured`).
//! 2. **Key at rest in the OS keyring** (service `ai.citrate.core`), never
//!    localStorage. The webview holds no key.
//! 3. **Exfiltration-proof binding (THE critical property).** The key is BOUND to
//!    its `baseURL` at set-time (they are sealed together in ONE JSON blob).
//!    `ai_chat(provider_id, …)` reads the STORED `{baseURL, model, apiKey}` for
//!    that id and calls the STORED `baseURL`. It NEVER accepts a webview-supplied
//!    URL. A compromised webview can pick WHICH configured provider to call, but
//!    can never redirect a sealed key to an attacker endpoint.
//! 4. **https-only baseURL**, validated at `ai_set_provider` time (reject
//!    http/other schemes).
//! 5. **Coarse, secret-free errors** — never echo the key, the Authorization
//!    header, or the full request/response body.

// AI1 seam: the keyring + HTTP seams are exercised by the injected mocks in the
// test build; the production `OsAiKeyring`/`UreqAiClient` paths only run in a
// Tauri build (the mock drives CI), so allow dead_code on the seam surface.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::Zeroize;

/// OS keyring service for AI provider configs (shared with the rest of the app's
/// keyring namespace, distinct accounts per provider id).
const KEYRING_SERVICE: &str = "ai.citrate.core";

/// Keyring account prefix for a provider's sealed config blob. The account is
/// `ai-<providerId>`; the value is the JSON of [`ProviderConfig`] (baseURL + model
/// + apiKey bound together — invariant 3).
const KEYRING_ACCOUNT_PREFIX: &str = "ai-";

/// Keyring account holding the default provider id (a plain, non-secret id string).
const KEYRING_DEFAULT_ACCOUNT: &str = "ai-default";

/// The known provider ids the frontend presets offer. `ai_chat`/`ai_set_provider`
/// do NOT hard-limit to these (a generic OpenAI-compatible id is allowed), but the
/// id is validated for shape so it cannot escape the keyring account namespace.
const _KNOWN_PRESETS: &[&str] = &["openai", "gateway", "custom"];

/// The real-provider system prompt (a tool-LESS variant of the demo's
/// `AGENT_SYSTEM_PROMPT`). This WP is plain chat + live-context injection: the
/// model gets no tool access, so the prompt must NOT claim it can read memory or
/// propose chain writes (Rule 1 — no capability it does not have). The live app
/// context is injected as a SEPARATE system line by `build_chat_body`.
const AGENT_SYSTEM_PROMPT_REAL: &str = "You are the Citrate member agent inside \
citrate-core, a desktop full-node app for the Citrate network (chain 40204, native \
token SALT). Answer the member plainly and concisely. You are given a snapshot of \
their live node, wallet, and membership state as context; ground any numbers you \
cite in that context and never fabricate figures. You have no tools in this \
session: you cannot read their memory graph, execute writes, or move funds — do \
not claim to. If asked to perform an action, explain that it happens through the \
app's own controls (which route every write through a human-approved signature \
ceremony).";

// ---------------------------------------------------------------------------
// Errors — coarse + secret-free (invariant 5). No variant carries the key, the
// Authorization header, or a full request/response body.
// ---------------------------------------------------------------------------

/// An AI inference / provider-config error. Every `Display` string is safe to
/// surface or log: it never contains the API key, the Authorization header, or a
/// raw request/response body (@rule8, invariant 5).
#[derive(Debug, PartialEq, Eq)]
pub enum AiError {
    /// The `base_url` was not an https URL (rejected at set-time — invariant 4).
    BadBaseUrl,
    /// The api key was empty / obviously malformed (rejected at set-time).
    BadKey,
    /// The provider id was empty or not a safe `[a-z0-9._-]` slug (so it can never
    /// escape the `ai-<id>` keyring account namespace).
    BadProviderId,
    /// No provider is configured for the given id (nothing sealed in the keyring).
    NotConfigured,
    /// The OS keyring backend is unreachable or the entry could not be written.
    KeyringUnavailable,
    /// A network/transport error talking to the provider endpoint.
    Network,
    /// The provider returned a non-2xx / error response (no body echoed).
    Provider,
    /// The response JSON did not contain `choices[0].message.content`.
    BadResponse,
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AiError::BadBaseUrl => "ai: base URL must be https",
            AiError::BadKey => "ai: api key missing or malformed",
            AiError::BadProviderId => "ai: invalid provider id",
            AiError::NotConfigured => "ai: no provider configured for that id",
            AiError::KeyringUnavailable => "ai: keyring unavailable",
            AiError::Network => "ai: could not reach the provider",
            AiError::Provider => "ai: provider returned an error",
            AiError::BadResponse => "ai: provider response could not be parsed",
        };
        f.write_str(s)
    }
}

impl std::error::Error for AiError {}

type Result<T> = std::result::Result<T, AiError>;

// ---------------------------------------------------------------------------
// The sealed provider config — baseURL + model + apiKey BOUND together
// (invariant 3). Only the plaintext of a single keyring entry; never serialized
// back out across the invoke boundary (invariant 1).
// ---------------------------------------------------------------------------

/// A provider's sealed config: the endpoint, the model, and the API key, all in
/// one keyring entry so the key can NEVER be paired with a different (attacker)
/// URL. `ai_chat` reads the WHOLE blob and uses THIS `base_url` — the webview
/// never supplies a URL (invariant 3).
#[derive(Clone, Serialize, Deserialize)]
struct ProviderConfig {
    #[serde(rename = "baseURL")]
    base_url: String,
    model: String,
    #[serde(rename = "apiKey")]
    api_key: String,
}

// AI-06: redact the api key from any `{:?}`/dbg!/tracing output — no key ever
// reaches a log (invariant 1/5).
impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl Drop for ProviderConfig {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

/// Non-secret provider status crossing the invoke boundary (invariant 1). Carries
/// the id, baseURL, model and a `configured` flag — NEVER the key or the
/// Authorization header.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderStatus {
    pub id: String,
    #[serde(rename = "baseURL")]
    pub base_url: String,
    pub model: String,
    pub configured: bool,
    /// Whether this id is the current default route.
    #[serde(rename = "isDefault")]
    pub is_default: bool,
}

// ---------------------------------------------------------------------------
// Keyring seam — the sealed config lives in the OS keyring in production, an
// in-memory map in tests. Mirrors custody.rs's `Keyring` trait.
// ---------------------------------------------------------------------------

/// The OS-keyring seam for AI provider configs. Production uses [`OsAiKeyring`];
/// tests inject an in-memory fake so no real OS keyring is touched in CI.
pub trait AiKeyring: Send + Sync {
    /// Read the value for `account`, or `None` if absent. `Err` means the backend
    /// itself is unreachable (fail closed).
    fn get(&self, account: &str) -> Result<Option<String>>;
    /// Store `value` for `account`.
    fn set(&self, account: &str, value: &str) -> Result<()>;
    /// Delete `account` (idempotent).
    fn delete(&self, account: &str) -> Result<()>;
}

/// The real platform keyring, via the `keyring` v3 crate (same idioms as
/// custody.rs). Values are UTF-8 JSON strings.
pub struct OsAiKeyring;

impl AiKeyring for OsAiKeyring {
    fn get(&self, account: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| AiError::KeyringUnavailable)?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(AiError::KeyringUnavailable),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| AiError::KeyringUnavailable)?;
        entry
            .set_password(value)
            .map_err(|_| AiError::KeyringUnavailable)
    }

    fn delete(&self, account: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| AiError::KeyringUnavailable)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(AiError::KeyringUnavailable),
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP seam — real `ureq` (rustls) in production, injectable for tests. Mirrors
// oidc.rs's `HttpClient`. The seam takes the ALREADY-BUILT url + bearer + body so
// the test transport can assert the exact request shape WITHOUT a live socket.
// ---------------------------------------------------------------------------

/// The single HTTP op the chat call needs: POST a JSON body with a Bearer header,
/// return the response body string. Abstracted so tests inject a mock and assert
/// the request shape (endpoint, Bearer present, messages) with no real network.
pub trait AiHttpClient: Send + Sync {
    /// `POST url` with `Content-Type: application/json` and
    /// `Authorization: Bearer {bearer}`, body = `body` (JSON). Returns the
    /// response body string, or a coarse [`AiError`] (never echoing the bearer).
    fn post_json(&self, url: &str, bearer: &str, body: &Value) -> Result<String>;
}

/// Production HTTP client: blocking `ureq` (rustls TLS), same transport as the A3
/// OIDC client.
pub struct UreqAiClient;

impl AiHttpClient for UreqAiClient {
    fn post_json(&self, url: &str, bearer: &str, body: &Value) -> Result<String> {
        let mut resp = ureq::post(url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| classify_ureq_err(&e))?;
        resp.body_mut()
            .read_to_string()
            .map_err(|_| AiError::Network)
    }
}

/// Map a `ureq` error to a coarse [`AiError`] WITHOUT surfacing the URL, body, or
/// any header (invariant 5). A non-2xx status is a provider error; anything else
/// is a transport error.
fn classify_ureq_err(e: &ureq::Error) -> AiError {
    match e {
        ureq::Error::StatusCode(_) => AiError::Provider,
        _ => AiError::Network,
    }
}

// ---------------------------------------------------------------------------
// Provider-id + baseURL validation
// ---------------------------------------------------------------------------

/// Validate a provider id: non-empty and only `[a-z0-9._-]`, so the derived
/// keyring account `ai-<id>` can never contain a separator that escapes the
/// namespace or collides with `ai-default`.
fn validate_provider_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 {
        return Err(AiError::BadProviderId);
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
    {
        return Err(AiError::BadProviderId);
    }
    // Never allow the id to spell the reserved default account suffix.
    if id == "default" {
        return Err(AiError::BadProviderId);
    }
    Ok(())
}

/// The keyring account for a provider id (`ai-<id>`). The id is validated first.
fn account_for(id: &str) -> String {
    format!("{KEYRING_ACCOUNT_PREFIX}{id}")
}

/// Validate a base URL: it MUST be https (invariant 4). Rejects http, ws, file,
/// and any non-https scheme. Trailing `/` is trimmed so `{baseURL}/chat/
/// completions` never doubles a slash.
fn validate_https_base_url(base_url: &str) -> Result<String> {
    let parsed = url::Url::parse(base_url).map_err(|_| AiError::BadBaseUrl)?;
    if parsed.scheme() != "https" {
        return Err(AiError::BadBaseUrl);
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(AiError::BadBaseUrl);
    }
    Ok(base_url.trim_end_matches('/').to_string())
}

// ---------------------------------------------------------------------------
// The AI provider manager — owns the keyring + HTTP seams. Everything secret is
// confined here; the invoke commands only ever read non-secret status or the
// completion string off it.
// ---------------------------------------------------------------------------

/// The process-wide AI provider manager. Holds the keyring seam (sealed configs)
/// and the HTTP seam (the chat POST). No secret ever leaves this struct across the
/// invoke boundary — `status` returns non-secret metadata, `chat` returns only the
/// model completion.
pub struct AiManager {
    keyring: Box<dyn AiKeyring>,
    http: Box<dyn AiHttpClient>,
}

impl AiManager {
    pub fn new(keyring: Box<dyn AiKeyring>, http: Box<dyn AiHttpClient>) -> Self {
        AiManager { keyring, http }
    }

    /// Seal a provider config: validate the https base URL + non-empty key, then
    /// store `{baseURL, model, apiKey}` as ONE keyring blob (binding — invariant
    /// 3). Also seeds the default provider id if none is set yet. Returns nothing
    /// (invariant 1 — never returns the key).
    fn set_provider(
        &self,
        provider_id: &str,
        base_url: &str,
        model: &str,
        api_key: &str,
    ) -> Result<()> {
        validate_provider_id(provider_id)?;
        let base_url = validate_https_base_url(base_url)?;
        // A key is required and must not be trivially empty/whitespace.
        if api_key.trim().is_empty() {
            return Err(AiError::BadKey);
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(AiError::BadKey);
        }
        let cfg = ProviderConfig {
            base_url,
            model: model.to_string(),
            api_key: api_key.to_string(),
        };
        let mut blob = serde_json::to_string(&cfg).map_err(|_| AiError::KeyringUnavailable)?;
        let result = self.keyring.set(&account_for(provider_id), &blob);
        blob.zeroize(); // scrub the transient serialized copy (it holds the key)
        result?;
        // First-configured provider becomes the default route (non-secret id).
        if self.keyring.get(KEYRING_DEFAULT_ACCOUNT)?.is_none() {
            self.keyring.set(KEYRING_DEFAULT_ACCOUNT, provider_id)?;
        }
        Ok(())
    }

    /// Read (and DECODE) the sealed config for `provider_id`. In-process only — the
    /// returned struct holds the key and is never serialized across the invoke
    /// boundary. `None`/absent → `NotConfigured`.
    fn read_config(&self, provider_id: &str) -> Result<ProviderConfig> {
        validate_provider_id(provider_id)?;
        let blob = self
            .keyring
            .get(&account_for(provider_id))?
            .ok_or(AiError::NotConfigured)?;
        let cfg: ProviderConfig =
            serde_json::from_str(&blob).map_err(|_| AiError::KeyringUnavailable)?;
        Ok(cfg)
    }

    /// The current default provider id, if one is set.
    fn default_provider(&self) -> Result<Option<String>> {
        self.keyring.get(KEYRING_DEFAULT_ACCOUNT)
    }

    /// Non-secret status for the given provider ids: `{id, baseURL, model,
    /// configured, isDefault}` — NEVER the key (invariant 1). An unconfigured id
    /// is reported `configured:false` with empty metadata rather than erroring, so
    /// the frontend can render the preset row honestly.
    fn provider_status(&self, ids: &[&str]) -> Result<Vec<ProviderStatus>> {
        let default = self.default_provider()?;
        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            validate_provider_id(id)?;
            match self.keyring.get(&account_for(id))? {
                Some(blob) => {
                    // Decode to surface baseURL + model (NOT the key).
                    let cfg: ProviderConfig =
                        serde_json::from_str(&blob).map_err(|_| AiError::KeyringUnavailable)?;
                    out.push(ProviderStatus {
                        id: id.to_string(),
                        base_url: cfg.base_url.clone(),
                        model: cfg.model.clone(),
                        configured: true,
                        is_default: default.as_deref() == Some(id),
                    });
                }
                None => out.push(ProviderStatus {
                    id: id.to_string(),
                    base_url: String::new(),
                    model: String::new(),
                    configured: false,
                    is_default: default.as_deref() == Some(id),
                }),
            }
        }
        Ok(out)
    }

    /// Delete a provider's sealed config. If it was the default, the default is
    /// cleared too (so `ai_chat` on a stale default fails closed rather than
    /// pointing at a deleted config).
    fn clear_provider(&self, provider_id: &str) -> Result<()> {
        validate_provider_id(provider_id)?;
        self.keyring.delete(&account_for(provider_id))?;
        if self.default_provider()?.as_deref() == Some(provider_id) {
            self.keyring.delete(KEYRING_DEFAULT_ACCOUNT)?;
        }
        Ok(())
    }

    /// **The real inference call (@rule8).** Read the SEALED config for
    /// `provider_id`, build the OpenAI `/v1/chat/completions` body, POST it to the
    /// STORED `baseURL` (invariant 3 — never a caller-supplied URL) with
    /// `Authorization: Bearer {apiKey}`, and return `choices[0].message.content`.
    /// Non-streaming. Errors are coarse + secret-free (invariant 5).
    fn chat(&self, provider_id: &str, messages_json: &str, context_json: &str) -> Result<String> {
        let cfg = self.read_config(provider_id)?;
        // INVARIANT 3: the endpoint comes from the STORED config, never the caller.
        let url = format!("{}/chat/completions", cfg.base_url);
        let body = build_chat_body(&cfg.model, messages_json, context_json)?;
        let resp = self.http.post_json(&url, &cfg.api_key, &body)?;
        parse_completion(&resp)
    }
}

/// Build the OpenAI `/v1/chat/completions` request body: the tool-less real system
/// prompt, then the injected live app context as a SEPARATE system line (so the
/// model grounds its numbers — Rule 1), then the caller's messages. `messages_json`
/// is a JSON array of `{role, content}`; `context_json` is an opaque JSON snapshot
/// of the app's live state (stringified into the context system line). A malformed
/// `messages_json` fails closed rather than sending a garbage body.
fn build_chat_body(model: &str, messages_json: &str, context_json: &str) -> Result<Value> {
    // The caller's chat history (validated to be an array of message objects).
    let history: Value = serde_json::from_str(messages_json).map_err(|_| AiError::BadResponse)?;
    let history = history.as_array().ok_or(AiError::BadResponse)?;

    let mut messages: Vec<Value> = Vec::with_capacity(history.len() + 2);
    messages.push(json!({ "role": "system", "content": AGENT_SYSTEM_PROMPT_REAL }));
    // The live context as a system line. Passed through verbatim as a string so the
    // model sees the real snapshot; we do not fabricate any field.
    messages.push(json!({
        "role": "system",
        "content": format!("Live app context (JSON snapshot of the member's node/wallet/membership): {context_json}"),
    }));
    for m in history {
        // Only forward well-formed {role, content} entries (defensive; never send a
        // malformed message shape to the provider).
        let role = m.get("role").and_then(Value::as_str);
        let content = m.get("content").and_then(Value::as_str);
        if let (Some(role), Some(content)) = (role, content) {
            messages.push(json!({ "role": role, "content": content }));
        }
    }
    Ok(json!({
        "model": model,
        "messages": messages,
        "stream": false,
    }))
}

/// Parse the OpenAI chat-completions response: `choices[0].message.content` → the
/// completion string. Any missing field is a coarse `BadResponse` (no body echoed).
fn parse_completion(resp: &str) -> Result<String> {
    let v: Value = serde_json::from_str(resp).map_err(|_| AiError::BadResponse)?;
    v.get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c0| c0.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or(AiError::BadResponse)
}

// ---------------------------------------------------------------------------
// Tauri command surface — @rule8. NONE of these returns the key or the
// Authorization header (invariant 1). `ai_chat` uses the STORED baseURL only.
// ---------------------------------------------------------------------------

/// Managed Tauri state: the process-wide AI provider manager.
pub struct AiState(pub AiManager);

/// The provider ids `ai_provider_status` reports on (the frontend presets). A
/// generic OpenAI-compatible provider is stored under the `custom` id.
const STATUS_PROVIDER_IDS: &[&str] = &["openai", "gateway", "custom"];

/// **Command — ai_set_provider (@rule8).** Seal `{baseURL, model, apiKey}` for
/// `provider_id` in the OS keyring, binding the key to its https baseURL. Rejects a
/// non-https baseURL or an empty key. Returns nothing — NEVER the key (invariant
/// 1).
#[tauri::command]
pub fn ai_set_provider(
    provider_id: String,
    base_url: String,
    model: String,
    api_key: String,
    ai: tauri::State<'_, AiState>,
) -> std::result::Result<(), String> {
    ai.0.set_provider(&provider_id, &base_url, &model, &api_key)
        .map_err(|e| e.to_string())
}

/// **Command — ai_provider_status (@rule8).** Non-secret status for the preset
/// provider ids: `{id, baseURL, model, configured, isDefault}` — NEVER the key or
/// the Authorization header (invariant 1).
#[tauri::command]
pub fn ai_provider_status(
    ai: tauri::State<'_, AiState>,
) -> std::result::Result<Vec<ProviderStatus>, String> {
    ai.0.provider_status(STATUS_PROVIDER_IDS)
        .map_err(|e| e.to_string())
}

/// **Command — ai_clear_provider (@rule8).** Delete a provider's sealed config
/// (and clear the default if it pointed here).
#[tauri::command]
pub fn ai_clear_provider(
    provider_id: String,
    ai: tauri::State<'_, AiState>,
) -> std::result::Result<(), String> {
    ai.0.clear_provider(&provider_id).map_err(|e| e.to_string())
}

/// **Command — ai_chat (@rule8).** REAL inference: read the SEALED config for
/// `provider_id`, POST the OpenAI chat body to the STORED baseURL with the sealed
/// key, and return the model completion. The webview picks WHICH provider id; it
/// can NEVER supply the URL (invariant 3). Errors are coarse + secret-free.
#[tauri::command]
pub fn ai_chat(
    provider_id: String,
    messages_json: String,
    context_json: String,
    ai: tauri::State<'_, AiState>,
) -> std::result::Result<String, String> {
    ai.0.chat(&provider_id, &messages_json, &context_json)
        .map_err(|e| e.to_string())
}

/// Build the managed AI state with the real OS keyring + ureq client (production).
pub fn build_ai_state() -> AiState {
    AiState(AiManager::new(
        Box::new(OsAiKeyring),
        Box::new(UreqAiClient),
    ))
}

#[cfg(test)]
mod tests {
    include!("ai_tests.rs");
}
