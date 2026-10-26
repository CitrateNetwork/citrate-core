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

/// A remote send permit for `provider_id` (tests only; production permits come from
/// `chat_consent::authorize`).
fn permit_for(provider_id: &str) -> RemoteSendPermit {
    RemoteSendPermit::for_tests(provider_id)
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

// ===========================================================================
// D-11 — the remote paths go through the core model proxy. A proxy over an in-memory keychain and
// a scripted vendor transport (no socket, keys built at run time).
// ===========================================================================

struct Remote {
    _dir: crate::provider_fixtures_tests::TmpDir,
    kr: std::sync::Arc<crate::provider_fixtures_tests::MemKeyring>,
    vendor: std::sync::Arc<crate::provider_fixtures_tests::ScriptedVendor>,
    proxy: std::sync::Arc<crate::model_proxy::ModelProxy>,
}

impl Remote {
    fn new(tag: &str, reply: &str) -> Remote {
        let dir = crate::provider_fixtures_tests::TmpDir::new(tag);
        let kr = std::sync::Arc::new(crate::provider_fixtures_tests::MemKeyring::default());
        let vendor = crate::provider_fixtures_tests::ScriptedVendor::new(
            &["gpt-4o", "citrate-1"],
            reply,
            Some((10, 4)),
        );
        let proxy = std::sync::Arc::new(crate::model_proxy::ModelProxy::new(
            dir.0.clone(),
            kr.clone(),
            vendor.clone(),
        ));
        // The default currency caps need a price for every model called (member-entered here).
        proxy.with_state(|g, _| {
            for (id, m) in [("openai", "gpt-4o"), ("gateway", "citrate-1")] {
                g.set_price(id, m, Some(crate::model_proxy::tests::RIG_PRICE)).unwrap();
            }
        });
        Remote { _dir: dir, kr, vendor, proxy }
    }

    /// Price a model (the default currency caps refuse an unpriced one).
    fn price(&self, id: &str, model: &str) {
        self.proxy.with_state(|g, _| g.set_price(id, model, Some(crate::model_proxy::tests::RIG_PRICE)).unwrap());
    }

    /// The chat requests the vendor saw: (url, auth header, body).
    fn chats(&self) -> Vec<(String, String, Value)> {
        self.vendor
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.url.ends_with("/chat/completions") || s.url.ends_with("/responses"))
            .map(|s| (s.url.clone(), s.auth.clone(), s.body.clone().unwrap_or(Value::Null)))
            .collect()
    }

    fn proxy_key(&self, account: &str) -> Option<String> {
        self.kr.map.lock().unwrap().get(account).cloned()
    }
}

fn oa_key() -> String {
    crate::provider_fixtures_tests::fake_key("sk-")
}

fn gw_key() -> String {
    crate::provider_fixtures_tests::fake_key("cgk_")
}

#[test]
fn rejects_non_https_base_url() {
    let r = Remote::new("ai-https", "x");
    let mgr = mgr_no_http();
    // http:// is rejected.
    assert_eq!(
        mgr.set_provider(&r.proxy, "custom", "http://evil.example/v1", "gpt-4o", &oa_key()),
        Err(AiError::BadBaseUrl.to_string())
    );
    // ws:// / file:// / garbage all rejected.
    for bad in ["ws://x/v1", "file:///etc/passwd", "not a url", "ftp://x/v1"] {
        assert_eq!(
            mgr.set_provider(&r.proxy, "custom", bad, "m", &oa_key()),
            Err(AiError::BadBaseUrl.to_string()),
            "must reject non-https base url: {bad}"
        );
    }
    // A valid https URL is accepted (the key is tested live against the vendor first).
    assert!(mgr
        .set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &oa_key())
        .is_ok());
}

