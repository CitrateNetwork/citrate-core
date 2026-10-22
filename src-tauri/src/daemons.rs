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

// ---------------------------------------------------------------------------
// K5a: sidecar-hosted tool sets and Store daemons
// ---------------------------------------------------------------------------

/// One tool set the Hermes sidecar offers to EVERY session while it is on (set by the child's
/// environment at start), outside the tools core passes per session. A Store daemon is bound to
/// the tools its approved permissions grant, so it is not started while any set its permissions
/// do not cover is on (a per-session switch for these sets is a later citrate-agent-runtime item).
pub(crate) struct HostToolSet {
    pub id: &'static str,
    /// What the member sees.
    pub label: &'static str,
    /// The child environment key that turns the set on, and the value that does (`""` = any
    /// non-empty value).
    pub env_key: &'static str,
    pub on_value: &'static str,
    /// The declared permission that covers it (and grants it to a Store daemon).
    pub permission: &'static str,
    /// How the member turns it off, or `None` when the app has no switch for it yet.
    pub turn_off: Option<&'static str>,
    /// Whether a declared permission may grant it to a Store daemon. `false` = never granted
    /// and never named in `toolSets`: the daemon does not start while it is on.
    pub grantable: bool,
}

/// Every sidecar-hosted tool set that can reach a daemon session. The sheet, project, shell and
/// undo tools need a grant document, which daemon sessions are never given (see
/// [`GRANT_DOC_ONLY_SETS`]); the file tools have a row because a launch-environment flag can
/// offer them without one (docs/STORE_CATALOG.md).
pub(crate) const HOST_TOOL_SETS: &[HostToolSet] = &[
    HostToolSet {
        id: "search",
        label: "Web search and page reading (web_search, read_url)",
        env_key: "CITRATE_HERMES_SEARCH",
        on_value: "1",
        permission: "web_search",
        turn_off: Some("turn off \"Let Hermes search the web and read pages\" in Settings > Web search & decisions"),
        grantable: true,
    },
    HostToolSet {
        id: "browser",
        label: "Hermes's browser (browser_*)",
        env_key: "CITRATE_HERMES_BROWSER",
        on_value: "1",
        permission: "web_browse",
        turn_off: Some("turn off \"Let Hermes open and use web pages in its own browser\" in Settings > Web search & decisions"),
        grantable: true,
    },
    HostToolSet {
        id: "mcp",
        label: "MCP servers (mcp__*)",
        env_key: "CITRATE_HERMES_MCP",
        on_value: "",
        permission: "mcp",
        turn_off: Some("turn off every server in Settings > MCP servers"),
        grantable: true,
    },
    HostToolSet {
        id: "toolchain",
        label: "the contract toolchain (forge_test, slither_scan, aderyn_scan, medusa_fuzz)",
        env_key: "CITRATE_HERMES_TOOLCHAIN",
        on_value: "1",
        permission: "toolchain",
        turn_off: Some("turn off the contract toolchain in Agent > Contracts"),
        grantable: true,
    },
    HostToolSet {
        id: "skills",
        label: "installed skills (skill_load)",
        env_key: "CITRATE_HERMES_SKILLS",
        on_value: "",
        permission: "skills_read",
        turn_off: None,
        grantable: true,
    },
    HostToolSet {
        id: "learn",
        label: "skill learning (learn_propose)",
        env_key: "CITRATE_HERMES_LEARN_DIR",
        on_value: "",
        permission: "learn_propose",
        turn_off: None,
        grantable: true,
    },
    HostToolSet {
        id: "capsules",
        label: "installed capsules",
        env_key: "CITRATE_HERMES_CAPSULES",
        on_value: "",
        permission: "capsules_run",
        turn_off: None,
        grantable: true,
    },
    // DGX R1: with CITRATE_HERMES_FILES=1 and a grants file, the sidecar offers the fs_* tools to
    // sessions opened WITHOUT a grant document. Core never sets it; the launch environment can.
    HostToolSet {
        id: "files",
        label: "file tools enabled by the launch environment (fs_*)",
        env_key: "CITRATE_HERMES_FILES",
        on_value: "1",
        permission: "files",
        turn_off: Some("start Citrate Core without CITRATE_HERMES_FILES in its environment"),
        grantable: false,
    },
];

