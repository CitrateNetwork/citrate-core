//! citrate-core 0.5.4 — **the HIC-2 envelope evaluator, in its own `fl_envelope` scope** (Hermes ×
//! FL WP-H4; gap analysis §4 V7 and V10).
//!
//! The member's envelope and worker terms are sealed by [`crate::fl_work`] (FL design board 11).
//! This module decides, in core, whether one join would fall inside that envelope or must ask the
//! member (HIC-1). Hermes and the webview never decide it, and the prompt plays no part.
//!
//! **A separate scope (V7).** This is not the SIWE sign-in budget (`web_budgets.rs`,
//! ADR-2026-09-30): it reads none of that budget's state, shares none of its caps and is never
//! reachable from the sign-in path. A test scans this file to keep it that way.
//!
//! **The rules, every one enforced here:**
//! 1. No envelope, or an envelope that is off: ask.
//! 2. The first join with a new poster, or with a new group, always asks.
//! 3. The offer's price per unit must be at least the floor in the member's terms (no terms: ask).
//! 4. Hours: the lease must fit the envelope's machine hours per round, and the member's daily
//!    hours window when the terms set one.
//! 5. Weekly caps: joins this week (rolled forward to now) must be under joins per week.
//!
//! Every failed rule is reported, not only the first, so the card can name them all.
//!
//! **Offer text can never change the envelope.** A join reaches the evaluator only as
//! [`JoinFacts`]: the offer id, the poster, the group hash, the price per unit and the lease hours,
//! each a typed value read from WorkMarket's `getOffer` and the hash-checked requirements file.
//! No string from an offer's title, description or job spec has a field to arrive in, and the
//! command boundary refuses one (`deny_unknown_fields`).
//!
//! **It authorizes nothing on chain until WorkMarket is pinned.** With no WorkMarket in this
//! build's address book (r1007 today), [`check`] answers [`Decision::Unavailable`] before it
//! evaluates anything. Even once WorkMarket is pinned, an inside-the-envelope verdict signs nothing:
//! a join is signed only through the SignatureCeremony (CLAUDE.md Rule 3), and an envelope-signed
//! join would need its own entry on the Rule 3 closed list. Join history: core has no record of
//! a join made under HIC-1 on this network (none can exist without WorkMarket), so every poster and
//! group counts as new and every join asks (fail closed).
//!
//! Rule 7: the data sources are `<app data>/fl-work/` (via [`crate::fl_work::WorkStore`], the work
//! key in the OS keyring) and the embedded address book ([`crate::addresses::work_market`]).

use serde::{Deserialize, Serialize};

use crate::fl_work::{Envelope, Hours, WorkerTerms};

/// The authorization scope this evaluator speaks for. Never the SIWE budget's.
pub const SCOPE: &str = "fl_envelope";

const MS_PER_WEEK: u64 = 7 * 86_400_000;
const SALT_DECIMALS: usize = 18;

/// Said when WorkMarket is not in this build's address book.
pub const NOT_DEPLOYED: &str =
    "WorkMarket is not deployed on this network yet, so the envelope authorizes nothing.";

/// The fixed rules, as the member reads them.
pub const RULES: [&str; 6] = [
    "The first join with a new group or poster always asks you.",
    "An offer below the floor in your terms always asks you.",
    "A lease longer than the envelope's machine hours per round, or than your daily hours, asks you.",
    "Once this week's joins are used, every join asks you until the week rolls over.",
    "An offer's text can never change the envelope.",
    "Opening a round, publishing a dataset, offboarding and disputes always ask you; they are never in an envelope.",
];

/// One join, as core reads it from WorkMarket. Typed values only: no offer text.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinFacts {
    /// WorkMarket offer id (decimal).
    pub offer_id: String,
    /// The offer's poster (`0x` + 40 hex).
    pub poster: String,
    /// The offer's `groupHash` (`0x` + 64 hex; all zero for an open offer).
    pub group_hash: String,
    /// The offer's `pricePerUnit` in wei of SALT (decimal).
    pub price_per_unit_wei: String,
    /// The lease's machine hours, from the hash-checked requirements file.
    pub lease_hours: u32,
}

