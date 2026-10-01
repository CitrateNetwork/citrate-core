//! HUP-S8.3 (US-8.1) — Tailscale-assisted connectivity: read-only detection + guidance.
//!
//! When the member's machines are not on one local network (or a firewall blocks the pairing
//! port), Tailscale is the suggested path: every machine on the member's tailnet gets a stable
//! `100.x` address that the pairing link carries as a bootstrap hint.
//!
//! **Read-only, always.** This module runs exactly one command, `tailscale status --json`
//! ([`STATUS_ARGS`]), with a short deadline, and parses the result. It never runs `up`, `down`,
//! `login`, `set` or anything that changes Tailscale; the guidance tells the MEMBER what to do in
//! Tailscale themselves. If the CLI cannot be found it says so (Rule 1), never guesses.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The only argv this module ever passes to the Tailscale CLI.
pub const STATUS_ARGS: &[&str] = &["status", "--json"];
/// Deadline for the status call.
const STATUS_TIMEOUT: Duration = Duration::from_secs(4);
/// Largest status output read (a large tailnet is a few hundred KiB).
const MAX_STATUS_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TsState {
    /// No Tailscale CLI/app found on this machine.
    NotInstalled,
    /// Installed, but the background service is not running.
    NotRunning,
    /// Running, but the member is not signed in (or the machine is not approved).
    NeedsLogin,
    /// Signed in, but Tailscale is turned off.
    Stopped,
    /// Starting up.
    Starting,
    /// Connected to the tailnet.
    Running,
    /// Output could not be understood.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TsPeer {
    pub host_name: String,
    pub os: String,
    pub ips: Vec<String>,
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleReport {
    pub state: TsState,
    pub version: Option<String>,
    pub self_host: Option<String>,
    pub self_ips: Vec<String>,
    pub peers: Vec<TsPeer>,
}

impl TailscaleReport {
    fn empty(state: TsState) -> Self {
        Self {
            state,
            version: None,
            self_host: None,
            self_ips: Vec::new(),
            peers: Vec::new(),
        }
    }

    pub fn not_installed() -> Self {
        Self::empty(TsState::NotInstalled)
    }
}

fn str_list(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse `tailscale status --json`. Never panics; anything unexpected is `Unknown`/skipped.
pub fn parse_status(json: &str) -> TailscaleReport {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return TailscaleReport::empty(TsState::Unknown);
    };
    let state = match v.get("BackendState").and_then(|s| s.as_str()) {
        Some("Running") => TsState::Running,
        Some("NeedsLogin") | Some("NeedsMachineAuth") => TsState::NeedsLogin,
        Some("Stopped") => TsState::Stopped,
        Some("Starting") => TsState::Starting,
        Some("NoState") => TsState::NotRunning,
        _ => TsState::Unknown,
    };
    let me = v.get("Self");
    let mut peers: Vec<TsPeer> = v
        .get("Peer")
        .and_then(|p| p.as_object())
        .map(|m| {
            m.values()
                .filter_map(|p| {
                    Some(TsPeer {
                        host_name: p.get("HostName")?.as_str()?.to_string(),
                        os: p
                            .get("OS")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        ips: str_list(p.get("TailscaleIPs")),
                        online: p.get("Online").and_then(|b| b.as_bool()).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    peers.sort_by(|a, b| b.online.cmp(&a.online).then(a.host_name.cmp(&b.host_name)));
    TailscaleReport {
        state,
        version: v
            .get("Version")
            .and_then(|s| s.as_str())
            .map(str::to_string),
        self_host: me
            .and_then(|m| m.get("HostName"))
            .and_then(|s| s.as_str())
            .map(str::to_string),
        self_ips: str_list(me.and_then(|m| m.get("TailscaleIPs"))),
        peers,
    }
}

/// Classify a failed status call from its stderr/stdout text.
pub fn classify_cli_failure(text: &str) -> TsState {
    let t = text.to_ascii_lowercase();
    if t.contains("failed to connect") || t.contains("not running") || t.contains("tailscaled") {
        TsState::NotRunning
    } else if t.contains("logged out") || t.contains("needslogin") || t.contains("log in") {
        TsState::NeedsLogin
    } else {
        TsState::Unknown
    }
}

/// This machine's tailnet IPv4 addresses (bootstrap hints for a pairing link).
pub fn tailnet_ipv4s(r: &TailscaleReport) -> Vec<String> {
    if r.state != TsState::Running {
        return Vec::new();
    }
    r.self_ips
        .iter()
        .filter(|ip| ip.parse::<std::net::Ipv4Addr>().is_ok())
        .cloned()
        .collect()
}

/// Where the CLI may live: each `PATH` entry, then the OS's usual install location.
pub fn candidate_paths(os: &str, path_env: Option<&str>) -> Vec<PathBuf> {
    let exe = if os == "windows" {
        "tailscale.exe"
    } else {
        "tailscale"
    };
    let sep = if os == "windows" { ';' } else { ':' };
    let mut v: Vec<PathBuf> = path_env
        .unwrap_or("")
        .split(sep)
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(exe))
        .collect();
    match os {
        "macos" => {
            v.push("/Applications/Tailscale.app/Contents/MacOS/Tailscale".into());
            v.push("/usr/local/bin/tailscale".into());
            v.push("/opt/homebrew/bin/tailscale".into());
        }
        "windows" => v.push("C:\\Program Files\\Tailscale\\tailscale.exe".into()),
        _ => {
            v.push("/usr/bin/tailscale".into());
            v.push("/usr/sbin/tailscale".into());
            v.push("/usr/local/bin/tailscale".into());
        }
    }
    v
}

/// Run `tailscale status --json` (read-only) and report. Blocking: call off the main thread.
pub fn detect_tailscale() -> TailscaleReport {
    let path_env = std::env::var("PATH").ok();
    let Some(cli) = candidate_paths(std::env::consts::OS, path_env.as_deref())
        .into_iter()
        .find(|p| p.is_file())
    else {
        return TailscaleReport::not_installed();
    };
    let Ok(mut child) = Command::new(&cli)
        .args(STATUS_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return TailscaleReport::empty(TsState::Unknown);
    };
    // Drain both pipes on their own threads so a large tailnet cannot fill the pipe and stall.
    fn drain<R: std::io::Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(r) = r {
                let _ = std::io::Read::read_to_end(
                    &mut std::io::Read::take(r, MAX_STATUS_BYTES as u64),
                    &mut buf,
                );
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    }
    let out_h = drain(child.stdout.take());
    let err_h = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() < STATUS_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = out_h.join().unwrap_or_default();
    let err = err_h.join().unwrap_or_default();
    match status {
        None => TailscaleReport::empty(TsState::Unknown),
        // `status --json` prints JSON even when logged out / stopped; prefer it when present.
        Some(_) if out.trim_start().starts_with('{') => parse_status(&out),
        Some(_) => TailscaleReport::empty(classify_cli_failure(&format!("{err}\n{out}"))),
    }
}

/// What the wizard knows about reachability right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// Devices discovered on the local network.
    pub lan_peers: usize,
    /// A pairing attempt could not reach the other machine at any of its addresses.
    pub unreachable: bool,
}

/// One step of guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidanceStep {
    pub id: String,
    pub text: String,
    pub url: Option<String>,
}

fn step(id: &str, text: String, url: Option<&str>) -> GuidanceStep {
    GuidanceStep {
        id: id.to_string(),
        text,
        url: url.map(str::to_string),
    }
}

/// Guidance for the member. Empty when nothing is wrong. Never a step that changes settings
/// on the member's behalf: each step is something the member does in Tailscale themselves.
pub fn guidance(r: &TailscaleReport, reach: &Reach) -> Vec<GuidanceStep> {
    // Something is reachable and nothing failed: no guidance needed. With no machine found on
    // the local network, or after a failed pairing attempt, guide.
    if !reach.unreachable && reach.lan_peers > 0 {
        return Vec::new();
    }
    let mut g = Vec::new();
    if r.state != TsState::Running {
        g.push(step(
            "same-network",
            "If both machines are on the same Wi-Fi or wired network, check that each one allows \
             Citrate Core through its firewall, then try the pairing link again."
                .to_string(),
            None,
        ));
    }
    match r.state {
        TsState::NotInstalled | TsState::Unknown => g.push(step(
            "install",
            "For machines on different networks, install Tailscale on each machine and sign in \
             with the same account. Citrate Core then uses the Tailscale addresses to pair."
                .to_string(),
            Some("https://tailscale.com/download"),
        )),
        TsState::NotRunning | TsState::Stopped | TsState::Starting => g.push(step(
            "start",
            "Tailscale is installed but not connected. Open Tailscale and turn it on, on both \
             machines, then create a new pairing link."
                .to_string(),
            None,
        )),
        TsState::NeedsLogin => g.push(step(
            "login",
            "Tailscale needs you to sign in. Open Tailscale and sign in with the same account on \
             both machines, then create a new pairing link."
                .to_string(),
            None,
        )),
        TsState::Running => {
            g.push(step(
                "same-tailnet",
                "Tailscale is connected here. Make sure the other machine is signed in to the \
                 same tailnet and shows as connected, then create a new pairing link: it will \
                 include this machine's Tailscale address."
                    .to_string(),
                None,
            ));
            let offline: Vec<&str> = r
                .peers
                .iter()
                .filter(|p| !p.online)
                .map(|p| p.host_name.as_str())
                .collect();
            if !offline.is_empty() {
                g.push(step(
                    "peer-offline",
                    format!(
                        "These machines on your tailnet are offline right now: {}. Wake them \
                         and open Tailscale on them.",
                        offline.join(", ")
                    ),
                    None,
                ));
            }
        }
    }
    g
}

#[cfg(test)]
mod tests {
    include!("fleet_tailscale_tests.rs");
}
