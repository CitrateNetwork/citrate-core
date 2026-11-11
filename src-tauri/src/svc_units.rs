//! V60 SVC-1: the service's unit files and their install/remove plan.
//!
//! - macOS: a per-user LaunchAgent, `~/Library/LaunchAgents/ai.citrate.svc.plist`
//!   (`src-tauri/svc/ai.citrate.svc.plist`), loaded with `launchctl bootstrap gui/<uid>`.
//! - Linux: a `systemd --user` unit, `~/.config/systemd/user/citrate-svc.service`
//!   (`src-tauri/svc/citrate-svc.service`), enabled with `systemctl --user enable --now`, and
//!   lingering asked for with `loginctl enable-linger` so it runs while the member is logged out.
//!   A refused linger is reported, not hidden: the service then runs only while logged in.
//!
//! The executable in both is the app's own (`citrate-core --svc`, see [`crate::svc`]): the
//! AppImage file itself when the app runs from one (`$APPIMAGE`), else the running executable.
//!
//! Every effect goes through a [`CommandRunner`]: the app passes [`SystemRunner`]; the tests
//! pass a recorder, so no test ever runs `launchctl`, `systemctl` or `loginctl`.
//! `SMAppService` registration (macOS 13+) is not used: it needs the plist inside the bundle
//! and an Objective-C call; the LaunchAgent file works on every supported macOS.

