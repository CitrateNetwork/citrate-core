// D-11 — the core model proxy end to end against a scripted vendor: egress, the guard, the
// write-ahead usage log, routes, and one-way keys.

use super::*;
use crate::model_provider::{ChatMessage, Role};
use crate::provider_fixtures_tests::{fake_key, MemKeyring, ScriptedVendor, TmpDir};
use crate::provider_guard::{Caps, HaltReason, Price};
use crate::provider_usage_log::log_path;

pub(crate) const RIG_PRICE: Price = Price {
    input_micros_per_mtok: 2_000_000,
    output_micros_per_mtok: 8_000_000,
};

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
        // The default currency caps need a price ($2 / $8 per million tokens, member-entered).
        proxy.with_state(|g, _| {
            g.set_price("openai", "gpt-known", Some(RIG_PRICE)).unwrap();
            proxy.save_guard(&mut *g).unwrap();
        });
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
    let chat = seen.iter().find(|s| s.method == crate::model_provider::Method::Post);
    assert_eq!(
        chat.map(|c| c.url.as_str()),
        Some("https://api.openai.com/v1/responses"),
        "OpenAI's native Responses endpoint"
    );
    let chat = chat.unwrap();
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
    rig.proxy.with_state(|g, _| g.set_price(&c.id, "box-model", Some(RIG_PRICE)).unwrap());
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
        })
        .unwrap();
    rig.proxy.remove_key("xai").unwrap();
    assert!(!rig.proxy.key_configured(&ProviderId::Vendor(Vendor::Xai)));
    assert!(rig.proxy.route_for(g.bearer.expose()).is_none());
    // A key in another vendor's format is refused by that vendor's native check, nothing sent.
    let before = rig.vendor.seen.lock().unwrap().len();
    assert!(matches!(
        rig.proxy.set_key("anthropic", SecretString::new(fake_key("xai-")), 1).err(),
        Some(ProxyError::Provider(ProviderError::KeyFormat(_)))
    ));
    assert_eq!(rig.vendor.seen.lock().unwrap().len(), before);
}

