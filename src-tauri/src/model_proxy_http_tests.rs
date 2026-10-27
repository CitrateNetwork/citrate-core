// D-11 — the loopback endpoint: who may call it, the OpenAI wire both ways, and that a streamed
// answer is only complete (`[DONE]`) once the usage record is written.

use super::*;
use crate::model_proxy::tests::{Rig, PROMPT};
use crate::provider_usage_log::log_path;

fn chat_body(model: &str, stream: bool) -> Vec<u8> {
    json!({"model": model, "stream": stream, "max_tokens": 50,
           "messages": [{"role": "system", "content": "be brief"},
                        {"role": "user", "content": [{"type": "text", "text": PROMPT}]}]})
    .to_string()
    .into_bytes()
}

fn post(bearer: &str, body: Vec<u8>, extra: &[(&str, &str)]) -> HttpReq {
    let mut headers = vec![
        ("host".to_string(), "127.0.0.1:1".to_string()),
        ("authorization".to_string(), format!("Bearer {bearer}")),
        ("content-type".to_string(), "application/json".to_string()),
    ];
    for (k, v) in extra {
        headers.retain(|(h, _)| h != k);
        headers.push((k.to_string(), v.to_string()));
    }
    HttpReq {
        method: "POST".into(),
        path: CHAT_PATH.into(),
        headers,
        body,
    }
}

fn serve(rig: &Rig, req: &HttpReq) -> String {
    let mut out = Vec::new();
    serve_one(&rig.proxy, req, 1, &mut out);
    String::from_utf8(out).unwrap()
}

fn status_of(resp: &str) -> u16 {
    resp.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn a_browser_origin_or_a_foreign_host_is_refused() {
    let rig = Rig::new("http-origin");
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
    let b = g.bearer.expose();
    let before = rig.vendor.calls();
    for extra in [
        vec![("origin", "https://evil.example")],
        vec![("origin", "null")],
        vec![("host", "evil.example:1")],
        vec![("host", "127.0.0.1:2")],
        vec![("host", "127.0.0.1.evil.example:1")],
    ] {
        let resp = serve(&rig, &post(b, chat_body("gpt-known", false), &extra));
        assert_eq!(status_of(&resp), 403, "{extra:?}: {resp}");
        assert!(!resp.to_ascii_lowercase().contains("access-control-allow"), "no CORS");
    }
    let mut no_host = post(b, chat_body("gpt-known", false), &[]);
    no_host.headers.retain(|(k, _)| k != "host");
    assert_eq!(status_of(&serve(&rig, &no_host)), 403);
    assert_eq!(rig.vendor.calls(), before);
    // localhost spelling is fine.
    let ok = serve(&rig, &post(b, chat_body("gpt-known", false), &[("host", "localhost:1")]));
    assert_eq!(status_of(&ok), 200, "{ok}");
}

#[test]
fn a_missing_or_unknown_bearer_is_401() {
    let rig = Rig::new("http-bearer");
    assert_eq!(status_of(&serve(&rig, &post("cmp_nope", chat_body("gpt-known", false), &[]))), 401);
    let mut none = post("x", chat_body("gpt-known", false), &[]);
    none.headers.retain(|(k, _)| k != "authorization");
    assert_eq!(status_of(&serve(&rig, &none)), 401);
    // The vendor key itself is not a route.
    assert_eq!(status_of(&serve(&rig, &post(&rig.key, chat_body("gpt-known", false), &[]))), 401);
}

#[test]
fn a_plain_answer_is_openai_shaped_and_carries_no_key() {
    let rig = Rig::new("http-plain");
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), chat_body("gpt-known", false), &[]));
    assert_eq!(status_of(&resp), 200, "{resp}");
    let body: Value = serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "hello from the vendor");
    assert_eq!(body["usage"]["prompt_tokens"], 12);
    assert_eq!(body["usage"]["completion_tokens"], 5);
    assert!(!resp.contains(&rig.key), "no key travels back to the sidecar");
    // The system message reached the vendor; the conversation header lands in the log.
    let resp = serve(
        &rig,
        &post(g.bearer.expose(), chat_body("gpt-known", false), &[(CONVERSATION_HEADER, "c-42")]),
    );
    assert_eq!(status_of(&resp), 200);
    assert!(std::fs::read_to_string(log_path(&rig.dir.0)).unwrap_or_default().contains("\"c-42\""));
}

#[test]
fn a_streamed_answer_ends_with_done_only_after_the_usage_record() {
    let rig = Rig::new("http-stream");
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), chat_body("gpt-known", true), &[]));
    assert_eq!(status_of(&resp), 200, "{resp}");
    assert!(resp.contains("text/event-stream"));
    assert!(resp.contains("\"content\":\"hello \""), "{resp}");
    assert!(resp.trim_end().ends_with("data: [DONE]"), "{resp}");
    assert!(resp.contains("\"prompt_tokens\":12"));
    let log = std::fs::read_to_string(log_path(&rig.dir.0)).unwrap_or_default();
    assert!(log.contains("\"end\""), "the end record exists once [DONE] was sent");
    assert!(!resp.contains(&rig.key));
}

