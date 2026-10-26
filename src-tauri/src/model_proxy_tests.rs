// D-11 — the core model proxy end to end against a scripted vendor: egress, the guard, the
// write-ahead usage log, routes, and one-way keys.

use super::*;
use crate::model_provider::{ChatMessage, Role};
use crate::provider_fixtures_tests::{fake_key, MemKeyring, ScriptedVendor, TmpDir};
use crate::provider_guard::{Caps, HaltReason};
use crate::provider_usage_log::log_path;

pub(crate) const PROMPT: &str = "a distinctive prompt that must never reach the log";

pub(crate) struct Rig {
    pub dir: TmpDir,
    pub kr: Arc<MemKeyring>,
    pub vendor: Arc<ScriptedVendor>,
    pub proxy: Arc<ModelProxy>,
    pub key: String,
}

impl Rig {
    pub(crate) fn new(tag: &str) -> Rig {
        Rig::with_vendor(tag, ScriptedVendor::new(&["gpt-known"], "hello from the vendor", Some((12, 5))))
    }

    pub(crate) fn with_vendor(tag: &str, vendor: Arc<ScriptedVendor>) -> Rig {
        let dir = TmpDir::new(tag);
        let kr = Arc::new(MemKeyring::default());
        let proxy = Arc::new(ModelProxy::new(dir.0.clone(), kr.clone(), vendor.clone()));
        proxy.set_port(1);
        let key = fake_key("sk-");
        proxy
            .set_key("openai", SecretString::new(key.clone()), now_ms())
            .unwrap();
        Rig {
            dir,
            kr,
            vendor,
            proxy,
            key,
        }
    }

    pub(crate) fn reopen(&mut self) {
        let p = Arc::new(ModelProxy::new(self.dir.0.clone(), self.kr.clone(), self.vendor.clone()));
        p.set_port(1);
        self.proxy = p;
    }

    pub(crate) fn route(&self, model: &str) -> ProxyRoute {
        ProxyRoute {
            provider: ProviderId::Vendor(Vendor::OpenAi),
            model: model.into(),
            session_id: "sess-1".into(),
        }
    }

    /// A chat-route settings folder with nothing stored ("ask first": remote sends allowed).
    pub(crate) fn gate(&self) -> std::path::PathBuf {
        self.dir.0.join("route")
    }

    pub(crate) fn log_text(&self) -> String {
        std::fs::read_to_string(log_path(&self.dir.0)).unwrap_or_default()
    }
}

pub(crate) fn req(model: &str, stream: bool) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: PROMPT.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }],
        tools: vec![],
        max_tokens: 100,
        stream,
    }
}

#[test]
fn a_call_reaches_the_pinned_host_with_the_key_only_in_its_auth_header() {
    let rig = Rig::new("proxy-happy");
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    assert_eq!(r.content, "hello from the vendor");
    let seen = rig.vendor.seen.lock().unwrap();
    let chat = seen.iter().find(|s| s.url.ends_with("/chat/completions")).unwrap();
    assert_eq!(chat.url, "https://api.openai.com/v1/chat/completions");
    assert_eq!(chat.method, crate::model_provider::Method::Post);
    assert!(chat.headers.iter().any(|(k, v)| k == "accept" && v == "application/json"));
    assert_eq!(chat.auth, format!("authorization: Bearer {}", rig.key));
    assert!(!chat.body.as_ref().unwrap().to_string().contains(&rig.key));
}

#[test]
fn every_call_leaves_a_begin_and_end_record_with_no_content() {
    let rig = Rig::new("proxy-log");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), Some("conv-7".into()), &mut |_| {})
        .unwrap();
    let text = rig.log_text();
    let lines: Vec<&str> = text.lines().collect();
    let kinds: Vec<String> = lines
        .iter()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["r"]["kind"].to_string())
        .collect();
    assert!(kinds.contains(&"\"begin\"".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"\"end\"".to_string()), "{kinds:?}");
    let end: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(end["r"]["tokensIn"], 12);
    assert_eq!(end["r"]["tokensOut"], 5);
    assert_eq!(end["r"]["provider"], "openai");
    assert_eq!(end["r"]["model"], "gpt-known");
    assert_eq!(end["r"]["session"], "sess-1");
    assert_eq!(end["r"]["conversation"], "conv-7");
    assert_eq!(end["r"]["outcome"], "ok");
    assert!(!text.contains(PROMPT), "no prompt in the log");
    assert!(!text.contains("hello from the vendor"), "no reply in the log");
    assert!(!text.contains(&rig.key), "no key in the log");
}

