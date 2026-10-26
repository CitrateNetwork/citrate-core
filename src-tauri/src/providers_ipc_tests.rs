// D-11 — the provider commands' bodies: honest status, usage from the signed log, guard changes
// that persist, and no key in any result.

use super::*;
use crate::model_provider::ModelInfo;
use crate::model_proxy::tests::{req, Rig};

fn all_views_json(rig: &Rig) -> String {
    let p = &rig.proxy;
    let mut out = String::new();
    out.push_str(&serde_json::to_string(&status(p)).unwrap());
    out.push_str(&serde_json::to_string(&p.with_state(|g, l| usage_summary_of(g, l, now_ms()))).unwrap());
    out.push_str(&serde_json::to_string(&key_test(p, "openai").unwrap()).unwrap());
    out.push_str(&serde_json::to_string(&caps_set(p, "openai", Caps::default()).unwrap()).unwrap());
    out.push_str(&serde_json::to_string(&halt(p, "openai").unwrap()).unwrap());
    out.push_str(&serde_json::to_string(&resume(p, "openai").unwrap()).unwrap());
    out
}

#[test]
fn no_command_result_carries_the_key() {
    let rig = Rig::new("ipc-nokey");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    let json = all_views_json(&rig);
    assert!(json.contains("openai"), "the views were built");
    assert!(!json.contains(&rig.key), "a key crossed into a command result");
    let view = key_set(&rig.proxy, "openai", SecretString::new(rig.key.clone())).unwrap();
    assert!(!serde_json::to_string(&view).unwrap().contains(&rig.key));
}

#[test]
fn status_is_honest_about_keys_adapters_and_custom_endpoints() {
    let rig = Rig::new("ipc-status");
    let s = status(&rig.proxy);
    let ids: Vec<&str> = s.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, vec!["openai", "anthropic", "gemini", "xai", "deepseek", "meta"]);
    let openai = &s[0];
    assert!(openai.key_configured && openai.verified);
    assert_eq!(openai.host, "api.openai.com");
    assert_eq!(openai.adapter, Some("native"));
    assert_eq!(openai.listed_models, vec!["gpt-known".to_string()]);
    assert!(openai.key_set_ms.is_some());
    let anthropic = &s[1];
    assert!(!anthropic.key_configured);
    assert_eq!(anthropic.adapter, Some("native"));
    assert!(s.iter().all(|v| v.adapter == Some("native")), "every vendor has its native adapter");
    // A custom endpoint is listed, unverified and unconfirmed.
    let c = custom_add(&rig.proxy, "Box", "https://llm.example.org/v1", vec!["m".into()]).unwrap();
    assert!(!c.confirmed && !c.verified);
    let s = status(&rig.proxy);
    let v = s.iter().find(|v| v.id == c.id).unwrap();
    assert_eq!((v.kind, v.verified, v.confirmed), ("custom", false, false));
    assert_eq!(v.adapter, Some("openai-compatible"));
    let c2 = custom_confirm(&rig.proxy, &c.id, "https://llm.example.org/v1").unwrap();
    assert!(c2.confirmed && !c2.verified);
}

#[test]
fn guard_changes_persist_and_unknown_ids_are_refused() {
    let mut rig = Rig::new("ipc-guard");
    let caps = Caps {
        daily_tokens: Some(5_000),
        monthly_tokens: Some(50_000),
        daily_micros: Some(1_000_000),
        monthly_micros: Some(9_000_000),
    };
    let v = caps_set(&rig.proxy, "openai", caps).unwrap();
    assert_eq!(v.caps, caps);
    let v = halt(&rig.proxy, "openai").unwrap();
    assert_eq!(v.halt.as_ref().unwrap().kind, "member");
    rig.reopen();
    let s = status(&rig.proxy);
    assert_eq!(s[0].caps, caps);
    assert!(s[0].halt.is_some());
    assert!(resume(&rig.proxy, "openai").unwrap().halt.is_none());
    assert!(caps_set(&rig.proxy, "nope", caps).is_err());
    assert!(caps_set(&rig.proxy, "custom-missing", caps).is_err());
    let p = price_set(
        &rig.proxy,
        "openai",
        "gpt-known",
        Some(Price { input_micros_per_mtok: 1, output_micros_per_mtok: 2 }),
    )
    .unwrap();
    assert_eq!(p.prices.len(), 1);
}

