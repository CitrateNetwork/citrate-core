// HUP-S10.3 — daemon scheduler + budget tests. Included into `daemons.rs` as `mod tests`, so the
// pure book is reachable via `super::*`. Every clock value is passed in (UTC ms + the local offset
// in minutes east of UTC), so these run the same on any machine and on case-sensitive Linux CI.
use super::*;

/// 2026-10-01T00:00:00Z, a Thursday.
const OCT1: u64 = 1_790_812_800_000;
const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

fn input(name: &str, schedule: &str) -> DaemonInput {
    DaemonInput {
        id: None,
        name: name.into(),
        prompt: "Summarise my node's last day in two lines.".into(),
        schedule: schedule.into(),
        budget: None,
    }
}

fn book_with(schedule: &str, now: u64) -> (DaemonBook, String) {
    let mut b = DaemonBook::default();
    let v = b
        .save(input("Node digest", schedule), now, 0)
        .expect("save");
    (b, v.id)
}

// ---------------------------------------------------------------------------
// Local civil time
// ---------------------------------------------------------------------------

#[test]
fn utc_ms_converts_to_local_civil_time() {
    let t = LocalTime::from_utc_ms(OCT1, 0);
    assert_eq!(
        (t.year, t.month, t.day, t.hour, t.minute),
        (2026, 10, 1, 0, 0)
    );
    assert_eq!(t.weekday, 4, "Thursday");
    assert_eq!(t.day_key(), "2026-10-01");
    // A leap day.
    let t = LocalTime::from_utc_ms(1_709_210_040_000, 0);
    assert_eq!(
        (t.year, t.month, t.day, t.hour, t.minute),
        (2024, 2, 29, 12, 34)
    );
    // The epoch, and an offset that crosses midnight backwards (UTC-5).
    let t = LocalTime::from_utc_ms(0, 0);
    assert_eq!((t.year, t.month, t.day, t.weekday), (1970, 1, 1, 4));
    let t = LocalTime::from_utc_ms(OCT1, -300);
    assert_eq!((t.year, t.month, t.day, t.hour), (2026, 9, 30, 19));
    assert_eq!(t.weekday, 3);
    // UTC+9 moves forward.
    let t = LocalTime::from_utc_ms(OCT1, 540);
    assert_eq!((t.day, t.hour), (1, 9));
}

#[test]
fn offsets_outside_any_real_time_zone_are_refused() {
    let mut b = DaemonBook::default();
    assert!(b.claim_due(OCT1, 15 * 60).is_err());
    assert!(b.claim_due(OCT1, -13 * 60).is_err());
    assert!(b.claim_due(OCT1, 14 * 60).is_ok());
}

// ---------------------------------------------------------------------------
// Schedules (5-field cron, local time)
// ---------------------------------------------------------------------------

#[test]
fn schedules_parse_the_supported_forms() {
    for ok in [
        "* * * * *",
        "0 9 * * *",
        "*/15 * * * *",
        "0 9-17 * * 1-5",
        "0,30 8 * * *",
        "5 4 1 * *",
        "0 0 * * 0",
        "0 0 * * 7",
        "0 */2 * * *",
        "10-50/20 * * * *",
        "@hourly",
        "@daily",
        "@weekly",
    ] {
        assert!(Schedule::parse(ok).is_ok(), "{ok:?} should parse");
    }
    for bad in [
        "",
        "* * * *",
        "* * * * * *",
        "60 * * * *",
        "* 24 * * *",
        "* * 0 * *",
        "* * 32 * *",
        "* * * 13 *",
        "* * * * 8",
        "*/0 * * * *",
        "5-1 * * * *",
        "a * * * *",
        "@reboot",
        "@yearly ",
        "1,,2 * * * *",
        "-1 * * * *",
    ] {
        assert!(Schedule::parse(bad).is_err(), "{bad:?} must not parse");
    }
}

