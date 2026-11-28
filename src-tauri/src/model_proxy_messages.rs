//! L1-WP12 (v060) — `POST /v1/messages` on the loopback model proxy, for agents that speak the
//! Anthropic Messages API (Claude Agent with a key the member keeps on the device; owner answer
//! L1-Q4, 2026-11-27). Planset v060 `lanes/L1_AGENT_RUNTIME.md` WP12.
//!
//! The Messages body is translated to the proxy's internal [`ChatRequest`] and runs the same seven
//! steps as `/v1/chat/completions`. The route binds provider and model; a body may only repeat the
//! route's model. The route token may arrive as `Authorization: Bearer` or `x-api-key` (Anthropic
//! clients send the latter); a vendor key in either is not a route. The wire is the
//! `anthropic-version` the Anthropic adapter is pinned to; a request naming another is refused.
//! Carried: system text (top-level and `system` turns), text, `tool_use`, `tool_result`, custom
//! tools. Dropped: `thinking` blocks and the `thinking` setting (a vendor's opaque state, like
//! Responses reasoning), and request options the internal request has no field for
//! (`output_config`, `context_management`, `safeguards`, `metadata`). Refused by name:
//! images and documents, server tools (`web_search_*`, `bash_*`, ...), `stop_sequences`.
//!
//! A streamed answer is the Messages SSE sequence (`message_start`, `content_block_*`,
//! `message_delta`); `message_stop` is written only after the call's usage end record is.

use crate::model_provider::{
    ChatMessage, ChatRequest, ChatResponse, FinishReason, Role, ToolCall, ToolDef,
};
use crate::model_proxy::{ModelProxy, ProxyError};
use crate::model_proxy_http::{write_json, HttpReq};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The Messages wire version this path speaks (the Anthropic adapter's pin).
pub const ANTHROPIC_VERSION: &str = crate::provider_anthropic::ANTHROPIC_VERSION;

fn msg(role: Role, content: String) -> ChatMessage {
    ChatMessage {
        role,
        content,
        tool_calls: vec![],
        tool_call_id: None,
    }
}

fn text_blocks(v: Option<&Value>) -> Result<String, String> {
    match v {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Array(blocks)) => {
            let mut out = String::new();
            for b in blocks {
                match (
                    b.get("type").and_then(Value::as_str),
                    b.get("text").and_then(Value::as_str),
                ) {
                    (Some("text"), Some(t)) => out.push_str(t),
                    (Some(other), _) => return Err(format!("{other} content is not supported")),
                    _ => return Err("a content block needs a type".into()),
                }
            }
            Ok(out)
        }
        Some(_) => Err("content must be text or an array of blocks".into()),
    }
}

/// One Messages turn → internal messages (a user turn's tool results come first, as tool messages).
fn turn(messages: &mut Vec<ChatMessage>, m: &Value) -> Result<(), String> {
    let role = match m.get("role").and_then(Value::as_str) {
        Some("user") => Role::User,
        Some("assistant") => Role::Assistant,
        // Claude Code sends system turns inside `messages`; they carry text only.
        Some("system") => Role::System,
        _ => return Err("each message needs a role of user, assistant or system".into()),
    };
    let blocks = match m.get("content") {
        Some(Value::String(s)) => {
            messages.push(msg(role, s.clone()));
            return Ok(());
        }
        Some(Value::Array(b)) => b,
        _ => return Err("message content must be text or an array of blocks".into()),
    };
    let mut text = String::new();
    let mut calls = Vec::new();
    for b in blocks {
        let s = |k: &str| b.get(k).and_then(Value::as_str);
        match s("type") {
            Some("text") => text.push_str(s("text").unwrap_or("")),
            Some("thinking" | "redacted_thinking") => {}
            Some("tool_use") if role == Role::Assistant => calls.push(ToolCall {
                id: s("id").ok_or("a tool_use needs an id")?.to_string(),
                name: s("name").ok_or("a tool_use needs a name")?.to_string(),
                arguments: b
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| json!({}))
                    .to_string(),
            }),
            Some("tool_result") if role == Role::User => messages.push(ChatMessage {
                role: Role::Tool,
                content: text_blocks(b.get("content"))?,
                tool_calls: vec![],
                tool_call_id: Some(
                    s("tool_use_id")
                        .ok_or("a tool_result needs a tool_use_id")?
                        .to_string(),
                ),
            }),
            Some(other) => return Err(format!("{other} content is not supported here")),
            None => return Err("a content block needs a type".into()),
        }
    }
    if !text.is_empty() || !calls.is_empty() {
        messages.push(ChatMessage {
            role,
            content: text,
            tool_calls: calls,
            tool_call_id: None,
        });
    }
    Ok(())
}

