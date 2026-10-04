//! HUP-S8.2 + S8.3 (US-8.1) — the fleet wizard backend: "Connect my machines".
//!
//! The wizard (src/fleet/) walks the member through:
//! 1. **This machine.** The local tier probe (`tier.rs`, the S1.6 subset of a sizeup receipt)
//!    and the role that tier suggests ([`role_for`]).
//! 2. **Discovery (opt-in).** mDNS lists other machines running Citrate Core
//!    (`fleet_mdns.rs`). Off by default, off again at the next start.
//! 3. **Pairing by link or QR.** A short-lived, single-use, signed link (`fleet_pairing.rs`).
//!    While a link is open this app listens on an ephemeral TCP port; the other machine opens the
//!    link, verifies it, connects to one of its address hints and presents it. The issuer redeems
//!    it (once), and both sides record each other in their local fleet roster with tier + role.
//! 4. **Connectivity (S8.3).** When the other machine cannot be reached, read-only Tailscale
//!    detection (`fleet_tailscale.rs`) drives plain-language guidance; tailnet addresses are
//!    added to the link's hints.
//!
//! **What this is not (yet).** The roster here is a LOCAL list of paired machines. The
//! wallet-signed `DeviceLink` that binds a device to the member on the cluster roster (D-31,
//! S8.1) is issued through the SignatureCeremony and is not part of this WP; the wizard says so.
//! No key here is a wallet key and nothing here signs a transaction (Rule 3). The pairing key is
//! an in-memory ed25519 key used only to sign pairing links.
//!
//! Pending owner sign-off (defaults, marked where they live): the role names per tier, the
//! pairing link lifetime, and discovery consent not being remembered across restarts.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::fleet_mdns::{Advert, Discovery, Seen};
use crate::fleet_pairing::{
    confirmation_code, qr_matrix, reply_bytes, verify_link, verify_reply, PairClaim, PairError,
    PairIssuer, QrMatrix, MAX_LABEL_LEN,
};
use crate::fleet_tailscale::{GuidanceStep, Reach, TailscaleReport};

/// The local fleet roster, in the app data dir.
pub const ROSTER_FILE: &str = "fleet.json";
/// Most devices the local roster keeps.
pub const MAX_DEVICES: usize = 64;
/// Connect deadline per address hint when joining.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(2500);
/// Read/write deadline on a pairing connection.
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest request/reply line on the pairing port.
const MAX_LINE: u64 = 4096;
/// How long a browse waits for answers.
const BROWSE_WAIT: Duration = Duration::from_millis(1500);

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The role a tier suggests. DEFAULT PENDING OWNER SIGN-OFF (names and mapping):
/// T0 = light (chat, small jobs), T1 = worker, T2 = heavy (serves the larger models).
pub fn role_for(tier: Option<&str>) -> &'static str {
    match tier {
        Some("T0") => "light",
        Some("T1") => "worker",
        Some("T2") => "heavy",
        _ => "unknown",
    }
}

fn tier_ok(t: &Option<String>) -> bool {
    t.as_deref().is_none_or(|t| matches!(t, "T0" | "T1" | "T2"))
}

fn label_ok(l: &str) -> bool {
    !l.trim().is_empty() && l.chars().count() <= MAX_LABEL_LEN && !l.chars().any(char::is_control)
}

fn id_ok(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---- roster ---------------------------------------------------------------------------------

/// How a roster entry came to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PairVia {
    /// This machine issued the link; the other machine joined.
    Issued,
    /// This machine opened another machine's link.
    Joined,
}

/// A paired machine, as recorded locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetDevice {
    pub id: String,
    pub label: String,
    pub tier: Option<String>,
    pub role: String,
    /// The IP it was reached at / connected from.
    pub addr: Option<String>,
    pub paired_at: u64,
    pub via: PairVia,
    /// The six-digit code both screens showed for this pairing (absent for older entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `fleet.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterFile {
    pub v: u8,
    /// This machine's fleet id (random, not a key, not derived from the wallet).
    pub device_id: String,
    /// The label the member gave this machine.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub devices: Vec<FleetDevice>,
}

impl RosterFile {
    pub fn fresh() -> Self {
        let mut r = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut r);
        Self {
            v: 1,
            device_id: hex::encode(r),
            label: None,
            devices: Vec::new(),
        }
    }
}