#[test]
fn a_schedule_matches_local_minutes() {
    let s = Schedule::parse("0 9 * * 1-5").expect("parse");
    // 09:00 local on a Thursday matches; 09:01 does not; Saturday does not.
    assert!(s.matches(&LocalTime::from_utc_ms(OCT1 + 9 * HOUR, 0)));
    assert!(!s.matches(&LocalTime::from_utc_ms(OCT1 + 9 * HOUR + MIN, 0)));
    assert!(!s.matches(&LocalTime::from_utc_ms(OCT1 + 2 * DAY + 9 * HOUR, 0)));
    // Local, not UTC: 09:00 in UTC+2 is 07:00Z.
    assert!(s.matches(&LocalTime::from_utc_ms(OCT1 + 7 * HOUR, 120)));
    // Day-of-month and day-of-week both restricted: either matches (standard cron).
    let s = Schedule::parse("0 0 13 * 5").expect("parse");
    assert!(
        s.matches(&LocalTime::from_utc_ms(OCT1 + 12 * DAY, 0)),
        "the 13th"
    );
    assert!(
        s.matches(&LocalTime::from_utc_ms(OCT1 + DAY, 0)),
        "a Friday"
    );
    assert!(!s.matches(&LocalTime::from_utc_ms(OCT1, 0)));
    // 7 is Sunday too.
    let s = Schedule::parse("0 0 * * 7").expect("parse");
    assert!(s.matches(&LocalTime::from_utc_ms(OCT1 + 3 * DAY, 0)));
}

#[test]
fn the_next_run_is_the_next_matching_minute() {
    let s = Schedule::parse("30 9 * * *").expect("parse");
    assert_eq!(s.next_after(OCT1, 0), Some(OCT1 + 9 * HOUR + 30 * MIN));
    // Strictly after: at 09:30 the next one is tomorrow.
    assert_eq!(
        s.next_after(OCT1 + 9 * HOUR + 30 * MIN, 0),
        Some(OCT1 + DAY + 9 * HOUR + 30 * MIN)
    );
    // A rare date is still found (29 February 2028).
    let s = Schedule::parse("0 0 29 2 *").expect("parse");
    let next = s.next_after(OCT1, 0).expect("a leap day within five years");
    let t = LocalTime::from_utc_ms(next, 0);
    assert_eq!((t.year, t.month, t.day), (2028, 2, 29));
    // A date that never happens is reported as never.
    let s = Schedule::parse("0 0 31 2 *").expect("parse");
    assert_eq!(s.next_after(OCT1, 0), None);
}

// ---------------------------------------------------------------------------
// Saving: validation and the budget defaults
// ---------------------------------------------------------------------------

#[test]
fn a_new_daemon_gets_the_conservative_default_budget_and_spend_zero() {
    let (b, id) = book_with("0 9 * * *", OCT1);
    let v = b.view(&id, OCT1, 0).expect("view");
    assert_eq!(v.budget.max_runs_per_day, DEFAULT_MAX_RUNS_PER_DAY);
    assert_eq!(v.budget.max_tokens_per_day, DEFAULT_MAX_TOKENS_PER_DAY);
    assert_eq!(v.budget.max_tokens_per_run, DEFAULT_MAX_TOKENS_PER_RUN);
    assert_eq!(v.budget.max_spend_salt, "0");
    assert!(!v.paused);
    assert_eq!(v.status, "scheduled");
    assert_eq!(v.runs_today, 0);
    assert_eq!(v.next_run_ms, Some(OCT1 + 9 * HOUR));
}

#[test]
fn saving_refuses_bad_input() {
    let mut b = DaemonBook::default();
    let mut bad = vec![];
    let mut i = input("x", "0 9 * * *");
    i.name = "  ".into();
    bad.push(i);
    let mut i = input("x", "0 9 * * *");
    i.prompt = "".into();
    bad.push(i);
    let mut i = input("x", "0 9 * * *");
    i.prompt = "p".repeat(PROMPT_MAX + 1);
    bad.push(i);
    let mut i = input("x", "0 9 * * *");
    i.name = "n".repeat(NAME_MAX + 1);
    bad.push(i);
    bad.push(input("x", "not cron"));
    let mut i = input("x", "0 9 * * *");
    i.id = Some("missing".into());
    bad.push(i);
    for i in bad {
        assert!(b.save(i, OCT1, 0).is_err());
    }
    assert!(b.list(OCT1, 0).is_empty());
}

#[test]
fn a_spend_budget_above_zero_is_refused() {
    let mut b = DaemonBook::default();
    for spend in ["1", "0.000001", "-1", "abc", ""] {
        let mut i = input("x", "0 9 * * *");
        i.budget = Some(BudgetInput {
            max_runs_per_day: 2,
            max_tokens_per_day: 1000,
            max_tokens_per_run: 500,
            max_spend_salt: spend.into(),
        });
        let e = b.save(i, OCT1, 0).expect_err(spend);
        assert!(!e.is_empty());
    }
    let mut i = input("x", "0 9 * * *");
    i.budget = Some(BudgetInput {
        max_runs_per_day: 2,
        max_tokens_per_day: 1000,
        max_tokens_per_run: 500,
        max_spend_salt: "0".into(),
    });
    assert!(b.save(i, OCT1, 0).is_ok());
}

