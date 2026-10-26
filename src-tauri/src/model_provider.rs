//! D-11 (RC52) — the model provider trait and the internal format every cloud adapter maps to.
//! ADR-2026-10-26-byok-provider-governance P-3, P-6, P-9b.
//!
//! One Rust trait, [`ModelProvider`], sits behind the core model proxy ([`crate::model_proxy`]).
//! Each native adapter (OpenAI, Anthropic Messages, Gemini, xAI, DeepSeek, Meta Model API, and the
//! generic OpenAI-compatible adapter used for a member-confirmed custom endpoint) implements it.
//!
//! ## What an adapter can and cannot do
//!
//! - An adapter **never sees the key**. It builds an [`OutboundRequest`] (a path under the resolved
//!   endpoint, headers, a JSON body and an [`AuthPlacement`] saying where the key goes) and hands
//!   it to [`ProviderCtx::send`]. Core composes the URL, attaches the key and sends it through the
//!   egress-checked transport. An adapter cannot name a host: the path is joined to the pinned base
//!   URL and the joined URL must still be on the pinned host.
//! - The key is an [`ApiKey`]: wiped on drop, no `Serialize`, no `Deserialize`, no `Debug`, no
//!   `Clone`. It can only be read inside this module (the transport header) and only be made from a
//!   keychain read or the member's one-way key entry.
//! - Errors are coarse ([`ProviderError`]): they never carry a URL, a header, a key or a body.
//!
//! ## Internal format
//!
//! [`ChatRequest`] / [`ChatResponse`] mirror the OpenAI chat-completions shape the sidecar already
//! speaks (`citrate-agent-loop`'s `CompletionRequest` / `AssistantTurn`): system/user/assistant/tool
//! messages, tool definitions with a JSON-schema `parameters`, tool calls whose `arguments` is JSON
//! text, and the provider's own token usage (`None` when the vendor did not report it, never zero).
//! Streaming hands each piece of assistant text to `on_text` as it arrives; the returned
//! [`ChatResponse`] is always the whole turn, tool calls included.

use crate::secret::SecretString;
use serde_json::Value;
use std::io::Read;

// ---------------------------------------------------------------------------
// Provider identity
// ---------------------------------------------------------------------------

/// The six governed cloud vendors (ADR P-3). Cognition is a remote-agent delegate, not a chat
/// model, and is listed in [`crate::provider_egress::DELEGATES`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Vendor {
    OpenAi,
    Anthropic,
    Gemini,
    Xai,
    DeepSeek,
    Meta,
}

impl Vendor {
    pub const ALL: [Vendor; 6] = [
        Vendor::OpenAi,
        Vendor::Anthropic,
        Vendor::Gemini,
        Vendor::Xai,
        Vendor::DeepSeek,
        Vendor::Meta,
    ];

    /// The stable wire id (`openai`, `anthropic`, `gemini`, `xai`, `deepseek`, `meta`).
    pub fn id(self) -> &'static str {
        match self {
            Vendor::OpenAi => "openai",
            Vendor::Anthropic => "anthropic",
            Vendor::Gemini => "gemini",
            Vendor::Xai => "xai",
            Vendor::DeepSeek => "deepseek",
            Vendor::Meta => "meta",
        }
    }

    pub fn from_id(id: &str) -> Option<Vendor> {
        Vendor::ALL.into_iter().find(|v| v.id() == id)
    }
}

/// Which provider a call goes to: a named vendor (pinned host) or a member-added custom endpoint
/// (`custom-<slug>`, shown as unverified everywhere).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProviderId {
    Vendor(Vendor),
    Custom(String),
}

/// Prefix of every custom endpoint's provider id.
pub const CUSTOM_PREFIX: &str = "custom-";

impl ProviderId {
    /// Parse a wire id. A custom id is `custom-` followed by 1..=40 of `[a-z0-9-]`.
    pub fn parse(id: &str) -> Option<ProviderId> {
        if let Some(v) = Vendor::from_id(id) {
            return Some(ProviderId::Vendor(v));
        }
        let slug = id.strip_prefix(CUSTOM_PREFIX)?;
        let ok = !slug.is_empty()
            && slug.len() <= 40
            && slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        ok.then(|| ProviderId::Custom(id.to_string()))
    }