#[test]
fn routes_bind_one_provider_model_and_session_and_are_not_keys() {
    let rig = Rig::new("proxy-routes");
    let g = rig.proxy.mint_route(rig.route("gpt-known")).unwrap();
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

#[test]
fn every_vendor_has_its_native_adapter_with_a_priced_tool_capable_catalog() {
    for v in Vendor::ALL {
        let (a, kind) = vendor_adapter(v);
        assert_eq!(kind, AdapterKind::Native, "{v:?}");
        assert_eq!(a.id(), ProviderId::Vendor(v), "the registry must never cross vendors");
        let models = a.models();
        assert!(!models.is_empty(), "{v:?} lists its models");
        let probe = req(&models[0].id, false);
        assert!(a.output_bound(&probe) >= u64::from(probe.max_tokens), "{v:?}");
        for m in models {
            assert!(m.supports_tools, "{}", m.id);
            assert!(
                m.input_micros_per_mtok.is_some() && m.output_micros_per_mtok.is_some(),
                "{} is priced (a currency cap refuses unpriced models)",
                m.id
            );
        }
    }
    assert_eq!(AdapterKind::Native.as_str(), "native");
}

/// Tripwire for the off-host class: whatever an adapter builds (key test, plain and streamed
/// call), every URL it sends is under its own vendor's pinned base, never another vendor's.
#[test]
fn every_native_adapter_stays_under_its_own_pinned_base() {
    use crate::provider_fixtures_tests::{tool_conversation, with_vendor_ctx, Recorded};
    for v in Vendor::ALL {
        let (a, _) = vendor_adapter(v);
        let first = a.models().first().map(|m| m.id.clone());
        assert!(first.is_some(), "{v:?} lists a model to call");
        let model = first.unwrap_or_default();
        let http = Recorded::new(vec![
            (500, "application/json", "{}".into()),
            (500, "application/json", "{}".into()),
            (500, "application/json", "{}".into()),
        ]);
        let prefix = match v {
            Vendor::Anthropic => "sk-ant-api03-",
            Vendor::Xai => "xai-",
            Vendor::Meta => "LLM_1_",
            _ => "sk-",
        };
        with_vendor_ctx(v, prefix, &http, |ctx| {
            assert_eq!(a.test_key(ctx), Err(ProviderError::Status(500)));
            for stream in [false, true] {
                let r = a.complete(ctx, &tool_conversation(&model, stream), &mut |_| {});
                assert_eq!(r, Err(ProviderError::Status(500)), "{v:?}");
            }
        });
        let base = crate::provider_egress::vendor_spec(v).base_url;
        let seen = http.seen.lock().unwrap();
        assert_eq!(seen.len(), 3, "{v:?}");
        for s in seen.iter() {
            // (`Recorded` also re-checks the pinned host and port at the transport.)
            assert!(s.url.starts_with(&format!("{base}/")), "{v:?} sent to {}", s.url);
        }
    }
}

#[test]
fn a_request_an_adapter_cannot_express_is_a_400_not_a_provider_failure() {
    let e = ProxyError::Provider(ProviderError::BadRequest("x"));
    assert_eq!(e.http(), (400, "bad_request"));
    assert_eq!(e.outcome(), "bad_request");
    assert_eq!(ProxyError::Provider(ProviderError::Status(500)).http().0, 502);
}

#[test]
fn under_the_default_currency_caps_an_unpriced_model_is_refused_with_nothing_sent() {
    let rig = Rig::new("proxy-unpriced");
    rig.proxy.with_state(|g, _| g.set_price("openai", "gpt-known", None).unwrap());
    let before = rig.vendor.calls();
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {});
    assert_eq!(r.err(), Some(ProxyError::Refused(Refusal::PriceUnknown)));
    assert_eq!(rig.vendor.calls(), before, "an unknown price never bypasses the cap");
    assert!(rig.log_text().contains("price_unknown"), "the refusal is recorded");
    // Priced again: the call goes, and its cost is the vendor's usage at that price.
    rig.proxy
        .with_state(|g, _| g.set_price("openai", "gpt-known", Some(RIG_PRICE)).unwrap());
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    let last: serde_json::Value =
        serde_json::from_str(rig.log_text().lines().last().unwrap()).unwrap();
    assert_eq!(last["r"]["costMicros"], RIG_PRICE.cost(12, 5));
}

#[test]
fn key_tests_respect_a_halt_and_count_toward_the_burst() {
    let rig = Rig::new("proxy-kt-halt");
    rig.proxy.with_state(|g, _| g.member_halt("openai", now_ms()));
    let before = rig.vendor.seen.lock().unwrap().len();
    let r = rig.proxy.test_key("openai");
    assert!(matches!(r, Err(ProxyError::Refused(Refusal::Halted(_)))), "{r:?}");
    assert_eq!(rig.vendor.seen.lock().unwrap().len(), before, "nothing sent while halted");
    rig.proxy.with_state(|g, _| g.member_resume("openai").unwrap());
    for _ in 0..crate::provider_guard::BURST_MAX_CALLS {
        let _ = rig.proxy.test_key("openai");
    }
    let r = rig.proxy.test_key("openai");
    assert!(matches!(r, Err(ProxyError::Refused(Refusal::Halted(_)))), "{r:?}");
}

#[test]
fn custom_endpoints_with_ip_or_local_hosts_are_refused_at_add() {
    let rig = Rig::new("proxy-custom-host");
    for bad in ["https://169.254.169.254/v1", "https://localhost/v1", "https://llm.example.org./v1"] {
        assert!(rig.proxy.add_custom("x", bad, vec!["m".into()], 1).is_err(), "{bad}");
    }
}

#[test]
fn a_caller_that_goes_away_stops_the_vendor_call_and_is_logged_as_cancelled() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let rig = Rig::new("proxy-cancel");
    let flag = Arc::new(AtomicBool::new(false));
    let mut pieces = Vec::new();
    let r = rig.proxy.call_until(
        &rig.route("gpt-known"),
        req("gpt-known", true),
        None,
        flag.clone(),
        &mut |t| {
            pieces.push(t.to_string());
            // The caller's connection broke while this piece was being forwarded.
            flag.store(true, Ordering::SeqCst);
        },
    );
    assert_eq!(r.err(), Some(ProxyError::Cancelled));
    assert_eq!(pieces, vec!["hello ".to_string()], "no vendor read after the cancel");
    let log = rig.log_text();
    assert!(log.contains("\"cancelled\""), "the end record says cancelled: {log}");
    assert_eq!(ProxyError::Cancelled.outcome(), "cancelled");
    // A call that is not cancelled is unaffected.
    let ok = rig.proxy.call_until(
        &rig.route("gpt-known"),
        req("gpt-known", true),
        None,
        Arc::new(AtomicBool::new(false)),
        &mut |_| {},
    );
    assert_eq!(ok.map(|r| r.content).ok().as_deref(), Some("hello from the vendor"));
}

