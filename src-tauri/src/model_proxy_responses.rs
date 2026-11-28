//! L1-WP10 (v060) — `POST /v1/responses` on the loopback model proxy, for agents that speak the
//! OpenAI Responses API (Codex). Planset v060 `lanes/L1_AGENT_RUNTIME.md` WP10.
//!
//! The Responses body is translated to the proxy's internal [`ChatRequest`] and runs the same
//! seven steps as `/v1/chat/completions` (resolve, egress, guard, begin, key, call, end). The route
//! binds provider and model; a body may only repeat the route's model. The proxy is stateless:
//! `previous_response_id`, `conversation` and `store: true` are refused, so an agent sends its full
//! input each turn. `reasoning` input items (a vendor's opaque state) are dropped for the same
//! reason. Only `function` tools and text content are carried.
//!
//! Which path reaches the vendor: always this translation. For OpenAI the adapter then calls the
//! vendor's own Responses endpoint on its pinned host (`provider_openai.rs`); every other vendor
//! gets its own wire from its adapter.
//!
//! A streamed answer is server-sent events typed as the Responses API types them; the final
//! `response.completed` is written only after the call's usage end record is.

use crate::model_provider::{
    ChatMessage, ChatRequest, ChatResponse, FinishReason, Role, ToolCall, ToolDef,
};
use crate::model_proxy::{ModelProxy, ProxyError};
use crate::model_proxy_http::{error_body, write_json};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const STATELESS: &str = "stateless proxy: send full input";

fn text_of(content: Option<&Value>) -> Result<String, String> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for p in parts {
                match (
                    p.get("type").and_then(Value::as_str),
                    p.get("text").and_then(Value::as_str),
                ) {
                    (Some("input_text" | "output_text" | "text"), Some(t)) => out.push_str(t),
                    _ => return Err("only text content parts are supported".into()),
                }
            }
            Ok(out)
        }
        Some(_) => Err("content must be text".into()),
    }
}

fn input_item(messages: &mut Vec<ChatMessage>, item: &Value) -> Result<(), String> {
    let s = |k: &str| item.get(k).and_then(Value::as_str);
    match s("type").unwrap_or("message") {
        "message" => messages.push(ChatMessage {
            role: s("role")
                .and_then(Role::parse)
                .ok_or("a message needs a role of system, developer, user or assistant")?,
            content: text_of(item.get("content"))?,
            tool_calls: vec![],
            tool_call_id: None,
        }),
        "function_call" => {
            let call = ToolCall {
                id: s("call_id")
                    .ok_or("a function_call needs a call_id")?
                    .to_string(),
                name: s("name").ok_or("a function_call needs a name")?.to_string(),
                arguments: s("arguments").unwrap_or("{}").to_string(),
            };
            match messages.last_mut() {
                Some(m) if m.role == Role::Assistant && m.tool_call_id.is_none() => {
                    m.tool_calls.push(call)
                }
                _ => messages.push(ChatMessage {
                    role: Role::Assistant,
                    content: String::new(),
                    tool_calls: vec![call],
                    tool_call_id: None,
                }),
            }
        }
        "function_call_output" => messages.push(ChatMessage {
            role: Role::Tool,
            content: text_of(item.get("output"))?,
            tool_calls: vec![],
            tool_call_id: Some(
                s("call_id")
                    .ok_or("a function_call_output needs a call_id")?
                    .to_string(),
            ),
        }),
        "reasoning" => {}
        other => return Err(format!("input items of type {other} are not supported")),
    }
    Ok(())
}