#[test]
fn budgets_are_bounded_by_the_hard_caps() {
    let mut b = DaemonBook::default();
    let cases = [
        (0, 1000, 500),
        (HARD_MAX_RUNS_PER_DAY + 1, 1000, 500),
        (2, 0, 0),
        (2, HARD_MAX_TOKENS_PER_DAY + 1, 500),
        (2, 1000, 0),
        (2, 1000, 1001), // a run may not exceed the day
    ];
    for (runs, day, run) in cases {
        let mut i = input("x", "0 9 * * *");
        i.budget = Some(BudgetInput {
            max_runs_per_day: runs,
            max_tokens_per_day: day,
            max_tokens_per_run: run,
            max_spend_salt: "0".into(),
        });
        assert!(b.save(i, OCT1, 0).is_err(), "{runs} {day} {run}");
    }
}

#[test]
fn there_is_a_cap_on_how_many_daemons_exist() {
    let mut b = DaemonBook::default();
    for n in 0..MAX_DAEMONS {
        b.save(input(&format!("d{n}"), "0 9 * * *"), OCT1, 0)
            .expect("under the cap");
    }
    assert!(b.save(input("one more", "0 9 * * *"), OCT1, 0).is_err());
}

#[test]
fn editing_keeps_the_ledger_and_does_not_fire_at_once() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let claims = b.claim_due(OCT1 + MIN, 0).expect("claim");
    assert_eq!(claims.len(), 1);
    b.finish_run(
        &id,
        &claims[0].run_id,
        100,
        RunOutcome::Answered,
        "ok",
        OCT1 + MIN,
    )
    .expect("finish");
    let mut i = input("Renamed", "0 9 * * *");
    i.id = Some(id.clone());
    b.save(i, OCT1 + 2 * MIN, 0).expect("edit");
    let v = b.view(&id, OCT1 + 2 * MIN, 0).expect("view");
    assert_eq!(v.name, "Renamed");
    assert_eq!(v.runs_today, 1);
    assert_eq!(v.tokens_today, 100);
    assert!(b.claim_due(OCT1 + 3 * MIN, 0).expect("claim").is_empty());
}

// ---------------------------------------------------------------------------
// Claiming due runs
// ---------------------------------------------------------------------------

#[test]
fn a_daemon_fires_once_when_its_minute_passes() {
    let (mut b, id) = book_with("0 9 * * *", OCT1);
    assert!(b.claim_due(OCT1 + 8 * HOUR, 0).expect("claim").is_empty());
    let claims = b.claim_due(OCT1 + 9 * HOUR + 20_000, 0).expect("claim");
    assert_eq!(claims.len(), 1);
    let c = &claims[0];
    assert_eq!(c.daemon_id, id);
    assert_eq!(c.tokens_allowed, DEFAULT_MAX_TOKENS_PER_RUN);
    assert!(c.prompt.contains("Summarise"));
    // The same minute does not fire twice, even after the run ends.
    b.finish_run(
        &id,
        &c.run_id,
        10,
        RunOutcome::Answered,
        "",
        OCT1 + 9 * HOUR + 30_000,
    )
    .expect("finish");
    assert!(b
        .claim_due(OCT1 + 9 * HOUR + 50_000, 0)
        .expect("claim")
        .is_empty());
    assert_eq!(
        b.view(&id, OCT1 + 10 * HOUR, 0).expect("v").status,
        "scheduled"
    );
}

#[test]
fn a_new_daemon_never_fires_for_a_minute_before_it_existed() {
    // Saved at 09:00:30; the 09:00 minute already started, so the first run is tomorrow.
    let (mut b, _) = book_with("0 9 * * *", OCT1 + 9 * HOUR + 30_000);
    assert!(b
        .claim_due(OCT1 + 9 * HOUR + 50_000, 0)
        .expect("claim")
        .is_empty());
    assert!(b.claim_due(OCT1 + 12 * HOUR, 0).expect("claim").is_empty());
}

