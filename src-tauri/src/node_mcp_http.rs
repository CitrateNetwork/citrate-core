//! HUP-S4.2 — the citrate-node MCP transports.
//!
//! **Streamable HTTP on loopback.** One endpoint, `POST /mcp`, bound to 127.0.0.1 only. Every
//! request must carry `Authorization: Bearer <connect token>`; the `Host` header must name the
//! loopback address (DNS-rebinding guard); a browser `Origin` from anywhere but this loopback
//! endpoint is refused, and no CORS headers are ever sent. `initialize` opens a session
//! (`Mcp-Session-Id`), which is bound to the token that opened it. Answers are plain
//! `application/json` (the spec allows JSON or SSE; this server needs no streaming). `GET` is 405
//! (no server-initiated stream is offered) and `DELETE` ends a session. One request per
//! connection (`Connection: close`). Bounded: 16 KiB of headers, 1 MiB body, 32 concurrent
//! connections, 15 s socket timeouts and a 15 s whole-request deadline.
//!
//! **stdio shim.** `citrate-core --mcp-stdio` (see `main.rs`) reads newline-delimited JSON-RPC on
//! stdin, forwards each message to the loopback endpoint with the token from
//! `CITRATE_NODE_MCP_TOKEN`, and writes each answer as one line on stdout. Clients that only speak
//! stdio (or that cannot set headers) connect through it.

use crate::node_mcp_protocol::{CallerCtx, McpCore, INVALID_REQUEST, PARSE_ERROR};
use crate::node_mcp_token::TokenStore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The default loopback port (pending owner sign-off; overridable in the node-mcp config file and
/// by `CITRATE_NODE_MCP_PORT`).
pub const DEFAULT_PORT: u16 = 47204;
/// The MCP endpoint path.
pub const MCP_PATH: &str = "/mcp";
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_CONNECTIONS: usize = 32;
const MAX_SESSIONS: usize = 64;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(15);
/// The whole request (headers and body) must arrive within this time, so a client that trickles
/// bytes cannot hold a connection slot for longer than this plus one socket timeout.
const REQUEST_DEADLINE: Duration = Duration::from_secs(15);

/// The loopback URL for a port.
pub fn endpoint_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{MCP_PATH}")
}

#[derive(Clone)]
struct Session {
    token_id: String,
    client_name: Option<String>,
    opened_seq: u64,
}

/// Everything a connection handler needs.
pub struct ServerShared {
    pub core: Arc<McpCore>,
    pub tokens: Arc<TokenStore>,
    sessions: Mutex<(u64, HashMap<String, Session>)>,
    port: AtomicUsize,
}

impl ServerShared {
    pub fn new(core: Arc<McpCore>, tokens: Arc<TokenStore>) -> Self {
        ServerShared {
            core,
            tokens,
            sessions: Mutex::new((0, HashMap::new())),
            port: AtomicUsize::new(0),
        }
    }

    fn lock_sessions(&self) -> std::sync::MutexGuard<'_, (u64, HashMap<String, Session>)> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Drop every session opened with token `token_id` (the token was revoked).
    pub fn drop_sessions_for(&self, token_id: &str) {
        self.lock_sessions().1.retain(|_, s| s.token_id != token_id);
    }

    fn open_session(&self, token_id: &str, client_name: Option<String>) -> String {
        use rand::RngCore;
        let mut b = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut b);
        let id = hex::encode(b);
        let mut g = self.lock_sessions();
        g.0 += 1;
        let seq = g.0;
        if g.1.len() >= MAX_SESSIONS {
            if let Some(oldest) =
                g.1.iter()
                    .min_by_key(|(_, s)| s.opened_seq)
                    .map(|(k, _)| k.clone())
            {
                g.1.remove(&oldest);
            }
        }
        g.1.insert(
            id.clone(),
            Session {
                token_id: token_id.to_string(),
                client_name,
                opened_seq: seq,
            },
        );
        id
    }
}

