// D-11 — the usage log: an HMAC chain that any edit breaks, write-ahead begin/end records that
// carry no content, and the day/month totals the caps rest on.

use super::*;
use crate::provider_fixtures_tests::{MemKeyring, TmpDir};
use std::sync::Arc;
use crate::provider_guard::DAY_MS;

const T0: u64 = 20_752 * DAY_MS + 3_600_000; // 2026-10-26 01:00 UTC

fn begin(call: &str, provider: &str, ts: u64, rin: u64, rout: u64, rmicros: Option<u64>) -> Record {
    let mut r = record(Kind::Begin, call, provider, "m1", "sess-1", ts);
    r.reserve_in = Some(rin);
    r.reserve_out = Some(rout);
    r.reserve_micros = rmicros;
    r.verified = true;
    r
}

fn end(call: &str, provider: &str, ts: u64, usage: Option<(u64, u64)>, cost: Option<u64>) -> Record {
    let mut r = record(Kind::End, call, provider, "m1", "sess-1", ts);
    r.tokens_in = usage.map(|u| u.0);
    r.tokens_out = usage.map(|u| u.1);
    r.cost_micros = cost;
    r.outcome = Some("ok".into());
    r
}

fn three_calls(dir: &TmpDir, kr: &Arc<MemKeyring>) -> UsageLog {
    let mut log = UsageLog::open(&dir.0, kr.clone());
    log.ready().unwrap();
    log.append(begin("c1", "openai", T0, 100, 50, Some(10))).unwrap();
    log.append(end("c1", "openai", T0 + 5, Some((40, 20)), Some(4))).unwrap();
    log.append(begin("c2", "openai", T0 + 10, 1_000, 500, None)).unwrap();
    log
}

#[test]
fn the_chain_verifies_across_reopen() {
    let dir = TmpDir::new("log-chain");
    let kr = installed_kr();
    drop(three_calls(&dir, &kr));
    let log = UsageLog::open(&dir.0, kr.clone());
    assert!(log.ready().is_ok(), "{:?}", log.broken_reason());
    assert_eq!(log.records(), 3);
    // Settled c1 counts its actual 60 tokens; unsettled c2 counts its full 1,500 reservation.
    let (d, m) = log.period_use("openai", T0 + 20);
    assert_eq!(d.tokens, 60 + 1_500);
    assert_eq!(m.tokens, 60 + 1_500);
    assert_eq!(d.micros, 4);
    assert_eq!(d.unpriced_calls, 1);
    assert_eq!(log.unsettled("openai"), 1);
    assert_eq!(log.call_counts("openai", T0 + 20), (2, 2));
}

fn reopen_after(dir: &TmpDir, kr: &Arc<MemKeyring>, edit: impl Fn(Vec<String>) -> Vec<String>) -> UsageLog {
    let p = log_path(&dir.0);
    let lines: Vec<String> = std::fs::read_to_string(&p)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    let mut out = edit(lines).join("\n");
    out.push('\n');
    std::fs::write(&p, out).unwrap();
    UsageLog::open(&dir.0, kr.clone())
}

#[test]
fn an_edited_record_breaks_the_chain_and_refuses_every_write() {
    let dir = TmpDir::new("log-edit");
    let kr = installed_kr();
    drop(three_calls(&dir, &kr));
    let mut log = reopen_after(&dir, &kr, |mut l| {
        l[1] = l[1].replace("\"tokensIn\":40", "\"tokensIn\":1");
        l
    });
    assert!(matches!(log.ready(), Err(LogError::Broken(_))), "an edited record must break the chain");
    assert!(log.append(begin("c3", "openai", T0, 1, 1, None)).is_err());
}

#[test]
fn a_deleted_or_reordered_or_cut_line_breaks_the_chain() {
    for (tag, edit) in [
        ("delete", Box::new(|mut l: Vec<String>| {
            l.remove(1);
            l
        }) as Box<dyn Fn(Vec<String>) -> Vec<String>>),
        ("reorder", Box::new(|mut l: Vec<String>| {
            l.swap(1, 2);
            l
        })),
        ("drop-first", Box::new(|mut l: Vec<String>| {
            l.remove(0);
            l
        })),
        ("cut", Box::new(|mut l: Vec<String>| {
            let last = l.len() - 1;
            l[last].truncate(20);
            l
        })),
    ] {
        let dir = TmpDir::new(&format!("log-{tag}"));
        let kr = installed_kr();
        drop(three_calls(&dir, &kr));
        let log = reopen_after(&dir, &kr, edit);
        assert!(log.ready().is_err(), "{tag}: the chain must break");
    }
}

