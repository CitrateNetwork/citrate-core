//! D-11 (RC52) — the loopback endpoint of the core model proxy (ADR P-6).
//!
//! `POST /v1/chat/completions` on `127.0.0.1:<ephemeral port>`, OpenAI chat-completions shape, so the
//! keyless sidecar's existing OpenAI-compatible client works unchanged: core hands it
//! `{ baseUrl: "http://127.0.0.1:<port>/v1", bearer: <route token> }` and the route token binds the
//! provider, model and session. The sidecar never holds a vendor key.
//!
//! Guards: `Authorization: Bearer <route token>` on every request; the `Host` header must name this
//! loopback endpoint (DNS-rebinding guard); any `Origin` header is refused (no browser may call it,
//! no CORS headers are sent); one request per connection; bounded headers (16 KiB), body (4 MiB),
//! connections (16) and a 15 s deadline for the whole request (headers and body), not per read. `stream: true` answers server-sent events; the
//! final `data: [DONE]` is written only after the usage record is.

use crate::model_provider::{ChatMessage, ChatRequest, ChatResponse, Role, ToolCall, ToolDef};
use crate::model_proxy::{ModelProxy, ProxyError};
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const CHAT_PATH: &str = "/v1/chat/completions";
/// Header naming the conversation a call belongs to (recorded in the usage log).
pub const CONVERSATION_HEADER: &str = "x-citrate-conversation";
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 16;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(15);

/// A parsed request.
#[derive(Debug, Default)]
pub struct HttpReq {
    pub method: String,
    pub path: String,
    /// Lower-cased names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpReq {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Read one request from a stream (bounded), optionally under a whole-request deadline.
fn read_request_inner(
    stream: &mut dyn Read,
    deadline: Option<std::time::Instant>,
) -> Result<HttpReq, (u16, &'static str)> {
    let late = || deadline.is_some_and(|d| std::time::Instant::now() >= d);
    let timed_out = |e: &std::io::Error| {
        matches!(
            e.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        )
    };
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    let mut total = 0usize;
    let mut next_line = |reader: &mut std::io::BufReader<&mut dyn Read>, line: &mut String| {
        line.clear();
        if late() {
            return Err((408, "request timeout"));
        }
        let n = reader
            .by_ref()
            .take((MAX_HEADER_BYTES + 1) as u64)
            .read_line(line)
            .map_err(|e| {
                if timed_out(&e) {
                    (408, "request timeout")
                } else {
                    (400, "bad request")
                }
            })?;
        total += n;
        if total > MAX_HEADER_BYTES {
            return Err((431, "headers too large"));
        }
        Ok(n)
    };
    next_line(&mut reader, &mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut req = HttpReq {
        method,
        path,
        ..HttpReq::default()
    };
    loop {
        if next_line(&mut reader, &mut line)? == 0 {
            return Err((400, "bad request"));
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        let Some((k, v)) = l.split_once(':') else {
            return Err((400, "bad header"));
        };
        req.headers
            .push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let len = match req.header("content-length") {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| (400, "bad content-length"))?,
        None => 0,
    };
    if len > MAX_BODY_BYTES {
        return Err((413, "body too large"));
    }
    if req.header("transfer-encoding").is_some() {
        return Err((411, "content-length required"));
    }
    req.body = vec![0u8; len];
    if late() {
        return Err((408, "request timeout"));
    }
    reader.read_exact(&mut req.body).map_err(|e| {
        if timed_out(&e) {
            (408, "request timeout")
        } else {
            (400, "body cut off")
        }
    })?;
    Ok(req)
}

/// A socket whose every read is bounded by what is left of one whole-request deadline.
struct DeadlineStream<'a> {
    stream: &'a TcpStream,
    deadline: std::time::Instant,
}

impl Read for DeadlineStream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self
            .deadline
            .saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        (&mut &*self.stream).read(buf)
    }
}

/// [`read_request`] under a whole-request deadline (headers and body), not only per read.
pub fn read_request_until(
    stream: &mut dyn Read,
    deadline: std::time::Instant,
) -> Result<HttpReq, (u16, &'static str)> {
    read_request_inner(stream, Some(deadline))
}

/// Host and Origin guards. The `Host` must be the loopback endpoint at `port`; any `Origin` is
/// refused.
pub fn request_allowed(req: &HttpReq, port: u16) -> Result<(), (u16, &'static str)> {
    if req.header("origin").is_some() {
        return Err((403, "browser requests are not accepted"));
    }
    let host = req.header("host").unwrap_or("");
    let ok = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .iter()
    .any(|h| h.eq_ignore_ascii_case(host));
    if !ok {
        return Err((403, "the Host header must name this loopback endpoint"));
    }
    Ok(())
}