#[test]
fn missed_runs_while_the_app_was_closed_catch_up_once_not_many_times() {
    let (mut b, _) = book_with("*/5 * * * *", OCT1);
    // The app was closed for three hours (36 matching minutes): one run, not 36.
    let claims = b.claim_due(OCT1 + 3 * HOUR, 0).expect("claim");
    assert_eq!(claims.len(), 1);
}

#[test]
fn runs_missed_more_than_a_day_ago_are_not_caught_up() {
    let (mut b, _) = book_with("0 9 * * *", OCT1);
    // Closed from before 09:00 on day 1 until 08:00 on day 3: the last matching minute (day 2,
    // 09:00) is within 24 h, so one run. Closed until day 3 at 10:00 instead: day 3's 09:00 is
    // inside the window, one run.
    assert_eq!(
        b.claim_due(OCT1 + 2 * DAY + 8 * HOUR, 0).expect("c").len(),
        1
    );
    let (mut b, _) = book_with("0 9 1 1 *", OCT1);
    // A yearly schedule whose minute was months ago does not fire on start.
    assert!(b.claim_due(OCT1 + 200 * DAY, 0).expect("c").is_empty());
}

#[test]
fn only_one_run_of_a_daemon_is_in_flight() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let first = b.claim_due(OCT1 + MIN, 0).expect("claim");
    assert_eq!(first.len(), 1);
    // Its next minutes pass while the run is still going: no second run.
    assert!(b.claim_due(OCT1 + 2 * MIN, 0).expect("claim").is_empty());
    assert_eq!(b.view(&id, OCT1 + 2 * MIN, 0).expect("v").status, "running");
    b.finish_run(
        &id,
        &first[0].run_id,
        1,
        RunOutcome::Answered,
        "",
        OCT1 + 2 * MIN,
    )
    .expect("finish");
    assert_eq!(b.claim_due(OCT1 + 3 * MIN, 0).expect("claim").len(), 1);
}

#[test]
fn a_run_the_app_never_finishes_is_released_as_abandoned_and_charged_its_allowance() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let c = b.claim_due(OCT1 + MIN, 0).expect("claim").remove(0);
    let later = OCT1 + MIN + STALE_RUN_MS + MIN;
    let next = b.claim_due(later, 0).expect("claim");
    assert_eq!(
        next.len(),
        1,
        "the stale run is released and the daemon runs again"
    );
    let v = b.view(&id, later, 0).expect("v");
    assert_eq!(
        v.tokens_today,
        u64::from(c.tokens_allowed),
        "charged in full, never free"
    );
    assert_eq!(v.runs_today, 2);
    // The stale run's late report is refused.
    assert!(b
        .finish_run(&id, &c.run_id, 5, RunOutcome::Answered, "", later)
        .is_err());
}

// ---------------------------------------------------------------------------
// Budget exhaustion
// ---------------------------------------------------------------------------

#[test]
fn the_daily_run_budget_runs_out_and_says_so() {
    let mut b = DaemonBook::default();
    let mut i = input("Often", "* * * * *");
    i.budget = Some(BudgetInput {
        max_runs_per_day: 3,
        max_tokens_per_day: 100_000,
        max_tokens_per_run: 1_000,
        max_spend_salt: "0".into(),
    });
    let id = b.save(i, OCT1, 0).expect("save").id;
    let mut fired = 0;
    for m in 1..=10u64 {
        for c in b.claim_due(OCT1 + m * MIN, 0).expect("claim") {
            fired += 1;
            b.finish_run(
                &id,
                &c.run_id,
                10,
                RunOutcome::Answered,
                "",
                OCT1 + m * MIN + 1,
            )
            .expect("finish");
        }
    }
    assert_eq!(fired, 3);
    let v = b.view(&id, OCT1 + 11 * MIN, 0).expect("v");
    assert_eq!(v.runs_today, 3);
    assert_eq!(v.status, "budget used up today");
    assert!(v.skipped_today >= 7);
    assert_eq!(
        v.next_run_ms,
        Some(OCT1 + DAY),
        "the next run is tomorrow's first match"
    );
}

