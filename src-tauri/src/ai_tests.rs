// CORE-AI1 — real AI inference (@rule8 evidence). Red-first, CI-safe.
//
// A mock keyring (in-memory map) + a mock HTTP client (records requests, scripts
// responses) — NO real OS keyring, NO live socket (Rule 1: both are TEST seams,
// never wired as the production default). Covers:
//   * https validation at set-time (reject http/other schemes — invariant 4).
//   * the sealed-config round-trip (baseURL + model + apiKey bound in ONE blob).
//   * ai_provider_status returns metadata ONLY — never the key (invariant 1).
//   * the request-body shape (endpoint {baseURL}/chat/completions, Bearer header
//     present, system+context+messages) and the choices[0].message.content parse.
//   * CRITICAL: ai_chat uses the STORED baseURL, not a caller-supplied one — the
//     exfil-binding guard, proven as a real negative control (invariant 3).

use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Mock keyring — an in-memory map (mirrors custody's in-memory fake). Never
// touches a real OS keyring in CI.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MockKeyring {
    map: Mutex<HashMap<String, String>>,
}
impl MockKeyring {
    fn new() -> Self {
        MockKeyring::default()
    }
}
impl AiKeyring for MockKeyring {
    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self.map.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<()> {
        self.map
            .lock()
            .unwrap()
            .insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<()> {
        self.map.lock().unwrap().remove(account);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mock HTTP client — records (url, bearer, body) and scripts a response, shared
// via `Rc` so a test can read recorded calls AFTER the manager takes ownership of
// its `Box<dyn AiHttpClient>` handle. Lets a test assert the EXACT request shape
// (endpoint, Bearer, messages) with no live socket.
// ---------------------------------------------------------------------------

struct SharedHttp {
    calls: RefCell<Vec<(String, String, Value)>>,
    responses: RefCell<VecDeque<Result<String>>>,
}
impl SharedHttp {
    fn new(responses: Vec<Result<String>>) -> Rc<Self> {
        Rc::new(SharedHttp {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(responses.into_iter().collect()),
        })
    }
    fn calls(&self) -> Vec<(String, String, Value)> {
        self.calls.borrow().clone()
    }
}

/// A handle the manager owns; it forwards to the shared mock the test also holds.
struct SharedHttpHandle(Rc<SharedHttp>);
// Single-threaded test use; the trait bound needs Send+Sync.
unsafe impl Send for SharedHttpHandle {}
unsafe impl Sync for SharedHttpHandle {}
impl AiHttpClient for SharedHttpHandle {
    fn post_json(&self, url: &str, bearer: &str, body: &Value) -> Result<String> {
        self.0
            .calls
            .borrow_mut()
            .push((url.to_string(), bearer.to_string(), body.clone()));
        self.0
            .responses
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(AiError::Network))
    }
}

/// Build a manager over a fresh mock keyring + the given shared HTTP mock.
fn mgr_with(http: &Rc<SharedHttp>) -> AiManager {
    AiManager::new(
        Box::new(MockKeyring::new()),
        Box::new(SharedHttpHandle(http.clone())),
    )
}

/// Build a manager with no HTTP calls scripted (for set/status/clear tests).
fn mgr_no_http() -> AiManager {
    AiManager::new(
        Box::new(MockKeyring::new()),
        Box::new(SharedHttpHandle(SharedHttp::new(vec![]))),
    )
}

/// A well-formed OpenAI chat-completions response body carrying `content`.
fn completion_response(content: &str) -> String {
    json!({
        "id": "chatcmpl-x",
        "choices": [ { "index": 0, "message": { "role": "assistant", "content": content } } ]
    })
    .to_string()
}

const OPENAI_BASE: &str = "https://api.openai.com/v1";
const GATEWAY_BASE: &str = "https://infer.citrate.ai/v1";

// ===========================================================================
// invariant 4 — https validation at set-time
// ===========================================================================

#[test]
fn rejects_non_https_base_url() {
    let mgr = mgr_no_http();
    // http:// is rejected.
    assert_eq!(
        mgr.set_provider("custom", "http://evil.example/v1", "gpt-4o", "sk-abc12345"),
        Err(AiError::BadBaseUrl)
    );
    // ws:// / file:// / garbage all rejected.
    for bad in ["ws://x/v1", "file:///etc/passwd", "not a url", "ftp://x/v1"] {
        assert_eq!(
            mgr.set_provider("custom", bad, "m", "sk-abc12345"),
            Err(AiError::BadBaseUrl),
            "must reject non-https base url: {bad}"
        );
    }
    // A valid https URL is accepted.
    assert!(mgr
        .set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-abc12345")
        .is_ok());
}

#[test]
fn rejects_empty_key_and_bad_provider_id() {
    let mgr = mgr_no_http();
    assert_eq!(
        mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "   "),
        Err(AiError::BadKey)
    );
    // An id with a path separator / uppercase / the reserved "default" is rejected
    // so it can never escape the `ai-<id>` keyring namespace.
    for bad in ["", "OpenAI", "a/b", "ai default", "default"] {
        assert_eq!(
            mgr.set_provider(bad, OPENAI_BASE, "gpt-4o", "sk-abc12345"),
            Err(AiError::BadProviderId),
            "must reject bad provider id: {bad:?}"
        );
    }
}

// ===========================================================================
// invariant 3/2 — the sealed-config round-trip (baseURL+model+apiKey bound)
// ===========================================================================

#[test]
fn seals_config_and_binds_key_to_base_url() {
    let mgr = mgr_no_http();
    mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-secret-xyz")
        .expect("seal ok");
    // The sealed blob binds all three together (read back in-process).
    let cfg = mgr.read_config("openai").expect("read back");
    assert_eq!(cfg.base_url, OPENAI_BASE);
    assert_eq!(cfg.model, "gpt-4o");
    assert_eq!(cfg.api_key, "sk-secret-xyz");
}

#[test]
fn trailing_slash_is_trimmed_so_endpoint_never_doubles() {
    let mgr = mgr_no_http();
    mgr.set_provider("custom", "https://x.example/v1/", "m", "sk-abc12345")
        .expect("seal");
    let cfg = mgr.read_config("custom").expect("read");
    assert_eq!(cfg.base_url, "https://x.example/v1", "trailing slash trimmed");
}

// ===========================================================================
// invariant 1 — ai_provider_status returns metadata ONLY, never the key
// ===========================================================================

#[test]
fn provider_status_never_leaks_the_key() {
    let mgr = mgr_no_http();
    mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-super-secret-KEY")
        .expect("seal");
    let statuses = mgr
        .provider_status(&["openai", "gateway", "custom"])
        .expect("status");
    // openai is configured; the others are honestly unconfigured (no fabrication).
    let openai = statuses.iter().find(|s| s.id == "openai").unwrap();
    assert!(openai.configured);
    assert_eq!(openai.base_url, OPENAI_BASE);
    assert_eq!(openai.model, "gpt-4o");
    assert!(openai.is_default, "first-configured becomes the default route");
    let gateway = statuses.iter().find(|s| s.id == "gateway").unwrap();
    assert!(!gateway.configured);
    assert!(gateway.base_url.is_empty());
    // CRITICAL: the serialized status carries NO key material anywhere (invariant 1).
    let json = serde_json::to_string(&statuses).unwrap();
    assert!(
        !json.contains("sk-super-secret-KEY"),
        "provider status must never serialize the api key"
    );
    assert!(
        !json.to_lowercase().contains("apikey") && !json.contains("Authorization"),
        "no apiKey/Authorization field in the status shape"
    );
}

#[test]
fn clear_provider_removes_config_and_default() {
    let mgr = mgr_no_http();
    mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-abc12345")
        .expect("seal");
    assert!(mgr.default_provider().unwrap().as_deref() == Some("openai"));
    mgr.clear_provider("openai").expect("clear");
    assert!(matches!(
        mgr.read_config("openai"),
        Err(AiError::NotConfigured)
    ));
    // The default was cleared too (it pointed at the now-deleted config).
    assert_eq!(mgr.default_provider().unwrap(), None);
}

// ===========================================================================
// request-body shape + completion parse (over the MOCK http seam)
// ===========================================================================

#[test]
fn chat_posts_openai_body_to_stored_endpoint_with_bearer() {
    let shared = SharedHttp::new(vec![Ok(completion_response("Your node is validating."))]);
    let mgr = mgr_with(&shared);
    mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-live-KEY")
        .expect("seal");

    let messages = json!([{ "role": "user", "content": "how is my node?" }]).to_string();
    let context = json!({ "height": 131234, "nodeState": "validating" }).to_string();
    let out = mgr.chat("openai", &messages, &context).expect("chat");
    assert_eq!(out, "Your node is validating.");

    let calls = shared.calls();
    assert_eq!(calls.len(), 1);
    let (url, bearer, body) = &calls[0];
    // Endpoint = STORED baseURL + /chat/completions (no double slash).
    assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    // Bearer is the sealed key (the seam receives it; it never crosses invoke).
    assert_eq!(bearer, "sk-live-KEY");
    // The model + a non-streaming flag + the messages array (system,context,user).
    assert_eq!(body["model"], "gpt-4o");
    assert_eq!(body["stream"], false);
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system", "real tool-less system prompt first");
    assert!(
        msgs[0]["content"].as_str().unwrap().contains("validating"),
        "the live context rides in the one leading system message"
    );
    let last = msgs.last().unwrap();
    assert_eq!(last["role"], "user");
    assert_eq!(last["content"], "how is my node?");
}

#[test]
fn chat_surfaces_coarse_error_without_body_on_provider_failure() {
    let shared = SharedHttp::new(vec![Err(AiError::Provider)]);
    let mgr = mgr_with(&shared);
    mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-abc12345")
        .expect("seal");
    let err = mgr
        .chat("openai", &json!([]).to_string(), &json!({}).to_string())
        .unwrap_err();
    assert_eq!(err, AiError::Provider);
    // The error string is coarse + secret-free (no key, no URL, no body).
    let s = err.to_string();
    assert!(!s.contains("sk-"));
    assert!(!s.contains("api.openai.com"));
}

#[test]
fn chat_on_unconfigured_provider_fails_closed() {
    let shared = SharedHttp::new(vec![]);
    let mgr = mgr_with(&shared);
    let err = mgr
        .chat("openai", &json!([]).to_string(), &json!({}).to_string())
        .unwrap_err();
    assert_eq!(err, AiError::NotConfigured);
    // No HTTP call was attempted (fail closed before any egress).
    assert!(shared.calls().is_empty());
}

#[test]
fn bad_completion_shape_is_bad_response_not_fabricated() {
    let shared = SharedHttp::new(vec![Ok(json!({ "choices": [] }).to_string())]);
    let mgr = mgr_with(&shared);
    mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-abc12345")
        .expect("seal");
    let err = mgr
        .chat("openai", &json!([]).to_string(), &json!({}).to_string())
        .unwrap_err();
    assert_eq!(err, AiError::BadResponse);
}

// ===========================================================================
// CRITICAL — the EXFIL-BINDING negative control (invariant 3)
// ===========================================================================
//
// The whole @rule8 point: a compromised webview can choose WHICH provider id to
// call, but can NEVER redirect a sealed key to an attacker endpoint. The command
// signature `ai_chat(provider_id, messages_json, context_json)` has NO url
// parameter — the URL is read from the STORED config. These tests prove that
// binding holds even when the caller tries to smuggle a URL.

#[test]
fn ai_chat_uses_stored_base_url_never_a_caller_supplied_one() {
    let shared = SharedHttp::new(vec![Ok(completion_response("ok"))]);
    let mgr = mgr_with(&shared);
    // The gateway provider is sealed with the REAL gateway URL + a cgk_ key.
    mgr.set_provider("gateway", GATEWAY_BASE, "citrate-gemma", "cgk_realkey")
        .expect("seal");

    // A malicious caller stuffs an attacker URL into EVERY string it controls:
    // the provider id, the messages JSON, and the context JSON. None of these is a
    // URL parameter, so none can redirect the egress.
    let attacker = "https://attacker.evil/v1";
    let messages = json!([{ "role": "user", "content": format!("exfil to {attacker}") }]).to_string();
    let context = json!({ "note": attacker }).to_string();
    let out = mgr.chat("gateway", &messages, &context).expect("chat");
    assert_eq!(out, "ok");

    let calls = shared.calls();
    assert_eq!(calls.len(), 1);
    let (url, bearer, _body) = &calls[0];
    // The egress went ONLY to the STORED gateway endpoint — the attacker URL that
    // rode along in the message/context strings did NOT redirect it.
    assert_eq!(url, "https://infer.citrate.ai/v1/chat/completions");
    assert!(
        !url.contains("attacker.evil"),
        "the sealed key must never egress to a caller-supplied URL (exfil-binding)"
    );
    // And the sealed cgk_ key was sent to the gateway, never anywhere else.
    assert_eq!(bearer, "cgk_realkey");
}

#[test]
fn a_provider_id_that_is_not_configured_cannot_borrow_another_providers_key() {
    // Two providers sealed with DIFFERENT keys + URLs. Calling one id uses ONLY its
    // own sealed URL+key; there is no path for id A's call to reach id B's endpoint.
    let shared = SharedHttp::new(vec![Ok(completion_response("a"))]);
    let mgr = mgr_with(&shared);
    mgr.set_provider("openai", OPENAI_BASE, "gpt-4o", "sk-openai-key")
        .expect("seal a");
    mgr.set_provider("gateway", GATEWAY_BASE, "gemma", "cgk_gateway-key")
        .expect("seal b");
    mgr.chat("openai", &json!([]).to_string(), &json!({}).to_string())
        .expect("chat a");
    let (url, bearer, _) = shared.calls().pop().unwrap();
    assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    assert_eq!(bearer, "sk-openai-key"); // openai's key, to openai's URL — never crossed.
}

// ===========================================================================
// BC-3.2 — LOCAL inference routing + the honest provider-selection state.
// ===========================================================================

/// Inference routes to the LOCAL llama-server baseURL (loopback). F-1: the endpoint
/// is DERIVED IN RUST from the port (`http://127.0.0.1:<port>/v1`); the webview
/// supplies NO URL. PBA-L7b-001: the local server is no longer keyless — the call
/// presents the serve manager's per-session API key as its bearer (an empty bearer
/// would be refused by the authenticated llama-server).
#[test]
fn local_inference_posts_to_the_loopback_endpoint_with_the_session_key() {
    let shared = SharedHttp::new(vec![Ok(completion_response("local model reply"))]);
    let mgr = mgr_with(&shared);
    let out = mgr
        .chat_local(
            18080,
            "k3y-for-this-session",
            "gemma-4-E4B-it",
            &json!([{ "role": "user", "content": "hi" }]).to_string(),
            &json!({ "height": 1 }).to_string(),
        )
        .expect("local chat");
    assert_eq!(out, "local model reply");
    let calls = shared.calls();
    assert_eq!(calls.len(), 1);
    let (url, bearer, body) = &calls[0];
    assert_eq!(url, "http://127.0.0.1:18080/v1/chat/completions");
    let parsed = url::Url::parse(url).unwrap();
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    assert_eq!(parsed.scheme(), "http");
    assert!(parsed.username().is_empty() && parsed.password().is_none());
    // PBA-L7b-001: the session key is the bearer.
    assert_eq!(bearer, "k3y-for-this-session");
    assert_eq!(body["model"], "gemma-4-E4B-it");
}

/// PBA-L7b-001: the agentic local path presents the same session key.
#[test]
fn pba_l7b_001_local_tools_turn_presents_the_session_key() {
    let shared = SharedHttp::new(vec![Ok(json!({
        "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
    })
    .to_string())]);
    let mgr = mgr_with(&shared);
    mgr.chat_local_tools(
        18080,
        "tools-session-key",
        "gemma",
        &json!([]).to_string(),
        &json!([]).to_string(),
        &json!({}).to_string(),
    )
    .expect("local tools turn");
    let calls = shared.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, "tools-session-key");
}

