//! V60 SVC-2: the member's background-service choice, the power rules, and the commands the
//! onboarding step and Settings › Node use.
//!
//! Planset: fed `.agentile/planset/2026-11-07-v060/02_ARCHITECTURE.md` §9 (Setup, Power),
//! `03_UX.md` §3.5, `04_FEATURES_BDD.md` F-B1. Owner decision 2026-11-07: on by default, with an
//! opt-out at setup that says a closed app won't produce blocks.
//!
//! - **Choice:** `<app data>/svc/settings.json` holds `enabled` (absent until the member's first
//!   setup; the onboarding step then applies the default, on) and the power rules. Turning it on
//!   writes and loads the unit ([`crate::svc_units`]) and hands the node and kubo over from the
//!   app to the service; turning it off drains the service, removes the unit, and the app runs
//!   them in-process again.
//! - **Power** (Settings › Node › Power): training only on AC power (default), pause on battery
//!   below 30 %, an optional nightly window. [`training_allowed`] is the one decision; its power
//!   reading comes from `pmset -g batt` (macOS) or `/sys/class/power_supply` (Linux). Today no
//!   training runs from the app (the device worker is not bundled, see `fl_rounds`), so the
//!   decision is shown in Settings and exported for the worker; it gates nothing yet.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The exact opt-out sentence (planset 02 §9, 03 §3.5, F-B1). The webview has the same string in
/// `src/shell/svcCopy.ts`; `svcCopy.test.ts` and [`OPT_OUT_COPY`]'s test pin both.
pub const OPT_OUT_COPY: &str =
    "If the app is closed and the service is off, your node won't produce blocks.";

/// The settings file inside the service directory.
pub const SETTINGS_FILE: &str = "settings.json";

/// A nightly window in local hours, `from` inclusive, `to` exclusive; wraps midnight when
/// `from > to` (23 → 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub from: u8,
    pub to: u8,
}

impl Window {
    pub fn contains(&self, hour: u8) -> bool {
        if self.from == self.to {
            return true;
        }
        if self.from < self.to {
            hour >= self.from && hour < self.to
        } else {
            hour >= self.from || hour < self.to
        }
    }
}

/// Settings › Node › Power.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerPolicy {
    /// Train on battery at all. Off by default: training only on AC.
    pub train_on_battery: bool,
    /// When training on battery is allowed, pause below this charge.
    pub pause_below_pct: u8,
    /// Train only inside this window (optional).
    pub nightly: Option<Window>,
}

impl Default for PowerPolicy {
    fn default() -> Self {
        PowerPolicy {
            train_on_battery: false,
            pause_below_pct: 30,
            nightly: None,
        }
    }
}

/// What is in `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SvcSettings {
    /// `None` until the member's first setup.
    pub enabled: Option<bool>,
    #[serde(default)]
    pub power: PowerPolicy,
}

pub fn settings_path(svc_dir: &Path) -> PathBuf {
    svc_dir.join(SETTINGS_FILE)
}

