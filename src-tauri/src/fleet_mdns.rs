//! HUP-S8.2 (US-8.1) — opt-in mDNS discovery of other machines running Citrate Core.
//!
//! **Off by default.** Nothing here runs until the member turns discovery on in the fleet wizard,
//! and it is off again at the next app start (the consent is not persisted). While on, this app:
//! - answers multicast DNS questions for [`SERVICE`] with one advert, and
//! - asks the same question when the wizard browses, collecting the adverts that come back.
//!
//! An advert says only: a random per-run instance id, the device label the member typed, its tier
//! and role, and (while a pairing link is open) the pairing port. No wallet address, no comms
//! address, no host name. The sender's IP comes from the UDP packet itself.
//!
//! This is a deliberately small DNS-SD subset (PTR + TXT, RFC 6762/6763 wire format) written on
//! `std` + `socket2`, so no mDNS library enters the T1 tree. The codec is pure and fixture-tested;
//! the socket loop ([`Discovery`]) is exercised by an `#[ignore]`d loopback test.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The DNS-SD service type Citrate Core advertises.
pub const SERVICE: &str = "_citrate-core._tcp.local";
/// mDNS multicast group + port.
pub const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
pub const MDNS_PORT: u16 = 5353;
/// Record type PTR.
pub const TYPE_PTR: u16 = 12;
/// Record type TXT.
pub const TYPE_TXT: u16 = 16;
const CLASS_IN: u16 = 1;
/// TTL on our adverts (seconds).
const TTL: u32 = 120;
/// Upper bound on questions / records parsed from one packet.
const MAX_ITEMS: usize = 64;
/// Upper bound on name compression jumps.
const MAX_JUMPS: usize = 16;

/// One Citrate Core instance's advert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Advert {
    /// Random per-run instance id (hex). The DNS-SD instance label.
    pub id: String,
    /// The device label the member chose.
    pub label: String,
    /// Tier id, when known.
    pub tier: Option<String>,
    /// Role id (see `fleet::role_for`).
    pub role: String,
    /// Pairing port while a pairing link is open, else 0.
    pub port: u16,
}

impl Advert {
    fn valid(&self) -> bool {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 32
            && self.id.bytes().all(|b| b.is_ascii_alphanumeric());
        let label_ok = !self.label.trim().is_empty()
            && self.label.chars().count() <= 64
            && !self.label.chars().any(char::is_control);
        let tier_ok = self
            .tier
            .as_deref()
            .is_none_or(|t| matches!(t, "T0" | "T1" | "T2"));
        let role_ok = !self.role.is_empty()
            && self.role.len() <= 16
            && self
                .role
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-');
        id_ok && label_ok && tier_ok && role_ok
    }

    fn txt(&self) -> Vec<String> {
        let mut kv = vec![
            "v=1".to_string(),
            format!("label={}", self.label),
            format!("role={}", self.role),
            format!("port={}", self.port),
        ];
        if let Some(t) = &self.tier {
            kv.push(format!("tier={t}"));
        }
        kv
    }
}

/// Record data this codec understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RData {
    Ptr(String),
    Txt(Vec<String>),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub name: String,
    pub qtype: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub rtype: u16,
    pub data: RData,
}

/// A parsed mDNS message (questions + every record section, flattened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub response: bool,
    pub questions: Vec<Question>,
    pub records: Vec<Record>,
}

/// Why a packet was not parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdnsError {
    Truncated,
    BadName,
    TooMany,
    Invalid,
}

// ---- encoding -------------------------------------------------------------------------------

/// A dotted name as DNS labels (no compression).
pub fn encode_name(name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 2);
    for label in name.split('.').filter(|l| !l.is_empty()) {
        let b = label.as_bytes();
        out.push(b.len().min(63) as u8);
        out.extend_from_slice(&b[..b.len().min(63)]);
    }
    out.push(0);
    out
}

fn header(response: bool, qd: u16, an: u16) -> Vec<u8> {
    let flags: u16 = if response { 0x8400 } else { 0 };
    let mut h = Vec::with_capacity(12);
    h.extend(0u16.to_be_bytes());
    h.extend(flags.to_be_bytes());
    h.extend(qd.to_be_bytes());
    h.extend(an.to_be_bytes());
    h.extend(0u16.to_be_bytes());
    h.extend(0u16.to_be_bytes());
    h
}

