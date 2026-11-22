// 0.5.7 fd-limit lane: the start-up soft RLIMIT_NOFILE raise. The arithmetic is pure, with
// injected limits; the inheritance test runs the real raise in a re-executed copy of this test
// binary whose soft limit a shell first dropped to 256 (as Finder/Dock does on macOS).
use super::*;

const INF: u64 = u64::MAX;

#[test]
fn target_is_min_of_want_and_hard_and_never_lowers() {
    // macOS Finder launch: soft 256, hard unlimited, kernel cap 61440.
    assert_eq!(target_soft(256, INF, TARGET_NOFILE, Some(61_440)), 61_440);
    // A kernel cap above the target: the target.
    assert_eq!(target_soft(256, INF, TARGET_NOFILE, Some(245_760)), TARGET_NOFILE);
    // Linux, no OS cap: the hard limit bounds it.
    assert_eq!(target_soft(1024, 4096, TARGET_NOFILE, None), 4096);
    assert_eq!(target_soft(1024, 1_048_576, TARGET_NOFILE, None), TARGET_NOFILE);
    // Already higher than the target: never lowered.
    assert_eq!(target_soft(100_000, INF, TARGET_NOFILE, None), 100_000);
    // Hard below the current soft can't happen, but the soft is kept even then.
    assert_eq!(target_soft(512, 256, TARGET_NOFILE, None), 512);
    // A zero cap reads as "no cap", never as 0.
    assert_eq!(target_soft(256, 8192, TARGET_NOFILE, Some(0)), 8192);
}

#[test]
fn macos_cap_uses_maxfilesperproc_else_open_max() {
    assert_eq!(macos_cap(Some(61_440)), 61_440);
    assert_eq!(macos_cap(Some(0)), MACOS_OPEN_MAX);
    assert_eq!(macos_cap(Some(-1)), MACOS_OPEN_MAX);
    assert_eq!(macos_cap(None), MACOS_OPEN_MAX);
    // With the OPEN_MAX fallback the target is 10240, not 65536.
    assert_eq!(target_soft(256, INF, TARGET_NOFILE, Some(macos_cap(None))), 10_240);
}

#[test]
fn report_line_names_before_and_after() {
    let r = FdLimitReport { before_soft: 256, hard: INF, after_soft: 61_440, error: None };
    assert_eq!(r.to_string(), "[fd-limit] open-file limit: soft 256 -> 61440 (hard unlimited)");
    let r = FdLimitReport { error: Some("setrlimit to 65536 refused".into()), ..r };
    assert!(r.to_string().ends_with("; setrlimit to 65536 refused"));
}

/// Set in the re-executed probe so it knows it is the child run.
#[cfg(unix)]
const PROBE_ENV: &str = "CITRATE_FD_LIMIT_PROBE";
/// What the probe prints before the child's soft limit, for the parent to find.
#[cfg(unix)]
const PROBE_MARK: &str = "FD_LIMIT_CHILD_SOFT=";

/// The child's soft open-file limit: on Linux from `/proc/<pid>/limits` ("Max open files"), the
/// kernel's own record of the spawned process; elsewhere from the child shell's `ulimit -Sn`.
#[cfg(target_os = "linux")]
fn spawned_child_soft() -> u64 {
    let mut child = std::process::Command::new("sleep").arg("5").spawn().expect("spawn sleep");
    let limits = std::fs::read_to_string(format!("/proc/{}/limits", child.id())).expect("read /proc/<pid>/limits");
    let _ = child.kill();
    let _ = child.wait();
    let row = limits.lines().find(|l| l.starts_with("Max open files")).expect("Max open files row");
    let soft = row.split_whitespace().nth(3).expect("soft column");
    if soft == "unlimited" {
        u64::MAX
    } else {
        soft.parse().expect("numeric soft limit")
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn spawned_child_soft() -> u64 {
    let out = std::process::Command::new("/bin/sh").args(["-c", "ulimit -Sn"]).output().expect("spawn sh");
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s == "unlimited" {
        u64::MAX
    } else {
        s.parse().expect("numeric soft limit")
    }
}

/// The probe: run by `a_child_spawned_after_the_raise_inherits_it` under a soft limit of 256.
/// Ignored in a normal run (it would raise the test binary's own limit for nothing).
#[cfg(unix)]
#[test]
#[ignore = "run only as the re-executed probe of a_child_spawned_after_the_raise_inherits_it"]
fn fd_limit_inherit_probe() {
    if std::env::var_os(PROBE_ENV).is_none() {
        return;
    }
    let report = raise_nofile(TARGET_NOFILE);
    println!("{report}");
    println!("{PROBE_MARK}{}", spawned_child_soft());
}

/// Acceptance (0.5.7): with the app's own soft limit starting at 256, a process spawned after the
/// start-up raise has a soft "Max open files" of at least 4096. `ulimit -Sn` (soft only): a bare
/// `ulimit -n` would drop the hard limit too and nothing could be raised.
#[cfg(unix)]
#[test]
fn a_child_spawned_after_the_raise_inherits_it() {
    let (_, hard) = get_nofile().expect("getrlimit");
    assert!(hard >= 4096, "this host's hard RLIMIT_NOFILE is {hard}; the acceptance needs at least 4096");
    let me = std::env::current_exe().expect("test binary path");
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("ulimit -Sn 256 && [ \"$(ulimit -Sn)\" = 256 ] && exec \"$0\" fd_limit::tests::fd_limit_inherit_probe --exact --ignored --nocapture --test-threads=1")
        .arg(&me)
        .env(PROBE_ENV, "1")
        .output()
        .expect("run the probe");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "probe failed: {stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("[fd-limit] open-file limit: soft 256 -> "), "the probe started from 256: {stdout}");
    let soft: u64 = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix(PROBE_MARK))
        .expect("the probe printed the child's soft limit")
        .parse()
        .expect("numeric");
    assert!(soft >= 4096, "the spawned child's soft open-file limit is {soft}, want at least 4096: {stdout}");
}
