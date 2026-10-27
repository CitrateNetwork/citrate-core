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
// Consumed by WP-S1-PROV-AGENT (the sidecar session opener mints routes); used by tests here.
#[cfg_attr(not(test), allow(dead_code))]
pub const ROUTE_TTL_MS: u64 = 12 * 60 * 60 * 1000;
#[cfg_attr(not(test), allow(dead_code))]
pub const MAX_ROUTES: usize = 64;
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
    /// The caller went away mid-call; the vendor call was stopped.
    Cancelled,
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
            ProxyError::Cancelled => {
                f.write_str("the caller went away; the vendor call was stopped")
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
            ProxyError::Provider(ProviderError::BadRequest(_)) => (400, "bad_request"),
            ProxyError::NotConfigured => (409, "not_configured"),
            ProxyError::NotAvailable => (501, "not_available"),
            ProxyError::Cancelled => (499, "cancelled"),
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
            ProxyError::BadRequest(_) | ProxyError::Provider(ProviderError::BadRequest(_)) => {
                "bad_request"
            }
            ProxyError::NotConfigured => "not_configured",
            ProxyError::NotAvailable => "not_available",
            ProxyError::Cancelled => "cancelled",
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
    /// The vendor's own API through its native adapter (WP-S1-PROV-ADAPT).
    Native,
    /// A member-confirmed custom endpoint, spoken to as OpenAI chat-completions.
    OpenAiCompatible,
}

impl AdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AdapterKind::Native => "native",
            AdapterKind::OpenAiCompatible => "openai-compatible",
        }
    }
}

