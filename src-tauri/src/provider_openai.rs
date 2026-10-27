//! D-11 (RC52) — the native OpenAI adapter (Responses API). ADR-2026-10-26 P-3.
//!
//! Vendor sources (fetched 2026-10-26):
//! - Create a response: <https://developers.openai.com/api/reference/resources/responses/methods/create>
//! - Streaming events: <https://developers.openai.com/api/docs/guides/streaming-responses>
//! - Migrating from Chat Completions: <https://developers.openai.com/api/docs/guides/migrate-to-responses>
//! - GPT-6 guide (tool calling needs Responses):
//!   <https://developers.openai.com/api/docs/guides/latest-model/gpt-6-astra>
//! - Function calling (strict default): <https://developers.openai.com/api/docs/guides/function-calling>
//! - Models list (key test): <https://developers.openai.com/api/reference/resources/models/methods/list>
//! - Errors: <https://developers.openai.com/api/docs/guides/error-codes>
//! - Prices: <https://developers.openai.com/api/docs/pricing>
//!
//! Why Responses and not Chat Completions: "GPT-6 Astra, GPT-6.1 Sol, and GPT-6.1 Terra support
//! Chat Completions, but tool calling requires Responses", and since GPT-5.4 Chat Completions has
//! no tool calling at a reasoning effort other than `none`. Hermes needs tools.
//!
//! Wire facts this adapter relies on:
//! - `POST /v1/responses` with `Authorization: Bearer`; `store: false` (nothing kept at the
//!   vendor); `max_output_tokens` (covers reasoning and visible output); `service_tier: default`
//!   (standard pricing), and a reply served at another tier is refused.
//! - `input` items: `{role, content}` messages (`developer` for system text, which keeps its
//!   place in the transcript), `{type: function_call, call_id, name, arguments}` for a replayed
//!   call and `{type: function_call_output, call_id, output}` for its result.
//! - Tools are flat `{type: function, name, description, parameters, strict: false}`; without
//!   `strict` the API rewrites the schema into strict mode when it can, which changes optional
//!   fields, so non-strict is sent explicitly (the sidecar's tools are best-effort today).
//! - Output items: `message` (`output_text` / `refusal` parts) and `function_call`; `status`
//!   `completed` | `incomplete` (`incomplete_details.reason`: `max_output_tokens`,
//!   `content_filter`, …).
//! - Stream: typed SSE events. Text arrives in `response.output_text.delta`, each finished call in
//!   `response.output_item.done`, and the final usage in `response.completed` /
//!   `response.incomplete`, which are also the end markers. `response.failed` and `error` end the
//!   call with an error.
//! - Usage: `usage.input_tokens` / `usage.output_tokens` (output includes reasoning tokens).

use crate::model_provider::{
    read_sse, status_error, AuthPlacement, ChatRequest, ChatResponse, FinishReason, KeyCharset,
    KeyFormat, KeyTest, Method, ModelInfo, ModelProvider, OutboundRequest, ProviderCtx,
    ProviderError, ProviderId, Role, ToolCall, Usage, Vendor,
};
use crate::provider_openai_compat::listed_models;
use serde_json::{json, Map, Value};

/// OpenAI documents no key format; its keys have always started `sk-` (`sk-proj-` today).
pub const KEY_PREFIX: &str = "sk-";

const MTOK: u64 = 1_000_000;

/// Current models and their published Standard-tier prices (micro-USD per million tokens), from
/// <https://developers.openai.com/api/docs/pricing> (2026-10-26). The long-context rate (prompts
/// over 272K tokens: 2x input, 1.5x output) is used so the spend guard never under-counts.
pub fn catalog() -> Vec<ModelInfo> {
    let m = |id: &str, name: &str, input: u64, output: u64| ModelInfo {
        id: id.into(),
        display_name: name.into(),
        supports_tools: true,
        input_micros_per_mtok: Some(input),
        output_micros_per_mtok: Some(output),
    };
    vec![
        m("gpt-6-astra", "GPT-6 Astra", 20 * MTOK, 75 * MTOK),
        m("gpt-6.1-sol", "GPT-6.1 Sol", 4 * MTOK, 15 * MTOK),
        m("gpt-6.1-terra", "GPT-6.1 Terra", 2 * MTOK, 15 * MTOK / 2),
        m("gpt-6-sol", "GPT-6 Sol", 4 * MTOK, 15 * MTOK),
        m("gpt-6-luna", "GPT-6 Luna", MTOK / 5, 3 * MTOK / 4),
    ]
}