#[test]
fn a_stream_whose_record_cannot_be_written_never_says_done() {
    let rig = Rig::new("http-stream-nolog");
    let p = log_path(&rig.dir.0);
    *rig.vendor.during_call.lock().unwrap() = Some(Box::new(move || {
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::create_dir_all(&p);
    }));
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), chat_body("gpt-known", true), &[]));
    assert!(!resp.contains("[DONE]"), "{resp}");
    assert!(resp.contains("usage_log_unavailable"), "{resp}");
}

#[test]
fn a_refused_stream_is_a_plain_error_status() {
    let rig = Rig::new("http-refused");
    rig.proxy.with_state(|g, _| g.member_halt("openai", 1));
    let _ = rig.proxy.with_state(|g, _| rig.proxy.save_guard(&mut *g));
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), chat_body("gpt-known", true), &[]));
    assert_eq!(status_of(&resp), 423, "{resp}");
    assert!(resp.contains("provider_halted"));
    assert!(!resp.contains("text/event-stream"));
}

#[test]
fn the_openai_body_maps_to_the_internal_request() {
    let v = json!({
        "model": "m", "stream": true, "max_completion_tokens": 9_999_999,
        "messages": [
            {"role": "developer", "content": "sys"},
            {"role": "user", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]},
            {"role": "assistant", "content": null, "tool_calls": [{"id": "t", "type": "function",
                "function": {"name": "f", "arguments": {"x": 1}}}]},
            {"role": "tool", "tool_call_id": "t", "content": "r"}
        ],
        "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}]
    });
    let r = from_openai_body(&v).unwrap();
    assert_eq!(r.messages[0].role, Role::System);
    assert_eq!(r.messages[1].content, "ab");
    assert_eq!(r.messages[2].tool_calls[0].arguments, "{\"x\":1}");
    assert_eq!(r.messages[3].tool_call_id.as_deref(), Some("t"));
    assert_eq!(r.tools[0].name, "f");
    assert_eq!(r.max_tokens, crate::model_proxy::MAX_OUTPUT_TOKENS);
    assert!(r.stream);
    for bad in [
        json!({"messages": []}),
        json!({"messages": [{"role": "wizard", "content": "x"}]}),
        json!({"messages": [{"role": "user", "content": [{"type": "image_url"}]}]}),
        json!({"messages": "x"}),
    ] {
        assert!(from_openai_body(&bad).is_err(), "{bad}");
    }
}

#[test]
fn requests_are_bounded() {
    let big = format!(
        "POST {CHAT_PATH} HTTP/1.1\r\nHost: 127.0.0.1:1\r\nContent-Length: {}\r\n\r\n",
        MAX_BODY_BYTES + 1
    );
    assert_eq!(read_soon(&mut big.as_bytes()).unwrap_err().0, 413);
    let chunked = format!("POST {CHAT_PATH} HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n");
    assert_eq!(read_soon(&mut chunked.as_bytes()).unwrap_err().0, 411);
    let huge_headers = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEADER_BYTES));
    assert_eq!(read_soon(&mut huge_headers.as_bytes()).unwrap_err().0, 431);
}

#[test]
fn the_server_binds_loopback_and_answers_over_a_socket() {
    use std::io::{Read as _, Write as _};
    let rig = Rig::new("http-socket");
    let port = start(rig.proxy.clone()).unwrap();
    assert_ne!(port, 0);
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
    assert_eq!(g.base_url, format!("http://127.0.0.1:{port}/v1"));
    let body = chat_body("gpt-known", false);
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "POST {CHAT_PATH} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        g.bearer.expose(),
        body.len()
    )
    .unwrap();
    s.write_all(&body).unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).unwrap();
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains("hello from the vendor"));
}

#[test]
fn a_request_past_its_whole_deadline_is_refused() {
    let raw = format!("POST {CHAT_PATH} HTTP/1.1\r\nHost: 127.0.0.1:1\r\nContent-Length: 2\r\n\r\n{{}}");
    let past = std::time::Instant::now() - std::time::Duration::from_secs(1);
    let r = read_request_until(&mut raw.as_bytes(), past);
    assert!(matches!(r, Err((408, _))), "a request past its deadline must be refused: {r:?}");
    let future = std::time::Instant::now() + std::time::Duration::from_secs(5);
    assert!(read_request_until(&mut raw.as_bytes(), future).is_ok());
}

fn read_soon(stream: &mut dyn Read) -> Result<HttpReq, (u16, &'static str)> {
    read_request_until(stream, std::time::Instant::now() + std::time::Duration::from_secs(5))
}

/// A client socket that hangs up once the first piece of text reached it.
struct HangsUp {
    got: Vec<u8>,
}

impl Write for HangsUp {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if String::from_utf8_lossy(&self.got).contains("hello ") {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        self.got.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_client_that_disconnects_mid_stream_cancels_the_vendor_call() {
    let rig = Rig::new("http-hangup");
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
    let mut out = HangsUp { got: Vec::new() };
    serve_one(&rig.proxy, &post(g.bearer.expose(), chat_body("gpt-known", true), &[]), 1, &mut out);
    let got = String::from_utf8_lossy(&out.got).to_string();
    assert!(got.contains("hello ") && !got.contains("[DONE]"), "{got}");
    let log = std::fs::read_to_string(log_path(&rig.dir.0)).unwrap_or_default();
    let last = log.lines().last().unwrap_or_default();
    assert!(last.contains("\"cancelled\""), "the vendor call was stopped: {last}");
}