/// The adapter registry: every governed vendor has its native adapter.
pub fn vendor_adapter(v: Vendor) -> (Arc<dyn ModelProvider>, AdapterKind) {
    use crate::provider_chat_vendors::ChatVendorAdapter;
    let a: Arc<dyn ModelProvider> = match v {
        Vendor::OpenAi => Arc::new(crate::provider_openai::OpenAiAdapter),
        Vendor::Anthropic => Arc::new(crate::provider_anthropic::AnthropicAdapter),
        Vendor::Gemini => Arc::new(crate::provider_gemini::GeminiAdapter),
        Vendor::Xai => Arc::new(ChatVendorAdapter::xai()),
        Vendor::DeepSeek => Arc::new(ChatVendorAdapter::deepseek()),
        Vendor::Meta => Arc::new(ChatVendorAdapter::meta()),
    };
    (a, AdapterKind::Native)
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
// Consumed by WP-S1-PROV-AGENT (the sidecar session opener mints routes); used by tests here.
#[cfg_attr(not(test), allow(dead_code))]
pub struct ProxyGrant {
    pub base_url: String,
    pub bearer: SecretString,
}

struct RouteEntry {
    route: ProxyRoute,
    expires_ms: u64,
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
        // The anchor is written before any provider key exists (below, on a fresh install), so a
        // missing anchor while a MAC key or a vendor key is still in the keychain means it was
        // removed: faking a fresh install would also take deleting those keys.
        let anchor_missing = match crate::provider_usage_log::read_anchor_opt(keyring.as_ref()) {
            Ok(None) if Self::keychain_has_provider_keys(keyring.as_ref()) => true,
            // A fresh install: anchor first, then the MAC keys (created by the opens below). If the
            // anchor cannot be written, this launch fails closed instead of going on without one.
            Ok(None) => crate::provider_usage_log::create_anchor(keyring.as_ref()).is_err(),
            _ => false,
        };
        let mut guard = crate::provider_guard::load(&dir, keyring.clone(), now);
        let mut log = UsageLog::open(&dir, keyring.clone());
        if anchor_missing {
            guard.unreadable = true;
            log.mark_broken("the usage log anchor is missing while provider keys remain");
        }
        ModelProxy {
            dir,
            keyring,
            http,
            inner: Mutex::new(Inner { guard, log }),
            routes: Mutex::new(HashMap::new()),
            port: std::sync::atomic::AtomicU16::new(0),
        }
    }

    /// Whether the keychain holds either provider MAC key or any vendor key (presence only; an
    /// unreadable keychain counts as present, which fails closed).
    fn keychain_has_provider_keys(keyring: &dyn AiKeyring) -> bool {
        let vendor_accounts = Vendor::ALL
            .into_iter()
            .map(|v| format!("{KEY_ACCOUNT_PREFIX}{}", v.id()));
        [
            crate::provider_usage_log::LOG_MAC_ACCOUNT.to_string(),
            crate::provider_guard::GUARD_MAC_ACCOUNT.to_string(),
        ]
        .into_iter()
        .chain(vendor_accounts)
        .any(|a| {
            !matches!(
                keyring.get(&a).map(|v| v.map(zeroize::Zeroizing::new)),
                Ok(None)
            )
        })
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
                let (a, k) = vendor_adapter(*v);
                // A registry slip must never send one vendor's key through another's adapter.
                if a.id() != *id {
                    return Err(ProxyError::NotAvailable);
                }
                let ep = crate::provider_egress::resolve_vendor(*v)
                    .map_err(|e| ProxyError::Egress(e.to_string()))?;
                Ok((a, k, ep))
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
                    Arc::new(OpenAiCompatAdapter::new(id.clone(), models)),
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
            ProviderId::Vendor(v) => format!("{KEY_ACCOUNT_PREFIX}{}", v.id()),
            ProviderId::Custom(c) => {
                use sha2::Digest;
                let h = sha2::Sha256::digest(ep.base_url.as_str().as_bytes());
                format!("{KEY_ACCOUNT_PREFIX}{c}:{}", hex::encode(&h[..8]))
            }
        }
    }

    fn read_key(&self, id: &ProviderId, ep: &ResolvedEndpoint) -> Result<ApiKey, ProxyError> {
        let v = self
            .keyring
            .get(&Self::key_account(id, ep))
            .map_err(|_| ProxyError::Storage("the keychain could not be read".into()))?
            .ok_or(ProxyError::NotConfigured)?;
        Ok(ApiKey::from_secret(SecretString::new(v)))
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
        self.execute(prepared, &req, None, on_text)
    }

    /// [`Self::call`] that stops when `cancel` is set (the caller went away): the vendor call is
    /// cut off at its next read and the end record says `cancelled`.
    pub fn call_until(
        &self,
        route: &ProxyRoute,
        req: ChatRequest,
        conversation: Option<String>,
        cancel: crate::model_provider::CancelFlag,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProxyError> {
        let (prepared, req) = self.prepare(route, req, conversation)?;
        self.execute(prepared, &req, Some(cancel), on_text)
    }

    /// Steps 1-5: everything that can refuse before anything is sent. The loopback endpoint calls
    /// this before it commits to a streamed answer, so a refusal is a plain error status.
    fn prepare(
        &self,
        route: &ProxyRoute,
        mut req: ChatRequest,
        conversation: Option<String>,
    ) -> Result<(Prepared, ChatRequest), ProxyError> {
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
            log.check_clock(now).map_err(ProxyError::LogUnavailable)?;
            let plan = CallPlan {
                provider: provider.clone(),
                model: req.model.clone(),
                reserve_in: input_token_bound(&req),
                // The adapter's true bound: reasoning a vendor does not cap is included.
                reserve_out: adapter.output_bound(&req),
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
                    &Err(e.clone()),
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
        cancel: Option<crate::model_provider::CancelFlag>,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProxyError> {
        let result = {
            let mut ctx = ProviderCtx::new(&p.endpoint, &p.key, self.http.as_ref());
            if let Some(c) = &cancel {
                ctx = ctx.with_cancel(c.clone());
            }
            let gone = || {
                cancel
                    .as_ref()
                    .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
            };
            match p.adapter.complete(&ctx, req, on_text) {
                Err(_) if gone() => Err(ProxyError::Cancelled),
                r => r.map_err(ProxyError::Provider),
            }
        };
        drop(p.key);
        self.finish(
            &p.call_id,
            &p.route,
            &p.endpoint,
            &p.plan,
            &p.conversation,
            &result,
        )?;
        result
    }

    /// Write a call's end record (and run the spike check). `Err` = it could not be written, so
    /// the caller must not return the reply.
    fn finish(
        &self,
        call_id: &str,
        route: &ProxyRoute,
        endpoint: &ResolvedEndpoint,
        plan: &CallPlan,
        conversation: &Option<String>,
        result: &Result<ChatResponse, ProxyError>,
    ) -> Result<(), ProxyError> {
        let now = now_ms();
        let mut inner = self.inner();
        let Inner { guard, log } = &mut *inner;
        let mut r = Self::record_for(Kind::End, call_id, route, endpoint, now);
        r.conversation = conversation.clone();
        let usage = result.as_ref().ok().and_then(|resp| resp.usage);
        if let Some(u) = usage {
            r.tokens_in = Some(u.input_tokens);
            r.tokens_out = Some(u.output_tokens);
            r.cost_micros = plan
                .price
                .map(|pr| pr.cost(u.input_tokens, u.output_tokens));
        }
        r.outcome = Some(match result {
            Ok(_) => "ok".into(),
            Err(e) => e.outcome().into(),
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

    /// Record a live key test (a use of the key). The log must be writable, and the test counts
    /// toward the burst halt. Testing the stored key also respects a halt; setting a new key does
    /// not (replacing a key while paused is how a member recovers).
    fn key_test_logged(
        &self,
        pid: &ProviderId,
        ep: &ResolvedEndpoint,
        adapter: &dyn ModelProvider,
        key: &ApiKey,
        respect_halt: bool,
    ) -> Result<KeyTest, ProxyError> {
        {
            let now = now_ms();
            let mut inner = self.inner();
            let Inner { guard, log } = &mut *inner;
            log.ready().map_err(ProxyError::LogUnavailable)?;
            log.check_clock(now).map_err(ProxyError::LogUnavailable)?;
            if let Err(r) = guard.admit_key_test(pid.as_str(), respect_halt, now) {
                let _ = crate::provider_guard::save(&self.dir, guard);
                return Err(ProxyError::Refused(r));
            }
        }
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
            .key_format()
            .check(&key)
            .map_err(ProxyError::Provider)?;
        let test = self.key_test_logged(&pid, &ep, adapter.as_ref(), &key, false)?;
        self.keyring
            .set(&Self::key_account(&pid, &ep), key.expose_for_keychain())
            .map_err(|_| ProxyError::Storage("the keychain could not be written".into()))?;
        let mut inner = self.inner();
        inner.guard.key_set(pid.as_str(), test.models.clone(), now);
        let _ = crate::provider_guard::save(&self.dir, &mut inner.guard);
        Ok(test)
    }

    /// Test the stored key live.
    pub fn test_key(&self, id: &str) -> Result<KeyTest, ProxyError> {
        let pid = ProviderId::parse(id).ok_or(ProxyError::BadRequest("unknown provider".into()))?;
        let (adapter, _, ep) = self.resolve(&pid)?;
        let key = self.read_key(&pid, &ep)?;
        let t = self.key_test_logged(&pid, &ep, adapter.as_ref(), &key, true)?;
        let mut inner = self.inner();
        inner.guard.key_tested(pid.as_str(), t.models.clone());
        let _ = crate::provider_guard::save(&self.dir, &mut inner.guard);
        Ok(t)
    }

    /// Delete the stored key (and revoke every route to this provider).
    pub fn remove_key(&self, id: &str) -> Result<(), ProxyError> {
        let pid = ProviderId::parse(id).ok_or(ProxyError::BadRequest("unknown provider".into()))?;
        let account = match &pid {
            ProviderId::Vendor(v) => format!("{KEY_ACCOUNT_PREFIX}{}", v.id()),
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
        let _ = crate::provider_guard::save(&self.dir, &mut inner.guard);
        Ok(())
    }

    /// Whether a key is stored (the keychain entry exists). Never reads it into memory beyond the
    /// check.
    pub fn key_configured(&self, id: &ProviderId) -> bool {
        let ep = match id {
            ProviderId::Vendor(v) => crate::provider_egress::resolve_vendor(*v).ok(),
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
        crate::provider_egress::check_custom_host(base_url)
            .map_err(|e| ProxyError::Egress(e.to_string()))?;
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
        crate::provider_guard::save(&self.dir, &mut inner.guard).map_err(ProxyError::Storage)?;
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
        crate::provider_guard::save(&self.dir, &mut inner.guard).map_err(ProxyError::Storage)
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
        crate::provider_guard::save(&self.dir, &mut inner.guard).map_err(ProxyError::Storage)
    }

    // ---- routes ----------------------------------------------------------------------------

    pub fn set_port(&self, port: u16) {
        self.port.store(port, std::sync::atomic::Ordering::SeqCst);
    }

    // Consumed by WP-S1-PROV-AGENT (the sidecar session opener mints routes); used by tests here.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn port(&self) -> u16 {
        self.port.load(std::sync::atomic::Ordering::SeqCst)
    }

    // Consumed by WP-S1-PROV-AGENT (the sidecar session opener mints routes); used by tests here.
    #[cfg_attr(not(test), allow(dead_code))]
    /// Mint a bearer bound to one provider, model and session. In-process only: no command
    /// returns one. The sidecar gets it in its session's `LlmEndpoint`.
    pub fn mint_route(&self, route: ProxyRoute) -> Result<ProxyGrant, ProxyError> {
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

    // Consumed by WP-S1-PROV-AGENT (the sidecar session opener mints routes); used by tests here.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn revoke_session(&self, session_id: &str) {
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        routes.retain(|_, r| r.route.session_id != session_id);
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

    pub(crate) fn save_guard(&self, g: &mut GuardBook) -> Result<(), String> {
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
            ProviderId::Vendor(v) => Some(vendor_adapter(*v).1),
            ProviderId::Custom(_) => Some(AdapterKind::OpenAiCompatible),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    include!("model_proxy_tests.rs");
}
