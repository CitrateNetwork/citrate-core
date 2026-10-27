// D-11 — the spend guard: caps are a hard stop before every call, the anomaly halt pauses a
// provider until the member resumes it, and the persisted state fails closed when it cannot be
// trusted.

use super::*;
use crate::provider_fixtures_tests::{MemKeyring, TmpDir};
use std::sync::Arc;

const T0: u64 = 20_752 * DAY_MS + 3_600_000; // 2026-10-26 01:00 UTC

fn plan(model: &str, input: u64, output: u64) -> CallPlan {
    CallPlan {
        provider: "openai".into(),
        model: model.into(),
        reserve_in: input,
        reserve_out: output,
        // Priced, so the default currency caps can be checked ($1 / $2 per million tokens).
        price: Some(Price {
            input_micros_per_mtok: 1_000_000,
            output_micros_per_mtok: 2_000_000,
        }),
    }
}

fn known() -> Vec<String> {
    vec!["gpt-known".to_string()]
}

fn used(tokens: u64, micros: u64) -> PeriodUse {
    PeriodUse {
        tokens,
        micros,
        unpriced_calls: 0,
    }
}

#[test]
fn utc_day_and_month_boundaries() {
    assert_eq!(month_of(T0), (2026, 10));
    assert_eq!(month_of(0), (1970, 1));
    // 2024-02-29 12:00 UTC = day 19782.
    assert_eq!(month_of(19_782 * DAY_MS + 43_200_000), (2024, 2));
    // 2026-10-31 23:59:59.999 is still October; one ms later is November.
    let nov1 = 20_758 * DAY_MS;
    assert_eq!(month_of(nov1 - 1), (2026, 10));
    assert_eq!(month_of(nov1), (2026, 11));
    assert_eq!(day_of(nov1 - 1) + 1, day_of(nov1));
}

#[test]
fn price_cost_rounds_up_and_never_overflows() {
    let p = Price {
        input_micros_per_mtok: 3_000_000,
        output_micros_per_mtok: 15_000_000,
    };
    assert_eq!(p.cost(1_000_000, 0), 3_000_000);
    assert_eq!(p.cost(1, 0), 3);
    assert_eq!(p.cost(1, 1), 18);
    let big = Price {
        input_micros_per_mtok: u64::MAX,
        output_micros_per_mtok: u64::MAX,
    };
    assert_eq!(big.cost(u64::MAX, u64::MAX), u64::MAX);
}

#[test]
fn token_caps_are_a_hard_stop_before_the_call() {
    let mut g = GuardBook::default();
    g.set_caps(
        "openai",
        Caps {
            daily_tokens: Some(10_000),
            monthly_tokens: Some(50_000),
            daily_micros: None,
            monthly_micros: None,
        },
    )
    .unwrap();
    // Fits exactly.
    assert!(g
        .admit(&plan("gpt-known", 4_000, 1_000), &known(), used(5_000, 0), used(5_000, 0), T0)
        .is_ok());
    // One token over the daily cap.
    let r = g.admit(&plan("gpt-known", 4_001, 1_000), &known(), used(5_000, 0), used(5_000, 0), T0);
    assert!(
        matches!(r, Err(Refusal::CapExceeded { which: "daily token", cap: 10_000, .. })),
        "{r:?}"
    );
    // Monthly cap.
    let r = g.admit(&plan("gpt-known", 100, 100), &known(), used(0, 0), used(49_900, 0), T0);
    assert!(matches!(r, Err(Refusal::CapExceeded { which: "monthly token", .. })), "{r:?}");
    // A refusal is not an anomaly: the provider is not halted.
    assert!(g.get("openai").halt.is_none());
}

#[test]
fn default_caps_apply_to_a_provider_never_configured() {
    let mut g = GuardBook::default();
    let r = g.admit(
        &plan("gpt-known", 10, DEFAULT_DAILY_TOKENS),
        &known(),
        used(0, 0),
        used(0, 0),
        T0,
    );
    assert!(matches!(r, Err(Refusal::CapExceeded { .. })), "{r:?}");
}