/// A Messages API body → the internal request.
pub fn from_messages_body(v: &Value) -> Result<ChatRequest, String> {
    if v.get("stop_sequences")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        return Err("stop_sequences are not supported by this proxy".into());
    }
    let mut messages = Vec::new();
    let system = text_blocks(v.get("system"))?;
    if !system.is_empty() {
        messages.push(msg(Role::System, system));
    }
    let turns = v
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("messages must be an array")?;
    if turns.is_empty() || turns.len() > 4096 {
        return Err("messages must hold 1 to 4096 entries".into());
    }
    for m in turns {
        turn(&mut messages, m)?;
    }
    let mut tools = Vec::new();
    for t in v
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match t.get("type").and_then(Value::as_str) {
            None | Some("custom") => tools.push(ToolDef {
                name: t
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("a tool needs a name")?
                    .to_string(),
                description: t
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                parameters: t
                    .get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
            }),
            Some(other) => {
                return Err(format!(
                    "server tool {other} is not supported; only custom tools"
                ))
            }
        }
    }
    Ok(ChatRequest {
        model: v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        messages,
        tools,
        max_tokens: v
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|n| n.min(u64::from(crate::model_proxy::MAX_OUTPUT_TOKENS)) as u32)
            .unwrap_or(0),
        stream: v.get("stream").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn stop_reason(f: FinishReason) -> &'static str {
    match f {
        FinishReason::ToolCalls => "tool_use",
        FinishReason::Length => "max_tokens",
        FinishReason::ContentFilter => "refusal",
        FinishReason::Stop | FinishReason::Other => "end_turn",
    }
}

