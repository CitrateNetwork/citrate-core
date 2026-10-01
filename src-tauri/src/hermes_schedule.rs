//! HUP-S10.2 (US-10.2 AC3) — Hermes's own schedule, kept on this device and shown as a calendar.
//!
//! A schedule entry is something Hermes is meant to do, or remind the member of, at a time: a
//! one-off or a daily or weekly repeat. The member adds, pauses and removes entries in the
//! Schedule view; Hermes reads them (the `schedule_list` chat tool, read-only). Nothing here runs
//! anything: the daemons lane (HUP-S10.3) asks [`ScheduleStore::due`] which starts fell between
//! its last check and now, and decides what to do under its own budgets and HIC rules.
//!
//! Storage: `<app data>/agent/hermes-schedule.json` (versioned, owner-only, written through a
//! temporary file and a rename). Like the grant document next to it, a file that does not parse
//! is treated as an empty schedule (nothing is due), is never written over, and can be set aside
//! from the view ([`ScheduleStore::reset_corrupted`]).
//!
//! Times are unix seconds (UTC). Repeats step by exactly 24 h or 7 days, so across a daylight
//! saving change the local clock time of a repeat moves by an hour; the view shows local time.
//!
//! Keyless: nothing here signs or holds a key (Rule 3).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const SCHEDULE_FILE: &str = "hermes-schedule.json";
pub const SCHEDULE_VERSION: u32 = 1;
// The caps below are conservative defaults, pending owner sign-off.
/// Most entries the schedule holds.
pub const MAX_ENTRIES: usize = 200;
/// Longest entry title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;
/// Longest entry notes, in characters.
pub const MAX_NOTES_CHARS: usize = 2_000;
/// Longest entry, in minutes (one week).
pub const MAX_DURATION_MINS: u32 = 7 * 24 * 60;
/// Most occurrences one listing or one `due` call returns.
pub const MAX_OCCURRENCES: usize = 1_000;
/// Widest calendar window a listing may ask for, in days.
pub const MAX_WINDOW_DAYS: u64 = 92;

const DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    None,
    Daily,
    Weekly,
}

impl Repeat {
    fn period(self) -> Option<u64> {
        match self {
            Repeat::None => None,
            Repeat::Daily => Some(DAY),
            Repeat::Weekly => Some(7 * DAY),
        }
    }
}

/// Who put the entry on the schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Member,
    Hermes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// Unix seconds of the first start.
    pub start: u64,
    pub duration_mins: u32,
    pub repeat: Repeat,
    /// Unix seconds; no start at or after it. `None` = no end.
    #[serde(default)]
    pub until: Option<u64>,
    pub enabled: bool,
    pub origin: Origin,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Schedule {
    pub version: u32,
    pub next_id: u64,
    pub entries: Vec<Entry>,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            version: SCHEDULE_VERSION,
            next_id: 1,
            entries: Vec::new(),
        }
    }
}

/// What the member (or Hermes, through a reviewed path) asks to add.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewEntry {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    pub start: u64,
    pub duration_mins: u32,
    pub repeat: Repeat,
    #[serde(default)]
    pub until: Option<u64>,
    #[serde(default = "member")]
    pub origin: Origin,
}

fn member() -> Origin {
    Origin::Member
}

/// One start of an entry, for the calendar or for `due`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub entry_id: String,
    pub title: String,
    pub start: u64,
    pub end: u64,
    pub repeat: Repeat,
    pub origin: Origin,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedSchedule {
    Ok(Schedule),
    /// The file exists but is not a valid schedule: nothing is due from it.
    Corrupted(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    Corrupted(String),
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleError::Corrupted(e) => write!(
                f,
                "the saved schedule could not be read ({e}), so nothing on it will run; reset it in the Schedule view to start again"
            ),
            ScheduleError::Invalid(e) => f.write_str(e),
            ScheduleError::Io(e) => write!(f, "could not save the schedule: {e}"),
        }
    }
}

fn invalid(m: impl Into<String>) -> ScheduleError {
    ScheduleError::Invalid(m.into())
}