#[test]
fn currency_caps_need_a_price_and_stop_over_the_cap() {
    let mut g = GuardBook::default();
    g.set_caps(
        "openai",
        Caps {
            daily_tokens: None,
            monthly_tokens: None,
            daily_micros: Some(1_000_000),
            monthly_micros: Some(5_000_000),
        },
    )
    .unwrap();
    // No price: refused rather than counted as free.
    let mut unpriced = plan("gpt-known", 10, 10);
    unpriced.price = None;
    let r = g.admit(&unpriced, &known(), used(0, 0), used(0, 0), T0);
    assert_eq!(r, Err(Refusal::PriceUnknown));
    let mut p = plan("gpt-known", 100_000, 10_000);
    p.price = Some(Price {
        input_micros_per_mtok: 5_000_000,
        output_micros_per_mtok: 15_000_000,
    });
    // 0.5 + 0.15 = 0.65 USD this call; 0.40 already used today: over the 1 USD daily cap.
    let r = g.admit(&p, &known(), used(0, 400_000), used(0, 400_000), T0);
    assert!(matches!(r, Err(Refusal::CapExceeded { which: "daily spend", call: 650_000, .. })), "{r:?}");
    assert!(g.admit(&p, &known(), used(0, 300_000), used(0, 300_000), T0).is_ok());
    // A member price overrides the adapter's.
    g.set_price("openai", "gpt-known", Some(Price { input_micros_per_mtok: 1, output_micros_per_mtok: 2 }))
        .unwrap();
    assert_eq!(
        g.price_for("openai", "gpt-known", None),
        Some(Price { input_micros_per_mtok: 1, output_micros_per_mtok: 2 })
    );
}

#[test]
fn an_unknown_model_halts_the_provider_and_resuming_approves_it() {
    let mut g = GuardBook::default();
    let r = g.admit(&plan("gpt-surprise", 10, 10), &known(), used(0, 0), used(0, 0), T0);
    assert!(
        matches!(&r, Err(Refusal::Halted(Halt { reason: HaltReason::UnknownModel { model }, .. })) if model == "gpt-surprise"),
        "{r:?}"
    );
    // Halted: even a known model is refused now.
    let r = g.admit(&plan("gpt-known", 10, 10), &known(), used(0, 0), used(0, 0), T0);
    assert!(matches!(r, Err(Refusal::Halted(_))), "{r:?}");
    g.member_resume("openai").unwrap();
    assert!(g.get("openai").approved_models.contains("gpt-surprise"));
    assert!(g
        .admit(&plan("gpt-surprise", 10, 10), &known(), used(0, 0), used(0, 0), T0)
        .is_ok());
    // A model the key test listed is known.
    g.key_set("openai", vec!["gpt-listed".into()], T0);
    assert!(g.model_known("openai", "gpt-listed", &[]));
}

#[test]
fn a_burst_of_calls_halts_the_provider() {
    let mut g = GuardBook::default();
    for i in 0..BURST_MAX_CALLS as u64 {
        g.admit(&plan("gpt-known", 1, 1), &known(), used(0, 0), used(0, 0), T0 + i)
            .unwrap();
    }
    let r = g.admit(&plan("gpt-known", 1, 1), &known(), used(0, 0), used(0, 0), T0 + 100);
    assert!(
        matches!(r, Err(Refusal::Halted(Halt { reason: HaltReason::Burst { .. }, .. }))),
        "{r:?}"
    );
    g.member_resume("openai").unwrap();
    // Outside the window the old calls no longer count.
    let mut g = GuardBook::default();
    for i in 0..BURST_MAX_CALLS as u64 {
        g.admit(&plan("gpt-known", 1, 1), &known(), used(0, 0), used(0, 0), T0 + i)
            .unwrap();
    }
    assert!(g
        .admit(&plan("gpt-known", 1, 1), &known(), used(0, 0), used(0, 0), T0 + BURST_WINDOW_MS + 100)
        .is_ok());
}