/// F-1 (BLOCKING) — the LOCAL path takes NO webview-supplied URL. `chat_local`
/// accepts only a `port` (a `u16`), so the whole class of "smuggle a remote host
/// past a prefix check" is eliminated: a compromised renderer literally cannot
/// name a host. This test documents that the derived URL, for EVERY port,
/// resolves to a loopback host with no userinfo — the exfil vectors that defeated
/// the old `starts_with("http://127.0.0.1:")` string check
/// (`http://127.0.0.1:8080@evil.com/v1`, `http://127.0.0.1:@evil.com/v1`,
/// `http://127.0.0.1.evil.com/v1`) can never be constructed from a bare port.
#[test]
fn local_inference_url_is_rust_derived_and_never_carries_a_foreign_host() {
    // The three historical exfil vectors — proof they would NOT survive the
    // rigorous host check the derived URL is built to satisfy. None has host
    // 127.0.0.1 with empty userinfo; all must be refused by the guard.
    let exfil_vectors = [
        "http://127.0.0.1:8080@evil.com/v1", // userinfo trick — host is evil.com
        "http://127.0.0.1:@evil.com/v1",     // empty-port userinfo trick
        "http://127.0.0.1.evil.com/v1",      // suffix trick — host is 127.0.0.1.evil.com
    ];
    for v in exfil_vectors {
        assert!(
            !loopback_url_is_safe(v),
            "the loopback guard MUST refuse the exfil vector: {v}"
        );
    }
    // A Rust-derived URL from any port is always safe (loopback, http, no userinfo).
    for port in [1u16, 18080, 65535] {
        let derived = format!("http://127.0.0.1:{port}/v1");
        assert!(
            loopback_url_is_safe(&derived),
            "the Rust-derived loopback URL must pass the guard: {derived}"
        );
    }
    // The genuine loopback aliases are accepted; anything else is refused.
    assert!(loopback_url_is_safe("http://127.0.0.1:18080/v1"));
    assert!(loopback_url_is_safe("http://[::1]:18080/v1"));
    assert!(loopback_url_is_safe("http://localhost:18080/v1"));
    // https / remote / wrong-scheme all refused.
    for bad in [
        "https://127.0.0.1:18080/v1",
        "http://10.0.0.1:18080/v1",
        "http://evil.example.com/v1",
        "ws://127.0.0.1:18080/v1",
    ] {
        assert!(!loopback_url_is_safe(bad), "must refuse: {bad}");
    }
}