    pub fn as_str(&self) -> &str {
        match self {
            ProviderId::Vendor(v) => v.id(),
            ProviderId::Custom(s) => s,
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, ProviderId::Custom(_))
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// The key (ADR P-9b): the type is the enforcement
// ---------------------------------------------------------------------------

/// A vendor API key, held by the core model proxy for one call.
///
/// Deliberately has **no** `Serialize`, `Deserialize`, `Debug`, `Display` or `Clone` (pinned at
/// compile time by `model_provider_tests::api_key_has_no_leaky_traits`). Any struct that holds one
/// therefore cannot be serialized across `invoke`, written into a sidecar body or a log line, or
/// printed. The bytes are wiped on drop. Only this module reads them, to set the auth header.
pub struct ApiKey(SecretString);

impl ApiKey {
    /// The member's one-way key entry (a `SecretString` command argument) or a keychain read.
    pub(crate) fn from_secret(s: SecretString) -> ApiKey {
        ApiKey(SecretString::new(s.expose().trim().to_string()))
    }

    // Every accessor below is private to this module (pinned by
    // `api_key_has_no_public_accessor_beyond_construction_and_the_keychain_write`): code holding
    // `&ApiKey` elsewhere cannot read it, not even piecewise. Formats are checked by `KeyFormat`.

    fn len(&self) -> usize {
        self.0.expose().len()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.0.expose().starts_with(prefix)
    }

    /// A non-capturing predicate only: it can test bytes but cannot collect them.
    fn all_bytes(&self, f: fn(u8) -> bool) -> bool {
        self.0.expose().bytes().all(f)
    }

    /// Whether `haystack` contains the key (the transport refuses a URL, header or body that
    /// carries it). An empty key matches nothing.
    fn appears_in(&self, haystack: &[u8]) -> bool {
        let k = self.0.expose().as_bytes();
        !k.is_empty() && haystack.windows(k.len()).any(|w| w == k)
    }

    /// The bytes, for the transport's auth header only ([`ProviderCtx::send`]).
    fn expose_for_transport(&self) -> &str {
        self.0.expose()
    }

    /// The bytes, for the keychain write in `model_proxy::ModelProxy::set_key` only (pinned by
    /// the `key_bytes_are_read_in_exactly_two_places` tripwire).
    pub(crate) fn expose_for_keychain(&self) -> &str {
        self.0.expose()
    }
}

/// The characters a vendor's keys are made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCharset {
    /// `A-Z a-z 0-9 - _` (OpenAI, Anthropic, xAI, DeepSeek).
    UrlSafe,
    // No governed vendor documents a dotted key today; kept for one that does, tested here.
    /// `A-Z a-z 0-9 - _ .`
    #[cfg_attr(not(test), allow(dead_code))]
    UrlSafeDot,
    /// Any printable ASCII except whitespace (custom endpoints).
    Printable,
}

/// A vendor's key format, as data: core evaluates it, so an adapter never holds the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyFormat {
    /// The key must start with one of these (empty = any).
    pub prefixes: &'static [&'static str],
    pub min_len: usize,
    pub max_len: usize,
    pub charset: KeyCharset,
}

impl KeyFormat {
    /// What a custom endpoint accepts: 20..=512 printable ASCII bytes, any prefix.
    pub const ANY: KeyFormat = KeyFormat {
        prefixes: &[],
        min_len: 20,
        max_len: 512,
        charset: KeyCharset::Printable,
    };

    /// Check `key` against this format, inside core. Nothing is sent and nothing is stored when
    /// it fails.
    pub fn check(&self, key: &ApiKey) -> Result<(), ProviderError> {
        let charset_ok = key.all_bytes(match self.charset {
            KeyCharset::UrlSafe => |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_',
            KeyCharset::UrlSafeDot => {
                |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.'
            }
            KeyCharset::Printable => |b: u8| b.is_ascii_graphic(),
        });
        if key.len() < self.min_len.max(1) || key.len() > self.max_len.min(512) || !charset_ok {
            return Err(ProviderError::KeyFormat(
                "that does not look like an API key for this provider (check you copied all of it)",
            ));
        }
        if !self.prefixes.is_empty() && !self.prefixes.iter().any(|p| key.starts_with(p)) {
            return Err(ProviderError::KeyFormat(
                "that key does not have this provider's prefix (check you picked the right provider)",
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The internal request / response format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "system" | "developer" => Some(Role::System),
            "user" => Some(Role::User),
            "assistant" => Some(Role::Assistant),
            "tool" => Some(Role::Tool),
            _ => None,
        }
    }
}

/// A tool call the model asked for. `arguments` is JSON text (an object), exactly as the sidecar's
/// `ToolCall` carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// One message of the conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    /// Assistant messages only: the tool calls that turn made.
    pub tool_calls: Vec<ToolCall>,
    /// Tool messages only: which call this is the result of.
    pub tool_call_id: Option<String>,
}

/// A tool the model may call. `parameters` is a JSON-schema object.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// One model call in the internal format.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    /// The vendor's model id (as listed by [`ModelProvider::models`]).
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    /// The output ceiling for this call. The spend guard reserves against it.
    pub max_tokens: u32,
    /// Whether the caller wants text deltas as they arrive.
    pub stream: bool,
}