/// Load the roster; a missing file is a fresh roster (not written until saved). A file that
/// cannot be parsed is an error and is left untouched.
pub fn load_roster(path: &Path) -> Result<RosterFile, String> {
    match std::fs::read(path) {
        Ok(b) => {
            let r: RosterFile = serde_json::from_slice(&b)
                .map_err(|_| "The fleet roster file could not be read.".to_string())?;
            if r.v != 1 || !id_ok(&r.device_id) {
                return Err("The fleet roster file could not be read.".into());
            }
            Ok(r)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RosterFile::fresh()),
        Err(e) => Err(format!("fleet roster: {e}")),
    }
}

/// Save atomically (temp file + rename).
pub fn save_roster(path: &Path, r: &RosterFile) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("fleet roster: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("fleet roster: {e}"))
}

/// Insert or replace by id; keeps the newest [`MAX_DEVICES`].
pub fn upsert(r: &mut RosterFile, d: FleetDevice) {
    r.devices.retain(|x| x.id != d.id);
    r.devices.push(d);
    if r.devices.len() > MAX_DEVICES {
        r.devices.sort_by_key(|x| std::cmp::Reverse(x.paired_at));
        r.devices.truncate(MAX_DEVICES);
    }
}

// ---- the pairing wire protocol (one JSON line each way) -------------------------------------

/// This machine as it presents itself while pairing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSelf {
    pub device_id: String,
    pub label: String,
    pub tier: Option<String>,
}

/// joiner → issuer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JoinRequest {
    pub v: u8,
    pub link: String,
    pub device_id: String,
    pub label: String,
    pub tier: Option<String>,
}

impl JoinRequest {
    pub fn valid(&self) -> bool {
        self.v == 1
            && self.link.len() <= crate::fleet_pairing::MAX_LINK_LEN
            && id_ok(&self.device_id)
            && label_ok(&self.label)
            && tier_ok(&self.tier)
    }
}

/// issuer → joiner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinReply {
    pub ok: bool,
    pub error: Option<PairError>,
    pub device_id: Option<String>,
    pub label: Option<String>,
    pub tier: Option<String>,
    /// The issuer's pairing-key signature over [`reply_bytes`]: the joiner checks it against the
    /// key in the link, so only the machine that minted the link can complete the pairing.
    #[serde(default)]
    pub proof: Option<String>,
}

impl JoinReply {
    fn refuse(e: PairError) -> Self {
        Self {
            ok: false,
            error: Some(e),
            device_id: None,
            label: None,
            tier: None,
            proof: None,
        }
    }
}

/// Addresses a pairing link may point the joiner at: private (RFC 1918), CGNAT / tailnet
/// (100.64.0.0/10), link-local, and IPv6 unique-local or link-local. Never this machine's loopback,
/// a public address, or the cloud metadata address, so a pasted link cannot make Core probe them.
pub fn hint_ip_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            if v == std::net::Ipv4Addr::new(169, 254, 169, 254) {
                return false;
            }
            v.is_private() || v.is_link_local() || (o[0] == 100 && (o[1] & 0xc0) == 64)
        }
        IpAddr::V6(v) => {
            let seg = v.segments()[0];
            (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80
        }
    }
}

/// Peers the pairing port answers: this machine and the local networks [`hint_ip_allowed`] covers.
/// The listener binds every interface (LAN and tailnet), so anything else is dropped unanswered.
pub fn peer_allowed(ip: IpAddr) -> bool {
    ip.is_loopback() || hint_ip_allowed(ip)
}

/// Shared state of a running pairing server.
pub struct PairCtx {
    pub issuer: Arc<Mutex<PairIssuer>>,
    pub roster_path: PathBuf,
    pub me: Mutex<DeviceSelf>,
}

fn handle_conn(stream: TcpStream, ctx: &PairCtx) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let peer_ip = stream.peer_addr().ok().map(|a| a.ip().to_string());
    let mut line = String::new();
    BufReader::new(stream.try_clone()?.take(MAX_LINE)).read_line(&mut line)?;
    let reply = answer(line.trim(), peer_ip, ctx);
    let mut out = serde_json::to_string(&reply).map_err(std::io::Error::other)?;
    out.push('\n');
    (&stream).write_all(out.as_bytes())
}

