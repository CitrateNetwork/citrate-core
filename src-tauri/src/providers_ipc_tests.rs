// D-11 — the provider commands' bodies: honest status, usage from the signed log, guard changes
// that persist, and no key in any result.

use super::*;
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
    assert_eq!(openai.adapter, Some("openai-compatible"));
    assert_eq!(openai.listed_models, vec!["gpt-known".to_string()]);
    assert!(openai.key_set_ms.is_some());
    let anthropic = &s[1];
    assert!(!anthropic.key_configured);
    assert_eq!(anthropic.adapter, None, "no adapter yet is said, not hidden");
    // A custom endpoint is listed, unverified and unconfirmed.
    let c = custom_add(&rig.proxy, "Box", "https://llm.example.org/v1", vec!["m".into()]).unwrap();
    assert!(!c.confirmed && !c.verified);
    let s = status(&rig.proxy);
    let v = s.iter().find(|v| v.id == c.id).unwrap();
    assert_eq!((v.kind, v.verified, v.confirmed), ("custom", false, false));
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

#[test]
fn model_prices_say_where_each_price_came_from() {
    let published = vec![
        ModelInfo {
            id: "pub-m".into(),
            display_name: "Pub".into(),
            supports_tools: true,
            input_micros_per_mtok: Some(1_000_000),
            output_micros_per_mtok: Some(4_000_000),
        },
        ModelInfo {
            id: "half-priced".into(),
            display_name: "Half".into(),
            supports_tools: true,
            input_micros_per_mtok: Some(1),
            output_micros_per_mtok: None,
        },
    ];
    let mut pg = ProviderGuard::default();
    pg.listed_models = vec!["pub-m".into(), "listed-only".into()];
    pg.approved_models.insert("approved-only".into());
    let member = Price { input_micros_per_mtok: 7, output_micros_per_mtok: 9 };
    pg.prices.insert("member-m".into(), member);
    let v = model_prices(&published, &["custom-m".into()], &pg);
    let by = |id: &str| v.iter().find(|m| m.id == id).cloned().unwrap();
    assert_eq!(v.len(), 6, "each model once: {v:?}");
    assert_eq!(by("pub-m").source, Some("published"));
    assert_eq!(by("pub-m").price.unwrap().output_micros_per_mtok, 4_000_000);
    assert_eq!((by("half-priced").source, by("half-priced").price), (None, None), "half a price is no price");
    assert_eq!((by("listed-only").source, by("approved-only").source, by("custom-m").source), (None, None, None));
    assert_eq!((by("member-m").source, by("member-m").price), (Some("member"), Some(member)));
    // The member's price wins over the published one, as it does when a call is charged.
    pg.prices.insert("pub-m".into(), member);
    let v = model_prices(&published, &[], &pg);
    let p = v.iter().find(|m| m.id == "pub-m").unwrap();
    assert_eq!((p.source, p.price), (Some("member"), Some(member)));
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