/// Token usage as the vendor reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Other,
}

impl FinishReason {
    /// The OpenAI `finish_reason` spelling the proxy answers the sidecar with.
    pub fn as_openai(self) -> &'static str {
        match self {
            FinishReason::Stop | FinishReason::Other => "stop",
            FinishReason::ToolCalls => "tool_calls",
            FinishReason::Length => "length",
            FinishReason::ContentFilter => "content_filter",
        }
    }
}

/// The whole assistant turn.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    /// `None` when the vendor did not report usage (the guard then charges the reservation).
    pub usage: Option<Usage>,
    pub finish: FinishReason,
}

/// A model a provider serves. Prices are micro-USD per million tokens and are `None` unless a
/// source for them exists (the vendor's published price list, or the member's own entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub supports_tools: bool,
    pub input_micros_per_mtok: Option<u64>,
    pub output_micros_per_mtok: Option<u64>,
}

/// The result of a live key test.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyTest {
    /// Model ids the vendor listed for this key (bounded by core to 500 ids of <= 200 bytes).
    pub models: Vec<String>,
}

// ---------------------------------------------------------------------------
// Errors (coarse, secret-free)
// ---------------------------------------------------------------------------

/// An adapter or transport failure. No variant carries a URL, header, key or body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// Could not reach the vendor (connect, TLS, timeout, cut-off stream).
    Transport(&'static str),
    /// The vendor answered with a non-success status.
    Status(u16),
    /// 401 / 403: the vendor rejected the key.
    Unauthorized,
    /// 429: the vendor's own rate limit.
    RateLimited,
    /// A 3xx answer. Redirects are never followed (ADR: no redirect may leave the pinned host).
    Redirect,
    /// The request would leave the pinned host, or the endpoint is not allowed.
    Egress(&'static str),
    /// The vendor's answer could not be read in the expected shape.
    BadResponse(&'static str),
    /// The key failed the vendor's format check (nothing was sent).
    KeyFormat(&'static str),
    /// The request cannot be put in the vendor's format (nothing was sent), such as a tool call in
    /// the history whose arguments are not a JSON object, or a model id unsafe in a URL path.
    BadRequest(&'static str),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Transport(m) => write!(f, "could not reach the provider: {m}"),
            ProviderError::Status(s) => write!(f, "the provider returned HTTP {s}"),
            ProviderError::Unauthorized => f.write_str("the provider rejected the key"),
            ProviderError::RateLimited => f.write_str("the provider's rate limit was reached"),
            ProviderError::Redirect => {
                f.write_str("the provider answered with a redirect, which is never followed")
            }
            ProviderError::Egress(m) => write!(f, "blocked: {m}"),
            ProviderError::BadResponse(m) => write!(f, "the provider's answer was unusable: {m}"),
            ProviderError::KeyFormat(m) => f.write_str(m),
            ProviderError::BadRequest(m) => write!(f, "the request could not be sent: {m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Map an HTTP status to the coarse error (non-2xx only).
pub fn status_error(status: u16) -> ProviderError {
    match status {
        300..=399 => ProviderError::Redirect,
        401 | 403 => ProviderError::Unauthorized,
        429 => ProviderError::RateLimited,
        s => ProviderError::Status(s),
    }
}

// ---------------------------------------------------------------------------
// The outbound request an adapter builds, and the transport seam
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// Where the transport puts the key. Never in a URL or query string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthPlacement {
    /// `Authorization: Bearer <key>` (OpenAI, xAI, DeepSeek, Meta, OpenAI-compatible).
    Bearer,
    /// A named header carrying the bare key (`x-api-key` for Anthropic, `x-goog-api-key` for
    /// Gemini).
    Header(&'static str),
}

/// What an adapter asks core to send. `path` is joined to the resolved endpoint's base URL (it
/// starts with `/` and may carry a query such as `?alt=sse`, never the key).
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundRequest {
    pub method: Method,
    pub path: String,
    /// Extra non-secret headers (`anthropic-version`, `accept`). `authorization`, the auth header
    /// name, `host`, `cookie` and proxy headers are refused.
    pub headers: Vec<(&'static str, String)>,
    pub auth: AuthPlacement,
    pub body: Option<Value>,
}

/// The vendor's answer: status, content type and a body reader (streamed answers are read line by
/// line as they arrive).
pub struct InboundResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Box<dyn Read + Send>,
}

impl std::fmt::Debug for InboundResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboundResponse")
            .field("status", &self.status)
            .field("content_type", &self.content_type)
            .finish_non_exhaustive()
    }
}

impl InboundResponse {
    /// Read the whole body as text (bounded at 8 MiB).
    pub fn text(self) -> Result<String, ProviderError> {
        let mut s = String::new();
        self.body
            .take(8 * 1024 * 1024)
            .read_to_string(&mut s)
            .map_err(|_| ProviderError::Transport("could not read the answer"))?;
        Ok(s)
    }

    /// Read the whole body as JSON.
    pub fn json(self) -> Result<Value, ProviderError> {
        let t = self.text()?;
        serde_json::from_str(&t).map_err(|_| ProviderError::BadResponse("not JSON"))
    }
}

/// A fully composed request, as the transport sees it. Built only by [`ProviderCtx::send`].
pub struct WireRequest<'a> {
    pub method: Method,
    pub url: url::Url,
    pub headers: Vec<(String, String)>,
    /// `(header name, value)` for the key: `("authorization", "Bearer …")` or `("x-api-key", …)`.
    pub auth_header: (&'static str, zeroize::Zeroizing<String>),
    pub body: Option<Vec<u8>>,
    /// The endpoint the URL must stay on (the transport re-checks it).
    pub endpoint: &'a crate::provider_egress::ResolvedEndpoint,
}

/// The HTTP transport seam. Production: [`crate::provider_egress::PinnedHttp`] (https only, no
/// redirects, proxy environment ignored, pinned host re-checked). Tests: recorded fixtures.
pub trait ProviderHttp: Send + Sync {
    fn send(&self, req: WireRequest<'_>) -> Result<InboundResponse, ProviderError>;
}

/// What an adapter gets for one call: the resolved endpoint, and a `send` that attaches the key
/// and enforces egress. The key itself is not reachable from here.
pub struct ProviderCtx<'a> {
    endpoint: &'a crate::provider_egress::ResolvedEndpoint,
    key: &'a ApiKey,
    http: &'a dyn ProviderHttp,
    cancel: Option<CancelFlag>,
}

/// Set by core when the caller of a call has gone away (the loopback client disconnected).
pub type CancelFlag = std::sync::Arc<std::sync::atomic::AtomicBool>;

/// A vendor answer body that fails its next read once the call is cancelled.
struct CancelOnFlag {
    inner: Box<dyn Read + Send>,
    flag: CancelFlag,
}

impl Read for CancelOnFlag {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.flag.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "the call was cancelled",
            ));
        }
        self.inner.read(buf)
    }
}