fn answer(line: &str, peer_ip: Option<String>, ctx: &PairCtx) -> JoinReply {
    let Ok(req) = serde_json::from_str::<JoinRequest>(line) else {
        return JoinReply::refuse(PairError::Malformed);
    };
    if !req.valid() {
        return JoinReply::refuse(PairError::Malformed);
    }
    let Ok(me) = ctx.me.lock().map(|g| g.clone()) else {
        return JoinReply::refuse(PairError::Malformed);
    };
    if req.device_id == me.device_id {
        return JoinReply::refuse(PairError::SameDevice);
    }
    // Load before consuming the token, so a broken roster file never burns a link.
    let Ok(mut roster) = load_roster(&ctx.roster_path) else {
        return JoinReply::refuse(PairError::Malformed);
    };
    let redeemed = match ctx.issuer.lock() {
        Ok(mut iss) => iss.redeem(&req.link, now_secs()).map(|claim| {
            let proof = iss.sign_reply(&reply_bytes(
                &claim.nonce,
                &req.device_id,
                &me.device_id,
                &me.label,
                me.tier.as_deref(),
            ));
            (claim, proof)
        }),
        Err(_) => Err(PairError::Malformed),
    };
    let (claim, proof) = match redeemed {
        Ok(x) => x,
        Err(e) => return JoinReply::refuse(e),
    };
    upsert(
        &mut roster,
        FleetDevice {
            id: req.device_id.clone(),
            label: req.label.trim().to_string(),
            tier: req.tier.clone(),
            role: role_for(req.tier.as_deref()).to_string(),
            addr: peer_ip,
            paired_at: now_secs(),
            via: PairVia::Issued,
            code: Some(confirmation_code(
                &claim.nonce,
                &claim.issuer_pub,
                &req.device_id,
            )),
        },
    );
    // The roster file lives beside the issuer's; keep its own id/label.
    roster.device_id = me.device_id.clone();
    let _ = save_roster(&ctx.roster_path, &roster);
    JoinReply {
        ok: true,
        error: None,
        device_id: Some(me.device_id),
        label: Some(me.label),
        tier: me.tier,
        proof: Some(proof),
    }
}

/// Accept pairing connections until `stop(now)` says so. One connection at a time.
pub fn serve_pairing<F: Fn(u64) -> bool>(listener: TcpListener, ctx: Arc<PairCtx>, stop: F) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    loop {
        if stop(now_secs()) {
            return;
        }
        match listener.accept() {
            Ok((s, peer)) => {
                if peer_allowed(peer.ip()) {
                    let _ = handle_conn(s, &ctx);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(150)),
        }
    }
}

/// Why joining failed. Shown in the wizard; `Unreachable` triggers the connectivity guidance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinError {
    /// The link itself is bad (offline check).
    Link(PairError),
    /// The issuer answered and refused.
    Refused(PairError),
    /// No address hint answered (the `ip:port`s tried).
    Unreachable(Vec<String>),
    /// Something answered but did not speak the pairing protocol.
    Protocol,
}

/// A successful join: the issuer as recorded in the joiner's roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinOutcome {
    pub device: FleetDevice,
    /// The six-digit code this screen shows; the other machine shows the same one.
    pub code: String,
}

fn exchange(addr: SocketAddr, req: &JoinRequest) -> Result<JoinReply, JoinError> {
    let s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|_| JoinError::Unreachable(vec![addr.to_string()]))?;
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));
    let mut out = serde_json::to_string(req).map_err(|_| JoinError::Protocol)?;
    out.push('\n');
    (&s).write_all(out.as_bytes())
        .map_err(|_| JoinError::Protocol)?;
    let mut line = String::new();
    BufReader::new((&s).take(MAX_LINE))
        .read_line(&mut line)
        .map_err(|_| JoinError::Protocol)?;
    serde_json::from_str(line.trim()).map_err(|_| JoinError::Protocol)
}

/// **Join** another machine's link: verify it offline, then present it at each hint in turn.
/// Only local-network hints are tried ([`hint_ip_allowed`]). Blocking (network): call off the
/// main thread.
pub fn join_link(link: &str, me: &DeviceSelf, now: u64) -> Result<JoinOutcome, JoinError> {
    join_link_with(link, me, now, &hint_ip_allowed)
}

