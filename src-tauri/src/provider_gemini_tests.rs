// D-11 / WP-S1-PROV-ADAPT — the Gemini generateContent adapter against recorded answers (fixtures
// in tests/fixtures/providers/gemini, built from the vendor's documented examples). No network.

use super::*;
use crate::provider_fixtures_tests::{
    expect_ok, fixture, fixture_json, tool_conversation, with_vendor_ctx, Recorded,
};

const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

fn call(http: &Recorded, req: &ChatRequest) -> (Result<ChatResponse, ProviderError>, Vec<String>) {
    let mut deltas = Vec::new();
    let r = with_vendor_ctx(Vendor::Gemini, "AIza", http, |ctx| {
        GeminiAdapter.complete(ctx, req, &mut |t| deltas.push(t.to_string()))
    });
    (r, deltas)
}

#[test]
fn request_bytes_match_the_golden_and_the_key_never_goes_in_the_query() {
    let http = Recorded::one(200, JSON, &fixture("gemini/response-text.json"));
    let (r, _) = call(&http, &tool_conversation("gemini-3.8-flash", false));
    expect_ok(r, "a recorded text answer");
    let sent = http.sent();
    assert_eq!(
        sent.url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash:generateContent"
    );
    assert_eq!(sent.body, Some(fixture_json("gemini/request.golden.json")));
    assert!(sent.auth.starts_with("x-goog-api-key: AIza"));
    assert!(!sent.url.contains("key="));
    // Streaming: same body, the SSE method.
    let http = Recorded::one(200, SSE, &fixture("gemini/stream-text-and-call.sse"));
    let (r, _) = call(&http, &tool_conversation("models/gemini-3.8-flash", true));
    expect_ok(r, "a recorded stream");
    let sent = http.sent();
    assert_eq!(
        sent.url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash:streamGenerateContent?alt=sse"
    );
    assert_eq!(sent.body, Some(fixture_json("gemini/request.golden.json")));
}

#[test]
fn replayed_calls_carry_the_documented_signature_on_the_first_call_of_each_step_only() {
    let b = expect_ok(to_wire(&tool_conversation("gemini-4-argon", false)), "wire body");
    let parts = &b["contents"][1]["parts"];
    assert_eq!(parts[1]["thoughtSignature"], REPLAY_SIGNATURE);
    assert!(parts[2]["functionCall"].is_object() && parts[2].get("thoughtSignature").is_none());
    // Results follow all the calls, together, in one user content (FC1, FC2, FR1, FR2).
    let results = &b["contents"][2]["parts"];
    assert!(results[0]["functionResponse"].is_object() && results[1]["functionResponse"].is_object());
}

#[test]
fn non_streamed_answers_map_text_calls_usage_and_finish() {
    let http = Recorded::one(200, JSON, &fixture("gemini/response-text.json"));
    let (r, _) = call(&http, &tool_conversation("gemini-3.8-flash", false));
    assert_eq!(
        r,
        Ok(ChatResponse {
            content: "Why did the chicken cross the road? To get to the other side!".into(),
            tool_calls: vec![],
            usage: Some(Usage { input_tokens: 4, output_tokens: 12 }),
            finish: FinishReason::Stop,
        })
    );
    let http = Recorded::one(200, JSON, &fixture("gemini/response-function-call.json"));
    let (r, _) = call(&http, &tool_conversation("gemini-3.8-flash", false));
    assert_eq!(
        r,
        Ok(ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_0".into(),
                name: "get_weather".into(),
                arguments: "{\"location\":\"Boston, MA\"}".into(),
            }],
            usage: None,
            finish: FinishReason::ToolCalls,
        }),
        "no usageMetadata is unknown usage, never zero"
    );
}

#[test]
fn a_stream_skips_thoughts_keeps_the_vendor_call_id_and_bills_thinking_as_output() {
    let http = Recorded::one(200, SSE, &fixture("gemini/stream-text-and-call.sse"));
    let (r, deltas) = call(&http, &tool_conversation("gemini-3.8-flash", true));
    let r = expect_ok(r, "the recorded stream");
    assert_eq!(deltas, vec!["Let me ".to_string(), "check.".to_string()]);
    assert_eq!(r.content, "Let me check.");
    assert_eq!(
        r.tool_calls,
        vec![ToolCall {
            id: "fc_8f2a".into(),
            name: "get_weather".into(),
            arguments: "{\"location\":\"Boston, MA\"}".into(),
        }]
    );
    assert_eq!(r.usage, Some(Usage { input_tokens: 40, output_tokens: 42 }));
    assert_eq!(r.finish, FinishReason::ToolCalls);
}

