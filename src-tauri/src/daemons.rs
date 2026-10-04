//! citrate-core — HUP-S10.3 daemons: recurring Hermes tasks on a local schedule, each inside a
//! budget (planset 02_ARCHITECTURE §1 "Daemon", §4 HIC-3; US-10.3 AC2).
//!
//! A daemon is a member-written prompt plus a 5-field cron schedule in LOCAL time. This module is
//! the source of truth for what may run and when: it decides which daemons are due, enforces each
//! daemon's budget (runs per day, tokens per day, tokens per run, spend), keeps the ledger, and
//! persists everything to `daemons.json` in the app's local data dir. The main window's runner
//! (`src/daemons/runner.ts`) asks [`daemons_claim_due`] every 30 s, runs each claimed turn on the
//! LOCAL model through the same gated tool path as chat, and reports back with [`daemons_finish_run`].
//!
//! Rules kept here:
//! - **Nothing runs by default.** There are no daemons until the member creates one.
//! - **Spend is zero.** `max_spend_salt` must be exactly "0": no daemon has a spend path in this
//!   release (pending owner sign-off on any non-zero spend budget).
//! - **Budget first.** A due daemon whose day budget is used up is skipped and the skip is
//!   recorded; it never runs "just this once". A run is charged the tokens it reports, and a run the
//!   app never reports back is released after [`STALE_RUN_MS`] and charged its full allowance.
//! - **One run in flight per daemon**, and missed minutes while the app was closed catch up at most
//!   once (only minutes in the last 24 h count).
//! - **Paused means paused.** A paused daemon (or "pause all") never fires, and resuming does not
//!   replay the minutes it missed.
//! - **Keyless (Rule 3).** Nothing here signs or holds a key. Any change a daemon's turn proposes
//!   goes to the member as an explicit approval (the runner marks every effectful call HIC-required,
//!   and the sidecar opens daemon sessions as `unattended`).
//!
//! The budget default values below are conservative placeholders, PENDING OWNER SIGN-OFF
//! (HUP-S10.5 "default budget values").

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// The commands this module registers (kept in step with lib.rs and main-window.toml by a test).
#[cfg(test)]
pub(crate) const COMMANDS: [&str; 8] = [
    "daemons_list",
    "daemon_save",
    "daemon_set_paused",
    "daemons_set_all_paused",
    "daemon_delete",
    "daemons_claim_due",
    "daemons_finish_run",
    "daemon_runs_between",
];

/// The file the book lives in, under the app's local data dir.
pub(crate) const FILE_NAME: &str = "daemons.json";
/// File format version.
const FORMAT_VERSION: u32 = 1;

/// At most this many daemons.
pub(crate) const MAX_DAEMONS: usize = 16;
/// Longest daemon name, in characters.
pub(crate) const NAME_MAX: usize = 80;
/// Longest daemon prompt, in characters.
pub(crate) const PROMPT_MAX: usize = 4000;
/// Longest kept "last run" note, in characters.
pub(crate) const NOTE_MAX: usize = 240;

/// Default runs per local day. PENDING OWNER SIGN-OFF (placeholder, deliberately low).
pub(crate) const DEFAULT_MAX_RUNS_PER_DAY: u32 = 4;
/// Default tokens per local day. PENDING OWNER SIGN-OFF (placeholder, deliberately low).
pub(crate) const DEFAULT_MAX_TOKENS_PER_DAY: u32 = 20_000;
/// Default tokens per run. PENDING OWNER SIGN-OFF (placeholder, deliberately low).
pub(crate) const DEFAULT_MAX_TOKENS_PER_RUN: u32 = 6_000;
/// Hard ceiling on runs per day, whatever the member sets (one every 30 minutes).
pub(crate) const HARD_MAX_RUNS_PER_DAY: u32 = 48;
/// Hard ceiling on tokens per day, whatever the member sets.
pub(crate) const HARD_MAX_TOKENS_PER_DAY: u32 = 200_000;
/// A run the app has not reported back after this long is released and charged its allowance.
pub(crate) const STALE_RUN_MS: u64 = 30 * 60_000;
/// Only matching minutes this recent are caught up after the app was closed.
const CATCH_UP_WINDOW_MS: u64 = 24 * 60 * 60_000;

const MINUTE_MS: u64 = 60_000;
const DAY_MINUTES: i64 = 24 * 60;

// ---------------------------------------------------------------------------
// Local civil time (no time-zone database: the webview passes its own offset)
// ---------------------------------------------------------------------------

