//! D-11 (RC52) — the core model proxy: the one choke point for every cloud model call.
//! ADR-2026-10-26 P-2, P-4, P-6, P-7 and the core invariant:
//!
//! > A vendor key is created by the user, stored only in their device keychain, and read only by
//! > the Rust core's model proxy. The proxy sends the key only to that vendor's pinned host, within
//! > the user's caps, and every use leaves a signed local record.
//!
//! ## One call ([`ModelProxy::call`])
//!
//! 1. **Resolve** the provider and model from the caller's route (a sidecar route token binds both;
//!    nothing in a request body can change them). No key yet.
//! 2. **Egress**: a governed vendor's pinned host, or a custom endpoint the member confirmed
//!    (unverified). Anything else is refused.
//! 3. **Guard**: the usage log must be writable; caps and the anomaly halt are checked against the
//!    log's own totals plus this call's worst case. A refusal sends nothing.
//! 4. **Begin record** written (durably) before anything is sent; if it cannot be written the call
//!    is refused.
//! 5. **Key** read from the keychain into an [`ApiKey`] (wiped on drop, not serializable).
//! 6. **Adapter** call through the pinned transport; text streams back as it arrives.
//! 7. **End record** (the vendor's tokens and the cost, no content) written before the reply is
//!    returned. If it cannot be written the reply is withheld.
//!
//! ## Data sources (Rule 7)
//! - Keys: the OS keychain (service `ai.citrate.core`, account `provider-key:<id>`; a custom
//!   endpoint's account also names a hash of its URL, so its key is bound to that URL).
//! - Caps, halts, approved models, prices, custom endpoints: `<app data>/providers/guard.json`,
//!   sealed (HMAC key in the keychain).
//! - Usage and spend: `<app data>/providers/usage.jsonl`, the signed usage log.
//! - Model lists: the adapter's own list, plus the vendor's `/models` answer at the key test.

use crate::ai::AiKeyring;
use crate::model_provider::{
    ApiKey, ChatRequest, ChatResponse, KeyTest, ModelProvider, ProviderCtx, ProviderError,
    ProviderHttp, ProviderId, Vendor,
};
use crate::provider_egress::{CustomEndpoint, ResolvedEndpoint};
use crate::provider_guard::{CallPlan, GuardBook, Price, Refusal};
use crate::provider_openai_compat::OpenAiCompatAdapter;
use crate::provider_usage_log::{Kind, LogError, UsageLog};
use crate::secret::SecretString;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// App-data subdirectory for the guard file and the usage log.
pub const DIR_NAME: &str = "providers";
/// Keychain account prefix for vendor keys.
pub const KEY_ACCOUNT_PREFIX: &str = "provider-key:";
/// Output ceiling when a request names none, and the most a request may ask for.
pub const DEFAULT_MAX_OUTPUT: u32 = 4096;
pub const MAX_OUTPUT_TOKENS: u32 = 65_536;
/// Tokens added per message for the chat template, on top of its UTF-8 bytes.
pub const PER_MESSAGE_OVERHEAD: u64 = 16;
/// Route tokens: lifetime and how many may be live at once.
pub const ROUTE_TTL_MS: u64 = 12 * 60 * 60 * 1000;
pub const MAX_ROUTES: usize = 64;
/// The model id the usage log and the guard use for a Cognition delegation (it has no chat model).
pub const DELEGATE_MODEL: &str = "devin";
/// Custom endpoints a member may add.
pub const MAX_CUSTOM_ENDPOINTS: usize = 8;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyError {
    /// No such route token (or it expired).
    Unauthorized,
    /// The request is malformed, or names a different model than its route.
    BadRequest(String),
    /// No key is stored for this provider.
    NotConfigured,
    /// No adapter for this provider in this build.
    NotAvailable,
    /// Egress refused (unknown or unconfirmed endpoint, off-host URL).
    Egress(String),
    /// The spend guard refused (cap, halt, unpriced model under a currency cap).
    Refused(Refusal),
    /// The usage log cannot take a record, so nothing is sent / the reply is withheld.
    LogUnavailable(LogError),
    /// The vendor call failed.
    Provider(ProviderError),
    /// The keychain or a settings file could not be used.
    Storage(String),
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProxyError::Unauthorized => {
                f.write_str("the model route is not valid (it may have expired)")
            }
            ProxyError::BadRequest(m) => write!(f, "bad model request: {m}"),
            ProxyError::NotConfigured => f.write_str("no key is set for this provider"),
            ProxyError::NotAvailable => {
                f.write_str("this provider is not available in this version yet")
            }
            ProxyError::Egress(m) => write!(f, "blocked: {m}"),
            ProxyError::Refused(r) => write!(f, "refused: {r}"),
            ProxyError::LogUnavailable(e) => write!(f, "refused: {e}; nothing was sent"),
            ProxyError::Provider(e) => write!(f, "{e}"),
            ProxyError::Storage(m) => write!(f, "provider settings: {m}"),
        }
    }
}

impl ProxyError {
    /// The HTTP status and error type the loopback endpoint answers with.
    pub fn http(&self) -> (u16, &'static str) {
        match self {
            ProxyError::Unauthorized => (401, "unauthorized"),
            ProxyError::BadRequest(_) => (400, "bad_request"),
            ProxyError::NotConfigured => (409, "not_configured"),
            ProxyError::NotAvailable => (501, "not_available"),
            ProxyError::Egress(_) => (403, "egress_refused"),
            ProxyError::Refused(Refusal::Halted(_)) => (423, "provider_halted"),
            ProxyError::Refused(_) => (429, "cap_exceeded"),
            ProxyError::LogUnavailable(_) => (503, "usage_log_unavailable"),
            ProxyError::Provider(_) => (502, "provider_error"),
            ProxyError::Storage(_) => (500, "storage"),
        }
    }