#[test]
fn a_different_key_or_no_key_cannot_use_the_log() {
    let dir = TmpDir::new("log-key");
    let kr = installed_kr();
    drop(three_calls(&dir, &kr));
    let other = Arc::new(MemKeyring::default());
    assert!(UsageLog::open(&dir.0, other.clone()).ready().is_err(), "another key must not verify");
    let log = UsageLog::open(&dir.0, Arc::new(MemKeyring::failing()));
    assert_eq!(log.ready(), Err(LogError::KeyUnavailable));
}

#[test]
fn end_without_reported_usage_keeps_the_reservation() {
    let dir = TmpDir::new("log-nousage");
    let kr = installed_kr();
    let mut log = UsageLog::open(&dir.0, kr.clone());
    log.append(begin("c1", "xai", T0, 300, 200, Some(9))).unwrap();
    log.append(end("c1", "xai", T0 + 1, None, None)).unwrap();
    let (d, _) = log.period_use("xai", T0 + 2);
    assert_eq!(d.tokens, 500);
    assert_eq!(d.micros, 9);
    assert_eq!(log.unsettled("xai"), 0);
    // Other providers are not mixed in.
    assert_eq!(log.period_use("openai", T0 + 2).0.tokens, 0);
}

#[test]
fn totals_follow_utc_day_and_month() {
    let dir = TmpDir::new("log-periods");
    let kr = installed_kr();
    let mut log = UsageLog::open(&dir.0, kr.clone());
    let yesterday = T0 - DAY_MS;
    let last_month = 20_727 * DAY_MS; // 2026-10-01 00:00 UTC is day 20727; a day before is September
    log.append(begin("a", "openai", yesterday, 10, 0, None)).unwrap();
    log.append(begin("b", "openai", T0, 20, 0, None)).unwrap();
    log.append(begin("c", "openai", last_month - 1, 40, 0, None)).unwrap();
    let (d, m) = log.period_use("openai", T0);
    assert_eq!(d.tokens, 20);
    assert_eq!(m.tokens, 30);
}

#[test]
fn reset_starts_a_new_chain_only_after_a_break_and_keeps_the_old_file() {
    let dir = TmpDir::new("log-reset");
    let kr = installed_kr();
    let mut log = three_calls(&dir, &kr);
    assert!(log.reset_broken(T0).is_err(), "an intact log is never reset");
    let mut log2 = reopen_after(&dir, &kr, |mut l| {
        l.remove(0);
        l
    });
    drop(log);
    assert!(log2.ready().is_err());
    let aside = log2.reset_broken(T0 + 99).unwrap();
    assert!(dir.0.join(&aside).exists(), "the broken log is kept aside");
    assert!(log2.ready().is_ok());
    // The new chain: the reset record, then today's carried spend (one record: all of it is today).
    assert_eq!(log2.records(), 2);
    let log3 = UsageLog::open(&dir.0, kr.clone());
    assert!(log3.ready().is_ok());
    // The anchored totals (60 settled + 1,500 reserved) come across; nothing is reset to zero.
    assert_eq!(log3.period_use("openai", T0 + 100).0.tokens, 60 + 1_500);
}

#[test]
fn record_fields_are_a_closed_allowlist() {
    // Tripwire: the usage log has no place for content or keys. A new field must be added here on
    // purpose (and must not be content).
    const ALLOWED: &[&str] = &[
        "v", "seq", "ts", "kind", "call", "provider", "model", "session", "conversation",
        "verified", "reserveIn", "reserveOut", "reserveMicros", "tokensIn", "tokensOut",
        "costMicros", "outcome",
    ];
    let v = serde_json::to_value(record(Kind::End, "c", "p", "m", "s", 1)).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    for k in &keys {
        assert!(ALLOWED.contains(k), "unexpected usage-log field {k:?}");
    }
    assert_eq!(keys.len(), ALLOWED.len());
    // And a record with an extra field (say, a prompt) does not even parse.
    let mut with_prompt = v.clone();
    with_prompt["prompt"] = serde_json::json!("hello");
    assert!(serde_json::from_value::<Record>(with_prompt).is_err());
}