#[test]
fn a_call_that_cannot_be_logged_is_refused_and_nothing_is_sent() {
    let mut rig = Rig::new("proxy-nolog");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    // Break the chain, reopen: the next call is refused before the vendor sees it.
    let p = log_path(&rig.dir.0);
    let t = std::fs::read_to_string(&p)
        .unwrap_or_default()
        .replace("\"tokensIn\":12", "\"tokensIn\":1");
    std::fs::write(&p, t).unwrap();
    rig.reopen();
    let before = rig.vendor.calls();
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {});
    assert!(matches!(r, Err(ProxyError::LogUnavailable(_))), "{r:?}");
    assert_eq!(rig.vendor.calls(), before, "nothing may be sent without a record");
}

#[test]
fn a_reply_whose_end_record_cannot_be_written_is_withheld() {
    let rig = Rig::new("proxy-noend");
    // Mid-call, replace the log file with a directory so the end record cannot be appended.
    let p = log_path(&rig.dir.0);
    *rig.vendor.during_call.lock().unwrap() = Some(Box::new(move || {
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::create_dir_all(&p);
    }));
    let mut deltas = String::new();
    let r = rig.proxy.call(
        &rig.route("gpt-known"),
        req("gpt-known", false),
        None,
        &mut |t| deltas.push_str(t),
    );
    assert!(matches!(r, Err(ProxyError::LogUnavailable(_))), "{r:?}");
}

#[test]
fn caps_stop_the_call_before_the_vendor_sees_it() {
    let rig = Rig::new("proxy-caps");
    rig.proxy.with_state(|g, _| {
        g.set_caps(
            "openai",
            Caps {
                daily_tokens: Some(150),
                monthly_tokens: Some(1_000),
                daily_micros: None,
                monthly_micros: None,
            },
        )
        .unwrap()
    });
    let before = rig.vendor.calls();
    // The request's worst case (its input bound + 100 output tokens) is over 150.
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {});
    assert!(
        matches!(r, Err(ProxyError::Refused(Refusal::CapExceeded { .. }))),
        "{r:?}"
    );
    assert_eq!(rig.vendor.calls(), before);
    assert!(rig.log_text().contains("\"refused\""), "the refusal is recorded");
}

#[test]
fn an_unknown_model_halts_the_provider_with_nothing_sent() {
    let rig = Rig::new("proxy-unknown");
    let before = rig.vendor.calls();
    let r = rig
        .proxy
        .call(&rig.route("gpt-other"), req("gpt-other", false), None, &mut |_| {});
    assert!(
        matches!(&r, Err(ProxyError::Refused(Refusal::Halted(h))) if matches!(h.reason, HaltReason::UnknownModel { .. })),
        "{r:?}"
    );
    assert_eq!(rig.vendor.calls(), before);
    // The halt is persisted.
    let mut rig = rig;
    rig.reopen();
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {});
    assert!(matches!(r, Err(ProxyError::Refused(Refusal::Halted(_)))), "{r:?}");
}

#[test]
fn a_request_cannot_change_its_routes_model() {
    let rig = Rig::new("proxy-mismatch");
    let before = rig.vendor.calls();
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-expensive", false), None, &mut |_| {});
    assert!(matches!(r, Err(ProxyError::BadRequest(_))), "{r:?}");
    assert_eq!(rig.vendor.calls(), before);
}