/// The LOCAL path fails closed on a non-loopback endpoint with NO egress. Because
/// `chat_local` now takes a `port` (never a URL), the internal guard is the last
/// line of defence against a future refactor reintroducing a foreign host — it is
/// exercised directly via `loopback_url_is_safe` above. Here we prove that the
/// end-to-end `chat_local` never egresses when its derived URL would be unsafe
/// (it cannot be, from a port — so egress ALWAYS goes to loopback).
#[test]
fn local_inference_rejects_non_loopback_url_no_egress() {
    let shared = SharedHttp::new(vec![Ok(completion_response("local model reply"))]);
    let mgr = mgr_with(&shared);
    // A port can only ever produce a loopback URL; the call succeeds AND the only
    // egress is to 127.0.0.1 — never a remote (the webview has no way to name one).
    let out = mgr
        .chat_local(
            18080,
            "k",
            "gemma",
            &json!([]).to_string(),
            &json!({}).to_string(),
        )
        .expect("local chat over a port always targets loopback");
    assert_eq!(out, "local model reply");
    let calls = shared.calls();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].0.starts_with("http://127.0.0.1:"),
        "egress is loopback-only; no foreign host is reachable from a port"
    );
    // And the guard itself refuses the classic exfil URL, no egress, if ever fed
    // one directly (belt-and-suspenders for a future refactor).
    assert!(!loopback_url_is_safe("http://127.0.0.1:8080@evil.com/v1"));
}

