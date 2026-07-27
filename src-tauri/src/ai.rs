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

/// W3.3 — the system prompt for the AGENTIC (tool-calling) chat path. Unlike
/// `AGENT_SYSTEM_PROMPT_REAL`, this session HAS tools: the model may read the
/// member's memory graph (incl. preloaded Citrate docs), read live chain state,
/// navigate the app, and PROPOSE (never execute) memory writes. General-purpose:
/// it answers both Citrate-specific and general questions.
const AGENT_SYSTEM_PROMPT_TOOLS: &str = "You are the Citrate member agent inside \
citrate-core, a desktop full-node app for the Citrate network (chain 40204, native \
token SALT). You are a knowledgeable, general-purpose assistant: answer both \
Citrate-specific questions AND general questions on any topic, plainly and \
concisely. Use your tools instead of guessing: memory_search / memory_recall read \
the member's memory graph, which includes preloaded Citrate documentation (the \
'citrate-docs' tenant) — prefer them for any Citrate protocol, how-to, or docs \
question, and cite what you find; if a search returns nothing, say so and do not \
fabricate Citrate facts. The member's live node/wallet/earnings snapshot is given \
as context — ground their numbers in it and never invent figures. app_navigate \
moves the member to a surface when it helps. memory_assert PROPOSES remembering a \
fact: it is a write, so it is never executed by you — it queues for the member's \
approval in the signature ceremony; tell them you proposed it, do not claim it is \
saved. For general-knowledge questions you may answer directly.";

/// Build the LOCAL llama-server baseURL from a loopback port
/// (`http://127.0.0.1:<port>/v1`). This is the SINGLE source of truth for the
/// local endpoint (the same shape `serve.rs::base_url` produces); `chat_local`
/// derives its URL from here so the webview never supplies a URL (F-1).
pub fn local_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

/// F-1 (BLOCKING) — the rigorous loopback guard. A URL is a safe LOCAL endpoint
/// IFF it parses, its scheme is exactly `http`, it carries NO userinfo
/// (`username().is_empty() && password().is_none()`), and its host is EXACTLY one
/// of the loopback aliases (`127.0.0.1`, `::1`, `localhost`). This closes the
/// exfil class the old `starts_with("http://127.0.0.1:")` string check allowed:
/// `http://127.0.0.1:8080@evil.com/v1` (userinfo → host `evil.com`),
/// `http://127.0.0.1:@evil.com/v1`, and `http://127.0.0.1.evil.com/v1` (host is a
/// subdomain of `evil.com`) all FAIL host_str() equality / userinfo emptiness,
/// mirroring the rigor of [`validate_https_base_url`].
fn loopback_url_is_safe(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "http" {
        return false;
    }
    // No userinfo — the userinfo trick (`user:pass@host`) is how a "127.0.0.1"
    // prefix can hide a foreign host after the `@`.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    // Host must be EXACTLY a loopback alias (not merely a prefix/suffix of one).
    // The `url` crate renders an IPv6 host in bracketed form (`[::1]`), so accept
    // that spelling too.
    matches!(
        parsed.host_str(),
        Some("127.0.0.1" | "::1" | "[::1]" | "localhost")
    )
}

// ---------------------------------------------------------------------------
// BC-3.2 — the honest inference-routing state. A PURE function over the real
// facts (local model ready + server healthy, whether a download is in flight,
// whether a gateway key is configured) → the single honest state the frontend
// renders. LOCAL wins over gateway wins over demo (Rule 1: every fallback is
// truthful, never a demo dressed as a real model).
// ---------------------------------------------------------------------------

/// The inputs the routing decision is made from (all honest, real facts).
#[derive(Debug, Clone, Copy)]
pub struct ProviderInputs {
    /// The local model is downloaded AND verified-`Ready` (BC-3.1).
    pub model_ready: bool,
    /// The local `llama-server` sidecar is Running/healthy (BC-3.2).
    pub server_healthy: bool,
    /// A model download is currently in progress.
    pub downloading: bool,
    /// A gateway provider key (cgk_) is configured (the remote fallback).
    pub gateway_key_configured: bool,
}

/// The honest inference-routing state surfaced to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceState {
    /// Local model ready + server healthy → chat runs LOCALLY.
    Ready,
    /// Local model ready but the server is not healthy → fall back to the gateway.
    LocalFallback,
    /// A download is in flight (no ready local server) → gateway meanwhile.
    Downloading,
    /// No local model, but a gateway key is configured → gateway-only.
    GatewayOnly,
    /// No local model, no gateway, and no download started → the onboarding step
    /// surfaces this to prompt a download.
    NoModel,
    /// Nothing configured that can run a real model → the built-in demo agent.
    Demo,
}

