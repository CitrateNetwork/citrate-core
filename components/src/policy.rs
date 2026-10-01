//! The CVE SLA, client side (policy text: `docs/COMPONENT_UPDATER.md`).
//!
//! The publisher side of the SLA is a promise: a signed manifest with the fix within the
//! deadline for the advisory's severity. The client side is enforceable: the app knows when it
//! last verified a manifest, so it can warn when updates are stale and keep the managed
//! browser off the open web when the manifest has expired (a browser that has missed security
//! updates must not browse untrusted pages).
//!
//! **All numbers here are placeholders pending owner sign-off.**
use crate::manifest::{SeenManifest, CLOCK_SKEW_SECS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlaPolicy {
    /// Publisher deadline from advisory to signed manifest, per severity (seconds).
    pub critical_secs: u64,
    pub high_secs: u64,
    pub medium_secs: u64,
    pub low_secs: u64,
    /// After this long without a verified manifest the app shows "updates are stale".
    pub stale_after_secs: u64,
    /// True until the owner signs off on the values.
    pub pending_owner_signoff: bool,
}

const HOUR: u64 = 3_600;
const DAY: u64 = 86_400;

impl Default for SlaPolicy {
    fn default() -> Self {
        Self {
            critical_secs: 72 * HOUR,
            high_secs: 7 * DAY,
            medium_secs: 30 * DAY,
            low_secs: 90 * DAY,
            stale_after_secs: 3 * DAY,
            pending_owner_signoff: true,
        }
    }
}

impl SlaPolicy {
    pub fn deadline_secs(&self, s: Severity) -> u64 {
        match s {
            Severity::Critical => self.critical_secs,
            Severity::High => self.high_secs,
            Severity::Medium => self.medium_secs,
            Severity::Low => self.low_secs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// No manifest was ever verified on this machine.
    NeverChecked,
    Fresh {
        age_secs: u64,
    },
    /// Older than `stale_after_secs`, or the clock is behind the last check.
    Stale {
        age_secs: u64,
    },
    /// The last verified manifest has expired.
    Expired,
}

/// How current this machine's component updates are, under the default policy.
pub fn freshness(seen: Option<&SeenManifest>, now: u64) -> Freshness {
    freshness_with(&SlaPolicy::default(), seen, now)
}

pub fn freshness_with(p: &SlaPolicy, seen: Option<&SeenManifest>, now: u64) -> Freshness {
    let Some(s) = seen else {
        return Freshness::NeverChecked;
    };
    if now >= s.expires_at {
        return Freshness::Expired;
    }
    if now.saturating_add(CLOCK_SKEW_SECS) < s.verified_at {
        // The clock went backwards past the last check: do not count it as fresh.
        return Freshness::Stale { age_secs: 0 };
    }
    let age = now.saturating_sub(s.verified_at);
    if age > p.stale_after_secs {
        Freshness::Stale { age_secs: age }
    } else {
        Freshness::Fresh { age_secs: age }
    }
}

/// Whether the managed browser may load untrusted web pages. Stale is a warning; never
/// checked or expired is a block.
pub fn browser_may_open_web(f: &Freshness) -> bool {
    matches!(f, Freshness::Fresh { .. } | Freshness::Stale { .. })
}