/// A local wall-clock minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 = Sunday … 6 = Saturday (cron numbering).
    pub weekday: u32,
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl LocalTime {
    /// The local minute containing `utc_ms`, at `offset_min` minutes east of UTC.
    pub(crate) fn from_utc_ms(utc_ms: u64, offset_min: i32) -> Self {
        Self::from_local_minutes(local_minutes(utc_ms, offset_min))
    }

    fn from_local_minutes(total: i64) -> Self {
        let days = total.div_euclid(DAY_MINUTES);
        let rem = total.rem_euclid(DAY_MINUTES);
        let (year, month, day) = civil_from_days(days);
        LocalTime {
            year,
            month,
            day,
            hour: (rem / 60) as u32,
            minute: (rem % 60) as u32,
            // 1970-01-01 was a Thursday (4).
            weekday: (days + 4).rem_euclid(7) as u32,
        }
    }

    /// `YYYY-MM-DD`, the budget's day key.
    pub(crate) fn day_key(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Minutes since the epoch on the local wall clock.
fn local_minutes(utc_ms: u64, offset_min: i32) -> i64 {
    (utc_ms / MINUTE_MS) as i64 + i64::from(offset_min)
}

/// The UTC ms at which local minute `m` starts.
fn utc_ms_of_local_minute(m: i64, offset_min: i32) -> Option<u64> {
    let utc_min = m - i64::from(offset_min);
    u64::try_from(utc_min).ok().map(|u| u * MINUTE_MS)
}

/// Real time zones run from UTC-12 to UTC+14.
fn check_offset(offset_min: i32) -> Result<(), String> {
    if (-12 * 60..=14 * 60).contains(&offset_min) {
        Ok(())
    } else {
        Err(format!(
            "{offset_min} minutes is not a real time-zone offset"
        ))
    }
}

// ---------------------------------------------------------------------------
// Schedules
// ---------------------------------------------------------------------------

/// A parsed 5-field cron schedule: minute, hour, day of month, month, day of week (0 or 7 =
/// Sunday). Each field takes `*`, `N`, `a-b`, `*/n`, `a-b/n` and comma lists. `@hourly`, `@daily`
/// and `@weekly` are accepted. When both day fields are restricted, either may match (as in cron).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Schedule {
    minutes: u64,
    hours: u32,
    doms: u32,
    months: u16,
    dows: u8,
    dom_any: bool,
    dow_any: bool,
}

fn parse_field(text: &str, lo: u32, hi: u32) -> Result<(u64, bool), String> {
    let mut bits = 0u64;
    let any = text == "*";
    for part in text.split(',') {
        if part.is_empty() {
            return Err(format!("empty item in {text:?}"));
        }
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let s: u32 = s.parse().map_err(|_| format!("bad step in {part:?}"))?;
                if s == 0 {
                    return Err(format!("a step of 0 in {part:?}"));
                }
                (r, s)
            }
            None => (part, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            let a: u32 = a.parse().map_err(|_| format!("bad range in {part:?}"))?;
            let b: u32 = b.parse().map_err(|_| format!("bad range in {part:?}"))?;
            (a, b)
        } else {
            let v: u32 = range.parse().map_err(|_| format!("bad value {part:?}"))?;
            if part.contains('/') {
                (v, hi)
            } else {
                (v, v)
            }
        };
        if a < lo || b > hi || a > b {
            return Err(format!("{part:?} is outside {lo}-{hi}"));
        }
        let mut v = a;
        while v <= b {
            bits |= 1u64 << v;
            v += step;
        }
    }
    Ok((bits, any))
}