/// A poster and group this device already joined under HIC-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counterparty {
    pub poster: String,
    pub group_hash: String,
}

/// Why a join must ask the member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AskReason {
    #[serde(rename_all = "camelCase")]
    BadFacts {
        problem: String,
    },
    NoEnvelope,
    EnvelopeOff,
    FirstJoinWithPoster,
    FirstJoinWithGroup,
    NoTerms,
    #[serde(rename_all = "camelCase")]
    BelowFloor {
        floor_wei: String,
        price_wei: String,
    },
    #[serde(rename_all = "camelCase")]
    OverHoursPerRound {
        max_hours: u32,
        lease_hours: u32,
    },
    #[serde(rename_all = "camelCase")]
    OverDailyHours {
        window_hours: u32,
        lease_hours: u32,
    },
    #[serde(rename_all = "camelCase")]
    WeeklyCapReached {
        joins_per_week: u32,
    },
}

impl AskReason {
    /// The member's words for this reason.
    pub fn words(&self) -> String {
        match self {
            AskReason::BadFacts { problem } => format!("The offer could not be read: {problem}."),
            AskReason::NoEnvelope => "You have not approved an envelope.".into(),
            AskReason::EnvelopeOff => "Your envelope is off.".into(),
            AskReason::FirstJoinWithPoster => "This is your first join with this poster.".into(),
            AskReason::FirstJoinWithGroup => "This is your first join with this group.".into(),
            AskReason::NoTerms => "You have not saved worker terms, so there is no floor.".into(),
            AskReason::BelowFloor { .. } => "The offer pays less than your floor.".into(),
            AskReason::OverHoursPerRound {
                max_hours,
                lease_hours,
            } => format!(
                "The lease is {lease_hours} machine hours; the envelope allows up to {max_hours}."
            ),
            AskReason::OverDailyHours {
                window_hours,
                lease_hours,
            } => format!(
                "The lease is {lease_hours} machine hours; your daily hours are {window_hours}."
            ),
            AskReason::WeeklyCapReached { joins_per_week } => {
                format!("This week's {joins_per_week} joins are used.")
            }
        }
    }
}

/// What core decided for one join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "camelCase")]
pub enum Decision {
    /// Nothing can be authorized here (WorkMarket is not pinned).
    #[serde(rename_all = "camelCase")]
    Unavailable { reason: String },
    /// The member must approve this join (HIC-1).
    #[serde(rename_all = "camelCase")]
    Ask {
        reasons: Vec<AskReason>,
        words: Vec<String>,
    },
    /// Inside the envelope. Signs nothing by itself.
    #[serde(rename_all = "camelCase")]
    Within { joins_left_after: u32 },
}