/// The bearer from `Authorization: Bearer …`.
fn bearer(req: &HttpReq) -> Option<&str> {
    let v = req.header("authorization")?;
    let (scheme, tok) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| tok.trim())
}

/// `content` as text: a string, null, or an array of `{type: "text", text}` parts.
fn content_text(v: Option<&Value>) -> Result<String, String> {
    match v {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for p in parts {
                match (
                    p.get("type").and_then(Value::as_str),
                    p.get("text").and_then(Value::as_str),
                ) {
                    (Some("text"), Some(t)) => out.push_str(t),
                    _ => return Err("only text content parts are supported".into()),
                }
            }
            Ok(out)
        }
        Some(_) => Err("message content must be text".into()),
    }
}

/// An OpenAI chat-completions body → the internal request.
pub fn from_openai_body(v: &Value) -> Result<ChatRequest, String> {
    let model = v
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let msgs = v
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("messages must be an array")?;
    if msgs.is_empty() || msgs.len() > 4096 {
        return Err("messages must hold 1 to 4096 entries".into());
    }
    let mut messages = Vec::with_capacity(msgs.len());
    for m in msgs {
        let role = m
            .get("role")
            .and_then(Value::as_str)
            .and_then(Role::parse)
            .ok_or("each message needs a role of system, user, assistant or tool")?;
        let mut tool_calls = Vec::new();
        for (i, tc) in m
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let f = tc.get("function").ok_or("a tool call needs a function")?;
            tool_calls.push(ToolCall {
                id: tc
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("call_{i}")),
                name: f
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("a tool call needs a name")?
                    .to_string(),
                arguments: match f.get("arguments") {
                    Some(Value::String(s)) => s.clone(),
                    None | Some(Value::Null) => "{}".into(),
                    Some(o) => o.to_string(),
                },
            });
        }
        messages.push(ChatMessage {
            role,
            content: content_text(m.get("content"))?,
            tool_calls,
            tool_call_id: m
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    let mut tools = Vec::new();
    for t in v
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let f = t.get("function").ok_or("a tool needs a function")?;
        tools.push(ToolDef {
            name: f
                .get("name")
                .and_then(Value::as_str)
                .ok_or("a tool needs a name")?
                .to_string(),
            description: f
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            parameters: f
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
        });
    }
    let max_tokens = v
        .get("max_completion_tokens")
        .or_else(|| v.get("max_tokens"))
        .and_then(Value::as_u64)
        .map(|n| n.min(u64::from(crate::model_proxy::MAX_OUTPUT_TOKENS)) as u32)
        .unwrap_or(0);
    Ok(ChatRequest {
        model,
        messages,
        tools,
        max_tokens,
        stream: v.get("stream").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn tool_calls_json(calls: &[ToolCall], with_index: bool) -> Value {
    Value::Array(
        calls
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut o = json!({"id": c.id, "type": "function",
                    "function": {"name": c.name, "arguments": c.arguments}});
                if with_index {
                    o["index"] = json!(i);
                }
                o
            })
            .collect(),
    )
}

fn usage_json(r: &ChatResponse) -> Option<Value> {
    r.usage.map(|u| {
        json!({"prompt_tokens": u.input_tokens, "completion_tokens": u.output_tokens,
               "total_tokens": u.total()})
    })
}

/// The non-streamed answer.
pub fn completion_json(model: &str, r: &ChatResponse) -> Value {
    let mut msg = json!({"role": "assistant", "content": r.content});
    if !r.tool_calls.is_empty() {
        msg["tool_calls"] = tool_calls_json(&r.tool_calls, false);
    }
    let mut out = json!({
        "id": "chatcmpl-citrate-proxy",
        "object": "chat.completion",
        "model": model,
        "choices": [{"index": 0, "message": msg, "finish_reason": r.finish.as_openai()}],
    });
    if let Some(u) = usage_json(r) {
        out["usage"] = u;
    }
    out
}

fn error_body(e: &ProxyError) -> Value {
    let (_, kind) = e.http();
    json!({"error": {"type": kind, "message": e.to_string()}})
}

fn write_json(out: &mut dyn Write, status: u16, body: &Value) {
    let b = body.to_string();
    let _ = write!(
        out,
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{b}",
        reason(status),
        b.len()
    );
    let _ = out.flush();
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        423 => "Locked",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn sse(out: &mut dyn Write, v: &Value) -> std::io::Result<()> {
    write!(out, "data: {v}\n\n")?;
    out.flush()
}

/// Serve one parsed request, writing the whole HTTP answer to `out`.
pub fn serve_one(proxy: &ModelProxy, req: &HttpReq, port: u16, out: &mut dyn Write) {
    if let Err((status, msg)) = request_allowed(req, port) {
        return write_json(
            out,
            status,
            &json!({"error": {"type": "forbidden", "message": msg}}),
        );
    }
    if req.path != CHAT_PATH {
        return write_json(
            out,
            404,
            &json!({"error": {"type": "not_found", "message": "not found"}}),
        );
    }
    if req.method != "POST" {
        return write_json(
            out,
            405,
            &json!({"error": {"type": "method", "message": "POST only"}}),
        );
    }
    let Some((token, route)) = bearer(req).and_then(|b| proxy.route_for(b).map(|r| (b, r))) else {
        return write_json(out, 401, &error_body(&ProxyError::Unauthorized));
    };
    let body: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) => {
            return write_json(
                out,
                400,
                &error_body(&ProxyError::BadRequest("the body is not JSON".into())),
            )
        }
    };
    let chat = match from_openai_body(&body) {
        Ok(c) => c,
        Err(m) => return write_json(out, 400, &error_body(&ProxyError::BadRequest(m))),
    };
    let conversation = req.header(CONVERSATION_HEADER).map(str::to_string);
    let model = route.model.clone();
    if !chat.stream {
        match proxy.call_routed_until(
            token,
            chat,
            conversation,
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
        ) {
            Ok(r) => write_json(out, 200, &completion_json(&model, &r)),
            Err(e) => write_json(out, e.http().0, &error_body(&e)),
        }
        return;
    }
    // Streamed: refusals are plain error statuses (nothing streamed yet).
    let mut started = false;
    let mut broken = false;
    // Set when the client's socket breaks: the vendor call is then cut off at its next read.
    let gone = Arc::new(AtomicBool::new(false));
    let result = proxy.call_routed_until(token, chat, conversation, gone.clone(), &mut |t| {
        if broken {
            return;
        }
        if !started {
            started = true;
            if write!(
                out,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
            )
            .is_err()
            {
                broken = true;
                gone.store(true, Ordering::SeqCst);
                return;
            }
        }
        if sse(out, &json!({"choices": [{"index": 0, "delta": {"content": t}}]})).is_err() {
            broken = true;
            gone.store(true, Ordering::SeqCst);
        }
    });
    match result {
        Err(e) if !started => write_json(out, e.http().0, &error_body(&e)),
        Err(e) => {
            // Mid-stream failure (or the usage record could not be written): an error chunk and
            // no `[DONE]`, so the client cannot take a partial answer for a whole one.
            let _ = sse(out, &error_body(&e));
        }
        Ok(r) => {
            if !started {
                let _ = write!(
                    out,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
                );
            }
            if !r.tool_calls.is_empty() {
                let _ = sse(
                    out,
                    &json!({"choices": [{"index": 0, "delta": {"tool_calls": tool_calls_json(&r.tool_calls, true)}}]}),
                );
            }
            let _ = sse(
                out,
                &json!({"choices": [{"index": 0, "delta": {}, "finish_reason": r.finish.as_openai()}]}),
            );
            if let Some(u) = usage_json(&r) {
                let _ = sse(out, &json!({"choices": [], "usage": u}));
            }
            let _ = write!(out, "data: [DONE]\n\n");
            let _ = out.flush();
        }
    }
}

