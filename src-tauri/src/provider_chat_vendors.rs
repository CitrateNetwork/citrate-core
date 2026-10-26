//! D-11 (RC52) — native adapters for the vendors whose own API is chat-completions shaped: xAI,
//! DeepSeek and Meta (Muse models on the Meta Model API). ADR-2026-10-26 P-3.
//!
//! They share the wire format of [`crate::provider_openai_compat`] and differ where it matters:
//!
//! | | xAI | DeepSeek | Meta Model API |
//! |---|---|---|---|
//! | base | `https://api.x.ai/v1` | `https://api.deepseek.com/v1` (the docs show the bare host) | `https://api.meta.ai/v1` |
//! | output cap field | `max_completion_tokens` (`max_tokens` deprecated) | `max_tokens` | `max_completion_tokens` (`max_tokens` a deprecated alias) |
//! | reasoning tokens | NOT in `completion_tokens` (added here) | in `completion_tokens` | in `completion_tokens` |
//! | streamed usage | running total on every chunk | on the last chunk | final chunk with `include_usage` |
//! | tool-call deltas | one whole call per chunk | OpenAI-style pieces | OpenAI-style pieces, by `index` |
//! | key | `xai-…` | `sk-…` | `LLM_<id>_<secret>` |
//!
//! DeepSeek's thinking mode is on by default and, when a request carries `tools`, it requires
//! every earlier turn's `reasoning_content` back or answers 400. The internal format does not
//! carry reasoning, so a DeepSeek request with tools turns thinking off
//! (`thinking: {type: disabled}`); a request without tools keeps the vendor default.
//!
//! Vendor sources (fetched 2026-10-26):
//! - xAI: <https://docs.x.ai/developers/rest-api-reference/inference/chat-completions>,
//!   <https://docs.x.ai/developers/model-capabilities/text/streaming>,
//!   <https://docs.x.ai/developers/tools/function-calling>,
//!   <https://docs.x.ai/developers/rest-api-reference/inference/models>,
//!   <https://docs.x.ai/developers/debugging>, prices <https://docs.x.ai/developers/pricing>.
//!   xAI labels Chat Completions "legacy" (no end date) and recommends `/v1/responses`; its
//!   Responses streaming events are not documented, so this adapter stays on the documented
//!   Chat Completions wire.
//! - DeepSeek: <https://api-docs.deepseek.com/api/create-chat-completion>,
//!   <https://api-docs.deepseek.com/guides/thinking_mode>, <https://api-docs.deepseek.com/guides/tool_calls>,
//!   <https://api-docs.deepseek.com/api/list-models>, <https://api-docs.deepseek.com/quick_start/error_codes>,
//!   <https://api-docs.deepseek.com/quick_start/rate_limit> (`: keep-alive` SSE comments, skipped
//!   by the SSE reader), prices <https://api-docs.deepseek.com/quick_start/pricing>.
//! - Meta: <https://dev.meta.ai/docs/overview>, <https://dev.meta.ai/docs/authentication>,
//!   <https://dev.meta.ai/docs/protocols/chat-completions>,
//!   <https://dev.meta.ai/docs/api-reference/chat-completions/schemas>,
//!   <https://dev.meta.ai/docs/tool-calling>, <https://dev.meta.ai/docs/models>,
//!   <https://dev.meta.ai/docs/error-handling>, prices <https://dev.meta.ai/docs/pricing-rate-limits>.

use crate::model_provider::{
    status_error, AuthPlacement, ChatRequest, ChatResponse, KeyCharset, KeyFormat, KeyTest, Method,
    ModelInfo, ModelProvider, OutboundRequest, ProviderCtx, ProviderError, ProviderId, Usage,
    Vendor,
};
use crate::provider_openai_compat::{
    listed_models, parse_response_with, read_stream_with, to_wire as compat_wire, usage_from,
    UsageFn,
};
use serde_json::{json, Value};

const MTOK: u64 = 1_000_000;

/// A chat-completions vendor's differences.
#[derive(Debug, Clone, Copy)]
struct Dialect {
    vendor: Vendor,
    key_prefixes: &'static [&'static str],
    key_charset: KeyCharset,
    /// `max_completion_tokens` or `max_tokens`.
    max_field: &'static str,
    usage: UsageFn,
}

/// xAI: `completion_tokens` excludes reasoning tokens (documented example: 32 prompt + 9
/// completion, `reasoning_tokens` 94, `total_tokens` 135), so they are added back: the vendor
/// bills them as output.
pub fn xai_usage(v: &Value) -> Option<Usage> {
    let base = usage_from(v)?;
    let reasoning = match v.pointer("/usage/completion_tokens_details/reasoning_tokens") {
        None | Some(Value::Null) => 0,
        Some(r) => r.as_u64()?,
    };
    Some(Usage {
        input_tokens: base.input_tokens,
        output_tokens: base.output_tokens.saturating_add(reasoning),
    })
}

const XAI: Dialect = Dialect {
    vendor: Vendor::Xai,
    key_prefixes: &["xai-"],
    key_charset: KeyCharset::UrlSafe,
    max_field: "max_completion_tokens",
    usage: xai_usage,
};

const DEEPSEEK: Dialect = Dialect {
    vendor: Vendor::DeepSeek,
    key_prefixes: &["sk-"],
    key_charset: KeyCharset::UrlSafe,
    max_field: "max_tokens",
    usage: usage_from,
};

const META: Dialect = Dialect {
    vendor: Vendor::Meta,
    key_prefixes: &["LLM_"],
    key_charset: KeyCharset::Printable,
    max_field: "max_completion_tokens",
    usage: usage_from,
};