#[test]
fn token_spikes_halt_the_provider() {
    let mut g = GuardBook::default();
    g.set_caps("openai", Caps { daily_tokens: None, monthly_tokens: None, daily_micros: None, monthly_micros: None })
        .unwrap();
    // One call's input bound above the ceiling: refused and halted before sending.
    let r = g.admit(&plan("gpt-known", SPIKE_INPUT_CEILING + 1, 1), &known(), used(0, 0), used(0, 0), T0);
    assert!(
        matches!(r, Err(Refusal::Halted(Halt { reason: HaltReason::TokenSpike { .. }, .. }))),
        "{r:?}"
    );
    // A finished call far above the trailing mean halts after it.
    let mut g = GuardBook::default();
    for _ in 0..SPIKE_MIN_SAMPLES {
        assert!(g.observe("openai", 10_000, T0).is_none());
    }
    assert!(g.observe("openai", 60_000, T0).is_none(), "under 8x mean is fine");
    let h = g.observe("openai", 200_000, T0);
    assert!(matches!(h, Some(Halt { reason: HaltReason::TokenSpike { tokens: 200_000, .. }, .. })), "{h:?}");
    assert!(g.get("openai").halt.is_some());
}

#[test]
fn member_halt_and_resume() {
    let mut g = GuardBook::default();
    g.member_halt("xai", T0);
    let mut p = plan("gpt-known", 1, 1);
    p.provider = "xai".into();
    assert!(matches!(g.admit(&p, &known(), used(0, 0), used(0, 0), T0), Err(Refusal::Halted(_))));
    g.member_resume("xai").unwrap();
    assert!(g.admit(&p, &known(), used(0, 0), used(0, 0), T0).is_ok());
}

#[test]
fn caps_are_validated() {
    let mut g = GuardBook::default();
    let bad = Caps { daily_tokens: Some(10), monthly_tokens: Some(5), daily_micros: None, monthly_micros: None };
    assert!(g.set_caps("openai", bad).is_err());
    let huge = Caps { daily_tokens: Some(MAX_TOKEN_CAP + 1), monthly_tokens: None, daily_micros: None, monthly_micros: None };
    assert!(g.set_caps("openai", huge).is_err());
    assert!(g.set_price("openai", "m", Some(Price { input_micros_per_mtok: MAX_PRICE_MICROS_PER_MTOK + 1, output_micros_per_mtok: 0 })).is_err());
}

#[test]
fn state_persists_sealed_and_an_edited_file_fails_closed() {
    let dir = TmpDir::new("guard-seal");
    let kr = Arc::new(MemKeyring::default());
    let mut g = load(&dir.0, kr.clone(), T0);
    assert!(!g.unreadable);
    g.set_caps("openai", Caps { daily_tokens: Some(777), monthly_tokens: Some(7_777), daily_micros: None, monthly_micros: None })
        .unwrap();
    g.member_halt("xai", T0);
    save(&dir.0, &mut g).unwrap();
    let g2 = load(&dir.0, kr.clone(), T0);
    assert_eq!(g2.get("openai").caps.daily_tokens, Some(777));
    assert!(g2.get("xai").halt.is_some());
    // An edit made outside the app (raise the cap) is detected: every provider is halted.
    let p = guard_path(&dir.0);
    let text = std::fs::read_to_string(&p).unwrap().replace("777", "999");
    std::fs::write(&p, text).unwrap();
    let mut g3 = load(&dir.0, kr.clone(), T0);
    assert!(g3.unreadable, "an edited guard file must not be trusted");
    assert!(matches!(
        g3.admit(&plan("gpt-known", 1, 1), &known(), used(0, 0), used(0, 0), T0),
        Err(Refusal::Halted(Halt { reason: HaltReason::GuardUnreadable, .. }))
    ));
    assert!(save(&dir.0, &mut g3).is_err(), "nothing is saved until the member resumes");
    // Resuming is the member's acknowledgement: defaults, readable again.
    g3.member_resume("openai").unwrap();
    assert!(!g3.unreadable);
    assert_eq!(g3.get("openai").caps, Caps::default());
    save(&dir.0, &mut g3).unwrap();
    assert!(!load(&dir.0, kr.clone(), T0).unreadable);
}