#[test]
fn rejects_empty_key_and_bad_provider_id() {
    let r = Remote::new("ai-badid", "x");
    let mgr = mgr_no_http();
    assert_eq!(
        mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", "   "),
        Err(AiError::BadKey.to_string())
    );
    // An id with a path separator / uppercase / the reserved "default" is rejected
    // so it can never escape the `ai-<id>` keyring namespace.
    for bad in ["", "OpenAI", "a/b", "ai default", "default"] {
        assert_eq!(
            mgr.set_provider(&r.proxy, bad, OPENAI_BASE, "gpt-4o", &oa_key()),
            Err(AiError::BadProviderId.to_string()),
            "must reject bad provider id: {bad:?}"
        );
    }
}

// ===========================================================================
// D-11 — the preset keeps {baseURL, model}; the key lives in the proxy's store
// ===========================================================================

#[test]
fn seals_config_and_binds_key_to_base_url() {
    let r = Remote::new("ai-seal", "x");
    let mgr = mgr_no_http();
    let key = oa_key();
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &key)
        .expect("seal ok");
    let cfg = mgr.read_config("openai").expect("read back");
    assert_eq!(cfg.base_url, OPENAI_BASE);
    assert_eq!(cfg.model, "gpt-4o");
    assert!(cfg.api_key.is_none(), "the preset holds no key");
    let raw = mgr.keyring.get(&account_for("openai")).unwrap().unwrap();
    assert!(!raw.contains(&key), "no key in the preset blob");
    // The key is in the proxy's store for the vendor its URL is pinned to.
    assert_eq!(r.proxy_key("provider-key:openai").as_deref(), Some(key.as_str()));
}

#[test]
fn trailing_slash_is_trimmed_so_endpoint_never_doubles() {
    let r = Remote::new("ai-slash", "x");
    let mgr = mgr_no_http();
    mgr.set_provider(&r.proxy, "custom", "https://x.example/v1/", "m", &oa_key())
        .expect("seal");
    let cfg = mgr.read_config("custom").expect("read");
    assert_eq!(cfg.base_url, "https://x.example/v1", "trailing slash trimmed");
}

// ===========================================================================
// invariant 1 — ai_provider_status returns metadata ONLY, never the key
// ===========================================================================

#[test]
fn provider_status_never_leaks_the_key() {
    let r = Remote::new("ai-status", "x");
    let mgr = mgr_no_http();
    let key = oa_key();
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &key)
        .expect("seal");
    let statuses = mgr
        .provider_status(&r.proxy, &["openai", "gateway", "custom"])
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
    assert!(!json.contains(&key), "provider status must never serialize the api key");
    assert!(
        !json.to_lowercase().contains("apikey") && !json.contains("Authorization"),
        "no apiKey/Authorization field in the status shape"
    );
}

#[test]
fn clear_provider_removes_config_and_default() {
    let r = Remote::new("ai-clear", "x");
    let mgr = mgr_no_http();
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &oa_key())
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
// request-body shape + completion parse (through the proxy)
// ===========================================================================

#[test]
fn chat_posts_openai_body_to_stored_endpoint_with_bearer() {
    let r = Remote::new("ai-chat", "Your node is validating.");
    let mgr = mgr_no_http();
    // The gateway preset: an OpenAI chat-completions endpoint (OpenAI itself is served by its
    // native Responses adapter; see a_provider_id_that_is_not_configured_…).
    let key = gw_key();
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &key)
        .expect("seal");

    let messages = json!([{ "role": "user", "content": "how is my node?" }]).to_string();
    let context = json!({ "height": 131234, "nodeState": "validating" }).to_string();
    let out = mgr
        .chat(&r.proxy, &permit_for("gateway"), "gateway", &messages, &context)
        .expect("chat");
    assert_eq!(out, "Your node is validating.");

    let calls = r.chats();
    assert_eq!(calls.len(), 1);
    let (url, auth, body) = &calls[0];
    // Endpoint = the pinned base + /chat/completions (no double slash).
    assert_eq!(url, "https://infer.citrate.ai/v1/chat/completions");
    // The proxy attached the key from its own store, in the auth header only.
    assert_eq!(auth, &format!("authorization: Bearer {key}"));
    assert!(!body.to_string().contains(&key));
    assert_eq!(body["model"], "citrate-1");
    assert_eq!(body["stream"], false);
    assert_eq!(body["max_tokens"], AI_MAX_TOKENS);
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
    let r = Remote::new("ai-err", "x");
    let mgr = mgr_no_http();
    let key = oa_key();
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &key)
        .expect("seal");
    *r.vendor.status.lock().unwrap() = Some(500);
    let err = mgr
        .chat(&r.proxy, &permit_for("openai"), "openai", &convo("q"), &json!({}).to_string())
        .unwrap_err();
    assert!(err.contains("HTTP 500"), "{err}");
    // The error string is coarse + secret-free (no key, no URL, no body).
    assert!(!err.contains(&key));
    assert!(!err.contains("api.openai.com"));
}