fn clean_text(
    s: &str,
    max: usize,
    what: &str,
    allow_newlines: bool,
) -> Result<String, ScheduleError> {
    let t = s.trim();
    if t.chars().count() > max {
        return Err(invalid(format!(
            "the {what} is longer than {max} characters"
        )));
    }
    if t.chars()
        .any(|c| c.is_control() && !(allow_newlines && (c == '\n' || c == '\t')))
    {
        return Err(invalid(format!("the {what} has control characters")));
    }
    Ok(t.to_string())
}

fn check_entry(e: &Entry) -> Result<(), String> {
    if e.title.trim().is_empty() || e.title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!("entry {} has a bad title", e.id));
    }
    if e.notes.chars().count() > MAX_NOTES_CHARS {
        return Err(format!("entry {} has notes that are too long", e.id));
    }
    if e.duration_mins == 0 || e.duration_mins > MAX_DURATION_MINS {
        return Err(format!("entry {} has a bad duration", e.id));
    }
    if e.until.is_some_and(|u| u <= e.start) {
        return Err(format!("entry {} ends before it starts", e.id));
    }
    Ok(())
}

fn validate(s: &Schedule) -> Result<(), String> {
    if s.version != SCHEDULE_VERSION {
        return Err(format!("unknown schedule version {}", s.version));
    }
    if s.entries.len() > MAX_ENTRIES {
        return Err("too many entries".into());
    }
    let mut ids = std::collections::HashSet::new();
    for e in &s.entries {
        if !ids.insert(e.id.as_str()) {
            return Err(format!("entry id {} appears twice", e.id));
        }
        check_entry(e)?;
    }
    Ok(())
}

/// A listing window must run forward and span at most [`MAX_WINDOW_DAYS`].
pub fn check_window(from: u64, to: u64) -> Result<(), String> {
    if to <= from {
        return Err("the calendar window must end after it starts".into());
    }
    if to - from > MAX_WINDOW_DAYS * DAY {
        return Err(format!(
            "the calendar window is wider than {MAX_WINDOW_DAYS} days"
        ));
    }
    Ok(())
}

/// The starts of `e` in `[lo, hi)`, at most `cap` of them.
fn starts_in(e: &Entry, lo: u64, hi: u64, cap: usize) -> Vec<u64> {
    let mut out = Vec::new();
    let hi = e.until.map_or(hi, |u| hi.min(u));
    if hi <= lo || cap == 0 {
        return out;
    }
    match e.repeat.period() {
        None => {
            if e.start >= lo && e.start < hi {
                out.push(e.start);
            }
        }
        Some(p) => {
            let first = if lo <= e.start {
                e.start
            } else {
                let k = (lo - e.start).div_ceil(p);
                e.start.saturating_add(k.saturating_mul(p))
            };
            let mut t = first;
            while t < hi && out.len() < cap {
                out.push(t);
                match t.checked_add(p) {
                    Some(n) => t = n,
                    None => break,
                }
            }
        }
    }
    out
}

fn occurrence(e: &Entry, start: u64) -> Occurrence {
    Occurrence {
        entry_id: e.id.clone(),
        title: e.title.clone(),
        start,
        end: start.saturating_add(u64::from(e.duration_mins) * 60),
        repeat: e.repeat,
        origin: e.origin,
        disabled: !e.enabled,
    }
}

fn sorted_capped(mut v: Vec<Occurrence>) -> Vec<Occurrence> {
    v.sort_by(|a, b| a.start.cmp(&b.start).then(a.entry_id.cmp(&b.entry_id)));
    v.truncate(MAX_OCCURRENCES);
    v
}

/// Every occurrence that overlaps `[from, to)` (disabled entries included, marked), sorted by
/// start, at most [`MAX_OCCURRENCES`].
pub fn occurrences(s: &Schedule, from: u64, to: u64) -> Vec<Occurrence> {
    let mut v = Vec::new();
    for e in &s.entries {
        // An occurrence overlaps when it starts before `to` and ends after `from`.
        let span = u64::from(e.duration_mins) * 60;
        let lo = from.saturating_sub(span.saturating_sub(1));
        for t in starts_in(e, lo, to, MAX_OCCURRENCES) {
            if t.saturating_add(span) > from {
                v.push(occurrence(e, t));
            }
        }
    }
    sorted_capped(v)
}

