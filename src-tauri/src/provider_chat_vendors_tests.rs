// D-11 / WP-S1-PROV-ADAPT — the xAI, DeepSeek and Meta adapters against recorded answers
// (fixtures in tests/fixtures/providers/{xai,deepseek,meta}, built from the vendors' documented
// examples). No network.

use super::*;
use crate::model_provider::{FinishReason, ToolCall};
use crate::provider_fixtures_tests::{
    expect_ok, fake_key, fixture, fixture_json, tool_conversation, with_vendor_ctx, Recorded,
};

const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

fn adapter(v: Vendor) -> ChatVendorAdapter {
    match v {
        Vendor::Xai => ChatVendorAdapter::xai(),
        Vendor::DeepSeek => ChatVendorAdapter::deepseek(),
        Vendor::Meta => ChatVendorAdapter::meta(),
        other => panic!("{other:?} is not a chat-completions vendor"),
    }
}

fn prefix(v: Vendor) -> &'static str {
    match v {
        Vendor::Xai => "xai-",
        Vendor::DeepSeek => "sk-",
        _ => "LLM_1607358788850350_",
    }
}

fn call(
    v: Vendor,
    http: &Recorded,
    req: &ChatRequest,
) -> (Result<ChatResponse, ProviderError>, Vec<String>) {
    let mut deltas = Vec::new();
    let r = with_vendor_ctx(v, prefix(v), http, |ctx| {
        adapter(v).complete(ctx, req, &mut |t| deltas.push(t.to_string()))
    });
    (r, deltas)
}

#[test]
fn request_bytes_match_each_vendors_golden_on_its_pinned_host() {
    for (v, model, url) in [
        (Vendor::Xai, "grok-4.7", "https://api.x.ai/v1/chat/completions"),
        (Vendor::DeepSeek, "deepseek-flash", "https://api.deepseek.com/v1/chat/completions"),
        (Vendor::Meta, "muse-spark-1.3", "https://api.meta.ai/v1/chat/completions"),
    ] {
        let http = Recorded::one(200, JSON, &fixture("deepseek/response.json"));
        let (r, _) = call(v, &http, &tool_conversation(model, false));
        expect_ok(r, model);
        let sent = http.sent();
        assert_eq!(sent.url, url);
        assert_eq!(
            sent.body,
            Some(fixture_json(&format!("{}/request.golden.json", v.id()))),
            "{model}"
        );
        assert!(sent.auth.starts_with(&format!("authorization: Bearer {}", prefix(v))));
    }
}

#[test]
fn deepseek_turns_thinking_off_only_when_tools_ride_along() {
    let a = ChatVendorAdapter::deepseek();
    let mut req = tool_conversation("deepseek-flash", true);
    assert_eq!(a.to_wire(&req)["thinking"], json!({"type": "disabled"}));
    assert_eq!(a.to_wire(&req)["stream_options"]["include_usage"], true);
    req.tools.clear();
    assert!(a.to_wire(&req).get("thinking").is_none(), "vendor default without tools");
    assert!(ChatVendorAdapter::xai().to_wire(&tool_conversation("grok-4.7", false)).get("thinking").is_none());
}

#[test]
fn xai_reasoning_tokens_are_added_to_output_because_completion_tokens_excludes_them() {
    let http = Recorded::one(200, JSON, &fixture("xai/response.json"));
    let (r, _) = call(Vendor::Xai, &http, &tool_conversation("grok-4.7", false));
    let r = expect_ok(r, "the documented xAI answer");
    assert_eq!(r.content, "101 multiplied by 3 is 303.");
    // 32 prompt; 9 completion + 94 reasoning (total_tokens 135 = 32 + 9 + 94).
    assert_eq!(r.usage, Some(Usage { input_tokens: 32, output_tokens: 103 }));
    // The same counts are taken as reported for a vendor whose completion_tokens includes them.
    let http = Recorded::one(200, JSON, &fixture("xai/response.json"));
    let (r, _) = call(Vendor::Meta, &http, &tool_conversation("muse-spark-1.3", false));
    assert_eq!(expect_ok(r, "meta").usage, Some(Usage { input_tokens: 32, output_tokens: 9 }));
}

#[test]
fn xai_stream_takes_a_whole_call_per_chunk_and_the_last_running_usage() {
    let http = Recorded::one(200, SSE, &fixture("xai/stream-tool-call.sse"));
    let (r, deltas) = call(Vendor::Xai, &http, &tool_conversation("grok-4.7", true));
    assert_eq!(deltas, vec!["Checking.".to_string()], "reasoning_content is not shown");
    assert_eq!(
        r,
        Ok(ChatResponse {
            content: "Checking.".into(),
            tool_calls: vec![ToolCall {
                id: "call_xai_1".into(),
                name: "get_weather".into(),
                arguments: "{\"location\":\"Boston, MA\"}".into(),
            }],
            usage: Some(Usage { input_tokens: 41, output_tokens: 40 }),
            finish: FinishReason::ToolCalls,
        })
    );
}

#[test]
fn deepseek_stream_skips_keep_alives_and_reasoning_and_reads_usage_on_the_last_chunk() {
    let http = Recorded::one(200, SSE, &fixture("deepseek/stream-text.sse"));
    let (r, deltas) = call(Vendor::DeepSeek, &http, &tool_conversation("deepseek-flash", true));
    assert_eq!(deltas, vec!["Hello".to_string(), "!".to_string()]);
    assert_eq!(
        r,
        Ok(ChatResponse {
            content: "Hello!".into(),
            tool_calls: vec![],
            usage: Some(Usage { input_tokens: 17, output_tokens: 9 }),
            finish: FinishReason::Stop,
        })
    );
    let http = Recorded::one(200, JSON, &format!("\n\n{}", fixture("deepseek/response.json")));
    let (r, _) = call(Vendor::DeepSeek, &http, &tool_conversation("deepseek-flash", false));
    assert_eq!(
        expect_ok(r, "leading keep-alive blank lines").usage,
        Some(Usage { input_tokens: 16, output_tokens: 10 })
    );
}