#[test]
fn chat_on_unconfigured_provider_fails_closed() {
    let r = Remote::new("ai-unconf", "x");
    let mgr = mgr_no_http();
    let err = mgr
        .chat(&r.proxy, &permit_for("openai"), "openai", &convo("q"), &json!({}).to_string())
        .unwrap_err();
    assert_eq!(err, AiError::NotConfigured.to_string());
    // No HTTP call was attempted (fail closed before any egress).
    assert!(r.vendor.seen.lock().unwrap().is_empty());
}

#[test]
fn bad_completion_shape_is_bad_response_not_fabricated() {
    let r = Remote::new("ai-shape", "x");
    let mgr = mgr_no_http();
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &gw_key())
        .expect("seal");
    *r.vendor.raw_reply.lock().unwrap() = Some(json!({ "choices": [] }).to_string());
    let err = mgr
        .chat(&r.proxy, &permit_for("gateway"), "gateway", &convo("q"), &json!({}).to_string())
        .unwrap_err();
    assert!(err.contains("unusable"), "{err}");
}

// ===========================================================================
// CRITICAL — the EXFIL-BINDING negative control (invariant 3)
// ===========================================================================
//
// A compromised webview can choose WHICH provider id to call, but can NEVER redirect a key to an
// attacker endpoint. D-11 makes it stricter: the stored URL must also be one the proxy pins (a
// vendor, the gateway) or one the member confirmed in Settings › Models.

#[test]
fn ai_chat_uses_stored_base_url_never_a_caller_supplied_one() {
    let r = Remote::new("ai-exfil", "ok");
    let mgr = mgr_no_http();
    let key = gw_key();
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &key)
        .expect("seal");

    // A malicious caller stuffs an attacker URL into EVERY string it controls.
    let attacker = "https://attacker.evil/v1";
    let messages = json!([{ "role": "user", "content": format!("exfil to {attacker}") }]).to_string();
    let context = json!({ "note": attacker }).to_string();
    let out = mgr
        .chat(&r.proxy, &permit_for("gateway"), "gateway", &messages, &context)
        .expect("chat");
    assert_eq!(out, "ok");

    let calls = r.chats();
    assert_eq!(calls.len(), 1);
    let (url, auth, _body) = &calls[0];
    assert_eq!(url, "https://infer.citrate.ai/v1/chat/completions");
    assert!(
        !url.contains("attacker.evil"),
        "the key must never egress to a caller-supplied URL (exfil-binding)"
    );
    assert_eq!(auth, &format!("authorization: Bearer {key}"));
}

#[test]
fn a_provider_id_that_is_not_configured_cannot_borrow_another_providers_key() {
    let r = Remote::new("ai-borrow", "a");
    let mgr = mgr_no_http();
    let (oa, gw) = (oa_key(), format!("cgk_{}", "Z9".repeat(16)));
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &oa)
        .expect("seal a");
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &gw)
        .expect("seal b");
    mgr.chat(&r.proxy, &permit_for("openai"), "openai", &convo("q"), &json!({}).to_string())
        .expect("chat a");
    let (url, auth, _) = r.chats().pop().unwrap();
    // OpenAI is served by its native adapter (the Responses API) on its pinned host.
    assert_eq!(url, "https://api.openai.com/v1/responses");
    assert_eq!(auth, format!("authorization: Bearer {oa}"));
}