/// Every host tool-set name the sidecar can scope per session (citrate-agent-runtime#87
/// `capabilities.sessionToolSets`). Each must have a [`HOST_TOOL_SETS`] row or be listed in
/// [`GRANT_DOC_ONLY_SETS`] (the tripwire test checks it).
pub(crate) const RUNTIME_HOST_SET_NAMES: &[&str] = &[
    "web_search",
    "web_browse",
    "mcp",
    "toolchain",
    "skills_read",
    "learn_propose",
    "capsules_run",
    "files",
    "shell_run",
];

/// Host tool sets the sidecar offers only to a session opened with a grant document, which a
/// daemon session never is: excluded by construction, so they need no env-hold row.
#[cfg(test)]
pub(crate) const GRANT_DOC_ONLY_SETS: &[&str] = &["shell_run"];

/// The marker a Store daemon's tool list carries for a host tool set its permissions cover.
pub(crate) fn host_set_marker(id: &str) -> String {
    format!("host:{id}")
}

/// The host tool sets on in the sidecar's environment `env`.
pub(crate) fn enabled_host_sets(env: &[(String, String)]) -> Vec<&'static HostToolSet> {
    enabled_host_sets_for(env, cfg!(windows))
}

/// Whether two environment variable names are the same name: exact on Unix, ASCII
/// case-insensitive where the platform's names are (Windows, DGX R2).
pub(crate) fn env_name_eq(a: &str, b: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// [`enabled_host_sets`] with the platform's env-name rule made explicit (tests drive both).
pub(crate) fn enabled_host_sets_for(
    env: &[(String, String)],
    case_insensitive: bool,
) -> Vec<&'static HostToolSet> {
    HOST_TOOL_SETS
        .iter()
        .filter(|set| {
            env.iter().any(|(k, v)| {
                env_name_eq(k, set.env_key, case_insensitive)
                    && if set.on_value.is_empty() {
                        !v.trim().is_empty()
                    } else {
                        v.trim() == set.on_value
                    }
            })
        })
        .collect()
}

/// The ids of the host tool sets on in `env`, in [`HOST_TOOL_SETS`] order.
pub(crate) fn enabled_host_set_ids(env: &[(String, String)]) -> Vec<String> {
    enabled_host_sets(env)
        .into_iter()
        .map(|s| s.id.to_string())
        .collect()
}

/// What core knows about the running sidecar when it decides whether a Store daemon may start.
pub(crate) struct SidecarFacts<'a> {
    /// The effective environment the sidecar was started with (which host tool sets every
    /// session gets unless scoped).
    pub env: &'a [(String, String)],
    /// citrate-agent-runtime#87: `GET /status` → `capabilities.sessionToolSets`, the host tool sets
    /// a session can be scoped to with `toolSets` (reported only together with
    /// `sessionPromptSnapshot`). `None` = an older sidecar without the capability.
    pub session_tool_sets: Option<&'a [String]>,
}

/// Whether the sidecar can scope EVERY host tool set per session, so a Store daemon's session is
/// opened with exactly its approved sets whatever is on for other sessions. A sidecar that
/// reports only some of them is treated as an older one.
pub(crate) fn sidecar_scopes_host_sets(session_tool_sets: Option<&[String]>) -> bool {
    // Every runtime set name, the grant-document-only ones included: a sidecar that cannot
    // scope one of them is treated as an older one.
    session_tool_sets.is_some_and(|names| {
        RUNTIME_HOST_SET_NAMES
            .iter()
            .all(|want| names.iter().any(|n| n == want))
    })
}

/// [`host_block_reason`] relaxed only for a sidecar that scopes every set per session.
pub(crate) fn host_block_reason_for(
    allowed: &[String],
    facts: &SidecarFacts<'_>,
) -> Option<String> {
    if sidecar_scopes_host_sets(facts.session_tool_sets) {
        return None;
    }
    host_block_reason(allowed, facts.env)
}

/// The `toolSets` a Store daemon's session asks for: the host tool-set permissions its approved
/// permissions cover, in [`HOST_TOOL_SETS`] order (empty = none).
pub(crate) fn store_tool_sets(allowed: &[String]) -> Vec<String> {
    HOST_TOOL_SETS
        .iter()
        .filter(|set| set.grantable && allowed.contains(&host_set_marker(set.id)))
        .map(|set| set.permission.to_string())
        .collect()
}