impl Schedule {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let expanded = match text {
            "@hourly" => "0 * * * *",
            "@daily" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            t if t.starts_with('@') => return Err(format!("{t:?} is not a supported schedule")),
            t => t,
        };
        let fields: Vec<&str> = expanded.split(' ').collect();
        if fields.len() != 5 || fields.iter().any(|f| f.is_empty()) {
            return Err(
                "a schedule has five fields separated by single spaces: minute hour day month weekday"
                    .into(),
            );
        }
        let (minutes, _) = parse_field(fields[0], 0, 59)?;
        let (hours, _) = parse_field(fields[1], 0, 23)?;
        let (doms, dom_any) = parse_field(fields[2], 1, 31)?;
        let (months, _) = parse_field(fields[3], 1, 12)?;
        let (dows, dow_any) = parse_field(fields[4], 0, 7)?;
        // 7 is Sunday too.
        let dows = (dows | (dows >> 7)) & 0x7f;
        Ok(Schedule {
            minutes,
            hours: hours as u32,
            doms: doms as u32,
            months: months as u16,
            dows: dows as u8,
            dom_any,
            dow_any,
        })
    }

    fn day_matches(&self, t: &LocalTime) -> bool {
        if self.months & (1 << t.month) == 0 {
            return false;
        }
        let dom = self.doms & (1 << t.day) != 0;
        let dow = self.dows & (1 << t.weekday) != 0;
        match (self.dom_any, self.dow_any) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    #[cfg(test)]
    pub(crate) fn matches(&self, t: &LocalTime) -> bool {
        self.minutes & (1 << t.minute) != 0
            && self.hours & (1 << t.hour) != 0
            && self.day_matches(t)
    }

    /// The first matching local minute strictly after `utc_ms`, as UTC ms; `None` if none within
    /// five years (a date that never happens, such as 31 February).
    pub(crate) fn next_after(&self, utc_ms: u64, offset_min: i32) -> Option<u64> {
        let start = local_minutes(utc_ms, offset_min) + 1;
        let first_day = start.div_euclid(DAY_MINUTES);
        let first_minute_of_day = start.rem_euclid(DAY_MINUTES);
        for d in 0..(5 * 366) {
            let day = first_day + d;
            let t = LocalTime::from_local_minutes(day * DAY_MINUTES);
            if self.day_matches(&t) {
                let from = if d == 0 { first_minute_of_day } else { 0 };
                for m in from..DAY_MINUTES {
                    let (h, mi) = ((m / 60) as u32, (m % 60) as u32);
                    if self.hours & (1 << h) != 0 && self.minutes & (1 << mi) != 0 {
                        return utc_ms_of_local_minute(day * DAY_MINUTES + m, offset_min);
                    }
                }
            }
        }
        None
    }

    /// Whether any matching minute starts in `(after_ms, until_ms]`. `next_after` already skips
    /// the minute containing `after_ms`, whose start is never after it.
    fn any_in(&self, after_ms: u64, until_ms: u64, offset_min: i32) -> bool {
        self.next_after(after_ms, offset_min)
            .is_some_and(|t| t <= until_ms)
    }
}

// ---------------------------------------------------------------------------
// The book
// ---------------------------------------------------------------------------

/// A daemon's budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Budget {
    pub max_runs_per_day: u32,
    pub max_tokens_per_day: u32,
    pub max_tokens_per_run: u32,
    /// Always "0" in this release.
    pub max_spend_salt: String,
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            max_runs_per_day: DEFAULT_MAX_RUNS_PER_DAY,
            max_tokens_per_day: DEFAULT_MAX_TOKENS_PER_DAY,
            max_tokens_per_run: DEFAULT_MAX_TOKENS_PER_RUN,
            max_spend_salt: "0".into(),
        }
    }
}

/// A budget as the member enters it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BudgetInput {
    pub max_runs_per_day: u32,
    pub max_tokens_per_day: u32,
    pub max_tokens_per_run: u32,
    pub max_spend_salt: String,
}

impl BudgetInput {
    fn validate(self) -> Result<Budget, String> {
        if self.max_spend_salt != "0" {
            return Err(
                "a daemon's spend budget is 0 in this release: daemons have no way to spend (any non-zero spend budget is pending owner sign-off)"
                    .into(),
            );
        }
        if !(1..=HARD_MAX_RUNS_PER_DAY).contains(&self.max_runs_per_day) {
            return Err(format!("runs per day must be 1 to {HARD_MAX_RUNS_PER_DAY}"));
        }
        if !(1..=HARD_MAX_TOKENS_PER_DAY).contains(&self.max_tokens_per_day) {
            return Err(format!(
                "tokens per day must be 1 to {HARD_MAX_TOKENS_PER_DAY}"
            ));
        }
        if self.max_tokens_per_run == 0 || self.max_tokens_per_run > self.max_tokens_per_day {
            return Err("tokens per run must be at least 1 and no more than tokens per day".into());
        }
        Ok(Budget {
            max_runs_per_day: self.max_runs_per_day,
            max_tokens_per_day: self.max_tokens_per_day,
            max_tokens_per_run: self.max_tokens_per_run,
            max_spend_salt: "0".into(),
        })
    }
}

/// A create or edit request. `id: None` creates.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub schedule: String,
    #[serde(default)]
    pub budget: Option<BudgetInput>,
}

/// HUP-S10.3 — where a run's token count came from: the model server's own usage report for every
/// model call of the run (`measured`), or the character estimate (`estimated`) when any call had
/// none. Older runners that do not say are `estimated`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TokenSource {
    Measured,
    #[default]
    Estimated,
}

impl TokenSource {
    fn label(self) -> &'static str {
        match self {
            TokenSource::Measured => "measured",
            TokenSource::Estimated => "estimated",
        }
    }
}

