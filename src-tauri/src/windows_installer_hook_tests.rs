//! SCL-S0.7 (RT-04): the Windows installer's pre-install hook, checked on every OS. The hook
//! itself runs on a hosted Windows runner (`.github/workflows/windows-installer-hook.yml`,
//! `src-tauri/windows/tests/test-stop-own-sidecars.ps1`); these tests keep its inputs honest.

const WINDOWS_CONF: &str = include_str!("../tauri.bundle-windows.conf.json");
const HOOKS: &str = include_str!("../windows/installer-hooks.nsh");
const STOP_SCRIPT: &str = include_str!("../windows/stop-own-sidecars.ps1");

fn windows_conf() -> serde_json::Value {
    serde_json::from_str(WINDOWS_CONF).expect("tauri.bundle-windows.conf.json is JSON")
}

/// The sidecar list the hook passes to the stop script.
fn hook_sidecars() -> Vec<String> {
    let line = HOOKS
        .lines()
        .find(|l| l.starts_with("!define CITRATE_SIDECARS "))
        .expect("installer-hooks.nsh defines CITRATE_SIDECARS");
    let quoted = line
        .split('"')
        .nth(1)
        .expect("CITRATE_SIDECARS is a quoted list");
    quoted.split(',').map(|s| s.trim().to_string()).collect()
}

#[test]
fn the_windows_bundle_uses_the_installer_hooks_file() {
    let conf = windows_conf();
    let hooks = conf["bundle"]["windows"]["nsis"]["installerHooks"]
        .as_str()
        .expect("bundle > windows > nsis > installerHooks is set");
    assert_eq!(hooks, "./windows/installer-hooks.nsh");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(hooks);
    assert!(path.is_file(), "{} exists", path.display());
    assert!(
        HOOKS.contains("!macro NSIS_HOOK_PREINSTALL"),
        "the hook Tauri expands before copying files is defined"
    );
}

#[test]
fn the_hook_stops_exactly_the_sidecars_the_windows_bundle_ships() {
    let conf = windows_conf();
    let mut shipped: Vec<String> = conf["bundle"]["externalBin"]
        .as_array()
        .expect("externalBin")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("externalBin entries are strings")
                .rsplit('/')
                .next()
                .expect("a file name")
                .to_string()
        })
        .collect();
    let mut listed = hook_sidecars();
    shipped.sort();
    listed.sort();
    assert_eq!(listed, shipped, "CITRATE_SIDECARS must equal externalBin");
    for n in &listed {
        assert!(
            !n.is_empty()
                && n.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                && !n.contains(".."),
            "a plain file name: {n:?}"
        );
    }
}

#[test]
fn the_stop_step_matches_exact_paths_and_never_bare_names() {
    let lower = STOP_SCRIPT.to_ascii_lowercase();
    // Never a name-wide kill.
    for banned in [
        "/im ",
        "stop-process -name",
        "-processname",
        "get-process -name",
        "/t ",
    ] {
        assert!(
            !lower.contains(banned),
            "stop script must not use {banned:?}"
        );
    }
    // The decision is exact full-path equality against <InstallDir>\<name>.exe.
    assert!(STOP_SCRIPT.contains("[System.IO.Path]::Combine($root, \"$name.exe\")"));
    assert!(STOP_SCRIPT.contains("$targets.Contains($full)"));
    assert!(STOP_SCRIPT.contains("OrdinalIgnoreCase"));
    // Graceful step first, then terminate, each by pid.
    let graceful = STOP_SCRIPT
        .find("taskkill.exe\" /PID")
        .expect("graceful step by pid");
    let terminate = STOP_SCRIPT
        .find("Stop-Process -Id")
        .expect("terminate by pid");
    assert!(graceful < terminate, "graceful step before terminate");
    // The hook itself signals nothing by name: it only runs the script on $INSTDIR.
    let hooks_lower = HOOKS.to_ascii_lowercase();
    for banned in ["killprocess", "taskkill", "stop-process"] {
        assert!(
            !hooks_lower.contains(banned),
            "hook must not call {banned:?}"
        );
    }
    assert!(HOOKS.contains("-InstallDir \"$INSTDIR\""));
}

#[test]
fn the_hook_runs_the_main_app_check_before_stopping_sidecars() {
    let body = HOOKS
        .split("!macro NSIS_HOOK_PREINSTALL")
        .nth(1)
        .and_then(|s| s.split("!macroend").next())
        .expect("NSIS_HOOK_PREINSTALL body");
    let check = body
        .find("CheckIfAppIsRunning \"${MAINBINARYNAME}.exe\"")
        .expect("main-app check");
    let stop = body
        .find("CITRATE_STOP_OWN_SIDECARS")
        .expect("sidecar stop");
    assert!(
        check < stop,
        "the old app must be gone before its sidecars are stopped"
    );
}