fn xai_rig(tag: &str) -> (Rig, ProxyRoute) {
    let rig = Rig::new(tag);
    rig.proxy
        .set_key("xai", SecretString::new(fake_key("xai-")), now_ms())
        .unwrap();
    let route = ProxyRoute {
        provider: ProviderId::Vendor(Vendor::Xai),
        model: "grok-4.7".into(),
        session_id: "sess-x".into(),
    };
    (rig, route)
}

#[test]
fn an_xai_call_whose_reasoning_could_pass_the_cap_is_refused_with_nothing_sent() {
    let (rig, route) = xai_rig("proxy-xai-bound");
    // A member cap below one call's worst case ($5/day against ~$6 reserved).
    rig.proxy.with_state(|g, _| {
        g.set_caps(
            "xai",
            Caps {
                daily_tokens: Some(1_000_000),
                monthly_tokens: Some(10_000_000),
                daily_micros: Some(5_000_000),
                monthly_micros: Some(50_000_000),
            },
        )
        .unwrap()
    });
    let before = rig.vendor.calls();
    let r = rig.proxy.call(&route, req("grok-4.7", false), None, &mut |_| {});
    // 500k reserved output tokens at $12/M is $6, over the $5 cap.
    match r {
        Err(ProxyError::Refused(Refusal::CapExceeded { call, .. })) => {
            assert!(call >= 6_000_000, "the reservation covers the whole window: {call}")
        }
        other => panic!("expected a cap refusal, got {other:?}"),
    }
    assert_eq!(rig.vendor.calls(), before, "nothing reached the vendor");
}

#[test]
fn the_xai_default_cap_admits_one_full_window_grok_call() {
    let (rig, route) = xai_rig("proxy-xai-default");
    let before = rig.vendor.calls();
    let r = rig.proxy.call(&route, req("grok-4.7", false), None, &mut |_| {});
    assert_eq!(
        r.map(|r| r.content).ok().as_deref(),
        Some("hello from the vendor"),
        "the owner's $20/day xAI default fits one ~$6 reservation"
    );
    assert_eq!(rig.vendor.calls(), before + 1);
}

#[test]
fn a_key_test_keeps_the_date_the_key_was_set() {
    // The rotation reminder reads `key_set_ms`; only setting a key may move it.
    let rig = Rig::new("proxy-kt-date");
    let set_at = rig.proxy.with_state(|g, _| g.get("openai").key_set_ms).unwrap();
    rig.proxy.test_key("openai").unwrap();
    assert_eq!(
        rig.proxy.with_state(|g, _| g.get("openai").key_set_ms),
        Some(set_at),
        "a key test must not reset the key's date"
    );
    // A real key set does.
    rig.proxy
        .set_key("openai", SecretString::new(fake_key("sk-")), set_at + 31 * 86_400_000)
        .unwrap();
    assert_eq!(
        rig.proxy.with_state(|g, _| g.get("openai").key_set_ms),
        Some(set_at + 31 * 86_400_000)
    );
}

/// Remove the anchor and every provider file: what "fake a fresh install" would take, short of
/// also deleting the keys.
fn wipe_anchor_and_files(rig: &Rig) {
    rig.kr
        .map
        .lock()
        .unwrap()
        .remove(crate::provider_usage_log::ANCHOR_ACCOUNT);
    for f in [
        crate::provider_usage_log::log_path(&rig.dir.0),
        crate::provider_guard::guard_path(&rig.dir.0),
    ] {
        let _ = std::fs::remove_file(f);
    }
}

