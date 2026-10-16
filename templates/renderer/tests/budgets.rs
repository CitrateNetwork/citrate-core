//! HUP-S6.9: per-tier Medusa call/coverage budgets are data
//! (`templates/medusa-budgets.json`), and this suite pins their shape.

use std::path::PathBuf;

use citrate_templates::budget::{MedusaBudgets, Tier};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn shipped() -> MedusaBudgets {
    match MedusaBudgets::load(&root()) {
        Ok(b) => b,
        Err(e) => panic!("shipped budgets failed to load: {e}"),
    }
}

#[test]
fn tier_ids_parse_strictly() {
    assert_eq!(Tier::parse("T0"), Some(Tier::T0));
    assert_eq!(Tier::parse("T1"), Some(Tier::T1));
    assert_eq!(Tier::parse("T2"), Some(Tier::T2));
    for bad in ["t1", "T3", " T1", "T1 ", "", "T01", "tier1"] {
        assert_eq!(Tier::parse(bad), None, "accepted {bad:?}");
    }
    assert_eq!(Tier::T1.id(), "T1");
}

#[test]
fn t1_matches_the_planset_50k_call_budget() {
    // 00_OVERVIEW red-team correction #10: "e.g. 50k calls against template invariants".
    assert_eq!(shipped().for_tier(Tier::T1).test_limit, 50_000);
}

#[test]
fn budgets_grow_with_the_tier_and_t0_is_lower() {
    let b = shipped();
    let (t0, t1, t2) = (
        b.for_tier(Tier::T0),
        b.for_tier(Tier::T1),
        b.for_tier(Tier::T2),
    );
    assert!(t0.test_limit < t1.test_limit && t1.test_limit < t2.test_limit);
    assert!(t0.workers <= t1.workers && t1.workers <= t2.workers);
    assert!(t0.coverage_plateau_calls < t1.coverage_plateau_calls);
    assert!(t1.coverage_plateau_calls < t2.coverage_plateau_calls);
    assert!(t0.min_coverage_pct < t1.min_coverage_pct);
    assert!(t1.min_coverage_pct <= t2.min_coverage_pct);
}

#[test]
fn every_budget_is_bounded_and_non_zero() {
    let b = shipped();
    for tier in [Tier::T0, Tier::T1, Tier::T2] {
        let t = b.for_tier(tier);
        assert!(t.test_limit > 0 && t.test_limit <= 10_000_000, "{tier:?}");
        assert!(t.workers >= 1 && t.workers <= 64, "{tier:?}");
        assert!(
            t.call_sequence_length >= 1 && t.call_sequence_length <= 1_000,
            "{tier:?}"
        );
        // A wall-clock ceiling only; the budget itself is counted in calls.
        assert!(t.timeout_secs > 0 && t.timeout_secs <= 3_600, "{tier:?}");
        assert!(
            t.coverage_plateau_calls > 0 && t.coverage_plateau_calls < t.test_limit,
            "{tier:?}"
        );
        assert!(
            t.min_coverage_pct >= 1 && t.min_coverage_pct <= 100,
            "{tier:?}"
        );
    }
}

#[test]
fn malformed_budget_files_are_rejected() {
    let good = r#"{"schema":1,"tiers":{
        "T0":{"test_limit":10,"workers":1,"call_sequence_length":5,"timeout_secs":60,"coverage_plateau_calls":5,"min_coverage_pct":50},
        "T1":{"test_limit":20,"workers":1,"call_sequence_length":5,"timeout_secs":60,"coverage_plateau_calls":6,"min_coverage_pct":60},
        "T2":{"test_limit":30,"workers":2,"call_sequence_length":5,"timeout_secs":60,"coverage_plateau_calls":7,"min_coverage_pct":70}}}"#;
    assert!(MedusaBudgets::from_json(good).is_ok());

    // Missing tier.
    let missing = good.replacen(r#""T2":"#, r#""T9":"#, 1);
    assert!(MedusaBudgets::from_json(&missing).is_err());
    // Unknown field.
    let extra = good.replacen(r#""workers":1,"#, r#""workers":1,"surprise":1,"#, 1);
    assert!(MedusaBudgets::from_json(&extra).is_err());
    // Zero call budget.
    let zero = good.replacen(r#""test_limit":10"#, r#""test_limit":0"#, 1);
    assert!(MedusaBudgets::from_json(&zero).is_err());
    // Coverage over 100%.
    let pct = good.replacen(r#""min_coverage_pct":50"#, r#""min_coverage_pct":101"#, 1);
    assert!(MedusaBudgets::from_json(&pct).is_err());
    // A larger tier with a smaller call budget than the one below it.
    let inverted = good.replacen(r#""test_limit":30"#, r#""test_limit":15"#, 1);
    assert!(MedusaBudgets::from_json(&inverted).is_err());
    // Unknown schema version.
    let schema = good.replacen(r#""schema":1"#, r#""schema":2"#, 1);
    assert!(MedusaBudgets::from_json(&schema).is_err());
}
