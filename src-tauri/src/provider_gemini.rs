//! D-11 (RC52) — the native Google Gemini adapter (`generateContent`). ADR-2026-10-26 P-3.
//!
//! Vendor sources (fetched 2026-10-26):
//! - Reference: <https://ai.google.dev/api/generate-content>, models list
//!   <https://ai.google.dev/api/models>
//! - Key header: <https://ai.google.dev/gemini-api/docs/generate-content/api-key>
//! - Function calling: <https://ai.google.dev/gemini-api/docs/generate-content/function-calling>
//! - Thought signatures: <https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures>
//! - Errors: <https://ai.google.dev/gemini-api/docs/generate-content/api-errors>
//! - Prices: <https://ai.google.dev/gemini-api/docs/pricing>
//!
//! Wire facts this adapter relies on:
//! - `POST /v1beta/models/{model}:generateContent`, or `:streamGenerateContent?alt=sse` (each SSE
//!   `data:` is a whole `GenerateContentResponse`; there is no `[DONE]`, so the stream counts as
//!   finished only once a candidate carries a `finishReason`). The key goes in `x-goog-api-key`,
//!   never in `?key=`.
//! - `systemInstruction`, `contents[{role: user|model, parts}]`; a tool call is a `functionCall`
//!   part `{id, name, args}` and its result a `functionResponse` part `{id, name, response}` in
//!   the next `user` content (parallel results together, after all the calls). Tools are
//!   `functionDeclarations` with `parametersJsonSchema` (full JSON Schema).
//! - Gemini 3 and newer require the call's `thoughtSignature` back on the first `functionCall`
//!   part of each step. The internal format does not carry signatures, so replayed calls carry
//!   the documented validator-skip value ([`REPLAY_SIGNATURE`]).
//! - Usage: input = `promptTokenCount` (+ `toolUsePromptTokenCount`); output =
//!   `candidatesTokenCount` + `thoughtsTokenCount` (thinking is billed as output).
//! - An invalid key is a 400 with reason `API_KEY_INVALID` (mapped to "the key was rejected").

use crate::model_provider::{
    read_sse, status_error, AuthPlacement, ChatRequest, ChatResponse, FinishReason, KeyFormat,
    KeyTest, Method, ModelInfo, ModelProvider, OutboundRequest, ProviderCtx, ProviderError,
    ProviderId, Role, ToolCall, Usage, Vendor,
};
use crate::provider_anthropic::arguments_object;
use crate::provider_openai_compat::{MAX_LISTED_MODELS, MAX_MODEL_ID_LEN};
use serde_json::{json, Map, Value};

/// The documented dummy signature for replayed function calls whose real signature is not kept
/// ("I am injecting function calls … you can set … `skip_thought_signature_validator`").
pub const REPLAY_SIGNATURE: &str = "skip_thought_signature_validator";

const MTOK: u64 = 1_000_000;

/// Current text models and their published paid-tier prices (micro-USD per million tokens),
/// from <https://ai.google.dev/gemini-api/docs/pricing> (2026-10-26). Where a price rises with
/// prompt size, or a launch price ends on a published date, the higher price is used so the
/// spend guard never under-counts.
pub fn catalog() -> Vec<ModelInfo> {
    let m = |id: &str, name: &str, input: u64, output: u64| ModelInfo {
        id: id.into(),
        display_name: name.into(),
        supports_tools: true,
        input_micros_per_mtok: Some(input),
        output_micros_per_mtok: Some(output),
    };
    vec![
        m("gemini-4-argon", "Gemini 4 Argon", 4 * MTOK, 20 * MTOK),
        m(
            "gemini-3.8-flash",
            "Gemini 3.8 Flash",
            3 * MTOK / 2,
            15 * MTOK / 2,
        ),
        m(
            "gemini-3.6-flash",
            "Gemini 3.6 Flash",
            3 * MTOK / 2,
            15 * MTOK / 2,
        ),
        m(
            "gemini-3.5-flash-lite",
            "Gemini 3.5 Flash-Lite",
            3 * MTOK / 10,
            5 * MTOK / 2,
        ),
        m(
            "gemini-3.1-pro-preview",
            "Gemini 3.1 Pro Preview",
            4 * MTOK,
            18 * MTOK,
        ),
        m("gemini-2.5-pro", "Gemini 2.5 Pro", 5 * MTOK / 2, 15 * MTOK),
        m(
            "gemini-2.5-flash",
            "Gemini 2.5 Flash",
            3 * MTOK / 10,
            5 * MTOK / 2,
        ),
        m(
            "gemini-2.5-flash-lite",
            "Gemini 2.5 Flash-Lite",
            MTOK / 10,
            2 * MTOK / 5,
        ),
    ]
}

