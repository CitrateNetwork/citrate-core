//! HUP-S6.9: per-tier Medusa budgets, loaded from `templates/medusa-budgets.json`.
//!
//! The budget is counted in fuzzer calls, not wall-clock minutes (00_OVERVIEW
//! red-team correction #10). `test_limit`, `workers`, `call_sequence_length` and
//! `timeout_secs` are written into each rendered `medusa.json` and enforced by
//! Medusa itself. `coverage_plateau_calls` (stop early when coverage has not
//! grown for that many calls) and `min_coverage_pct` (the bar a run must reach to
//! count toward a READY verdict) are NOT Medusa options: they are recorded in the
//! rendered `citrate-template.lock.json` for the toolchain runner (HUP-S6.3) and
//! the deploy gate (HUP-S6.4) to enforce.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The budgets file inside a template root.
pub const BUDGETS_FILE: &str = "medusa-budgets.json";

/// Hardware tier. Mirrors `src-tauri/src/tier.rs` (ids `T0`/`T1`/`T2`); kept local
/// so this crate has no Tauri dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    T0,
    T1,
    T2,
}

impl Tier {
    /// The tier id, exactly `T0`, `T1` or `T2`.
    pub fn id(self) -> &'static str {
        match self {
            Tier::T0 => "T0",
            Tier::T1 => "T1",
            Tier::T2 => "T2",
        }
    }

    /// Strict parse: only the exact ids are accepted.
    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "T0" => Some(Tier::T0),
            "T1" => Some(Tier::T1),
            "T2" => Some(Tier::T2),
            _ => None,
        }
    }
}

/// One tier's Medusa budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MedusaBudget {
    /// Total fuzzer calls (Medusa `fuzzing.testLimit`).
    pub test_limit: u64,
    /// Parallel workers (Medusa `fuzzing.workers`).
    pub workers: u32,
    /// Calls per sequence (Medusa `fuzzing.callSequenceLength`).
    pub call_sequence_length: u32,
    /// Wall-clock safety ceiling in seconds (Medusa `fuzzing.timeout`).
    pub timeout_secs: u64,
    /// Runner-enforced: stop when coverage has not grown for this many calls.
    pub coverage_plateau_calls: u64,
    /// Runner-enforced: minimum line coverage of the template sources, percent.
    pub min_coverage_pct: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TierTable {
    #[serde(rename = "T0")]
    t0: MedusaBudget,
    #[serde(rename = "T1")]
    t1: MedusaBudget,
    #[serde(rename = "T2")]
    t2: MedusaBudget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetsFile {
    schema: u32,
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
    tiers: TierTable,
}

/// All three tiers' budgets, validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MedusaBudgets {
    t0: MedusaBudget,
    t1: MedusaBudget,
    t2: MedusaBudget,
}

impl MedusaBudgets {
    /// Load and validate `<root>/medusa-budgets.json`.
    pub fn load(root: &Path) -> Result<MedusaBudgets, String> {
        let path = root.join(BUDGETS_FILE);
        let raw =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        MedusaBudgets::from_json(&raw)
    }

    /// Parse and validate a budgets document.
    pub fn from_json(raw: &str) -> Result<MedusaBudgets, String> {
        let file: BudgetsFile =
            serde_json::from_str(raw).map_err(|e| format!("medusa budgets: {e}"))?;
        if file.schema != 1 {
            return Err(format!(
                "medusa budgets: unsupported schema {}",
                file.schema
            ));
        }
        let b = MedusaBudgets {
            t0: file.tiers.t0,
            t1: file.tiers.t1,
            t2: file.tiers.t2,
        };
        for tier in [Tier::T0, Tier::T1, Tier::T2] {
            check_one(tier, b.for_tier(tier))?;
        }
        if !(b.t0.test_limit <= b.t1.test_limit && b.t1.test_limit <= b.t2.test_limit) {
            return Err("medusa budgets: test_limit must not shrink from T0 to T2".into());
        }
        if !(b.t0.workers <= b.t1.workers && b.t1.workers <= b.t2.workers) {
            return Err("medusa budgets: workers must not shrink from T0 to T2".into());
        }
        Ok(b)
    }

    /// The budget for one tier.
    pub fn for_tier(&self, tier: Tier) -> &MedusaBudget {
        match tier {
            Tier::T0 => &self.t0,
            Tier::T1 => &self.t1,
            Tier::T2 => &self.t2,
        }
    }
}

fn check_one(tier: Tier, b: &MedusaBudget) -> Result<(), String> {
    let id = tier.id();
    if b.test_limit == 0 || b.test_limit > 10_000_000 {
        return Err(format!(
            "medusa budgets {id}: test_limit must be 1..=10000000"
        ));
    }
    if b.workers == 0 || b.workers > 64 {
        return Err(format!("medusa budgets {id}: workers must be 1..=64"));
    }
    if b.call_sequence_length == 0 || b.call_sequence_length > 1_000 {
        return Err(format!(
            "medusa budgets {id}: call_sequence_length must be 1..=1000"
        ));
    }
    if b.timeout_secs == 0 || b.timeout_secs > 3_600 {
        return Err(format!(
            "medusa budgets {id}: timeout_secs must be 1..=3600"
        ));
    }
    if b.coverage_plateau_calls == 0 || b.coverage_plateau_calls >= b.test_limit {
        return Err(format!(
            "medusa budgets {id}: coverage_plateau_calls must be 1..test_limit"
        ));
    }
    if b.min_coverage_pct == 0 || b.min_coverage_pct > 100 {
        return Err(format!(
            "medusa budgets {id}: min_coverage_pct must be 1..=100"
        ));
    }
    Ok(())
}