#[test]
fn the_daily_token_budget_runs_out_and_caps_the_last_run() {
    let mut b = DaemonBook::default();
    let mut i = input("Thirsty", "* * * * *");
    i.budget = Some(BudgetInput {
        max_runs_per_day: 48,
        max_tokens_per_day: 2_500,
        max_tokens_per_run: 1_000,
        max_spend_salt: "0".into(),
    });
    let id = b.save(i, OCT1, 0).expect("save").id;
    let mut allowed = vec![];
    for m in 1..=10u64 {
        for c in b.claim_due(OCT1 + m * MIN, 0).expect("claim") {
            allowed.push(c.tokens_allowed);
            b.finish_run(
                &id,
                &c.run_id,
                c.tokens_allowed,
                RunOutcome::Answered,
                "",
                OCT1 + m * MIN + 1,
            )
            .expect("finish");
        }
    }
    // 1000 + 1000 + the remaining 500, then nothing.
    assert_eq!(allowed, vec![1_000, 1_000, 500]);
    let v = b.view(&id, OCT1 + 11 * MIN, 0).expect("v");
    assert_eq!(v.tokens_today, 2_500);
    assert_eq!(v.status, "budget used up today");
}

#[test]
fn a_run_that_overshoots_is_charged_what_it_used() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let c = b.claim_due(OCT1 + MIN, 0).expect("claim").remove(0);
    b.finish_run(
        &id,
        &c.run_id,
        c.tokens_allowed + 700,
        RunOutcome::OverBudget,
        "",
        OCT1 + MIN,
    )
    .expect("finish");
    let v = b.view(&id, OCT1 + MIN, 0).expect("v");
    assert_eq!(v.tokens_today, u64::from(c.tokens_allowed) + 700);
    assert_eq!(v.last_outcome.as_deref(), Some("over_budget"));
}

#[test]
fn the_budget_resets_on_the_next_local_day() {
    let mut b = DaemonBook::default();
    let mut i = input("Daily", "* * * * *");
    i.budget = Some(BudgetInput {
        max_runs_per_day: 1,
        max_tokens_per_day: 1_000,
        max_tokens_per_run: 1_000,
        max_spend_salt: "0".into(),
    });
    // UTC-5: local midnight is 05:00Z.
    let id = b.save(i, OCT1, -300).expect("save").id;
    let c = b.claim_due(OCT1 + MIN, -300).expect("claim").remove(0);
    b.finish_run(&id, &c.run_id, 10, RunOutcome::Answered, "", OCT1 + MIN)
        .expect("finish");
    assert!(b
        .claim_due(OCT1 + 4 * HOUR, -300)
        .expect("claim")
        .is_empty());
    // 05:01Z is 00:01 local the next day: a fresh budget.
    assert_eq!(
        b.claim_due(OCT1 + 5 * HOUR + MIN, -300)
            .expect("claim")
            .len(),
        1
    );
    assert_eq!(
        b.view(&id, OCT1 + 5 * HOUR + MIN, -300)
            .expect("v")
            .runs_today,
        1
    );
}

// ---------------------------------------------------------------------------
// Pausing
// ---------------------------------------------------------------------------

#[test]
fn a_paused_daemon_never_fires_and_does_not_catch_up_on_resume() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    b.set_paused(&id, true, OCT1).expect("pause");
    assert_eq!(b.view(&id, OCT1, 0).expect("v").status, "paused");
    for m in 1..=30u64 {
        assert!(b.claim_due(OCT1 + m * MIN, 0).expect("claim").is_empty());
    }
    b.set_paused(&id, false, OCT1 + 30 * MIN + 1)
        .expect("resume");
    assert!(b
        .claim_due(OCT1 + 30 * MIN + 2, 0)
        .expect("claim")
        .is_empty());
    assert_eq!(b.claim_due(OCT1 + 31 * MIN, 0).expect("claim").len(), 1);
}

#[test]
fn resuming_without_any_tick_while_paused_does_not_replay() {
    // The app was closed for the whole pause, so no tick ever saw the paused minutes.
    let (mut b, id) = book_with("* * * * *", OCT1);
    b.set_paused(&id, true, OCT1).expect("pause");
    b.set_paused(&id, false, OCT1 + 30 * MIN + 1)
        .expect("resume");
    assert!(b
        .claim_due(OCT1 + 30 * MIN + 2, 0)
        .expect("claim")
        .is_empty());
    // The same for "pause all".
    let (mut b, _) = book_with("* * * * *", OCT1);
    b.set_all_paused(true, OCT1);
    b.set_all_paused(false, OCT1 + 30 * MIN + 1);
    assert!(b
        .claim_due(OCT1 + 30 * MIN + 2, 0)
        .expect("claim")
        .is_empty());
}

