// D-11 — the OpenAI-compatible adapter against recorded answers (no network).

use super::*;
use crate::model_provider::{ApiKey, ChatMessage, InboundResponse, ProviderHttp, ToolDef, Vendor, WireRequest};
use crate::secret::SecretString;
use std::sync::Mutex;

/// Answers each send with the next recorded `(status, content type, body)`.
struct Replay {
    answers: Mutex<Vec<(u16, &'static str, String)>>,
    seen: Mutex<Vec<(String, Option<Value>)>>,
}

impl Replay {
    fn new(answers: Vec<(u16, &'static str, String)>) -> Self {
        Replay {
            answers: Mutex::new(answers),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl ProviderHttp for Replay {
    fn send(&self, req: WireRequest<'_>) -> Result<InboundResponse, ProviderError> {
        self.seen.lock().unwrap().push((
            req.url.to_string(),
            req.body.as_ref().map(|b| serde_json::from_slice(b).unwrap()),
        ));
        let (status, ct, body) = self.answers.lock().unwrap().remove(0);
        Ok(InboundResponse {
            status,
            content_type: ct.into(),
            body: Box::new(std::io::Cursor::new(body.into_bytes())),
        })
    }
}

fn adapter() -> OpenAiCompatAdapter {
    OpenAiCompatAdapter::new(ProviderId::Vendor(Vendor::OpenAi), &["sk-"], vec![])
}

fn key() -> ApiKey {
    ApiKey::from_secret(SecretString::new("sk-test-aaaaaaaaaaaaaaaaaaaaaaaa".into()))
}

fn req(stream: bool) -> ChatRequest {
    ChatRequest {
        model: "gpt-test".into(),
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: "be brief".into(),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::User,
                content: "hi".into(),
                tool_calls: vec![],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "node_status".into(),
                    arguments: "{}".into(),
                }],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "{\"height\":1}".into(),
                tool_calls: vec![],
                tool_call_id: Some("c1".into()),
            },
        ],
        tools: vec![ToolDef {
            name: "node_status".into(),
            description: "status".into(),
            parameters: json!({"type": "object"}),
        }],
        max_tokens: 256,
        stream,
    }
}

#[test]
fn wire_body_carries_the_tool_protocol() {
    let b = to_wire(&req(true));
    assert_eq!(b["model"], "gpt-test");
    assert_eq!(b["stream"], true);
    assert_eq!(b["stream_options"]["include_usage"], true);
    assert_eq!(b["messages"][2]["content"], Value::Null);
    assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["name"], "node_status");
    assert_eq!(b["messages"][3]["tool_call_id"], "c1");
    assert_eq!(b["tools"][0]["function"]["name"], "node_status");
    assert!(to_wire(&req(false)).get("stream_options").is_none());
}

#[test]
fn non_streamed_answer_with_tool_calls_and_usage() {
    let ep = crate::provider_egress::resolve_vendor(Vendor::OpenAi).unwrap();
    let k = key();
    let http = Replay::new(vec![(
        200,
        "application/json",
        json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"t1","type":"function",
          "function":{"name":"node_status","arguments":"{\"a\":1}"}}]},"finish_reason":"tool_calls"}],
          "usage":{"prompt_tokens":12,"completion_tokens":3}})
        .to_string(),
    )]);
    let ctx = ProviderCtx::new(&ep, &k, &http);
    let r = adapter().complete(&ctx, &req(false), &mut |_| {}).unwrap();
    assert_eq!(r.tool_calls.len(), 1);
    assert_eq!(r.tool_calls[0].arguments, "{\"a\":1}");
    assert_eq!(r.finish, FinishReason::ToolCalls);
    assert_eq!(r.usage, Some(Usage { input_tokens: 12, output_tokens: 3 }));
    assert_eq!(http.seen.lock().unwrap()[0].0, "https://api.openai.com/v1/chat/completions");
}

#[test]
fn streamed_answer_hands_out_deltas_and_joins_tool_call_pieces() {
    let ep = crate::provider_egress::resolve_vendor(Vendor::OpenAi).unwrap();
    let k = key();
    let sse = [
        r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#,
        r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"no","arguments":"{\"a\""}}]}}]}"#,
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"de","arguments":":2}"}}]},"finish_reason":"tool_calls"}]}"#,
        r#"data: {"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":7}}"#,
        "data: [DONE]",
    ]
    .join("\n\n");
    let http = Replay::new(vec![(200, "text/event-stream", sse)]);
    let ctx = ProviderCtx::new(&ep, &k, &http);
    let mut deltas = Vec::new();
    let r = adapter()
        .complete(&ctx, &req(true), &mut |t| deltas.push(t.to_string()))
        .unwrap();
    assert_eq!(deltas, vec!["Hel", "lo"]);
    assert_eq!(r.content, "Hello");
    assert_eq!(r.tool_calls[0].name, "node");
    assert_eq!(r.tool_calls[0].arguments, "{\"a\":2}");
    assert_eq!(r.usage, Some(Usage { input_tokens: 5, output_tokens: 7 }));
}

#[test]
fn a_stream_cut_off_before_it_finished_is_an_error_not_a_short_answer() {
    let ep = crate::provider_egress::resolve_vendor(Vendor::OpenAi).unwrap();
    let k = key();
    let http = Replay::new(vec![(
        200,
        "text/event-stream",
        r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#.into(),
    )]);
    let ctx = ProviderCtx::new(&ep, &k, &http);
    let r = adapter().complete(&ctx, &req(true), &mut |_| {});
    assert_eq!(r, Err(ProviderError::Transport("the answer stream ended early")));
}

#[test]
fn key_test_lists_models_and_maps_rejection() {
    let ep = crate::provider_egress::resolve_vendor(Vendor::OpenAi).unwrap();
    let k = key();
    let http = Replay::new(vec![
        (
            200,
            "application/json",
            json!({"data":[{"id":"gpt-a"},{"id":""},{"id":"gpt-b"}]}).to_string(),
        ),
        (401, "application/json", "{\"error\":\"bad key\"}".into()),
    ]);
    let ctx = ProviderCtx::new(&ep, &k, &http);
    assert_eq!(
        adapter().test_key(&ctx).unwrap().models,
        vec!["gpt-a".to_string(), "gpt-b".to_string()]
    );
    assert_eq!(adapter().test_key(&ctx), Err(ProviderError::Unauthorized));
    assert_eq!(http.seen.lock().unwrap()[0].0, "https://api.openai.com/v1/models");
}

#[test]
fn key_format_checks_the_prefix() {
    assert!(adapter().key_format().check(&key()).is_ok());
    let wrong = ApiKey::from_secret(SecretString::new("xai-aaaaaaaaaaaaaaaaaaaaaaaaaa".into()));
    assert!(matches!(adapter().key_format().check(&wrong), Err(ProviderError::KeyFormat(_))));
}