pub struct OpenAiAdapter;

/// The tier every call asks for ("processed with the standard pricing and performance").
pub const STANDARD_TIER: &str = "default";

/// The reply names the tier that served it; any other tier is billed off the catalog price, so
/// the call fails closed (the guard then charges the reservation).
fn check_tier(response: &Value) -> Result<(), ProviderError> {
    match response.get("service_tier").and_then(Value::as_str) {
        None => Ok(()),
        Some(t) if t == STANDARD_TIER => Ok(()),
        Some(_) => Err(ProviderError::BadResponse(
            "the provider served the call outside the standard tier",
        )),
    }
}

/// The Responses body for a request.
pub fn to_wire(req: &ChatRequest) -> Result<Value, ProviderError> {
    let mut input = Vec::with_capacity(req.messages.len());
    for m in &req.messages {
        match m.role {
            Role::System => {
                if !m.content.is_empty() {
                    input.push(json!({"role": "developer", "content": m.content}));
                }
            }
            Role::User => {
                if !m.content.is_empty() {
                    input.push(json!({"role": "user", "content": m.content}));
                }
            }
            Role::Assistant => {
                if !m.content.is_empty() {
                    input.push(json!({"role": "assistant", "content": m.content}));
                }
                for c in &m.tool_calls {
                    input.push(json!({"type": "function_call", "call_id": c.id,
                        "name": c.name, "arguments": if c.arguments.is_empty() {
                            "{}" } else { c.arguments.as_str() }}));
                }
            }
            Role::Tool => {
                let id = m.tool_call_id.as_deref().filter(|s| !s.is_empty()).ok_or(
                    ProviderError::BadRequest("a tool result does not name its tool call"),
                )?;
                input.push(json!({"type": "function_call_output", "call_id": id,
                    "output": m.content}));
            }
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("input".into(), Value::Array(input));
    body.insert("max_output_tokens".into(), json!(req.max_tokens));
    body.insert("store".into(), json!(false));
    // The standard tier, explicitly: without it a project whose default is Fast is billed above
    // the catalog price ("When not set, the default behavior is 'auto'").
    body.insert("service_tier".into(), json!(STANDARD_TIER));
    if !req.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({"type": "function", "name": t.name, "description": t.description,
                            "parameters": t.parameters, "strict": false})
                    })
                    .collect(),
            ),
        );
    }
    if req.stream {
        body.insert("stream".into(), json!(true));
    }
    Ok(Value::Object(body))
}

/// `usage.input_tokens` / `usage.output_tokens`, both present, else unknown.
pub fn usage_from(response: &Value) -> Option<Usage> {
    let u = response.get("usage")?;
    Some(Usage {
        input_tokens: u.get("input_tokens")?.as_u64()?,
        output_tokens: u.get("output_tokens")?.as_u64()?,
    })
}

fn call_of(item: &Value) -> Result<ToolCall, ProviderError> {
    let id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
    if id.is_empty() || name.is_empty() {
        return Err(ProviderError::BadResponse("a tool call has no id or name"));
    }
    let args = item.get("arguments").and_then(Value::as_str).unwrap_or("");
    Ok(ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: if args.trim().is_empty() {
            "{}".into()
        } else {
            args.into()
        },
    })
}

/// The finish reason of a finished response object.
fn finish_of(response: &Value, has_calls: bool, refused: bool) -> FinishReason {
    match response
        .pointer("/incomplete_details/reason")
        .and_then(Value::as_str)
    {
        Some("max_output_tokens") => return FinishReason::Length,
        Some("content_filter") => return FinishReason::ContentFilter,
        Some(_) => return FinishReason::Other,
        None => {}
    }
    if refused {
        FinishReason::ContentFilter
    } else if has_calls {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    }
}

/// A `response.failed` / `error` event's code as the coarse error.
fn error_of(code: Option<&str>) -> ProviderError {
    match code {
        Some("rate_limit_exceeded") => ProviderError::RateLimited,
        Some("server_error") | Some("server_is_overloaded") => ProviderError::Status(503),
        _ => ProviderError::BadResponse("the provider reported an error mid-answer"),
    }
}