use std::path::{Path, PathBuf};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The LaunchAgent label.
pub const LAUNCH_AGENT_LABEL: &str = "ai.citrate.svc";
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// The systemd user unit name.
pub const SYSTEMD_UNIT: &str = "citrate-svc.service";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PLIST_TEMPLATE: &str = include_str!("../svc/ai.citrate.svc.plist");
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const SYSTEMD_TEMPLATE: &str = include_str!("../svc/citrate-svc.service");

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Escape text for an XML element.
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// Quote a path as one systemd command-line word: double quotes, `\` and `"` escaped, `%`
/// doubled (specifier) and `$` doubled (variable expansion).
pub(crate) fn systemd_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A path that can go into a unit: absolute, no control characters (a newline would end the
/// `ExecStart=` line or the XML string).
fn check_path(p: &Path) -> Result<String, String> {
    let s = p
        .to_str()
        .ok_or_else(|| format!("not a UTF-8 path: {}", p.display()))?;
    if !p.is_absolute() {
        return Err(format!("not an absolute path: {s}"));
    }
    if s.chars().any(char::is_control) {
        return Err("the path contains a control character".to_string());
    }
    Ok(s.to_string())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The rendered LaunchAgent.
pub fn render_plist(exe: &Path, log: &Path) -> Result<String, String> {
    let exe = check_path(exe)?;
    let log = check_path(log)?;
    Ok(PLIST_TEMPLATE
        .replace("{{EXE}}", &xml_escape(&exe))
        .replace("{{LOG}}", &xml_escape(&log)))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// The rendered systemd user unit.
pub fn render_systemd(exe: &Path) -> Result<String, String> {
    let exe = check_path(exe)?;
    Ok(SYSTEMD_TEMPLATE.replace("{{EXE}}", &systemd_quote(&exe)))
}

/// The executable the unit runs: `$APPIMAGE` when absolute (the AppImage's own binaries are in
/// a mount that exists only while it runs), else the running executable.
pub fn service_exe(appimage: Option<PathBuf>, current_exe: Option<PathBuf>) -> Option<PathBuf> {
    crate::svc::exe_to_watch(appimage, current_exe)
}

/// Runs a program (the unit manager's CLI).
pub trait CommandRunner {
    /// Run `program args…`; `Ok(status success)`, `Err` when it could not be started.
    fn run(&self, program: &str, args: &[String]) -> Result<bool, String>;
}

/// The real runner.
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<bool, String> {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .map_err(|e| format!("{program}: {}", e.kind()))
    }
}

/// Where the unit goes and how it is loaded, for one platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitPlan {
    pub unit_path: PathBuf,
    pub contents: String,
    /// Run in order after the file is written; a failure stops the install.
    pub load: Vec<(String, Vec<String>)>,
    /// Run after `load`; a failure is reported, not fatal (lingering).
    pub best_effort: Vec<(String, Vec<String>)>,
    /// Run before the file is removed; failures are ignored (not loaded is fine).
    pub unload: Vec<(String, Vec<String>)>,
}

fn cmd(p: &str, a: &[&str]) -> (String, Vec<String>) {
    (p.to_string(), a.iter().map(|s| s.to_string()).collect())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The macOS plan for user `uid`.
pub fn macos_plan(home: &Path, uid: u32, exe: &Path, log: &Path) -> Result<UnitPlan, String> {
    let unit_path = home
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"));
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{LAUNCH_AGENT_LABEL}");
    let unit = unit_path.to_str().ok_or("not a UTF-8 path")?.to_string();
    Ok(UnitPlan {
        contents: render_plist(exe, log)?,
        // A loaded older copy is booted out first so the new file is what runs.
        load: vec![
            cmd("launchctl", &["bootout", &target]),
            cmd("launchctl", &["bootstrap", &domain, &unit]),
        ],
        best_effort: Vec::new(),
        unload: vec![cmd("launchctl", &["bootout", &target])],
        unit_path,
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// The Linux plan for `user`.
pub fn linux_plan(config_home: &Path, user: &str, exe: &Path) -> Result<UnitPlan, String> {
    let unit_path = config_home.join("systemd/user").join(SYSTEMD_UNIT);
    Ok(UnitPlan {
        contents: render_systemd(exe)?,
        load: vec![
            cmd("systemctl", &["--user", "daemon-reload"]),
            cmd("systemctl", &["--user", "enable", "--now", SYSTEMD_UNIT]),
        ],
        best_effort: vec![cmd("loginctl", &["enable-linger", user])],
        unload: vec![cmd(
            "systemctl",
            &["--user", "disable", "--now", SYSTEMD_UNIT],
        )],
        unit_path,
    })
}

/// What an install did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub unit_path: String,
    /// Linux: whether lingering was granted (the service runs while logged out). macOS: false
    /// (a LaunchAgent runs only in the member's session).
    pub runs_logged_out: bool,
    /// Plain notes for the member (a refused linger, a bootout of an older copy).
    pub notes: Vec<String>,
}

/// Write the unit and load it. The first `load` step of the macOS plan (bootout of an older
/// copy) may fail when nothing is loaded; every other `load` step must succeed.
pub fn install(plan: &UnitPlan, runner: &dyn CommandRunner) -> Result<InstallReport, String> {
    if let Some(dir) = plan.unit_path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("create {}: {}", dir.display(), e.kind()))?;
    }
    crate::atomic_file::replace(&plan.unit_path, plan.contents.as_bytes())
        .map_err(|e| format!("write {}: {e}", plan.unit_path.display()))?;
    let mut notes = Vec::new();
    for (i, (p, a)) in plan.load.iter().enumerate() {
        let ok = runner.run(p, a)?;
        let tolerated =
            p == "launchctl" && a.first().map(String::as_str) == Some("bootout") && i == 0;
        if !ok && !tolerated {
            return Err(format!("`{p} {}` failed", a.join(" ")));
        }
    }
    let mut runs_logged_out = false;
    for (p, a) in &plan.best_effort {
        match runner.run(p, a) {
            Ok(true) => runs_logged_out = true,
            Ok(false) | Err(_) => notes.push(
                "Lingering was not granted: the service runs while you are logged in and stops when you log out.".to_string(),
            ),
        }
    }
    Ok(InstallReport {
        unit_path: plan.unit_path.display().to_string(),
        runs_logged_out,
        notes,
    })
}

/// Unload the unit and remove its file (idempotent).
pub fn uninstall(plan: &UnitPlan, runner: &dyn CommandRunner) -> Result<(), String> {
    for (p, a) in &plan.unload {
        let _ = runner.run(p, a);
    }
    match std::fs::remove_file(&plan.unit_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("remove {}: {}", plan.unit_path.display(), e.kind())),
    }
    if plan.unit_path.extension().and_then(|e| e.to_str()) == Some("service") {
        let _ = runner.run(
            "systemctl",
            &["--user".to_string(), "daemon-reload".to_string()],
        );
    }
    Ok(())
}

/// Whether the unit file is in place.
pub fn installed(plan: &UnitPlan) -> bool {
    plan.unit_path.is_file()
}

#[cfg(test)]
#[path = "svc_units_tests.rs"]
mod tests;
