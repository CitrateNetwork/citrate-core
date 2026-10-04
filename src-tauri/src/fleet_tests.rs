// HUP-S8.2 — fleet wizard backend: roles, roster file, and pairing end to end over loopback TCP.
// Written red-first, then brought green.

use super::*;
use crate::fleet_pairing::{PairError, PairIssuer, PAIR_TTL_SECS};
use std::sync::{Arc, Mutex};

fn tmpdir(tag: &str) -> std::path::PathBuf {
    use rand::RngCore;
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    let d = std::env::temp_dir().join(format!("n4-fleet-test-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn me(id: &str, label: &str, tier: Option<&str>) -> DeviceSelf {
    DeviceSelf {
        device_id: id.into(),
        label: label.into(),
        tier: tier.map(str::to_string),
        link_code: None,
    }
}

// ---- roles ----------------------------------------------------------------------------------

#[test]
fn roles_follow_the_tier() {
    assert_eq!(role_for(Some("T0")), "light");
    assert_eq!(role_for(Some("T1")), "worker");
    assert_eq!(role_for(Some("T2")), "heavy");
    assert_eq!(role_for(None), "unknown");
    assert_eq!(role_for(Some("T7")), "unknown");
}

// ---- roster file ----------------------------------------------------------------------------

#[test]
fn a_missing_roster_starts_fresh_with_a_device_id() {
    let d = tmpdir("fresh");
    let r = load_roster(&d.join(ROSTER_FILE)).unwrap();
    assert_eq!(r.device_id.len(), 32);
    assert!(r.devices.is_empty());
    // Not written until something is saved.
    assert!(!d.join(ROSTER_FILE).exists());
}

#[test]
fn the_roster_round_trips_and_upserts_by_id() {
    let d = tmpdir("rt");
    let p = d.join(ROSTER_FILE);
    let mut r = load_roster(&p).unwrap();
    let dev = FleetDevice {
        id: "b".repeat(32),
        label: "Linux box".into(),
        tier: Some("T1".into()),
        role: "worker".into(),
        addr: Some("192.168.1.30".into()),
        paired_at: 10,
        via: PairVia::Issued,
        device_link: None,
        code: None,
    };
    upsert(&mut r, dev.clone());
    upsert(
        &mut r,
        FleetDevice {
            label: "Renamed".into(),
            paired_at: 11,
            ..dev.clone()
        },
    );
    save_roster(&p, &r).unwrap();
    let back = load_roster(&p).unwrap();
    assert_eq!(back.device_id, r.device_id);
    assert_eq!(back.devices.len(), 1);
    assert_eq!(back.devices[0].label, "Renamed");
}

#[test]
fn a_corrupt_roster_is_reported_not_overwritten() {
    let d = tmpdir("corrupt");
    let p = d.join(ROSTER_FILE);
    std::fs::write(&p, b"{not json").unwrap();
    assert!(load_roster(&p).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"{not json");
}

#[test]
fn the_roster_is_capped() {
    let mut r = RosterFile::fresh();
    for i in 0..(MAX_DEVICES + 5) {
        upsert(
            &mut r,
            FleetDevice {
                id: format!("{i:032x}"),
                label: format!("d{i}"),
                tier: None,
                role: "unknown".into(),
                addr: None,
                paired_at: i as u64,
                via: PairVia::Joined,
                device_link: None,
                code: None,
            },
        );
    }
    assert_eq!(r.devices.len(), MAX_DEVICES);
}

// ---- join request validation ----------------------------------------------------------------

#[test]
fn join_requests_are_validated() {
    let ok = JoinRequest {
        v: 1,
        link: "citrate://pair?c=x&s=y".into(),
        device_id: "a".repeat(32),
        label: "Laptop".into(),
        tier: Some("T0".into()),
        device_link: None,
    };
    assert!(ok.valid());
    // A device link code rides along, bounded like a pasted code.
    assert!(JoinRequest {
        device_link: Some("{}".into()),
        ..ok.clone()
    }
    .valid());
    assert!(!JoinRequest {
        device_link: Some("x".repeat(4097)),
        ..ok.clone()
    }
    .valid());
    assert!(!JoinRequest { v: 2, ..ok.clone() }.valid());
    assert!(!JoinRequest {
        device_id: "zz".into(),
        ..ok.clone()
    }
    .valid());
    assert!(!JoinRequest {
        label: "".into(),
        ..ok.clone()
    }
    .valid());
    assert!(!JoinRequest {
        tier: Some("T5".into()),
        ..ok.clone()
    }
    .valid());
}

// ---- pairing end to end (loopback TCP) ------------------------------------------------------

/// The tests run both machines on this one: loopback hints are allowed here. Production
/// `join_link` never connects to a loopback hint (see `hint_ip_allowed`).
fn join_local(link: &str, me: &DeviceSelf, now: u64) -> Result<JoinOutcome, JoinError> {
    join_link_with(link, me, now, &|ip: std::net::IpAddr| {
        ip.is_loopback() || hint_ip_allowed(ip)
    })
}

struct Rig {
    issuer: Arc<Mutex<PairIssuer>>,
    port: u16,
    roster: std::path::PathBuf,
    now: u64,
}

fn rig(tag: &str) -> Rig {
    let d = tmpdir(tag);
    let roster = d.join(ROSTER_FILE);
    let issuer = Arc::new(Mutex::new(PairIssuer::from_seed([5u8; 32])));
    let ctx = Arc::new(PairCtx {
        issuer: issuer.clone(),
        roster_path: roster.clone(),
        me: Mutex::new(me(&"c".repeat(32), "Studio", Some("T2"))),
        link_hook: None,
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let now = now_secs();
    // Keep the token alive for the whole test by issuing before the server starts.
    std::thread::spawn(move || serve_pairing(listener, ctx, |_| false));
    Rig {
        issuer,
        port,
        roster,
        now,
    }
}

fn link_for(r: &Rig) -> String {
    let hint = format!("127.0.0.1:{}", r.port);
    r.issuer
        .lock()
        .unwrap()
        .issue("Studio", Some("T2"), vec![hint], r.now)
        .unwrap()
        .1
}

#[test]
fn a_device_pairs_once_and_both_rosters_record_it() {
    let r = rig("e2e");
    let link = link_for(&r);
    let joiner = me(&"d".repeat(32), "Linux box", Some("T1"));
    let got = join_local(&link, &joiner, r.now).unwrap();
    // The joiner learns the issuer.
    assert_eq!(got.device.id, "c".repeat(32));
    assert_eq!(got.device.label, "Studio");
    assert_eq!(got.device.tier.as_deref(), Some("T2"));
    assert_eq!(got.device.role, "heavy");
    assert_eq!(got.device.via, PairVia::Joined);
    assert_eq!(got.device.addr.as_deref(), Some("127.0.0.1"));
    // The issuer recorded the joiner, with the tier and role it reported.
    let roster = load_roster(&r.roster).unwrap();
    assert_eq!(roster.devices.len(), 1);
    let d = &roster.devices[0];
    assert_eq!(d.id, "d".repeat(32));
    assert_eq!(d.label, "Linux box");
    assert_eq!(d.role, "worker");
    assert_eq!(d.via, PairVia::Issued);
    assert_eq!(d.addr.as_deref(), Some("127.0.0.1"));
    // Single use over the wire too.
    let again = join_local(&link, &me(&"e".repeat(32), "Other", None), r.now);
    assert_eq!(
        again.err(),
        Some(JoinError::Refused(PairError::AlreadyUsed))
    );
    assert_eq!(load_roster(&r.roster).unwrap().devices.len(), 1);
}

#[test]
fn an_expired_link_is_refused_before_any_connection() {
    let r = rig("exp");
    let link = link_for(&r);
    let joiner = me(&"d".repeat(32), "Linux box", Some("T1"));
    assert_eq!(
        join_local(&link, &joiner, r.now + PAIR_TTL_SECS).err(),
        Some(JoinError::Link(PairError::Expired))
    );
}

#[test]
fn a_device_cannot_pair_with_itself() {
    let r = rig("self");
    let link = link_for(&r);
    let same = me(&"c".repeat(32), "Studio again", Some("T2"));
    assert_eq!(
        join_local(&link, &same, r.now).err(),
        Some(JoinError::Refused(PairError::SameDevice))
    );
}

#[test]
fn an_unreachable_issuer_is_reported_as_unreachable() {
    // A port nothing listens on: bind, read the port, drop the listener.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut iss = PairIssuer::from_seed([6u8; 32]);
    let now = now_secs();
    let (_c, link) = iss
        .issue("Studio", None, vec![format!("127.0.0.1:{port}")], now)
        .unwrap();
    let res = join_local(&link, &me(&"d".repeat(32), "Linux box", None), now);
    match res {
        Err(JoinError::Unreachable(tried)) => assert_eq!(tried, vec![format!("127.0.0.1:{port}")]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn junk_on_the_pairing_port_is_answered_with_an_error_and_changes_nothing() {
    use std::io::{BufRead, BufReader, Write};
    let r = rig("junk");
    let _link = link_for(&r);
    let mut s = std::net::TcpStream::connect(("127.0.0.1", r.port)).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).unwrap();
    let reply: JoinReply = serde_json::from_str(line.trim()).unwrap();
    assert!(!reply.ok);
    assert_eq!(reply.error, Some(PairError::Malformed));
    assert_eq!(r.issuer.lock().unwrap().outstanding(r.now), 1);
    assert!(!r.roster.exists());
}

#[test]
fn the_server_stops_when_told() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ctx = Arc::new(PairCtx {
        issuer: Arc::new(Mutex::new(PairIssuer::from_seed([1u8; 32]))),
        roster_path: tmpdir("stop").join(ROSTER_FILE),
        me: Mutex::new(me(&"c".repeat(32), "Studio", None)),
        link_hook: None,
    });
    let h = std::thread::spawn(move || serve_pairing(listener, ctx, |_| true));
    assert!(h.join().is_ok());
}

// ---- hints ----------------------------------------------------------------------------------

#[test]
fn hints_dedupe_and_cap() {
    let h = build_hints(
        Some("192.168.1.20".parse().unwrap()),
        &[
            "100.64.1.10".into(),
            "100.64.1.10".into(),
            "not-an-ip".into(),
        ],
        41234,
    );
    assert_eq!(h, vec!["192.168.1.20:41234", "100.64.1.10:41234"]);
    let none = build_hints(None, &[], 1);
    assert!(none.is_empty());
}

// ---- review additions: the issuer validates what arrives on the wire --------------------------

fn send_line(port: u16, line: &str) -> JoinReply {
    use std::io::{BufRead, BufReader, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    s.write_all(b"\n").unwrap();
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply).unwrap();
    serde_json::from_str(reply.trim()).unwrap()
}

#[test]
fn an_invalid_join_request_on_the_wire_is_refused_and_burns_nothing() {
    let r = rig("badreq");
    let link = link_for(&r);
    // Well-formed JSON with a valid link, but a label carrying control characters and a tier
    // outside T0..T2: the issuer must refuse it before redeeming the link.
    let req = serde_json::json!({
        "v": 1,
        "link": link,
        "deviceId": "d".repeat(32),
        "label": "evil\u{1b}[2Jlabel",
        "tier": "T9",
    });
    let reply = send_line(r.port, &req.to_string());
    assert!(!reply.ok);
    assert_eq!(reply.error, Some(PairError::Malformed));
    assert_eq!(r.issuer.lock().unwrap().outstanding(r.now), 1);
    assert!(!r.roster.exists());
    // The link still works for the real machine afterwards.
    let ok = join_local(&link, &me(&"d".repeat(32), "Linux box", Some("T1")), r.now);
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn a_broken_issuer_roster_never_burns_the_link() {
    let r = rig("brokenroster");
    let link = link_for(&r);
    std::fs::write(&r.roster, b"{not json").unwrap();
    let res = join_local(&link, &me(&"d".repeat(32), "Linux box", Some("T1")), r.now);
    assert_eq!(res.err(), Some(JoinError::Refused(PairError::Malformed)));
    assert_eq!(r.issuer.lock().unwrap().outstanding(r.now), 1);
    // The unreadable file is left exactly as it was.
    assert_eq!(std::fs::read(&r.roster).unwrap(), b"{not json");
}

// ---- HUP-S8.1/S8.2: paired machines exchange their DeviceLink codes ----------------------------

#[test]
fn paired_machines_exchange_their_device_link_codes() {
    let d = tmpdir("links");
    let roster = d.join(ROSTER_FILE);
    let issuer = Arc::new(Mutex::new(PairIssuer::from_seed([6u8; 32])));
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_hook = seen.clone();
    let mut studio = me(&"c".repeat(32), "Studio", Some("T2"));
    studio.link_code = Some("issuer-link-code".into());
    let ctx = Arc::new(PairCtx {
        issuer: issuer.clone(),
        roster_path: roster.clone(),
        me: Mutex::new(studio),
        link_hook: Some(Arc::new(move |code: Option<&str>| {
            seen_hook.lock().unwrap().push(code.map(str::to_string));
            PairedLink::Added
        })),
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let now = now_secs();
    std::thread::spawn(move || serve_pairing(listener, ctx, |_| false));
    let link = issuer
        .lock()
        .unwrap()
        .issue("Studio", Some("T2"), vec![format!("127.0.0.1:{port}")], now)
        .unwrap()
        .1;

    let mut joiner = me(&"d".repeat(32), "Linux box", Some("T1"));
    joiner.link_code = Some("joiner-link-code".into());
    let got = join_local(&link, &joiner, now).unwrap();
    // The joiner receives the issuer's code (core verifies and stores it).
    assert_eq!(got.peer_link_code.as_deref(), Some("issuer-link-code"));
    // The issuer handed the joiner's code to its hook, and recorded the outcome on the roster entry.
    assert_eq!(
        *seen.lock().unwrap(),
        vec![Some("joiner-link-code".to_string())]
    );
    let r = load_roster(&roster).unwrap();
    assert_eq!(r.devices[0].device_link, Some(PairedLink::Added));
}

#[test]
fn a_pairing_without_link_codes_is_unchanged() {
    let r = rig("nolinks");
    let link = link_for(&r);
    let got = join_local(&link, &me(&"d".repeat(32), "Linux box", None), r.now).unwrap();
    assert_eq!(got.peer_link_code, None);
    let roster = load_roster(&r.roster).unwrap();
    assert_eq!(roster.devices[0].device_link, None);
}

// ---- red-team follow-ups: the joiner authenticates the issuer; hints stay on private networks --

#[test]
fn both_sides_show_the_same_confirmation_code() {
    let r = rig("code");
    let link = link_for(&r);
    let got = join_local(&link, &me(&"d".repeat(32), "Linux box", Some("T1")), r.now).unwrap();
    assert_eq!(got.code.len(), 6);
    assert!(got.code.bytes().all(|b| b.is_ascii_digit()));
    let roster = load_roster(&r.roster).unwrap();
    assert_eq!(roster.devices[0].code.as_deref(), Some(got.code.as_str()));
}

/// A listener that answers like an issuer but does not hold the pairing key.
fn impostor(reply: serde_json::Value) -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((s, _)) = l.accept() {
            let mut line = String::new();
            let _ = BufReader::new(s.try_clone().unwrap()).read_line(&mut line);
            let _ = (&s).write_all(format!("{reply}\n").as_bytes());
        }
    });
    port
}

#[test]
fn a_reply_without_the_issuers_proof_is_not_a_pairing() {
    let now = now_secs();
    for proof in [serde_json::Value::Null, serde_json::json!("00".repeat(64))] {
        let port = impostor(serde_json::json!({
            "ok": true, "error": null, "deviceId": "c".repeat(32), "label": "Studio",
            "tier": "T2", "proof": proof,
        }));
        let mut iss = PairIssuer::from_seed([9u8; 32]);
        let (_c, link) = iss
            .issue("Studio", None, vec![format!("127.0.0.1:{port}")], now)
            .unwrap();
        assert_eq!(
            join_local(&link, &me(&"d".repeat(32), "Linux box", None), now).err(),
            Some(JoinError::Protocol)
        );
    }
}

#[test]
fn hints_are_only_private_cgnat_or_link_local_addresses() {
    for ok in ["10.1.2.3", "172.16.0.9", "192.168.1.20", "100.64.1.10", "169.254.10.1", "fe80::1", "fd12::1"] {
        assert!(hint_ip_allowed(ok.parse().unwrap()), "{ok}");
    }
    for bad in ["8.8.8.8", "127.0.0.1", "0.0.0.0", "169.254.169.254", "255.255.255.255", "::1", "2001:4860::8888", "224.0.0.1", "172.32.0.1", "100.128.0.1"] {
        assert!(!hint_ip_allowed(bad.parse().unwrap()), "{bad}");
    }
}

#[test]
fn a_link_pointing_at_this_machine_or_the_internet_makes_no_connection() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let port = l.local_addr().unwrap().port();
    let now = now_secs();
    let mut iss = PairIssuer::from_seed([7u8; 32]);
    let (_c, link) = iss
        .issue("Studio", None, vec![format!("127.0.0.1:{port}"), "8.8.8.8:53".into()], now)
        .unwrap();
    let res = join_link(&link, &me(&"d".repeat(32), "Linux box", None), now);
    assert!(matches!(res, Err(JoinError::Unreachable(ref t)) if t.is_empty()), "{res:?}");
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(l.accept().is_err(), "nothing connected to the loopback hint");
}

#[test]
fn the_pairing_port_answers_only_local_network_peers() {
    for ok in ["127.0.0.1", "192.168.1.4", "100.70.1.2", "fe80::2"] {
        assert!(peer_allowed(ok.parse().unwrap()), "{ok}");
    }
    for bad in ["8.8.8.8", "2001:4860::8888"] {
        assert!(!peer_allowed(bad.parse().unwrap()), "{bad}");
    }
}