#[test]
fn a_token_spike_after_a_call_halts_the_provider() {
    let spiky = ScriptedVendor::new(&["gpt-known"], "ok", Some((500_000, 10)));
    let rig = Rig::with_vendor("proxy-spike", spiky);
    rig.proxy.with_state(|g, _| {
        g.set_caps("openai", Caps { daily_tokens: None, monthly_tokens: None, daily_micros: None, monthly_micros: None })
            .unwrap();
        for _ in 0..10 {
            g.observe("openai", 20, now_ms());
        }
    });
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {});
    assert!(r.is_ok(), "the spiking call itself returns: {r:?}");
    let halted = rig.proxy.with_state(|g, _| g.get("openai").halt);
    assert!(
        matches!(halted, Some(crate::provider_guard::Halt { reason: HaltReason::TokenSpike { .. }, .. })),
        "{halted:?}"
    );
}

#[test]
fn an_unconfirmed_custom_endpoint_is_refused() {
    let rig = Rig::new("proxy-custom");
    let c = rig
        .proxy
        .add_custom("My box", "https://llm.example.org/v1", vec!["box-model".into()], 1)
        .unwrap();
    assert!(!c.confirmed, "a custom endpoint starts unconfirmed");
    let route = ProxyRoute {
        provider: ProviderId::Custom(c.id.clone()),
        model: "box-model".into(),
        session_id: "s".into(),
    };
    let r = rig.proxy.call(&route, req("box-model", false), None, &mut |_| {});
    assert!(matches!(r, Err(ProxyError::Egress(_))), "{r:?}");
    // Confirming needs the exact URL.
    assert!(rig.proxy.confirm_custom(&c.id, "https://evil.example/v1").is_err());
    let still = rig.proxy.with_state(|g, _| g.customs[0].confirmed);
    assert!(!still);
    rig.proxy
        .confirm_custom(&c.id, "https://llm.example.org/v1")
        .unwrap();
    rig.proxy
        .set_key(&c.id, SecretString::new(fake_key("ck-")), 2)
        .unwrap();
    let r = rig.proxy.call(&route, req("box-model", false), None, &mut |_| {});
    assert!(r.is_ok(), "{r:?}");
    let seen = rig.vendor.seen.lock().unwrap();
    assert_eq!(seen.last().unwrap().url, "https://llm.example.org/v1/chat/completions");
    // The record marks it unverified.
    assert!(rig.log_text().contains("\"verified\":false"));
}

#[test]
fn keys_are_one_way_and_a_failing_key_is_never_stored() {
    let rig = Rig::new("proxy-keys");
    // Wrong format: refused before any network call.
    let before = rig.vendor.seen.lock().unwrap().len();
    let r = rig.proxy.set_key("xai", SecretString::new(fake_key("sk-")), 1);
    assert!(matches!(r, Err(ProxyError::Provider(ProviderError::KeyFormat(_)))), "{r:?}");
    assert_eq!(rig.vendor.seen.lock().unwrap().len(), before);
    // The vendor rejects it: not stored.
    *rig.vendor.status.lock().unwrap() = Some(401);
    let r = rig.proxy.set_key("xai", SecretString::new(fake_key("xai-")), 1);
    assert_eq!(r.unwrap_err(), ProxyError::Provider(ProviderError::Unauthorized));
    assert!(!rig.proxy.key_configured(&ProviderId::Vendor(Vendor::Xai)));
    *rig.vendor.status.lock().unwrap() = None;
    // Good key: stored, listed models kept, the key test is recorded.
    let t = rig.proxy.set_key("xai", SecretString::new(fake_key("xai-")), 1).unwrap();
    assert_eq!(t.models, vec!["gpt-known".to_string()]);
    assert!(rig.proxy.key_configured(&ProviderId::Vendor(Vendor::Xai)));
    assert!(rig.log_text().contains("\"key_test\""));
    // Removing it deletes the keychain entry and revokes routes.
    let g = rig
        .proxy
        .mint_route(ProxyRoute {
            provider: ProviderId::Vendor(Vendor::Xai),
            model: "gpt-known".into(),
            session_id: "s2".into(),
        }, rig.gate())
        .unwrap();
    rig.proxy.remove_key("xai").unwrap();
    assert!(!rig.proxy.key_configured(&ProviderId::Vendor(Vendor::Xai)));
    assert!(rig.proxy.route_for(g.bearer.expose()).is_none());
    // Not-yet-available vendors say so.
    assert_eq!(
        rig.proxy.set_key("anthropic", SecretString::new(fake_key("sk-ant-")), 1).err(),
        Some(ProxyError::NotAvailable)
    );
}

