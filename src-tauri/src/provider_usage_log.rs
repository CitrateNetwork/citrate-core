//! D-11 (RC52) — the signed, tamper-evident local usage log for cloud model calls. ADR P-7 (d).
//!
//! One JSON line per record, in `<app data>/providers/usage.jsonl` (owner-only). Each line carries
//! the record, the previous line's MAC and its own MAC: `HMAC-SHA256(key, prev ‖ "\n" ‖ record)`,
//! under a 32-byte key kept in the OS keyring. Editing, deleting, inserting or reordering a line
//! breaks the chain from that point; the log then reports itself broken and **every cloud call is
//! refused** until the member starts a new log (the broken file is kept aside, never deleted).
//!
//! **No content.** A [`Record`] has no field for a prompt, a reply, a tool argument or a key; the
//! field set is closed (`deny_unknown_fields`) and pinned by `record_fields_are_a_closed_allowlist`.
//!
//! **Write-ahead.** A call writes a `begin` record (its worst-case reservation) before anything is
//! sent, and an `end` record (the vendor's reported tokens and the cost) before the reply is
//! returned. A call that cannot write either is refused / withheld. A `begin` with no `end` (a
//! crash mid-call) keeps counting its full reservation against the caps.
//!
//! The per-provider day and month totals the spend guard checks are computed from this log, so the
//! caps rest on the signed record rather than on a separate counter.
//!
//! **Anchor.** A chain detects edits, but not a file cut short or removed while the app is closed.
//! So every append also writes an [`Anchor`] to the OS keyring: the last sequence number and MAC,
//! the latest record time, and each provider's day and month totals. At open, a file shorter than
//! its anchor, one whose record at the anchored sequence has a different MAC, or a missing file
//! while the anchor is non-zero, is broken (every cloud call refused). A file one or more records
//! *ahead* of the anchor is a crash between the two writes and is accepted (records cannot be made
//! without the MAC key). A reset after a break carries the anchored day and month totals into the
//! new chain, so starting a new log never resets spend. The guard file's generation lives in the
//! same anchor ([`crate::provider_guard`]). The keyring is the trust root for both.

use crate::ai::AiKeyring;
use crate::provider_guard::{civil_of, day_of, month_of, PeriodUse};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zeroize::Zeroizing;

pub const LOG_FILE: &str = "usage.jsonl";
pub const LOG_MAC_ACCOUNT: &str = "provider-usage-log-mac-v1";
/// Keyring account holding the anchor: the log's last sequence number and MAC, its day and month
/// totals, and the guard file's generation. A file shorter than, or older than, its anchor fails
/// closed.
pub const ANCHOR_ACCOUNT: &str = "provider-anchor-v1";
const RECORD_VERSION: u32 = 1;
/// How far the clock may read before the last record before calls are refused (skew allowance).
pub const CLOCK_SKEW_MS: u64 = 10 * 60 * 1000;

/// One provider's totals as of its latest record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorTotals {
    pub day: u64,
    /// `year * 100 + month`.
    pub month: i64,
    pub day_tokens: u64,
    pub day_micros: u64,
    pub month_tokens: u64,
    pub month_micros: u64,
}

/// What the keyring remembers about the usage log and the guard file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    #[serde(default)]
    pub log_seq: u64,
    #[serde(default)]
    pub log_head: String,
    #[serde(default)]
    pub last_ts: u64,
    #[serde(default)]
    pub guard_gen: u64,
    #[serde(default)]
    pub totals: BTreeMap<String, AnchorTotals>,
}

/// The anchor, or `None` when the keyring has none (a fresh install, or one removed: a log or
/// guard file that exists without it fails closed).
pub fn read_anchor_opt(keyring: &dyn AiKeyring) -> Result<Option<Anchor>, ()> {
    match keyring.get(ANCHOR_ACCOUNT) {
        Err(_) => Err(()),
        Ok(None) => Ok(None),
        Ok(Some(s)) => serde_json::from_str(&s).map(Some).map_err(|_| ()),
    }
}