/// The provider-selection state machine: LOCAL (ready+healthy) wins; a ready
/// model with a dead server falls back to the gateway; a download in flight is
/// honestly Downloading; a gateway key alone is gateway-only; nothing → demo.
#[test]
fn inference_state_selection_is_honest_per_branch() {
    // LOCAL wins.
    assert_eq!(
        select_inference_state(ProviderInputs {
            model_ready: true,
            server_healthy: true,
            downloading: false,
            gateway_key_configured: false,
        }),
        InferenceState::Ready
    );
    // Ready model, dead server, gateway present → local-fallback.
    assert_eq!(
        select_inference_state(ProviderInputs {
            model_ready: true,
            server_healthy: false,
            downloading: false,
            gateway_key_configured: true,
        }),
        InferenceState::LocalFallback
    );
    // No local model, downloading → downloading (over gateway-only).
    assert_eq!(
        select_inference_state(ProviderInputs {
            model_ready: false,
            server_healthy: false,
            downloading: true,
            gateway_key_configured: true,
        }),
        InferenceState::Downloading
    );
    // No local model, not downloading, gateway present → gateway-only.
    assert_eq!(
        select_inference_state(ProviderInputs {
            model_ready: false,
            server_healthy: false,
            downloading: false,
            gateway_key_configured: true,
        }),
        InferenceState::GatewayOnly
    );
    // Nothing usable → demo.
    assert_eq!(
        select_inference_state(ProviderInputs {
            model_ready: false,
            server_healthy: false,
            downloading: false,
            gateway_key_configured: false,
        }),
        InferenceState::Demo
    );
}