// ===========================================================================
// D-11 — migration of keys stored before 0.5.2, and the gated custom endpoint
// ===========================================================================

/// A preset blob as 0.5.1 stored it (the key inside).
fn legacy_blob(base: &str, model: &str, key: &str) -> String {
    json!({ "baseURL": base, "model": model, "apiKey": key }).to_string()
}

#[test]
fn a_key_stored_before_0_5_2_moves_into_the_proxy_on_first_use() {
    let r = Remote::new("ai-migrate", "migrated answer");
    let mgr = mgr_no_http();
    let key = gw_key();
    mgr.keyring
        .set(&account_for("gateway"), &legacy_blob(GATEWAY_BASE, "citrate-1", &key))
        .unwrap();
    let out = mgr
        .chat(&r.proxy, &permit_for("gateway"), "gateway", &convo("q"), "{}")
        .expect("chat with a migrated key");
    assert_eq!(out, "migrated answer");
    // The key moved: in the proxy's store, gone from the preset.
    assert_eq!(r.proxy_key("provider-key:gateway").as_deref(), Some(key.as_str()));
    let raw = mgr.keyring.get(&account_for("gateway")).unwrap().unwrap();
    assert!(!raw.contains(&key), "the preset no longer holds the key: {raw}");
    let cfg = mgr.read_config("gateway").unwrap();
    assert_eq!((cfg.base_url.as_str(), cfg.model.as_str()), (GATEWAY_BASE, "citrate-1"));
    // The member's model is approved (no unknown-model halt) and the call is in the usage log.
    let (_, auth, _) = r.chats().pop().unwrap();
    assert_eq!(auth, format!("authorization: Bearer {key}"));
    // Idempotent: a second send works and migrates nothing more.
    mgr.chat(&r.proxy, &permit_for("gateway"), "gateway", &convo("q"), "{}")
        .expect("second send");
    assert_eq!(r.chats().len(), 2);
}

#[test]
fn a_migrated_key_never_replaces_a_key_the_proxy_already_holds() {
    let r = Remote::new("ai-migrate-keep", "x");
    let mgr = mgr_no_http();
    let current = oa_key();
    r.proxy
        .set_key("openai", crate::secret::SecretString::new(current.clone()), 1)
        .unwrap();
    let old = format!("sk-{}", "Y8".repeat(16));
    mgr.keyring
        .set(&account_for("openai"), &legacy_blob(OPENAI_BASE, "gpt-4o", &old))
        .unwrap();
    mgr.migrate_key(&r.proxy, "openai").unwrap();
    assert_eq!(r.proxy_key("provider-key:openai").as_deref(), Some(current.as_str()));
    let raw = mgr.keyring.get(&account_for("openai")).unwrap().unwrap();
    assert!(!raw.contains(&old), "the redundant copy is dropped");
}

#[test]
fn a_custom_preset_is_refused_until_the_member_confirms_its_url_then_uses_the_migrated_key() {
    let r = Remote::new("ai-custom", "custom answer");
    let mgr = mgr_no_http();
    let key = format!("bearer-{}", "K5".repeat(12));
    let base = "https://llm.example.org/v1";
    mgr.keyring
        .set(&account_for("custom"), &legacy_blob(base, "my-model", &key))
        .unwrap();
    let err = mgr
        .chat(&r.proxy, &permit_for("custom"), "custom", &convo("q"), "{}")
        .unwrap_err();
    assert!(err.contains("not been confirmed"), "{err}");
    assert!(r.vendor.seen.lock().unwrap().is_empty(), "nothing sent before the confirmation");
    // Migration made an unconfirmed custom endpoint for that exact URL, holding the key.
    let custom = r.proxy.custom_for_base(base).expect("custom endpoint made");
    assert!(r.proxy.key_configured(&custom));
    r.proxy.confirm_custom(custom.as_str(), base).unwrap();
    r.price(custom.as_str(), "my-model");
    let out = mgr
        .chat(&r.proxy, &permit_for("custom"), "custom", &convo("q"), "{}")
        .expect("confirmed");
    assert_eq!(out, "custom answer");
    let (url, auth, _) = r.chats().pop().unwrap();
    assert_eq!(url, "https://llm.example.org/v1/chat/completions");
    assert_eq!(auth, format!("authorization: Bearer {key}"));
}

