//! D-11 (RC52) — the native Anthropic adapter (Messages API). ADR-2026-10-26 P-3.
//!
//! Vendor sources (fetched 2026-10-26):
//! - Messages: <https://platform.claude.com/docs/en/api/messages>
//! - Versions (`anthropic-version`): <https://platform.claude.com/docs/en/api/versioning>
//! - Tool use: <https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls>
//! - Streaming events: <https://platform.claude.com/docs/en/build-with-claude/streaming>
//! - Stop reasons: <https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons>
//! - Errors: <https://platform.claude.com/docs/en/api/errors>
//! - Models list (key test): <https://platform.claude.com/docs/en/api/models/list>
//! - Prices: <https://platform.claude.com/docs/en/about-claude/pricing>
//!
//! Wire facts this adapter relies on:
//! - `POST /v1/messages`, key in `x-api-key` (still supported; `Authorization: Bearer` is the newer
//!   spelling), `anthropic-version: 2023-06-01`. The request uses no field the 2026-10-21 version
//!   changed (no `temperature` / `top_p` / `top_k`, no documents, no citations).
//! - System text is the top-level `system`; turns are `user` / `assistant` with content blocks;
//!   a tool call is a `tool_use` block (`input` is a JSON object) and its result a `tool_result`
//!   block in the next `user` turn, results FIRST, any text after. Consecutive same-role turns
//!   are merged here (the API would merge them too).
//! - Stream: `message_start` (input usage) → per block `content_block_start` /
//!   `content_block_delta` (`text_delta`, `input_json_delta`) / `content_block_stop` →
//!   `message_delta` (`stop_reason`, CUMULATIVE `usage.output_tokens`) → `message_stop`. `ping`
//!   and unknown events are skipped; an `error` event ends the call with an error.
//! - Usage: total input = `input_tokens` + `cache_creation_input_tokens` +
//!   `cache_read_input_tokens`; output = `output_tokens`.

use crate::model_provider::{
    read_sse, status_error, AuthPlacement, ChatMessage, ChatRequest, ChatResponse, FinishReason,
    KeyCharset, KeyFormat, KeyTest, Method, ModelInfo, ModelProvider, OutboundRequest, ProviderCtx,
    ProviderError, ProviderId, Role, ToolCall, Usage, Vendor,
};
use crate::provider_openai_compat::listed_models;
use serde_json::{json, Map, Value};

/// The API version header value. See the module docs for why this one.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// The documented key prefix (`sk-ant-api…`).
pub const KEY_PREFIX: &str = "sk-ant-api";

const MTOK: u64 = 1_000_000;

/// Current models and their published prices (micro-USD per million tokens), from
/// <https://platform.claude.com/docs/en/about-claude/pricing> (2026-10-26). Where a price depends
/// on prompt length (Haiku 5.5 over 100k tokens) the higher rate is used, so the spend guard
/// never under-counts. Cached input is metered at the full input rate for the same reason.
pub fn catalog() -> Vec<ModelInfo> {
    let m = |id: &str, name: &str, input: u64, output: u64| ModelInfo {
        id: id.into(),
        display_name: name.into(),
        supports_tools: true,
        input_micros_per_mtok: Some(input),
        output_micros_per_mtok: Some(output),
    };
    vec![
        m("claude-fable-5-1", "Claude Fable 5.1", 10 * MTOK, 50 * MTOK),
        m("claude-opus-5-5", "Claude Opus 5.5", 4 * MTOK, 20 * MTOK),
        m(
            "claude-sonnet-5-5",
            "Claude Sonnet 5.5",
            2 * MTOK,
            10 * MTOK,
        ),
        m(
            "claude-haiku-5-5",
            "Claude Haiku 5.5",
            MTOK / 2,
            5 * MTOK / 2,
        ),
    ]
}

pub struct AnthropicAdapter;

/// A tool call's `arguments` (JSON text) as the object Anthropic's `input` must be.
pub(crate) fn arguments_object(args: &str) -> Result<Value, ProviderError> {
    let t = args.trim();
    if t.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(t) {
        Ok(v @ Value::Object(_)) => Ok(v),
        _ => Err(ProviderError::BadRequest(
            "a tool call in the conversation has arguments that are not a JSON object",
        )),
    }
}