/// The enabled starts in `(after, upto]`: what became due since the last check. Each start is
/// returned by exactly one of a run of checks with contiguous intervals.
pub fn due_in(s: &Schedule, after: u64, upto: u64) -> Vec<Occurrence> {
    if upto <= after {
        return Vec::new();
    }
    let mut v = Vec::new();
    for e in s.entries.iter().filter(|e| e.enabled) {
        for t in starts_in(
            e,
            after.saturating_add(1),
            upto.saturating_add(1),
            MAX_OCCURRENCES,
        ) {
            v.push(occurrence(e, t));
        }
    }
    sorted_capped(v)
}

/// The schedule file.
#[derive(Debug, Clone)]
pub struct ScheduleStore {
    dir: PathBuf,
}

impl ScheduleStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ScheduleStore { dir: dir.into() }
    }

    /// The production store: `<app data>/agent`.
    pub fn for_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Self, String> {
        use tauri::Manager;
        let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        Ok(ScheduleStore::new(data.join("agent")))
    }

    pub fn file(&self) -> PathBuf {
        self.dir.join(SCHEDULE_FILE)
    }

    pub fn load_schedule(&self) -> LoadedSchedule {
        let text = match std::fs::read_to_string(self.file()) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return LoadedSchedule::Ok(Schedule::default())
            }
            Err(e) => return LoadedSchedule::Corrupted(format!("unreadable: {}", e.kind())),
        };
        let s: Schedule = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => return LoadedSchedule::Corrupted(format!("not a schedule: {e}")),
        };
        match validate(&s) {
            Ok(()) => LoadedSchedule::Ok(s),
            Err(e) => LoadedSchedule::Corrupted(e),
        }
    }

    pub fn load_ok(&self) -> Result<Schedule, ScheduleError> {
        match self.load_schedule() {
            LoadedSchedule::Ok(s) => Ok(s),
            LoadedSchedule::Corrupted(e) => Err(ScheduleError::Corrupted(e)),
        }
    }

    fn save_schedule(&self, s: &Schedule) -> Result<(), ScheduleError> {
        validate(s).map_err(ScheduleError::Invalid)?;
        let text = serde_json::to_string_pretty(s).map_err(|e| ScheduleError::Io(e.to_string()))?;
        let tmp = self.dir.join(format!("{SCHEDULE_FILE}.tmp"));
        citrate_core_kit::fsutil::write_secret_file(&tmp, text.as_bytes())
            .map_err(|e| ScheduleError::Io(e.kind().to_string()))?;
        std::fs::rename(&tmp, self.file()).map_err(|e| ScheduleError::Io(e.kind().to_string()))
    }

    /// Add an entry; returns it with its id.
    pub fn add_entry(&self, n: NewEntry, now: u64) -> Result<Entry, ScheduleError> {
        let mut s = self.load_ok()?;
        if s.entries.len() >= MAX_ENTRIES {
            return Err(invalid(format!(
                "the schedule already holds {MAX_ENTRIES} entries; remove one first"
            )));
        }
        let title = clean_text(&n.title, MAX_TITLE_CHARS, "title", false)?;
        if title.is_empty() {
            return Err(invalid("the title is empty"));
        }
        let notes = clean_text(&n.notes, MAX_NOTES_CHARS, "notes", true)?;
        if n.duration_mins == 0 || n.duration_mins > MAX_DURATION_MINS {
            return Err(invalid(format!(
                "the duration must be 1 to {MAX_DURATION_MINS} minutes"
            )));
        }
        if n.until.is_some_and(|u| u <= n.start) {
            return Err(invalid("the end of the repeat is before the first start"));
        }
        let e = Entry {
            id: format!("s{}", s.next_id),
            title,
            notes,
            start: n.start,
            duration_mins: n.duration_mins,
            repeat: n.repeat,
            until: n.until,
            enabled: true,
            origin: n.origin,
            created_at: now,
        };
        s.next_id += 1;
        s.entries.push(e.clone());
        self.save_schedule(&s)?;
        Ok(e)
    }

    pub fn remove_entry(&self, id: &str) -> Result<(), ScheduleError> {
        let mut s = self.load_ok()?;
        let before = s.entries.len();
        s.entries.retain(|e| e.id != id);
        if s.entries.len() == before {
            return Err(invalid(format!("there is no schedule entry {id}")));
        }
        self.save_schedule(&s)
    }

    pub fn set_entry_enabled(&self, id: &str, enabled: bool) -> Result<(), ScheduleError> {
        let mut s = self.load_ok()?;
        let e = s
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| invalid(format!("there is no schedule entry {id}")))?;
        e.enabled = enabled;
        self.save_schedule(&s)
    }

    /// What became due in `(after, upto]` (the daemons lane's query). A corrupted schedule has
    /// nothing due.
    pub fn due_between(&self, after: u64, upto: u64) -> Vec<Occurrence> {
        match self.load_schedule() {
            LoadedSchedule::Ok(s) => due_in(&s, after, upto),
            LoadedSchedule::Corrupted(_) => Vec::new(),
        }
    }

    /// Set a corrupted file aside and start empty. Refused when the file is valid.
    pub fn reset_corrupted(&self, now: u64) -> Result<PathBuf, ScheduleError> {
        if let LoadedSchedule::Ok(_) = self.load_schedule() {
            return Err(invalid(
                "the schedule is readable; remove entries one by one instead",
            ));
        }
        let kept = self.dir.join(format!("{SCHEDULE_FILE}.corrupt-{now}"));
        std::fs::rename(self.file(), &kept).map_err(|e| ScheduleError::Io(e.kind().to_string()))?;
        self.save_schedule(&Schedule::default())?;
        Ok(kept)
    }
}