/// Read-modify-write an **existing** anchor. Callers hold the proxy's state lock, so writers do
/// not race. A missing anchor is an error: only a fresh install ([`create_anchor`]) and a log reset
/// ([`UsageLog::reset_broken`], the member's acknowledgement) may create one, so no other save can
/// quietly re-legitimise a log whose anchor was removed.
pub fn update_anchor(keyring: &dyn AiKeyring, f: impl FnOnce(&mut Anchor)) -> Result<(), ()> {
    let mut a = read_anchor_opt(keyring)?.ok_or(())?;
    f(&mut a);
    write_anchor(keyring, &a)
}

fn write_anchor(keyring: &dyn AiKeyring, a: &Anchor) -> Result<(), ()> {
    let s = serde_json::to_string(a).map_err(|_| ())?;
    keyring.set(ANCHOR_ACCOUNT, &s).map_err(|_| ())
}

/// A fresh install's empty anchor, written before any provider key exists.
pub(crate) fn create_anchor(keyring: &dyn AiKeyring) -> Result<(), ()> {
    write_anchor(keyring, &Anchor::default())
}

fn month_code(ms: u64) -> i64 {
    let (y, m) = month_of(ms);
    y * 100 + i64::from(m)
}
/// The MAC a chain starts from.
pub const GENESIS_PREV: &str = "genesis";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Before a model call is sent: its reservation.
    Begin,
    /// After it returned (or failed): the vendor's usage and the outcome.
    End,
    /// A call refused before anything was sent (cap, halt, egress).
    Refused,
    /// A live key test (a free vendor endpoint).
    KeyTest,
    /// A new chain started after a broken one (names the archived file).
    Reset,
    /// Spend carried into a new chain from the anchor (a reset never resets spend).
    Carry,
}

/// One usage record. Closed field set; no content, no key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub v: u32,
    pub seq: u64,
    pub ts: u64,
    pub kind: Kind,
    /// Call id (`pc-<hex>`), the same on a call's begin and end.
    pub call: String,
    pub provider: String,
    pub model: String,
    pub session: String,
    #[serde(default)]
    pub conversation: Option<String>,
    /// Whether the endpoint is a governed vendor (false for a custom endpoint).
    pub verified: bool,
    #[serde(default)]
    pub reserve_in: Option<u64>,
    #[serde(default)]
    pub reserve_out: Option<u64>,
    #[serde(default)]
    pub reserve_micros: Option<u64>,
    #[serde(default)]
    pub tokens_in: Option<u64>,
    #[serde(default)]
    pub tokens_out: Option<u64>,
    #[serde(default)]
    pub cost_micros: Option<u64>,
    /// `ok`, or a coarse failure / refusal code.
    #[serde(default)]
    pub outcome: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    r: Record,
    prev: String,
    mac: String,
}

type Mac = hmac::Hmac<sha2::Sha256>;

fn mac_hex(key: &[u8], prev: &str, record_json: &str) -> Option<String> {
    use hmac::Mac as _;
    let mut m = <Mac as hmac::Mac>::new_from_slice(key).ok()?;
    m.update(prev.as_bytes());
    m.update(b"\n");
    m.update(record_json.as_bytes());
    Some(hex::encode(m.finalize().into_bytes()))
}

/// Why the log cannot take a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    /// The keyring holding the log key could not be read.
    KeyUnavailable,
    /// The chain is broken at this line (1-based), or the file could not be read.
    Broken(String),
    /// The clock reads well before the last record.
    ClockBehind,
    /// The record could not be written.
    Write(String),
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogError::KeyUnavailable => f.write_str("the usage log key is unavailable"),
            LogError::Broken(m) => write!(f, "the usage log failed verification ({m})"),
            LogError::ClockBehind => f.write_str(
                "this computer's clock reads earlier than the last recorded call; check the date and time",
            ),
            LogError::Write(m) => write!(f, "the usage log could not be written ({m})"),
        }
    }
}

