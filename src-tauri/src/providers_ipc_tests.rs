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
    assert_eq!(o.today.unpriced_calls, 2, "no price known: spend is unknown, not zero");
    assert_eq!(o.unsettled_calls, 0);
    // The log is intact, so it is not reset.
    assert!(usage_log_reset(&rig.proxy).is_err());
}
