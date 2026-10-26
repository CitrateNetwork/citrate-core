// D-11 / WP-S1-PROV-ADAPT — the OpenAI Responses adapter against recorded answers (fixtures in
// tests/fixtures/providers/openai, built from the vendor's documented examples). No network.

use super::*;
use crate::provider_fixtures_tests::{
    expect_ok, fixture, fixture_json, tool_conversation, with_vendor_ctx, Recorded,
};

const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

fn call(http: &Recorded, req: &ChatRequest) -> (Result<ChatResponse, ProviderError>, Vec<String>) {
    let mut deltas = Vec::new();
    let r = with_vendor_ctx(Vendor::OpenAi, "sk-proj-", http, |ctx| {
        OpenAiAdapter.complete(ctx, req, &mut |t| deltas.push(t.to_string()))
    });
    (r, deltas)
}

#[test]
fn request_bytes_match_the_golden_on_the_responses_endpoint_with_nothing_stored() {
    let http = Recorded::one(200, JSON, &fixture("openai/response-function-call.json"));
    let (r, _) = call(&http, &tool_conversation("gpt-6.1-sol", false));
    expect_ok(r, "a recorded answer");
    let sent = http.sent();
    assert_eq!(sent.url, "https://api.openai.com/v1/responses");
    assert_eq!(sent.body, Some(fixture_json("openai/request.golden.json")));
    assert!(sent.auth.starts_with("authorization: Bearer sk-proj-"));
    let body = sent.body.unwrap();
    assert_eq!(body["store"], false, "nothing is kept at the vendor");
    assert!(body.get("max_tokens").is_none() && body.get("max_completion_tokens").is_none());
    let http = Recorded::one(200, SSE, &fixture("openai/stream-text.sse"));
    let (r, _) = call(&http, &tool_conversation("gpt-6.1-sol", true));
    expect_ok(r, "a recorded stream");
    let mut golden = fixture_json("openai/request.golden.json");
    golden["stream"] = json!(true);
    assert_eq!(http.sent().body, Some(golden));
}

#[test]
fn a_non_streamed_function_call_maps_call_id_arguments_and_usage() {
    let http = Recorded::one(200, JSON, &fixture("openai/response-function-call.json"));
    let (r, _) = call(&http, &tool_conversation("gpt-6-astra", false));
    assert_eq!(
        r,
        Ok(ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_unLAR8MvFNptuiZK6K6HCy5k".into(),
                name: "get_current_weather".into(),
                arguments: "{\"location\":\"Boston, MA\",\"unit\":\"celsius\"}".into(),
            }],
            usage: Some(Usage { input_tokens: 291, output_tokens: 23 }),
            finish: FinishReason::ToolCalls,
        })
    );
}

#[test]
fn the_documented_text_stream_hands_out_deltas_and_takes_usage_from_completed() {
    let http = Recorded::one(200, SSE, &fixture("openai/stream-text.sse"));
    let (r, deltas) = call(&http, &tool_conversation("gpt-6-astra", true));
    let r = expect_ok(r, "the documented stream");
    assert_eq!(deltas.len(), 10);
    assert_eq!(r.content, "Hi there! How can I assist you today?");
    assert_eq!(r.usage, Some(Usage { input_tokens: 37, output_tokens: 11 }));
    assert_eq!(r.finish, FinishReason::Stop);
}

#[test]
fn a_streamed_function_call_is_taken_whole_from_output_item_done() {
    let http = Recorded::one(200, SSE, &fixture("openai/stream-function-call.sse"));
    let (r, deltas) = call(&http, &tool_conversation("gpt-6-astra", true));
    assert!(deltas.is_empty(), "argument deltas are not text");
    assert_eq!(
        r,
        Ok(ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_abc123".into(),
                name: "get_weather".into(),
                arguments: "{\"location\":\"Boston, MA\"}".into(),
            }],
            usage: Some(Usage { input_tokens: 60, output_tokens: 18 }),
            finish: FinishReason::ToolCalls,
        })
    );
}

#[test]
fn incomplete_failed_and_cut_streams_are_told_apart() {
    let http = Recorded::one(200, SSE, &fixture("openai/stream-incomplete.sse"));
    let (r, _) = call(&http, &tool_conversation("gpt-6-astra", true));
    let r = expect_ok(r, "an incomplete answer is still an answer");
    assert_eq!(r.finish, FinishReason::Length);
    assert_eq!(r.usage, Some(Usage { input_tokens: 9, output_tokens: 512 }));
    let http = Recorded::one(200, SSE, &fixture("openai/stream-failed.sse"));
    let (r, _) = call(&http, &tool_conversation("gpt-6-astra", true));
    assert_eq!(r, Err(ProviderError::RateLimited));
    let http = Recorded::one(200, SSE, &fixture("openai/stream-cut.sse"));
    let (r, _) = call(&http, &tool_conversation("gpt-6-astra", true));
    assert_eq!(r, Err(ProviderError::Transport("the answer stream ended early")));
}

#[test]
fn http_errors_map_to_coarse_errors_and_redirects_are_not_followed() {
    for (status, want) in [
        (401, ProviderError::Unauthorized),
        (403, ProviderError::Unauthorized),
        (429, ProviderError::RateLimited),
        (503, ProviderError::Status(503)),
        (307, ProviderError::Redirect),
    ] {
        let http = Recorded::one(status, JSON, "{\"error\":{\"message\":\"m\",\"type\":\"t\"}}");
        let (r, _) = call(&http, &tool_conversation("gpt-6-astra", false));
        assert_eq!(r, Err(want), "HTTP {status}");
        assert_eq!(http.count(), 1);
    }
}

#[test]
fn key_test_lists_models_and_the_key_format_wants_sk() {
    let http = Recorded::one(200, JSON, &fixture("openai/models.json"));
    let t = with_vendor_ctx(Vendor::OpenAi, "sk-proj-", &http, |ctx| OpenAiAdapter.test_key(ctx));
    assert_eq!(
        t,
        Ok(KeyTest {
            models: vec!["gpt-6-astra".into(), "gpt-6.1-sol".into()]
        })
    );
    assert_eq!(http.sent().url, "https://api.openai.com/v1/models");
    let key = |s: String| ApiKey::from_secret(crate::secret::SecretString::new(s));
    let fake = crate::provider_fixtures_tests::fake_key;
    assert!(OpenAiAdapter.check_key_format(&key(fake("sk-proj-"))).is_ok());
    assert!(matches!(
        OpenAiAdapter.check_key_format(&key(fake("xai-"))),
        Err(ProviderError::KeyFormat(_))
    ));
}

#[test]
fn a_tool_result_without_its_call_id_is_refused() {
    let mut req = tool_conversation("gpt-6-astra", false);
    req.messages[4].tool_call_id = Some(String::new());
    assert!(matches!(to_wire(&req), Err(ProviderError::BadRequest(_))));
}