fn tool_input(arguments: &str) -> Value {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// The whole answer as a Messages API `message`.
pub fn message_json(id: &str, model: &str, r: &ChatResponse) -> Value {
    let mut content = Vec::new();
    if !r.content.is_empty() {
        content.push(json!({"type": "text", "text": r.content}));
    }
    for c in &r.tool_calls {
        content.push(json!({"type": "tool_use", "id": c.id, "name": c.name, "input": tool_input(&c.arguments)}));
    }
    let u = r.usage.unwrap_or_default();
    json!({"id": id, "type": "message", "role": "assistant", "model": model, "content": content,
           "stop_reason": stop_reason(r.finish), "stop_sequence": null,
           "usage": {"input_tokens": u.input_tokens, "output_tokens": u.output_tokens}})
}

/// The Anthropic error shape.
fn error_json(e: &ProxyError) -> Value {
    json!({"type": "error", "error": {"type": e.http().1, "message": e.to_string()}})
}

/// What a streamed answer has written so far.
#[derive(Default)]
pub struct StreamState {
    blocks: usize,
    text_open: bool,
}

fn event(out: &mut dyn Write, kind: &str, mut data: Value) -> std::io::Result<()> {
    data["type"] = json!(kind);
    write!(out, "event: {kind}\ndata: {data}\n\n")?;
    out.flush()
}

fn stream_head(out: &mut dyn Write, id: &str, model: &str) -> std::io::Result<()> {
    write!(out, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n")?;
    event(
        out,
        "message_start",
        json!({"message": {"id": id, "type": "message", "role": "assistant", "model": model,
        "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 0, "output_tokens": 0}}}),
    )
}

fn text_delta(out: &mut dyn Write, st: &mut StreamState, t: &str) -> std::io::Result<()> {
    if !st.text_open {
        st.text_open = true;
        event(
            out,
            "content_block_start",
            json!({"index": st.blocks, "content_block": {"type": "text", "text": ""}}),
        )?;
    }
    event(
        out,
        "content_block_delta",
        json!({"index": st.blocks, "delta": {"type": "text_delta", "text": t}}),
    )
}

/// Everything after the text: its stop, each tool_use block, `message_delta`, then `message_stop`
/// (the caller writes this only once the usage end record exists).
pub fn write_stream_tail(
    out: &mut dyn Write,
    st: &mut StreamState,
    r: &ChatResponse,
) -> std::io::Result<()> {
    if st.text_open {
        event(out, "content_block_stop", json!({"index": st.blocks}))?;
        st.text_open = false;
        st.blocks += 1;
    }
    for c in &r.tool_calls {
        let i = st.blocks;
        event(
            out,
            "content_block_start",
            json!({"index": i, "content_block": {"type": "tool_use", "id": c.id, "name": c.name, "input": {}}}),
        )?;
        event(
            out,
            "content_block_delta",
            json!({"index": i, "delta": {"type": "input_json_delta", "partial_json": c.arguments}}),
        )?;
        event(out, "content_block_stop", json!({"index": i}))?;
        st.blocks += 1;
    }
    let u = r.usage.unwrap_or_default();
    event(
        out,
        "message_delta",
        json!({"delta": {"stop_reason": stop_reason(r.finish), "stop_sequence": null},
        "usage": {"input_tokens": u.input_tokens, "output_tokens": u.output_tokens}}),
    )?;
    event(out, "message_stop", json!({}))
}

/// The route token of a Messages request: `Authorization: Bearer` or `x-api-key`.
pub fn route_token(req: &HttpReq) -> Option<&str> {
    req.header("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| req.header("x-api-key"))
        .map(str::trim)
}

/// Serve one `/v1/messages` call whose guards, route and JSON body the caller has checked.
pub fn serve(
    proxy: &ModelProxy,
    req: &HttpReq,
    token: &str,
    model: &str,
    body: &Value,
    conversation: Option<String>,
    out: &mut dyn Write,
) {
    if let Some(v) = req
        .header("anthropic-version")
        .filter(|v| *v != ANTHROPIC_VERSION)
    {
        let e = ProxyError::BadRequest(format!(
            "anthropic-version {v} is not served; this proxy speaks {ANTHROPIC_VERSION}"
        ));
        return write_json(out, 400, &error_json(&e));
    }
    let chat = match from_messages_body(body) {
        Ok(c) => c,
        Err(m) => return write_json(out, 400, &error_json(&ProxyError::BadRequest(m))),
    };
    let id = format!("msg_{:016x}", rand::random::<u64>());
    if !chat.stream {
        match proxy.call_routed_until(
            token,
            chat,
            conversation,
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        ) {
            Ok(r) => write_json(out, 200, &message_json(&id, model, &r)),
            Err(e) => write_json(out, e.http().0, &error_json(&e)),
        }
        return;
    }
    let mut st = StreamState::default();
    let (mut started, mut broken) = (false, false);
    let gone = Arc::new(AtomicBool::new(false));
    let result = proxy.call_routed_until(token, chat, conversation, gone.clone(), &mut |t| {
        if broken {
            return;
        }
        let wrote = (|| {
            if !started {
                started = true;
                stream_head(out, &id, model)?;
            }
            text_delta(out, &mut st, t)
        })();
        if wrote.is_err() {
            broken = true;
            gone.store(true, Ordering::SeqCst);
        }
    });
    match result {
        Err(e) if !started => write_json(out, e.http().0, &error_json(&e)),
        // Mid-stream failure (or no usage record): an error event and no message_stop.
        Err(e) => drop(event(out, "error", error_json(&e))),
        Ok(r) => {
            if !started {
                let _ = stream_head(out, &id, model);
            }
            let _ = write_stream_tail(out, &mut st, &r);
        }
    }
}

#[cfg(test)]
mod tests {
    include!("model_proxy_messages_tests.rs");
}