/// How a run ended, as the runner reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunOutcome {
    Answered,
    Failed,
    Stopped,
    TimedOut,
    OverBudget,
}

impl RunOutcome {
    fn label(self) -> &'static str {
        match self {
            RunOutcome::Answered => "answered",
            RunOutcome::Failed => "failed",
            RunOutcome::Stopped => "stopped",
            RunOutcome::TimedOut => "timed_out",
            RunOutcome::OverBudget => "over_budget",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Running {
    run_id: String,
    started_ms: u64,
    tokens_allowed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: String,
    name: String,
    prompt: String,
    schedule: String,
    budget: Budget,
    paused: bool,
    created_ms: u64,
    /// Schedule minutes up to here have been considered.
    anchor_ms: u64,
    day: String,
    runs_today: u32,
    tokens_today: u64,
    skipped_today: u32,
    last_run_ms: Option<u64>,
    last_outcome: Option<String>,
    last_note: Option<String>,
    /// HUP-S10.3: "measured" or "estimated" for the last run's tokens (absent in older files).
    #[serde(default)]
    last_token_source: Option<String>,
    running: Option<Running>,
}

/// One claimed run, handed to the runner.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Claim {
    pub daemon_id: String,
    pub run_id: String,
    pub name: String,
    pub prompt: String,
    pub tokens_allowed: u32,
    pub started_ms: u64,
}

/// What the UI and the Activity monitor show for one daemon.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonView {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: String,
    pub budget: Budget,
    pub paused: bool,
    /// "paused" | "running" | "budget used up today" | "scheduled" | "never runs"
    pub status: String,
    pub running: bool,
    pub runs_today: u32,
    pub tokens_today: u64,
    pub skipped_today: u32,
    pub spend_today_salt: String,
    pub next_run_ms: Option<u64>,
    pub last_run_ms: Option<u64>,
    pub last_outcome: Option<String>,
    pub last_note: Option<String>,
    /// "measured" | "estimated" for the last run's tokens; None before the first reported run.
    pub last_token_source: Option<String>,
}

/// The whole list plus the global pause switch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DaemonsView {
    pub all_paused: bool,
    pub daemons: Vec<DaemonView>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileBody {
    version: u32,
    all_paused: bool,
    daemons: Vec<Record>,
}

/// Every daemon and its ledger.
#[derive(Debug, Clone, Default)]
pub(crate) struct DaemonBook {
    all_paused: bool,
    records: Vec<Record>,
}

fn new_id(prefix: &str) -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    format!("{prefix}{}", hex::encode(b))
}

fn one_line(s: &str, max: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    flat.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

impl Record {
    fn roll_day(&mut self, today: &str) {
        if self.day != today {
            self.day = today.to_string();
            self.runs_today = 0;
            self.tokens_today = 0;
            self.skipped_today = 0;
        }
    }

    fn exhausted(&self) -> Option<&'static str> {
        if self.runs_today >= self.budget.max_runs_per_day {
            Some("skipped: the daily run budget is used up")
        } else if self.tokens_today >= u64::from(self.budget.max_tokens_per_day) {
            Some("skipped: the daily token budget is used up")
        } else {
            None
        }
    }

    fn view(&self, all_paused: bool, now_ms: u64, offset_min: i32) -> DaemonView {
        let today = LocalTime::from_utc_ms(now_ms, offset_min).day_key();
        let (runs, tokens, skipped) = if self.day == today {
            (self.runs_today, self.tokens_today, self.skipped_today)
        } else {
            (0, 0, 0)
        };
        let used_up = runs >= self.budget.max_runs_per_day
            || tokens >= u64::from(self.budget.max_tokens_per_day);
        let schedule = Schedule::parse(&self.schedule).ok();
        let next_run_ms = schedule.as_ref().and_then(|s| {
            if used_up {
                // Nothing more today: the first match from tomorrow's local midnight on.
                let tomorrow =
                    (local_minutes(now_ms, offset_min).div_euclid(DAY_MINUTES) + 1) * DAY_MINUTES;
                utc_ms_of_local_minute(tomorrow - 1, offset_min)
                    .and_then(|before| s.next_after(before, offset_min))
            } else {
                s.next_after(now_ms, offset_min)
            }
        });
        let paused = self.paused || all_paused;
        let status = if paused {
            "paused"
        } else if self.running.is_some() {
            "running"
        } else if used_up {
            "budget used up today"
        } else if next_run_ms.is_none() {
            "never runs"
        } else {
            "scheduled"
        };
        DaemonView {
            id: self.id.clone(),
            name: self.name.clone(),
            prompt: self.prompt.clone(),
            schedule: self.schedule.clone(),
            budget: self.budget.clone(),
            paused,
            status: status.into(),
            running: self.running.is_some(),
            runs_today: runs,
            tokens_today: tokens,
            skipped_today: skipped,
            spend_today_salt: "0".into(),
            next_run_ms: if paused { None } else { next_run_ms },
            last_run_ms: self.last_run_ms,
            last_outcome: self.last_outcome.clone(),
            last_note: self.last_note.clone(),
            last_token_source: self.last_token_source.clone(),
        }
    }
}