#[cfg(unix)]
#[test]
fn the_log_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TmpDir::new("log-mode");
    let kr = installed_kr();
    drop(three_calls(&dir, &kr));
    let mode = std::fs::metadata(log_path(&dir.0)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn dropping_the_last_lines_or_the_whole_file_is_detected() {
    // Trailing lines removed while the app is closed.
    for n in [1usize, 2, 3] {
        let dir = TmpDir::new(&format!("log-tail-{n}"));
        let kr = installed_kr();
        drop(three_calls(&dir, &kr));
        let log = reopen_after(&dir, &kr, |mut l| {
            l.truncate(l.len() - n);
            l
        });
        assert!(
            matches!(log.ready(), Err(LogError::Broken(_))),
            "dropping the last {n} line(s) must break the log"
        );
    }
    // The whole file removed after records were written.
    let dir = TmpDir::new("log-gone");
    let kr = installed_kr();
    drop(three_calls(&dir, &kr));
    std::fs::remove_file(log_path(&dir.0)).unwrap();
    let log = UsageLog::open(&dir.0, kr.clone());
    assert!(matches!(log.ready(), Err(LogError::Broken(_))), "a removed log must not reopen empty");
    // A brand-new install (no anchor, no file) is fine.
    let fresh = TmpDir::new("log-fresh");
    assert!(UsageLog::open(&fresh.0, Arc::new(MemKeyring::default())).ready().is_ok());
}

#[test]
fn a_crash_between_the_append_and_the_anchor_update_is_not_a_break() {
    // The file may be one record ahead of the anchor (written, then the app stopped).
    let dir = TmpDir::new("log-ahead");
    let kr = installed_kr();
    drop(three_calls(&dir, &kr));
    let anchor = kr.map.lock().unwrap().get(ANCHOR_ACCOUNT).cloned().unwrap_or_default();
    let mut log = UsageLog::open(&dir.0, kr.clone());
    log.append(end("c2", "openai", T0 + 11, Some((1, 1)), None)).unwrap();
    drop(log);
    kr.map
        .lock()
        .unwrap()
        .insert(ANCHOR_ACCOUNT.to_string(), anchor);
    let log = UsageLog::open(&dir.0, kr.clone());
    assert!(log.ready().is_ok(), "{:?}", log.broken_reason());
    assert_eq!(log.records(), 4);
}

#[test]
fn a_reset_carries_the_day_and_month_totals_forward() {
    let dir = TmpDir::new("log-carry");
    let kr = installed_kr();
    let log = three_calls(&dir, &kr);
    let (d0, m0) = log.period_use("openai", T0 + 20);
    drop(log);
    std::fs::remove_file(log_path(&dir.0)).unwrap();
    let mut log = UsageLog::open(&dir.0, kr.clone());
    assert!(log.ready().is_err());
    log.reset_broken(T0 + 30).unwrap();
    let (d, m) = log.period_use("openai", T0 + 40);
    assert!(d.tokens >= d0.tokens && m.tokens >= m0.tokens, "a reset must not reset spend: {d:?} {m:?}");
    assert!(d.micros >= d0.micros && m.micros >= m0.micros);
    // And the carry survives a reopen.
    let log = UsageLog::open(&dir.0, kr.clone());
    assert!(log.ready().is_ok());
    assert!(log.period_use("openai", T0 + 40).1.tokens >= m0.tokens);
}

#[test]
fn a_clock_well_behind_the_last_record_is_refused() {
    let dir = TmpDir::new("log-clock");
    let kr = installed_kr();
    let log = three_calls(&dir, &kr);
    assert!(log.check_clock(T0 + 20).is_ok());
    assert!(log.check_clock(T0 - 60_000).is_ok(), "small skew is tolerated");
    assert!(log.check_clock(T0 - DAY_MS).is_err(), "a clock moved back a day is refused");
}

#[test]
fn a_log_with_records_but_no_anchor_fails_closed() {
    // Only the anchor removed, then the log cut short: without this check it would pass as a
    // fresh install with lower totals.
    let dir = TmpDir::new("log-noanchor");
    let kr = installed_kr();
    drop(three_calls(&dir, &kr));
    kr.map.lock().unwrap().remove(ANCHOR_ACCOUNT);
    let log = reopen_after(&dir, &kr, |mut l| {
        l.truncate(1);
        l
    });
    assert!(matches!(log.ready(), Err(LogError::Broken(_))), "a log with records needs its anchor");
    // An empty log file with no anchor is still a fresh install.
    let fresh = TmpDir::new("log-noanchor-empty");
    std::fs::write(log_path(&fresh.0), "").unwrap();
    assert!(UsageLog::open(&fresh.0, Arc::new(MemKeyring::default())).ready().is_ok());
}

/// A keychain as a fresh install leaves it: the empty anchor `ModelProxy::new` writes first.
fn installed_kr() -> Arc<MemKeyring> {
    let kr = Arc::new(MemKeyring::default());
    crate::provider_usage_log::create_anchor(kr.as_ref()).unwrap();
    kr
}
