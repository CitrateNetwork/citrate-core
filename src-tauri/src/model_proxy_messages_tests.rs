// L1-WP12 — `POST /v1/messages` on the loopback model proxy (Claude Agent with a key kept on the
// device behind the proxy, owner answer L1-Q4). Same seven steps, the route binds the model, the
// route token may come as `Authorization: Bearer` or `x-api-key`, and a streamed answer says
// `message_stop` only once the usage record is written.

use super::*;
use crate::model_provider::{FinishReason, Role, ToolCall, Usage};
use crate::model_proxy::tests::{Rig, PROMPT};
use crate::model_proxy_http::{serve_one, HttpReq, MESSAGES_PATH};
use crate::provider_usage_log::log_path;

fn body(model: &str, stream: bool) -> Vec<u8> {
    json!({"model": model, "stream": stream, "max_tokens": 50,
           "system": [{"type": "text", "text": "be brief"}],
           "messages": [{"role": "user", "content": [{"type": "text", "text": PROMPT}]}]})
    .to_string()
    .into_bytes()
}

fn post_with(auth: (&str, String), body: Vec<u8>, extra: &[(&str, &str)]) -> HttpReq {
    let mut headers = vec![
        ("host".to_string(), "127.0.0.1:1".to_string()),
        (auth.0.to_string(), auth.1),
        ("content-type".to_string(), "application/json".to_string()),
        ("anthropic-version".to_string(), "2023-06-01".to_string()),
    ];
    for (k, v) in extra {
        headers.retain(|(h, _)| h != k);
        headers.push((k.to_string(), v.to_string()));
    }
    HttpReq { method: "POST".into(), path: MESSAGES_PATH.into(), headers, body }
}

fn post(bearer: &str, body: Vec<u8>, extra: &[(&str, &str)]) -> HttpReq {
    post_with(("authorization", format!("Bearer {bearer}")), body, extra)
}

fn serve(rig: &Rig, req: &HttpReq) -> String {
    let mut out = Vec::new();
    serve_one(&rig.proxy, req, 1, &mut out);
    String::from_utf8(out).unwrap()
}

fn status_of(resp: &str) -> u16 {
    resp.split_whitespace().nth(1).unwrap().parse().unwrap()
}

fn json_body(resp: &str) -> Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}

#[test]
fn messages_requires_route_bearer() {
    let rig = Rig::new("msg-bearer");
    assert_eq!(status_of(&serve(&rig, &post("cmp_nope", body("gpt-known", false), &[]))), 401);
    let mut none = post("x", body("gpt-known", false), &[]);
    none.headers.retain(|(k, _)| k != "authorization");
    assert_eq!(status_of(&serve(&rig, &none)), 401);
    // A vendor key is never a route, in either header.
    assert_eq!(status_of(&serve(&rig, &post(&rig.key, body("gpt-known", false), &[]))), 401);
    let as_api_key = post_with(("x-api-key", rig.key.clone()), body("gpt-known", false), &[]);
    assert_eq!(status_of(&serve(&rig, &as_api_key)), 401);
    assert_eq!(rig.vendor.calls(), 0);
    // The route token itself may come as x-api-key (Anthropic clients' convention).
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let ok = serve(&rig, &post_with(("x-api-key", g.bearer.expose().to_string()), body("gpt-known", false), &[]));
    assert_eq!(status_of(&ok), 200, "{ok}");
    for extra in [vec![("origin", "https://evil.example")], vec![("host", "evil.example:1")]] {
        assert_eq!(status_of(&serve(&rig, &post(g.bearer.expose(), body("gpt-known", false), &extra))), 403);
    }
}