/// A running loopback server.
pub struct RunningServer {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RunningServer {
    /// Stop accepting and join the accept thread (in-flight connections finish on their own).
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Bind `127.0.0.1:port` (0 = any free port) and serve until stopped.
pub fn start(shared: Arc<ServerShared>, port: u16) -> Result<RunningServer, String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
        format!("could not listen on 127.0.0.1:{port}: {e}. Another program may be using the port.")
    })?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    shared.port.store(addr.port() as usize, Ordering::SeqCst);
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let active = Arc::new(AtomicUsize::new(0));
    let thread = std::thread::Builder::new()
        .name("node-mcp-accept".into())
        .spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        if !peer.ip().is_loopback() {
                            continue; // bound to loopback; belt and braces
                        }
                        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                            let _ = respond_simple(stream, 503, "server busy");
                            continue;
                        }
                        active.fetch_add(1, Ordering::SeqCst);
                        let shared = shared.clone();
                        let active2 = active.clone();
                        let spawned = std::thread::Builder::new()
                            .name("node-mcp-conn".into())
                            .spawn(move || {
                                handle_connection(&shared, stream);
                                active2.fetch_sub(1, Ordering::SeqCst);
                            });
                        if spawned.is_err() {
                            active.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(40)),
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(RunningServer {
        addr,
        stop,
        thread: Some(thread),
    })
}

/// A parsed HTTP request.
#[derive(Debug, Default)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    /// Lowercased header names.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// An HTTP response to write.
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn simple(status: u16, msg: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
        body: msg.as_bytes().to_vec(),
    }
}

fn json_response(status: u16, v: &Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: v.to_string().into_bytes(),
    }
}

fn respond_simple(stream: TcpStream, status: u16, msg: &str) -> std::io::Result<()> {
    write_response(stream, &simple(status, msg))
}

fn write_response(mut stream: TcpStream, r: &HttpResponse) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", r.status, reason(r.status));
    for (k, v) in &r.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        r.body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&r.body)?;
    stream.flush()
}

/// Read one HTTP/1.1 request (bounded). `Err` carries the response to send instead.
pub fn read_request(stream: &mut impl Read) -> Result<HttpRequest, HttpResponse> {
    read_request_by(stream, std::time::Instant::now() + REQUEST_DEADLINE)
}

