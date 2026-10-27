//! D-11 (RC52) — the OpenAI-compatible adapter: the first [`ModelProvider`] implementation.
//!
//! Speaks `POST {base}/chat/completions` (streamed with `stream_options.include_usage`) and
//! `GET {base}/models` for the key test. It serves a member-confirmed custom endpoint, and the
//! vendors whose public API is OpenAI-compatible at their pinned base until their native adapter
//! (WP-S1-PROV-ADAPT) replaces it. The key travels only as `Authorization: Bearer`, attached by
//! core ([`ProviderCtx::send`]).

use crate::model_provider::{
    read_sse, status_error, AuthPlacement, ChatRequest, ChatResponse, FinishReason, KeyCharset,
    KeyFormat, KeyTest, Method, ModelInfo, ModelProvider, OutboundRequest, ProviderCtx,
    ProviderError, ProviderId, Role, ToolCall, Usage,
};
use serde_json::{json, Value};

/// The most model ids kept from a `/models` listing, and their longest accepted length.
pub const MAX_LISTED_MODELS: usize = 500;
pub const MAX_MODEL_ID_LEN: usize = 200;

pub struct OpenAiCompatAdapter {
    id: ProviderId,
    /// Accepted key prefixes (`sk-` for OpenAI/DeepSeek, `xai-` for xAI); empty = any.
    key_prefixes: &'static [&'static str],
    models: Vec<ModelInfo>,
}

impl OpenAiCompatAdapter {
    pub fn new(
        id: ProviderId,
        key_prefixes: &'static [&'static str],
        models: Vec<ModelInfo>,
    ) -> Self {
        OpenAiCompatAdapter {
            id,
            key_prefixes,
            models,
        }
    }
}

/// The OpenAI chat-completions body for a request.
pub fn to_wire(req: &ChatRequest) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let mut o = serde_json::Map::new();
            o.insert("role".into(), json!(m.role.as_str()));
            if m.role == Role::Assistant && !m.tool_calls.is_empty() && m.content.is_empty() {
                o.insert("content".into(), Value::Null);
            } else {
                o.insert("content".into(), json!(m.content));
            }
            if !m.tool_calls.is_empty() {
                o.insert(
                    "tool_calls".into(),
                    Value::Array(
                        m.tool_calls
                            .iter()
                            .map(|c| {
                                json!({"id": c.id, "type": "function",
                                       "function": {"name": c.name, "arguments": c.arguments}})
                            })
                            .collect(),
                    ),
                );
            }
            if let Some(id) = &m.tool_call_id {
                o.insert("tool_call_id".into(), json!(id));
            }
            Value::Object(o)
        })
        .collect();
    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "stream": req.stream,
    });
    if req.stream {
        body["stream_options"] = json!({"include_usage": true});
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| {
                    json!({"type": "function", "function": {
                        "name": t.name, "description": t.description, "parameters": t.parameters}})
                })
                .collect(),
        );
    }
    body
}

fn finish_from(s: Option<&str>) -> FinishReason {
    match s {
        Some("stop") => FinishReason::Stop,
        Some("tool_calls") | Some("function_call") => FinishReason::ToolCalls,
        Some("length") => FinishReason::Length,
        Some("content_filter") => FinishReason::ContentFilter,
        _ => FinishReason::Other,
    }
}

/// `usage.prompt_tokens` / `usage.completion_tokens`, both present as integers, else unknown.
pub fn usage_from(v: &Value) -> Option<Usage> {
    let u = v.get("usage")?;
    Some(Usage {
        input_tokens: u.get("prompt_tokens")?.as_u64()?,
        output_tokens: u.get("completion_tokens")?.as_u64()?,
    })
}

fn arguments_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Parse a non-streamed chat-completions answer.
pub fn parse_response(v: &Value) -> Result<ChatResponse, ProviderError> {
    let choice = v
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or(ProviderError::BadResponse("no choices"))?;
    let msg = choice
        .get("message")
        .ok_or(ProviderError::BadResponse("no message"))?;
    let content = msg
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut tool_calls = Vec::new();
    for (i, tc) in msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let f = tc.get("function");
        let name = f
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let args = arguments_text(f.and_then(|f| f.get("arguments")));
        tool_calls.push(ToolCall {
            id: tc
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{i}")),
            name: name.to_string(),
            arguments: if args.is_empty() { "{}".into() } else { args },
        });
    }
    if content.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::BadResponse("empty assistant message"));
    }
    Ok(ChatResponse {
        content,
        tool_calls,
        usage: usage_from(v),
        finish: finish_from(choice.get("finish_reason").and_then(Value::as_str)),
    })
}