impl<'a> ProviderCtx<'a> {
    pub(crate) fn new(
        endpoint: &'a crate::provider_egress::ResolvedEndpoint,
        key: &'a ApiKey,
        http: &'a dyn ProviderHttp,
    ) -> Self {
        ProviderCtx {
            endpoint,
            key,
            http,
            cancel: None,
        }
    }

    /// Stop this call when `flag` is set: nothing more is sent, and the vendor's answer body
    /// fails at its next read, so every adapter's read loop ends and the vendor connection is
    /// dropped (which stops the vendor generating). Bytes already buffered may still be parsed;
    /// they are not forwarded by a caller that has gone.
    pub(crate) fn with_cancel(mut self, flag: CancelFlag) -> Self {
        self.cancel = Some(flag);
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|f| f.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// Compose, check and send. Fails closed (nothing sent) when the joined URL leaves the pinned
    /// host, carries the key, or a header is not allowed.
    pub fn send(&self, req: OutboundRequest) -> Result<InboundResponse, ProviderError> {
        if self.cancelled() {
            return Err(ProviderError::Transport("the call was cancelled"));
        }
        let url = crate::provider_egress::join_checked(self.endpoint, &req.path)?;
        if self.key.appears_in(url.as_str().as_bytes()) {
            return Err(ProviderError::Egress("a key may never travel in a URL"));
        }
        let mut headers = Vec::with_capacity(req.headers.len());
        for (name, value) in req.headers {
            crate::provider_egress::check_extra_header(name, &value)?;
            if self.key.appears_in(value.as_bytes()) {
                return Err(ProviderError::Egress(
                    "a key may only travel in its auth header",
                ));
            }
            headers.push((name.to_string(), value));
        }
        let body = match &req.body {
            Some(v) => {
                let b = serde_json::to_vec(v)
                    .map_err(|_| ProviderError::BadResponse("request body"))?;
                if self.key.appears_in(&b) {
                    return Err(ProviderError::Egress(
                        "a key may only travel in its auth header",
                    ));
                }
                Some(b)
            }
            None => None,
        };
        let auth_header = match req.auth {
            AuthPlacement::Bearer => (
                "authorization",
                zeroize::Zeroizing::new(format!("Bearer {}", self.key.expose_for_transport())),
            ),
            AuthPlacement::Header(name) => {
                crate::provider_egress::check_auth_header_name(name)?;
                (
                    name,
                    zeroize::Zeroizing::new(self.key.expose_for_transport().to_string()),
                )
            }
        };
        let mut resp = self.http.send(WireRequest {
            method: req.method,
            url,
            headers,
            auth_header,
            body,
            endpoint: self.endpoint,
        })?;
        if let Some(flag) = &self.cancel {
            resp.body = Box::new(CancelOnFlag {
                inner: resp.body,
                flag: flag.clone(),
            });
        }
        Ok(resp)
    }
}

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

/// A native cloud model adapter (ADR P-3). Stateless: everything per call arrives in the context.
pub trait ModelProvider: Send + Sync {
    /// Which provider this adapter serves.
    fn id(&self) -> ProviderId;

