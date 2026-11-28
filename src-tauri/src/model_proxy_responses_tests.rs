// L1-WP10 — `POST /v1/responses` on the loopback model proxy (Codex speaks the Responses API). The
// same seven steps as chat, the route binds provider and model, and a streamed answer says
// `response.completed` only once the usage record is written.

use super::*;
use crate::model_provider::{FinishReason, Role, ToolCall, Usage};
use crate::model_proxy::tests::{Rig, PROMPT};
use crate::model_proxy_http::{serve_one, HttpReq, RESPONSES_PATH};
use crate::provider_usage_log::log_path;

fn body(model: &str, stream: bool) -> Vec<u8> {
    json!({"model": model, "stream": stream, "store": false, "max_output_tokens": 50,
           "instructions": "be brief",
           "input": [{"type": "message", "role": "user",
                      "content": [{"type": "input_text", "text": PROMPT}]}]})
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
    HttpReq { method: "POST".into(), path: RESPONSES_PATH.into(), headers, body }
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

/// The upstream request bodies the scripted vendor saw.
fn upstream(rig: &Rig) -> Vec<Value> {
    rig.vendor.seen.lock().unwrap().iter().filter_map(|s| s.body.clone()).collect()
}

#[test]
fn responses_path_requires_route_bearer() {
    let rig = Rig::new("resp-bearer");
    assert_eq!(status_of(&serve(&rig, &post("cmp_nope", body("gpt-known", false), &[]))), 401);
    let mut none = post("x", body("gpt-known", false), &[]);
    none.headers.retain(|(k, _)| k != "authorization");
    assert_eq!(status_of(&serve(&rig, &none)), 401);
    assert_eq!(status_of(&serve(&rig, &post(&rig.key, body("gpt-known", false), &[]))), 401, "a vendor key is not a route");
    assert_eq!(rig.vendor.calls(), 0);
}

#[test]
fn origin_and_host_guards_apply() {
    let rig = Rig::new("resp-guards");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    for extra in [
        vec![("origin", "https://evil.example")],
        vec![("host", "evil.example:1")],
        vec![("host", "127.0.0.1:2")],
    ] {
        let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", false), &extra));
        assert_eq!(status_of(&resp), 403, "{extra:?}: {resp}");
    }
    let mut get = post(g.bearer.expose(), body("gpt-known", false), &[]);
    get.method = "GET".into();
    assert_eq!(status_of(&serve(&rig, &get)), 405);
    assert_eq!(rig.vendor.calls(), 0);
}

#[test]
fn responses_maps_to_chat_request_and_back() {
    let v = json!({
        "model": "m", "stream": true, "max_output_tokens": 9_999_999, "instructions": "sys",
        "input": [
            {"role": "developer", "content": "dev rules"},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "a"}, {"type": "input_text", "text": "b"}]},
            {"type": "reasoning", "id": "rs_1", "encrypted_content": "opaque"},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "ok"}]}
        ],
        "tools": [{"type": "function", "name": "f", "description": "d", "parameters": {"type": "object"}}]
    });
    let r = from_responses_body(&v).unwrap();
    let roles: Vec<Role> = r.messages.iter().map(|m| m.role).collect();
    assert_eq!(roles, vec![Role::System, Role::System, Role::User, Role::Assistant]);
    assert_eq!((r.messages[0].content.as_str(), r.messages[2].content.as_str()), ("sys", "ab"));
    assert_eq!(r.tools[0].name, "f");
    assert_eq!(r.max_tokens, crate::model_proxy::MAX_OUTPUT_TOKENS);
    assert!(r.stream);
    assert_eq!(from_responses_body(&json!({"input": "just text"})).unwrap().messages[0].content, "just text");
    for bad in [
        json!({"input": []}),
        json!({"input": 7}),
        json!({"input": [{"type": "message", "role": "wizard", "content": "x"}]}),
        json!({"input": [{"type": "message", "role": "user", "content": [{"type": "input_image", "image_url": "x"}]}]}),
        json!({"input": "x", "tools": [{"type": "web_search"}]}),
        json!({"input": [{"type": "computer_call"}]}),
    ] {
        assert!(from_responses_body(&bad).is_err(), "{bad}");
    }

    let rig = Rig::new("resp-plain");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", false), &[]));
    assert_eq!(status_of(&resp), 200, "{resp}");
    let b = json_body(&resp);
    assert_eq!((b["object"].as_str(), b["status"].as_str(), b["model"].as_str()), (Some("response"), Some("completed"), Some("gpt-known")));
    assert_eq!(b["output"][0]["type"], "message");
    assert_eq!(b["output"][0]["content"][0]["type"], "output_text");
    assert_eq!(b["output"][0]["content"][0]["text"], "hello from the vendor");
    assert_eq!((b["usage"]["input_tokens"].as_u64(), b["usage"]["output_tokens"].as_u64()), (Some(12), Some(5)));
    assert!(!resp.contains(&rig.key));
    let up = upstream(&rig).pop().unwrap().to_string();
    assert!(up.contains(PROMPT) && up.contains("be brief"), "the input and instructions reached the vendor");
}