#[test]
fn pausing_everything_stops_every_daemon() {
    let mut b = DaemonBook::default();
    b.save(input("a", "* * * * *"), OCT1, 0).expect("a");
    b.save(input("b", "* * * * *"), OCT1, 0).expect("b");
    b.set_all_paused(true, OCT1);
    assert!(b.all_paused());
    for m in 1..=5u64 {
        assert!(b.claim_due(OCT1 + m * MIN, 0).expect("claim").is_empty());
    }
    for v in b.list(OCT1 + 5 * MIN, 0) {
        assert_eq!(v.status, "paused");
    }
    b.set_all_paused(false, OCT1 + 5 * MIN + 1);
    assert_eq!(b.claim_due(OCT1 + 6 * MIN, 0).expect("claim").len(), 2);
}

#[test]
fn pausing_a_running_daemon_keeps_its_run_reportable() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let c = b.claim_due(OCT1 + MIN, 0).expect("claim").remove(0);
    b.set_paused(&id, true, OCT1 + MIN).expect("pause");
    b.finish_run(
        &id,
        &c.run_id,
        5,
        RunOutcome::Stopped,
        "paused by you",
        OCT1 + MIN,
    )
    .expect("the stopped run still reports");
    let v = b.view(&id, OCT1 + MIN, 0).expect("v");
    assert_eq!(v.status, "paused");
    assert_eq!(v.last_outcome.as_deref(), Some("stopped"));
}

#[test]
fn unknown_ids_and_runs_are_refused() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    assert!(b.set_paused("nope", true, OCT1).is_err());
    assert!(b.delete("nope").is_err());
    assert!(b
        .finish_run(&id, "r-not-a-run", 1, RunOutcome::Answered, "", OCT1)
        .is_err());
    assert!(b
        .finish_run("nope", "r", 1, RunOutcome::Answered, "", OCT1)
        .is_err());
    b.delete(&id).expect("delete");
    assert!(b.list(OCT1, 0).is_empty());
}

#[test]
fn the_last_note_is_bounded_and_single_line() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let c = b.claim_due(OCT1 + MIN, 0).expect("claim").remove(0);
    let long = format!("line one\nline two {}", "x".repeat(2 * NOTE_MAX));
    b.finish_run(&id, &c.run_id, 1, RunOutcome::Answered, &long, OCT1 + MIN)
        .expect("finish");
    let note = b
        .view(&id, OCT1 + MIN, 0)
        .expect("v")
        .last_note
        .expect("note");
    assert!(note.chars().count() <= NOTE_MAX);
    assert!(!note.contains('\n'));
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

#[test]
fn the_book_round_trips_through_its_file_format() {
    let (mut b, id) = book_with("*/10 * * * *", OCT1);
    let c = b.claim_due(OCT1 + 10 * MIN, 0).expect("claim").remove(0);
    b.set_all_paused(true, OCT1 + 10 * MIN);
    let text = b.to_json().expect("json");
    let back = DaemonBook::from_json(&text).expect("parse");
    assert!(back.all_paused());
    let v = back.view(&id, OCT1 + 10 * MIN, 0).expect("v");
    assert_eq!(v.status, "paused");
    assert_eq!(v.runs_today, 1);
    let mut back = back;
    back.finish_run(&id, &c.run_id, 3, RunOutcome::Answered, "", OCT1 + 11 * MIN)
        .expect("the in-flight run survives a reload");
}

#[test]
fn a_damaged_file_is_an_error_not_an_empty_book() {
    for bad in [
        "",
        "{",
        "[]",
        "{\"version\":99,\"allPaused\":false,\"daemons\":[]}",
    ] {
        assert!(DaemonBook::from_json(bad).is_err(), "{bad:?}");
    }
    // A record whose schedule no longer parses is refused too (never silently dropped).
    let (b, _) = book_with("0 9 * * *", OCT1);
    let text = b.to_json().expect("json").replace("0 9 * * *", "bogus");
    assert!(DaemonBook::from_json(&text).is_err());
}