/// Parse a non-streamed response object.
pub fn parse_response(v: &Value) -> Result<ChatResponse, ProviderError> {
    if v.get("object").and_then(Value::as_str) != Some("response") {
        return Err(ProviderError::BadResponse("not a response object"));
    }
    if v.get("status").and_then(Value::as_str) == Some("failed") {
        return Err(error_of(v.pointer("/error/code").and_then(Value::as_str)));
    }
    check_tier(v)?;
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    let mut refused = false;
    for item in v
        .get("output")
        .and_then(Value::as_array)
        .ok_or(ProviderError::BadResponse("no output"))?
    {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for p in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    match p.get("type").and_then(Value::as_str) {
                        Some("output_text") => {
                            content.push_str(p.get("text").and_then(Value::as_str).unwrap_or(""))
                        }
                        Some("refusal") => {
                            refused = true;
                            content
                                .push_str(p.get("refusal").and_then(Value::as_str).unwrap_or(""));
                        }
                        _ => {}
                    }
                }
            }
            Some("function_call") => tool_calls.push(call_of(item)?),
            _ => {}
        }
    }
    if content.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::BadResponse("empty assistant message"));
    }
    let finish = finish_of(v, !tool_calls.is_empty(), refused);
    Ok(ChatResponse {
        content,
        tool_calls,
        usage: usage_from(v),
        finish,
    })
}

/// Read a streamed response (typed SSE events).
pub fn read_stream(
    body: impl std::io::Read,
    on_text: &mut dyn FnMut(&str),
) -> Result<ChatResponse, ProviderError> {
    let mut content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut refused = false;
    let mut last: Option<Value> = None;
    read_sse(body, &mut |_, data| {
        let v: Value = serde_json::from_str(data)
            .map_err(|_| ProviderError::BadResponse("a stream event was not JSON"))?;
        match v.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(t) = v.get("delta").and_then(Value::as_str) {
                    if !t.is_empty() {
                        content.push_str(t);
                        on_text(t);
                    }
                }
            }
            Some("response.refusal.delta") => {
                refused = true;
                if let Some(t) = v.get("delta").and_then(Value::as_str) {
                    content.push_str(t);
                }
            }
            Some("response.output_item.done") => {
                if let Some(item) = v.get("item") {
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        if tool_calls.len() >= 64 {
                            return Err(ProviderError::BadResponse("too many tool calls"));
                        }
                        tool_calls.push(call_of(item)?);
                    }
                }
            }
            Some("response.completed") | Some("response.incomplete") => {
                last = Some(v.get("response").cloned().unwrap_or(Value::Null));
                return Ok(false);
            }
            Some("response.failed") => {
                return Err(error_of(
                    v.pointer("/response/error/code").and_then(Value::as_str),
                ));
            }
            Some("error") => return Err(error_of(v.get("code").and_then(Value::as_str))),
            _ => {}
        }
        Ok(true)
    })?;
    let Some(response) = last else {
        return Err(ProviderError::Transport("the answer stream ended early"));
    };
    check_tier(&response)?;
    if content.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::BadResponse("empty assistant message"));
    }
    let finish = finish_of(&response, !tool_calls.is_empty(), refused);
    Ok(ChatResponse {
        content,
        tool_calls,
        usage: usage_from(&response),
        finish,
    })
}

impl ModelProvider for OpenAiAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(Vendor::OpenAi)
    }

    fn models(&self) -> Vec<ModelInfo> {
        catalog()
    }

    /// `max_output_tokens` is "an upper bound for the number of tokens that can be generated for
    /// a response, including visible output tokens and reasoning tokens" (Responses reference).
    fn output_bound(&self, req: &ChatRequest) -> u64 {
        u64::from(req.max_tokens)
    }

    fn key_format(&self) -> KeyFormat {
        KeyFormat {
            prefixes: &[KEY_PREFIX],
            charset: KeyCharset::UrlSafe,
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
        let body = to_wire(req)?;
        let resp = ctx.send(OutboundRequest {
            method: Method::Post,
            path: "/responses".into(),
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
            read_stream(resp.body, on_text)
        } else {
            parse_response(&resp.json()?)
        }
    }
}

#[cfg(test)]
mod tests {
    include!("provider_openai_tests.rs");
}