pub struct GeminiAdapter;

/// The model id as it goes in the URL path: `[A-Za-z0-9._-]`, 1..=128 bytes, an optional
/// `models/` prefix dropped. Anything else is refused before a request is built.
pub fn path_model(model: &str) -> Result<&str, ProviderError> {
    let m = model.strip_prefix("models/").unwrap_or(model);
    let ok = !m.is_empty()
        && m.len() <= 128
        && !m.starts_with('.')
        && m.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(m)
    } else {
        Err(ProviderError::BadRequest(
            "that model id cannot be used in a request path",
        ))
    }
}

/// A tool result's `response` object: `{"result": <the result>}` (JSON when it parses, else the
/// text), as in the vendor's function-calling examples.
fn function_response(content: &str) -> Value {
    let result = serde_json::from_str::<Value>(content.trim())
        .unwrap_or_else(|_| Value::String(content.to_string()));
    json!({ "result": result })
}

fn push_content(
    contents: &mut Vec<(&'static str, Vec<Value>)>,
    role: &'static str,
    parts: Vec<Value>,
) {
    if parts.is_empty() {
        return;
    }
    match contents.last_mut() {
        Some((r, p)) if *r == role => p.extend(parts),
        _ => contents.push((role, parts)),
    }
}

/// The `generateContent` body for a request.
pub fn to_wire(req: &ChatRequest) -> Result<Value, ProviderError> {
    let mut system: Vec<&str> = Vec::new();
    let mut contents: Vec<(&'static str, Vec<Value>)> = Vec::new();
    // Tool-call id -> function name (a functionResponse must name its function).
    let mut names: Vec<(&str, &str)> = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::System => {
                if !m.content.trim().is_empty() {
                    system.push(&m.content);
                }
            }
            Role::User => {
                if !m.content.is_empty() {
                    push_content(&mut contents, "user", vec![json!({"text": m.content})]);
                }
            }
            Role::Assistant => {
                let mut parts = Vec::new();
                if !m.content.is_empty() {
                    parts.push(json!({"text": m.content}));
                }
                for (i, c) in m.tool_calls.iter().enumerate() {
                    let mut p = json!({"functionCall": {"id": c.id, "name": c.name,
                        "args": arguments_object(&c.arguments)?}});
                    if i == 0 {
                        p["thoughtSignature"] = json!(REPLAY_SIGNATURE);
                    }
                    parts.push(p);
                    names.push((c.id.as_str(), c.name.as_str()));
                }
                push_content(&mut contents, "model", parts);
            }
            Role::Tool => {
                let id = m.tool_call_id.as_deref().filter(|s| !s.is_empty()).ok_or(
                    ProviderError::BadRequest("a tool result does not name its tool call"),
                )?;
                let name = names
                    .iter()
                    .rev()
                    .find(|(i, _)| *i == id)
                    .map(|(_, n)| *n)
                    .ok_or(ProviderError::BadRequest(
                        "a tool result answers a tool call that is not in the conversation",
                    ))?;
                push_content(
                    &mut contents,
                    "user",
                    vec![json!({"functionResponse": {"id": id, "name": name,
                        "response": function_response(&m.content)}})],
                );
            }
        }
    }
    let mut body = Map::new();
    if !system.is_empty() {
        body.insert(
            "systemInstruction".into(),
            json!({"parts": [{"text": system.join("\n\n")}]}),
        );
    }
    body.insert(
        "contents".into(),
        Value::Array(
            contents
                .into_iter()
                .map(|(role, parts)| json!({"role": role, "parts": parts}))
                .collect(),
        ),
    );
    if !req.tools.is_empty() {
        body.insert(
            "tools".into(),
            json!([{"functionDeclarations": req.tools.iter().map(|t| json!({
                "name": t.name, "description": t.description,
                "parametersJsonSchema": t.parameters})).collect::<Vec<_>>()}]),
        );
    }
    body.insert(
        "generationConfig".into(),
        json!({"maxOutputTokens": req.max_tokens}),
    );
    Ok(Value::Object(body))
}