#[derive(Default)]
struct Partial {
    id: Option<String>,
    name: String,
    arguments: String,
}

/// Read a streamed answer (`data: {chunk}` … `data: [DONE]`).
pub fn read_stream(
    body: impl std::io::Read,
    on_text: &mut dyn FnMut(&str),
) -> Result<ChatResponse, ProviderError> {
    let mut content = String::new();
    let mut calls: Vec<Partial> = Vec::new();
    let mut usage = None;
    let mut finish = None;
    let mut done = false;
    read_sse(body, &mut |_, data| {
        if data == "[DONE]" {
            done = true;
            return Ok(false);
        }
        let v: Value = serde_json::from_str(data)
            .map_err(|_| ProviderError::BadResponse("a stream chunk was not JSON"))?;
        if v.get("error").is_some() {
            return Err(ProviderError::BadResponse(
                "the provider reported an error mid-answer",
            ));
        }
        if let Some(u) = usage_from(&v) {
            usage = Some(u);
        }
        let Some(choice) = v
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        else {
            return Ok(true);
        };
        if let Some(f) = choice.get("finish_reason").and_then(Value::as_str) {
            finish = Some(finish_from(Some(f)));
        }
        let Some(delta) = choice.get("delta") else {
            return Ok(true);
        };
        for (pos, tc) in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let idx = tc
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|i| usize::try_from(i).ok())
                .unwrap_or(pos);
            if idx > 64 {
                return Err(ProviderError::BadResponse(
                    "a tool call index is out of range",
                ));
            }
            while calls.len() <= idx {
                calls.push(Partial::default());
            }
            let slot = &mut calls[idx];
            if let Some(id) = tc
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                slot.id = Some(id.to_string());
            }
            if let Some(f) = tc.get("function") {
                if let Some(n) = f.get("name").and_then(Value::as_str) {
                    slot.name.push_str(n);
                }
                slot.arguments.push_str(&arguments_text(f.get("arguments")));
            }
        }
        if let Some(t) = delta.get("content").and_then(Value::as_str) {
            if !t.is_empty() {
                content.push_str(t);
                on_text(t);
            }
        }
        Ok(true)
    })?;
    if !done && finish.is_none() {
        return Err(ProviderError::Transport("the answer stream ended early"));
    }
    let tool_calls: Vec<ToolCall> = calls
        .into_iter()
        .enumerate()
        .filter(|(_, c)| !c.name.is_empty())
        .map(|(i, c)| ToolCall {
            id: c.id.unwrap_or_else(|| format!("call_{i}")),
            name: c.name,
            arguments: if c.arguments.is_empty() {
                "{}".into()
            } else {
                c.arguments
            },
        })
        .collect();
    if content.is_empty() && tool_calls.is_empty() {
        return Err(ProviderError::BadResponse("empty assistant message"));
    }
    Ok(ChatResponse {
        content,
        tool_calls,
        usage,
        finish: finish.unwrap_or(FinishReason::Stop),
    })
}

/// The model ids of a `/models` listing (`data[].id`), bounded.
pub fn listed_models(v: &Value) -> Vec<String> {
    v.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .filter(|id| {
            !id.is_empty() && id.len() <= MAX_MODEL_ID_LEN && !id.chars().any(char::is_control)
        })
        .take(MAX_LISTED_MODELS)
        .map(str::to_string)
        .collect()
}

impl ModelProvider for OpenAiCompatAdapter {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn models(&self) -> Vec<ModelInfo> {
        self.models.clone()
    }

    fn key_format(&self) -> KeyFormat {
        KeyFormat {
            prefixes: self.key_prefixes,
            charset: KeyCharset::Printable,
            ..KeyFormat::ANY
        }
    }

    fn test_key(&self, ctx: &ProviderCtx<'_>) -> Result<KeyTest, ProviderError> {
        let resp = ctx.send(OutboundRequest {
            method: Method::Get,
            path: "/models".into(),
            headers: vec![],
            auth: AuthPlacement::Bearer,
            body: None,
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(status_error(resp.status));
        }
        let v = resp.json()?;
        Ok(KeyTest {
            models: listed_models(&v),
        })
    }

    fn complete(
        &self,
        ctx: &ProviderCtx<'_>,
        req: &ChatRequest,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
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
            body: Some(to_wire(req)),
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
    include!("provider_openai_compat_tests.rs");
}