    /// The coarse outcome code written to the usage log.
    pub fn outcome(&self) -> &'static str {
        match self {
            ProxyError::Unauthorized => "unauthorized",
            ProxyError::BadRequest(_) => "bad_request",
            ProxyError::NotConfigured => "not_configured",
            ProxyError::NotAvailable => "not_available",
            ProxyError::Egress(_) => "egress_refused",
            ProxyError::Refused(Refusal::Halted(_)) => "halted",
            ProxyError::Refused(Refusal::PriceUnknown) => "price_unknown",
            ProxyError::Refused(Refusal::CapExceeded { .. }) => "cap_exceeded",
            ProxyError::LogUnavailable(_) => "log_unavailable",
            ProxyError::Provider(ProviderError::Unauthorized) => "key_rejected",
            ProxyError::Provider(ProviderError::RateLimited) => "rate_limited",
            ProxyError::Provider(ProviderError::Redirect) => "redirect_refused",
            ProxyError::Provider(ProviderError::Egress(_)) => "egress_refused",
            ProxyError::Provider(_) => "provider_error",
            ProxyError::Storage(_) => "storage",
        }
    }
}

// ---------------------------------------------------------------------------
// Adapters
// ---------------------------------------------------------------------------

/// How a provider is served in this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    OpenAiCompatible,
    /// A remote-agent delegate (Cognition): keys and the usage log only, never a chat call.
    Delegate,
}

impl AdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AdapterKind::OpenAiCompatible => "openai-compatible",
            AdapterKind::Delegate => "delegate",
        }
    }
}

/// The adapter registry. Vendors whose public API is OpenAI-compatible at their pinned base are
/// served by [`OpenAiCompatAdapter`] until their native adapter lands (WP-S1-PROV-ADAPT replaces
/// these entries); Anthropic, Gemini and Meta have no adapter yet and report "not available".
pub fn vendor_adapter(v: Vendor) -> Option<(Arc<dyn ModelProvider>, AdapterKind)> {
    let compat = |prefix: &'static str| -> Option<(Arc<dyn ModelProvider>, AdapterKind)> {
        Some((
            Arc::new(OpenAiCompatAdapter::new(
                ProviderId::Vendor(v),
                prefix,
                vec![],
            )),
            AdapterKind::OpenAiCompatible,
        ))
    };
    match v {
        Vendor::OpenAi => compat("sk-"),
        Vendor::Xai => compat("xai-"),
        Vendor::DeepSeek => compat("sk-"),
        Vendor::Anthropic | Vendor::Gemini | Vendor::Meta => None,
    }
}

// ---------------------------------------------------------------------------
// Routes (what the sidecar holds instead of a key)
// ---------------------------------------------------------------------------

/// What a route token is bound to. Only core mints one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyRoute {
    pub provider: ProviderId,
    pub model: String,
    pub session_id: String,
}

/// What the sidecar gets: the loopback base URL and a bearer for this route only.
pub struct ProxyGrant {
    pub base_url: String,
    pub bearer: SecretString,
}

struct RouteEntry {
    route: ProxyRoute,
    /// The chat-route settings folder (`chat_route::route_dir`): every call on this route is
    /// refused while "Local model only" is on there.
    gate: PathBuf,
    expires_ms: u64,
}

fn same_base(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

fn bearer_hash(b: &str) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(b.as_bytes()).into()
}

