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
        msgs[1]["content"].as_str().unwrap().contains("validating"),
        "the live context is injected as a system line"
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