/// The models each vendor serves for chat, with published prices (micro-USD per million tokens,
/// sources in the module docs, 2026-10-26). Where a price rises with prompt size (xAI at 200K
/// prompt tokens) or by time of day (DeepSeek peak hours), the higher price is used so the spend
/// guard never under-counts; cached input is metered at the full input rate.
pub fn catalog(v: Vendor) -> Vec<ModelInfo> {
    let m = |id: &str, name: &str, input: u64, output: u64| ModelInfo {
        id: id.into(),
        display_name: name.into(),
        supports_tools: true,
        input_micros_per_mtok: Some(input),
        output_micros_per_mtok: Some(output),
    };
    match v {
        Vendor::Xai => vec![
            m("grok-4.7", "Grok 4.7", 4 * MTOK, 12 * MTOK),
            m("grok-4.6", "Grok 4.6", 4 * MTOK, 12 * MTOK),
            m("grok-4.5", "Grok 4.5", 4 * MTOK, 12 * MTOK),
            m("grok-4.3", "Grok 4.3", 5 * MTOK / 2, 5 * MTOK),
            m(
                "grok-4.20-0309-reasoning",
                "Grok 4.20 Reasoning",
                5 * MTOK / 2,
                5 * MTOK,
            ),
            m(
                "grok-4.20-0309-non-reasoning",
                "Grok 4.20 Non-reasoning",
                5 * MTOK / 2,
                5 * MTOK,
            ),
        ],
        Vendor::DeepSeek => vec![
            m(
                "deepseek-flash",
                "DeepSeek Flash",
                3 * MTOK / 10,
                6 * MTOK / 5,
            ),
            m(
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                33 * MTOK / 25,
                99 * MTOK / 25,
            ),
        ],
        // Contributor-mode Spark models are left out of the known list: they are a separate,
        // much cheaper mode whose terms the member should pick deliberately (they can still be
        // approved, or listed by the key test).
        Vendor::Meta => vec![
            m("muse-wonder-1.0", "Muse Wonder 1.0", 4 * MTOK, 20 * MTOK),
            m(
                "muse-spark-1.3",
                "Muse Spark 1.3",
                5 * MTOK / 4,
                17 * MTOK / 4,
            ),
            m(
                "muse-spark-1.2",
                "Muse Spark 1.2",
                5 * MTOK / 4,
                17 * MTOK / 4,
            ),
            m(
                "muse-spark-1.1",
                "Muse Spark 1.1",
                5 * MTOK / 4,
                17 * MTOK / 4,
            ),
        ],
        Vendor::OpenAi | Vendor::Anthropic | Vendor::Gemini => vec![],
    }
}

/// A native adapter for one of the chat-completions vendors.
pub struct ChatVendorAdapter {
    d: Dialect,
}

impl ChatVendorAdapter {
    pub fn xai() -> ChatVendorAdapter {
        ChatVendorAdapter { d: XAI }
    }

    pub fn deepseek() -> ChatVendorAdapter {
        ChatVendorAdapter { d: DEEPSEEK }
    }

    pub fn meta() -> ChatVendorAdapter {
        ChatVendorAdapter { d: META }
    }

    /// The request body in this vendor's dialect.
    pub fn to_wire(&self, req: &ChatRequest) -> Value {
        let mut body = compat_wire(req);
        if let Some(o) = body.as_object_mut() {
            if self.d.max_field != "max_tokens" {
                o.remove("max_tokens");
                o.insert(self.d.max_field.into(), json!(req.max_tokens));
            }
            if self.d.vendor == Vendor::DeepSeek && !req.tools.is_empty() {
                o.insert("thinking".into(), json!({"type": "disabled"}));
            }
        }
        body
    }
}

impl ModelProvider for ChatVendorAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(self.d.vendor)
    }

    fn models(&self) -> Vec<ModelInfo> {
        catalog(self.d.vendor)
    }

    fn output_bound(&self, req: &ChatRequest) -> u64 {
        // RED STUB: the old behaviour (the visible-output cap).
        u64::from(req.max_tokens)
    }

    fn key_format(&self) -> KeyFormat {
        KeyFormat {
            prefixes: self.d.key_prefixes,
            charset: self.d.key_charset,
            ..KeyFormat::ANY
        }
    }

    fn test_key(&self, ctx: &ProviderCtx<'_>) -> Result<KeyTest, ProviderError> {
        let resp = ctx.send(OutboundRequest {
            method: Method::Get,
            path: "/models".into(),
            headers: vec![("accept", "application/json".into())],
            auth: AuthPlacement::Bearer,
            body: None,
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(status_error(resp.status));
        }
        Ok(KeyTest {
            models: listed_models(&resp.json()?),
        })
    }

    fn complete(
        &self,
        ctx: &ProviderCtx<'_>,
        req: &ChatRequest,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
        let body = self.to_wire(req);
        let resp = ctx.send(OutboundRequest {
            method: Method::Post,
            path: "/chat/completions".into(),
            headers: vec![(
                "accept",
                if req.stream {
                    "text/event-stream".into()
                } else {
                    "application/json".into()
                },
            )],
            auth: AuthPlacement::Bearer,
            body: Some(body),
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(status_error(resp.status));
        }
        if req.stream && resp.content_type.contains("text/event-stream") {
            read_stream_with(resp.body, on_text, self.d.usage)
        } else {
            parse_response_with(&resp.json()?, self.d.usage)
        }
    }
}

#[cfg(test)]
mod tests {
    include!("provider_chat_vendors_tests.rs");
}