/// [`join_link`] with the address rule given (tests run both ends on loopback).
pub fn join_link_with(
    link: &str,
    me: &DeviceSelf,
    now: u64,
    allowed: &dyn Fn(IpAddr) -> bool,
) -> Result<JoinOutcome, JoinError> {
    let claim = verify_link(link, now).map_err(JoinError::Link)?;
    let req = JoinRequest {
        v: 1,
        link: link.to_string(),
        device_id: me.device_id.clone(),
        label: me.label.clone(),
        tier: me.tier.clone(),
    };
    let mut tried = Vec::new();
    for hint in &claim.hints {
        let Ok(addr) = hint.parse::<SocketAddr>() else {
            continue;
        };
        if !allowed(addr.ip()) {
            continue;
        }
        match exchange(addr, &req) {
            Err(JoinError::Unreachable(_)) => tried.push(hint.clone()),
            Err(e) => return Err(e),
            Ok(reply) if reply.ok => {
                let (Some(id), Some(label)) = (reply.device_id, reply.label) else {
                    return Err(JoinError::Protocol);
                };
                if !id_ok(&id) || !label_ok(&label) || !tier_ok(&reply.tier) {
                    return Err(JoinError::Protocol);
                }
                // Only the holder of the link's pairing key can answer for it.
                let signed = reply_bytes(
                    &claim.nonce,
                    &me.device_id,
                    &id,
                    &label,
                    reply.tier.as_deref(),
                );
                let proven = reply
                    .proof
                    .as_deref()
                    .is_some_and(|p| verify_reply(&claim.issuer_pub, p, &signed));
                if !proven {
                    return Err(JoinError::Protocol);
                }
                let code = confirmation_code(&claim.nonce, &claim.issuer_pub, &me.device_id);
                return Ok(JoinOutcome {
                    code: code.clone(),
                    device: FleetDevice {
                        role: role_for(reply.tier.as_deref()).to_string(),
                        id,
                        label,
                        tier: reply.tier,
                        addr: Some(addr.ip().to_string()),
                        paired_at: now,
                        via: PairVia::Joined,
                        code: Some(code),
                    },
                });
            }
            Ok(reply) => {
                return Err(JoinError::Refused(
                    reply.error.unwrap_or(PairError::Malformed),
                ))
            }
        }
    }
    Err(JoinError::Unreachable(tried))
}

/// `ip:port` hints: this machine's primary LAN address, then tailnet IPv4s. Deduped, capped.
pub fn build_hints(lan: Option<IpAddr>, tailnet: &[String], port: u16) -> Vec<String> {
    let mut ips: Vec<IpAddr> = lan.into_iter().collect();
    ips.extend(tailnet.iter().filter_map(|s| s.parse::<IpAddr>().ok()));
    let mut out: Vec<String> = Vec::new();
    for ip in ips {
        let h = SocketAddr::new(ip, port).to_string();
        if !out.contains(&h) && out.len() < crate::fleet_pairing::MAX_HINTS {
            out.push(h);
        }
    }
    out
}

/// The primary LAN address: the local end of a UDP "connect" (sends no packet).
fn primary_lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

// ---- runtime state + commands ---------------------------------------------------------------

struct Runtime {
    issuer: Arc<Mutex<PairIssuer>>,
    /// Random per-run mDNS instance id (not the roster id, so adverts are not linkable).
    instance_id: String,
    discovery: Option<Discovery>,
    /// The open pairing server: its port and its stop flag.
    server: Option<(u16, Arc<AtomicBool>, Arc<PairCtx>)>,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RT: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RT.get_or_init(|| {
        let mut r = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut r);
        Mutex::new(Runtime {
            issuer: Arc::new(Mutex::new(PairIssuer::new_random())),
            instance_id: hex::encode(r),
            discovery: None,
            server: None,
        })
    })
}

fn lock_rt() -> Result<std::sync::MutexGuard<'static, Runtime>, String> {
    runtime()
        .lock()
        .map_err(|_| "fleet state unavailable".to_string())
}