/// A one-shot multicast question: PTR `_citrate-core._tcp.local`.
pub fn encode_query() -> Vec<u8> {
    let mut p = header(false, 1, 0);
    p.extend(encode_name(SERVICE));
    p.extend(TYPE_PTR.to_be_bytes());
    p.extend(CLASS_IN.to_be_bytes());
    p
}

fn record(p: &mut Vec<u8>, name: &str, rtype: u16, rdata: &[u8]) -> Result<(), MdnsError> {
    p.extend(encode_name(name));
    p.extend(rtype.to_be_bytes());
    p.extend(CLASS_IN.to_be_bytes());
    p.extend(TTL.to_be_bytes());
    let len = u16::try_from(rdata.len()).map_err(|_| MdnsError::Invalid)?;
    p.extend(len.to_be_bytes());
    p.extend_from_slice(rdata);
    Ok(())
}

/// The answer to a service question: PTR(service → instance) + TXT(instance).
pub fn encode_response(a: &Advert) -> Result<Vec<u8>, MdnsError> {
    if !a.valid() {
        return Err(MdnsError::Invalid);
    }
    let instance = format!("{}.{SERVICE}", a.id);
    let mut p = header(true, 0, 2);
    record(&mut p, SERVICE, TYPE_PTR, &encode_name(&instance))?;
    let mut txt = Vec::new();
    for s in a.txt() {
        let b = s.as_bytes();
        let n = u8::try_from(b.len()).map_err(|_| MdnsError::Invalid)?;
        txt.push(n);
        txt.extend_from_slice(b);
    }
    record(&mut p, &instance, TYPE_TXT, &txt)?;
    Ok(p)
}

// ---- parsing --------------------------------------------------------------------------------

fn u16_at(b: &[u8], at: usize) -> Result<u16, MdnsError> {
    let s = b.get(at..at + 2).ok_or(MdnsError::Truncated)?;
    Ok(u16::from_be_bytes([s[0], s[1]]))
}

/// Read a (possibly compressed) name at `at`; returns (name, offset after it in the stream).
fn read_name(b: &[u8], at: usize) -> Result<(String, usize), MdnsError> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = at;
    let mut end: Option<usize> = None;
    let mut jumps = 0;
    loop {
        let len = *b.get(pos).ok_or(MdnsError::Truncated)? as usize;
        if len & 0xC0 == 0xC0 {
            let lo = *b.get(pos + 1).ok_or(MdnsError::Truncated)? as usize;
            let target = ((len & 0x3F) << 8) | lo;
            end.get_or_insert(pos + 2);
            jumps += 1;
            if jumps > MAX_JUMPS || target >= b.len() {
                return Err(MdnsError::BadName);
            }
            pos = target;
            continue;
        }
        if len & 0xC0 != 0 {
            return Err(MdnsError::BadName);
        }
        if len == 0 {
            let after = end.unwrap_or(pos + 1);
            return Ok((labels.join("."), after));
        }
        let s = b.get(pos + 1..pos + 1 + len).ok_or(MdnsError::Truncated)?;
        labels.push(String::from_utf8_lossy(s).into_owned());
        if labels.len() > 32 {
            return Err(MdnsError::BadName);
        }
        pos += 1 + len;
    }
}

fn parse_txt(d: &[u8]) -> Result<Vec<String>, MdnsError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let n = d[i] as usize;
        let s = d.get(i + 1..i + 1 + n).ok_or(MdnsError::Truncated)?;
        out.push(String::from_utf8_lossy(s).into_owned());
        i += 1 + n;
    }
    Ok(out)
}

