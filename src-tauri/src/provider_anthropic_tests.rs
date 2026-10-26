// D-11 / WP-S1-PROV-ADAPT — the Anthropic Messages adapter against recorded answers (fixtures in
// tests/fixtures/providers/anthropic, built from the vendor's documented examples). No network.

use super::*;
use crate::provider_fixtures_tests::{
    expect_ok, fixture, fixture_json, tool_conversation, with_vendor_ctx, Recorded,
};

const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

fn call(http: &Recorded, req: &ChatRequest) -> (Result<ChatResponse, ProviderError>, Vec<String>) {
    let mut deltas = Vec::new();
    let r = with_vendor_ctx(Vendor::Anthropic, KEY_PREFIX, http, |ctx| {
        AnthropicAdapter.complete(ctx, req, &mut |t| deltas.push(t.to_string()))
    });
    (r, deltas)
}

#[test]
fn request_bytes_match_the_golden_and_carry_key_and_version_headers_only() {
    let http = Recorded::one(200, JSON, &fixture("anthropic/response-text.json"));
    let (r, _) = call(&http, &tool_conversation("claude-sonnet-5-5", false));
    expect_ok(r, "a recorded text answer");
    let sent = http.sent();
    assert_eq!(sent.url, "https://api.anthropic.com/v1/messages");
    assert_eq!(sent.body, Some(fixture_json("anthropic/request.golden.json")));
    assert!(sent.auth.starts_with("x-api-key: sk-ant-api"), "the key goes in x-api-key");
    assert!(sent
        .headers
        .iter()
        .any(|(k, v)| k == "anthropic-version" && v == ANTHROPIC_VERSION));
    // Streaming adds only `stream: true` to the same body.
    let http = Recorded::one(200, SSE, &fixture("anthropic/stream-tool-use.sse"));
    let (r, _) = call(&http, &tool_conversation("claude-sonnet-5-5", true));
    expect_ok(r, "a recorded stream");
    let mut golden = fixture_json("anthropic/request.golden.json");
    golden["stream"] = json!(true);
    assert_eq!(http.sent().body, Some(golden));
}

#[test]
fn system_text_anywhere_is_folded_into_the_system_field_and_turns_merge() {
    let mut req = tool_conversation("claude-opus-5-5", false);
    req.messages.push(ChatMessage {
        role: Role::System,
        content: "Answer in Celsius.".into(),
        tool_calls: vec![],
        tool_call_id: None,
    });
    let b = expect_ok(to_wire(&req), "wire body");
    assert_eq!(b["system"], "You are Hermes. Be brief.\n\nAnswer in Celsius.");
    let roles: Option<Vec<&str>> = b["messages"]
        .as_array()
        .map(|ms| ms.iter().filter_map(|m| m["role"].as_str()).collect());
    assert_eq!(roles, Some(vec!["user", "assistant", "user"]), "alternating, merged");
}

#[test]
fn a_non_streamed_answer_maps_text_tool_use_and_usage_including_cache_tokens() {
    let http = Recorded::one(200, JSON, &fixture("anthropic/response-tool-use.json"));
    let (r, _) = call(&http, &tool_conversation("claude-opus-5-5", false));
    assert_eq!(
        r,
        Ok(ChatResponse {
            content: "Okay, let's check the weather for San Francisco, CA:".into(),
            tool_calls: vec![ToolCall {
                id: "toolu_01T1x1fJ34qAmk2tNTrN7Up6".into(),
                name: "get_weather".into(),
                arguments: "{\"location\":\"San Francisco, CA\"}".into(),
            }],
            // 472 fresh + 100 cache writes + 2000 cache reads.
            usage: Some(Usage {
                input_tokens: 2572,
                output_tokens: 89
            }),
            finish: FinishReason::ToolCalls,
        })
    );
    let http = Recorded::one(200, JSON, &fixture("anthropic/response-text.json"));
    let (r, _) = call(&http, &tool_conversation("claude-opus-5-5", false));
    let r = expect_ok(r, "text answer");
    assert_eq!(r.finish, FinishReason::Stop);
    assert_eq!(r.usage, Some(Usage { input_tokens: 25, output_tokens: 18 }));
}