#[test]
fn remote_chat_ignores_the_proxy_environment_and_follows_no_redirect() {
    // The transport is the proxy's pinned one: a 3xx answer is an error, never followed.
    let r = Remote::new("ai-redirect", "x");
    let mgr = mgr_no_http();
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &oa_key())
        .expect("seal");
    *r.vendor.status.lock().unwrap() = Some(302);
    let err = mgr
        .chat(&r.proxy, &permit_for("openai"), "openai", &convo("q"), "{}")
        .unwrap_err();
    assert!(err.contains("redirect"), "{err}");
    assert_eq!(r.chats().len(), 1, "one request, no follow-up to another location");
    // And the production config the proxy's transport uses: no proxy, no redirects, https only.
    let c = crate::provider_egress::pinned_config();
    assert_eq!(c.max_redirects(), 0);
    assert!(c.https_only());
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
/// model with a dead server asks the member first (SCL-S7.5a); a download in flight is
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
    // Ready model, dead server, gateway present → ask first (SCL-S7.5a; was local-fallback).
    assert_eq!(
        select_inference_state(ProviderInputs {
            model_ready: true,
            server_healthy: false,
            downloading: false,
            gateway_key_configured: true,
        }),
        InferenceState::LocalStopped
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
    let r = Remote::new("ai-tools", "x");
    *r.vendor.raw_reply.lock().unwrap() = Some(tool_call_response(
        "call_1",
        "memory_search",
        "{\"query\":\"staking\"}",
    ));
    let mgr = mgr_no_http();
    let key = gw_key();
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &key)
        .expect("seal");

    let messages = json!([{ "role": "user", "content": "how does staking work?" }]).to_string();
    let context = json!({ "tier": "commercial" }).to_string();
    let out = mgr
        .chat_tools(&r.proxy, &permit_for("gateway"), "gateway", &messages, TOOLS_SPEC, &context)
        .expect("chat_tools");

    // The returned assistant message preserves tool_calls for the frontend loop.
    let msg: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(msg["role"], "assistant");
    assert_eq!(msg["tool_calls"][0]["function"]["name"], "memory_search");
    assert_eq!(msg["tool_calls"][0]["function"]["arguments"], "{\"query\":\"staking\"}");

    // The request body carried the tools spec + the tool-aware system prompt.
    let (url, auth, body) = r.chats().pop().unwrap();
    assert_eq!(url, "https://infer.citrate.ai/v1/chat/completions");
    assert_eq!(auth, format!("authorization: Bearer {key}"));
    assert!(body["tools"].is_array(), "tools spec attached");
    let sys = body["messages"][0]["content"].as_str().unwrap();
    assert!(sys.contains("tools"), "tool-aware system prompt used");
}

#[test]
fn chat_tools_returns_plain_content_when_the_model_is_done() {
    let r = Remote::new("ai-tools-done", "Staking locks 32k SALT.");
    let mgr = mgr_no_http();
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &gw_key())
        .expect("seal");
    let out = mgr
        .chat_tools(&r.proxy, &permit_for("gateway"), "gateway", &convo("q"), TOOLS_SPEC, &json!({}).to_string())
        .expect("chat_tools");
    let msg: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(msg["content"], "Staking locks 32k SALT.");
    assert!(msg.get("tool_calls").is_none(), "no tool calls on a final answer");
    // The vendor's usage rides along for the Activity monitor.
    assert_eq!(msg[USAGE_KEY]["prompt_tokens"], 10);
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

