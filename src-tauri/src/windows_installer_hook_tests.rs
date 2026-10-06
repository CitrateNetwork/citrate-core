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