#[test]
fn usage_summary_comes_from_the_signed_log() {
    let rig = Rig::new("ipc-usage");
    for _ in 0..2 {
        rig.proxy
            .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
            .unwrap();
    }
    let u = rig.proxy.with_state(|g, l| usage_summary_of(g, l, now_ms()));
    assert!(u.log_intact);
    let o = u.providers.iter().find(|p| p.id == "openai").unwrap();
    assert_eq!(o.today.calls, 2);
    assert_eq!(o.today.tokens, 2 * (12 + 5));
    assert_eq!(o.today.unpriced_calls, 0);
    let one = crate::model_proxy::tests::RIG_PRICE.cost(12, 5);
    assert_eq!(o.today.spend_micros, 2 * one, "spend is the vendor's usage at the price");
    assert_eq!(o.caps, Caps::default());
    assert_eq!(o.caps.daily_micros, Some(5_000_000));
    assert_eq!(o.unsettled_calls, 0);
    // The log is intact, so it is not reset.
    assert!(usage_log_reset(&rig.proxy).is_err());
}

#[test]
fn usage_summary_lists_recent_calls_as_metadata_only() {
    let rig = Rig::new("ipc-recent");
    for _ in 0..2 {
        rig.proxy
            .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
            .unwrap();
    }
    let u = rig.proxy.with_state(|g, l| usage_summary_of(g, l, now_ms()));
    assert_eq!(u.recent_calls.len(), 2);
    let c = &u.recent_calls[0];
    assert_eq!((c.provider.as_str(), c.model.as_str(), c.outcome.as_deref()), ("openai", "gpt-known", Some("ok")));
    assert_eq!((c.tokens_in, c.tokens_out), (Some(12), Some(5)));
    assert!(c.cost_micros.is_some(), "the rig's model is priced, so the cost is known");
    let json = serde_json::to_string(&u).unwrap();
    assert!(json.contains("\"recentCalls\""));
    assert!(!json.contains(&rig.key));
    assert!(!json.contains("sess-1"), "no session id in the view");
}

/// A test-only adapter that serves a fixed model list (prices included).
struct ListedModels(Vec<ModelInfo>);

impl ModelProvider for ListedModels {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(Vendor::OpenAi)
    }
    fn models(&self) -> Vec<ModelInfo> {
        self.0.clone()
    }
    fn key_format(&self) -> crate::model_provider::KeyFormat {
        crate::model_provider::KeyFormat::ANY
    }
    fn output_bound(&self, req: &ChatRequest) -> u64 {
        u64::from(req.max_tokens)
    }
    fn test_key(
        &self,
        _ctx: &crate::model_provider::ProviderCtx<'_>,
    ) -> Result<crate::model_provider::KeyTest, crate::model_provider::ProviderError> {
        unreachable!("not called by model_prices")
    }
    fn complete(
        &self,
        _ctx: &crate::model_provider::ProviderCtx<'_>,
        _req: &ChatRequest,
        _on_text: &mut dyn FnMut(&str),
    ) -> Result<crate::model_provider::ChatResponse, crate::model_provider::ProviderError> {
        unreachable!("not called by model_prices")
    }
}