/// Said when core cannot see the sidecar's environment (not running, or restarting).
pub(crate) const HOST_ENV_UNKNOWN: &str = "Not started: Hermes is not running or is restarting, so the tools it offers could not be checked. It runs at its next scheduled time.";

/// K5a: the check right before a Store daemon's session is opened. `claimed` is the host tool
/// sets on when the run was claimed; `current` the environment of the sidecar now (`None` = not
/// running or restarting). Refused when the environment is unknown, when the sets changed since
/// the claim (a restart with other settings), or when an uncovered set is on.
pub(crate) fn store_open_check(
    allowed: &[String],
    claimed: Option<&[String]>,
    current: Option<&[(String, String)]>,
    session_tool_sets: Option<&[String]>,
) -> Result<(), String> {
    let env = current.ok_or_else(|| HOST_ENV_UNKNOWN.to_string())?;
    let now = enabled_host_set_ids(env);
    match claimed {
        Some(c) if c == now.as_slice() => {}
        _ => {
            return Err(
                "Not started: the tools Hermes offers changed since this run was claimed (it restarted with other settings). It runs at its next scheduled time."
                    .into(),
            )
        }
    }
    let facts = SidecarFacts {
        env,
        session_tool_sets,
    };
    match host_block_reason_for(allowed, &facts) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// Why a Store daemon bound to `allowed` must not start while the sidecar runs with `env`, or
/// `None` when every host tool set that is on is covered by its approved permissions.
pub(crate) fn host_block_reason(allowed: &[String], env: &[(String, String)]) -> Option<String> {
    let uncovered: Vec<&HostToolSet> = enabled_host_sets(env)
        .into_iter()
        .filter(|set| !set.grantable || !allowed.contains(&host_set_marker(set.id)))
        .collect();
    if uncovered.is_empty() {
        return None;
    }
    let names: Vec<&str> = uncovered.iter().map(|s| s.label).collect();
    let steps: Vec<&str> = uncovered.iter().filter_map(|s| s.turn_off).collect();
    let stuck = uncovered.iter().any(|s| s.turn_off.is_none());
    let mut msg = format!(
        "Not started: Hermes offers every session tools this daemon was not approved for: {}.",
        names.join("; ")
    );
    if !steps.is_empty() {
        msg.push_str(&format!(" To run it sandboxed, {}.", steps.join(", and ")));
    }
    if stuck {
        msg.push_str(
            " Some of these cannot be turned off in the app yet; Store daemons run once Hermes can open a session without them.",
        );
    }
    Some(msg)
}

/// Why [`DaemonBook::delete_idle`] refused.
pub(crate) const DAEMON_RUN_IN_FLIGHT: &str = "a run of this daemon is in flight";

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
    /// Core refused to open the run's session (an open-time check, DGX R5).
    NotStarted,
}