impl DaemonBook {
    pub(crate) fn all_paused(&self) -> bool {
        self.all_paused
    }

    pub(crate) fn list(&self, now_ms: u64, offset_min: i32) -> Vec<DaemonView> {
        self.records
            .iter()
            .map(|r| r.view(self.all_paused, now_ms, offset_min))
            .collect()
    }

    pub(crate) fn view(&self, id: &str, now_ms: u64, offset_min: i32) -> Option<DaemonView> {
        self.records
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.view(self.all_paused, now_ms, offset_min))
    }

    fn find_mut(&mut self, id: &str) -> Result<&mut Record, String> {
        self.records
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| "no such daemon".to_string())
    }

    /// Create or edit a daemon. An edit keeps today's ledger; neither fires for a minute that had
    /// already started when it was saved.
    pub(crate) fn save(
        &mut self,
        input: DaemonInput,
        now_ms: u64,
        offset_min: i32,
    ) -> Result<DaemonView, String> {
        check_offset(offset_min)?;
        let name = one_line(&input.name, NAME_MAX + 1);
        if name.is_empty() || name.chars().count() > NAME_MAX {
            return Err(format!(
                "a daemon needs a name of 1 to {NAME_MAX} characters"
            ));
        }
        let prompt = input.prompt.trim().to_string();
        if prompt.is_empty() || prompt.chars().count() > PROMPT_MAX {
            return Err(format!(
                "a daemon needs a task of 1 to {PROMPT_MAX} characters"
            ));
        }
        let schedule_text = input.schedule.trim().to_string();
        Schedule::parse(&schedule_text)?;
        let budget = match input.budget {
            Some(b) => b.validate()?,
            None => Budget::default(),
        };
        let today = LocalTime::from_utc_ms(now_ms, offset_min).day_key();
        let id = match input.id {
            Some(id) => {
                let r = self.find_mut(&id)?;
                r.name = name;
                r.prompt = prompt;
                r.schedule = schedule_text;
                r.budget = budget;
                r.anchor_ms = now_ms;
                id
            }
            None => {
                if self.records.len() >= MAX_DAEMONS {
                    return Err(format!("at most {MAX_DAEMONS} daemons"));
                }
                let id = new_id("d");
                self.records.push(Record {
                    id: id.clone(),
                    name,
                    prompt,
                    schedule: schedule_text,
                    budget,
                    paused: false,
                    created_ms: now_ms,
                    anchor_ms: now_ms,
                    day: today,
                    runs_today: 0,
                    tokens_today: 0,
                    skipped_today: 0,
                    last_run_ms: None,
                    last_outcome: None,
                    last_note: None,
                    last_token_source: None,
                    running: None,
                });
                id
            }
        };
        self.view(&id, now_ms, offset_min)
            .ok_or_else(|| "internal: the saved daemon is missing".to_string())
    }

    pub(crate) fn delete(&mut self, id: &str) -> Result<(), String> {
        let before = self.records.len();
        self.records.retain(|r| r.id != id);
        if self.records.len() == before {
            return Err("no such daemon".into());
        }
        Ok(())
    }

    /// Pause or resume one daemon. Resuming never replays the minutes it missed.
    pub(crate) fn set_paused(&mut self, id: &str, paused: bool, now_ms: u64) -> Result<(), String> {
        let r = self.find_mut(id)?;
        r.paused = paused;
        r.anchor_ms = r.anchor_ms.max(now_ms);
        Ok(())
    }

    /// Pause or resume every daemon. Resuming never replays the minutes they missed.
    pub(crate) fn set_all_paused(&mut self, paused: bool, now_ms: u64) {
        self.all_paused = paused;
        for r in &mut self.records {
            r.anchor_ms = r.anchor_ms.max(now_ms);
        }
    }

    /// Claim every daemon that is due at `now_ms` and inside its budget. Each claim counts as one
    /// of the day's runs at once; its tokens are charged when it finishes.
    pub(crate) fn claim_due(&mut self, now_ms: u64, offset_min: i32) -> Result<Vec<Claim>, String> {
        check_offset(offset_min)?;
        let today = LocalTime::from_utc_ms(now_ms, offset_min).day_key();
        let all_paused = self.all_paused;
        let mut claims = vec![];
        for r in &mut self.records {
            r.roll_day(&today);
            // Release a run the app never reported back, charging its full allowance.
            if let Some(run) = &r.running {
                if now_ms.saturating_sub(run.started_ms) >= STALE_RUN_MS {
                    r.tokens_today = r.tokens_today.saturating_add(u64::from(run.tokens_allowed));
                    r.last_outcome = Some("abandoned".into());
                    r.last_note = Some(
                        "the app did not report this run's end; it was charged in full".into(),
                    );
                    r.running = None;
                } else {
                    r.anchor_ms = r.anchor_ms.max(now_ms);
                    continue;
                }
            }
            let after = r.anchor_ms.max(now_ms.saturating_sub(CATCH_UP_WINDOW_MS));
            r.anchor_ms = r.anchor_ms.max(now_ms);
            if r.paused || all_paused {
                continue;
            }
            let Ok(schedule) = Schedule::parse(&r.schedule) else {
                continue;
            };
            if !schedule.any_in(after, now_ms, offset_min) {
                continue;
            }
            if let Some(why) = r.exhausted() {
                r.skipped_today = r.skipped_today.saturating_add(1);
                r.last_note = Some(why.into());
                continue;
            }
            let remaining = u64::from(r.budget.max_tokens_per_day).saturating_sub(r.tokens_today);
            let tokens_allowed = u64::from(r.budget.max_tokens_per_run).min(remaining) as u32;
            let run_id = new_id("r");
            r.runs_today = r.runs_today.saturating_add(1);
            r.last_run_ms = Some(now_ms);
            r.running = Some(Running {
                run_id: run_id.clone(),
                started_ms: now_ms,
                tokens_allowed,
            });
            claims.push(Claim {
                daemon_id: r.id.clone(),
                run_id,
                name: r.name.clone(),
                prompt: r.prompt.clone(),
                tokens_allowed,
                started_ms: now_ms,
            });
        }
        Ok(claims)
    }

    /// Record the end of a claimed run: its outcome, the tokens it used (charged as reported, even
    /// past its allowance) and a one-line note. Tokens count as estimated. (Tests; the command
    /// uses [`Self::finish_run_with`].)
    #[cfg(test)]
    pub(crate) fn finish_run(
        &mut self,
        id: &str,
        run_id: &str,
        tokens_used: u32,
        outcome: RunOutcome,
        note: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        self.finish_run_with(
            id,
            run_id,
            tokens_used,
            TokenSource::Estimated,
            outcome,
            note,
            now_ms,
        )
        .map(|_| ())
    }

    /// [`Self::finish_run`] with the token source, returning the run's metering-log entry.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish_run_with(
        &mut self,
        id: &str,
        run_id: &str,
        tokens_used: u32,
        source: TokenSource,
        outcome: RunOutcome,
        note: &str,
        now_ms: u64,
    ) -> Result<RunLogEntry, String> {
        let r = self.find_mut(id)?;
        let started_ms = match &r.running {
            Some(run) if run.run_id == run_id => run.started_ms,
            _ => return Err("that run is not in flight (it finished, or was released)".into()),
        };
        r.running = None;
        r.tokens_today = r.tokens_today.saturating_add(u64::from(tokens_used));
        r.last_outcome = Some(outcome.label().into());
        r.last_token_source = Some(source.label().into());
        r.last_run_ms = Some(r.last_run_ms.map_or(now_ms, |t| t.max(now_ms)));
        let note = one_line(note, NOTE_MAX);
        r.last_note = if note.is_empty() { None } else { Some(note) };
        Ok(RunLogEntry {
            schema: RUN_LOG_SCHEMA,
            daemon_id: r.id.clone(),
            run_id: run_id.to_string(),
            name: r.name.clone(),
            started_ms,
            ended_ms: now_ms.max(started_ms),
            tokens: tokens_used,
            token_source: source,
            outcome,
        })
    }

    pub(crate) fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&FileBody {
            version: FORMAT_VERSION,
            all_paused: self.all_paused,
            daemons: self.records.clone(),
        })
        .map_err(|e| e.to_string())
    }

    /// Parse a saved book. A damaged file is an error, never an empty book.
    pub(crate) fn from_json(text: &str) -> Result<Self, String> {
        let body: FileBody = serde_json::from_str(text)
            .map_err(|e| format!("the daemons file could not be read: {e}"))?;
        if body.version != FORMAT_VERSION {
            return Err(format!(
                "the daemons file has version {}, this app reads version {FORMAT_VERSION}",
                body.version
            ));
        }
        if body.daemons.len() > MAX_DAEMONS {
            return Err("the daemons file lists too many daemons".into());
        }
        for r in &body.daemons {
            Schedule::parse(&r.schedule)
                .map_err(|e| format!("daemon {:?} has a bad schedule: {e}", r.name))?;
            if r.budget.max_spend_salt != "0" {
                return Err(format!("daemon {:?} has a non-zero spend budget", r.name));
            }
        }
        Ok(DaemonBook {
            all_paused: body.all_paused,
            records: body.daemons,
        })
    }
}