fn finish_from(reason: &str, has_calls: bool) -> FinishReason {
    match reason {
        "STOP" if has_calls => FinishReason::ToolCalls,
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "LANGUAGE"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION" => FinishReason::ContentFilter,
        _ if has_calls => FinishReason::ToolCalls,
        _ => FinishReason::Other,
    }
}

fn opt_count(u: &Value, field: &str) -> Option<u64> {
    match u.get(field) {
        None | Some(Value::Null) => Some(0),
        Some(v) => v.as_u64(),
    }
}

/// `usageMetadata`: `promptTokenCount` must be present; the other counts default to 0 (the
/// vendor omits zero counts) and a malformed one makes the usage unknown.
pub fn usage_from(v: &Value) -> Option<Usage> {
    let u = v.get("usageMetadata")?;
    let prompt = u.get("promptTokenCount")?.as_u64()?;
    Some(Usage {
        input_tokens: prompt.saturating_add(opt_count(u, "toolUsePromptTokenCount")?),
        output_tokens: opt_count(u, "candidatesTokenCount")?
            .saturating_add(opt_count(u, "thoughtsTokenCount")?),
    })
}

/// Fold one response (or stream chunk) into the turn so far. Returns the candidate's
/// `finishReason` when it has one.
fn absorb(
    v: &Value,
    content: &mut String,
    calls: &mut Vec<ToolCall>,
    on_text: &mut dyn FnMut(&str),
) -> Result<Option<String>, ProviderError> {
    if v.get("error").is_some() {
        return Err(error_of(v));
    }
    let candidate = v
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first());
    let Some(candidate) = candidate else {
        if v.pointer("/promptFeedback/blockReason").is_some() {
            return Err(ProviderError::BadResponse(
                "the provider blocked the prompt",
            ));
        }
        return Ok(None);
    };
    for p in candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if p.get("thought").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(t) = p.get("text").and_then(Value::as_str) {
            if !t.is_empty() {
                content.push_str(t);
                on_text(t);
            }
        }
        if let Some(fc) = p.get("functionCall") {
            let name = fc.get("name").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() {
                return Err(ProviderError::BadResponse("a tool call has no name"));
            }
            if calls.len() >= 64 {
                return Err(ProviderError::BadResponse("too many tool calls"));
            }
            let id = fc
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{}", calls.len()));
            let arguments = match fc.get("args") {
                Some(a @ Value::Object(_)) => a.to_string(),
                _ => "{}".into(),
            };
            calls.push(ToolCall {
                id,
                name: name.into(),
                arguments,
            });
        }
    }
    Ok(candidate
        .get("finishReason")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty() && *r != "FINISH_REASON_UNSPECIFIED")
        .map(str::to_string))
}

/// The coarse error for a Gemini error body (an HTTP error answer or an error in a stream).
fn error_of(v: &Value) -> ProviderError {
    let reason_is = |r: &str| {
        v.pointer("/error/details")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|d| d.get("reason").and_then(Value::as_str) == Some(r))
    };
    if reason_is("API_KEY_INVALID") || reason_is("API_KEY_SERVICE_BLOCKED") {
        return ProviderError::Unauthorized;
    }
    match v.pointer("/error/code").and_then(Value::as_u64) {
        Some(c) => status_error(u16::try_from(c).unwrap_or(500)),
        None => ProviderError::BadResponse("the provider reported an error mid-answer"),
    }
}