#[test]
fn function_call_round_trip() {
    // In: the agent replays a function call and its output; both reach the vendor as one tool turn.
    let v = json!({"model": "gpt-known", "input": [
        {"type": "message", "role": "user", "content": "list files"},
        {"type": "function_call", "call_id": "call_7", "name": "shell", "arguments": "{\"cmd\":\"ls\"}"},
        {"type": "function_call_output", "call_id": "call_7", "output": "a.txt"}
    ], "tools": [{"type": "function", "name": "shell", "parameters": {"type": "object"}}]});
    let r = from_responses_body(&v).unwrap();
    assert_eq!(r.messages[1].role, Role::Assistant);
    assert_eq!(r.messages[1].tool_calls[0].id, "call_7");
    assert_eq!(r.messages[1].tool_calls[0].arguments, "{\"cmd\":\"ls\"}");
    assert_eq!((r.messages[2].role, r.messages[2].tool_call_id.as_deref(), r.messages[2].content.as_str()), (Role::Tool, Some("call_7"), "a.txt"));
    let rig = Rig::new("resp-fc");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), v.to_string().into_bytes(), &[]));
    assert_eq!(status_of(&resp), 200, "{resp}");
    let up = upstream(&rig).pop().unwrap().to_string();
    assert!(up.contains("call_7") && up.contains("a.txt") && up.contains("shell"), "{up}");
    // Out: a model tool call becomes a function_call output item, plain and streamed.
    let answer = ChatResponse {
        content: String::new(),
        tool_calls: vec![ToolCall { id: "call_9".into(), name: "shell".into(), arguments: "{\"cmd\":\"pwd\"}".into() }],
        usage: Some(Usage { input_tokens: 3, output_tokens: 4 }),
        finish: FinishReason::ToolCalls,
    };
    let j = response_json("resp_x", "gpt-known", &answer);
    assert_eq!(j["output"][0]["type"], "function_call");
    assert_eq!((j["output"][0]["call_id"].as_str(), j["output"][0]["name"].as_str()), (Some("call_9"), Some("shell")));
    assert_eq!(j["output"][0]["arguments"], "{\"cmd\":\"pwd\"}");
    let mut out = Vec::new();
    let mut st = StreamState::default();
    write_stream_tail(&mut out, &mut st, "resp_x", "gpt-known", &answer).unwrap();
    let s = String::from_utf8(out).unwrap();
    for ev in ["response.output_item.added", "response.function_call_arguments.delta", "response.function_call_arguments.done", "response.output_item.done", "response.completed"] {
        assert!(s.contains(&format!("event: {ev}\n")), "{ev}: {s}");
    }
    assert!(s.find("function_call_arguments.done").unwrap() < s.find("response.completed").unwrap());
}