#[test]
fn model_prices_say_where_each_price_came_from() {
    let info = |id: &str, i: Option<u64>, o: Option<u64>| ModelInfo {
        id: id.into(),
        display_name: id.into(),
        supports_tools: true,
        input_micros_per_mtok: i,
        output_micros_per_mtok: o,
    };
    let adapter = ListedModels(vec![
        info("pub-m", Some(1_000_000), Some(4_000_000)),
        info("half-priced", Some(1), None),
    ]);
    let member = Price { input_micros_per_mtok: 7, output_micros_per_mtok: 9 };
    let mut g = GuardBook::default();
    g.key_set("openai", vec!["pub-m".into(), "listed-only".into()], 1);
    g.set_price("openai", "member-m", Some(member)).unwrap();
    let v = model_prices(&g, "openai", Some(&adapter), &["custom-m".into()]);
    let by = |v: &[ModelPriceView], id: &str| v.iter().find(|m| m.id == id).cloned().unwrap();
    assert_eq!(v.len(), 5, "each model once: {v:?}");
    assert_eq!(by(&v, "pub-m").source, Some("published"));
    assert_eq!(by(&v, "pub-m").price.unwrap().output_micros_per_mtok, 4_000_000);
    assert_eq!((by(&v, "half-priced").source, by(&v, "half-priced").price), (None, None), "half a price is no price");
    assert_eq!((by(&v, "listed-only").source, by(&v, "custom-m").source), (None, None));
    assert_eq!((by(&v, "member-m").source, by(&v, "member-m").price), (Some("member"), Some(member)));
    // The member's price wins over the published one, as it does when a call is charged.
    g.set_price("openai", "pub-m", Some(member)).unwrap();
    let v = model_prices(&g, "openai", Some(&adapter), &[]);
    let p = by(&v, "pub-m");
    assert_eq!((p.source, p.price), (Some("member"), Some(member)));
    // Every shown price is the one the guard charges with.
    for m in &v {
        let adapter_price = ModelProxy::adapter_price(&adapter, &m.id);
        assert_eq!(m.price, g.price_for("openai", &m.id, adapter_price), "{}", m.id);
    }
}

#[test]
fn the_price_shown_is_the_price_a_call_is_charged() {
    let rig = Rig::new("ipc-shown-is-charged");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    let shown = status(&rig.proxy)[0]
        .models
        .iter()
        .find(|m| m.id == "gpt-known")
        .and_then(|m| m.price)
        .expect("the rig's model shows a price");
    let u = rig.proxy.with_state(|g, l| usage_summary_of(g, l, now_ms()));
    let c = &u.recent_calls[0];
    let (tin, tout) = (c.tokens_in.unwrap(), c.tokens_out.unwrap());
    assert_eq!(c.cost_micros, Some(shown.cost(tin, tout)), "the charge used the price the card shows");
}

#[test]
fn status_lists_each_model_with_its_price_source() {
    let rig = Rig::new("ipc-model-prices");
    let s = status(&rig.proxy);
    let m = s[0].models.iter().find(|m| m.id == "gpt-known");
    assert!(m.is_some(), "the priced model is listed: {:?}", s[0].models);
    assert_eq!(m.and_then(|m| m.source), Some("member"));
    price_set(&rig.proxy, "openai", "gpt-known", None).unwrap();
    let s = status(&rig.proxy);
    let m = s[0].models.iter().find(|m| m.id == "gpt-known").unwrap();
    assert_eq!((m.source, m.price), (None, None), "an unknown price is shown as unknown");
}

#[test]
fn usage_summary_reports_a_clock_moved_back() {
    let rig = Rig::new("ipc-clock");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    let now = now_ms();
    let ok = rig.proxy.with_state(|g, l| usage_summary_of(g, l, now));
    assert_eq!(ok.clock_problem, None);
    let behind = rig
        .proxy
        .with_state(|g, l| usage_summary_of(g, l, now - crate::provider_guard::DAY_MS));
    assert!(behind.clock_problem.as_deref().unwrap_or("").contains("clock"), "{:?}", behind.clock_problem);
    assert!(serde_json::to_string(&behind).unwrap().contains("\"clockProblem\""));
}

#[test]
fn status_reports_the_least_reservation_per_call_from_the_adapter_bound() {
    let rig = Rig::new("ipc-reserve");
    let s = status(&rig.proxy);
    let xai = s.iter().find(|v| v.id == "xai").unwrap();
    let grok = xai.models.iter().find(|m| m.id == "grok-4.7").expect("grok-4.7 is a published xAI model");
    let price = grok.price.expect("grok-4.7 ships a published price");
    // xAI documents no reasoning cap, so one call reserves the full 500k context window as output.
    let floor = price.cost(0, 500_000);
    let r = grok.min_reserve_micros.expect("a priced model reports its reservation");
    assert!(r >= floor, "reserve {r} below the output-bound cost {floor}");
    assert!(r > 5_000_000, "a grok-4.7 call reserves more than the $5 default daily cap: {r}");
    // Every reported reservation equals what the proxy would reserve for an empty prompt.
    for v in &s {
        for m in &v.models {
            assert_eq!(m.min_reserve_micros.is_some(), m.price.is_some(), "{}/{}", v.id, m.id);
        }
    }
}