/// Parse one mDNS packet. Bounded: never trusts header counts for allocation.
pub fn parse(b: &[u8]) -> Result<Message, MdnsError> {
    if b.len() < 12 {
        return Err(MdnsError::Truncated);
    }
    let flags = u16_at(b, 2)?;
    let qd = u16_at(b, 4)? as usize;
    let rr = u16_at(b, 6)? as usize + u16_at(b, 8)? as usize + u16_at(b, 10)? as usize;
    if qd > MAX_ITEMS || rr > MAX_ITEMS {
        return Err(MdnsError::TooMany);
    }
    let mut pos = 12;
    let mut questions = Vec::new();
    for _ in 0..qd {
        let (name, after) = read_name(b, pos)?;
        let qtype = u16_at(b, after)?;
        u16_at(b, after + 2)?;
        questions.push(Question { name, qtype });
        pos = after + 4;
    }
    let mut records = Vec::new();
    for _ in 0..rr {
        let (name, after) = read_name(b, pos)?;
        let rtype = u16_at(b, after)?;
        let len = u16_at(b, after + 8)? as usize;
        let start = after + 10;
        let d = b.get(start..start + len).ok_or(MdnsError::Truncated)?;
        let data = match rtype {
            TYPE_PTR => RData::Ptr(read_name(b, start)?.0),
            TYPE_TXT => RData::Txt(parse_txt(d)?),
            _ => RData::Other,
        };
        records.push(Record { name, rtype, data });
        pos = start + len;
    }
    Ok(Message {
        response: flags & 0x8000 != 0,
        questions,
        records,
    })
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim_end_matches('.')
        .eq_ignore_ascii_case(b.trim_end_matches('.'))
}

/// Does this packet ask for our service?
pub fn is_service_query(m: &Message) -> bool {
    !m.response
        && m.questions
            .iter()
            .any(|q| same_name(&q.name, SERVICE) && (q.qtype == TYPE_PTR || q.qtype == 255))
}

fn advert_from_txt(id: &str, kv: &[String]) -> Option<Advert> {
    let get = |k: &str| {
        kv.iter()
            .find_map(|s| s.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
    };
    if get("v") != Some("1") {
        return None;
    }
    let a = Advert {
        id: id.to_string(),
        label: get("label")?.to_string(),
        tier: get("tier").map(str::to_string),
        role: get("role")?.to_string(),
        port: get("port")?.parse().ok()?,
    };
    a.valid().then_some(a)
}

/// Every valid Citrate Core advert in a response.
pub fn adverts(m: &Message) -> Vec<Advert> {
    if !m.response {
        return Vec::new();
    }
    let suffix = format!(".{SERVICE}");
    let mut out = Vec::new();
    for r in &m.records {
        let RData::Ptr(target) = &r.data else {
            continue;
        };
        if !same_name(&r.name, SERVICE) {
            continue;
        }
        let t = target.trim_end_matches('.');
        if t.len() <= suffix.len() || !t[t.len() - suffix.len()..].eq_ignore_ascii_case(&suffix) {
            continue;
        }
        let id = &t[..t.len() - suffix.len()];
        let txt = m.records.iter().find_map(|x| match &x.data {
            RData::Txt(kv) if same_name(&x.name, t) => Some(kv),
            _ => None,
        });
        if let Some(a) = txt.and_then(|kv| advert_from_txt(id, kv)) {
            if !out.contains(&a) {
                out.push(a);
            }
        }
    }
    out
}

// ---- the socket loop (opt-in) ---------------------------------------------------------------

/// A device seen on the local network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    pub advert: Advert,
    /// The sender's IP (from the packet).
    pub ip: String,
    /// Seconds since the advert was last heard.
    pub age_secs: u64,
}

/// How long a heard advert stays listed.
const SEEN_TTL: Duration = Duration::from_secs(TTL as u64);

/// The running discovery responder/browser. Dropping it (or [`Discovery::stop`]) ends the thread.
pub struct Discovery {
    stop: Arc<AtomicBool>,
    own: Arc<Mutex<Advert>>,
    seen: SeenMap,
    socket: UdpSocket,
}

fn open_socket() -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(all(unix, not(target_os = "solaris"), not(target_os = "illumos")))]
    s.set_reuse_port(true)?;
    s.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT)).into())?;
    s.join_multicast_v4(&MDNS_GROUP, &Ipv4Addr::UNSPECIFIED)?;
    s.set_multicast_loop_v4(true)?;
    s.set_multicast_ttl_v4(255)?;
    s.set_read_timeout(Some(Duration::from_millis(400)))?;
    Ok(s.into())
}

fn group_addr() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(MDNS_GROUP, MDNS_PORT))
}