fn default_label() -> String {
    match std::env::consts::OS {
        "macos" => "This Mac".into(),
        "windows" => "This Windows PC".into(),
        "linux" => "This Linux machine".into(),
        _ => "This machine".into(),
    }
}

/// This machine in the wizard.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetSelf {
    pub device_id: String,
    pub label: String,
    pub tier: Option<String>,
    pub role: String,
}

/// What `fleet_probe` returns: this machine + its full tier report.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetProbe {
    pub device: FleetSelf,
    pub tier: crate::tier::TierReport,
}

fn roster_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(ROSTER_FILE))
}

fn self_view(app: &tauri::AppHandle) -> Result<(FleetSelf, crate::tier::TierReport), String> {
    let roster = load_roster(&roster_path(app)?)?;
    let report = crate::tier::tier_recommend_sync(app.clone())?;
    let tier = Some(report.effective.id().to_string());
    Ok((
        FleetSelf {
            device_id: roster.device_id.clone(),
            label: roster.label.clone().unwrap_or_else(default_label),
            role: role_for(tier.as_deref()).to_string(),
            tier,
        },
        report,
    ))
}

fn ensure_roster_saved(path: &Path) -> Result<RosterFile, String> {
    let r = load_roster(path)?;
    if !path.exists() {
        save_roster(path, &r)?;
    }
    Ok(r)
}

fn advert_for(rt: &Runtime, me: &FleetSelf) -> Advert {
    Advert {
        id: rt.instance_id.clone(),
        label: me.label.clone(),
        tier: me.tier.clone(),
        role: me.role.clone(),
        port: rt.server.as_ref().map(|s| s.0).unwrap_or(0),
    }
}

/// **Command — fleet_probe.** This machine: fleet id, label, tier report, role. Local only.
#[tauri::command]
pub async fn fleet_probe(app_h: tauri::AppHandle) -> Result<FleetProbe, String> {
    crate::blocking::off_main(move || fleet_probe_sync(app_h.clone())).await
}

pub fn fleet_probe_sync(app: tauri::AppHandle) -> Result<FleetProbe, String> {
    ensure_roster_saved(&roster_path(&app)?)?;
    let (device, tier) = self_view(&app)?;
    Ok(FleetProbe { device, tier })
}

/// **Command — fleet_set_label.** Name this machine (shown to the member's other machines).
#[tauri::command]
pub async fn fleet_set_label(app_h: tauri::AppHandle, label: String) -> Result<FleetSelf, String> {
    crate::blocking::off_main(move || fleet_set_label_sync(app_h.clone(), label)).await
}

pub fn fleet_set_label_sync(app: tauri::AppHandle, label: String) -> Result<FleetSelf, String> {
    if !label_ok(&label) {
        return Err(format!(
            "A name of 1 to {MAX_LABEL_LEN} characters, please."
        ));
    }
    let path = roster_path(&app)?;
    let mut r = load_roster(&path)?;
    r.label = Some(label.trim().to_string());
    save_roster(&path, &r)?;
    let (me, _) = self_view(&app)?;
    if let Ok(rt) = lock_rt() {
        if let Some(d) = &rt.discovery {
            d.set_advert(advert_for(&rt, &me));
        }
        if let Some((_, _, ctx)) = &rt.server {
            if let Ok(mut g) = ctx.me.lock() {
                g.label = me.label.clone();
            }
        }
    }
    Ok(me)
}

/// **Command — fleet_roster.** The machines paired with this one (local record).
#[tauri::command]
pub async fn fleet_roster(app_h: tauri::AppHandle) -> Result<Vec<FleetDevice>, String> {
    crate::blocking::off_main(move || fleet_roster_sync(app_h.clone())).await
}

pub fn fleet_roster_sync(app: tauri::AppHandle) -> Result<Vec<FleetDevice>, String> {
    let mut d = load_roster(&roster_path(&app)?)?.devices;
    d.sort_by_key(|x| std::cmp::Reverse(x.paired_at));
    Ok(d)
}

/// Discovery on/off state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryState {
    pub enabled: bool,
}

/// **Command — fleet_discovery_set.** Turn opt-in mDNS discovery on or off.
#[tauri::command]
pub async fn fleet_discovery_set(
    app_h: tauri::AppHandle,
    enabled: bool,
) -> Result<DiscoveryState, String> {
    crate::blocking::off_main(move || fleet_discovery_set_sync(app_h.clone(), enabled)).await
}