// ---------------------------------------------------------------------------
// The daemon run log (HUP-S10.3: every finished run, in the metering folder)
// ---------------------------------------------------------------------------

/// The run log, inside the metering folder core gives the Hermes sidecar
/// (`<app_local_data>/hermes/metering`). Core is its only writer; the sidecar writes only its own
/// `metering.jsonl` there.
pub(crate) const RUN_LOG_FILE: &str = "daemon-runs.jsonl";
/// Run log line format version.
pub(crate) const RUN_LOG_SCHEMA: u32 = 1;
/// The most entries one read returns (the newest ones in the window).
pub(crate) const RUN_LOG_READ_MAX: usize = 500;
/// A run log larger than this is not read (a damaged or runaway file is reported, never parsed).
pub(crate) const RUN_LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// One finished daemon run. No conversation content: the daemon's name (the member's own
/// words), times, the token count and its source, and the outcome. The run's reply and note
/// are never written here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunLogEntry {
    pub schema: u32,
    pub daemon_id: String,
    pub run_id: String,
    pub name: String,
    pub started_ms: u64,
    pub ended_ms: u64,
    pub tokens: u32,
    pub token_source: TokenSource,
    pub outcome: RunOutcome,
}

static RUN_LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Append one entry as a JSON line (the folder is created if needed).
pub(crate) fn append_run_log(path: &Path, entry: &RunLogEntry) -> Result<(), String> {
    use std::io::Write;
    let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
    let _guard = RUN_LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create the metering folder: {e}"))?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("could not open the daemon run log: {e}"))?;
    f.write_all(format!("{line}\n").as_bytes())
        .map_err(|e| format!("could not write the daemon run log: {e}"))
}