// ---------------------------------------------------------------------------
// W3.3 — agentic tool loop (ai_chat_tools / build_chat_body_with_tools)
// ---------------------------------------------------------------------------

/// A chat-completions response whose assistant turn is a tool call (no content).
fn tool_call_response(id: &str, name: &str, args_json: &str) -> String {
    json!({
        "id": "chatcmpl-t",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": args_json }
                }]
            }
        }]
    })
    .to_string()
}

const TOOLS_SPEC: &str = r#"[{"type":"function","function":{"name":"memory_search","description":"search mem","parameters":{"type":"object","properties":{"query":{"type":"string"}}}}}]"#;

#[test]
fn chat_tools_posts_tools_and_returns_the_tool_call_message() {
    let shared = SharedHttp::new(vec![Ok(tool_call_response(
        "call_1",
        "memory_search",
        "{\"query\":\"staking\"}",
    ))]);
    let mgr = mgr_with(&shared);
    mgr.set_provider("gateway", GATEWAY_BASE, "citrate-1", "cgk_live_KEY")
        .expect("seal");

    let messages = json!([{ "role": "user", "content": "how does staking work?" }]).to_string();
    let context = json!({ "tier": "commercial" }).to_string();
    let out = mgr
        .chat_tools("gateway", &messages, TOOLS_SPEC, &context)
        .expect("chat_tools");

    // The returned assistant message preserves tool_calls for the frontend loop.
    let msg: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(msg["role"], "assistant");
    assert_eq!(msg["tool_calls"][0]["function"]["name"], "memory_search");

    // The request body carried the tools spec + the tool-aware system prompt.
    let (url, bearer, body) = shared.calls().pop().unwrap();
    assert_eq!(url, "https://infer.citrate.ai/v1/chat/completions");
    assert_eq!(bearer, "cgk_live_KEY");
    assert!(body["tools"].is_array(), "tools spec attached");
    let sys = body["messages"][0]["content"].as_str().unwrap();
    assert!(sys.contains("tools"), "tool-aware system prompt used");
}