/// A Responses API body → the internal request.
pub fn from_responses_body(v: &Value) -> Result<ChatRequest, String> {
    let set = |k: &str| v.get(k).is_some_and(|x| !x.is_null());
    if set("previous_response_id") || set("conversation") || v.get("store") == Some(&json!(true)) {
        return Err(STATELESS.into());
    }
    let mut messages = Vec::new();
    if let Some(i) = v
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|i| !i.is_empty())
    {
        messages.push(ChatMessage {
            role: Role::System,
            content: i.to_string(),
            tool_calls: vec![],
            tool_call_id: None,
        });
    }
    match v.get("input") {
        Some(Value::String(t)) => messages.push(ChatMessage {
            role: Role::User,
            content: t.clone(),
            tool_calls: vec![],
            tool_call_id: None,
        }),
        Some(Value::Array(items)) if !items.is_empty() && items.len() <= 4096 => {
            for item in items {
                input_item(&mut messages, item)?;
            }
        }
        _ => return Err("input must be text or an array of 1 to 4096 items".into()),
    }
    if messages.is_empty() {
        return Err("input holds no message".into());
    }
    let mut tools = Vec::new();
    for t in v
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match t.get("type").and_then(Value::as_str) {
            Some("function") => tools.push(ToolDef {
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
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
            }),
            other => {
                return Err(format!(
                    "only function tools are supported, not {}",
                    other.unwrap_or("untyped")
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
            .get("max_output_tokens")
            .and_then(Value::as_u64)
            .map(|n| n.min(u64::from(crate::model_proxy::MAX_OUTPUT_TOKENS)) as u32)
            .unwrap_or(0),
        stream: v.get("stream").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn message_item(text: &str, status: &str) -> Value {
    json!({"type": "message", "id": "msg_0", "role": "assistant", "status": status,
           "content": [{"type": "output_text", "text": text, "annotations": []}]})
}

fn call_item(i: usize, c: &ToolCall, status: &str, arguments: &str) -> Value {
    json!({"type": "function_call", "id": format!("fc_{i}"), "call_id": c.id, "name": c.name,
           "arguments": arguments, "status": status})
}

/// The whole answer as a Responses API `response` object.
pub fn response_json(id: &str, model: &str, r: &ChatResponse) -> Value {
    let mut output = Vec::new();
    if !r.content.is_empty() || r.tool_calls.is_empty() {
        output.push(message_item(&r.content, "completed"));
    }
    for (i, c) in r.tool_calls.iter().enumerate() {
        output.push(call_item(i, c, "completed", &c.arguments));
    }
    let incomplete = match r.finish {
        FinishReason::Length => Some("max_output_tokens"),
        FinishReason::ContentFilter => Some("content_filter"),
        _ => None,
    };
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    json!({
        "id": id, "object": "response", "created_at": created, "model": model,
        "status": if incomplete.is_some() { "incomplete" } else { "completed" },
        "incomplete_details": incomplete.map(|reason| json!({"reason": reason})),
        "output": output,
        "usage": r.usage.map(|u| json!({"input_tokens": u.input_tokens, "output_tokens": u.output_tokens, "total_tokens": u.total()})),
    })
}

/// What a streamed answer has written so far.
#[derive(Default)]
pub struct StreamState {
    seq: u64,
    text_open: bool,
    text: String,
}

fn event(
    out: &mut dyn Write,
    st: &mut StreamState,
    kind: &str,
    mut data: Value,
) -> std::io::Result<()> {
    data["type"] = json!(kind);
    data["sequence_number"] = json!(st.seq);
    st.seq += 1;
    write!(out, "event: {kind}\ndata: {data}\n\n")?;
    out.flush()
}

fn stream_head(
    out: &mut dyn Write,
    st: &mut StreamState,
    id: &str,
    model: &str,
) -> std::io::Result<()> {
    write!(out, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n")?;
    let r = json!({"id": id, "object": "response", "model": model, "status": "in_progress", "output": []});
    event(out, st, "response.created", json!({"response": r}))
}

fn text_delta(out: &mut dyn Write, st: &mut StreamState, t: &str) -> std::io::Result<()> {
    if !st.text_open {
        st.text_open = true;
        event(
            out,
            st,
            "response.output_item.added",
            json!({"output_index": 0, "item": message_item("", "in_progress")}),
        )?;
        event(
            out,
            st,
            "response.content_part.added",
            json!({"item_id": "msg_0", "output_index": 0, "content_index": 0,
            "part": {"type": "output_text", "text": "", "annotations": []}}),
        )?;
    }
    st.text.push_str(t);
    event(
        out,
        st,
        "response.output_text.delta",
        json!({"item_id": "msg_0", "output_index": 0, "content_index": 0, "delta": t}),
    )
}

/// Everything after the text: the text's done events, each function call, then
/// `response.completed` (the caller writes this only once the usage end record exists).
pub fn write_stream_tail(
    out: &mut dyn Write,
    st: &mut StreamState,
    id: &str,
    model: &str,
    r: &ChatResponse,
) -> std::io::Result<()> {
    let mut index = 0;
    if st.text_open {
        let text = std::mem::take(&mut st.text);
        event(
            out,
            st,
            "response.output_text.done",
            json!({"item_id": "msg_0", "output_index": 0, "content_index": 0, "text": text}),
        )?;
        event(
            out,
            st,
            "response.content_part.done",
            json!({"item_id": "msg_0", "output_index": 0, "content_index": 0,
            "part": {"type": "output_text", "text": text, "annotations": []}}),
        )?;
        event(
            out,
            st,
            "response.output_item.done",
            json!({"output_index": 0, "item": message_item(&text, "completed")}),
        )?;
        index = 1;
    }
    for (i, c) in r.tool_calls.iter().enumerate() {
        let (oi, item_id) = (index + i, format!("fc_{i}"));
        event(
            out,
            st,
            "response.output_item.added",
            json!({"output_index": oi, "item": call_item(i, c, "in_progress", "")}),
        )?;
        event(
            out,
            st,
            "response.function_call_arguments.delta",
            json!({"item_id": item_id, "output_index": oi, "delta": c.arguments}),
        )?;
        event(
            out,
            st,
            "response.function_call_arguments.done",
            json!({"item_id": item_id, "output_index": oi, "arguments": c.arguments}),
        )?;
        event(
            out,
            st,
            "response.output_item.done",
            json!({"output_index": oi, "item": call_item(i, c, "completed", &c.arguments)}),
        )?;
    }
    event(
        out,
        st,
        "response.completed",
        json!({"response": response_json(id, model, r)}),
    )
}

/// Serve one `/v1/responses` call whose guards, route and JSON body the caller has checked.
pub fn serve(
    proxy: &ModelProxy,
    token: &str,
    model: &str,
    body: &Value,
    conversation: Option<String>,
    out: &mut dyn Write,
) {
    let req = match from_responses_body(body) {
        Ok(r) => r,
        Err(m) => return write_json(out, 400, &error_body(&ProxyError::BadRequest(m))),
    };
    let id = format!("resp_{:016x}", rand::random::<u64>());
    if !req.stream {
        match proxy.call_routed_until(
            token,
            req,
            conversation,
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        ) {
            Ok(r) => write_json(out, 200, &response_json(&id, model, &r)),
            Err(e) => write_json(out, e.http().0, &error_body(&e)),
        }
        return;
    }
    let mut st = StreamState::default();
    let (mut started, mut broken) = (false, false);
    let gone = Arc::new(AtomicBool::new(false));
    let result = proxy.call_routed_until(token, req, conversation, gone.clone(), &mut |t| {
        if broken {
            return;
        }
        let wrote = (|| {
            if !started {
                started = true;
                stream_head(out, &mut st, &id, model)?;
            }
            text_delta(out, &mut st, t)
        })();
        if wrote.is_err() {
            broken = true;
            gone.store(true, Ordering::SeqCst);
        }
    });
    match result {
        Err(e) if !started => write_json(out, e.http().0, &error_body(&e)),
        Err(e) => {
            // Mid-stream failure (or the usage record could not be written): an error event and no
            // response.completed, so the agent cannot take a partial answer for a whole one.
            let (_, kind) = e.http();
            let _ = event(
                out,
                &mut st,
                "error",
                json!({"code": kind, "message": e.to_string()}),
            );
        }
        Ok(r) => {
            if !started {
                let _ = stream_head(out, &mut st, &id, model);
            }
            let _ = write_stream_tail(out, &mut st, &id, model, &r);
        }
    }
}

#[cfg(test)]
mod tests {
    include!("model_proxy_responses_tests.rs");
}
