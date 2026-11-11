//! V60 SVC-2: the opt-out copy, the saved choice, and the power rules.

use super::*;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("csv-set-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// F-B1 "Opt-out is explicit": the sentence, word for word (planset 02 §9, 03 §3.5).
#[test]
fn opt_out_copy_exact() {
    assert_eq!(
        OPT_OUT_COPY,
        "If the app is closed and the service is off, your node won't produce blocks."
    );
    // The webview's copy is the same string.
    let ts = include_str!("../../src/shell/svcCopy.ts");
    assert!(
        ts.contains(&format!("\"{OPT_OUT_COPY}\"")),
        "svcCopy.ts drifted"
    );
}

/// Planset 02 §9 Power: training only on AC by default.
#[test]
fn no_training_on_battery_by_default() {
    let p = PowerPolicy::default();
    assert!(!p.train_on_battery);
    assert_eq!(p.pause_below_pct, 30);
    assert_eq!(p.nightly, None);
    let battery_full = PowerState {
        on_ac: Some(false),
        battery_pct: Some(100),
        has_battery: true,
    };
    let v = training_allowed(&p, &battery_full, 12);
    assert!(!v.allowed);
    assert!(v.reason.contains("only on AC"), "{}", v.reason);
    let ac = PowerState {
        on_ac: Some(true),
        battery_pct: Some(10),
        has_battery: true,
    };
    assert!(training_allowed(&p, &ac, 12).allowed);
}

#[test]
fn on_battery_when_allowed_pauses_below_the_threshold() {
    let p = PowerPolicy {
        train_on_battery: true,
        ..PowerPolicy::default()
    };
    let at = |pct| PowerState {
        on_ac: Some(false),
        battery_pct: Some(pct),
        has_battery: true,
    };
    assert!(!training_allowed(&p, &at(29), 12).allowed);
    assert!(training_allowed(&p, &at(30), 12).allowed);
    let unknown = PowerState {
        on_ac: Some(false),
        battery_pct: None,
        has_battery: true,
    };
    assert!(!training_allowed(&p, &unknown, 12).allowed);
    // An unreadable power state never counts as AC.
    let none = PowerState {
        on_ac: None,
        battery_pct: None,
        has_battery: false,
    };
    assert!(!training_allowed(&PowerPolicy::default(), &none, 12).allowed);
}

#[test]
fn the_nightly_window_wraps_midnight() {
    let w = Window { from: 23, to: 7 };
    assert!(w.contains(23) && w.contains(0) && w.contains(6));
    assert!(!w.contains(7) && !w.contains(12) && !w.contains(22));
    let day = Window { from: 9, to: 17 };
    assert!(day.contains(9) && !day.contains(17));
    let p = PowerPolicy {
        nightly: Some(w),
        ..PowerPolicy::default()
    };
    let ac = PowerState {
        on_ac: Some(true),
        battery_pct: None,
        has_battery: false,
    };
    assert!(training_allowed(&p, &ac, 2).allowed);
    let v = training_allowed(&p, &ac, 12);
    assert!(
        !v.allowed && v.reason.contains("23:00 to 07:00"),
        "{}",
        v.reason
    );
}

#[test]
fn policies_are_checked() {
    assert!(check_policy(&PowerPolicy::default()).is_ok());
    assert!(check_policy(&PowerPolicy {
        pause_below_pct: 101,
        ..PowerPolicy::default()
    })
    .is_err());
    assert!(check_policy(&PowerPolicy {
        nightly: Some(Window { from: 24, to: 7 }),
        ..PowerPolicy::default()
    })
    .is_err());
}

#[test]
fn the_choice_is_undecided_until_saved_and_survives_a_reload() {
    let dir = tmp("choice");
    assert_eq!(load(&dir), SvcSettings::default());
    assert_eq!(load(&dir).enabled, None);
    let s = SvcSettings {
        enabled: Some(false),
        power: PowerPolicy {
            train_on_battery: true,
            pause_below_pct: 50,
            nightly: Some(Window { from: 22, to: 6 }),
        },
    };
    save(&dir, &s).unwrap();
    assert_eq!(load(&dir), s);
    // An older file without power rules reads the default rules.
    std::fs::write(settings_path(&dir), r#"{"enabled":true}"#).unwrap();
    assert_eq!(load(&dir).power, PowerPolicy::default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pmset_output_is_parsed() {
    let laptop_batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1234567)\t85%; discharging; 4:10 remaining present: true\n";
    assert_eq!(
        parse_pmset(laptop_batt),
        PowerState {
            on_ac: Some(false),
            battery_pct: Some(85),
            has_battery: true
        }
    );
    let laptop_ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1234567)\t100%; charged; 0:00 remaining present: true\n";
    assert_eq!(parse_pmset(laptop_ac).on_ac, Some(true));
    let desktop = "Now drawing from 'AC Power'\n";
    assert_eq!(
        parse_pmset(desktop),
        PowerState {
            on_ac: Some(true),
            battery_pct: None,
            has_battery: false
        }
    );
    assert_eq!(parse_pmset("garbage").on_ac, None);
}

#[test]
fn sysfs_power_supplies_are_read() {
    let root = tmp("sysfs");
    let put = |dir: &str, files: &[(&str, &str)]| {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        for (f, v) in files {
            std::fs::write(d.join(f), format!("{v}\n")).unwrap();
        }
    };
    // No supplies: a desktop or server, on AC.
    assert_eq!(read_sysfs(&root).on_ac, Some(true));
    put(
        "BAT0",
        &[
            ("type", "Battery"),
            ("capacity", "42"),
            ("status", "Discharging"),
        ],
    );
    let s = read_sysfs(&root);
    assert_eq!(
        (s.on_ac, s.battery_pct, s.has_battery),
        (Some(false), Some(42), true)
    );
    put("AC", &[("type", "Mains"), ("online", "1")]);
    assert_eq!(read_sysfs(&root).on_ac, Some(true));
    put("AC", &[("type", "Mains"), ("online", "0")]);
    assert_eq!(read_sysfs(&root).on_ac, Some(false));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_local_hour_is_an_hour() {
    assert!(local_hour_now() <= 23);
}