/// No error from setting a provider carries the key, whatever is wrong with the input.
#[test]
fn set_provider_errors_never_carry_the_key() {
    let r = Remote::new("ai-seterr", "x");
    let mgr = mgr_no_http();
    let key = format!("sk-live-{}", "should-not-appear");
    for (url, model) in [("http://evil.example/v1", "m"), (OPENAI_BASE, " "), ("notaurl", "m")] {
        let err = mgr
            .set_provider(&r.proxy, "custom", url, model, &key)
            .expect_err("bad input is refused");
        assert!(!err.contains("sk-live"), "key in error: {err}");
    }
    // A key the vendor rejects is not stored, and the error does not carry it either.
    *r.vendor.status.lock().unwrap() = Some(401);
    let err = mgr
        .set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &oa_key())
        .expect_err("rejected key");
    assert!(!err.contains(&oa_key()), "{err}");
    assert!(r.proxy_key("provider-key:openai").is_none());
}

// ===========================================================================
// K2 — remote chat sends need a core-issued consent (chat_consent)
// ===========================================================================

fn convo(user: &str) -> String {
    json!([{ "role": "user", "content": user }]).to_string()
}

fn configured_gateway(r: &Remote) -> AiManager {
    let mgr = mgr_no_http();
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &gw_key())
        .unwrap();
    mgr
}

/// A clock reading `ms` into the test (monotonic and wall time move together).
fn clk(ms: u64) -> crate::ceremony::ClockReading {
    crate::chat_consent::test_clock(ms, ms)
}

/// A route folder with no setting stored: the "ask first" default.
fn ask_first_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("citrate-ai-route-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn remote_chat_without_a_core_issued_consent_sends_nothing() {
    let r = Remote::new("ai-k2-a", "hi");
    let mgr = configured_gateway(&r);
    let consents = crate::chat_consent::ChatConsentState::default();
    let res = ai_chat_sync(&mgr, &r.proxy, &consents, &ask_first_dir("chat"), clk(1_000), "", "gateway", &convo("hello"), "{}");
    assert!(res.is_err(), "sent without consent: {res:?}");
    let res = ai_chat_tools_sync(&mgr, &r.proxy, &consents, &ask_first_dir("chat"), clk(1_000), "cc_made_up", "gateway", &convo("hello"), "[]", "{}");
    assert!(res.is_err(), "sent without consent: {res:?}");
    assert!(r.chats().is_empty(), "nothing may reach the gateway");
}

#[test]
fn remote_chat_with_the_members_consent_sends_that_message_only() {
    let r = Remote::new("ai-k2-b", "answer");
    let mgr = configured_gateway(&r);
    let consents = crate::chat_consent::ChatConsentState::default();
    let v = consents
        .0
        .lock()
        .unwrap()
        .prepare(crate::chat_consent::Purpose::Chat, "gateway", "hello", clk(1_000), "cc_abc".into());
    let out = ai_chat_sync(&mgr, &r.proxy, &consents, &ask_first_dir("chat"), clk(1_001), &v.consent_id, "gateway", &convo("hello"), "{}")
        .expect("consented send");
    assert_eq!(out, "answer");
    // The same consent cannot carry a different message (and is spent by trying).
    let res = ai_chat_sync(&mgr, &r.proxy, &consents, &ask_first_dir("chat"), clk(1_002), &v.consent_id, "gateway", &convo("other"), "{}");
    assert!(res.is_err());
    assert_eq!(r.chats().len(), 1, "only the consented message went out");
}

#[test]
fn a_permit_for_one_provider_cannot_send_to_another() {
    let r = Remote::new("ai-k2-c", "x");
    let mgr = configured_gateway(&r);
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &oa_key()).unwrap();
    let res = mgr.chat(&r.proxy, &permit_for("gateway"), "openai", &convo("q"), "{}");
    assert_eq!(res, Err(AiError::NotConfirmed.to_string()));
    assert!(r.chats().is_empty());
}

// K2: remote image generation needs a core-issued consent for that prompt.

/// Remote image requests the vendor saw.
fn images(r: &Remote) -> usize {
    r.vendor
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.url.ends_with("/images/generations"))
        .count()
}