/// The body of the stop-step macro the pre-install hook expands.
fn stop_macro() -> &'static str {
    HOOKS
        .split("!macro CITRATE_STOP_OWN_SIDECARS")
        .nth(1)
        .and_then(|s| s.split("!macroend").next())
        .expect("CITRATE_STOP_OWN_SIDECARS body")
}

/// A `!define NAME <int>` in installer-hooks.nsh (the first, i.e. the shipped default).
fn hook_define_ms(name: &str) -> u64 {
    let prefix = format!("!define {name} ");
    HOOKS
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(prefix.as_str()))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("installer-hooks.nsh defines {name} as a number"))
}

/// A `[int]$Name = <int>` parameter default in stop-own-sidecars.ps1.
fn script_default_secs(name: &str) -> u64 {
    let needle = format!("[int]${name} = ");
    let at = STOP_SCRIPT
        .find(needle.as_str())
        .unwrap_or_else(|| panic!("stop script has {needle:?}"))
        + needle.len();
    STOP_SCRIPT[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("a number")
}

#[test]
fn the_stop_step_waits_for_powershell_with_a_finite_outer_bound() {
    // #305: PowerShell start-up or the CIM query can hang; the wait on it must be bounded.
    let body = stop_macro();
    for unbounded in ["nsexec::", "execwait", "execshellwait", "execdos::"] {
        assert!(
            !body.to_ascii_lowercase().contains(unbounded),
            "the stop step must not wait through {unbounded:?} (no outer bound)"
        );
    }
    assert!(body.contains("kernel32::CreateProcessW("));
    assert!(body.contains("WaitForSingleObject(p r4, i ${CITRATE_STOP_TIMEOUT_MS})"));
    let bound = hook_define_ms("CITRATE_STOP_TIMEOUT_MS");
    assert!(
        (10_000..=120_000).contains(&bound),
        "a bound of 10-120 s, got {bound} ms"
    );
    // The script's own worst case fits inside the bound, so a healthy machine never hits it.
    let own = (script_default_secs("GraceSeconds") + script_default_secs("KillWaitSeconds")) * 1000;
    assert!(
        own * 2 <= bound,
        "the script's own waits ({own} ms) must sit well inside the outer bound ({bound} ms)"
    );
    assert!(hook_define_ms("CITRATE_STOP_REAP_MS") <= 10_000);
}

#[test]
fn a_timeout_ends_only_what_the_stop_step_started() {
    let body = stop_macro();
    // Started suspended, put in a job of its own, then resumed: nothing runs outside the job.
    assert!(HOOKS.contains("!define CITRATE_CREATE_FLAGS 0x08000004"));
    let create = body.find("CreateProcessW(").expect("create");
    let assign = body
        .find("AssignProcessToJobObject(p r9, p r4)")
        .expect("assign");
    let resume = body.find("ResumeThread(p r5)").expect("resume");
    assert!(
        create < assign && assign < resume,
        "create suspended, join the job, then resume"
    );
    // On timeout: that job (or, without a job, that one process handle), nothing by name.
    let timeout = body.find("StrCpy $8 \"timeout\"").expect("timeout branch");
    let after = &body[timeout..];
    assert!(after.contains("TerminateJobObject(p r9, i 1)"));
    assert!(after.contains("TerminateProcess(p r4, i 1)"));
    assert!(after.contains("WaitForSingleObject(p r4, i ${CITRATE_STOP_REAP_MS})"));
    for h in [
        "CloseHandle(p r5)",
        "CloseHandle(p r4)",
        "CloseHandle(p r9)",
    ] {
        assert!(body.contains(h), "the step closes {h}");
    }
}

#[test]
fn a_timeout_is_reported_and_the_install_continues() {
    let body = stop_macro();
    assert!(
        body.contains("${If} $8 == \"timeout\"")
            && body.contains("did not finish within ${CITRATE_STOP_TIMEOUT_MS} ms and was stopped (result timeout); installing continues"),
        "a timeout is a visible detail line"
    );
    assert!(body.contains("could not be started (result error); installing continues"));
    // Fail-open: the step never stops the installer.
    let lower = body.to_ascii_lowercase();
    for stop in ["abort", "quit", "seterrorlevel", "messagebox"] {
        assert!(
            !lower.contains(stop),
            "the stop step must not {stop:?} the install"
        );
    }
    // The script's log is replayed into the details, and the result file exists only in test builds.
    assert!(STOP_SCRIPT.contains("[string]$LogPath = ''"));
    assert!(body.contains("-LogPath \"$PLUGINSDIR\\citrate-stop-own-sidecars.log\""));
    assert!(body.contains("FileReadUTF16LE"));
    let test_only = body
        .find("!ifdef CITRATE_HOOK_TEST_RESULT_FILE")
        .expect("test-only result file");
    assert!(!body[..test_only].contains("CITRATE_HOOK_TEST_RESULT_FILE"));
}