#[test]
fn the_file_is_written_atomically_and_read_back() {
    let dir = std::env::temp_dir().join(format!("citrate-daemons-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(FILE_NAME);
    assert!(load_book(&path)
        .expect("missing file = empty book")
        .list(OCT1, 0)
        .is_empty());
    let (b, id) = book_with("0 9 * * *", OCT1);
    store_book(&path, &b).expect("store");
    let back = load_book(&path).expect("load");
    assert!(back.view(&id, OCT1, 0).is_some());
    assert!(!dir.join(format!("{FILE_NAME}.tmp")).exists());
    std::fs::write(&path, "{").expect("damage");
    assert!(load_book(&path).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[test]
fn every_daemon_command_is_async_registered_and_allowed_for_the_main_window_only() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    let src = include_str!("daemons.rs");
    for cmd in COMMANDS {
        assert!(
            lib.contains(&format!("daemons::{cmd},")),
            "{cmd} registered in generate_handler!"
        );
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
        assert!(
            src.contains(&format!("pub async fn {cmd}(")),
            "{cmd} is an async command"
        );
    }
}

// ---------------------------------------------------------------------------
// HUP-S10.3 — measured tokens and the daemon run log (metering folder)
// ---------------------------------------------------------------------------

fn run_log_tmp(tag: &str) -> std::path::PathBuf {
    use rand::RngCore;
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    std::env::temp_dir()
        .join(format!("n6-daemon-runlog-{tag}-{}", hex::encode(r)))
        .join("metering")
        .join(RUN_LOG_FILE)
}

#[test]
fn a_finished_run_records_its_token_source_and_returns_a_content_free_log_entry() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let c = b.claim_due(OCT1 + MIN, 0).expect("claim").remove(0);
    let entry = b
        .finish_run_with(
            &id,
            &c.run_id,
            812,
            TokenSource::Measured,
            RunOutcome::Answered,
            "the model's reply text never reaches the log",
            OCT1 + 2 * MIN,
        )
        .expect("finish");
    assert_eq!(
        entry,
        RunLogEntry {
            schema: RUN_LOG_SCHEMA,
            daemon_id: id.clone(),
            run_id: c.run_id.clone(),
            name: "Node digest".into(),
            started_ms: OCT1 + MIN,
            ended_ms: OCT1 + 2 * MIN,
            tokens: 812,
            token_source: TokenSource::Measured,
            outcome: RunOutcome::Answered,
        }
    );
    let line = serde_json::to_string(&entry).expect("json");
    assert!(!line.contains("reply text"), "no run content in the log");
    let v = b.view(&id, OCT1 + 2 * MIN, 0).expect("v");
    assert_eq!(v.last_token_source.as_deref(), Some("measured"));
    assert_eq!(v.tokens_today, 812);
}

#[test]
fn the_plain_finish_counts_tokens_as_estimated() {
    let (mut b, id) = book_with("* * * * *", OCT1);
    let c = b.claim_due(OCT1 + MIN, 0).expect("claim").remove(0);
    b.finish_run(&id, &c.run_id, 40, RunOutcome::Failed, "", OCT1 + 2 * MIN)
        .expect("finish");
    let v = b.view(&id, OCT1 + 2 * MIN, 0).expect("v");
    assert_eq!(v.last_token_source.as_deref(), Some("estimated"));
    // A runner that does not say where its count came from is estimated.
    let src: TokenSource = Default::default();
    assert_eq!(src, TokenSource::Estimated);
}

#[test]
fn an_older_daemons_file_without_a_token_source_still_loads() {
    let (b, _) = book_with("0 9 * * *", OCT1);
    let json = b
        .to_json()
        .expect("json")
        .replace("\"lastTokenSource\": null,", "");
    assert!(!json.contains("lastTokenSource"));
    assert!(DaemonBook::from_json(&json).is_ok());
}

#[test]
fn the_run_log_appends_and_reads_back_a_window() {
    let path = run_log_tmp("window");
    let mk = |run: &str, ended: u64, src: TokenSource| RunLogEntry {
        schema: RUN_LOG_SCHEMA,
        daemon_id: "d1".into(),
        run_id: run.into(),
        name: "Node digest".into(),
        started_ms: ended - MIN,
        ended_ms: ended,
        tokens: 100,
        token_source: src,
        outcome: RunOutcome::Answered,
    };
    assert_eq!(
        read_run_log(&path, 0, u64::MAX).expect("missing log"),
        (vec![], 0),
        "a missing log is an empty list"
    );
    append_run_log(&path, &mk("r1", OCT1 - MIN, TokenSource::Estimated)).expect("a1");
    append_run_log(&path, &mk("r2", OCT1 + HOUR, TokenSource::Measured)).expect("a2");
    append_run_log(&path, &mk("r3", OCT1 + DAY, TokenSource::Measured)).expect("a3");
    let (runs, bad) = read_run_log(&path, OCT1, OCT1 + DAY).expect("read");
    assert_eq!(bad, 0);
    assert_eq!(
        runs.iter().map(|r| r.run_id.as_str()).collect::<Vec<_>>(),
        vec!["r2"],
        "only runs that ended inside [from, to)"
    );
    assert_eq!(runs[0].token_source, TokenSource::Measured);
    let _ = std::fs::remove_dir_all(path.parent().and_then(|p| p.parent()).expect("dir"));
}

#[test]
fn a_damaged_run_log_line_is_counted_not_guessed() {
    let path = run_log_tmp("damaged");
    std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
    std::fs::write(&path, "not json\n{\"schema\":99}\n").expect("seed");
    let entry = RunLogEntry {
        schema: RUN_LOG_SCHEMA,
        daemon_id: "d1".into(),
        run_id: "r1".into(),
        name: "x".into(),
        started_ms: OCT1,
        ended_ms: OCT1,
        tokens: 1,
        token_source: TokenSource::Estimated,
        outcome: RunOutcome::Stopped,
    };
    append_run_log(&path, &entry).expect("append");
    let (runs, bad) = read_run_log(&path, 0, u64::MAX).expect("read");
    assert_eq!(runs.len(), 1);
    assert_eq!(bad, 2);
    let _ = std::fs::remove_dir_all(path.parent().and_then(|p| p.parent()).expect("dir"));
}

#[test]
fn a_read_returns_only_the_newest_entries_when_the_window_holds_too_many() {
    let path = run_log_tmp("cap");
    for i in 0..(RUN_LOG_READ_MAX as u64 + 3) {
        append_run_log(
            &path,
            &RunLogEntry {
                schema: RUN_LOG_SCHEMA,
                daemon_id: "d1".into(),
                run_id: format!("r{i}"),
                name: "x".into(),
                started_ms: OCT1 + i,
                ended_ms: OCT1 + i,
                tokens: 1,
                token_source: TokenSource::Estimated,
                outcome: RunOutcome::Answered,
            },
        )
        .expect("append");
    }
    let (runs, _) = read_run_log(&path, 0, u64::MAX).expect("read");
    assert_eq!(runs.len(), RUN_LOG_READ_MAX);
    assert_eq!(runs[0].run_id, "r3");
    assert_eq!(
        runs.last().map(|r| r.run_id.clone()),
        Some(format!("r{}", RUN_LOG_READ_MAX + 2))
    );
    let _ = std::fs::remove_dir_all(path.parent().and_then(|p| p.parent()).expect("dir"));
}

#[test]
fn a_run_log_past_the_read_cap_still_yields_its_newest_runs() {
    // Reviewer (N5-everyday): at the hard ceiling (16 daemons x 48 runs a day) the log passes the
    // read cap in weeks. The journal must keep reading the newest runs, never fail for good.
    let path = run_log_tmp("tail");
    let mk = |i: u64| RunLogEntry {
        schema: RUN_LOG_SCHEMA,
        daemon_id: "d1".into(),
        run_id: format!("r{i}"),
        name: "Node digest".into(),
        started_ms: OCT1 + i,
        ended_ms: OCT1 + i,
        tokens: 1,
        token_source: TokenSource::Estimated,
        outcome: RunOutcome::Answered,
    };
    for i in 0..40 {
        append_run_log(&path, &mk(i)).expect("append");
    }
    let len = std::fs::metadata(&path).expect("meta").len();
    let cap = len / 4;
    let (runs, bad) = read_run_log_capped(&path, 0, u64::MAX, cap).expect("a big log is read");
    assert_eq!(bad, 0, "the cut first line is dropped, not counted as damaged");
    assert!(!runs.is_empty() && runs.len() < 40, "only the tail is read");
    assert_eq!(runs.last().map(|r| r.run_id.as_str()), Some("r39"));
    let first: u64 = runs[0].run_id[1..].parse().expect("id");
    assert_eq!(
        runs.iter().map(|r| r.run_id.clone()).collect::<Vec<_>>(),
        (first..40).map(|i| format!("r{i}")).collect::<Vec<_>>(),
        "a contiguous run of the newest entries"
    );
    let _ = std::fs::remove_dir_all(path.parent().and_then(|p| p.parent()).expect("dir"));
}