/// A call's contribution to the totals.
#[derive(Debug, Clone, Default)]
struct CallAgg {
    provider: String,
    ts: u64,
    reserve_tokens: u64,
    reserve_micros: Option<u64>,
    /// `Some` once the end record arrived.
    settled: Option<(u64, Option<u64>)>,
}

impl CallAgg {
    fn tokens(&self) -> u64 {
        self.settled.map(|(t, _)| t).unwrap_or(self.reserve_tokens)
    }
    fn micros(&self) -> Option<u64> {
        match self.settled {
            Some((_, m)) => m,
            None => self.reserve_micros,
        }
    }
}

pub struct UsageLog {
    path: PathBuf,
    keyring: Arc<dyn AiKeyring>,
    key: Option<Zeroizing<Vec<u8>>>,
    last_ts: u64,
    broken: Option<String>,
    head: String,
    next_seq: u64,
    calls: BTreeMap<String, CallAgg>,
    records: u64,
}

impl std::fmt::Debug for UsageLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsageLog")
            .field("path", &self.path)
            .field("broken", &self.broken)
            .field("records", &self.records)
            .finish_non_exhaustive()
    }
}

pub fn log_path(dir: &Path) -> PathBuf {
    dir.join(LOG_FILE)
}

impl UsageLog {
    /// Open and verify the log in `dir`. Never fails: a problem is recorded and reported by
    /// [`UsageLog::ready`], which refuses every call until it is resolved.
    pub fn open(dir: &Path, keyring: Arc<dyn AiKeyring>) -> UsageLog {
        let mut log = UsageLog {
            path: log_path(dir),
            keyring,
            key: None,
            last_ts: 0,
            broken: None,
            head: GENESIS_PREV.into(),
            next_seq: 1,
            calls: BTreeMap::new(),
            records: 0,
        };
        let Ok(key) = crate::provider_guard::mac_key(log.keyring.as_ref(), LOG_MAC_ACCOUNT) else {
            return log;
        };
        let Ok(anchor) = read_anchor_opt(log.keyring.as_ref()) else {
            return log;
        };
        log.key = Some(key);
        let has_records = std::fs::metadata(&log.path).is_ok_and(|m| m.len() > 0);
        if anchor.is_none() && has_records {
            log.broken = Some("the usage log anchor is missing".into());
            return log;
        }
        log.replay(&anchor.unwrap_or_default());
        log
    }