impl InferenceState {
    /// The kebab-case wire string the frontend reads.
    pub fn as_str(&self) -> &'static str {
        match self {
            InferenceState::Ready => "ready",
            InferenceState::LocalFallback => "local-fallback",
            InferenceState::Downloading => "downloading",
            InferenceState::GatewayOnly => "gateway-only",
            InferenceState::NoModel => "no-model",
            InferenceState::Demo => "demo",
        }
    }
}

/// Decide the honest inference state. Priority: LOCAL (ready + healthy) → gateway
/// (local-fallback if the model is ready but the server is down, else
/// gateway-only) → downloading (in flight) → demo (nothing else). This is the
/// ONE place the real-vs-fallback route is decided (Rule 1 — no fabricated route).
pub fn select_inference_state(i: ProviderInputs) -> InferenceState {
    // 1) The best path: a ready local model with a healthy server → run locally.
    if i.model_ready && i.server_healthy {
        return InferenceState::Ready;
    }
    // 2) A ready model but no healthy server: honest local-fallback iff a gateway
    //    key exists; otherwise it is still "ready-but-not-serving" — route to demo
    //    only if there is no gateway (we never claim a local model that isn't
    //    actually serving).
    if i.model_ready && i.gateway_key_configured {
        return InferenceState::LocalFallback;
    }
    // 3) No usable local model. If a download is in flight, say so (the UI shows
    //    progress + routes to the gateway/demo meanwhile).
    if i.downloading {
        return InferenceState::Downloading;
    }
    // 4) No local model + a gateway key → gateway-only.
    if i.gateway_key_configured {
        return InferenceState::GatewayOnly;
    }
    // 5) Nothing configured → the honest built-in demo.
    InferenceState::Demo
}

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

    /// W3.3 — one turn of the agentic loop: POST the tool-enabled body to the
    /// stored endpoint and return the assistant MESSAGE (content and/or tool_calls)
    /// as JSON. The frontend runs the loop (executes tool calls, appends results,
    /// calls again), so a single Rust op stays stateless + key-sealed. INVARIANT 3
    /// holds: the URL is derived from the stored config, never the caller.
    fn chat_tools(
        &self,
        provider_id: &str,
        messages_json: &str,
        tools_json: &str,
        context_json: &str,
    ) -> Result<String> {
        let cfg = self.read_config(provider_id)?;
        let url = format!("{}/chat/completions", cfg.base_url);
        let body = build_chat_body_with_tools(&cfg.model, messages_json, tools_json, context_json)?;
        let resp = self.http.post_json(&url, &cfg.api_key, &body)?;
        parse_chat_message(&resp)
    }

    /// **BC-3.2 — LOCAL inference (F-1 hardened).** POST the OpenAI chat body to
    /// the LOCAL `llama-server` on the loopback endpoint with NO api key (an empty
    /// bearer — the local model needs none). The endpoint is DERIVED IN RUST from
    /// the loopback `port` ([`local_base_url`], the same source `serve.rs` uses);
    /// the webview supplies only `port` (a `u16`) — NEVER a URL. This eliminates
    /// the attacker-controllable-URL class entirely (a `u16` cannot name a foreign
    /// host), mirroring how the money-path commands never take a webview URL. A
    /// defence-in-depth [`loopback_url_is_safe`] check re-validates the derived URL
    /// so a future refactor cannot reintroduce a foreign host. No secret is
    /// involved.
    fn chat_local(
        &self,
        port: u16,
        model: &str,
        messages_json: &str,
        context_json: &str,
    ) -> Result<String> {
        let base = local_base_url(port);
        // Defence in depth: the Rust-derived URL must satisfy the rigorous loopback
        // guard (exact loopback host, http scheme, no userinfo). A port can only
        // ever produce a safe URL; this fails closed if that ever ceased to hold.
        if !loopback_url_is_safe(&base) {
            return Err(AiError::BadBaseUrl);
        }
        let url = format!("{base}/chat/completions");
        let body = build_chat_body(model, messages_json, context_json)?;
        // Empty bearer — the local server accepts unauthenticated loopback calls.
        let resp = self.http.post_json(&url, "", &body)?;
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

/// W3.3 — build the agentic chat body: like `build_chat_body` but (1) uses the
/// tool-aware system prompt, (2) forwards the full multi-turn tool protocol
/// (assistant messages carrying `tool_calls`, and `tool`-role results carrying
/// `tool_call_id`), and (3) attaches the `tools` spec so the model can call them.
/// `tools_json` is the OpenAI `tools` array (validated to be an array).
fn build_chat_body_with_tools(
    model: &str,
    messages_json: &str,
    tools_json: &str,
    context_json: &str,
) -> Result<Value> {
    let history: Value = serde_json::from_str(messages_json).map_err(|_| AiError::BadResponse)?;
    let history = history.as_array().ok_or(AiError::BadResponse)?;
    let tools: Value = serde_json::from_str(tools_json).map_err(|_| AiError::BadResponse)?;
    if !tools.is_array() {
        return Err(AiError::BadResponse);
    }

    let mut messages: Vec<Value> = Vec::with_capacity(history.len() + 2);
    messages.push(json!({ "role": "system", "content": AGENT_SYSTEM_PROMPT_TOOLS }));
    messages.push(json!({
        "role": "system",
        "content": format!("Live app context (JSON snapshot of the member's node/wallet/membership): {context_json}"),
    }));
    for m in history {
        let Some(role) = m.get("role").and_then(Value::as_str) else {
            continue;
        };
        // Forward the shapes the tool protocol needs, defensively:
        //  - assistant with tool_calls (content may be null),
        //  - tool result with tool_call_id + content,
        //  - plain {role, content}.
        let mut msg = serde_json::Map::new();
        msg.insert("role".into(), json!(role));
        if let Some(content) = m.get("content").and_then(Value::as_str) {
            msg.insert("content".into(), json!(content));
        }
        if role == "assistant" {
            if let Some(tc) = m.get("tool_calls").filter(|v| v.is_array()) {
                msg.insert("tool_calls".into(), tc.clone());
            }
        }
        if role == "tool" {
            if let Some(id) = m.get("tool_call_id").and_then(Value::as_str) {
                msg.insert("tool_call_id".into(), json!(id));
            }
        }
        // Skip a message that carries neither content nor tool_calls (nothing to send).
        if msg.contains_key("content") || msg.contains_key("tool_calls") {
            messages.push(Value::Object(msg));
        }
    }
    Ok(json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "stream": false,
    }))
}