#[test]
fn an_unreadable_keyring_fails_closed() {
    let dir = TmpDir::new("guard-kr");
    let mut g = load(&dir.0, Arc::new(MemKeyring::failing()), T0);
    assert!(g.unreadable);
    assert!(g
        .admit(&plan("gpt-known", 1, 1), &known(), used(0, 0), used(0, 0), T0)
        .is_err());
}

#[test]
fn owner_default_caps_include_currency_and_an_unknown_price_is_refused() {
    // Owner sign-off (ADR P-7a, fed#394): $5/day and $50/month per provider, plus 1M/10M tokens.
    let d = Caps::default();
    assert_eq!(d.daily_tokens, Some(1_000_000));
    assert_eq!(d.monthly_tokens, Some(10_000_000));
    assert_eq!(d.daily_micros, Some(5_000_000));
    assert_eq!(d.monthly_micros, Some(50_000_000));
    let mut g = GuardBook::default();
    // A provider never configured: an unpriced model is refused, never counted as free.
    let mut unpriced = plan("gpt-known", 10, 10);
    unpriced.price = None;
    let r = g.admit(&unpriced, &known(), used(0, 0), used(0, 0), T0);
    assert_eq!(r, Err(Refusal::PriceUnknown));
    // Priced: the $5 daily default is a hard stop.
    let mut p = plan("gpt-known", 100_000, 10_000);
    p.price = Some(Price {
        input_micros_per_mtok: 5_000_000,
        output_micros_per_mtok: 15_000_000,
    }); // this call costs up to $0.65
    assert!(g.admit(&p, &known(), used(0, 4_350_000), used(0, 4_350_000), T0).is_ok());
    let r = g.admit(&p, &known(), used(0, 4_350_001), used(0, 4_350_001), T0);
    assert!(matches!(r, Err(Refusal::CapExceeded { which: "daily spend", cap: 5_000_000, .. })), "{r:?}");
    // And the $50 monthly default.
    let r = g.admit(&p, &known(), used(0, 0), used(0, 49_500_000), T0);
    assert!(matches!(r, Err(Refusal::CapExceeded { which: "monthly spend", cap: 50_000_000, .. })), "{r:?}");
}

#[test]
fn a_deleted_or_rolled_back_guard_file_fails_closed() {
    let dir = TmpDir::new("guard-rollback");
    let kr = Arc::new(MemKeyring::default());
    let mut g = load(&dir.0, kr.clone(), T0);
    g.set_caps("openai", Caps { daily_tokens: Some(10), monthly_tokens: Some(10), daily_micros: None, monthly_micros: None })
        .unwrap();
    save(&dir.0, &mut g).unwrap();
    let old = std::fs::read(guard_path(&dir.0)).unwrap();
    g.member_halt("openai", T0);
    save(&dir.0, &mut g).unwrap();
    assert!(load(&dir.0, kr.clone(), T0).get("openai").halt.is_some());
    // Rolled back to the older (validly sealed) copy: the halt would silently vanish.
    std::fs::write(guard_path(&dir.0), &old).unwrap();
    let g2 = load(&dir.0, kr.clone(), T0);
    assert!(g2.unreadable, "an older sealed guard file must not load as current");
    // Deleted: the caps would silently revert to defaults.
    let dir = TmpDir::new("guard-deleted");
    let kr = Arc::new(MemKeyring::default());
    let mut g = load(&dir.0, kr.clone(), T0);
    g.member_halt("openai", T0);
    save(&dir.0, &mut g).unwrap();
    std::fs::remove_file(guard_path(&dir.0)).unwrap();
    assert!(load(&dir.0, kr.clone(), T0).unreadable, "a deleted guard file must not load as defaults");
    // A fresh install has neither file nor anchor and loads normally.
    let fresh = TmpDir::new("guard-fresh");
    assert!(!load(&fresh.0, Arc::new(MemKeyring::default()), T0).unreadable);
}

