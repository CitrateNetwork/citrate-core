//! HUP-S5.5 CVE SLA: the client side of the policy. A component manifest that has not been
//! refreshed is "stale" and then "expired"; the managed browser may open the open web only
//! while the manifest is fresh or inside the grace window.
mod common;

use citrate_components::manifest::SeenManifest;
use citrate_components::policy::{browser_may_open_web, freshness, Freshness, Severity, SlaPolicy};
use common::{DAY, NOW};

fn seen(verified_at: u64, expires_at: u64) -> SeenManifest {
    SeenManifest {
        sequence: 1,
        digest_hex: "00".repeat(32),
        verified_at,
        expires_at,
    }
}

#[test]
fn never_checked_blocks_the_open_web() {
    let f = freshness(None, NOW);
    assert_eq!(f, Freshness::NeverChecked);
    assert!(!browser_may_open_web(&f));
}

#[test]
fn fresh_stale_expired() {
    let p = SlaPolicy::default();
    let s = seen(NOW, NOW + 14 * DAY);
    assert!(matches!(
        freshness(Some(&s), NOW + DAY),
        Freshness::Fresh { .. }
    ));
    let stale = freshness(Some(&s), NOW + p.stale_after_secs + 1);
    assert!(matches!(stale, Freshness::Stale { .. }), "{stale:?}");
    assert!(
        browser_may_open_web(&stale),
        "stale is a warning, not a block"
    );
    let exp = freshness(Some(&s), NOW + 14 * DAY);
    assert_eq!(exp, Freshness::Expired);
    assert!(!browser_may_open_web(&exp));
}

#[test]
fn a_clock_before_the_check_is_not_fresh_forever() {
    // A clock set back before the last check must not count as "fresh".
    let s = seen(NOW, NOW + 14 * DAY);
    let f = freshness(Some(&s), NOW - 30 * DAY);
    assert!(matches!(f, Freshness::Stale { .. }), "{f:?}");
}

#[test]
fn the_sla_table_is_ordered_and_placeholder_values_are_marked() {
    let p = SlaPolicy::default();
    let d = |s| p.deadline_secs(s);
    assert!(d(Severity::Critical) < d(Severity::High));
    assert!(d(Severity::High) < d(Severity::Medium));
    assert!(d(Severity::Medium) < d(Severity::Low));
    assert!(
        p.pending_owner_signoff,
        "values stay marked until the owner signs them off"
    );
    // The browser must not be allowed to outlive the critical deadline on a stale manifest.
    assert!(p.stale_after_secs <= d(Severity::Critical) * 2);
}
