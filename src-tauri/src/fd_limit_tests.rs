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
    // Review P2: on macOS the probe also prints the kernel cap it read (sysctlbyname
    // kern.maxfilesperproc), so the parent checks the clamp against the real value.
    #[cfg(target_os = "macos")]
    println!("{CAP_MARK}{}", os_cap().unwrap_or(0));
    let report = if std::env::var(PROBE_ENV).as_deref() == Ok("over-cap") {
        // Ask for more than the kernel allows, as an unclamped caller would: setrlimit refuses
        // (or the kernel clamps), and the OPEN_MAX fallback is what is left.
        raise_nofile_capped(u64::MAX - 1, Some(u64::MAX - 1))
    } else {
        raise_nofile(TARGET_NOFILE)
    };
    println!("{report}");
    println!("{AFTER_MARK}{}", report.after_soft);
    println!("{PROBE_MARK}{}", spawned_child_soft());
}

#[cfg(target_os = "macos")]
const CAP_MARK: &str = "FD_LIMIT_KERNEL_CAP=";
#[cfg(unix)]
const AFTER_MARK: &str = "FD_LIMIT_AFTER_SOFT=";

/// Run the probe under a soft limit of 256 with `mode`; its stdout.
#[cfg(unix)]
fn run_probe(mode: &str) -> String {
    let me = std::env::current_exe().expect("test binary path");
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("ulimit -Sn 256 && [ \"$(ulimit -Sn)\" = 256 ] && exec \"$0\" fd_limit::tests::fd_limit_inherit_probe --exact --ignored --nocapture --test-threads=1")
        .arg(&me)
        .env(PROBE_ENV, mode)
        .output()
        .expect("run the probe");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "probe failed: {stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("[fd-limit] open-file limit: soft 256 -> "), "the probe started from 256: {stdout}");
    stdout
}

/// A value the probe printed after `mark`. Found anywhere in a line: with `--nocapture` the
/// probe's first line shares libtest's "test ... " prefix (seen on macos-15).
#[cfg(unix)]
fn probe_value(stdout: &str, mark: &str) -> u64 {
    stdout
        .lines()
        .find_map(|l| l.split_once(mark).map(|(_, v)| v.trim()))
        .unwrap_or_else(|| panic!("the probe printed {mark}: {stdout}"))
        .parse()
        .expect("numeric")
}

/// Review P2, on a real Mac: the start-up raise reads kern.maxfilesperproc with sysctlbyname and
/// clamps to it: the soft limit after is min(65536, that cap), and a spawned child has it.
#[cfg(target_os = "macos")]
#[test]
fn macos_raise_clamps_to_the_kernel_cap() {
    let out = run_probe("1");
    let cap = probe_value(&out, CAP_MARK);
    assert!(cap >= MACOS_OPEN_MAX.min(4096), "sysctlbyname read a real cap: {out}");
    let after = probe_value(&out, AFTER_MARK);
    let (_, hard) = get_nofile().expect("getrlimit");
    assert_eq!(after, target_soft(256, hard, TARGET_NOFILE, Some(cap)), "clamped to min(target, hard, kern.maxfilesperproc): {out}");
    assert_eq!(probe_value(&out, PROBE_MARK), after, "the child inherited it: {out}");
}

/// Review P2, on a real Mac: asking past the kernel cap takes the refused path. Either setrlimit
/// refuses and the OPEN_MAX retry sets 10240, or the kernel clamps; both end at or under the cap
/// and well above 256. The report says which.
#[cfg(target_os = "macos")]
#[test]
fn macos_over_cap_request_falls_back_to_open_max() {
    let out = run_probe("over-cap");
    let cap = probe_value(&out, CAP_MARK);
    let after = probe_value(&out, AFTER_MARK);
    assert!((4096..=cap.max(MACOS_OPEN_MAX)).contains(&after), "after {after}, cap {cap}: {out}");
    if out.contains("refused") {
        assert!(out.contains(&format!("used {MACOS_OPEN_MAX}")), "the OPEN_MAX retry ran: {out}");
        assert_eq!(after, MACOS_OPEN_MAX);
    }
    println!("{out}");
}

/// Acceptance (0.5.7): with the app's own soft limit starting at 256, a process spawned after the
/// start-up raise has a soft "Max open files" of at least 4096. `ulimit -Sn` (soft only): a bare
/// `ulimit -n` would drop the hard limit too and nothing could be raised.
#[cfg(unix)]
#[test]
fn a_child_spawned_after_the_raise_inherits_it() {
    let (_, hard) = get_nofile().expect("getrlimit");
    assert!(hard >= 4096, "this host's hard RLIMIT_NOFILE is {hard}; the acceptance needs at least 4096");
    let stdout = run_probe("1");
    let soft = probe_value(&stdout, PROBE_MARK);
    assert!(soft >= 4096, "the spawned child's soft open-file limit is {soft}, want at least 4096: {stdout}");
}