#[test]
fn messages_maps_to_chat_request_and_back() {
    let v = json!({
        "model": "m", "max_tokens": 9_999_999, "stream": true, "system": "sys",
        "thinking": {"type": "enabled", "budget_tokens": 1024},
        "messages": [
            {"role": "user", "content": "q"},
            {"role": "assistant", "content": [{"type": "thinking", "thinking": "hmm", "signature": "s"}, {"type": "text", "text": "a"}]},
            {"role": "user", "content": [{"type": "text", "text": "b"}, {"type": "text", "text": "c"}]}
        ],
        "tools": [{"name": "f", "description": "d", "input_schema": {"type": "object"}}]
    });
    let r = from_messages_body(&v).unwrap();
    let roles: Vec<Role> = r.messages.iter().map(|m| m.role).collect();
    assert_eq!(roles, vec![Role::System, Role::User, Role::Assistant, Role::User]);
    assert_eq!((r.messages[2].content.as_str(), r.messages[3].content.as_str()), ("a", "bc"));
    assert_eq!(r.tools[0].name, "f");
    assert_eq!(r.max_tokens, crate::model_proxy::MAX_OUTPUT_TOKENS);
    assert!(r.stream);
    for bad in [
        json!({"messages": []}),
        json!({"messages": [{"role": "tool", "content": "x"}]}),
        json!({"messages": [{"role": "user", "content": [{"type": "image", "source": {}}]}]}),
        json!({"messages": [{"role": "user", "content": "x"}], "tools": [{"type": "web_search_20250305", "name": "web_search"}]}),
        json!({"messages": [{"role": "user", "content": "x"}], "stop_sequences": ["END"]}),
    ] {
        assert!(from_messages_body(&bad).is_err(), "{bad}");
    }
    let rig = Rig::new("msg-plain");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", false), &[]));
    assert_eq!(status_of(&resp), 200, "{resp}");
    let b = json_body(&resp);
    assert_eq!((b["type"].as_str(), b["role"].as_str(), b["model"].as_str()), (Some("message"), Some("assistant"), Some("gpt-known")));
    assert_eq!(b["content"][0], json!({"type": "text", "text": "hello from the vendor"}));
    assert_eq!(b["stop_reason"], "end_turn");
    assert_eq!((b["usage"]["input_tokens"].as_u64(), b["usage"]["output_tokens"].as_u64()), (Some(12), Some(5)));
    assert!(!resp.contains(&rig.key));
}

#[test]
fn tool_use_round_trip() {
    let v = json!({"model": "gpt-known", "max_tokens": 50, "messages": [
        {"role": "user", "content": "list files"},
        {"role": "assistant", "content": [{"type": "text", "text": "looking"},
            {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}]},
        {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1",
            "content": [{"type": "text", "text": "a.txt"}]}, {"type": "text", "text": "and?"}]}
    ], "tools": [{"name": "Bash", "input_schema": {"type": "object"}}]});
    let r = from_messages_body(&v).unwrap();
    assert_eq!(r.messages[1].tool_calls[0].id, "toolu_1");
    assert_eq!(r.messages[1].tool_calls[0].arguments, "{\"command\":\"ls\"}");
    assert_eq!((r.messages[2].role, r.messages[2].tool_call_id.as_deref(), r.messages[2].content.as_str()), (Role::Tool, Some("toolu_1"), "a.txt"));
    assert_eq!((r.messages[3].role, r.messages[3].content.as_str()), (Role::User, "and?"));
    let rig = Rig::new("msg-tool");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    assert_eq!(status_of(&serve(&rig, &post(g.bearer.expose(), v.to_string().into_bytes(), &[]))), 200);
    let up = rig.vendor.seen.lock().unwrap().iter().rev().find_map(|s| s.body.clone()).unwrap().to_string();
    assert!(up.contains("toolu_1") && up.contains("a.txt") && up.contains("Bash"), "{up}");
    let answer = ChatResponse {
        content: "running it".into(),
        tool_calls: vec![ToolCall { id: "toolu_2".into(), name: "Bash".into(), arguments: "{\"command\":\"pwd\"}".into() }],
        usage: Some(Usage { input_tokens: 3, output_tokens: 4 }),
        finish: FinishReason::ToolCalls,
    };
    let j = message_json("msg_x", "gpt-known", &answer);
    assert_eq!(j["stop_reason"], "tool_use");
    assert_eq!(j["content"][1], json!({"type": "tool_use", "id": "toolu_2", "name": "Bash", "input": {"command": "pwd"}}));
    let mut out = Vec::new();
    let mut st = StreamState::default();
    write_stream_tail(&mut out, &mut st, &answer).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("\"type\":\"input_json_delta\""), "{s}");
    assert!(s.contains("\"partial_json\":\"{\\\"command\\\":\\\"pwd\\\"}\""), "{s}");
    assert!(s.trim_end().ends_with("event: message_stop\ndata: {\"type\":\"message_stop\"}"), "{s}");
}