/// Read the settings; a missing or unreadable file is the default (undecided, default power).
pub fn load(svc_dir: &Path) -> SvcSettings {
    std::fs::read_to_string(settings_path(svc_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(svc_dir: &Path, s: &SvcSettings) -> Result<(), String> {
    citrate_core_kit::fsutil::ensure_private_dir(svc_dir)
        .map_err(|e| format!("service directory: {}", e.kind()))?;
    let body = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    crate::atomic_file::replace(&settings_path(svc_dir), &body)
        .map_err(|e| format!("cannot save the service settings: {}", e.kind()))
}

/// Validate a policy from the webview.
pub fn check_policy(p: &PowerPolicy) -> Result<(), String> {
    if p.pause_below_pct > 100 {
        return Err("the battery threshold is a percentage (0 to 100)".to_string());
    }
    if let Some(w) = p.nightly {
        if w.from > 23 || w.to > 23 {
            return Err("the nightly window uses hours 0 to 23".to_string());
        }
    }
    Ok(())
}

/// The machine's power state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerState {
    /// `None` when it could not be read.
    pub on_ac: Option<bool>,
    pub battery_pct: Option<u8>,
    pub has_battery: bool,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Parse `pmset -g batt`: "Now drawing from 'AC Power'" or "'Battery Power'", and the first
/// "NN%" on the battery line.
pub fn parse_pmset(out: &str) -> PowerState {
    let on_ac = if out.contains("'AC Power'") {
        Some(true)
    } else if out.contains("'Battery Power'") {
        Some(false)
    } else {
        None
    };
    let battery_line = out.lines().find(|l| l.contains("InternalBattery"));
    let battery_pct = battery_line.and_then(|l| {
        let i = l.find('%')?;
        let digits: String = l[..i]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        digits.parse::<u8>().ok().filter(|p| *p <= 100)
    });
    PowerState {
        // A desktop Mac has no battery line and draws from AC.
        on_ac,
        battery_pct,
        has_battery: battery_line.is_some(),
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
/// Read `/sys/class/power_supply`: a `Mains`/`USB` supply that is `online` means AC; a
/// `Battery` supply's `capacity` is the charge. No battery and no supply files: a desktop.
pub fn read_sysfs(root: &Path) -> PowerState {
    let mut mains_online: Option<bool> = None;
    let mut battery_pct = None;
    let mut has_battery = false;
    let mut discharging = false;
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
            match kind.trim() {
                "Mains" | "USB" => {
                    let online = std::fs::read_to_string(p.join("online"))
                        .map(|s| s.trim() == "1")
                        .unwrap_or(false);
                    mains_online = Some(mains_online.unwrap_or(false) || online);
                }
                "Battery" => {
                    has_battery = true;
                    battery_pct = std::fs::read_to_string(p.join("capacity"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u8>().ok())
                        .filter(|v| *v <= 100);
                    discharging |= std::fs::read_to_string(p.join("status"))
                        .map(|s| s.trim() == "Discharging")
                        .unwrap_or(false);
                }
                _ => {}
            }
        }
    }
    let on_ac = match (mains_online, has_battery) {
        (Some(m), _) => Some(m),
        // A battery with no mains supply listed: AC unless it is discharging.
        (None, true) => Some(!discharging),
        // No supplies at all: a desktop or a server, always on AC.
        (None, false) => Some(true),
    };
    PowerState {
        on_ac,
        battery_pct,
        has_battery,
    }
}

/// This machine's power state, read now.
pub fn read_power() -> PowerState {
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("/usr/bin/pmset")
            .args(["-g", "batt"])
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(o) if o.status.success() => parse_pmset(&String::from_utf8_lossy(&o.stdout)),
            _ => PowerState {
                on_ac: None,
                battery_pct: None,
                has_battery: false,
            },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        read_sysfs(Path::new("/sys/class/power_supply"))
    }
}

/// Whether training may run now under `policy`, and why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingVerdict {
    pub allowed: bool,
    pub reason: String,
}

/// The one decision. An unreadable power state holds training (it is not known to be on AC).
pub fn training_allowed(
    policy: &PowerPolicy,
    power: &PowerState,
    local_hour: u8,
) -> TrainingVerdict {
    let no = |r: &str| TrainingVerdict {
        allowed: false,
        reason: r.to_string(),
    };
    if let Some(w) = policy.nightly {
        if !w.contains(local_hour) {
            return no(&format!(
                "outside the training window ({:02}:00 to {:02}:00)",
                w.from, w.to
            ));
        }
    }
    match power.on_ac {
        None => no("the power state could not be read"),
        Some(true) => TrainingVerdict {
            allowed: true,
            reason: "on AC power".to_string(),
        },
        Some(false) if !policy.train_on_battery => no("on battery; training runs only on AC power"),
        Some(false) => match power.battery_pct {
            Some(p) if p < policy.pause_below_pct => no(&format!(
                "on battery at {p} %, below {} %",
                policy.pause_below_pct
            )),
            Some(_) => TrainingVerdict {
                allowed: true,
                reason: "on battery, above the threshold".to_string(),
            },
            None => no("on battery and the charge could not be read"),
        },
    }
}

/// The local hour (0 to 23) from the C library's local time.
pub(crate) fn local_hour_now() -> u8 {
    #[cfg(unix)]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as libc::time_t)
            .unwrap_or(0);
        let mut tm: libc::tm = unsafe_zeroed_tm();
        // SAFETY: localtime_r writes only into `tm`, which outlives the call; `now` is a valid
        // time_t on the stack.
        #[allow(unsafe_code)]
        let ok = unsafe { !libc::localtime_r(&now, &mut tm).is_null() };
        if ok {
            return tm.tm_hour.clamp(0, 23) as u8;
        }
        0
    }
    #[cfg(not(unix))]
    {
        0
    }
}

#[cfg(unix)]
fn unsafe_zeroed_tm() -> libc::tm {
    // SAFETY: libc::tm is a plain C struct of integers (and, on some platforms, a nullable
    // `const char*` zone pointer); all-zero is a valid value for each field.
    #[allow(unsafe_code)]
    unsafe {
        std::mem::zeroed()
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// What Settings › Node and the onboarding step show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SvcView {
    /// macOS and Linux; false elsewhere (the app is always in-process there).
    pub supported: bool,
    /// The member's choice; `None` before the first setup.
    pub enabled: Option<bool>,
    pub unit_installed: bool,
    /// The app is talking to a running service.
    pub attached: bool,
    /// Why the app is in-process (when not attached).
    pub in_process_reason: Option<String>,
    /// The service's own status, when attached.
    pub service: Option<crate::svc::SvcStatus>,
    /// From the last install: whether it also runs while logged out (Linux lingering).
    pub runs_logged_out: Option<bool>,
    pub notes: Vec<String>,
    pub opt_out_copy: &'static str,
    pub power: PowerPolicy,
}

fn data_root<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// The unit plan for this machine.
fn plan<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<crate::svc_units::UnitPlan, String> {
    let exe = crate::svc_units::service_exe(
        std::env::var_os("APPIMAGE").map(PathBuf::from),
        std::env::current_exe().ok(),
    )
    .ok_or("cannot find this app's executable")?;
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        let home = dirs::home_dir().ok_or("no home directory")?;
        let log = home
            .join("Library/Logs")
            .join(crate::svc::APP_IDENTIFIER)
            .join("svc.log");
        crate::svc_units::macos_plan(&home, crate::svc::euid(), &exe, &log)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        let cfg = dirs::config_dir().ok_or("no config directory")?;
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .map_err(|_| "cannot tell the user name for lingering".to_string())?;
        crate::svc_units::linux_plan(&cfg, &user, &exe)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (app, exe);
        Err("the background service is not available on this platform yet".to_string())
    }
}

fn supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

fn view<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    runs_logged_out: Option<bool>,
    notes: Vec<String>,
) -> Result<SvcView, String> {
    let root = data_root(app)?;
    let paths = crate::svc::SvcPaths::under(&root);
    let settings = load(&paths.dir);
    crate::svc::invalidate_link(app);
    let link = tauri::Manager::try_state::<crate::svc::SvcLinkState>(app)
        .map(|s| s.link())
        .unwrap_or_else(|| crate::svc::Link::InProcess("no link state".to_string()));
    let (attached, reason, service) = match link {
        crate::svc::Link::Attached(c) => (true, None, c.status().ok()),
        crate::svc::Link::InProcess(r) => (false, Some(r), None),
    };
    Ok(SvcView {
        supported: supported(),
        enabled: settings.enabled,
        unit_installed: plan(app)
            .map(|p| crate::svc_units::installed(&p))
            .unwrap_or(false),
        attached,
        in_process_reason: reason,
        service,
        runs_logged_out,
        notes,
        opt_out_copy: OPT_OUT_COPY,
        power: settings.power,
    })
}