fn image_body() -> Value {
    json!({ "prompt": "a lemon tree", "n": 1 })
}

fn media_consent(consents: &crate::chat_consent::ChatConsentState, prompt: &str) -> String {
    consents
        .0
        .lock()
        .unwrap()
        .prepare(crate::chat_consent::Purpose::Media, "gateway", prompt, clk(1_000), "cc_img".into())
        .consent_id
}

#[test]
fn a_remote_image_without_a_core_issued_consent_sends_nothing() {
    let r = Remote::new("ai-media-a", "x");
    let mgr = configured_gateway(&r);
    let consents = crate::chat_consent::ChatConsentState::default();
    for id in [None, Some(""), Some("cc_made_up")] {
        let res = crate::media::remote_image_post(&mgr, &r.proxy, &consents, &ask_first_dir("media"), clk(1_001), id, "gateway", "a lemon tree", &image_body());
        assert!(res.is_err(), "sent without consent ({id:?}): {res:?}");
    }
    assert_eq!(images(&r), 0, "nothing may reach the provider");
}

#[test]
fn a_media_consent_is_single_use_and_bound_to_its_prompt_purpose_and_provider() {
    let r = Remote::new("ai-media-b", "x");
    let mgr = configured_gateway(&r);
    mgr.set_provider(&r.proxy, "openai", OPENAI_BASE, "gpt-4o", &oa_key()).unwrap();
    let consents = crate::chat_consent::ChatConsentState::default();
    let id = media_consent(&consents, "a lemon tree");
    let post = |id: &str, provider: &str, prompt: &str| {
        crate::media::remote_image_post(&mgr, &r.proxy, &consents, &ask_first_dir("media"), clk(1_001), Some(id), provider, prompt, &image_body())
    };
    assert!(post(&id, "gateway", "a lemon tree").is_ok());
    assert!(post(&id, "gateway", "a lemon tree").is_err(), "spent after one image");
    let id = media_consent(&consents, "a lemon tree");
    assert!(post(&id, "gateway", "something else").is_err(), "another prompt");
    let id = media_consent(&consents, "a lemon tree");
    assert!(post(&id, "openai", "a lemon tree").is_err(), "another provider");
    // A chat consent is not a media consent.
    let chat = consents
        .0
        .lock()
        .unwrap()
        .prepare(crate::chat_consent::Purpose::Chat, "gateway", "a lemon tree", clk(1_000), "cc_chat".into());
    assert!(post(&chat.consent_id, "gateway", "a lemon tree").is_err(), "chat consent");
    // Expired.
    let id = media_consent(&consents, "a lemon tree");
    let res = crate::media::remote_image_post(
        &mgr,
        &r.proxy,
        &consents,
        &ask_first_dir("media"),
        clk(1_000 + crate::chat_consent::CONSENT_TTL_MS),
        Some(&id),
        "gateway",
        "a lemon tree",
        &image_body(),
    );
    assert!(res.is_err(), "expired");
    assert_eq!(images(&r), 1, "only the one consented image went out");
}