fn handle(proxy: &ModelProxy, mut stream: TcpStream, port: u16) {
    let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
    let deadline = std::time::Instant::now() + SOCKET_TIMEOUT;
    let read = read_request_until(
        &mut DeadlineStream {
            stream: &stream,
            deadline,
        },
        deadline,
    );
    let req = match read {
        Ok(r) => r,
        Err((status, msg)) => {
            return write_json(
                &mut stream,
                status,
                &json!({"error": {"type": "bad_request", "message": msg}}),
            )
        }
    };
    serve_one(proxy, &req, port, &mut stream);
}

/// Bind `127.0.0.1:0` and serve on a background thread. Returns the port.
pub fn start(proxy: Arc<ModelProxy>) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    proxy.set_port(port);
    let live = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("model-proxy".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(stream) = conn else { continue };
                if live.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    let mut s = stream;
                    write_json(
                        &mut s,
                        503,
                        &json!({"error": {"type": "busy", "message": "busy"}}),
                    );
                    continue;
                }
                live.fetch_add(1, Ordering::SeqCst);
                let (p, l) = (proxy.clone(), live.clone());
                let spawned = std::thread::Builder::new()
                    .name("model-proxy-conn".into())
                    .spawn(move || {
                        handle(&p, stream, port);
                        l.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    live.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })?;
    Ok(port)
}

#[cfg(test)]
mod tests {
    include!("model_proxy_http_tests.rs");
}
