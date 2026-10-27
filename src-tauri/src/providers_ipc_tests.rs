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
fn earlier_spend_is_flagged_unknown_after_an_unanchored_reset_until_the_period_rolls() {
    let mut rig = Rig::new("ipc-earlier-unknown");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    // A normal summary: nothing unknown.
    let s = rig.proxy.with_state(|g, l| usage_summary_of(g, l, now_ms()));
    let o = s.providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(!o.today.earlier_spend_unknown && !o.month.earlier_spend_unknown);
    // Anchor removed, then the member resets the log: earlier spend is unknown for every provider.
    rig.kr
        .map
        .lock()
        .unwrap()
        .remove(crate::provider_usage_log::ANCHOR_ACCOUNT);
    rig.reopen();
    let at = now_ms();
    rig.proxy
        .with_state(|_, l| l.reset_broken(at).map(|_| ()))
        .unwrap();
    rig.reopen();
    let s = rig.proxy.with_state(|g, l| usage_summary_of(g, l, at + 1));
    for p in &s.providers {
        assert!(p.today.earlier_spend_unknown, "{}: today", p.id);
        assert!(p.month.earlier_spend_unknown, "{}: month", p.id);
    }
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"earlierSpendUnknown\":true"), "wire name");
    // The next UTC day clears the day flag; the next month clears both.
    let next_day = (at / 86_400_000 + 1) * 86_400_000 + 1;
    let s = rig.proxy.with_state(|g, l| usage_summary_of(g, l, next_day));
    let same_month =
        crate::provider_guard::month_of(next_day) == crate::provider_guard::month_of(at);
    let o = s.providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(!o.today.earlier_spend_unknown);
    assert_eq!(o.month.earlier_spend_unknown, same_month);
    let next_month = at + 32 * 86_400_000;
    let s = rig.proxy.with_state(|g, l| usage_summary_of(g, l, next_month));
    let o = s.providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(!o.today.earlier_spend_unknown && !o.month.earlier_spend_unknown);
}

#[test]
fn a_reset_with_its_anchor_intact_leaves_earlier_spend_known() {
    let mut rig = Rig::new("ipc-earlier-known");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    let p = crate::provider_usage_log::log_path(&rig.dir.0);
    let t = std::fs::read_to_string(&p)
        .unwrap_or_default()
        .replace("\"tokensIn\":12", "\"tokensIn\":1");
    std::fs::write(&p, t).unwrap();
    rig.reopen();
    let at = now_ms();
    rig.proxy
        .with_state(|_, l| l.reset_broken(at).map(|_| ()))
        .unwrap();
    let s = rig.proxy.with_state(|g, l| usage_summary_of(g, l, at + 1));
    let o = s.providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(!o.today.earlier_spend_unknown && !o.month.earlier_spend_unknown);
    assert!(o.today.tokens >= 17, "the anchored totals were carried");
}

#[test]
fn a_later_normal_reset_in_the_same_period_keeps_earlier_spend_unknown() {
    let mut rig = Rig::new("ipc-earlier-carried");
    rig.proxy
        .call(&rig.route("gpt-known"), req("gpt-known", false), None, &mut |_| {})
        .unwrap();
    rig.kr
        .map
        .lock()
        .unwrap()
        .remove(crate::provider_usage_log::ANCHOR_ACCOUNT);
    rig.reopen();
    let at = now_ms();
    rig.proxy.reset_usage_log(at).unwrap();
    crate::providers_ipc::resume(&rig.proxy, "openai").unwrap();
    // The log breaks again (a stray line) and the member resets it normally, same period.
    let p = crate::provider_usage_log::log_path(&rig.dir.0);
    let mut t = std::fs::read_to_string(&p).unwrap_or_default();
    t.push_str("not a record\n");
    std::fs::write(&p, t).unwrap();
    rig.reopen();
    assert!(rig.proxy.with_state(|_, l| l.ready().is_err()));
    rig.proxy.reset_usage_log(at + 2).unwrap();
    rig.reopen();
    let s = rig.proxy.with_state(|g, l| usage_summary_of(g, l, at + 3));
    let o = s.providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(
        o.today.earlier_spend_unknown && o.month.earlier_spend_unknown,
        "only the period rollover clears it"
    );
    let next_month = at + 32 * 86_400_000;
    let s = rig.proxy.with_state(|g, l| usage_summary_of(g, l, next_month));
    let o = s.providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(!o.today.earlier_spend_unknown && !o.month.earlier_spend_unknown);
}