/// [`read_request`] with an explicit whole-request deadline (408 once it passes).
pub fn read_request_by(
    stream: &mut impl Read,
    deadline: std::time::Instant,
) -> Result<HttpRequest, HttpResponse> {
    let late = || std::time::Instant::now() >= deadline;
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err(simple(400, "headers too large"));
        }
        if late() {
            return Err(simple(408, "request not received in time"));
        }
        let n = stream
            .read(&mut chunk)
            .map_err(|_| simple(400, "could not read the request"))?;
        if n == 0 {
            return Err(simple(400, "incomplete request"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    if header_end > MAX_HEADER_BYTES {
        return Err(simple(400, "headers too large"));
    }
    let head = std::str::from_utf8(&buf[..header_end]).map_err(|_| simple(400, "bad headers"))?;
    let mut lines = head.split("\r\n");
    let start = lines.next().unwrap_or_default();
    let mut parts = start.split(' ');
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(simple(
            411,
            "send a Content-Length body (chunked bodies are not accepted)",
        ));
    }
    let len = match headers.get("content-length") {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| simple(400, "bad Content-Length"))?,
        None => 0,
    };
    if len > MAX_BODY_BYTES {
        return Err(simple(413, "body larger than 1 MiB"));
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < len {
        if late() {
            return Err(simple(408, "request not received in time"));
        }
        let n = stream
            .read(&mut chunk)
            .map_err(|_| simple(400, "could not read the body"))?;
        if n == 0 {
            return Err(simple(400, "body shorter than Content-Length"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn handle_connection(shared: &ServerShared, mut stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(SOCKET_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SOCKET_TIMEOUT));
    let resp = match read_request(&mut stream) {
        Ok(req) => handle_request(shared, &req),
        Err(r) => r,
    };
    let _ = write_response(stream, &resp);
}

/// Is `host` (a Host header value) this loopback endpoint?
fn host_ok(host: &str, port: u16) -> bool {
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ]
    .iter()
    .any(|h| h.eq_ignore_ascii_case(host))
}

/// Is `origin` (an Origin header value) this loopback endpoint? (`null` and every other site are not.)
fn origin_ok(origin: &str, port: u16) -> bool {
    [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        format!("http://[::1]:{port}"),
    ]
    .iter()
    .any(|o| o.eq_ignore_ascii_case(origin))
}

/// Apply every transport rule and dispatch. Public for tests that drive it without a socket.
pub fn handle_request(shared: &ServerShared, req: &HttpRequest) -> HttpResponse {
    let port = shared.port.load(Ordering::SeqCst) as u16;
    if req.path != MCP_PATH {
        return simple(404, "not found");
    }
    match req.headers.get("host") {
        Some(h) if host_ok(h, port) => {}
        _ => return simple(421, "this server answers on its loopback address only"),
    }
    if let Some(o) = req.headers.get("origin") {
        if !origin_ok(o, port) {
            return simple(403, "cross-origin requests are not allowed");
        }
    }
    let token = req
        .headers
        .get("authorization")
        .and_then(|a| {
            a.strip_prefix("Bearer ")
                .or_else(|| a.strip_prefix("bearer "))
        })
        .map(str::trim);
    let Some(auth) = token.and_then(|t| shared.tokens.verify(t, shared.core.now())) else {
        let mut r = simple(
            401,
            "a valid connect token is required (create one in Citrate Core: Settings, API endpoints & keys)",
        );
        r.headers.push((
            "WWW-Authenticate".into(),
            "Bearer realm=\"citrate-node\"".into(),
        ));
        return r;
    };
    if let Some(v) = req.headers.get("mcp-protocol-version") {
        if !crate::node_mcp_protocol::SUPPORTED_PROTOCOL_VERSIONS.contains(&v.as_str()) {
            return simple(400, "unsupported MCP-Protocol-Version");
        }
    }
    let session_id = req.headers.get("mcp-session-id").cloned();
    match req.method.as_str() {
        "POST" => {}
        "DELETE" => {
            let Some(sid) = session_id else {
                return simple(400, "Mcp-Session-Id required");
            };
            let mut g = shared.lock_sessions();
            return match g.1.get(&sid) {
                Some(s) if s.token_id == auth.id => {
                    g.1.remove(&sid);
                    HttpResponse {
                        status: 204,
                        headers: vec![],
                        body: vec![],
                    }
                }
                _ => simple(404, "unknown session"),
            };
        }
        _ => {
            let mut r = simple(405, "use POST");
            r.headers.push(("Allow".into(), "POST, DELETE".into()));
            return r;
        }
    }
    if let Some(ct) = req.headers.get("content-type") {
        if !ct.to_ascii_lowercase().starts_with("application/json") {
            return simple(415, "Content-Type must be application/json");
        }
    }
    if let Some(acc) = req.headers.get("accept") {
        let a = acc.to_ascii_lowercase();
        if !(a.contains("application/json") || a.contains("*/*") || a.contains("text/event-stream"))
        {
            return simple(406, "this server answers with application/json");
        }
    }
    let msg: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) => {
            return json_response(
                400,
                &json!({"jsonrpc": "2.0", "id": null, "error": {"code": PARSE_ERROR, "message": "parse error"}}),
            )
        }
    };
    if msg.is_array() {
        return json_response(
            400,
            &json!({"jsonrpc": "2.0", "id": null, "error": {"code": INVALID_REQUEST, "message": "JSON-RPC batches are not supported"}}),
        );
    }
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let is_request = msg.get("id").is_some() && !method.is_empty();

    let mut new_session: Option<String> = None;
    let ctx = if method == "initialize" && is_request {
        let client_name = msg
            .get("params")
            .and_then(|p| p.get("clientInfo"))
            .and_then(|c| c.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let sid = shared.open_session(&auth.id, client_name.clone());
        new_session = Some(sid);
        CallerCtx {
            token_id: auth.id.clone(),
            token_label: auth.label.clone(),
            client_name,
        }
    } else {
        let Some(sid) = session_id else {
            return simple(400, "Mcp-Session-Id required (send initialize first)");
        };
        let s = {
            let g = shared.lock_sessions();
            g.1.get(&sid).cloned()
        };
        match s {
            Some(s) if s.token_id == auth.id => CallerCtx {
                token_id: auth.id.clone(),
                token_label: auth.label.clone(),
                client_name: s.client_name,
            },
            _ => return simple(404, "unknown or expired session; initialize again"),
        }
    };

    match shared.core.dispatch(&ctx, &msg) {
        None => HttpResponse {
            status: 202,
            headers: vec![],
            body: vec![],
        },
        Some(reply) => {
            let mut r = json_response(200, &reply);
            if let Some(sid) = new_session {
                r.headers.push(("Mcp-Session-Id".into(), sid));
            }
            r
        }
    }
}

// ---------------------------------------------------------------------------
// stdio shim
// ---------------------------------------------------------------------------

/// The HTTP exchange the shim performs (a seam so the shim's line handling is testable against
/// the real server without a process boundary).
pub trait ShimTransport {
    /// POST `body` with the given extra headers. Returns (status, response headers lowercased, body).
    fn post(
        &self,
        body: &str,
        headers: &[(String, String)],
    ) -> Result<(u16, HashMap<String, String>, String), String>;
}

/// The production shim transport: blocking ureq to the loopback endpoint.
pub struct UreqShimTransport {
    pub url: String,
}

impl ShimTransport for UreqShimTransport {
    fn post(
        &self,
        body: &str,
        headers: &[(String, String)],
    ) -> Result<(u16, HashMap<String, String>, String), String> {
        let mut req = ureq::post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let mut resp = req
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .send(body)
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let mut h = HashMap::new();
        for (k, v) in resp.headers() {
            if let Ok(s) = v.to_str() {
                h.insert(k.as_str().to_ascii_lowercase(), s.to_string());
            }
        }
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        Ok((status, h, text))
    }
}

/// Run the shim: one JSON-RPC message per input line, one answer per output line. Returns the
/// process exit code.
pub fn run_stdio_shim(
    input: impl BufRead,
    mut output: impl Write,
    transport: &dyn ShimTransport,
    token: &str,
) -> i32 {
    let mut session: Option<String> = None;
    for line in input.lines() {
        let Ok(line) = line else {
            return 1;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Option<Value> = serde_json::from_str(line).ok();
        let id = parsed.as_ref().and_then(|v| v.get("id").cloned());
        let mut headers = vec![("Authorization".to_string(), format!("Bearer {token}"))];
        if let Some(s) = &session {
            headers.push(("Mcp-Session-Id".to_string(), s.clone()));
        }
        let answer: Option<Value> = match transport.post(line, &headers) {
            Ok((200, h, body)) => {
                if let Some(s) = h.get("mcp-session-id") {
                    session = Some(s.clone());
                }
                serde_json::from_str::<Value>(&body).ok()
            }
            Ok((202, _, _)) => None,
            Ok((status, _, body)) => id.map(|id| {
                let detail: String = body.chars().take(300).collect();
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000,
                    "message": format!("citrate-node MCP server answered HTTP {status}: {detail}")}})
            }),
            Err(e) => id.map(|id| {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000,
                    "message": format!("Citrate Core is not running or its MCP server is off ({e})")}})
            }),
        };
        if let Some(a) = answer {
            if writeln!(output, "{a}").is_err() || output.flush().is_err() {
                return 1;
            }
        }
    }
    0
}