/// The entries that ended in `[from_ms, to_ms)`, oldest first, at most [`RUN_LOG_READ_MAX`]
/// (the newest). A missing log is an empty list; a line that does not parse is skipped and
/// counted in the second value, never guessed at.
pub(crate) fn read_run_log(
    path: &Path,
    from_ms: u64,
    to_ms: u64,
) -> Result<(Vec<RunLogEntry>, u32), String> {
    let _guard = RUN_LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    match std::fs::metadata(path) {
        Ok(m) if m.len() > RUN_LOG_MAX_BYTES => {
            return Err("the daemon run log is larger than this app reads".into())
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(format!("the daemon run log could not be read: {e}")),
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("the daemon run log could not be read: {e}"))?;
    let mut out = Vec::new();
    let mut unreadable = 0u32;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<RunLogEntry>(line) {
            Ok(e) if e.schema == RUN_LOG_SCHEMA => {
                if e.ended_ms >= from_ms && e.ended_ms < to_ms {
                    out.push(e);
                }
            }
            _ => unreadable = unreadable.saturating_add(1),
        }
    }
    if out.len() > RUN_LOG_READ_MAX {
        out.drain(..out.len() - RUN_LOG_READ_MAX);
    }
    Ok((out, unreadable))
}

/// The runs in a window plus how many log lines could not be read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunLogView {
    pub runs: Vec<RunLogEntry>,
    pub unreadable: u32,
}

fn run_log_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("hermes").join("metering").join(RUN_LOG_FILE))
}

// ---------------------------------------------------------------------------
// File I/O (one process-wide lock; atomic replace)
// ---------------------------------------------------------------------------

static BOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Read the book; a missing file is an empty book, a damaged one is an error.
pub(crate) fn load_book(path: &Path) -> Result<DaemonBook, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => DaemonBook::from_json(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DaemonBook::default()),
        Err(e) => Err(format!("the daemons file could not be read: {e}")),
    }
}

/// Write the book through a temporary file and a rename.
pub(crate) fn store_book(path: &Path, book: &DaemonBook) -> Result<(), String> {
    let text = book.to_json()?;
    let tmp = path.with_file_name(format!("{FILE_NAME}.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("could not save the daemons file: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("could not save the daemons file: {e}"))
}

fn book_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(FILE_NAME))
}