#[test]
fn stream_completed_only_after_end_record() {
    let rig = Rig::new("resp-stream");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", true), &[]));
    assert_eq!(status_of(&resp), 200, "{resp}");
    assert!(resp.contains("text/event-stream"));
    assert!(resp.contains("event: response.created\n"));
    assert!(resp.contains("event: response.output_text.delta\n"));
    assert!(resp.contains("\"delta\":\"hello \""), "{resp}");
    let last = resp.trim_end().rsplit("\n\n").next().unwrap();
    assert!(last.starts_with("event: response.completed\n"), "{last}");
    assert!(last.contains("\"input_tokens\":12"), "{last}");
    let seqs: Vec<u64> = resp.lines().filter_map(|l| l.strip_prefix("data: ")).map(|d| serde_json::from_str::<Value>(d).unwrap()["sequence_number"].as_u64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "sequence numbers count up: {seqs:?}");
    assert!(rig.log_text().contains("\"end\""));
    assert!(!resp.contains(&rig.key));

    // The usage log breaks during the call: no response.completed, an error event instead.
    let rig = Rig::new("resp-stream-nolog");
    let p = log_path(&rig.dir.0);
    *rig.vendor.during_call.lock().unwrap() = Some(Box::new(move || {
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::create_dir_all(&p);
    }));
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", true), &[]));
    assert!(!resp.contains("response.completed"), "{resp}");
    assert!(resp.contains("usage_log_unavailable"), "{resp}");
}

#[test]
fn previous_response_id_refused() {
    let rig = Rig::new("resp-stateless");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    for extra in [json!({"previous_response_id": "resp_1"}), json!({"store": true}), json!({"conversation": "conv_1"})] {
        let mut v: Value = serde_json::from_slice(&body("gpt-known", false)).unwrap();
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        let resp = serve(&rig, &post(g.bearer.expose(), v.to_string().into_bytes(), &[]));
        assert_eq!(status_of(&resp), 400, "{extra}: {resp}");
        assert!(resp.contains("stateless proxy: send full input"), "{resp}");
    }
    assert_eq!(rig.vendor.calls(), 0);
}

#[test]
fn model_in_body_cannot_override_route() {
    let rig = Rig::new("resp-model");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-other", false), &[]));
    assert_eq!(status_of(&resp), 400, "{resp}");
    assert!(resp.contains("different model than its route"));
    assert_eq!(rig.vendor.calls(), 0);
    let mut v: Value = serde_json::from_slice(&body("", false)).unwrap();
    v.as_object_mut().unwrap().remove("model");
    let resp = serve(&rig, &post(g.bearer.expose(), v.to_string().into_bytes(), &[]));
    assert_eq!(status_of(&resp), 200, "no model in the body means the route's");
    assert_eq!(json_body(&resp)["model"], "gpt-known");
}

#[test]
fn caps_apply_to_responses() {
    let rig = Rig::new("resp-caps");
    rig.proxy.with_state(|g, _| g.member_halt("openai", 1));
    let _ = rig.proxy.with_state(|g, _| rig.proxy.save_guard(&mut *g));
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    for stream in [false, true] {
        let resp = serve(&rig, &post(g.bearer.expose(), body("gpt-known", stream), &[]));
        assert_eq!(status_of(&resp), 423, "{resp}");
        assert!(resp.contains("provider_halted") && !resp.contains("text/event-stream"));
    }
    assert_eq!(rig.vendor.calls(), 0);
}

#[test]
fn responses_usage_is_recorded_under_the_route_session() {
    // Cost attribution: every call lands in the usage log under the route's session label, which
    // for an ACP agent is `ag:<agent id>:<hex>` (agent_auth::mint_agent_route).
    let rig = Rig::new("resp-attr");
    let mut route = rig.route("gpt-known");
    route.session_id = "ag:codex:00000000000000aa".into();
    let g = rig.proxy.mint_route(route, rig.gate()).unwrap();
    assert_eq!(status_of(&serve(&rig, &post(g.bearer.expose(), body("gpt-known", false), &[]))), 200);
    let end = rig.log_text().lines().rev().find(|l| l.contains("\"end\"")).unwrap().to_string();
    assert!(end.contains("\"session\":\"ag:codex:00000000000000aa\""), "{end}");
    assert!(end.contains("\"tokensIn\":12") && end.contains("\"tokensOut\":5"), "{end}");
    assert!(!rig.log_text().contains(PROMPT), "no content in the log");
}