/// The shim only ever sends the connect token to this machine's loopback endpoint.
pub fn shim_url_is_loopback(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else {
        return false;
    };
    u.scheme() == "http"
        && u.username().is_empty()
        && u.password().is_none()
        && matches!(
            u.host(),
            Some(url::Host::Domain("localhost"))
                | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
                | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        )
}

/// The endpoint the shim posts to: `CITRATE_NODE_MCP_URL` if set (loopback only), else the default
/// endpoint on `CITRATE_NODE_MCP_PORT` (or [`DEFAULT_PORT`]).
pub fn resolve_shim_url(
    url_env: Option<String>,
    port_env: Option<String>,
) -> Result<String, String> {
    let url = match url_env.filter(|u| !u.trim().is_empty()) {
        Some(u) => u.trim().to_string(),
        None => {
            let port = match port_env {
                Some(p) => p
                    .trim()
                    .parse::<u16>()
                    .map_err(|_| format!("CITRATE_NODE_MCP_PORT is not a port number: {p}"))?,
                None => DEFAULT_PORT,
            };
            endpoint_url(port)
        }
    };
    if !shim_url_is_loopback(&url) {
        return Err("CITRATE_NODE_MCP_URL must be an http://127.0.0.1 or http://localhost address; the connect token is never sent anywhere else.".to_string());
    }
    Ok(url)
}

/// `citrate-core --mcp-stdio`: the shim's entry point (no GUI is started).
pub fn stdio_main() -> i32 {
    let Ok(token) = std::env::var("CITRATE_NODE_MCP_TOKEN") else {
        eprintln!("citrate-core --mcp-stdio: set CITRATE_NODE_MCP_TOKEN to a connect token from Settings, API endpoints & keys.");
        return 2;
    };
    let url = match resolve_shim_url(
        std::env::var("CITRATE_NODE_MCP_URL").ok(),
        std::env::var("CITRATE_NODE_MCP_PORT").ok(),
    ) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("citrate-core --mcp-stdio: {e}");
            return 2;
        }
    };
    let transport = UreqShimTransport { url };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_stdio_shim(stdin.lock(), stdout.lock(), &transport, token.trim())
}