impl RunOutcome {
    fn label(self) -> &'static str {
        match self {
            RunOutcome::Answered => "answered",
            RunOutcome::Failed => "failed",
            RunOutcome::Stopped => "stopped",
            RunOutcome::TimedOut => "timed_out",
            RunOutcome::OverBudget => "over_budget",
            RunOutcome::NotStarted => "not_started",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Running {
    run_id: String,
    started_ms: u64,
    tokens_allowed: u32,
    /// K5a: for a Store daemon, the host tool sets on when this run was claimed (ids). The
    /// session open checks the sets are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host_sets: Option<Vec<String>>,
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
    /// K5a: the agent tools a Store-installed daemon may be offered and may call, from the
    /// permissions the member approved at install (deny by default: an empty list is no tools).
    /// `None` for a daemon the member wrote here, which keeps the member-daemon tool set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowed_tools: Option<Vec<String>>,
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
    /// K5a: the only tools this run may be offered and call; `None` = the member-daemon set.
    pub allowed_tools: Option<Vec<String>>,
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
        self.save_with_new_id(input, None, now_ms, offset_min)
    }

    /// A new daemon id in the book's format (`d` + 16 hex). The Store (K5a) takes one before it
    /// creates the daemon, so its install record can name the daemon first.
    pub(crate) fn fresh_id() -> String {
        new_id("d")
    }

    /// Create a daemon under a given new id (from [`Self::fresh_id`]), PAUSED from the first
    /// write, bound to `allowed_tools` (the tools its approved permissions grant; empty = none).
    /// Refused when the id is already in use or `input.id` is set.
    pub(crate) fn create_paused_with_id(
        &mut self,
        id: String,
        input: DaemonInput,
        allowed_tools: Vec<String>,
        now_ms: u64,
        offset_min: i32,
    ) -> Result<DaemonView, String> {
        if input.id.is_some() {
            return Err("internal: a new daemon has no id yet".into());
        }
        if self.records.iter().any(|r| r.id == id) {
            return Err("internal: that daemon id is already in use".into());
        }
        let v = self.save_with_new_id(input, Some(id), now_ms, offset_min)?;
        self.set_paused(&v.id, true, now_ms)?;
        self.find_mut(&v.id)?.allowed_tools = Some(allowed_tools);
        self.view(&v.id, now_ms, offset_min)
            .ok_or_else(|| "internal: the saved daemon is missing".to_string())
    }

    fn save_with_new_id(
        &mut self,
        input: DaemonInput,
        preset_id: Option<String>,
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
                let id = preset_id.unwrap_or_else(|| new_id("d"));
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
                    allowed_tools: None,
                });
                id
            }
        };
        self.view(&id, now_ms, offset_min)
            .ok_or_else(|| "internal: the saved daemon is missing".to_string())
    }

    /// K5a: delete a daemon only when no run of it is in flight. A run the app has not reported
    /// back within [`STALE_RUN_MS`] no longer counts (the same release `claim_due` applies).
    pub(crate) fn delete_idle(&mut self, id: &str, now_ms: u64) -> Result<(), String> {
        if let Some(r) = self.records.iter().find(|r| r.id == id) {
            if let Some(run) = &r.running {
                if now_ms < run.started_ms.saturating_add(STALE_RUN_MS) {
                    return Err(DAEMON_RUN_IN_FLIGHT.into());
                }
            }
        }
        self.delete(id)
    }

    /// The tools a daemon is bound to (`None` = a member-written daemon, or no such daemon).
    pub(crate) fn allowed_tools_of(&self, id: &str) -> Option<Vec<String>> {
        self.records
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.allowed_tools.clone())
    }

    /// Whether the book holds daemon `id`.
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.records.iter().any(|r| r.id == id)
    }

    /// Whether daemon `id` has run `run_id` in flight (claimed, not yet finished or released).
    pub(crate) fn run_in_flight(&self, id: &str, run_id: &str) -> bool {
        self.records
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.running.as_ref())
            .is_some_and(|run| run.run_id == run_id)
    }

    /// The host tool sets the daemon's run in flight was claimed against.
    pub(crate) fn claimed_host_sets(&self, id: &str) -> Option<Vec<String>> {
        self.records
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.running.as_ref())
            .and_then(|run| run.host_sets.clone())
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
    /// Resume (start) a daemon. A Store daemon is refused while the running sidecar offers a host
    /// tool set its approved permissions do not cover, with the reason and how to run it
    /// sandboxed. `host_env` `None` = Hermes is not running (the check runs again at claim time).
    #[cfg(test)]
    pub(crate) fn resume_checked(
        &mut self,
        id: &str,
        now_ms: u64,
        host_env: Option<&[(String, String)]>,
    ) -> Result<(), String> {
        let facts = host_env.map(|env| SidecarFacts {
            env,
            session_tool_sets: None,
        });
        self.resume_with(id, now_ms, facts.as_ref())
    }

    /// [`Self::resume_checked`] with everything core knows about the running sidecar.
    pub(crate) fn resume_with(
        &mut self,
        id: &str,
        now_ms: u64,
        host: Option<&SidecarFacts<'_>>,
    ) -> Result<(), String> {
        let r = self.find_mut(id)?;
        if let (Some(allowed), Some(facts)) = (&r.allowed_tools, host) {
            if let Some(why) = host_block_reason_for(allowed, facts) {
                return Err(why);
            }
        }
        self.set_paused(id, false, now_ms)
    }

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

    /// [`Self::claim_due_with`] with no sidecar environment (tests drive schedules with it).
    #[cfg(test)]
    pub(crate) fn claim_due(&mut self, now_ms: u64, offset_min: i32) -> Result<Vec<Claim>, String> {
        self.claim_due_with(now_ms, offset_min, None)
    }

    /// Claim every daemon that is due at `now_ms` and inside its budget. Each claim counts as one
    /// of the day's runs at once; its tokens are charged when it finishes. With the running
    /// sidecar's environment (K5a), a Store daemon due now is not
    /// started while a host tool set its permissions do not cover is on (it is recorded as
    /// `not_started`, with the reason). `None` = no sidecar environment known (nothing runs then
    /// anyway: daemons run only in Hermes).
    #[cfg(test)]
    pub(crate) fn claim_due_with(
        &mut self,
        now_ms: u64,
        offset_min: i32,
        host_env: Option<&[(String, String)]>,
    ) -> Result<Vec<Claim>, String> {
        let facts = host_env.map(|env| SidecarFacts {
            env,
            session_tool_sets: None,
        });
        self.claim_due_facts(now_ms, offset_min, facts.as_ref())
    }

    /// [`Self::claim_due_with`] with everything core knows about the running sidecar: one that
    /// scopes every host tool set per session (citrate-agent-runtime#87) lets a Store daemon be
    /// claimed with sets on, because its session is opened with exactly its approved sets.
    pub(crate) fn claim_due_facts(
        &mut self,
        now_ms: u64,
        offset_min: i32,
        host: Option<&SidecarFacts<'_>>,
    ) -> Result<Vec<Claim>, String> {
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
            // A Store daemon is claimed only against a KNOWN sidecar environment: unknown (Hermes
            // not running, in crash backoff, or restarting) blocks it like an uncovered set does.
            let mut host_sets = None;
            if let Some(allowed) = &r.allowed_tools {
                let why = match host {
                    None => Some(HOST_ENV_UNKNOWN.to_string()),
                    Some(facts) => host_block_reason_for(allowed, facts),
                };
                if let Some(why) = why {
                    r.skipped_today = r.skipped_today.saturating_add(1);
                    r.last_outcome = Some("not_started".into());
                    r.last_note = Some(one_line(&why, NOTE_MAX));
                    continue;
                }
                host_sets = host.map(|f| enabled_host_set_ids(f.env));
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
                host_sets,
            });
            claims.push(Claim {
                daemon_id: r.id.clone(),
                run_id,
                name: r.name.clone(),
                prompt: r.prompt.clone(),
                tokens_allowed,
                started_ms: now_ms,
                allowed_tools: r.allowed_tools.clone(),
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
        // A run that never started ran no inference: it is never charged (the runner sends 0;
        // core does not rely on that).
        let tokens_used = if outcome == RunOutcome::NotStarted {
            0
        } else {
            tokens_used
        };
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
/// The most bytes of the run log one read looks at (the newest ones; older runs are not read).
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
/// counted in the second value, never guessed at. A log larger than [`RUN_LOG_MAX_BYTES`] is
/// read from its last [`RUN_LOG_MAX_BYTES`] only (the newest runs), so a long-lived log never
/// stops the journal from reading today's runs.
pub(crate) fn read_run_log(
    path: &Path,
    from_ms: u64,
    to_ms: u64,
) -> Result<(Vec<RunLogEntry>, u32), String> {
    read_run_log_capped(path, from_ms, to_ms, RUN_LOG_MAX_BYTES)
}

/// [`read_run_log`] with the byte cap as a parameter (tests use a small one).
pub(crate) fn read_run_log_capped(
    path: &Path,
    from_ms: u64,
    to_ms: u64,
    cap: u64,
) -> Result<(Vec<RunLogEntry>, u32), String> {
    use std::io::{Read, Seek, SeekFrom};
    let _guard = RUN_LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(format!("the daemon run log could not be read: {e}")),
    };
    let len = f
        .metadata()
        .map_err(|e| format!("the daemon run log could not be read: {e}"))?
        .len();
    let cut = len > cap;
    if cut {
        f.seek(SeekFrom::Start(len - cap))
            .map_err(|e| format!("the daemon run log could not be read: {e}"))?;
    }
    let mut bytes = Vec::new();
    f.take(cap)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("the daemon run log could not be read: {e}"))?;
    // Reading from the middle of the file starts inside a line: drop that partial line.
    let start = if cut {
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| i + 1)
    } else {
        0
    };
    let text = String::from_utf8_lossy(&bytes[start..]);
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