/// **svc_status** — the service's state for Settings › Node and the onboarding step. Data
/// sources: `svc/settings.json`, the unit file's presence, and the service's own `status` over
/// its socket.
#[tauri::command]
pub async fn svc_status(app_h: tauri::AppHandle) -> Result<SvcView, String> {
    crate::blocking::off_main(move || view(&app_h, None, Vec::new())).await
}

/// Wait up to `within` for the service to answer.
fn wait_attached<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    within: std::time::Duration,
) -> Option<crate::svc::SvcClient> {
    let t0 = std::time::Instant::now();
    loop {
        crate::svc::invalidate_link(app);
        if let Some(c) = crate::svc::attached_client(app) {
            return Some(c);
        }
        if t0.elapsed() >= within {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// **svc_set_enabled** — the member's choice (onboarding or Settings › Node).
/// - on: write and load the unit, wait for the service, stop the app's in-process node and kubo,
///   and let the service start them (the coinbase goes with the next `node_start`);
/// - off: drain the service and tell it to exit, remove the unit. The webview then starts the
///   node in-process again (`node_start`).
#[tauri::command]
pub async fn svc_set_enabled(app_h: tauri::AppHandle, enabled: bool) -> Result<SvcView, String> {
    crate::blocking::off_main(move || {
        if !supported() {
            return Err("the background service is not available on this platform yet".to_string());
        }
        let root = data_root(&app_h)?;
        let paths = crate::svc::SvcPaths::under(&root);
        let mut settings = load(&paths.dir);
        settings.enabled = Some(enabled);
        save(&paths.dir, &settings)?;
        let plan = plan(&app_h)?;
        let runner = crate::svc_units::SystemRunner;
        if enabled {
            let report = crate::svc_units::install(&plan, &runner)?;
            let mut notes = report.notes.clone();
            match wait_attached(&app_h, std::time::Duration::from_secs(15)) {
                Some(client) => {
                    // Hand over: the in-process node and kubo stop, the service starts its own.
                    if let Some(n) = tauri::Manager::try_state::<crate::node::NodeState>(&app_h) {
                        n.0.stop();
                    }
                    if let Some(i) = tauri::Manager::try_state::<crate::ipfs::IpfsState>(&app_h) {
                        i.0.stop();
                    }
                    let _ = client.call(crate::svc::SvcOp::IpfsStart);
                    if let Err(e) = client.call(crate::svc::SvcOp::NodeStart) {
                        notes.push(format!("The service will start the node when it can: {e}"));
                    }
                }
                None => notes.push(
                    "The service was installed but did not answer yet; the node keeps running in the app until it does."
                        .to_string(),
                ),
            }
            view(&app_h, Some(report.runs_logged_out), notes)
        } else {
            if let Some(client) = crate::svc::attached_client(&app_h) {
                // Drain and exit 0: the unit manager does not restart a requested stop.
                let _ = client.call(crate::svc::SvcOp::Shutdown);
            }
            crate::svc_units::uninstall(&plan, &runner)?;
            crate::svc::invalidate_link(&app_h);
            view(&app_h, None, Vec::new())
        }
    })
    .await
}

/// **svc_power_set** — save Settings › Node › Power.
#[tauri::command]
pub async fn svc_power_set(
    app_h: tauri::AppHandle,
    policy: PowerPolicy,
) -> Result<SvcView, String> {
    crate::blocking::off_main(move || {
        check_policy(&policy)?;
        let paths = crate::svc::SvcPaths::under(data_root(&app_h)?);
        let mut settings = load(&paths.dir);
        settings.power = policy;
        save(&paths.dir, &settings)?;
        view(&app_h, None, Vec::new())
    })
    .await
}

/// What Settings › Node › Power shows: the power state read now and the training decision.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerView {
    pub state: PowerState,
    pub training: TrainingVerdict,
    /// Where the state was read from.
    pub source: &'static str,
}

/// **svc_power_status** — data sources: `pmset -g batt` (macOS) or `/sys/class/power_supply`
/// (Linux), and the saved policy.
#[tauri::command]
pub async fn svc_power_status(app_h: tauri::AppHandle) -> Result<PowerView, String> {
    crate::blocking::off_main(move || {
        let paths = crate::svc::SvcPaths::under(data_root(&app_h)?);
        let policy = load(&paths.dir).power;
        let state = read_power();
        Ok(PowerView {
            training: training_allowed(&policy, &state, local_hour_now()),
            state,
            source: if cfg!(target_os = "macos") {
                "pmset -g batt"
            } else {
                "/sys/class/power_supply"
            },
        })
    })
    .await
}

#[cfg(test)]
#[path = "svc_settings_tests.rs"]
mod tests;