pub fn fleet_discovery_set_sync(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<DiscoveryState, String> {
    if !enabled {
        let mut rt = lock_rt()?;
        if let Some(d) = rt.discovery.take() {
            d.stop();
        }
        return Ok(DiscoveryState { enabled: false });
    }
    let (me, _) = self_view(&app)?;
    let mut rt = lock_rt()?;
    if rt.discovery.is_none() {
        let advert = advert_for(&rt, &me);
        rt.discovery = Some(Discovery::start(advert)?);
    }
    Ok(DiscoveryState { enabled: true })
}

/// **Command — fleet_discovery_browse.** Ask the local network once and list what answered.
/// Errors when discovery is off (the member has not opted in).
#[tauri::command]
pub async fn fleet_discovery_browse() -> Result<Vec<Seen>, String> {
    crate::blocking::off_main(fleet_discovery_browse_sync).await
}

pub fn fleet_discovery_browse_sync() -> Result<Vec<Seen>, String> {
    {
        let rt = lock_rt()?;
        let d = rt
            .discovery
            .as_ref()
            .ok_or_else(|| "Discovery is off. Turn it on to look for your machines.".to_string())?;
        d.query()?;
    }
    std::thread::sleep(BROWSE_WAIT);
    let rt = lock_rt()?;
    Ok(rt.discovery.as_ref().map(|d| d.seen()).unwrap_or_default())
}

/// What `fleet_pair_create` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairOffer {
    pub link: String,
    pub qr: QrMatrix,
    pub expires_at: u64,
    pub hints: Vec<String>,
}

/// **Command — fleet_pair_create.** Open a pairing link (and QR). Starts the pairing listener
/// for as long as any link is open; it closes when the last link is used or expires.
#[tauri::command]
pub async fn fleet_pair_create(app_h: tauri::AppHandle) -> Result<PairOffer, String> {
    crate::blocking::off_main(move || fleet_pair_create_sync(app_h.clone())).await
}

pub fn fleet_pair_create_sync(app: tauri::AppHandle) -> Result<PairOffer, String> {
    let path = roster_path(&app)?;
    ensure_roster_saved(&path)?;
    let (me, _) = self_view(&app)?;
    let tailnet =
        crate::fleet_tailscale::tailnet_ipv4s(&crate::fleet_tailscale::detect_tailscale());
    let mut rt = lock_rt()?;
    let port = match &rt.server {
        Some((port, running, _)) if running.load(Ordering::SeqCst) => *port,
        _ => {
            let listener =
                TcpListener::bind("0.0.0.0:0").map_err(|e| format!("pairing listener: {e}"))?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            let running = Arc::new(AtomicBool::new(true));
            let ctx = Arc::new(PairCtx {
                issuer: rt.issuer.clone(),
                roster_path: path.clone(),
                me: Mutex::new(DeviceSelf {
                    device_id: me.device_id.clone(),
                    label: me.label.clone(),
                    tier: me.tier.clone(),
                }),
            });
            let (iss, flag, c) = (rt.issuer.clone(), running.clone(), ctx.clone());
            std::thread::Builder::new()
                .name("fleet-pair".into())
                .spawn(move || {
                    // Grace so the first link can be issued before the "none open" check.
                    std::thread::sleep(Duration::from_millis(300));
                    serve_pairing(listener, c, |now| {
                        iss.lock().map(|i| i.outstanding(now) == 0).unwrap_or(true)
                    });
                    flag.store(false, Ordering::SeqCst);
                })
                .map_err(|e| format!("pairing listener: {e}"))?;
            rt.server = Some((port, running, ctx));
            port
        }
    };
    let hints = build_hints(primary_lan_ip(), &tailnet, port);
    if hints.is_empty() {
        return Err(
            "This machine has no network address to pair on. Connect to a network \
                    (or Tailscale) and try again."
                .into(),
        );
    }
    let (claim, link) = rt
        .issuer
        .lock()
        .map_err(|_| "pairing state unavailable".to_string())?
        .issue(&me.label, me.tier.as_deref(), hints.clone(), now_secs())
        .map_err(|e| e.message().to_string())?;
    if let Some(d) = &rt.discovery {
        d.set_advert(advert_for(&rt, &me));
    }
    Ok(PairOffer {
        qr: qr_matrix(&link)?,
        link,
        expires_at: claim.expires_at,
        hints,
    })
}