fn valid_label(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

// ---------------------------------------------------------------------------
// The proxy
// ---------------------------------------------------------------------------

struct Inner {
    guard: GuardBook,
    log: UsageLog,
}

pub struct ModelProxy {
    dir: PathBuf,
    keyring: Arc<dyn AiKeyring>,
    http: Arc<dyn ProviderHttp>,
    inner: Mutex<Inner>,
    routes: Mutex<HashMap<[u8; 32], RouteEntry>>,
    port: std::sync::atomic::AtomicU16,
}

/// A call that passed the guard and has its begin record written.
struct Prepared {
    call_id: String,
    endpoint: ResolvedEndpoint,
    key: ApiKey,
    adapter: Arc<dyn ModelProvider>,
    plan: CallPlan,
    route: ProxyRoute,
    conversation: Option<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// An upper bound on a request's input tokens: UTF-8 bytes of every message, tool call and tool
/// schema, plus a per-message allowance.
pub fn input_token_bound(req: &ChatRequest) -> u64 {
    let mut n: u64 = PER_MESSAGE_OVERHEAD;
    for m in &req.messages {
        n = n.saturating_add(m.content.len() as u64 + PER_MESSAGE_OVERHEAD);
        for c in &m.tool_calls {
            n = n.saturating_add((c.id.len() + c.name.len() + c.arguments.len()) as u64);
        }
        if let Some(id) = &m.tool_call_id {
            n = n.saturating_add(id.len() as u64);
        }
    }
    for t in &req.tools {
        n = n.saturating_add(
            (t.name.len() + t.description.len() + t.parameters.to_string().len()) as u64,
        );
    }
    n
}

impl ModelProxy {
    /// Open the proxy's state in `dir` (`<app data>/providers`).
    pub fn new(dir: PathBuf, keyring: Arc<dyn AiKeyring>, http: Arc<dyn ProviderHttp>) -> Self {
        let now = now_ms();
        let guard = crate::provider_guard::load(&dir, keyring.as_ref(), now);
        let log = UsageLog::open(&dir, keyring.as_ref());
        ModelProxy {
            dir,
            keyring,
            http,
            inner: Mutex::new(Inner { guard, log }),
            routes: Mutex::new(HashMap::new()),
            port: std::sync::atomic::AtomicU16::new(0),
        }
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- adapters and endpoints -------------------------------------------------------------

    /// The adapter and the resolved endpoint for a provider id. A custom endpoint must exist and
    /// be confirmed by the member.
    fn resolve(
        &self,
        id: &ProviderId,
    ) -> Result<(Arc<dyn ModelProvider>, AdapterKind, ResolvedEndpoint), ProxyError> {
        match id {
            ProviderId::Vendor(v) => {
                let (a, k) = vendor_adapter(*v).ok_or(ProxyError::NotAvailable)?;
                // A registry slip must never send one vendor's key through another's adapter.
                if a.id() != *id {
                    return Err(ProxyError::NotAvailable);
                }
                let ep = crate::provider_egress::resolve_vendor(*v)
                    .map_err(|e| ProxyError::Egress(e.to_string()))?;
                Ok((a, k, ep))
            }
            ProviderId::Gateway => {
                let ep = crate::provider_egress::resolve_gateway()
                    .map_err(|e| ProxyError::Egress(e.to_string()))?;
                Ok((
                    Arc::new(OpenAiCompatAdapter::new(
                        ProviderId::Gateway,
                        "cgk_",
                        vec![],
                    )),
                    AdapterKind::OpenAiCompatible,
                    ep,
                ))
            }
            ProviderId::Cognition => {
                let ep = crate::provider_egress::resolve_delegate()
                    .map_err(|e| ProxyError::Egress(e.to_string()))?;
                Ok((
                    Arc::new(crate::cognition::CognitionAdapter),
                    AdapterKind::Delegate,
                    ep,
                ))
            }
            ProviderId::Custom(cid) => {
                let c = self
                    .inner()
                    .guard
                    .customs
                    .iter()
                    .find(|c| &c.id == cid)
                    .cloned()
                    .ok_or_else(|| {
                        ProxyError::Egress("that custom endpoint is not set up".into())
                    })?;
                if !c.confirmed {
                    return Err(ProxyError::Egress(
                        "that custom endpoint has not been confirmed in Settings".into(),
                    ));
                }
                let ep = crate::provider_egress::resolve_custom(&c)
                    .map_err(|e| ProxyError::Egress(e.to_string()))?;
                let models = c
                    .models
                    .iter()
                    .map(|m| crate::model_provider::ModelInfo {
                        id: m.clone(),
                        display_name: m.clone(),
                        supports_tools: true,
                        input_micros_per_mtok: None,
                        output_micros_per_mtok: None,
                    })
                    .collect();
                Ok((
                    Arc::new(OpenAiCompatAdapter::new(id.clone(), "", models)),
                    AdapterKind::OpenAiCompatible,
                    ep,
                ))
            }
        }
    }

    /// The keychain account for a provider's key. A custom endpoint's account names a hash of its
    /// URL, so a key is never sent to a URL other than the one it was set for.
    fn key_account(id: &ProviderId, ep: &ResolvedEndpoint) -> String {
        match id {
            ProviderId::Vendor(_) | ProviderId::Gateway | ProviderId::Cognition => {
                format!("{KEY_ACCOUNT_PREFIX}{}", id.as_str())
            }
            ProviderId::Custom(c) => {
                use sha2::Digest;
                let h = sha2::Sha256::digest(ep.base_url.as_str().as_bytes());
                format!("{KEY_ACCOUNT_PREFIX}{c}:{}", hex::encode(&h[..8]))
            }
        }
    }

    fn read_key(&self, id: &ProviderId, ep: &ResolvedEndpoint) -> Result<ApiKey, ProxyError> {
        self.read_account(&Self::key_account(id, ep))
    }

    fn read_account(&self, account: &str) -> Result<ApiKey, ProxyError> {
        let v = self
            .keyring
            .get(account)
            .map_err(|_| ProxyError::Storage("the keychain could not be read".into()))?
            .ok_or(ProxyError::NotConfigured)?;
        Ok(ApiKey::from_secret(SecretString::new(v)))
    }

    /// The provider whose resolved base URL is exactly `base_url`: a governed vendor's pinned
    /// base, the Citrate gateway's, or a custom endpoint the member confirmed in Settings › Models.
    /// Anything else is refused: no cloud call goes to a host the proxy does not pin or the member
    /// did not confirm (ADR P-4).
    pub fn provider_for_base(&self, base_url: &str) -> Result<ProviderId, ProxyError> {
        for spec in crate::provider_egress::VENDORS {
            if same_base(spec.base_url, base_url) {
                return Ok(ProviderId::Vendor(spec.vendor));
            }
        }
        if same_base(crate::provider_egress::GATEWAY_BASE_URL, base_url) {
            return Ok(ProviderId::Gateway);
        }
        let custom = self
            .inner()
            .guard
            .customs
            .iter()
            .find(|c| same_base(&c.base_url, base_url))
            .cloned();
        match custom {
            Some(c) if c.confirmed => Ok(ProviderId::Custom(c.id)),
            Some(_) => Err(ProxyError::Egress(
                "that endpoint has not been confirmed in Settings › Models".into(),
            )),
            None => Err(ProxyError::Egress(
                "that endpoint is not a provider connected in Settings › Models (add it there as a custom endpoint and confirm it)".into(),
            )),
        }
    }

    // ---- the call ------------------------------------------------------------------------------

    /// One model call for `route`, end to end (see the module docs).
    pub fn call(
        &self,
        route: &ProxyRoute,
        req: ChatRequest,
        conversation: Option<String>,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProxyError> {
        let (prepared, req) = self.prepare(route, req, conversation)?;
        self.execute(prepared, &req, on_text)
    }

    /// A call that arrived on a sidecar route token (the loopback endpoint).
    pub(crate) fn call_routed(
        &self,
        bearer: &str,
        req: ChatRequest,
        conversation: Option<String>,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProxyError> {
        let entry = {
            let routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
            routes
                .get(&bearer_hash(bearer))
                .filter(|r| r.expires_ms > now_ms())
                .map(|r| (r.route.clone(), r.gate.clone()))
        };
        let (route, gate) = entry.ok_or(ProxyError::Unauthorized)?;
        crate::chat_route::remote_chat_allowed(&gate).map_err(ProxyError::Egress)?;
        self.call(&route, req, conversation, on_text)
    }

    /// Steps 1-5: everything that can refuse before anything is sent. The loopback endpoint calls
    /// this before it commits to a streamed answer, so a refusal is a plain error status.
    fn prepare(
        &self,
        route: &ProxyRoute,
        mut req: ChatRequest,
        conversation: Option<String>,
    ) -> Result<(Prepared, ChatRequest), ProxyError> {
        // 0. A delegate is never a chat model.
        if route.provider.is_delegate() {
            return Err(ProxyError::BadRequest(
                "Cognition runs delegated tasks; it is not a chat model".into(),
            ));
        }
        // 1. The route decides the provider and model; the body may only repeat the model.
        if req.model.is_empty() {
            req.model = route.model.clone();
        }
        if req.model != route.model {
            return Err(ProxyError::BadRequest(
                "the request names a different model than its route".into(),
            ));
        }
        if req.max_tokens == 0 {
            req.max_tokens = DEFAULT_MAX_OUTPUT;
        }
        req.max_tokens = req.max_tokens.min(MAX_OUTPUT_TOKENS);
        let conversation = conversation.filter(|c| valid_label(c, 64));
        // 2. Egress: a pinned vendor host, or a confirmed custom endpoint.
        let (adapter, _kind, endpoint) = self.resolve(&route.provider)?;
        // 3-4. Guard and the begin record, under the state lock (so concurrent calls see each
        //      other's reservations).
        let provider = route.provider.to_string();
        let adapter_models: Vec<String> = adapter.models().into_iter().map(|m| m.id).collect();
        let call_id = format!("pc-{:016x}", rand::random::<u64>());
        let now = now_ms();
        let plan = {
            let mut inner = self.inner();
            let Inner { guard, log } = &mut *inner;
            log.ready().map_err(ProxyError::LogUnavailable)?;
            let plan = CallPlan {
                provider: provider.clone(),
                model: req.model.clone(),
                reserve_in: input_token_bound(&req),
                reserve_out: u64::from(req.max_tokens),
                price: guard.price_for(
                    &provider,
                    &req.model,
                    Self::adapter_price(adapter.as_ref(), &req.model),
                ),
            };
            let (today, month) = log.period_use(&provider, now);
            if let Err(refusal) = guard.admit(&plan, &adapter_models, today, month, now) {
                let err = ProxyError::Refused(refusal);
                let mut r = Self::record_for(Kind::Refused, &call_id, route, &endpoint, now);
                r.conversation = conversation.clone();
                r.outcome = Some(err.outcome().into());
                // Best effort: the refusal stands whether or not it could be recorded.
                let _ = log.append(r);
                let _ = crate::provider_guard::save(&self.dir, guard);
                return Err(err);
            }
            let mut r = Self::record_for(Kind::Begin, &call_id, route, &endpoint, now);
            r.conversation = conversation.clone();
            r.reserve_in = Some(plan.reserve_in);
            r.reserve_out = Some(plan.reserve_out);
            r.reserve_micros = plan.reserve_micros();
            log.append(r).map_err(ProxyError::LogUnavailable)?;
            plan
        };
        // 5. The key, last: a refused call never reads it.
        let key = match self.read_key(&route.provider, &endpoint) {
            Ok(k) => k,
            Err(e) => {
                self.finish(
                    &call_id,
                    route,
                    &endpoint,
                    &plan,
                    &conversation,
                    None,
                    Some(&e),
                )?;
                return Err(e);
            }
        };
        Ok((
            Prepared {
                call_id,
                endpoint,
                key,
                adapter,
                plan,
                route: route.clone(),
                conversation,
            },
            req,
        ))
    }

    /// Steps 6-7: the vendor call and the end record. The reply is returned only once the end
    /// record is written.
    fn execute(
        &self,
        p: Prepared,
        req: &ChatRequest,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProxyError> {
        let result = {
            let ctx = ProviderCtx::new(&p.endpoint, &p.key, self.http.as_ref());
            p.adapter
                .complete(&ctx, req, on_text)
                .map_err(ProxyError::Provider)
        };
        drop(p.key);
        self.finish(
            &p.call_id,
            &p.route,
            &p.endpoint,
            &p.plan,
            &p.conversation,
            result.as_ref().ok().and_then(|r| r.usage),
            result.as_ref().err(),
        )?;
        result
    }

    /// One logged vendor operation core composes itself (a fixed-path media request, a Cognition
    /// delegation step): the same steps as [`Self::call`] (egress, guard, begin record written
    /// before anything is sent, the key read last, end record written before the result is
    /// returned), with no token reservation. `model_is_members` marks a model the member chose in a
    /// setting (it does not trip the unknown-model halt).
    pub(crate) fn logged_op<T>(
        &self,
        provider: &ProviderId,
        model: &str,
        session_id: &str,
        model_is_members: bool,
        op: impl FnOnce(&ProviderCtx<'_>) -> Result<T, ProviderError>,
    ) -> Result<T, ProxyError> {
        if model.is_empty() || model.len() > 200 || !valid_label(session_id, 128) {
            return Err(ProxyError::BadRequest("bad route".into()));
        }
        let (adapter, _kind, endpoint) = self.resolve(provider)?;
        let route = ProxyRoute {
            provider: provider.clone(),
            model: model.to_string(),
            session_id: session_id.to_string(),
        };
        let mut known: Vec<String> = adapter.models().into_iter().map(|m| m.id).collect();
        if model_is_members {
            known.push(model.to_string());
        }
        let call_id = format!("po-{:016x}", rand::random::<u64>());
        let now = now_ms();
        let plan = {
            let mut inner = self.inner();
            let Inner { guard, log } = &mut *inner;
            log.ready().map_err(ProxyError::LogUnavailable)?;
            let plan = CallPlan {
                provider: provider.to_string(),
                model: model.to_string(),
                reserve_in: 0,
                reserve_out: 0,
                price: None,
            };
            let (today, month) = log.period_use(&plan.provider, now);
            if let Err(refusal) = guard.admit(&plan, &known, today, month, now) {
                let err = ProxyError::Refused(refusal);
                let mut r = Self::record_for(Kind::Refused, &call_id, &route, &endpoint, now);
                r.outcome = Some(err.outcome().into());
                let _ = log.append(r);
                let _ = crate::provider_guard::save(&self.dir, guard);
                return Err(err);
            }
            let mut r = Self::record_for(Kind::Begin, &call_id, &route, &endpoint, now);
            r.reserve_in = Some(0);
            r.reserve_out = Some(0);
            log.append(r).map_err(ProxyError::LogUnavailable)?;
            plan
        };
        let key = match self.read_key(provider, &endpoint) {
            Ok(k) => k,
            Err(e) => {
                self.finish(&call_id, &route, &endpoint, &plan, &None, None, Some(&e))?;
                return Err(e);
            }
        };
        let result = op(&ProviderCtx::new(&endpoint, &key, self.http.as_ref()))
            .map_err(ProxyError::Provider);
        drop(key);
        self.finish(
            &call_id,
            &route,
            &endpoint,
            &plan,
            &None,
            None,
            result.as_ref().err(),
        )?;
        result
    }

    /// Write a call's end record (and run the spike check). `Err` = it could not be written, so
    /// the caller must not return the reply.
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &self,
        call_id: &str,
        route: &ProxyRoute,
        endpoint: &ResolvedEndpoint,
        plan: &CallPlan,
        conversation: &Option<String>,
        usage: Option<crate::model_provider::Usage>,
        error: Option<&ProxyError>,
    ) -> Result<(), ProxyError> {
        let now = now_ms();
        let mut inner = self.inner();
        let Inner { guard, log } = &mut *inner;
        let mut r = Self::record_for(Kind::End, call_id, route, endpoint, now);
        r.conversation = conversation.clone();
        if let Some(u) = usage {
            r.tokens_in = Some(u.input_tokens);
            r.tokens_out = Some(u.output_tokens);
            r.cost_micros = plan
                .price
                .map(|pr| pr.cost(u.input_tokens, u.output_tokens));
        }
        r.outcome = Some(match error {
            None => "ok".into(),
            Some(e) => e.outcome().into(),
        });
        log.append(r).map_err(ProxyError::LogUnavailable)?;
        if let Some(u) = usage {
            if guard.observe(&plan.provider, u.total(), now).is_some() {
                let _ = crate::provider_guard::save(&self.dir, guard);
            }
        }
        Ok(())
    }

    /// A usage record for this route and endpoint.
    fn record_for(
        kind: Kind,
        call_id: &str,
        route: &ProxyRoute,
        endpoint: &ResolvedEndpoint,
        now: u64,
    ) -> crate::provider_usage_log::Record {
        let mut r = crate::provider_usage_log::record(
            kind,
            call_id,
            route.provider.as_str(),
            &route.model,
            &route.session_id,
            now,
        );
        r.verified = endpoint.verified;
        r
    }

    /// Record a live key test (a use of the key) and require the log to be writable first.
    fn key_test_logged(
        &self,
        pid: &ProviderId,
        ep: &ResolvedEndpoint,
        adapter: &dyn ModelProvider,
        key: &ApiKey,
    ) -> Result<KeyTest, ProxyError> {
        self.inner()
            .log
            .ready()
            .map_err(ProxyError::LogUnavailable)?;
        let res = adapter
            .test_key(&ProviderCtx::new(ep, key, self.http.as_ref()))
            .map_err(ProxyError::Provider);
        let route = ProxyRoute {
            provider: pid.clone(),
            model: String::new(),
            session_id: "settings".into(),
        };
        let mut r = Self::record_for(
            Kind::KeyTest,
            &format!("kt-{:016x}", rand::random::<u64>()),
            &route,
            ep,
            now_ms(),
        );
        r.outcome = Some(match &res {
            Ok(_) => "ok".into(),
            Err(e) => e.outcome().into(),
        });
        self.inner()
            .log
            .append(r)
            .map_err(ProxyError::LogUnavailable)?;
        res.map(|mut t| {
            t.models
                .truncate(crate::provider_openai_compat::MAX_LISTED_MODELS);
            t
        })
    }

    // ---- keys (one way) ---------------------------------------------------------------------

    /// Check the key's format, test it live (a free vendor endpoint), and only then store it.
    pub fn set_key(&self, id: &str, key: SecretString, now: u64) -> Result<KeyTest, ProxyError> {
        let pid = ProviderId::parse(id).ok_or(ProxyError::BadRequest("unknown provider".into()))?;
        let (adapter, _, ep) = self.resolve(&pid)?;
        let key = ApiKey::from_secret(key);
        adapter
            .check_key_format(&key)
            .map_err(ProxyError::Provider)?;
        let test = self.key_test_logged(&pid, &ep, adapter.as_ref(), &key)?;
        self.store_key(&pid, &ep, &key)?;
        let mut inner = self.inner();
        inner.guard.key_set(pid.as_str(), test.models.clone(), now);
        let _ = crate::provider_guard::save(&self.dir, &inner.guard);
        Ok(test)
    }

    /// The one keychain write of a key into the proxy's store.
    fn store_key(
        &self,
        pid: &ProviderId,
        ep: &ResolvedEndpoint,
        key: &ApiKey,
    ) -> Result<(), ProxyError> {
        self.keyring
            .set(&Self::key_account(pid, ep), key.expose_for_keychain())
            .map_err(|_| ProxyError::Storage("the keychain could not be written".into()))
    }

    /// Move a key the member already set elsewhere in core (a legacy Settings › Providers preset,
    /// an escalation endpoint) into the proxy's store, without a live test: it was in use before.
    /// The vendor format check still applies. An existing proxy key is kept unless `replace`:
    /// the result says whether this key was stored (`true`) or an existing one stays (`false`).
    pub(crate) fn import_key(
        &self,
        id: &ProviderId,
        key: SecretString,
        replace: bool,
    ) -> Result<bool, ProxyError> {
        if id.is_delegate() {
            return Err(ProxyError::BadRequest(
                "a delegate key is set in Settings › Models".into(),
            ));
        }
        let (adapter, _, ep) = match id {
            ProviderId::Custom(_) => self.resolve_any_custom_adapter(id)?,
            _ => self.resolve(id)?,
        };
        let key = ApiKey::from_secret(key);
        adapter
            .check_key_format(&key)
            .map_err(ProxyError::Provider)?;
        if !replace && self.key_configured(id) {
            return Ok(false);
        }
        self.store_key(id, &ep, &key)?;
        let mut inner = self.inner();
        let listed = inner.guard.get(id.as_str()).listed_models.clone();
        inner.guard.key_set(id.as_str(), listed, now_ms());
        crate::provider_guard::save(&self.dir, &inner.guard).map_err(ProxyError::Storage)?;
        Ok(true)
    }

    /// Whether calls to `id` can be resolved now (an adapter exists; a custom endpoint is
    /// confirmed).
    pub(crate) fn resolves(&self, id: &ProviderId) -> bool {
        self.resolve(id).is_ok()
    }

    /// The custom endpoint set up for exactly `base_url`, confirmed or not.
    pub(crate) fn custom_for_base(&self, base_url: &str) -> Result<ProviderId, ProxyError> {
        self.inner()
            .guard
            .customs
            .iter()
            .find(|c| same_base(&c.base_url, base_url))
            .map(|c| ProviderId::Custom(c.id.clone()))
            .ok_or_else(|| ProxyError::Egress("that custom endpoint is not set up".into()))
    }

    /// One fixed-path JSON POST core composes itself (a remote image, `/images/generations`),
    /// through [`Self::logged_op`]. Returns the answer body.
    pub(crate) fn post_fixed(
        &self,
        id: &ProviderId,
        model: &str,
        session_id: &str,
        path: &'static str,
        body: &serde_json::Value,
    ) -> Result<String, ProxyError> {
        self.logged_op(id, model, session_id, true, |ctx| {
            let resp = ctx.send(crate::model_provider::OutboundRequest {
                method: crate::model_provider::Method::Post,
                path: path.to_string(),
                headers: vec![("accept", "application/json".to_string())],
                auth: crate::model_provider::AuthPlacement::Bearer,
                body: Some(body.clone()),
            })?;
            if !(200..300).contains(&resp.status) {
                return Err(crate::model_provider::status_error(resp.status));
            }
            resp.text()
        })
    }

    /// The member named `model` for `id` in a setting: approve it for the anomaly halt.
    pub(crate) fn approve_model(&self, id: &ProviderId, model: &str) -> Result<(), ProxyError> {
        let mut inner = self.inner();
        inner
            .guard
            .approve_model(id.as_str(), model)
            .map_err(ProxyError::BadRequest)?;
        crate::provider_guard::save(&self.dir, &inner.guard).map_err(ProxyError::Storage)
    }

    /// The provider a member-entered base URL maps to (a pinned vendor or the gateway), else the
    /// custom endpoint for that URL (added unconfirmed, labelled `label`, when there is none).
    pub(crate) fn provider_or_custom(
        &self,
        label: &str,
        base_url: &str,
        model: &str,
    ) -> Result<ProviderId, ProxyError> {
        match self.provider_for_base(base_url) {
            Ok(pid) => Ok(pid),
            Err(_) => self.ensure_custom(label, base_url, model),
        }
    }

    /// Add a custom endpoint for `base_url` unless one exists (a migrated legacy endpoint). It
    /// starts unconfirmed, like any custom endpoint: calls to it are refused until the member
    /// confirms the URL in Settings › Models.
    pub(crate) fn ensure_custom(
        &self,
        label: &str,
        base_url: &str,
        model: &str,
    ) -> Result<ProviderId, ProxyError> {
        let existing = self
            .inner()
            .guard
            .customs
            .iter()
            .find(|c| same_base(&c.base_url, base_url))
            .map(|c| c.id.clone());
        if let Some(id) = existing {
            return Ok(ProviderId::Custom(id));
        }
        let c = self.add_custom(label, base_url, vec![model.to_string()], now_ms())?;
        Ok(ProviderId::Custom(c.id))
    }

    /// Test the stored key live.
    pub fn test_key(&self, id: &str, now: u64) -> Result<KeyTest, ProxyError> {
        let pid = ProviderId::parse(id).ok_or(ProxyError::BadRequest("unknown provider".into()))?;
        let (adapter, _, ep) = self.resolve(&pid)?;
        let key = self.read_key(&pid, &ep)?;
        let t = self.key_test_logged(&pid, &ep, adapter.as_ref(), &key)?;
        let mut inner = self.inner();
        inner.guard.key_set(pid.as_str(), t.models.clone(), now);
        let _ = crate::provider_guard::save(&self.dir, &inner.guard);
        Ok(t)
    }

    /// Delete the stored key (and revoke every route to this provider).
    pub fn remove_key(&self, id: &str) -> Result<(), ProxyError> {
        let pid = ProviderId::parse(id).ok_or(ProxyError::BadRequest("unknown provider".into()))?;
        let account = match &pid {
            ProviderId::Vendor(_) | ProviderId::Gateway | ProviderId::Cognition => {
                format!("{KEY_ACCOUNT_PREFIX}{}", pid.as_str())
            }
            ProviderId::Custom(_) => {
                let (_, _, ep) = self.resolve_any_custom(&pid)?;
                Self::key_account(&pid, &ep)
            }
        };
        self.keyring
            .delete(&account)
            .map_err(|_| ProxyError::Storage("the keychain could not be written".into()))?;
        self.revoke_provider(&pid);
        let mut inner = self.inner();
        inner.guard.key_removed(pid.as_str());
        let _ = crate::provider_guard::save(&self.dir, &inner.guard);
        Ok(())
    }

    /// Whether a key is stored (the keychain entry exists). Never reads it into memory beyond the
    /// check.
    pub fn key_configured(&self, id: &ProviderId) -> bool {
        let ep = match id {
            ProviderId::Vendor(v) => crate::provider_egress::resolve_vendor(*v).ok(),
            ProviderId::Gateway => crate::provider_egress::resolve_gateway().ok(),
            ProviderId::Cognition => crate::provider_egress::resolve_delegate().ok(),
            ProviderId::Custom(_) => self.resolve_any_custom(id).ok().map(|(_, _, e)| e),
        };
        ep.is_some_and(|ep| {
            self.keyring
                .get(&Self::key_account(id, &ep))
                .ok()
                .flatten()
                // Presence only: the value is wiped at once.
                .map(zeroize::Zeroizing::new)
                .is_some()
        })
    }

    /// A custom endpoint's adapter and endpoint, confirmed or not (a key import may precede the
    /// member's confirmation; every call still needs it).
    fn resolve_any_custom_adapter(
        &self,
        id: &ProviderId,
    ) -> Result<(Arc<dyn ModelProvider>, AdapterKind, ResolvedEndpoint), ProxyError> {
        let (c, kind, ep) = self.resolve_any_custom(id)?;
        let models = c
            .models
            .iter()
            .map(|m| crate::model_provider::ModelInfo {
                id: m.clone(),
                display_name: m.clone(),
                supports_tools: true,
                input_micros_per_mtok: None,
                output_micros_per_mtok: None,
            })
            .collect();
        Ok((
            Arc::new(OpenAiCompatAdapter::new(id.clone(), "", models)),
            kind,
            ep,
        ))
    }

    /// A custom endpoint's resolution, confirmed or not (for key removal and status).
    fn resolve_any_custom(
        &self,
        id: &ProviderId,
    ) -> Result<(CustomEndpoint, AdapterKind, ResolvedEndpoint), ProxyError> {
        let c = self
            .inner()
            .guard
            .customs
            .iter()
            .find(|c| c.id == id.as_str())
            .cloned()
            .ok_or_else(|| ProxyError::Egress("that custom endpoint is not set up".into()))?;
        let ep = crate::provider_egress::resolve_custom(&c)
            .map_err(|e| ProxyError::Egress(e.to_string()))?;
        Ok((c, AdapterKind::OpenAiCompatible, ep))
    }

    // ---- custom endpoints (ADR P-4) ---------------------------------------------------------

    /// Add a custom OpenAI-compatible endpoint. It starts unconfirmed: every call to it is refused
    /// until the member confirms this exact URL.
    pub fn add_custom(
        &self,
        label: &str,
        base_url: &str,
        models: Vec<String>,
        now: u64,
    ) -> Result<CustomEndpoint, ProxyError> {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
            return Err(ProxyError::BadRequest(
                "give the endpoint a name of 1 to 80 characters".into(),
            ));
        }
        let url = crate::provider_egress::parse_base_url(base_url)
            .map_err(|e| ProxyError::Egress(e.to_string()))?;
        let models: Vec<String> = models
            .into_iter()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        if models.is_empty()
            || models.len() > crate::provider_guard::MAX_APPROVED_MODELS
            || models
                .iter()
                .any(|m| m.len() > 200 || m.chars().any(char::is_control))
        {
            return Err(ProxyError::BadRequest(
                "list the model ids this endpoint serves".into(),
            ));
        }
        let mut inner = self.inner();
        if inner.guard.customs.len() >= MAX_CUSTOM_ENDPOINTS {
            return Err(ProxyError::BadRequest("too many custom endpoints".into()));
        }
        let c = CustomEndpoint {
            id: format!(
                "{}{:08x}",
                crate::model_provider::CUSTOM_PREFIX,
                rand::random::<u32>()
            ),
            label: label.to_string(),
            base_url: url.as_str().trim_end_matches('/').to_string(),
            models,
            confirmed: false,
            created_ms: now,
        };
        inner.guard.customs.push(c.clone());
        crate::provider_guard::save(&self.dir, &inner.guard).map_err(ProxyError::Storage)?;
        Ok(c)
    }

    /// The member confirms a custom endpoint by echoing its exact URL.
    pub fn confirm_custom(&self, id: &str, base_url_echo: &str) -> Result<(), ProxyError> {
        let mut inner = self.inner();
        let c = inner
            .guard
            .customs
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| ProxyError::Egress("that custom endpoint is not set up".into()))?;
        if base_url_echo.trim().trim_end_matches('/') != c.base_url {
            return Err(ProxyError::BadRequest(
                "the URL does not match the endpoint being confirmed".into(),
            ));
        }
        c.confirmed = true;
        crate::provider_guard::save(&self.dir, &inner.guard).map_err(ProxyError::Storage)
    }

    /// Remove a custom endpoint and its key.
    pub fn remove_custom(&self, id: &str) -> Result<(), ProxyError> {
        let pid = ProviderId::parse(id)
            .filter(ProviderId::is_custom)
            .ok_or(ProxyError::BadRequest("not a custom endpoint".into()))?;
        if let Ok((_, _, ep)) = self.resolve_any_custom(&pid) {
            self.keyring
                .delete(&Self::key_account(&pid, &ep))
                .map_err(|_| ProxyError::Storage("the keychain could not be written".into()))?;
        }
        self.revoke_provider(&pid);
        let mut inner = self.inner();
        inner.guard.customs.retain(|c| c.id != id);
        inner.guard.providers.remove(id);
        crate::provider_guard::save(&self.dir, &inner.guard).map_err(ProxyError::Storage)
    }

    // ---- routes ----------------------------------------------------------------------------

    pub fn set_port(&self, port: u16) {
        self.port.store(port, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn port(&self) -> u16 {
        self.port.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Mint a bearer bound to one provider, model and session. In-process only: no command
    /// returns one. The sidecar gets it in its session's `LlmEndpoint`. Every call on the route is
    /// refused while "Local model only" is on (`gate` is the chat-route settings folder,
    /// `chat_route::route_dir`; checked per call, so a switch made during a running agent turn
    /// stops its next model request).
    pub fn mint_route(&self, route: ProxyRoute, gate: PathBuf) -> Result<ProxyGrant, ProxyError> {
        self.mint(route, gate)
    }

    fn mint(&self, route: ProxyRoute, gate: PathBuf) -> Result<ProxyGrant, ProxyError> {
        if route.provider.is_delegate() {
            return Err(ProxyError::BadRequest(
                "Cognition runs delegated tasks; it is not a chat model".into(),
            ));
        }
        let port = self.port();
        if port == 0 {
            return Err(ProxyError::Storage("the model proxy is not running".into()));
        }
        if !valid_label(&route.session_id, 128) || route.model.is_empty() || route.model.len() > 200
        {
            return Err(ProxyError::BadRequest("bad route".into()));
        }
        let bearer = format!("cmp_{}", hex::encode(rand::random::<[u8; 32]>()));
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        let now = now_ms();
        routes.retain(|_, r| r.expires_ms > now);
        if routes.len() >= MAX_ROUTES {
            if let Some(oldest) = routes
                .iter()
                .min_by_key(|(_, r)| r.expires_ms)
                .map(|(k, _)| *k)
            {
                routes.remove(&oldest);
            }
        }
        routes.insert(
            bearer_hash(&bearer),
            RouteEntry {
                route,
                gate,
                expires_ms: now.saturating_add(ROUTE_TTL_MS),
            },
        );
        Ok(ProxyGrant {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            bearer: SecretString::new(bearer),
        })
    }

    /// The route a bearer is bound to, if it is live.
    pub fn route_for(&self, bearer: &str) -> Option<ProxyRoute> {
        let routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        routes
            .get(&bearer_hash(bearer))
            .filter(|r| r.expires_ms > now_ms())
            .map(|r| r.route.clone())
    }

    pub fn revoke_session(&self, session_id: &str) {
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        routes.retain(|_, r| r.route.session_id != session_id);
    }

    /// Revoke every route whose session id starts with `prefix` (all cloud agent sessions when
    /// the sidecar stops).
    pub fn revoke_sessions_with_prefix(&self, prefix: &str) {
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        routes.retain(|_, r| !r.route.session_id.starts_with(prefix));
    }

    fn revoke_provider(&self, id: &ProviderId) {
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        routes.retain(|_, r| &r.route.provider != id);
    }

    // ---- guard / log access for the commands ---------------------------------------------------

    pub(crate) fn with_state<T>(&self, f: impl FnOnce(&mut GuardBook, &mut UsageLog) -> T) -> T {
        let mut inner = self.inner();
        let Inner { guard, log } = &mut *inner;
        f(guard, log)
    }

    pub(crate) fn save_guard(&self, g: &GuardBook) -> Result<(), String> {
        crate::provider_guard::save(&self.dir, g)
    }

    /// The price used for a call: the member's entry, else the adapter's published one.
    pub(crate) fn adapter_price(adapter: &dyn ModelProvider, model: &str) -> Option<Price> {
        adapter
            .models()
            .into_iter()
            .find(|m| m.id == model)
            .and_then(|m| {
                Some(Price {
                    input_micros_per_mtok: m.input_micros_per_mtok?,
                    output_micros_per_mtok: m.output_micros_per_mtok?,
                })
            })
    }

    pub(crate) fn kind_for(&self, id: &ProviderId) -> Option<AdapterKind> {
        match id {
            ProviderId::Vendor(v) => vendor_adapter(*v).map(|(_, k)| k),
            ProviderId::Gateway | ProviderId::Custom(_) => Some(AdapterKind::OpenAiCompatible),
            ProviderId::Cognition => Some(AdapterKind::Delegate),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    include!("model_proxy_tests.rs");
}