/// The Schedule view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleView {
    /// `ok` or `corrupted`.
    pub status: &'static str,
    pub error: Option<String>,
    pub entries: Vec<Entry>,
    pub occurrences: Vec<Occurrence>,
}

fn schedule_view(store: &ScheduleStore, from: u64, to: u64) -> ScheduleView {
    match store.load_schedule() {
        LoadedSchedule::Ok(s) => ScheduleView {
            status: "ok",
            error: None,
            occurrences: occurrences(&s, from, to),
            entries: s.entries,
        },
        LoadedSchedule::Corrupted(e) => ScheduleView {
            status: "corrupted",
            error: Some(ScheduleError::Corrupted(e).to_string()),
            entries: Vec::new(),
            occurrences: Vec::new(),
        },
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// **hermes_schedule_list** — the schedule and its occurrences in `[from, to)` (unix seconds).
#[tauri::command]
pub async fn hermes_schedule_list(
    app: tauri::AppHandle,
    from: u64,
    to: u64,
) -> Result<ScheduleView, String> {
    check_window(from, to)?;
    crate::blocking::off_main(move || {
        let store = ScheduleStore::for_app(&app)?;
        Ok(schedule_view(&store, from, to))
    })
    .await
}

/// **hermes_schedule_add** — the member adds an entry (origin is always `member` from the app).
#[tauri::command]
pub async fn hermes_schedule_add(app: tauri::AppHandle, entry: NewEntry) -> Result<Entry, String> {
    crate::blocking::off_main(move || {
        let store = ScheduleStore::for_app(&app)?;
        let entry = NewEntry {
            origin: Origin::Member,
            ..entry
        };
        store
            .add_entry(entry, now_secs())
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_schedule_remove** — remove an entry.
#[tauri::command]
pub async fn hermes_schedule_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::blocking::off_main(move || {
        ScheduleStore::for_app(&app)?
            .remove_entry(&id)
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_schedule_set_enabled** — pause or resume an entry.
#[tauri::command]
pub async fn hermes_schedule_set_enabled(
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        ScheduleStore::for_app(&app)?
            .set_entry_enabled(&id, enabled)
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_schedule_due** — the starts in `(after, upto]` (unix seconds) of enabled entries: what
/// became due since the caller's last check. This is the query the scheduler (HUP-S10.3) runs;
/// it only reads. A corrupted schedule has nothing due.
#[tauri::command]
pub async fn hermes_schedule_due(
    app: tauri::AppHandle,
    after: u64,
    upto: u64,
) -> Result<Vec<Occurrence>, String> {
    crate::blocking::off_main(move || Ok(ScheduleStore::for_app(&app)?.due_between(after, upto)))
        .await
}

/// **hermes_schedule_reset** — set a corrupted schedule aside and start empty.
#[tauri::command]
pub async fn hermes_schedule_reset(app: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || {
        ScheduleStore::for_app(&app)?
            .reset_corrupted(now_secs())
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_schedule_tests.rs");
}