#[test]
fn routes_bind_one_provider_model_and_session_and_are_not_keys() {
    let rig = Rig::new("proxy-routes");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    assert_eq!(g.base_url, "http://127.0.0.1:1/v1");
    assert_ne!(g.bearer.expose(), rig.key);
    assert!(!g.bearer.expose().contains(&rig.key));
    assert_eq!(rig.proxy.route_for(g.bearer.expose()), Some(rig.route("gpt-known")));
    assert!(rig.proxy.route_for("cmp_not-a-route").is_none());
    rig.proxy.revoke_session("sess-1");
    assert!(rig.proxy.route_for(g.bearer.expose()).is_none());
    // Routes never reach the keychain or the guard file.
    for v in rig.kr.values() {
        assert!(!v.contains("cmp_"));
    }
}

// ---- D-11 (WP-S1-PROV-AGENT) -------------------------------------------------------------------

#[test]
fn a_base_url_maps_only_to_a_pinned_vendor_the_gateway_or_a_confirmed_custom_endpoint() {
    let rig = Rig::new("proxy-base");
    assert_eq!(rig.proxy.provider_for_base("https://api.openai.com/v1/"), Ok(ProviderId::Vendor(Vendor::OpenAi)));
    assert_eq!(rig.proxy.provider_for_base("https://api.x.ai/v1"), Ok(ProviderId::Vendor(Vendor::Xai)));
    assert_eq!(rig.proxy.provider_for_base("https://infer.citrate.ai/v1"), Ok(ProviderId::Gateway));
    for bad in ["https://api.openai.com.evil.example/v1", "https://attacker.example/v1", "http://api.openai.com/v1"] {
        assert!(matches!(rig.proxy.provider_for_base(bad), Err(ProxyError::Egress(_))), "{bad}");
    }
    let c = rig.proxy.add_custom("Mine", "https://llm.example.org/v1", vec!["m".into()], 1).unwrap();
    let e = rig.proxy.provider_for_base("https://llm.example.org/v1").unwrap_err();
    assert!(e.to_string().contains("not been confirmed"), "{e}");
    rig.proxy.confirm_custom(&c.id, "https://llm.example.org/v1").unwrap();
    assert_eq!(rig.proxy.provider_for_base("https://llm.example.org/v1"), Ok(ProviderId::Custom(c.id)));
}

#[test]
fn the_gateway_is_served_on_its_pinned_host_with_a_cgk_key() {
    let rig = Rig::new("proxy-gateway");
    let key = fake_key("cgk_");
    rig.proxy.set_key("gateway", SecretString::new(key.clone()), 1).unwrap();
    assert!(rig.proxy.set_key("gateway", SecretString::new(fake_key("sk-")), 1).is_err(), "wrong prefix");
    let route = ProxyRoute { provider: ProviderId::Gateway, model: "gpt-known".into(), session_id: "g".into() };
    rig.proxy.call(&route, req("gpt-known", false), None, &mut |_| {}).unwrap();
    let seen = rig.vendor.seen.lock().unwrap();
    let chat = seen.iter().rfind(|s| s.url.ends_with("/chat/completions")).unwrap();
    assert_eq!(chat.url, "https://infer.citrate.ai/v1/chat/completions");
    assert_eq!(chat.auth, format!("authorization: Bearer {key}"));
}