/// **Command — fleet_pair_inspect.** Check a pasted/scanned link offline (signature, expiry).
#[tauri::command]
pub async fn fleet_pair_inspect(link: String) -> Result<PairClaim, String> {
    crate::blocking::off_main(move || {
        verify_link(link.trim(), now_secs()).map_err(|e| e.message().to_string())
    })
    .await
}

/// What `fleet_pair_join` returns: success with the paired machine, or a typed failure the
/// wizard can act on (`unreachable` opens the connectivity step).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinResult {
    pub ok: bool,
    pub device: Option<FleetDevice>,
    /// `link` | `refused` | `unreachable` | `protocol`.
    pub error_kind: Option<String>,
    pub message: Option<String>,
    pub tried: Vec<String>,
}

pub fn join_result(r: Result<JoinOutcome, JoinError>) -> JoinResult {
    let fail = |kind: &str, msg: String, tried: Vec<String>| JoinResult {
        ok: false,
        device: None,
        error_kind: Some(kind.to_string()),
        message: Some(msg),
        tried,
    };
    match r {
        Ok(o) => JoinResult {
            ok: true,
            device: Some(o.device),
            error_kind: None,
            message: None,
            tried: Vec::new(),
        },
        Err(JoinError::Link(e)) => fail("link", e.message().into(), Vec::new()),
        Err(JoinError::Refused(e)) => fail("refused", e.message().into(), Vec::new()),
        Err(JoinError::Unreachable(t)) => fail(
            "unreachable",
            "The other machine could not be reached at any of its addresses.".into(),
            t,
        ),
        Err(JoinError::Protocol) => fail(
            "protocol",
            "Something answered, but it is not a Citrate Core pairing listener.".into(),
            Vec::new(),
        ),
    }
}

/// **Command — fleet_pair_join.** Pair with another machine using its link. Records it locally.
#[tauri::command]
pub async fn fleet_pair_join(app_h: tauri::AppHandle, link: String) -> Result<JoinResult, String> {
    crate::blocking::off_main(move || fleet_pair_join_sync(app_h.clone(), link)).await
}

pub fn fleet_pair_join_sync(app: tauri::AppHandle, link: String) -> Result<JoinResult, String> {
    let path = roster_path(&app)?;
    let mut roster = ensure_roster_saved(&path)?;
    let (me, _) = self_view(&app)?;
    // A link this app minted is refused before any connection.
    let own_pub = lock_rt()?
        .issuer
        .lock()
        .map(|i| i.public_hex())
        .map_err(|_| "pairing state unavailable".to_string())?;
    if verify_link(link.trim(), now_secs()).is_ok_and(|c| c.issuer_pub == own_pub) {
        return Ok(join_result(Err(JoinError::Refused(PairError::SameDevice))));
    }
    let res = join_link(
        link.trim(),
        &DeviceSelf {
            device_id: me.device_id,
            label: me.label,
            tier: me.tier,
        },
        now_secs(),
    );
    if let Ok(o) = &res {
        upsert(&mut roster, o.device.clone());
        save_roster(&path, &roster)?;
    }
    Ok(join_result(res))
}

/// What `fleet_tailscale` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleView {
    pub report: TailscaleReport,
    pub guidance: Vec<GuidanceStep>,
}

/// **Command — fleet_tailscale.** Read-only Tailscale status + guidance for the wizard's
/// reachability (`lan_peers` found by discovery; `unreachable` after a failed join).
#[tauri::command]
pub async fn fleet_tailscale(lan_peers: u32, unreachable: bool) -> Result<TailscaleView, String> {
    crate::blocking::off_main(move || {
        let report = crate::fleet_tailscale::detect_tailscale();
        let guidance = crate::fleet_tailscale::guidance(
            &report,
            &Reach {
                lan_peers: lan_peers as usize,
                unreachable,
            },
        );
        Ok(TailscaleView { report, guidance })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("fleet_tests.rs");
}