/// Parse the assistant MESSAGE object (`choices[0].message`) from a chat-completions
/// response, preserving `tool_calls` so the frontend loop can act on them. Returned
/// as a JSON string (the message object). Missing message → coarse `BadResponse`.
fn parse_chat_message(resp: &str) -> Result<String> {
    let v: Value = serde_json::from_str(resp).map_err(|_| AiError::BadResponse)?;
    let msg = v
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c0| c0.get("message"))
        .ok_or(AiError::BadResponse)?;
    // A valid assistant turn has content and/or tool_calls; reject an empty shape
    // rather than return a fabricated blank (Rule 1).
    if msg.get("content").and_then(Value::as_str).is_none()
        && !msg.get("tool_calls").map(Value::is_array).unwrap_or(false)
    {
        return Err(AiError::BadResponse);
    }
    Ok(msg.to_string())
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

/// **Command — ai_chat_tools (W3.3, @rule8).** One turn of the AGENTIC loop: read
/// the SEALED config for `provider_id`, POST the tool-enabled OpenAI body to the
/// STORED baseURL with the sealed key, and return the assistant MESSAGE (content
/// and/or `tool_calls`) as JSON. The webview runs the loop — executing tool calls
/// through its own gated handlers and appending results — but can NEVER supply the
/// URL or the key (invariants 1 + 3). Errors are coarse + secret-free.
#[tauri::command]
pub fn ai_chat_tools(
    provider_id: String,
    messages_json: String,
    tools_json: String,
    context_json: String,
    ai: tauri::State<'_, AiState>,
) -> std::result::Result<String, String> {
    ai.0.chat_tools(&provider_id, &messages_json, &tools_json, &context_json)
        .map_err(|e| e.to_string())
}

/// **Command — ai_chat_local (BC-3.2, F-1 hardened).** REAL LOCAL inference
/// against the bundled `llama-server` (llama.cpp) on the loopback endpoint, with
/// NO api key. The endpoint is derived IN RUST from the serve manager's loopback
/// port ([`crate::serve::ServeState`]) — the webview supplies ONLY the messages +
/// context, NEVER a URL or host. A compromised renderer therefore cannot redirect
/// the "local" route to a remote host (no attacker-controllable URL exists on this
/// path). Returns the model completion only.
#[tauri::command]
pub fn ai_chat_local(
    messages_json: String,
    context_json: String,
    ai: tauri::State<'_, AiState>,
    serve: tauri::State<'_, crate::serve::ServeState>,
) -> std::result::Result<String, String> {
    // The port + model name come from Rust-owned state, never the webview. (For a
    // single-model llama-server the model field is a label; using the pinned model
    // name keeps the webview from supplying anything on the local path.)
    let port = serve.0.port();
    ai.0.chat_local(port, crate::model::MODEL_FILE, &messages_json, &context_json)
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