#[test]
fn the_documented_tool_use_stream_yields_text_deltas_a_joined_call_and_cumulative_usage() {
    let http = Recorded::one(200, SSE, &fixture("anthropic/stream-tool-use.sse"));
    let (r, deltas) = call(&http, &tool_conversation("claude-opus-5-5", true));
    let r = expect_ok(r, "the documented stream");
    assert_eq!(deltas.first().map(String::as_str), Some("Okay"));
    assert_eq!(deltas.concat(), "Okay, let's check the weather for San Francisco, CA:");
    assert_eq!(r.content, deltas.concat());
    assert_eq!(
        r.tool_calls,
        vec![ToolCall {
            id: "toolu_01T1x1fJ34qAmk2tNTrN7Up6".into(),
            name: "get_weather".into(),
            arguments: "{\"location\":\"San Francisco, CA\"}".into(),
        }]
    );
    // Input from message_start, output from the (cumulative) message_delta, not 2 + 89.
    assert_eq!(r.usage, Some(Usage { input_tokens: 472, output_tokens: 89 }));
    assert_eq!(r.finish, FinishReason::ToolCalls);
}

#[test]
fn a_stream_without_message_stop_or_with_an_error_event_is_an_error() {
    let http = Recorded::one(200, SSE, &fixture("anthropic/stream-cut.sse"));
    let (r, deltas) = call(&http, &tool_conversation("claude-opus-5-5", true));
    assert_eq!(r, Err(ProviderError::Transport("the answer stream ended early")));
    assert_eq!(deltas, vec!["Okay".to_string()], "text already shown is not taken back");
    let http = Recorded::one(200, SSE, &fixture("anthropic/stream-error.sse"));
    let (r, _) = call(&http, &tool_conversation("claude-opus-5-5", true));
    assert_eq!(r, Err(ProviderError::Status(529)), "overloaded_error");
}

#[test]
fn http_errors_map_to_coarse_errors_that_carry_no_body() {
    for (status, want) in [
        (401, ProviderError::Unauthorized),
        (403, ProviderError::Unauthorized),
        (429, ProviderError::RateLimited),
        (529, ProviderError::Status(529)),
        (500, ProviderError::Status(500)),
        (302, ProviderError::Redirect),
    ] {
        let http = Recorded::one(status, JSON, &fixture("anthropic/error-401.json"));
        let (r, _) = call(&http, &tool_conversation("claude-opus-5-5", false));
        assert_eq!(r, Err(want.clone()), "HTTP {status}");
        assert!(!format!("{want}").contains("invalid x-api-key"));
        assert_eq!(http.count(), 1, "a redirect is never followed");
    }
}

#[test]
fn key_test_lists_models_and_key_format_wants_the_documented_prefix() {
    let http = Recorded::one(200, JSON, &fixture("anthropic/models.json"));
    let t = with_vendor_ctx(Vendor::Anthropic, KEY_PREFIX, &http, |ctx| {
        AnthropicAdapter.test_key(ctx)
    });
    assert_eq!(
        t,
        Ok(KeyTest {
            models: vec!["claude-opus-5-5".into(), "claude-haiku-5-5".into()]
        })
    );
    let sent = http.sent();
    assert_eq!(sent.url, "https://api.anthropic.com/v1/models?limit=1000");
    assert_eq!(sent.method, Method::Get);
    let key = |s: String| ApiKey::from_secret(crate::secret::SecretString::new(s));
    let fake = crate::provider_fixtures_tests::fake_key;
    assert!(AnthropicAdapter.check_key_format(&key(fake("sk-ant-api03-"))).is_ok());
    for wrong in ["sk-proj-", "xai-", "AIza", "LLM_"] {
        assert!(
            matches!(
                AnthropicAdapter.check_key_format(&key(fake(wrong))),
                Err(ProviderError::KeyFormat(_))
            ),
            "{wrong}"
        );
    }
}

#[test]
fn a_history_it_cannot_express_is_refused_before_anything_is_sent() {
    let mut req = tool_conversation("claude-opus-5-5", false);
    req.messages[2].tool_calls[0].arguments = "[1,2]".into();
    let http = Recorded::new(vec![]);
    let (r, _) = call(&http, &req);
    assert!(matches!(r, Err(ProviderError::BadRequest(_))), "{r:?}");
    let mut req = tool_conversation("claude-opus-5-5", false);
    req.messages[3].tool_call_id = None;
    let (r, _) = call(&http, &req);
    assert!(matches!(r, Err(ProviderError::BadRequest(_))), "{r:?}");
    assert_eq!(http.count(), 0);
}

#[test]
fn every_catalog_model_is_priced_from_the_vendor_page() {
    let models = AnthropicAdapter.models();
    assert!(models.len() >= 4);
    for m in &models {
        assert!(m.input_micros_per_mtok.is_some() && m.output_micros_per_mtok.is_some(), "{}", m.id);
    }
    let haiku = models.iter().find(|m| m.id == "claude-haiku-5-5").unwrap();
    assert_eq!(
        (haiku.input_micros_per_mtok, haiku.output_micros_per_mtok),
        (Some(500_000), Some(2_500_000)),
        "the over-100k rate, so the cap never under-counts"
    );
}