fn hex_of_len(s: &str, n: usize) -> bool {
    s.len() == 2 + n && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn decimal_u128(s: &str) -> Option<u128> {
    if s.is_empty() || s.len() > 39 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// A decimal SALT amount (as `fl_work` validates it, e.g. "1.20") in wei.
pub fn salt_to_wei(s: &str) -> Option<u128> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty() || frac.len() > SALT_DECIMALS || (s.contains('.') && frac.is_empty()) {
        return None;
    }
    let digits = format!("{whole}{frac:0<width$}", width = SALT_DECIMALS);
    decimal_u128(&digits)
}

impl JoinFacts {
    fn problem(&self) -> Option<String> {
        if decimal_u128(&self.offer_id).is_none() {
            return Some("the offer id is not a number".into());
        }
        if !hex_of_len(&self.poster, 40) {
            return Some("the poster is not an address".into());
        }
        if !hex_of_len(&self.group_hash, 64) {
            return Some("the group hash is not 32 bytes".into());
        }
        if decimal_u128(&self.price_per_unit_wei).is_none() {
            return Some("the price is not a whole number of wei".into());
        }
        if self.lease_hours == 0 {
            return Some("the lease has no hours".into());
        }
        None
    }

    fn open_offer(&self) -> bool {
        self.group_hash[2..].bytes().all(|b| b == b'0')
    }
}

fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.split_once(':')?;
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

/// The daily window's length in whole hours (an overnight window wraps; equal ends is all day).
fn window_hours(hours: &Hours) -> Option<u32> {
    match hours {
        Hours::Any(_) => None,
        Hours::Window { start, end } => {
            let (s, e) = (minutes(start)?, minutes(end)?);
            let len = if e > s { e - s } else { e + 24 * 60 - s };
            Some(len / 60)
        }
    }
}

/// This week's joins, rolled forward to `now` the same way the sealed store rolls them.
fn joins_used(env: &Envelope, now: u64) -> u32 {
    if now >= env.week_start.saturating_add(MS_PER_WEEK) {
        0
    } else {
        env.joins_this_week
    }
}

/// Joins left this week under an envelope that is on.
pub fn joins_left(env: &Envelope, now: u64) -> u32 {
    env.joins_per_week.saturating_sub(joins_used(env, now))
}

/// **The evaluator.** Pure: the envelope and terms as sealed, the join's typed facts, the
/// counterparties already joined under HIC-1, and the time. Collects every reason to ask.
pub fn evaluate(
    envelope: Option<&Envelope>,
    terms: Option<&WorkerTerms>,
    facts: &JoinFacts,
    joined: &[Counterparty],
    now: u64,
) -> Decision {
    let mut reasons = Vec::new();
    if let Some(problem) = facts.problem() {
        reasons.push(AskReason::BadFacts { problem });
        return ask(reasons);
    }
    let env = match envelope {
        None => {
            reasons.push(AskReason::NoEnvelope);
            None
        }
        Some(e) if !e.on => {
            reasons.push(AskReason::EnvelopeOff);
            None
        }
        Some(e) => Some(e),
    };
    let poster = facts.poster.to_ascii_lowercase();
    let group = facts.group_hash.to_ascii_lowercase();
    if !joined
        .iter()
        .any(|c| c.poster.eq_ignore_ascii_case(&poster))
    {
        reasons.push(AskReason::FirstJoinWithPoster);
    }
    if !facts.open_offer()
        && !joined
            .iter()
            .any(|c| c.group_hash.eq_ignore_ascii_case(&group))
    {
        reasons.push(AskReason::FirstJoinWithGroup);
    }
    let price = decimal_u128(&facts.price_per_unit_wei).unwrap_or(0);
    match terms {
        None => reasons.push(AskReason::NoTerms),
        Some(t) => {
            match salt_to_wei(&t.floor_salt_per_unit) {
                Some(floor) if price >= floor => {}
                Some(floor) => reasons.push(AskReason::BelowFloor {
                    floor_wei: floor.to_string(),
                    price_wei: price.to_string(),
                }),
                // Terms the store accepted always parse; anything else has no usable floor.
                None => reasons.push(AskReason::NoTerms),
            }
            if let Some(w) = window_hours(&t.hours) {
                if facts.lease_hours > w {
                    reasons.push(AskReason::OverDailyHours {
                        window_hours: w,
                        lease_hours: facts.lease_hours,
                    });
                }
            }
        }
    }
    if let Some(e) = env {
        if facts.lease_hours > e.max_hours_per_round {
            reasons.push(AskReason::OverHoursPerRound {
                max_hours: e.max_hours_per_round,
                lease_hours: facts.lease_hours,
            });
        }
        if joins_left(e, now) == 0 {
            reasons.push(AskReason::WeeklyCapReached {
                joins_per_week: e.joins_per_week,
            });
        }
        if reasons.is_empty() {
            return Decision::Within {
                joins_left_after: joins_left(e, now) - 1,
            };
        }
    }
    ask(reasons)
}

fn ask(reasons: Vec<AskReason>) -> Decision {
    let words = reasons.iter().map(AskReason::words).collect();
    Decision::Ask { reasons, words }
}

/// The gate in front of the evaluator: nothing is evaluated while WorkMarket is not pinned.
pub fn check(
    work_market: Option<&str>,
    envelope: Option<&Envelope>,
    terms: Option<&WorkerTerms>,
    facts: &JoinFacts,
    joined: &[Counterparty],
    now: u64,
) -> Decision {
    if work_market.is_none() {
        return Decision::Unavailable {
            reason: NOT_DEPLOYED.to_string(),
        };
    }
    evaluate(envelope, terms, facts, joined, now)
}

/// What `fl_envelope_status` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeStatus {
    pub scope: &'static str,
    /// The envelope as sealed, its week rolled forward to now; null when none was approved.
    pub envelope: Option<Envelope>,
    /// Joins left this week while it is on; null when it is off or absent.
    pub joins_left: Option<u32>,
    /// Whether WorkMarket is in this build's address book.
    pub work_market_pinned: bool,
    /// Whether the envelope can authorize anything here: never in this build (see the module note).
    pub authorizes: bool,
    pub note: String,
    pub rules: Vec<&'static str>,
}