fn fails_closed(rig: &Rig) -> (bool, bool) {
    rig.proxy
        .with_state(|g, l| (g.unreadable, l.ready().is_err()))
}

#[test]
fn a_missing_anchor_while_keys_remain_is_not_a_fresh_install() {
    // MAC keys and a vendor key remain: both the guard and the log fail closed.
    let mut rig = Rig::new("proxy-anchor-keys");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    wipe_anchor_and_files(&rig);
    rig.reopen();
    assert_eq!(fails_closed(&rig), (true, true), "guard unreadable, log broken");
    let before = rig.vendor.calls();
    let r = rig
        .proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {});
    assert!(r.is_err(), "{r:?}");
    assert_eq!(rig.vendor.calls(), before, "nothing is sent");
}

#[test]
fn a_missing_anchor_with_only_a_vendor_key_left_still_fails_closed() {
    let mut rig = Rig::new("proxy-anchor-vendor");
    wipe_anchor_and_files(&rig);
    {
        let mut m = rig.kr.map.lock().unwrap();
        m.remove(crate::provider_usage_log::LOG_MAC_ACCOUNT);
        m.remove(crate::provider_guard::GUARD_MAC_ACCOUNT);
    }
    rig.reopen();
    assert_eq!(fails_closed(&rig), (true, true));
}

#[test]
fn a_first_launch_with_no_calls_reopens_cleanly() {
    // A fresh install opens (creating the MAC keys) and closes with nothing recorded: the next
    // launch is not mistaken for a wiped anchor.
    let dir = crate::provider_fixtures_tests::TmpDir::new("proxy-anchor-fresh");
    let kr = Arc::new(MemKeyring::default());
    let vendor = ScriptedVendor::new(&["gpt-known"], "x", None);
    drop(ModelProxy::new(dir.0.clone(), kr.clone(), vendor.clone()));
    let p = ModelProxy::new(dir.0.clone(), kr.clone(), vendor);
    assert_eq!(p.with_state(|g, l| (g.unreadable, l.ready().is_err())), (false, false));
}

#[test]
fn resuming_the_guard_alone_does_not_legitimise_a_cut_short_log() {
    // Anchor removed and the log cut short while provider keys remain: halted and broken.
    let mut rig = Rig::new("proxy-anchor-resume");
    for _ in 0..3 {
        rig.proxy
            .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
            .unwrap();
    }
    rig.kr
        .map
        .lock()
        .unwrap()
        .remove(crate::provider_usage_log::ANCHOR_ACCOUNT);
    let p = crate::provider_usage_log::log_path(&rig.dir.0);
    let kept: Vec<String> = std::fs::read_to_string(&p)
        .unwrap_or_default()
        .lines()
        .take(3)
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&p, kept.concat()).unwrap();
    rig.reopen();
    assert_eq!(fails_closed(&rig), (true, true));
    // The member resumes the guard only (what the halt message offers), then the app restarts.
    let _ = crate::providers_ipc::resume(&rig.proxy, "openai");
    rig.reopen();
    assert!(
        rig.proxy.with_state(|_, l| l.ready().is_err()),
        "a guard resume must not make a cut-short log valid; only a log reset may"
    );
    // The log reset is the way back: it archives the file and starts a new chain.
    rig.proxy
        .with_state(|_, l| l.reset_broken(now_ms()).map(|_| ()))
        .unwrap();
    let _ = crate::providers_ipc::resume(&rig.proxy, "openai");
    rig.reopen();
    assert_eq!(fails_closed(&rig), (false, false));
}

#[test]
fn a_first_launch_that_cannot_write_its_anchor_fails_closed() {
    let dir = crate::provider_fixtures_tests::TmpDir::new("proxy-anchor-nowrite");
    let kr = Arc::new(MemKeyring::default());
    *kr.fail_set_account.lock().unwrap() =
        Some(crate::provider_usage_log::ANCHOR_ACCOUNT.to_string());
    let vendor = ScriptedVendor::new(&["gpt-known"], "x", None);
    let p = ModelProxy::new(dir.0.clone(), kr.clone(), vendor);
    assert_eq!(
        p.with_state(|g, l| (g.unreadable, l.ready().is_err())),
        (true, true),
        "without an anchor the first launch must not go on as if it had one"
    );
}
