//! Startup crash-loop detection for a supervised sidecar.
//!
//! The supervisor restarts a child that exits with bounded backoff ([`crate::supervisor`]). When
//! the child exits the same way again and again (0.5.7: the node exiting on start with
//! "Too many open files" because macOS gave the app a soft `RLIMIT_NOFILE` of 256), a restart that
//! only shows "Starting" hides the cause from the member. This module keeps a short, bounded
//! history of crashes and names a loop once the same error class repeats.
//!
//! Data source (Rule 7): the crash record the supervisor writes on every unexpected exit, whose
//! `stderr_tail` is the child's own stderr from the sidecar log ring. Nothing here is inferred
//! from anything else.
//!
//! A loop is [`CRASH_LOOP_THRESHOLD`] or more crashes of one class in a row, each within
//! [`CRASH_LOOP_GAP_MS`] of the one before. It ends when a crash of another class breaks the run,
//! or when the child has stayed up for more than the gap since the last crash.

use std::collections::VecDeque;

/// Crashes of one class in a row that make a loop.
pub const CRASH_LOOP_THRESHOLD: u32 = 3;
/// The longest gap between two crashes of one loop (10 minutes). Longer than the supervisor's
/// backoff cap (30 s) and its slow retry, so a loop that reached `Failed` stays one loop.
pub const CRASH_LOOP_GAP_MS: u64 = 10 * 60_000;
/// Crashes kept (drop-oldest), so a long loop cannot grow the history.
pub const CRASH_HISTORY_CAPACITY: usize = 16;
/// The longest error line kept, in bytes (cut on a char boundary).
pub const CRASH_LINE_MAX_BYTES: usize = 400;

/// What kind of error a crash ended with. `TooManyOpenFiles` gets its own guidance in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CrashKind {
    /// EMFILE: the process hit its open-file limit ("Too many open files", "os error 24").
    TooManyOpenFiles,
    /// Any other error.
    Other,
}

/// A startup crash loop: the same error, `count` times in a row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashLoop {
    pub kind: CrashKind,
    /// The child's own error line from the latest crash (bounded, see [`CRASH_LINE_MAX_BYTES`]).
    pub line: String,
    /// Crashes in the run.
    pub count: u32,
    /// The first and the latest crash of the run, unix ms on the supervisor's clock.
    pub first_at_unix_ms: u64,
    pub last_at_unix_ms: u64,
}

/// The error one crash ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashError {
    pub kind: CrashKind,
    /// What two crashes must share to be "the same error": the kind for a known kind, else the
    /// line with digit runs folded (so a pid, a path number or a port does not split a loop).
    pub class: String,
    /// The line itself, bounded.
    pub line: String,
}

fn is_emfile(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("too many open files") || l.contains("os error 24") || l.contains("emfile")
}

fn looks_like_error(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("error") || l.contains("panicked") || l.contains("fatal")
}

fn bounded(line: &str) -> String {
    if line.len() <= CRASH_LINE_MAX_BYTES {
        return line.to_string();
    }
    let mut end = CRASH_LINE_MAX_BYTES;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_string()
}

fn fold_digits(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_digits = false;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            out.push(ch);
            in_digits = false;
        }
    }
    out
}

/// The error a crash ended with, from its stderr tail. The tail is read from the latest error
/// report: the last line that starts one ("Error..." or a panic), else the last line that reads as
/// an error. If that report names EMFILE anywhere ("Error: ...\nCaused by: ... Too many open
/// files"), the crash is [`CrashKind::TooManyOpenFiles`] with that line; otherwise the report's
/// first line is the error. No error line at all: the last non-empty line, else the rendered exit
/// status (`exit`, e.g. "exit code: 1"). Reading from the latest report means an earlier
/// incarnation's lines, still in the shared log ring, never decide a later crash's kind.
pub fn classify(stderr_tail: &str, exit: &str) -> CrashError {
    let lines: Vec<&str> = stderr_tail
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let starts_report = |l: &str| {
        l.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("error")) || l.contains("panicked")
    };
    let head = lines
        .iter()
        .rposition(|l| starts_report(l))
        .or_else(|| lines.iter().rposition(|l| looks_like_error(l)));
    let Some(head) = head else {
        let line = lines
            .last()
            .map(|l| bounded(l))
            .unwrap_or_else(|| bounded(exit.trim()));
        return other(line);
    };
    if let Some(l) = lines[head..].iter().rev().find(|l| is_emfile(l)) {
        return CrashError {
            kind: CrashKind::TooManyOpenFiles,
            class: "too-many-open-files".to_string(),
            line: bounded(l),
        };
    }
    other(bounded(lines[head]))
}

fn other(line: String) -> CrashError {
    CrashError {
        kind: CrashKind::Other,
        class: format!("other:{}", fold_digits(&line)),
        line,
    }
}

#[derive(Debug, Clone)]
struct Entry {
    at_unix_ms: u64,
    error: CrashError,
}

/// A bounded history of a supervisor's crashes, most recent last.
#[derive(Debug, Clone, Default)]
pub struct CrashHistory {
    entries: VecDeque<Entry>,
}

impl CrashHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one crash: its time and the child's stderr tail and exit status.
    pub fn record(&mut self, at_unix_ms: u64, stderr_tail: &str, exit: &str) {
        if self.entries.len() >= CRASH_HISTORY_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            at_unix_ms,
            error: classify(stderr_tail, exit),
        });
    }

    /// The loop the latest crashes form, as seen at `now_unix_ms`. `alive` is whether a child is
    /// running now: a child up for longer than the gap since the last crash has ended the loop.
    pub fn crash_loop(&self, now_unix_ms: u64, alive: bool) -> Option<CrashLoop> {
        let last = self.entries.back()?;
        if alive && now_unix_ms.saturating_sub(last.at_unix_ms) > CRASH_LOOP_GAP_MS {
            return None;
        }
        let mut count: u32 = 1;
        let mut first = last.at_unix_ms;
        let mut later = last.at_unix_ms;
        for e in self.entries.iter().rev().skip(1) {
            if e.error.class != last.error.class
                || later.saturating_sub(e.at_unix_ms) > CRASH_LOOP_GAP_MS
            {
                break;
            }
            count += 1;
            first = e.at_unix_ms;
            later = e.at_unix_ms;
        }
        (count >= CRASH_LOOP_THRESHOLD).then(|| CrashLoop {
            kind: last.error.kind,
            line: last.error.line.clone(),
            count,
            first_at_unix_ms: first,
            last_at_unix_ms: last.at_unix_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    include!("crash_loop_tests.rs");
}