#[test]
fn a_stream_with_no_finish_reason_is_an_error_and_a_blocked_prompt_is_refused() {
    let http = Recorded::one(200, SSE, &fixture("gemini/stream-cut.sse"));
    let (r, _) = call(&http, &tool_conversation("gemini-3.8-flash", true));
    assert_eq!(r, Err(ProviderError::Transport("the answer stream ended early")));
    let http = Recorded::one(200, JSON, &fixture("gemini/blocked.json"));
    let (r, _) = call(&http, &tool_conversation("gemini-3.8-flash", false));
    assert_eq!(r, Err(ProviderError::BadResponse("the provider blocked the prompt")));
}

#[test]
fn an_invalid_key_400_is_a_rejected_key_and_other_errors_stay_coarse() {
    let http = Recorded::one(400, JSON, &fixture("gemini/error-invalid-key.json"));
    let t = with_vendor_ctx(Vendor::Gemini, "AIza", &http, |ctx| GeminiAdapter.test_key(ctx));
    assert_eq!(t, Err(ProviderError::Unauthorized));
    for (status, body, want) in [
        (400, "gemini/error-bad-request.json", ProviderError::Status(400)),
        (403, "gemini/error-bad-request.json", ProviderError::Unauthorized),
        (429, "gemini/error-bad-request.json", ProviderError::RateLimited),
        (503, "gemini/error-bad-request.json", ProviderError::Status(503)),
        (301, "gemini/error-bad-request.json", ProviderError::Redirect),
    ] {
        let http = Recorded::one(status, JSON, &fixture(body));
        let (r, _) = call(&http, &tool_conversation("gemini-3.8-flash", false));
        assert_eq!(r, Err(want), "HTTP {status}");
        assert_eq!(http.count(), 1);
    }
}

#[test]
fn a_model_id_that_would_change_the_path_is_refused_before_anything_is_sent() {
    let http = Recorded::new(vec![]);
    for bad in [
        "../files",
        "gemini-3.8-flash:generateContent?key=x",
        "a/b",
        "gemini 3",
        "",
        ".hidden",
        "models/../../v1/files",
    ] {
        let (r, _) = call(&http, &tool_conversation(bad, false));
        assert!(matches!(r, Err(ProviderError::BadRequest(_))), "{bad:?}: {r:?}");
    }
    assert_eq!(http.count(), 0);
}

#[test]
fn key_test_keeps_text_models_only_and_the_key_format_is_shape_only() {
    let http = Recorded::one(200, JSON, &fixture("gemini/models.json"));
    let t = with_vendor_ctx(Vendor::Gemini, "AIza", &http, |ctx| GeminiAdapter.test_key(ctx));
    assert_eq!(
        t,
        Ok(KeyTest {
            models: vec!["gemini-3.8-flash".into(), "gemini-4-argon".into()]
        })
    );
    assert_eq!(
        http.sent().url,
        "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000"
    );
    let key = |s: &str| crate::model_provider::ApiKey::from_secret(crate::secret::SecretString::new(s.into()));
    let fake = crate::provider_fixtures_tests::fake_key;
    assert!(GeminiAdapter.key_format().check(&key(&fake("AIza"))).is_ok());
    assert!(GeminiAdapter.key_format().check(&key(&fake("AQ."))).is_ok(), "no documented prefix");
    assert!(matches!(
        GeminiAdapter.key_format().check(&key("AIza short")),
        Err(ProviderError::KeyFormat(_))
    ));
}

#[test]
fn a_tool_result_for_an_unknown_call_is_refused() {
    let mut req = tool_conversation("gemini-3.8-flash", false);
    req.messages[3].tool_call_id = Some("call_9".into());
    assert!(matches!(to_wire(&req), Err(ProviderError::BadRequest(_))));
}

#[test]
fn a_continuation_finish_is_a_length_stop() {
    let http = Recorded::one(200, JSON, &fixture("gemini/response-continuation.json"));
    let (r, _) = call(&http, &tool_conversation("gemini-3.8-flash", false));
    assert_eq!(expect_ok(r, "continuation").finish, FinishReason::Length);
}