/// The error for a non-2xx answer. A 400 is read for the vendor's "invalid key" reason (Gemini
/// answers a bad key with 400, not 401); the body is never carried into the error.
fn http_error(resp: crate::model_provider::InboundResponse) -> ProviderError {
    let status = resp.status;
    if status == 400 {
        if let Ok(v) = resp.json() {
            if v.get("error").is_some() {
                return match error_of(&v) {
                    ProviderError::Unauthorized => ProviderError::Unauthorized,
                    _ => status_error(status),
                };
            }
        }
    }
    status_error(status)
}

fn finish_turn(
    content: String,
    tool_calls: Vec<ToolCall>,
    usage: Option<Usage>,
    reason: &str,
) -> Result<ChatResponse, ProviderError> {
    if content.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::BadResponse("empty assistant message"));
    }
    let has_calls = !tool_calls.is_empty();
    Ok(ChatResponse {
        content,
        tool_calls,
        usage,
        finish: finish_from(reason, has_calls),
    })
}

/// Parse a non-streamed `generateContent` answer.
pub fn parse_response(v: &Value) -> Result<ChatResponse, ProviderError> {
    let mut content = String::new();
    let mut calls = Vec::new();
    let reason = absorb(v, &mut content, &mut calls, &mut |_| {})?;
    finish_turn(
        content,
        calls,
        usage_from(v),
        reason.as_deref().unwrap_or(""),
    )
}

/// Read a `streamGenerateContent?alt=sse` answer.
pub fn read_stream(
    body: impl std::io::Read,
    on_text: &mut dyn FnMut(&str),
) -> Result<ChatResponse, ProviderError> {
    let mut content = String::new();
    let mut calls = Vec::new();
    let mut usage = None;
    let mut reason: Option<String> = None;
    read_sse(body, &mut |_, data| {
        let v: Value = serde_json::from_str(data)
            .map_err(|_| ProviderError::BadResponse("a stream chunk was not JSON"))?;
        if let Some(r) = absorb(&v, &mut content, &mut calls, on_text)? {
            reason = Some(r);
        }
        if let Some(u) = usage_from(&v) {
            usage = Some(u);
        }
        Ok(true)
    })?;
    let Some(reason) = reason else {
        return Err(ProviderError::Transport("the answer stream ended early"));
    };
    finish_turn(content, calls, usage, &reason)
}

/// The text-generation models of a `GET /models` listing (`models[].name` without `models/`).
pub fn listed_models(v: &Value) -> Vec<String> {
    v.get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| {
            m.get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_none_or(|ms| ms.iter().any(|x| x == "generateContent"))
        })
        .filter_map(|m| m.get("name").and_then(Value::as_str))
        .map(|n| n.strip_prefix("models/").unwrap_or(n))
        .filter(|id| {
            !id.is_empty() && id.len() <= MAX_MODEL_ID_LEN && !id.chars().any(char::is_control)
        })
        .take(MAX_LISTED_MODELS)
        .map(str::to_string)
        .collect()
}

impl ModelProvider for GeminiAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(Vendor::Gemini)
    }

    fn models(&self) -> Vec<ModelInfo> {
        catalog()
    }

    /// Gemini documents no key prefix, and its newer authorization keys have their own format,
    /// so only the shape is checked; the live key test decides.
    fn key_format(&self) -> KeyFormat {
        KeyFormat::ANY
    }

    fn test_key(&self, ctx: &ProviderCtx<'_>) -> Result<KeyTest, ProviderError> {
        let resp = ctx.send(OutboundRequest {
            method: Method::Get,
            path: "/models?pageSize=1000".into(),
            headers: vec![("accept", "application/json".into())],
            auth: AuthPlacement::Header("x-goog-api-key"),
            body: None,
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(http_error(resp));
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
        let model = path_model(&req.model)?;
        let body = to_wire(req)?;
        let (path, accept) = if req.stream {
            (
                format!("/models/{model}:streamGenerateContent?alt=sse"),
                "text/event-stream",
            )
        } else {
            (
                format!("/models/{model}:generateContent"),
                "application/json",
            )
        };
        let resp = ctx.send(OutboundRequest {
            method: Method::Post,
            path,
            headers: vec![("accept", accept.into())],
            auth: AuthPlacement::Header("x-goog-api-key"),
            body: Some(body),
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(http_error(resp));
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
    include!("provider_gemini_tests.rs");
}