#[test]
fn chat_tools_returns_plain_content_when_the_model_is_done() {
    let shared = SharedHttp::new(vec![Ok(completion_response("Staking locks 32k SALT."))]);
    let mgr = mgr_with(&shared);
    mgr.set_provider("gateway", GATEWAY_BASE, "citrate-1", "cgk_KEY")
        .expect("seal");
    let out = mgr
        .chat_tools("gateway", &json!([]).to_string(), TOOLS_SPEC, &json!({}).to_string())
        .expect("chat_tools");
    let msg: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(msg["content"], "Staking locks 32k SALT.");
    assert!(msg.get("tool_calls").is_none(), "no tool calls on a final answer");
}

#[test]
fn build_chat_body_with_tools_forwards_the_tool_protocol_shapes() {
    // An assistant turn with tool_calls, then a tool result carrying tool_call_id.
    let messages = json!([
        { "role": "user", "content": "recall my staking" },
        { "role": "assistant", "content": null, "tool_calls": [
            { "id": "c1", "type": "function", "function": { "name": "memory_recall", "arguments": "{}" } }
        ]},
        { "role": "tool", "tool_call_id": "c1", "content": "32000 SALT staked" }
    ])
    .to_string();
    let body = build_chat_body_with_tools("m", &messages, TOOLS_SPEC, "{}").unwrap();
    let msgs = body["messages"].as_array().unwrap();
    // system prompt + context + 3 forwarded turns.
    let asst = msgs.iter().find(|m| m["role"] == "assistant").unwrap();
    assert_eq!(asst["tool_calls"][0]["id"], "c1", "assistant tool_calls forwarded");
    let tool = msgs.iter().find(|m| m["role"] == "tool").unwrap();
    assert_eq!(tool["tool_call_id"], "c1", "tool result carries its call id");
    assert_eq!(tool["content"], "32000 SALT staked");
}

#[test]
fn parse_chat_message_rejects_an_empty_assistant_shape() {
    // No content AND no tool_calls → BadResponse (never a fabricated blank turn).
    let empty = json!({ "choices": [{ "message": { "role": "assistant" } }] }).to_string();
    assert!(matches!(parse_chat_message(&empty), Err(AiError::BadResponse)));
}

#[test]
fn build_chat_body_with_tools_rejects_a_non_array_tools_spec() {
    assert!(matches!(
        build_chat_body_with_tools("m", "[]", "{\"not\":\"an array\"}", "{}"),
        Err(AiError::BadResponse)
    ));
}