pub(crate) fn book_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
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
    with_book_file(&book_path(app)?, write, f)
}

/// [`with_book`] over an explicit file: load, change and (when `write`) store the book under the
/// one process-wide lock. The Store's daemon installer (K5a) writes through here.
pub(crate) fn with_book_file<T>(
    path: &Path,
    write: bool,
    f: impl FnOnce(&mut DaemonBook) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = BOOK_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut book = load_book(path)?;
    let out = f(&mut book)?;
    if write {
        store_book(path, &book)?;
    }
    Ok(out)
}

/// The running sidecar's effective start environment and its per-session tool-set capability
/// (`None`s while Hermes is not running, or for a sidecar without the capability).
type SidecarFactsOwned = (Option<Vec<(String, String)>>, Option<Vec<String>>);

fn sidecar_facts(app: &tauri::AppHandle) -> SidecarFactsOwned {
    match crate::hermes::manager(app) {
        Ok(m) => (m.started_env(), m.session_tool_sets()),
        Err(_) => (None, None),
    }
}

/// What the daemon session open is for: `Ok(None)` a member daemon, `Ok(Some(allowed))` a Store
/// daemon bound to `allowed`.
/// FAIL-CLOSED (DGX R3): no daemon id, or an id the book does not hold, is refused instead of
/// being opened unchecked.
pub(crate) fn open_target(
    book: &DaemonBook,
    daemon_id: Option<&str>,
    run_id: Option<&str>,
) -> Result<Option<Vec<String>>, String> {
    let id = daemon_id.ok_or_else(|| {
        "Not started: the daemon run did not say which daemon it is for.".to_string()
    })?;
    if !book.contains(id) {
        return Err("Not started: that daemon is no longer in the daemon list.".into());
    }
    // Bound in core: the session is for the run core claimed for this daemon, still in flight.
    // The webview names both; any other pairing (no run, another daemon's run, a finished or
    // released run) opens nothing.
    let run_id = run_id
        .ok_or_else(|| "Not started: the daemon run did not say which run it is.".to_string())?;
    if !book.run_in_flight(id, run_id) {
        return Err("Not started: that run is not in flight for this daemon.".into());
    }
    Ok(book.allowed_tools_of(id))
}