    fn replay(&mut self, anchor: &Anchor) {
        let Some(key) = self.key.clone() else {
            return;
        };
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if anchor.log_seq > 0 {
                    self.broken = Some(format!(
                        "the log file is missing; {} records were written",
                        anchor.log_seq
                    ));
                }
                return;
            }
            Err(e) => {
                self.broken = Some(format!("unreadable: {}", e.kind()));
                return;
            }
        };
        for (i, raw) in text.lines().enumerate() {
            let n = i + 1;
            let Ok(line) = serde_json::from_str::<Line>(raw) else {
                self.broken = Some(format!("line {n} is not a record"));
                return;
            };
            let Ok(rj) = serde_json::to_string(&line.r) else {
                self.broken = Some(format!("line {n} could not be re-encoded"));
                return;
            };
            let expect = mac_hex(&key, &self.head, &rj);
            if line.prev != self.head || expect.as_deref() != Some(line.mac.as_str()) {
                self.broken = Some(format!("line {n} does not match the chain"));
                return;
            }
            if line.r.seq != self.next_seq || line.r.v != RECORD_VERSION {
                self.broken = Some(format!("line {n} is out of sequence"));
                return;
            }
            if line.r.seq == anchor.log_seq && line.mac != anchor.log_head {
                self.broken = Some(format!("line {n} is not the anchored record"));
                return;
            }
            self.apply(&line.r);
            self.head = line.mac;
            self.next_seq += 1;
            self.records += 1;
        }
        if !text.is_empty() && !text.ends_with('\n') {
            self.broken = Some("the last line is incomplete".into());
            return;
        }
        if self.records < anchor.log_seq {
            self.broken = Some(format!(
                "the log ends at record {} but {} were written",
                self.records, anchor.log_seq
            ));
        }
    }

    fn apply(&mut self, r: &Record) {
        self.last_ts = self.last_ts.max(r.ts);
        match r.kind {
            Kind::Carry => {
                self.calls.insert(
                    r.call.clone(),
                    CallAgg {
                        provider: r.provider.clone(),
                        ts: r.ts,
                        reserve_tokens: r.tokens_in.unwrap_or(0),
                        reserve_micros: r.cost_micros,
                        settled: Some((r.tokens_in.unwrap_or(0), r.cost_micros)),
                    },
                );
            }
            Kind::Begin => {
                self.calls.insert(
                    r.call.clone(),
                    CallAgg {
                        provider: r.provider.clone(),
                        ts: r.ts,
                        reserve_tokens: r
                            .reserve_in
                            .unwrap_or(0)
                            .saturating_add(r.reserve_out.unwrap_or(0)),
                        reserve_micros: r.reserve_micros,
                        settled: None,
                    },
                );
            }
            Kind::End => {
                if let Some(c) = self.calls.get_mut(&r.call) {
                    let tokens = match (r.tokens_in, r.tokens_out) {
                        (Some(i), Some(o)) => i.saturating_add(o),
                        // Usage not reported: the reservation stands.
                        _ => c.reserve_tokens,
                    };
                    let micros = r.cost_micros.or(if r.tokens_in.is_none() {
                        c.reserve_micros
                    } else {
                        None
                    });
                    c.settled = Some((tokens, micros));
                }
            }
            Kind::Refused | Kind::KeyTest | Kind::Reset => {}
        }
    }

    /// Whether a record can be written now. Every cloud call checks this first.
    pub fn ready(&self) -> Result<(), LogError> {
        if self.key.is_none() {
            return Err(LogError::KeyUnavailable);
        }
        if let Some(b) = &self.broken {
            return Err(LogError::Broken(b.clone()));
        }
        Ok(())
    }

    /// Refuse when the clock reads well before the log's last record (a clock moved back would
    /// move calls out of the current day or month).
    pub fn check_clock(&self, now_ms: u64) -> Result<(), LogError> {
        if now_ms.saturating_add(CLOCK_SKEW_MS) < self.last_ts {
            return Err(LogError::ClockBehind);
        }
        Ok(())
    }

    /// Mark the log broken from outside (the proxy found its anchor removed): every write is
    /// refused until the member starts a new log.
    pub(crate) fn mark_broken(&mut self, reason: &str) {
        if self.broken.is_none() {
            self.broken = Some(reason.to_string());
        }
    }

    /// Whether spend earlier in the UTC (day, month) of `now_ms` is unknown because the log was
    /// reset without its anchor in that period.
    pub fn earlier_spend_unknown(&self, now_ms: u64) -> (bool, bool) {
        let _ = now_ms;
        (false, false)
    }

    pub fn broken_reason(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    pub fn records(&self) -> u64 {
        self.records
    }

    /// Append one record (seq and version are filled in). Durable (fsync) before it returns.
    pub fn append(&mut self, mut r: Record) -> Result<(), LogError> {
        self.ready()?;
        let key = self.key.clone().ok_or(LogError::KeyUnavailable)?;
        r.v = RECORD_VERSION;
        r.seq = self.next_seq;
        let rj = serde_json::to_string(&r).map_err(|_| LogError::Write("encode".into()))?;
        let mac = mac_hex(&key, &self.head, &rj).ok_or(LogError::Write("seal".into()))?;
        let line = serde_json::to_string(&Line {
            r: r.clone(),
            prev: self.head.clone(),
            mac: mac.clone(),
        })
        .map_err(|_| LogError::Write("encode".into()))?;
        let io = |e: std::io::Error| LogError::Write(e.kind().to_string());
        if let Some(dir) = self.path.parent() {
            citrate_core_kit::fsutil::ensure_private_dir(dir).map_err(io)?;
        }
        let mut f = if self.path.exists() {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&self.path)
                .map_err(io)?
        } else {
            let f = citrate_core_kit::fsutil::create_secret_file(&self.path).map_err(io)?;
            drop(f);
            std::fs::OpenOptions::new()
                .append(true)
                .open(&self.path)
                .map_err(io)?
        };
        f.write_all(format!("{line}\n").as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)?;
        self.apply(&r);
        self.head = mac;
        self.next_seq += 1;
        self.records += 1;
        // The anchor follows the file. If it cannot be written the record still stands (the file
        // is ahead of the anchor, which open accepts), but the caller treats the append as failed.
        let (seq, head, last_ts) = (r.seq, self.head.clone(), self.last_ts);
        // Totals as of the latest record time (a carried record may be dated earlier).
        let totals = (!r.provider.is_empty()).then(|| {
            let (d, m) = self.period_use(&r.provider, last_ts);
            AnchorTotals {
                day: day_of(last_ts),
                month: month_code(last_ts),
                day_tokens: d.tokens,
                day_micros: d.micros,
                month_tokens: m.tokens,
                month_micros: m.micros,
            }
        });
        update_anchor(self.keyring.as_ref(), |a| {
            a.log_seq = seq;
            a.log_head = head;
            a.last_ts = a.last_ts.max(last_ts);
            if let Some(t) = totals {
                a.totals.insert(r.provider.clone(), t);
            }
        })
        .map_err(|_| LogError::Write("the usage log anchor could not be written".into()))
    }

    /// What this provider used in the UTC day and UTC month of `now_ms`, unsettled reservations
    /// included at their full size.
    pub fn period_use(&self, provider: &str, now_ms: u64) -> (PeriodUse, PeriodUse) {
        let (day, month) = (day_of(now_ms), month_of(now_ms));
        let mut d = PeriodUse::default();
        let mut m = PeriodUse::default();
        for c in self.calls.values().filter(|c| c.provider == provider) {
            let in_month = month_of(c.ts) == month;
            if !in_month {
                continue;
            }
            let add = |p: &mut PeriodUse| {
                p.tokens = p.tokens.saturating_add(c.tokens());
                match c.micros() {
                    Some(x) => p.micros = p.micros.saturating_add(x),
                    None => p.unpriced_calls += 1,
                }
            };
            add(&mut m);
            if day_of(c.ts) == day {
                add(&mut d);
            }
        }
        (d, m)
    }

    /// Calls with a begin but no end (in flight, or interrupted by a crash).
    pub fn unsettled(&self, provider: &str) -> u64 {
        self.calls
            .values()
            .filter(|c| c.provider == provider && c.settled.is_none())
            .count() as u64
    }

    /// Calls this provider made in the UTC day / month of `now_ms`.
    pub fn call_counts(&self, provider: &str, now_ms: u64) -> (u64, u64) {
        let (day, month) = (day_of(now_ms), month_of(now_ms));
        let mut out = (0, 0);
        for c in self.calls.values().filter(|c| c.provider == provider) {
            if month_of(c.ts) == month {
                out.1 += 1;
                if day_of(c.ts) == day {
                    out.0 += 1;
                }
            }
        }
        out
    }

    /// Start a new chain after a broken one. The broken file is renamed aside (never deleted) and
    /// the new chain's first record names it. Refused while the log is intact.
    pub fn reset_broken(&mut self, now_ms: u64) -> Result<String, LogError> {
        if self.key.is_none() {
            return Err(LogError::KeyUnavailable);
        }
        if self.broken.is_none() {
            return Err(LogError::Write(
                "the log is intact; nothing to reset".into(),
            ));
        }
        let aside = self
            .path
            .with_file_name(format!("usage.broken-{now_ms}.jsonl"));
        if self.path.exists() {
            std::fs::rename(&self.path, &aside)
                .map_err(|e| LogError::Write(e.kind().to_string()))?;
        }
        let aside_name = aside
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        // The reset is the one path besides a fresh install that may (re)create the anchor.
        let mut anchor = read_anchor_opt(self.keyring.as_ref())
            .map_err(|_| LogError::KeyUnavailable)?
            .unwrap_or_default();
        let carried = std::mem::take(&mut anchor.totals);
        anchor.log_seq = 0;
        anchor.log_head = String::new();
        write_anchor(self.keyring.as_ref(), &anchor).map_err(|_| LogError::KeyUnavailable)?;
        self.broken = None;
        self.head = GENESIS_PREV.into();
        self.next_seq = 1;
        self.calls.clear();
        self.records = 0;
        self.append(Record {
            v: RECORD_VERSION,
            seq: 0,
            ts: now_ms,
            kind: Kind::Reset,
            call: String::new(),
            provider: String::new(),
            model: String::new(),
            session: String::new(),
            conversation: None,
            verified: false,
            reserve_in: None,
            reserve_out: None,
            reserve_micros: None,
            tokens_in: None,
            tokens_out: None,
            cost_micros: None,
            outcome: Some(format!("archived {aside_name}")),
        })?;
        // Carry this month's anchored spend into the new chain: today's part at `now`, the rest of
        // the month at the month's first instant (or at `now` on the 1st, which only over-counts).
        let (day, month) = (day_of(now_ms), month_code(now_ms));
        let (_, _, dom) = civil_of(now_ms);
        let month_start = (day - (u64::from(dom) - 1)) * crate::provider_guard::DAY_MS;
        for (provider, t) in carried.into_iter().filter(|(_, t)| t.month == month) {
            let (d_tok, d_mic) = if t.day == day {
                (t.day_tokens, t.day_micros)
            } else {
                (0, 0)
            };
            let parts = [
                (now_ms, d_tok, d_mic),
                (
                    if month_start < day * crate::provider_guard::DAY_MS {
                        month_start
                    } else {
                        now_ms
                    },
                    t.month_tokens.saturating_sub(d_tok),
                    t.month_micros.saturating_sub(d_mic),
                ),
            ];
            for (i, (ts, tokens, micros)) in parts.into_iter().enumerate() {
                if tokens == 0 && micros == 0 {
                    continue;
                }
                let mut r = record(
                    Kind::Carry,
                    &format!("carry-{i}-{provider}"),
                    &provider,
                    "",
                    "reset",
                    ts,
                );
                r.tokens_in = Some(tokens);
                r.cost_micros = Some(micros);
                r.outcome = Some("carried".into());
                self.append(r)?;
            }
        }
        Ok(aside_name)
    }
}

/// A blank record of `kind` for `call` (the caller fills what applies).
pub fn record(
    kind: Kind,
    call: &str,
    provider: &str,
    model: &str,
    session: &str,
    ts: u64,
) -> Record {
    Record {
        v: RECORD_VERSION,
        seq: 0,
        ts,
        kind,
        call: call.into(),
        provider: provider.into(),
        model: model.into(),
        session: session.into(),
        conversation: None,
        verified: false,
        reserve_in: None,
        reserve_out: None,
        reserve_micros: None,
        tokens_in: None,
        tokens_out: None,
        cost_micros: None,
        outcome: None,
    }
}

#[cfg(test)]
mod tests {
    include!("provider_usage_log_tests.rs");
}