#[test]
fn a_crash_between_the_guard_write_and_the_anchor_update_is_not_a_rollback() {
    let dir = TmpDir::new("guard-ahead");
    let kr = Arc::new(MemKeyring::default());
    let mut g = load(&dir.0, kr.clone(), T0);
    save(&dir.0, &mut g).unwrap();
    let anchor = kr.map.lock().unwrap().get(crate::provider_usage_log::ANCHOR_ACCOUNT).cloned().unwrap_or_default();
    g.member_halt("openai", T0);
    save(&dir.0, &mut g).unwrap();
    kr.map
        .lock()
        .unwrap()
        .insert(crate::provider_usage_log::ANCHOR_ACCOUNT.to_string(), anchor);
    let g2 = load(&dir.0, kr.clone(), T0);
    assert!(!g2.unreadable);
    assert!(g2.get("openai").halt.is_some());
}

#[test]
fn a_guard_file_with_no_anchor_fails_closed() {
    let dir = TmpDir::new("guard-noanchor");
    let kr = Arc::new(MemKeyring::default());
    let mut g = load(&dir.0, kr.clone(), T0);
    g.member_halt("openai", T0);
    save(&dir.0, &mut g).unwrap();
    kr.map.lock().unwrap().remove(crate::provider_usage_log::ANCHOR_ACCOUNT);
    assert!(load(&dir.0, kr.clone(), T0).unreadable, "a saved guard file needs its anchor");
}

#[test]
fn default_currency_caps_are_per_vendor_with_xai_overridden() {
    // Owner decision amending P-7a: xAI $20/day and $50/month; every other provider $5 / $50;
    // token caps 1M / 10M for all.
    let xai = Caps::default_for("xai");
    assert_eq!(xai.daily_micros, Some(20_000_000));
    assert_eq!(xai.monthly_micros, Some(50_000_000));
    assert_eq!((xai.daily_tokens, xai.monthly_tokens), (Some(1_000_000), Some(10_000_000)));
    for p in ["openai", "anthropic", "gemini", "deepseek", "meta", "custom-box"] {
        assert_eq!(Caps::default_for(p), Caps::default(), "{p}");
    }
    assert_eq!(Caps::default().daily_micros, Some(5_000_000));
    assert!(xai.validate().is_ok());
    // A provider never configured gets its own default, in the guard and in what it reports.
    let mut g = GuardBook::default();
    assert_eq!(g.get("xai").caps, xai);
    assert_eq!(g.get("openai").caps, Caps::default());
    // $6 call (an xAI full-context reservation) fits xAI's $20 day, not the generic $5.
    let mut p = plan("gpt-known", 1, 1);
    p.provider = "xai".into();
    p.price = Some(Price { input_micros_per_mtok: 3_000_000_000_000, output_micros_per_mtok: 3_000_000_000_000 });
    assert_eq!(p.reserve_micros(), Some(6_000_000));
    assert!(g.admit(&p, &known(), used(0, 0), used(0, 0), T0).is_ok());
    p.provider = "openai".into();
    let r = g.admit(&p, &known(), used(0, 0), used(0, 0), T0);
    assert!(matches!(r, Err(Refusal::CapExceeded { which: "daily spend", .. })), "{r:?}");
    // And the member's own caps still win once set.
    g.member_halt("xai", T0);
    g.member_resume("xai").unwrap();
    assert_eq!(g.get("xai").caps, xai, "an entry created by a halt keeps the vendor default");
}
