//! V60 SVC-1: the rendered units, their static validation, and the install plan. No test runs
//! `launchctl`, `systemctl` or `loginctl`: every effect goes through the recording runner.

use super::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

const MAC_EXE: &str = "/Applications/Citrate Core.app/Contents/MacOS/citrate-core";

/// Records each command; answers from a script (default success). Test-only.
struct Recorder {
    calls: RefCell<Vec<String>>,
    fail: Vec<&'static str>,
}
impl Recorder {
    fn new(fail: Vec<&'static str>) -> Self {
        Recorder {
            calls: RefCell::new(Vec::new()),
            fail,
        }
    }
}
impl CommandRunner for Recorder {
    fn run(&self, program: &str, args: &[String]) -> Result<bool, String> {
        let line = format!("{program} {}", args.join(" "));
        self.calls.borrow_mut().push(line.clone());
        Ok(!self.fail.iter().any(|f| line.starts_with(f)))
    }
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("csv-units-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn the_plist_runs_the_app_executable_with_the_svc_flag() {
    let s = render_plist(
        Path::new(MAC_EXE),
        Path::new("/Users/m/Library/Logs/ai.citrate.core/svc.log"),
    )
    .unwrap();
    assert!(s.contains("<string>ai.citrate.svc</string>"));
    assert!(s.contains(&format!(
        "<string>{MAC_EXE}</string>\n\t\t<string>--svc</string>"
    )));
    // A crash restarts, a requested stop does not; the stop budget is above the 30 s drain.
    assert!(s.contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
    assert!(s.contains("<key>ExitTimeOut</key>\n\t<integer>40</integer>"));
    assert!(s.contains("<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>"));
    assert!(s.contains("<key>Umask</key>\n\t<integer>63</integer>"));
    assert!(!s.contains("{{"));
}

#[test]
fn the_systemd_unit_runs_the_app_executable_with_the_svc_flag() {
    let s = render_systemd(Path::new("/home/m/Apps/Citrate Core.AppImage")).unwrap();
    assert!(s.contains("ExecStart=\"/home/m/Apps/Citrate Core.AppImage\" --svc\n"));
    for line in [
        "Restart=on-failure",
        "RestartSec=10",
        "TimeoutStopSec=40",
        "KillMode=mixed",
        "UMask=0077",
        "WantedBy=default.target",
    ] {
        assert!(s.lines().any(|l| l == line), "missing {line}");
    }
    assert!(!s.contains("{{"));
}

#[test]
fn the_stop_budget_is_above_the_drain() {
    // TimeoutStopSec / ExitTimeOut are 40 s: the unit manager must not SIGKILL a drain that is
    // still inside its own 30 s exit wait.
    assert!(crate::update_drain::EXIT_WAIT < std::time::Duration::from_secs(40));
}

#[test]
fn paths_are_escaped_for_each_format() {
    assert_eq!(xml_escape("a&b<c>\"'"), "a&amp;b&lt;c&gt;&quot;&apos;");
    assert_eq!(systemd_quote("/a b/%h$X\"\\"), "\"/a b/%%h$$X\\\"\\\\\"");
    let p = render_plist(Path::new("/A&B/x"), Path::new("/l")).unwrap();
    assert!(p.contains("<string>/A&amp;B/x</string>"));
    // A newline would end ExecStart= or the XML string: refused, as are relative paths.
    assert!(render_systemd(Path::new("/a\nExecStartPre=/bin/evil")).is_err());
    assert!(render_plist(Path::new("rel/exe"), Path::new("/l")).is_err());
}

#[test]
fn service_exe_is_the_appimage_when_there_is_one() {
    assert_eq!(
        service_exe(
            Some(PathBuf::from("/home/m/C.AppImage")),
            Some(PathBuf::from("/tmp/.mount/usr/bin/citrate-core"))
        ),
        Some(PathBuf::from("/home/m/C.AppImage"))
    );
    assert_eq!(
        service_exe(None, Some(PathBuf::from(MAC_EXE))),
        Some(PathBuf::from(MAC_EXE))
    );
}

/// Static validation of the rendered LaunchAgent with the system's own checker (macOS only:
/// `plutil -lint` reads the file and loads nothing).
#[cfg(target_os = "macos")]
#[test]
fn the_rendered_plist_passes_plutil_lint() {
    let dir = tmp("plutil");
    let f = dir.join("ai.citrate.svc.plist");
    std::fs::write(
        &f,
        render_plist(Path::new(MAC_EXE), &dir.join("svc.log")).unwrap(),
    )
    .unwrap();
    let out = std::process::Command::new("plutil")
        .arg("-lint")
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Static validation of the rendered unit with `systemd-analyze verify` (Linux with systemd:
/// it parses the file and checks the ExecStart binary exists; it starts nothing).
#[cfg(target_os = "linux")]
#[test]
fn the_rendered_unit_passes_systemd_analyze_verify() {
    use std::os::unix::fs::PermissionsExt;
    let Ok(probe) = std::process::Command::new("systemd-analyze")
        .arg("--version")
        .output()
    else {
        eprintln!("SKIPPED: systemd-analyze is not installed on this host");
        return;
    };
    assert!(probe.status.success());
    let dir = tmp("analyze");
    // An executable at a path with a space, as an AppImage in "~/Citrate Apps" would be.
    let sub = dir.join("Citrate Apps");
    std::fs::create_dir_all(&sub).unwrap();
    let exe = sub.join("Citrate-Core.AppImage");
    std::fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let unit = dir.join(SYSTEMD_UNIT);
    std::fs::write(&unit, render_systemd(&exe).unwrap()).unwrap();
    let out = std::process::Command::new("systemd-analyze")
        .args(["--user", "verify"])
        .arg(&unit)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(
        !text.contains(SYSTEMD_UNIT),
        "warnings about the unit: {text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn macos_install_writes_the_agent_and_bootstraps_it() {
    let home = tmp("mac");
    let plan = macos_plan(&home, 501, Path::new(MAC_EXE), &home.join("svc.log")).unwrap();
    assert_eq!(
        plan.unit_path,
        home.join("Library/LaunchAgents/ai.citrate.svc.plist")
    );
    // Nothing loaded yet: the bootout of an older copy fails and is tolerated.
    let r = Recorder::new(vec!["launchctl bootout"]);
    let rep = install(&plan, &r).unwrap();
    assert_eq!(
        r.calls.borrow().as_slice(),
        [
            "launchctl bootout gui/501/ai.citrate.svc".to_string(),
            format!("launchctl bootstrap gui/501 {}", plan.unit_path.display()),
        ]
    );
    assert!(!rep.runs_logged_out);
    assert!(installed(&plan));
    assert_eq!(
        std::fs::read_to_string(&plan.unit_path).unwrap(),
        plan.contents
    );
    // A failed bootstrap is an error.
    let r = Recorder::new(vec!["launchctl bootstrap"]);
    assert!(install(&plan, &r).is_err());
    // Uninstall boots it out and removes the file; twice is fine.
    let r = Recorder::new(vec![]);
    uninstall(&plan, &r).unwrap();
    uninstall(&plan, &r).unwrap();
    assert!(!installed(&plan));
    assert_eq!(
        r.calls.borrow()[0],
        "launchctl bootout gui/501/ai.citrate.svc"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn linux_install_enables_the_unit_and_reports_lingering() {
    let cfg = tmp("linux");
    let exe = Path::new("/usr/bin/citrate-core");
    let plan = linux_plan(&cfg, "member", exe).unwrap();
    assert_eq!(plan.unit_path, cfg.join("systemd/user/citrate-svc.service"));
    let r = Recorder::new(vec![]);
    let rep = install(&plan, &r).unwrap();
    assert_eq!(
        r.calls.borrow().as_slice(),
        [
            "systemctl --user daemon-reload",
            "systemctl --user enable --now citrate-svc.service",
            "loginctl enable-linger member",
        ]
    );
    assert!(rep.runs_logged_out);
    assert!(rep.notes.is_empty());
    // Lingering refused (a managed machine): installed, and the member is told what that means.
    let r = Recorder::new(vec!["loginctl"]);
    let rep = install(&plan, &r).unwrap();
    assert!(!rep.runs_logged_out);
    assert!(rep.notes[0].contains("stops when you log out"));
    // A failed enable is an error.
    let r = Recorder::new(vec!["systemctl --user enable"]);
    assert!(install(&plan, &r).is_err());
    let r = Recorder::new(vec![]);
    uninstall(&plan, &r).unwrap();
    assert_eq!(
        r.calls.borrow().as_slice(),
        [
            "systemctl --user disable --now citrate-svc.service",
            "systemctl --user daemon-reload"
        ]
    );
    assert!(!installed(&plan));
    let _ = std::fs::remove_dir_all(&cfg);
}