// ===========================================================================
// HUP-S0.1 — every chat request is BOUNDED (the pinwheel / runaway-generation fix)
// ===========================================================================

#[test]
fn every_chat_body_caps_generation_with_max_tokens() {
    // Without max_tokens a local model can generate until the context window fills, holding
    // the request (and, before S0.1, the main thread) for minutes.
    let plain = build_chat_body("m", "[]", "{}").unwrap();
    assert_eq!(plain["max_tokens"], AI_MAX_TOKENS);
    let tools = build_chat_body_with_tools("m", "[]", TOOLS_SPEC, "{}").unwrap();
    assert_eq!(tools["max_tokens"], AI_MAX_TOKENS);
    const { assert!(AI_MAX_TOKENS >= 512 && AI_MAX_TOKENS <= 4096, "a sane per-turn cap") };
}

#[test]
fn the_production_ai_http_client_has_connect_and_overall_deadlines() {
    // A dead endpoint or a hung generation must fail the turn, never hang it forever.
    assert!(AI_CONNECT_TIMEOUT <= std::time::Duration::from_secs(15));
    assert!(AI_REQUEST_TIMEOUT >= std::time::Duration::from_secs(60), "room for a slow local turn");
    assert!(AI_REQUEST_TIMEOUT <= std::time::Duration::from_secs(600), "but bounded");
}

// HUP-S0.5 — belt and braces: even if a model/template leaks control tokens or raw thinking into
// `content`, the chat never shows them.
#[test]
fn leaked_template_and_thinking_tokens_are_stripped_from_replies() {
    let raw = "<think>internal plan</think>Your node is <|im_end|>validating.<end_of_turn>";
    assert_eq!(strip_template_tokens(raw), "Your node is validating.");
    let raw2 = "<|im_start|>assistant\nHello<|eot_id|>";
    assert_eq!(strip_template_tokens(raw2), "Hello");
    // An unterminated think block (cut by max_tokens) is dropped, not shown.
    assert_eq!(strip_template_tokens("Answer first.<think>still thinking"), "Answer first.");
    // Ordinary text, including angle brackets in code, is untouched.
    let code = "Use `a <b> c` and x < y";
    assert_eq!(strip_template_tokens(code), code);
}

#[test]
fn parsed_replies_are_sanitized() {
    let resp = json!({"choices":[{"message":{"role":"assistant","content":"<think>x</think>Hi<|im_end|>"}}]}).to_string();
    let msg: Value = serde_json::from_str(&parse_chat_message(&resp).unwrap()).unwrap();
    assert_eq!(msg["content"], "Hi");
    assert_eq!(parse_completion(&resp).unwrap(), "Hi");
}


// Bug (owner, 2026-10-01): local models answered "ai: provider returned an error" while the gateway
// worked. The body carried TWO system messages (prompt, then the live context); Qwen-family
// `--jinja` chat templates raise "System message must be at the beginning" → llama-server 500.
// Contract: exactly one system message, first, holding the prompt AND the live context; any
// system-role message in the history is folded into it, never forwarded later.
fn system_positions(body: &Value) -> Vec<usize> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, m)| m["role"] == "system")
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn chat_bodies_carry_exactly_one_leading_system_message_with_the_context() {
    let history = json!([{ "role": "user", "content": "are you working?" }]).to_string();
    let ctx = json!({ "node": { "height": 57993 } }).to_string();
    for body in [
        build_chat_body("m", &history, &ctx).unwrap(),
        build_chat_body_with_tools("m", &history, TOOLS_SPEC, &ctx).unwrap(),
    ] {
        assert_eq!(system_positions(&body), vec![0], "{body}");
        let sys = body["messages"][0]["content"].as_str().unwrap();
        assert!(sys.contains("57993"), "live context inside the system message");
        assert_eq!(body["messages"].as_array().unwrap().last().unwrap()["content"], "are you working?");
    }
}

#[test]
fn a_system_message_in_the_history_is_folded_into_the_leading_one() {
    let history = json!([
        { "role": "user", "content": "hi" },
        { "role": "system", "content": "Brief: Lemon Drops" },
        { "role": "assistant", "content": "hello" }
    ])
    .to_string();
    for body in [
        build_chat_body("m", &history, "{}").unwrap(),
        build_chat_body_with_tools("m", &history, TOOLS_SPEC, "{}").unwrap(),
    ] {
        assert_eq!(system_positions(&body), vec![0], "{body}");
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("Brief: Lemon Drops"));
        let roles: Vec<&str> = body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant"]);
    }
}