/// Load, change and (when `write`) store the book under the lock.
fn with_book<T>(
    app: &tauri::AppHandle,
    write: bool,
    f: impl FnOnce(&mut DaemonBook) -> Result<T, String>,
) -> Result<T, String> {
    let path = book_path(app)?;
    let _guard = BOOK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut book = load_book(&path)?;
    let out = f(&mut book)?;
    if write {
        store_book(&path, &book)?;
    }
    Ok(out)
}

fn views(book: &DaemonBook, now_ms: u64, offset_min: i32) -> DaemonsView {
    DaemonsView {
        all_paused: book.all_paused(),
        daemons: book.list(now_ms, offset_min),
    }
}

// ---------------------------------------------------------------------------
// Commands (all async, off the main thread)
// ---------------------------------------------------------------------------

/// **daemons_list** — every daemon with its status, today's ledger and next run.
#[tauri::command]
pub async fn daemons_list(
    app: tauri::AppHandle,
    now_ms: u64,
    offset_min: i32,
) -> Result<DaemonsView, String> {
    crate::blocking::off_main(move || {
        check_offset(offset_min)?;
        with_book(&app, false, |b| Ok(views(b, now_ms, offset_min)))
    })
    .await
}

/// **daemon_save** — create (no id) or edit a daemon.
#[tauri::command]
pub async fn daemon_save(
    app: tauri::AppHandle,
    input: DaemonInput,
    now_ms: u64,
    offset_min: i32,
) -> Result<DaemonView, String> {
    crate::blocking::off_main(move || with_book(&app, true, |b| b.save(input, now_ms, offset_min)))
        .await
}

/// **daemon_set_paused** — pause or resume one daemon.
#[tauri::command]
pub async fn daemon_set_paused(
    app: tauri::AppHandle,
    id: String,
    paused: bool,
    now_ms: u64,
) -> Result<(), String> {
    crate::blocking::off_main(move || with_book(&app, true, |b| b.set_paused(&id, paused, now_ms)))
        .await
}

/// **daemons_set_all_paused** — pause or resume every daemon.
#[tauri::command]
pub async fn daemons_set_all_paused(
    app: tauri::AppHandle,
    paused: bool,
    now_ms: u64,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        with_book(&app, true, |b| {
            b.set_all_paused(paused, now_ms);
            Ok(())
        })
    })
    .await
}

/// **daemon_delete** — remove a daemon and its ledger.
#[tauri::command]
pub async fn daemon_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::blocking::off_main(move || with_book(&app, true, |b| b.delete(&id))).await
}

/// **daemons_claim_due** — the runner's tick: claim every due daemon inside its budget.
#[tauri::command]
pub async fn daemons_claim_due(
    app: tauri::AppHandle,
    now_ms: u64,
    offset_min: i32,
) -> Result<Vec<Claim>, String> {
    crate::blocking::off_main(move || with_book(&app, true, |b| b.claim_due(now_ms, offset_min)))
        .await
}

/// **daemons_finish_run** — the runner reports a claimed run's end. The run is charged to the
/// daemon's ledger and appended to the daemon run log in the metering folder. `token_source`
/// absent (an older runner) counts as estimated.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn daemons_finish_run(
    app: tauri::AppHandle,
    id: String,
    run_id: String,
    tokens_used: u32,
    token_source: Option<TokenSource>,
    outcome: RunOutcome,
    note: String,
    now_ms: u64,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let entry = with_book(&app, true, |b| {
            b.finish_run_with(
                &id,
                &run_id,
                tokens_used,
                token_source.unwrap_or_default(),
                outcome,
                &note,
                now_ms,
            )
        })?;
        // The ledger already holds the charge; a log write that fails is reported, not retried.
        append_run_log(&run_log_path(&app)?, &entry)
    })
    .await
}

/// **daemon_runs_between** — the daemon runs that ended in `[from_ms, to_ms)` (unix ms), from the
/// run log. Read by the journal's daily entry.
#[tauri::command]
pub async fn daemon_runs_between(
    app: tauri::AppHandle,
    from_ms: u64,
    to_ms: u64,
) -> Result<RunLogView, String> {
    crate::blocking::off_main(move || {
        if to_ms <= from_ms || to_ms - from_ms > 8 * 24 * 60 * 60_000 {
            return Err("the window must end after it starts and span at most 8 days".into());
        }
        let (runs, unreadable) = read_run_log(&run_log_path(&app)?, from_ms, to_ms)?;
        Ok(RunLogView { runs, unreadable })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("daemons_tests.rs");
}