    /// The models this adapter knows. A call for a model outside this list (and outside the
    /// member's approved extras) trips the anomaly halt.
    fn models(&self) -> Vec<ModelInfo>;

    /// The vendor's key format, as data. Core checks it before the key is stored or sent; the
    /// adapter never receives the key.
    fn key_format(&self) -> KeyFormat;

    /// The most output tokens (reasoning included) the vendor can bill for `req`. The spend guard
    /// reserves against it before the call, so it must be a true upper bound: `req.max_tokens`
    /// only where the vendor documents that its output cap covers reasoning.
    fn output_bound(&self, req: &ChatRequest) -> u64;

    /// A live test with a free endpoint (such as the model list). A key that fails is not stored.
    /// Returns the model ids the vendor listed for this key (empty when the endpoint lists none);
    /// core keeps them as known models for the anomaly halt.
    fn test_key(&self, ctx: &ProviderCtx<'_>) -> Result<KeyTest, ProviderError>;

    /// One model call. When `req.stream` is set, hand each piece of assistant text to `on_text` as
    /// it arrives. Always return the whole turn (tool calls included) and the vendor's usage.
    fn complete(
        &self,
        ctx: &ProviderCtx<'_>,
        req: &ChatRequest,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError>;
}

// ---------------------------------------------------------------------------
// Shared helpers for adapters
// ---------------------------------------------------------------------------

/// Read a server-sent-event stream line by line, calling `on_event(event_name, data)` for every
/// `data:` line (`event_name` is the last `event:` field seen, empty if none). Return `Ok(false)`
/// from the callback to stop early. Lines over 1 MiB are an error.
pub fn read_sse(
    body: impl Read,
    on_event: &mut dyn FnMut(&str, &str) -> Result<bool, ProviderError>,
) -> Result<(), ProviderError> {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(body);
    let mut line = String::new();
    let mut event = String::new();
    loop {
        line.clear();
        let n = (&mut reader)
            .take(1024 * 1024 + 1)
            .read_line(&mut line)
            .map_err(|_| ProviderError::Transport("the answer stream was cut off"))?;
        if n == 0 {
            return Ok(());
        }
        if n > 1024 * 1024 {
            return Err(ProviderError::BadResponse("a stream line is too long"));
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            event.clear();
            continue;
        }
        if let Some(e) = l.strip_prefix("event:") {
            event = e.trim().to_string();
            continue;
        }
        if let Some(d) = l.strip_prefix("data:") {
            if !on_event(&event, d.trim_start())? {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    include!("model_provider_tests.rs");
}