/// Append a turn, merging it into the previous one when the role repeats.
fn push_turn(turns: &mut Vec<(&'static str, Vec<Value>)>, role: &'static str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    match turns.last_mut() {
        Some((r, b)) if *r == role => b.extend(blocks),
        _ => turns.push((role, blocks)),
    }
}

/// The Messages API body for a request.
pub fn to_wire(req: &ChatRequest) -> Result<Value, ProviderError> {
    let mut system: Vec<&str> = Vec::new();
    let mut turns: Vec<(&'static str, Vec<Value>)> = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::System => {
                if !m.content.trim().is_empty() {
                    system.push(&m.content);
                }
            }
            Role::User => {
                let blocks = text_block(m);
                push_turn(&mut turns, "user", blocks);
            }
            Role::Assistant => {
                let mut blocks = text_block(m);
                for c in &m.tool_calls {
                    blocks.push(json!({"type": "tool_use", "id": c.id, "name": c.name,
                        "input": arguments_object(&c.arguments)?}));
                }
                push_turn(&mut turns, "assistant", blocks);
            }
            Role::Tool => {
                let id = m.tool_call_id.as_deref().filter(|s| !s.is_empty()).ok_or(
                    ProviderError::BadRequest("a tool result does not name its tool call"),
                )?;
                let block = json!({"type": "tool_result", "tool_use_id": id,
                    "content": m.content});
                // Results go FIRST in their user turn: insert before any text already merged.
                match turns.last_mut() {
                    Some((r, b)) if *r == "user" => {
                        let at = b
                            .iter()
                            .position(|x| x["type"] != "tool_result")
                            .unwrap_or(b.len());
                        b.insert(at, block);
                    }
                    _ => turns.push(("user", vec![block])),
                }
            }
        }
    }
    if turns.first().is_some_and(|(r, _)| *r != "user") {
        return Err(ProviderError::BadRequest(
            "the conversation must start with a user turn",
        ));
    }
    let messages: Vec<Value> = turns
        .into_iter()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect();
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("max_tokens".into(), json!(req.max_tokens));
    if !system.is_empty() {
        body.insert("system".into(), json!(system.join("\n\n")));
    }
    body.insert("messages".into(), Value::Array(messages));
    if !req.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        json!({"name": t.name, "description": t.description,
                            "input_schema": t.parameters})
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

/// A text block for a non-empty message (Anthropic refuses empty text blocks).
fn text_block(m: &ChatMessage) -> Vec<Value> {
    if m.content.is_empty() {
        vec![]
    } else {
        vec![json!({"type": "text", "text": m.content})]
    }
}

fn finish_from(stop_reason: Option<&str>) -> FinishReason {
    match stop_reason {
        Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("tool_use") => FinishReason::ToolCalls,
        Some("max_tokens") | Some("model_context_window_exceeded") => FinishReason::Length,
        Some("refusal") => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

fn count(u: &Value, field: &str) -> Option<u64> {
    match u.get(field) {
        None | Some(Value::Null) => Some(0),
        Some(v) => v.as_u64(),
    }
}

/// Input tokens of a `usage` object (fresh + cache writes + cache reads); `None` when
/// `input_tokens` is missing or any count is malformed.
fn input_of(u: &Value) -> Option<u64> {
    let fresh = u.get("input_tokens")?.as_u64()?;
    Some(
        fresh
            .saturating_add(count(u, "cache_creation_input_tokens")?)
            .saturating_add(count(u, "cache_read_input_tokens")?),
    )
}

/// `usage` of a whole message: both counts present, else unknown.
pub fn usage_from(v: &Value) -> Option<Usage> {
    let u = v.get("usage")?;
    Some(Usage {
        input_tokens: input_of(u)?,
        output_tokens: u.get("output_tokens")?.as_u64()?,
    })
}

/// Parse a non-streamed Messages answer.
pub fn parse_response(v: &Value) -> Result<ChatResponse, ProviderError> {
    if v.get("type").and_then(Value::as_str) != Some("message") {
        return Err(ProviderError::BadResponse("not a message"));
    }
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for b in v
        .get("content")
        .and_then(Value::as_array)
        .ok_or(ProviderError::BadResponse("no content"))?
    {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => content.push_str(b.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("tool_use") => tool_calls.push(tool_call_of(b, b.get("input"))?),
            _ => {}
        }
    }
    if content.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::BadResponse("empty assistant message"));
    }
    Ok(ChatResponse {
        content,
        tool_calls,
        usage: usage_from(v),
        finish: finish_from(v.get("stop_reason").and_then(Value::as_str)),
    })
}

fn tool_call_of(block: &Value, input: Option<&Value>) -> Result<ToolCall, ProviderError> {
    let id = block.get("id").and_then(Value::as_str).unwrap_or("");
    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
    if id.is_empty() || name.is_empty() {
        return Err(ProviderError::BadResponse("a tool call has no id or name"));
    }
    let arguments = match input {
        Some(v @ Value::Object(_)) => v.to_string(),
        _ => "{}".into(),
    };
    Ok(ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    })
}

/// A mid-stream `error` event, mapped like the HTTP status of the same error type.
fn stream_error(v: &Value) -> ProviderError {
    match v.pointer("/error/type").and_then(Value::as_str) {
        Some("authentication_error") | Some("permission_error") => ProviderError::Unauthorized,
        Some("rate_limit_error") => ProviderError::RateLimited,
        Some("overloaded_error") => ProviderError::Status(529),
        Some("api_error") => ProviderError::Status(500),
        _ => ProviderError::BadResponse("the provider reported an error mid-answer"),
    }
}

enum Block {
    Text,
    Tool {
        id: String,
        name: String,
        json: String,
    },
    Other,
}