#[test]
fn an_imported_key_keeps_an_existing_one_unless_replaced_and_is_format_checked() {
    let rig = Rig::new("proxy-import");
    let other = format!("sk-{}", "W3".repeat(16));
    let pid = ProviderId::Vendor(Vendor::OpenAi);
    assert_eq!(rig.proxy.import_key(&pid, SecretString::new(other.clone()), false), Ok(false));
    assert_eq!(rig.kr.map.lock().unwrap().get("provider-key:openai"), Some(&rig.key));
    assert_eq!(rig.proxy.import_key(&pid, SecretString::new(other.clone()), true), Ok(true));
    assert_eq!(rig.kr.map.lock().unwrap().get("provider-key:openai"), Some(&other));
    assert!(rig.proxy.import_key(&pid, SecretString::new("short".into()), true).is_err());
    assert!(rig.proxy.import_key(&ProviderId::Cognition, SecretString::new(fake_key("cog_")), true).is_err());
    // Importing reads nothing from the network.
    assert_eq!(rig.vendor.seen.lock().unwrap().iter().filter(|s| s.url.ends_with("/models")).count(), 1);
}

#[test]
fn a_fixed_path_post_is_a_logged_proxy_call_on_the_pinned_host() {
    let rig = Rig::new("proxy-fixed");
    let pid = ProviderId::Vendor(Vendor::OpenAi);
    let body = serde_json::json!({"prompt": "a lemon tree", "model": "img-1"});
    rig.proxy.post_fixed(&pid, "img-1", "media", "/images/generations", &body).unwrap();
    let seen = rig.vendor.seen.lock().unwrap().clone();
    let post = seen.iter().find(|s| s.url.ends_with("/images/generations")).unwrap();
    assert_eq!(post.url, "https://api.openai.com/v1/images/generations");
    assert_eq!(post.auth, format!("authorization: Bearer {}", rig.key));
    let text = rig.log_text();
    assert!(text.contains("\"media\"") && text.contains("\"img-1\""), "{text}");
    assert!(!text.contains("lemon"), "no content in the log");
    // A halted provider sends nothing.
    rig.proxy.with_state(|g, _| g.member_halt("openai", 5));
    let n = rig.vendor.seen.lock().unwrap().len();
    assert!(matches!(rig.proxy.post_fixed(&pid, "img-1", "media", "/images/generations", &body), Err(ProxyError::Refused(_))));
    assert_eq!(rig.vendor.seen.lock().unwrap().len(), n);
}

#[test]
fn a_route_is_gated_on_local_model_only_per_call() {
    let rig = Rig::new("proxy-gate");
    let g = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    rig.proxy.call_routed(g.bearer.expose(), req("gpt-known", false), None, &mut |_| {}).unwrap();
    crate::chat_route::set(&rig.gate(), crate::chat_route::ChatRoute::LocalOnly, 1).unwrap();
    let before = rig.vendor.calls();
    let r = rig.proxy.call_routed(g.bearer.expose(), req("gpt-known", false), None, &mut |_| {});
    assert!(matches!(r, Err(ProxyError::Egress(_))), "{r:?}");
    assert_eq!(rig.vendor.calls(), before);
    // An unreadable setting counts as local only too.
    std::fs::write(rig.gate().join(crate::chat_route::ROUTE_FILE), "{").unwrap();
    assert!(rig.proxy.call_routed(g.bearer.expose(), req("gpt-known", false), None, &mut |_| {}).is_err());
    assert!(matches!(rig.proxy.call_routed("cmp_unknown", req("gpt-known", false), None, &mut |_| {}), Err(ProxyError::Unauthorized)));
}

#[test]
fn routes_with_a_prefix_are_revoked_together() {
    let rig = Rig::new("proxy-prefix");
    let mk = |s: &str| {
        rig.proxy
            .mint_route(ProxyRoute { session_id: s.into(), ..rig.route("gpt-known") }, rig.gate())
            .unwrap()
    };
    let (a, b, c) = (mk("hs-1"), mk("hs-2"), mk("esc-1"));
    rig.proxy.revoke_sessions_with_prefix("hs-");
    assert!(rig.proxy.route_for(a.bearer.expose()).is_none());
    assert!(rig.proxy.route_for(b.bearer.expose()).is_none());
    assert!(rig.proxy.route_for(c.bearer.expose()).is_some());
}