impl Discovery {
    /// Join the mDNS group and start answering for `own`.
    pub fn start(own: Advert) -> Result<Self, String> {
        if !own.valid() {
            return Err("invalid advert".into());
        }
        let socket = open_socket().map_err(|e| format!("mDNS socket: {e}"))?;
        let rx = socket
            .try_clone()
            .map_err(|e| format!("mDNS socket: {e}"))?;
        let d = Discovery {
            stop: Arc::new(AtomicBool::new(false)),
            own: Arc::new(Mutex::new(own)),
            seen: Arc::new(Mutex::new(HashMap::new())),
            socket,
        };
        let (stop, own, seen) = (d.stop.clone(), d.own.clone(), d.seen.clone());
        std::thread::Builder::new()
            .name("fleet-mdns".into())
            .spawn(move || run_loop(rx, stop, own, seen))
            .map_err(|e| format!("mDNS thread: {e}"))?;
        Ok(d)
    }

    /// Replace the advert this device answers with (label/tier/role/pairing port changed).
    pub fn set_advert(&self, a: Advert) {
        if a.valid() {
            if let Ok(mut g) = self.own.lock() {
                *g = a;
            }
        }
    }

    /// Send one question to the group. Answers arrive on the loop thread.
    pub fn query(&self) -> Result<(), String> {
        self.socket
            .send_to(&encode_query(), group_addr())
            .map(|_| ())
            .map_err(|e| format!("mDNS send: {e}"))
    }

    /// Devices heard within the advert TTL, excluding this one.
    pub fn seen(&self) -> Vec<Seen> {
        let own_id = self.own.lock().map(|a| a.id.clone()).unwrap_or_default();
        let Ok(g) = self.seen.lock() else {
            return Vec::new();
        };
        let mut v: Vec<Seen> = g
            .values()
            .filter(|(a, _, t)| a.id != own_id && t.elapsed() < SEEN_TTL)
            .map(|(a, ip, t)| Seen {
                advert: a.clone(),
                ip: ip.clone(),
                age_secs: t.elapsed().as_secs(),
            })
            .collect();
        v.sort_by(|a, b| a.advert.label.cmp(&b.advert.label));
        v
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stop();
    }
}

type SeenMap = Arc<Mutex<HashMap<String, (Advert, String, Instant)>>>;

fn run_loop(sock: UdpSocket, stop: Arc<AtomicBool>, own: Arc<Mutex<Advert>>, seen: SeenMap) {
    let mut buf = [0u8; 9000];
    while !stop.load(Ordering::SeqCst) {
        let (n, from) = match sock.recv_from(&mut buf) {
            Ok(x) => x,
            Err(_) => continue, // read timeout: re-check the stop flag
        };
        let Ok(m) = parse(&buf[..n]) else { continue };
        if is_service_query(&m) {
            let a = own.lock().map(|g| g.clone());
            if let Ok(pkt) = a
                .map_err(|_| MdnsError::Invalid)
                .and_then(|a| encode_response(&a))
            {
                let _ = sock.send_to(&pkt, group_addr());
            }
            continue;
        }
        let found = adverts(&m);
        if found.is_empty() {
            continue;
        }
        if let Ok(mut g) = seen.lock() {
            if g.len() > 256 {
                g.retain(|_, (_, _, t)| t.elapsed() < SEEN_TTL);
            }
            for a in found {
                if g.len() < 256 || g.contains_key(&a.id) {
                    g.insert(a.id.clone(), (a, from.ip().to_string(), Instant::now()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    include!("fleet_mdns_tests.rs");

    /// Real sockets: start a responder and browse for it over the loopback multicast path.
    /// `#[ignore]`d because CI sandboxes often forbid multicast; run with `--ignored` on a desk.
    #[test]
    #[ignore]
    fn loopback_discovery_hears_a_second_instance() {
        let a = Discovery::start(advert()).unwrap();
        let b = Discovery::start(Advert {
            id: "ffff000011112222".into(),
            label: "Browser".into(),
            ..advert()
        })
        .unwrap();
        for _ in 0..10 {
            b.query().unwrap();
            std::thread::sleep(Duration::from_millis(300));
            if !b.seen().is_empty() {
                break;
            }
        }
        let seen = b.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].advert.id, advert().id);
        a.stop();
        b.stop();
    }
}