/// Read a streamed Messages answer.
pub fn read_stream(
    body: impl std::io::Read,
    on_text: &mut dyn FnMut(&str),
) -> Result<ChatResponse, ProviderError> {
    let mut content = String::new();
    let mut blocks: Vec<(u64, Block)> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut input_tokens: Option<u64> = None;
    let mut output_tokens: Option<u64> = None;
    let mut stop_reason: Option<String> = None;
    let mut stopped = false;
    read_sse(body, &mut |_, data| {
        let v: Value = serde_json::from_str(data)
            .map_err(|_| ProviderError::BadResponse("a stream event was not JSON"))?;
        match v.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(u) = v.pointer("/message/usage") {
                    input_tokens = input_of(u);
                    output_tokens = u.get("output_tokens").and_then(Value::as_u64);
                }
            }
            Some("content_block_start") => {
                let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
                if blocks.len() > 256 {
                    return Err(ProviderError::BadResponse("too many content blocks"));
                }
                let cb = v.get("content_block").cloned().unwrap_or(Value::Null);
                let block = match cb.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = cb.get("text").and_then(Value::as_str) {
                            if !t.is_empty() {
                                content.push_str(t);
                                on_text(t);
                            }
                        }
                        Block::Text
                    }
                    Some("tool_use") => Block::Tool {
                        id: cb.get("id").and_then(Value::as_str).unwrap_or("").into(),
                        name: cb.get("name").and_then(Value::as_str).unwrap_or("").into(),
                        json: String::new(),
                    },
                    _ => Block::Other,
                };
                blocks.push((index, block));
            }
            Some("content_block_delta") => {
                let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some((_, block)) = blocks.iter_mut().rev().find(|(i, _)| *i == index) else {
                    return Err(ProviderError::BadResponse(
                        "a delta for a block never started",
                    ));
                };
                let d = v.get("delta").cloned().unwrap_or(Value::Null);
                match (d.get("type").and_then(Value::as_str), block) {
                    (Some("text_delta"), Block::Text) => {
                        if let Some(t) = d.get("text").and_then(Value::as_str) {
                            if !t.is_empty() {
                                content.push_str(t);
                                on_text(t);
                            }
                        }
                    }
                    (Some("input_json_delta"), Block::Tool { json, .. }) => {
                        let piece = d.get("partial_json").and_then(Value::as_str).unwrap_or("");
                        if json.len() + piece.len() > 4 * 1024 * 1024 {
                            return Err(ProviderError::BadResponse("tool call arguments too long"));
                        }
                        json.push_str(piece);
                    }
                    _ => {}
                }
            }
            Some("content_block_stop") => {
                let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
                if let Some(pos) = blocks.iter().rposition(|(i, _)| *i == index) {
                    if let (_, Block::Tool { id, name, json }) = blocks.remove(pos) {
                        let input = if json.trim().is_empty() {
                            json!({})
                        } else {
                            serde_json::from_str(&json).map_err(|_| {
                                ProviderError::BadResponse("a streamed tool call was not JSON")
                            })?
                        };
                        let block = json!({"id": id, "name": name});
                        tool_calls.push(tool_call_of(&block, Some(&input))?);
                    }
                }
            }
            Some("message_delta") => {
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    stop_reason = Some(r.to_string());
                }
                if let Some(u) = v.get("usage") {
                    // Cumulative counts: the latest wins.
                    if let Some(o) = u.get("output_tokens").and_then(Value::as_u64) {
                        output_tokens = Some(o);
                    }
                    if u.get("input_tokens").and_then(Value::as_u64).is_some() {
                        input_tokens = input_of(u);
                    }
                }
            }
            Some("message_stop") => {
                stopped = true;
                return Ok(false);
            }
            Some("error") => return Err(stream_error(&v)),
            _ => {}
        }
        Ok(true)
    })?;
    if !stopped {
        return Err(ProviderError::Transport("the answer stream ended early"));
    }
    if content.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::BadResponse("empty assistant message"));
    }
    let usage = match (input_tokens, output_tokens) {
        (Some(i), Some(o)) => Some(Usage {
            input_tokens: i,
            output_tokens: o,
        }),
        _ => None,
    };
    Ok(ChatResponse {
        content,
        tool_calls,
        usage,
        finish: finish_from(stop_reason.as_deref()),
    })
}

fn headers(accept: &str) -> Vec<(&'static str, String)> {
    vec![
        ("anthropic-version", ANTHROPIC_VERSION.into()),
        ("accept", accept.into()),
    ]
}

impl ModelProvider for AnthropicAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(Vendor::Anthropic)
    }

    fn models(&self) -> Vec<ModelInfo> {
        catalog()
    }

    /// `max_tokens` bounds the whole output, extended thinking included ("`max_tokens` …
    /// includes your thinking budget", <https://platform.claude.com/docs/en/build-with-claude/extended-thinking>).
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
            path: "/models?limit=1000".into(),
            headers: headers("application/json"),
            auth: AuthPlacement::Header("x-api-key"),
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
            path: "/messages".into(),
            headers: headers(if req.stream {
                "text/event-stream"
            } else {
                "application/json"
            }),
            auth: AuthPlacement::Header("x-api-key"),
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
    include!("provider_anthropic_tests.rs");
}
