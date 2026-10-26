// D-11 — the usage log: an HMAC chain that any edit breaks, write-ahead begin/end records that
// carry no content, and the day/month totals the caps rest on.

use super::*;
use crate::provider_fixtures_tests::{MemKeyring, TmpDir};
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

fn three_calls(dir: &TmpDir, kr: &MemKeyring) -> UsageLog {
    let mut log = UsageLog::open(&dir.0, kr);
    log.ready().unwrap();
    log.append(begin("c1", "openai", T0, 100, 50, Some(10))).unwrap();
    log.append(end("c1", "openai", T0 + 5, Some((40, 20)), Some(4))).unwrap();
    log.append(begin("c2", "openai", T0 + 10, 1_000, 500, None)).unwrap();
    log
}

#[test]
fn the_chain_verifies_across_reopen() {
    let dir = TmpDir::new("log-chain");
    let kr = MemKeyring::default();
    drop(three_calls(&dir, &kr));
    let log = UsageLog::open(&dir.0, &kr);
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

fn reopen_after(dir: &TmpDir, kr: &MemKeyring, edit: impl Fn(Vec<String>) -> Vec<String>) -> UsageLog {
    let p = log_path(&dir.0);
    let lines: Vec<String> = std::fs::read_to_string(&p)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    let mut out = edit(lines).join("\n");
    out.push('\n');
    std::fs::write(&p, out).unwrap();
    UsageLog::open(&dir.0, kr)
}

#[test]
fn an_edited_record_breaks_the_chain_and_refuses_every_write() {
    let dir = TmpDir::new("log-edit");
    let kr = MemKeyring::default();
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
        let kr = MemKeyring::default();
        drop(three_calls(&dir, &kr));
        let log = reopen_after(&dir, &kr, edit);
        assert!(log.ready().is_err(), "{tag}: the chain must break");
    }
}

#[test]
fn a_different_key_or_no_key_cannot_use_the_log() {
    let dir = TmpDir::new("log-key");
    let kr = MemKeyring::default();
    drop(three_calls(&dir, &kr));
    let other = MemKeyring::default();
    assert!(UsageLog::open(&dir.0, &other).ready().is_err(), "another key must not verify");
    let log = UsageLog::open(&dir.0, &MemKeyring::failing());
    assert_eq!(log.ready(), Err(LogError::KeyUnavailable));
}

#[test]
fn end_without_reported_usage_keeps_the_reservation() {
    let dir = TmpDir::new("log-nousage");
    let kr = MemKeyring::default();
    let mut log = UsageLog::open(&dir.0, &kr);
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
    let kr = MemKeyring::default();
    let mut log = UsageLog::open(&dir.0, &kr);
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
    let kr = MemKeyring::default();
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
    assert_eq!(log2.records(), 1);
    let log3 = UsageLog::open(&dir.0, &kr);
    assert!(log3.ready().is_ok());
    assert_eq!(log3.period_use("openai", T0).0.tokens, 0);
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
    let kr = MemKeyring::default();
    drop(three_calls(&dir, &kr));
    let mode = std::fs::metadata(log_path(&dir.0)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn recent_calls_are_newest_first_settle_on_end_and_survive_reopen() {
    let dir = TmpDir::new("ulog-recent");
    let kr = MemKeyring::default();
    let mut log = three_calls(&dir, &kr);
    let mut refused = record(Kind::Refused, "c3", "anthropic", "m2", "sess-1", T0 + 20);
    refused.outcome = Some("cap_exceeded".into());
    log.append(refused).unwrap();
    let check = |log: &UsageLog| {
        let r = log.recent();
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].provider.as_str(), r[0].refused, r[0].outcome.as_deref()), ("anthropic", true, Some("cap_exceeded")));
        assert_eq!((r[1].at_ms, r[1].outcome.as_deref(), r[1].tokens_in), (T0 + 10, None, None), "c2 has no end: unsettled");
        assert_eq!(r[2].outcome.as_deref(), Some("ok"));
        assert_eq!((r[2].tokens_in, r[2].tokens_out, r[2].cost_micros), (Some(40), Some(20), Some(4)));
        assert!(r[2].verified && !r[2].refused);
    };
    check(&log);
    check(&UsageLog::open(&dir.0, &kr));
}

#[test]
fn recent_calls_are_bounded_and_carry_no_ids() {
    let dir = TmpDir::new("ulog-recent-cap");
    let kr = MemKeyring::default();
    let mut log = UsageLog::open(&dir.0, &kr);
    for i in 0..(RECENT_MAX as u64 + 5) {
        log.append(begin(&format!("pc-{i:04}"), "openai", T0 + i, 1, 1, None)).unwrap();
    }
    let r = log.recent();
    assert_eq!(r.len(), RECENT_MAX);
    assert_eq!(r[0].at_ms, T0 + RECENT_MAX as u64 + 4, "the newest is first");
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("pc-0"), "call ids stay in the log, not the view");
    assert!(!json.contains("sess-1"), "session ids stay in the log, not the view");
}