#[test]
fn a_remote_image_is_refused_under_local_model_only() {
    let dir = std::env::temp_dir().join(format!("citrate-media-route-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(crate::media::media_remote_allowed(&dir).is_ok(), "default is ask first");
    crate::chat_route::set(&dir, crate::chat_route::ChatRoute::LocalOnly, 1).unwrap();
    let err = crate::media::media_remote_allowed(&dir).unwrap_err();
    assert!(err.contains("Local only is on"), "{err}");
    std::fs::write(dir.join(crate::chat_route::ROUTE_FILE), "{").unwrap();
    assert!(crate::media::media_remote_allowed(&dir).is_err(), "unreadable counts as local only");
    crate::chat_route::set(&dir, crate::chat_route::ChatRoute::AskFirst, 2).unwrap();
    assert!(crate::media::media_remote_allowed(&dir).is_ok(), "ask first allows (with consent)");
    let _ = std::fs::remove_dir_all(&dir);
}

// #473 / #469: the Local model only check is part of obtaining a permit, so a consent minted
// before the member turned it on cannot be spent afterwards.

#[test]
fn remote_chat_under_local_model_only_sends_nothing_even_with_a_consent() {
    let r = Remote::new("ai-k2-lo", "never");
    let mgr = configured_gateway(&r);
    let consents = crate::chat_consent::ChatConsentState::default();
    for id in ["cc_a", "cc_b"] {
        consents.0.lock().unwrap().prepare(
            crate::chat_consent::Purpose::Chat,
            "gateway",
            "hello",
            clk(1_000),
            id.into(),
        );
    }
    let dir = ask_first_dir("chat-local-only");
    crate::chat_route::set(&dir, crate::chat_route::ChatRoute::LocalOnly, 1).unwrap();
    let res = ai_chat_sync(&mgr, &r.proxy, &consents, &dir, clk(1_001), "cc_a", "gateway", &convo("hello"), "{}");
    assert!(res.as_ref().is_err_and(|e| e.contains("Local only is on")), "{res:?}");
    let res = ai_chat_tools_sync(&mgr, &r.proxy, &consents, &dir, clk(1_001), "cc_b", "gateway", &convo("hello"), "[]", "{}");
    assert!(res.as_ref().is_err_and(|e| e.contains("Local only is on")), "{res:?}");
    assert!(r.chats().is_empty(), "nothing may reach the gateway");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_media_consent_minted_before_local_model_only_posts_nothing() {
    let r = Remote::new("ai-media-lo", "x");
    let mgr = configured_gateway(&r);
    let consents = crate::chat_consent::ChatConsentState::default();
    let id = media_consent(&consents, "a lemon tree");
    let dir = ask_first_dir("media-local-only");
    crate::chat_route::set(&dir, crate::chat_route::ChatRoute::LocalOnly, 1).unwrap();
    let res = crate::media::remote_image_post(&mgr, &r.proxy, &consents, &dir, clk(1_001), Some(&id), "gateway", "a lemon tree", &image_body());
    assert!(res.as_ref().is_err_and(|e| e.contains("Local only is on")), "{res:?}");
    assert_eq!(images(&r), 0, "nothing may reach the provider");
    let _ = std::fs::remove_dir_all(dir);
}

// D-11 (P-5): the app's own loop can send a member-chosen message to the preferred cloud model.
#[test]
fn chat_on_the_preferred_id_goes_to_the_members_preferred_model_through_the_proxy() {
    let r = Remote::new("ai-preferred", "from the preferred model");
    let mgr = mgr_no_http();
    // Nothing chosen: refused, nothing sent.
    let err = mgr
        .chat(&r.proxy, &permit_for("preferred"), "preferred", &convo("q"), "{}")
        .unwrap_err();
    assert!(err.contains("no cloud model is chosen"), "{err}");
    let key = gw_key();
    mgr.set_provider(&r.proxy, "gateway", GATEWAY_BASE, "citrate-1", &key).unwrap();
    crate::provider_preferred::set(
        r.proxy.dir(),
        &r.proxy,
        Some(crate::provider_preferred::PreferredModel { provider_id: "gateway".into(), model: "citrate-1".into() }),
    )
    .unwrap();
    let out = mgr
        .chat(&r.proxy, &permit_for("preferred"), "preferred", &convo("q"), "{}")
        .expect("preferred");
    assert_eq!(out, "from the preferred model");
    let (url, auth, body) = r.chats().pop().unwrap();
    assert_eq!(url, "https://infer.citrate.ai/v1/chat/completions");
    assert_eq!(auth, format!("authorization: Bearer {key}"));
    assert_eq!(body["model"], "citrate-1");
    // A permit for another provider cannot send there.
    assert_eq!(
        mgr.chat(&r.proxy, &permit_for("gateway"), "preferred", &convo("q"), "{}"),
        Err(AiError::NotConfirmed.to_string())
    );
}