#[test]
fn meta_answers_in_the_openai_shape_with_tool_call_pieces_and_final_usage() {
    let http = Recorded::one(200, JSON, &fixture("meta/response-tool-call.json"));
    let (r, _) = call(Vendor::Meta, &http, &tool_conversation("muse-spark-1.3", false));
    let r = expect_ok(r, "the documented Meta tool call");
    assert_eq!(r.tool_calls[0].id, "call_abc123");
    assert_eq!(r.tool_calls[0].arguments, "{\"location\": \"Seattle, WA\"}");
    assert_eq!(r.usage, Some(Usage { input_tokens: 25, output_tokens: 19 }));
    assert_eq!(r.finish, FinishReason::ToolCalls);
    let http = Recorded::one(200, SSE, &fixture("meta/stream-tool-call.sse"));
    let (r, deltas) = call(Vendor::Meta, &http, &tool_conversation("muse-spark-1.3", true));
    assert!(deltas.is_empty());
    let r = expect_ok(r, "the Meta stream");
    assert_eq!(r.tool_calls[0].id, "call_m1");
    assert_eq!(r.tool_calls[0].arguments, "{\"location\":\"Seattle, WA\"}");
    assert_eq!(r.usage, Some(Usage { input_tokens: 25, output_tokens: 19 }));
}

#[test]
fn errors_map_per_vendor_including_deepseeks_insufficient_balance() {
    for v in [Vendor::Xai, Vendor::DeepSeek, Vendor::Meta] {
        for (status, want) in [
            (401, ProviderError::Unauthorized),
            (429, ProviderError::RateLimited),
            (402, ProviderError::Status(402)),
            (503, ProviderError::Status(503)),
            (308, ProviderError::Redirect),
        ] {
            let http = Recorded::one(status, JSON, &fixture("meta/error-401.json"));
            let (r, _) = call(v, &http, &tool_conversation("m", false));
            assert_eq!(r, Err(want), "{v:?} HTTP {status}");
            assert_eq!(http.count(), 1);
        }
    }
    let cut = fixture("meta/stream-tool-call.sse");
    // Cut at the start of the chunk that would have finished the call (a dropped connection).
    let at = cut[..cut.find("Seattle").unwrap()].rfind("data:").unwrap();
    let cut = &cut[..at];
    let http = Recorded::one(200, SSE, cut);
    let (r, _) = call(Vendor::Meta, &http, &tool_conversation("muse-spark-1.3", true));
    assert_eq!(r, Err(ProviderError::Transport("the answer stream ended early")));
}

#[test]
fn key_tests_list_models_and_key_formats_follow_each_vendor() {
    for (v, url, first) in [
        (Vendor::Xai, "https://api.x.ai/v1/models", "grok-4.7"),
        (Vendor::DeepSeek, "https://api.deepseek.com/v1/models", "deepseek-flash"),
        (Vendor::Meta, "https://api.meta.ai/v1/models", "muse-spark-1.3"),
    ] {
        let http = Recorded::one(200, JSON, &fixture(&format!("{}/models.json", v.id())));
        let t = with_vendor_ctx(v, prefix(v), &http, |ctx| adapter(v).test_key(ctx));
        let t = expect_ok(t, url);
        assert_eq!(t.models.first().map(String::as_str), Some(first));
        assert_eq!(http.sent().url, url);
    }
    let key = |s: String| crate::model_provider::ApiKey::from_secret(crate::secret::SecretString::new(s));
    assert!(ChatVendorAdapter::meta().key_format().check(&key(fake_key("LLM_1607358788850350_"))).is_ok());
    assert!(ChatVendorAdapter::meta().key_format().check(&key(fake_key("LLM|"))).is_err(), "the retired Llama API shape");
    assert!(ChatVendorAdapter::xai().key_format().check(&key(fake_key("xai-"))).is_ok());
    assert!(ChatVendorAdapter::xai().key_format().check(&key(fake_key("sk-"))).is_err());
    assert!(ChatVendorAdapter::deepseek().key_format().check(&key(fake_key("sk-"))).is_ok());
    assert!(ChatVendorAdapter::deepseek().key_format().check(&key(fake_key("xai-"))).is_err());
}

#[test]
fn every_catalog_model_is_priced_and_meta_contributor_mode_is_not_preselected() {
    for v in [Vendor::Xai, Vendor::DeepSeek, Vendor::Meta] {
        let models = adapter(v).models();
        assert!(!models.is_empty(), "{v:?}");
        for m in &models {
            assert!(m.input_micros_per_mtok.is_some() && m.output_micros_per_mtok.is_some(), "{}", m.id);
        }
    }
    assert!(!adapter(Vendor::Meta).models().iter().any(|m| m.id.contains("contributor")));
    let pro = catalog(Vendor::DeepSeek).into_iter().find(|m| m.id == "deepseek-v4-pro").unwrap();
    assert_eq!(
        (pro.input_micros_per_mtok, pro.output_micros_per_mtok),
        (Some(1_320_000), Some(3_960_000)),
        "the peak-hour rate, so the cap never under-counts"
    );
}