// ===========================================================================
// HUP-S7.6 (US-7.4 AC1) — the server's reported usage rides on the returned message
// ===========================================================================

#[test]
fn reported_usage_and_generation_time_ride_on_the_returned_message() {
    let resp = json!({
        "choices": [{ "message": { "role": "assistant", "content": "hi" } }],
        "usage": { "prompt_tokens": 812, "completion_tokens": 40, "total_tokens": 852 },
        "timings": { "prompt_n": 12, "predicted_n": 40, "predicted_ms": 1333.4, "predicted_per_second": 30.0 }
    })
    .to_string();
    let msg: Value = serde_json::from_str(&parse_chat_message(&resp).unwrap()).unwrap();
    assert_eq!(msg["content"], "hi");
    assert_eq!(
        msg[USAGE_KEY],
        json!({ "prompt_tokens": 812, "completion_tokens": 40, "generation_ms": 1333 })
    );
}

#[test]
fn usage_without_timings_has_no_generation_time_and_missing_usage_adds_nothing() {
    let resp = json!({
        "choices": [{ "message": { "role": "assistant", "content": "hi" } }],
        "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
    })
    .to_string();
    let msg: Value = serde_json::from_str(&parse_chat_message(&resp).unwrap()).unwrap();
    assert_eq!(msg[USAGE_KEY], json!({ "prompt_tokens": 5, "completion_tokens": 2 }));
    for bad in [
        json!({ "choices": [{ "message": { "content": "hi" } }] }),
        json!({ "choices": [{ "message": { "content": "hi" } }], "usage": { "prompt_tokens": 5 } }),
        json!({ "choices": [{ "message": { "content": "hi" } }], "usage": { "prompt_tokens": -1, "completion_tokens": 2 } }),
        json!({ "choices": [{ "message": { "content": "hi" } }], "usage": { "prompt_tokens": "5", "completion_tokens": 2 } }),
    ] {
        let msg: Value = serde_json::from_str(&parse_chat_message(&bad.to_string()).unwrap()).unwrap();
        assert!(msg.get(USAGE_KEY).is_none(), "{bad}");
    }
    let odd = json!({
        "choices": [{ "message": { "content": "hi" } }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1 },
        "timings": { "predicted_ms": -3.0 }
    });
    let msg: Value = serde_json::from_str(&parse_chat_message(&odd.to_string()).unwrap()).unwrap();
    assert!(msg[USAGE_KEY].get("generation_ms").is_none());
}

/// HUP-S7.5 (D-27): llama-server's prompt time is the server's time to first token.
#[test]
fn the_servers_prompt_time_rides_along_as_time_to_first_token() {
    let resp = json!({
        "choices": [{ "message": { "content": "hi" } }],
        "usage": { "prompt_tokens": 12, "completion_tokens": 4 },
        "timings": { "prompt_n": 12, "prompt_ms": 95.6, "predicted_n": 4, "predicted_ms": 140.2 }
    });
    let msg: Value = serde_json::from_str(&parse_chat_message(&resp.to_string()).unwrap()).unwrap();
    assert_eq!(
        msg[USAGE_KEY],
        json!({ "prompt_tokens": 12, "completion_tokens": 4, "generation_ms": 140, "prompt_ms": 96 })
    );
    let bad = json!({
        "choices": [{ "message": { "content": "hi" } }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1 },
        "timings": { "prompt_ms": "fast", "predicted_ms": 5.0 }
    });
    let msg: Value = serde_json::from_str(&parse_chat_message(&bad.to_string()).unwrap()).unwrap();
    assert!(msg[USAGE_KEY].get("prompt_ms").is_none(), "unknown, never guessed");
}

#[test]
fn the_usage_key_is_never_sent_back_to_the_model() {
    let history = json!([
        { "role": "user", "content": "q" },
        { "role": "assistant", "content": null, "tool_calls": [], (USAGE_KEY): { "prompt_tokens": 1, "completion_tokens": 1 } },
        { "role": "assistant", "content": "a", (USAGE_KEY): { "prompt_tokens": 1, "completion_tokens": 1 } }
    ])
    .to_string();
    let body = build_chat_body_with_tools("m", &history, "[]", "{}").unwrap();
    assert!(!body.to_string().contains(USAGE_KEY));
}