/// The status, from the sealed envelope and the address book.
pub fn status(envelope: Option<Envelope>, work_market: Option<&str>, now: u64) -> EnvelopeStatus {
    let on = envelope.as_ref().is_some_and(|e| e.on);
    let joins_left = envelope
        .as_ref()
        .filter(|e| e.on)
        .map(|e| joins_left(e, now));
    let note = match (&envelope, work_market) {
        (_, None) => NOT_DEPLOYED.to_string(),
        (None, Some(_)) => "You have not approved an envelope, so every join asks you.".into(),
        (Some(_), Some(_)) if !on => "Your envelope is off, so every join asks you.".into(),
        (Some(_), Some(_)) => "This device has no record of a join it made with your approval, so every poster and group is new and every join asks you.".into(),
    };
    EnvelopeStatus {
        scope: SCOPE,
        envelope,
        joins_left,
        work_market_pinned: work_market.is_some(),
        authorizes: false,
        note,
        rules: RULES.to_vec(),
    }
}

// ------------------------------------------------------------------------------ commands

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn work_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(crate::fl_work::WORK_DIR))
}

/// **fl_envelope_status** — the envelope, what is left of it, whether it can authorize anything on
/// this network, and the fixed rules. Data sources: `<app data>/fl-work/envelope.sealed` (work key
/// in the OS keyring) and the embedded address book. Read-only.
#[tauri::command]
pub async fn fl_envelope_status(app_h: tauri::AppHandle) -> Result<EnvelopeStatus, String> {
    crate::blocking::off_main(move || {
        let keyring = crate::custody::OsKeyring::legacy();
        let store = crate::fl_work::WorkStore::new(work_dir(&app_h)?, &keyring);
        let now = now_ms();
        Ok(status(
            store.envelope(now)?,
            crate::addresses::work_market(),
            now,
        ))
    })
    .await
}

/// **fl_envelope_check** — core's decision for one join's typed facts: unavailable (WorkMarket is
/// not pinned), ask (with every reason), or within the envelope (which signs nothing). Offer text
/// has no field here. Read-only.
#[tauri::command]
pub async fn fl_envelope_check(
    app_h: tauri::AppHandle,
    facts: JoinFacts,
) -> Result<Decision, String> {
    crate::blocking::off_main(move || {
        let work_market = crate::addresses::work_market();
        if work_market.is_none() {
            return Ok(check(None, None, None, &facts, &[], now_ms()));
        }
        let keyring = crate::custody::OsKeyring::legacy();
        let store = crate::fl_work::WorkStore::new(work_dir(&app_h)?, &keyring);
        let now = now_ms();
        let envelope = store.envelope(now)?;
        let terms = store.terms()?;
        // No HIC-1 join can be recorded on a network without WorkMarket; see the module note.
        Ok(check(
            work_market,
            envelope.as_ref(),
            terms.as_ref(),
            &facts,
            &[],
            now,
        ))
    })
    .await
}

#[cfg(test)]
#[path = "fl_envelope_tests.rs"]
mod tests;