/// How a Store daemon's session may be opened, after [`check_store_daemon_open`].
pub(crate) struct StoreOpen {
    /// The daemon's approved tools (core tools plus `host:<set>` markers), from the book.
    pub allowed: Vec<String>,
    /// The sidecar scopes every host tool set per session (citrate-agent-runtime#87): the session
    /// must then echo `toolSets` and `promptSnapshot: false` back, or it is closed and refused.
    /// `false` = the env-hold fallback: no uncovered host set is on in the sidecar at all.
    pub scoped: bool,
}

/// K5a: for a Store daemon, the open-time check (`store_open_check`) against the sidecar's
/// current environment and capability. `Ok(Some(..))` for a Store daemon that may open;
/// `Ok(None)` for a member daemon or an unknown id.
pub(crate) fn check_store_daemon_open(
    app: &tauri::AppHandle,
    id: Option<&str>,
    run_id: Option<&str>,
) -> Result<Option<StoreOpen>, String> {
    let (allowed, claimed) = with_book(app, false, |b| {
        let allowed = open_target(b, id, run_id)?;
        Ok((allowed, id.and_then(|id| b.claimed_host_sets(id))))
    })?;
    let Some(allowed) = allowed else {
        return Ok(None);
    };
    let (current, sets) = sidecar_facts(app);
    store_open_check(
        &allowed,
        claimed.as_deref(),
        current.as_deref(),
        sets.as_deref(),
    )?;
    Ok(Some(StoreOpen {
        allowed,
        scoped: sidecar_scopes_host_sets(sets.as_deref()),
    }))
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
    crate::blocking::off_main(move || {
        if paused {
            return with_book(&app, true, |b| b.set_paused(&id, true, now_ms));
        }
        let (env, sets) = sidecar_facts(&app);
        let facts = env.as_deref().map(|env| SidecarFacts {
            env,
            session_tool_sets: sets.as_deref(),
        });
        with_book(&app, true, |b| b.resume_with(&id, now_ms, facts.as_ref()))
    })
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
    crate::blocking::off_main(move || {
        // K5a: the sidecar's environment decides whether a Store daemon may start now.
        let (env, sets) = sidecar_facts(&app);
        let facts = env.as_deref().map(|env| SidecarFacts {
            env,
            session_tool_sets: sets.as_deref(),
        });
        with_book(&app, true, |b| {
            b.claim_due_facts(now_ms, offset_min, facts.as_ref())
        })
    })
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
