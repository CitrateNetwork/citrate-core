// HUP-S10.2 (US-10.2 AC3) — Hermes's own schedule: the store, the calendar expansion, and the
// `due` query the daemons lane (S10.3) reads.

use super::*;

const H: u64 = 3600;
const D: u64 = 24 * H;
/// 2026-10-01T00:00:00Z
const T0: u64 = 1_790_812_800;

fn tmp() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "citrate-hermes-schedule-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn input(title: &str, start: u64, repeat: Repeat) -> NewEntry {
    NewEntry {
        title: title.into(),
        notes: String::new(),
        start,
        duration_mins: 30,
        repeat,
        until: None,
        origin: Origin::Member,
    }
}

#[test]
fn a_missing_file_is_an_empty_schedule() {
    let s = ScheduleStore::new(tmp());
    match s.load_schedule() {
        LoadedSchedule::Ok(sch) => assert!(sch.entries.is_empty()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn entries_are_added_listed_disabled_and_removed() {
    let s = ScheduleStore::new(tmp());
    let a = s
        .add_entry(input("Morning brief", T0 + 8 * H, Repeat::Daily), T0)
        .unwrap();
    let b = s
        .add_entry(input("Pay rent", T0 + 3 * D, Repeat::None), T0)
        .unwrap();
    assert_ne!(a.id, b.id);
    let sch = s.load_ok().unwrap();
    assert_eq!(sch.entries.len(), 2);
    s.set_entry_enabled(&a.id, false).unwrap();
    assert!(!s.load_ok().unwrap().entries[0].enabled);
    s.remove_entry(&b.id).unwrap();
    let sch = s.load_ok().unwrap();
    assert_eq!(sch.entries.len(), 1);
    assert!(matches!(
        s.remove_entry("nope"),
        Err(ScheduleError::Invalid(_))
    ));
}

#[test]
fn bad_entries_are_refused() {
    let s = ScheduleStore::new(tmp());
    let mut e = input("", T0, Repeat::None);
    assert!(matches!(
        s.add_entry(e.clone(), T0),
        Err(ScheduleError::Invalid(_))
    ));
    e.title = "x".repeat(MAX_TITLE_CHARS + 1);
    assert!(matches!(
        s.add_entry(e.clone(), T0),
        Err(ScheduleError::Invalid(_))
    ));
    e.title = "ok".into();
    e.duration_mins = 0;
    assert!(matches!(
        s.add_entry(e.clone(), T0),
        Err(ScheduleError::Invalid(_))
    ));
    e.duration_mins = 30;
    e.notes = "n".repeat(MAX_NOTES_CHARS + 1);
    assert!(matches!(
        s.add_entry(e.clone(), T0),
        Err(ScheduleError::Invalid(_))
    ));
    e.notes.clear();
    e.until = Some(T0 - 1);
    assert!(matches!(s.add_entry(e, T0), Err(ScheduleError::Invalid(_))));
    // Control characters in a title never reach the calendar.
    let e = input("a\u{7}b", T0, Repeat::None);
    assert!(matches!(s.add_entry(e, T0), Err(ScheduleError::Invalid(_))));
}

#[test]
fn the_entry_count_is_capped() {
    let s = ScheduleStore::new(tmp());
    for i in 0..MAX_ENTRIES {
        s.add_entry(input(&format!("e{i}"), T0 + i as u64, Repeat::None), T0)
            .unwrap();
    }
    assert!(matches!(
        s.add_entry(input("one more", T0, Repeat::None), T0),
        Err(ScheduleError::Invalid(_))
    ));
}

#[test]
fn a_corrupted_file_is_never_overwritten_and_grants_nothing() {
    let dir = tmp();
    std::fs::write(dir.join(SCHEDULE_FILE), "{not json").unwrap();
    let s = ScheduleStore::new(dir.clone());
    assert!(matches!(s.load_schedule(), LoadedSchedule::Corrupted(_)));
    assert!(matches!(
        s.add_entry(input("x", T0, Repeat::None), T0),
        Err(ScheduleError::Corrupted(_))
    ));
    assert_eq!(
        std::fs::read_to_string(dir.join(SCHEDULE_FILE)).unwrap(),
        "{not json"
    );
    // Nothing is due from a corrupted schedule.
    assert!(s.due_between(T0, T0 + 30 * D).is_empty());
    // Reset sets it aside and starts empty.
    let aside = s.reset_corrupted(T0).unwrap();
    assert!(aside.exists());
    assert!(matches!(s.load_schedule(), LoadedSchedule::Ok(_)));
}

#[test]
fn one_off_entries_show_once_in_a_window_they_overlap() {
    let mut sch = Schedule::default();
    sch.entries
        .push(entry("1", T0 + 10 * H, 60, Repeat::None, None));
    let occ = occurrences(&sch, T0, T0 + D);
    assert_eq!(occ.len(), 1);
    assert_eq!(occ[0].start, T0 + 10 * H);
    assert_eq!(occ[0].end, T0 + 11 * H);
    // An event that started before the window but is still running overlaps it.
    let occ = occurrences(&sch, T0 + 10 * H + 30 * 60, T0 + D);
    assert_eq!(occ.len(), 1);
    // A window after it is empty; the end is exclusive.
    assert!(occurrences(&sch, T0 + 11 * H, T0 + D).is_empty());
}

#[test]
fn daily_and_weekly_entries_repeat_until_their_end() {
    let mut sch = Schedule::default();
    sch.entries
        .push(entry("d", T0 + 8 * H, 15, Repeat::Daily, None));
    sch.entries.push(entry(
        "w",
        T0 + 9 * H,
        60,
        Repeat::Weekly,
        Some(T0 + 15 * D),
    ));
    let occ = occurrences(&sch, T0, T0 + 28 * D);
    let daily: Vec<_> = occ.iter().filter(|o| o.entry_id == "d").collect();
    let weekly: Vec<_> = occ.iter().filter(|o| o.entry_id == "w").collect();
    assert_eq!(daily.len(), 28);
    assert_eq!(daily[1].start, T0 + D + 8 * H);
    // Weekly at T0, T0+7d, T0+14d; T0+21d is past `until`.
    assert_eq!(weekly.len(), 3);
    assert_eq!(weekly[2].start, T0 + 14 * D + 9 * H);
    // Sorted by start.
    assert!(occ.windows(2).all(|w| w[0].start <= w[1].start));
    // A window far in the future starts at the right occurrence, not the first.
    let later = occurrences(&sch, T0 + 100 * D, T0 + 101 * D);
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].start, T0 + 100 * D + 8 * H);
}

#[test]
fn disabled_entries_are_listed_but_never_due() {
    let mut sch = Schedule::default();
    let mut e = entry("x", T0 + H, 30, Repeat::Daily, None);
    e.enabled = false;
    sch.entries.push(e);
    assert!(occurrences(&sch, T0, T0 + D)[0].disabled);
    assert!(due_in(&sch, T0, T0 + 3 * D).is_empty());
}

#[test]
fn due_returns_starts_in_the_half_open_interval_after_then_upto() {
    let mut sch = Schedule::default();
    sch.entries
        .push(entry("d", T0 + 8 * H, 15, Repeat::Daily, None));
    // (T0, T0+8h] includes the 08:00 start.
    let d = due_in(&sch, T0, T0 + 8 * H);
    assert_eq!(d.len(), 1);
    // (T0+8h, T0+9h] does not: it was already due in the previous check.
    assert!(due_in(&sch, T0 + 8 * H, T0 + 9 * H).is_empty());
    // Catching up after the app was closed for three days returns each missed start once.
    assert_eq!(due_in(&sch, T0, T0 + 3 * D).len(), 3);
    // The catch-up is capped.
    assert!(due_in(&sch, T0, T0 + 10_000 * D).len() <= MAX_OCCURRENCES);
    // An empty or backwards interval is nothing.
    assert!(due_in(&sch, T0 + 9 * H, T0 + 8 * H).is_empty());
}

#[test]
fn a_huge_window_is_capped_not_unbounded() {
    let mut sch = Schedule::default();
    sch.entries.push(entry("d", T0, 1, Repeat::Daily, None));
    let occ = occurrences(&sch, T0, T0 + 5000 * D);
    assert!(occ.len() <= MAX_OCCURRENCES);
}

#[test]
fn the_window_must_be_forward_and_bounded_at_the_command_boundary() {
    assert!(check_window(T0, T0 + D).is_ok());
    assert!(check_window(T0 + D, T0).is_err());
    assert!(check_window(T0, T0 + (MAX_WINDOW_DAYS + 1) * D).is_err());
}

#[test]
fn the_file_round_trips_and_is_versioned() {
    let s = ScheduleStore::new(tmp());
    s.add_entry(input("Weekly review", T0, Repeat::Weekly), T0)
        .unwrap();
    let text = std::fs::read_to_string(s.file()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["version"], SCHEDULE_VERSION);
    assert_eq!(v["entries"][0]["repeat"], "weekly");
    assert_eq!(v["entries"][0]["origin"], "member");
    // An unknown version is not read as a schedule.
    std::fs::write(
        s.file(),
        text.replace(
            &format!("\"version\": {SCHEDULE_VERSION}"),
            "\"version\": 99",
        ),
    )
    .unwrap();
    assert!(matches!(s.load_schedule(), LoadedSchedule::Corrupted(_)));
}

fn entry(id: &str, start: u64, mins: u32, repeat: Repeat, until: Option<u64>) -> Entry {
    Entry {
        id: id.into(),
        title: id.into(),
        notes: String::new(),
        start,
        duration_mins: mins,
        repeat,
        until,
        enabled: true,
        origin: Origin::Member,
        created_at: T0,
    }
}

#[test]
fn an_exhausted_id_counter_refuses_the_add_and_leaves_the_file_alone() {
    let s = ScheduleStore::new(tmp());
    let sch = Schedule {
        next_id: u64::MAX,
        ..Schedule::default()
    };
    std::fs::write(s.file(), serde_json::to_string(&sch).unwrap()).unwrap();
    let before = std::fs::read_to_string(s.file()).unwrap();
    let err = s
        .add_entry(input("One more", T0, Repeat::None), T0)
        .unwrap_err();
    assert!(matches!(err, ScheduleError::Invalid(_)), "{err:?}");
    assert_eq!(std::fs::read_to_string(s.file()).unwrap(), before);
}