#[test]
fn messages_stream_events_after_end_record() {
    let rig = Rig::new("msg-stream");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", true), &[]));
    assert_eq!(status_of(&resp), 200, "{resp}");
    let kinds: Vec<&str> = resp.lines().filter_map(|l| l.strip_prefix("event: ")).collect();
    assert_eq!(kinds.first(), Some(&"message_start"));
    assert_eq!(kinds[1], "content_block_start");
    assert!(kinds.contains(&"content_block_delta"));
    assert_eq!(&kinds[kinds.len() - 3..], &["content_block_stop", "message_delta", "message_stop"]);
    assert!(resp.contains("\"text\":\"hello \""), "{resp}");
    assert!(resp.contains("\"stop_reason\":\"end_turn\""));
    assert!(rig.log_text().contains("\"end\""));
    assert!(!resp.contains(&rig.key));
    let rig = Rig::new("msg-stream-nolog");
    let p = log_path(&rig.dir.0);
    *rig.vendor.during_call.lock().unwrap() = Some(Box::new(move || {
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::create_dir_all(&p);
    }));
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", true), &[]));
    assert!(!resp.contains("message_stop"), "{resp}");
    assert!(resp.contains("event: error") && resp.contains("usage_log_unavailable"), "{resp}");
}

#[test]
fn anthropic_version_header_pinned() {
    assert_eq!(ANTHROPIC_VERSION, crate::provider_anthropic::ANTHROPIC_VERSION, "the wire this path speaks is the adapter's pin");
    let rig = Rig::new("msg-version");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let other = serve(&rig, &post(g.bearer.expose(), body("gpt-known", false), &[("anthropic-version", "2099-01-01")]));
    assert_eq!(status_of(&other), 400, "{other}");
    assert!(other.contains("2023-06-01"), "names the pinned version: {other}");
    assert_eq!(rig.vendor.calls(), 0);
    let mut missing = post(g.bearer.expose(), body("gpt-known", false), &[]);
    missing.headers.retain(|(k, _)| k != "anthropic-version");
    assert_eq!(status_of(&serve(&rig, &missing)), 200, "no header means the pinned version");
}

#[test]
fn messages_model_and_caps_follow_the_route() {
    let rig = Rig::new("msg-route");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("claude-other", false), &[]));
    assert_eq!(status_of(&resp), 400, "{resp}");
    assert!(resp.contains("different model than its route"));
    rig.proxy.with_state(|g, _| g.member_halt("openai", 1));
    let _ = rig.proxy.with_state(|g, _| rig.proxy.save_guard(&mut *g));
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", true), &[]));
    assert_eq!(status_of(&resp), 423, "{resp}");
    assert!(!resp.contains("text/event-stream"));
    assert_eq!(rig.vendor.calls(), 0);
}

#[test]
fn messages_accepts_the_beta_query_claude_code_sends() {
    // claude-agent-acp 0.113.0 (Claude Code) posts to `/v1/messages?beta=true` (seen on the build
    // host, 2026-11-28). Only that exact query is accepted; any other query stays 404.
    let rig = Rig::new("msg-beta");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let mut req = post(g.bearer.expose(), body("gpt-known", false), &[]);
    req.path = format!("{MESSAGES_PATH}?beta=true");
    let ok = serve(&rig, &req);
    assert_eq!(status_of(&ok), 200, "{ok}");
    for q in ["?beta=false", "?x=1", "?beta=true&x=1"] {
        req.path = format!("{MESSAGES_PATH}{q}");
        assert_eq!(status_of(&serve(&rig, &req)), 404, "{q}");
    }
}

#[test]
fn messages_carries_the_system_turns_claude_code_sends() {
    // claude-agent-acp 0.113.0 sends `role: "system"` turns inside `messages` (seen on the build host
    // 2026-11-28, request shape only), plus fields this proxy does not carry (`output_config`,
    // `context_management`, `safeguards`), which are left out of the vendor call.
    let v = json!({"model": "m", "max_tokens": 10, "output_config": {"effort": "low"},
        "context_management": {"edits": []}, "safeguards": [],
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "q"}]},
            {"role": "system", "content": [{"type": "text", "text": "reminder"}]}
        ]});
    let r = from_messages_body(&v).unwrap();
    let roles: Vec<Role> = r.messages.iter().map(|m| m.role).collect();
    assert_eq!(roles, vec![Role::User, Role::System]);
    assert_eq!(r.messages[1].content, "reminder");
    let bad = json!({"messages": [{"role": "system", "content": [{"type": "tool_use", "id": "t", "name": "x", "input": {}}]}]});
    assert!(from_messages_body(&bad).is_err(), "a system turn carries text only");
}
