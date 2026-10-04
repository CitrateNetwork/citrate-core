// HUP-S8.1 follow-on: sharing DeviceLinks between members over the group relay. Real secp256k1
// signatures throughout (scoped test keys built at runtime; the wallet signature stands in for the
// ceremony's output, which is what `device_link_tests.rs` proves separately).

use super::*;
use crate::device_link::{
    revocation_message, sign_eip191, sign_revocation, DeviceLinkBody, DeviceLinkStore,
    DeviceLinkWire, RevocationWire,
};

fn seed(byte: u8) -> String {
    let mut b = [byte; 32];
    b[0] = byte | 1;
    hex::encode(b)
}

fn addr(seed_hex: &str) -> String {
    crate::comms::address_from_secret_hex(seed_hex).expect("valid seed")
}

/// A fully signed link of `device` to `member`, with `wallet` standing in for the ceremony.
fn link(member: &str, device: &str, wallet: &str, index: u32, label: &str, at: u64) -> DeviceLinkWire {
    let body = DeviceLinkBody::new(&addr(member), &addr(device), &addr(wallet), index, label, at)
        .expect("valid body");
    let msg = body.signing_message();
    DeviceLinkWire {
        member_sig: sign_eip191(member, &msg).expect("sign"),
        device_sig: sign_eip191(device, &msg).expect("sign"),
        wallet_sig: sign_eip191(wallet, &msg).expect("sign"),
        member: body.member,
        device: body.device,
        wallet: body.wallet,
        index: body.index,
        label: body.label,
        issued_at: body.issued_at,
    }
}

fn own_store(member: &str, devices: &[(&str, u32, &str)]) -> DeviceLinkStore {
    let wallet = seed(0x0b);
    let mut s = DeviceLinkStore::default();
    for (d, i, l) in devices {
        s.upsert_link(link(member, d, &wallet, *i, l, 1_790_000_000))
            .expect("upsert");
    }
    s
}

fn body_of(offer: &ShareOffer) -> &str {
    offer
        .body
        .strip_prefix(DEVICE_LINKS_MSG_PREFIX)
        .expect("prefixed")
}

// ---- what a member shares ----

#[test]
fn nothing_is_shared_until_this_member_has_a_link_or_a_revocation() {
    let m = seed(0x21);
    assert!(share_offer(&DeviceLinkStore::default(), &addr(&m)).is_none());
}

#[test]
fn a_member_shares_only_its_own_links_and_revocations() {
    let m = seed(0x21);
    let other = seed(0x31);
    let mut s = own_store(&m, &[(&seed(0x22), 0, "Studio Mac")]);
    // A link of another member that this store happens to hold is never re-shared by us.
    s.links.push(link(&other, &seed(0x32), &seed(0x0c), 0, "Not mine", 1_790_000_000));
    s.revoke(sign_revocation(&m, &addr(&seed(0x23)), 1_790_000_100).expect("rev"));
    // Nor is another member's revocation, should one ever sit in this store.
    s.revocations
        .push(sign_revocation(&other, &addr(&seed(0x33)), 1_790_000_100).expect("rev"));
    let offer = share_offer(&s, &addr(&m)).expect("something to share");
    assert!(offer.body.starts_with(DEVICE_LINKS_MSG_PREFIX));
    let p: SharePayload = serde_json::from_str(body_of(&offer)).expect("json");
    assert_eq!(p.v, 1);
    assert_eq!(p.links.len(), 1);
    assert_eq!(p.links[0].member, addr(&m));
    assert_eq!(p.revocations.len(), 1);
    assert_eq!(offer.digest.len(), 64, "sha256 hex");
    // Same content, same digest (so a group is not sent the same set twice).
    assert_eq!(share_offer(&s, &addr(&m)).expect("again").digest, offer.digest);
}

// ---- what a member accepts ----

#[test]
fn a_peers_links_are_accepted_only_from_that_member_and_only_when_every_signature_verifies() {
    let me = seed(0x41);
    let peer = seed(0x51);
    let peer_dev = seed(0x52);
    let s = own_store(&peer, &[(&peer_dev, 0, "Peer box")]);
    let offer = share_offer(&s, &addr(&peer)).expect("offer");

    // Sent by the peer itself: accepted.
    let mut store = PeerLinkStore::default();
    let r = ingest(&mut store, &addr(&me), &addr(&peer), &offer.body).expect("ingest");
    assert_eq!((r.links, r.revocations), (1, 0));
    assert_eq!(store.links.len(), 1);

    // Relayed by somebody else: refused (the sender must be the member the links name).
    let mut store2 = PeerLinkStore::default();
    let r = ingest(&mut store2, &addr(&me), &addr(&seed(0x61)), &offer.body).expect("ingest");
    assert_eq!(r.links, 0);
    assert!(store2.links.is_empty());
    assert!(!r.refused.is_empty());

    // One flipped signature byte: refused.
    let mut p: SharePayload = serde_json::from_str(body_of(&offer)).expect("json");
    let mut sig = p.links[0].device_sig.clone();
    let last = sig.pop().expect("char");
    sig.push(if last == '0' { '1' } else { '0' });
    p.links[0].device_sig = sig;
    let forged = format!("{DEVICE_LINKS_MSG_PREFIX}{}", serde_json::to_string(&p).expect("json"));
    let mut store3 = PeerLinkStore::default();
    let r = ingest(&mut store3, &addr(&me), &addr(&peer), &forged).expect("ingest");
    assert_eq!(r.links, 0);
    assert!(store3.links.is_empty());
}

#[test]
fn our_own_announcements_are_ignored() {
    let me = seed(0x41);
    let s = own_store(&me, &[(&seed(0x42), 0, "Mine")]);
    let offer = share_offer(&s, &addr(&me)).expect("offer");
    let mut store = PeerLinkStore::default();
    let r = ingest(&mut store, &addr(&me), &addr(&me), &offer.body).expect("ingest");
    assert_eq!(r.links, 0);
    assert!(store.links.is_empty(), "own links live in the own store");
}

#[test]
fn a_peers_revocation_removes_its_link_and_keeps_it_out_for_good() {
    let me = seed(0x41);
    let peer = seed(0x51);
    let dev = seed(0x52);
    let s = own_store(&peer, &[(&dev, 0, "Peer box")]);
    let first = share_offer(&s, &addr(&peer)).expect("offer");
    let mut store = PeerLinkStore::default();
    ingest(&mut store, &addr(&me), &addr(&peer), &first.body).expect("ingest");
    assert_eq!(store.links.len(), 1);

    let mut s2 = s.clone();
    s2.revoke(sign_revocation(&peer, &addr(&dev), 1_790_000_100).expect("rev"));
    let second = share_offer(&s2, &addr(&peer)).expect("offer");
    let r = ingest(&mut store, &addr(&me), &addr(&peer), &second.body).expect("ingest");
    assert_eq!(r.revocations, 1);
    assert!(store.links.is_empty(), "the revoked link is dropped");

    // The old announcement arrives again (out of order): the link stays out.
    let r = ingest(&mut store, &addr(&me), &addr(&peer), &first.body).expect("ingest");
    assert_eq!(r.links, 0);
    assert!(store.links.is_empty());
}

#[test]
fn a_revocation_signed_by_someone_else_is_refused() {
    let me = seed(0x41);
    let peer = seed(0x51);
    let mallory = seed(0x71);
    let dev = seed(0x52);
    // Mallory signs a revocation that claims to be the peer's.
    let rev = RevocationWire {
        member: addr(&peer),
        device: addr(&dev),
        revoked_at: 1_790_000_100,
        member_sig: sign_eip191(&mallory, &revocation_message(&addr(&peer), &addr(&dev), 1_790_000_100))
            .expect("sign"),
    };
    let body = format!(
        "{DEVICE_LINKS_MSG_PREFIX}{}",
        serde_json::to_string(&SharePayload { v: 1, links: vec![], revocations: vec![rev] })
            .expect("json")
    );
    let mut store = PeerLinkStore::default();
    let r = ingest(&mut store, &addr(&me), &addr(&peer), &body).expect("ingest");
    assert_eq!(r.revocations, 0);
    assert!(store.revocations.is_empty());
}

#[test]
fn oversized_or_foreign_messages_are_refused_without_touching_the_store() {
    let me = seed(0x41);
    let mut store = PeerLinkStore::default();
    assert!(ingest(&mut store, &addr(&me), &addr(&seed(0x51)), "hello").is_err());
    let big = format!("{DEVICE_LINKS_MSG_PREFIX}{}", "x".repeat(MAX_MESSAGE_BYTES));
    assert!(ingest(&mut store, &addr(&me), &addr(&seed(0x51)), &big).is_err());
    assert_eq!(store, PeerLinkStore::default());
}

#[test]
fn a_member_cannot_flood_the_store_past_its_cap() {
    let me = seed(0x41);
    let peer = seed(0x51);
    let wallet = seed(0x0b);
    let mut store = PeerLinkStore::default();
    // More than one member may hold: build MAX_LINKS + 3 links (each verifies on its own).
    let links: Vec<DeviceLinkWire> = (0..(crate::device_link::MAX_LINKS + 3))
        .map(|i| {
            let mut b = [0x90u8; 32];
            b[30] = (i >> 8) as u8;
            b[31] = (i & 0xff) as u8 | 1;
            link(&peer, &hex::encode(b), &wallet, i as u32, "Box", 1_790_000_000)
        })
        .collect();
    let body = format!(
        "{DEVICE_LINKS_MSG_PREFIX}{}",
        serde_json::to_string(&SharePayload { v: 1, links, revocations: vec![] }).expect("json")
    );
    // Fits under the message cap? If not, the message is refused whole, which is also a cap.
    match ingest(&mut store, &addr(&me), &addr(&peer), &body) {
        Ok(r) => assert!(r.links <= crate::device_link::MAX_LINKS),
        Err(_) => assert!(store.links.is_empty()),
    }
    assert!(store.links.len() <= crate::device_link::MAX_LINKS);
}

// ---- what goes to the cluster daemon ----

fn roster_of(members: &[&str]) -> Vec<(String, String)> {
    members
        .iter()
        .map(|m| (format!("0x{}", addr(m)), "member".to_string()))
        .collect()
}

#[test]
fn the_daemon_gets_own_links_first_then_peers_of_roster_members_only() {
    let me = seed(0x41);
    let peer = seed(0x51);
    let stranger = seed(0x61);
    let own = own_store(&me, &[(&seed(0x42), 0, "Mine")]);
    let mut peers = PeerLinkStore::default();
    for (m, d) in [(&peer, seed(0x52)), (&stranger, seed(0x62))] {
        let s = own_store(m, &[(&d, 0, "Theirs")]);
        let o = share_offer(&s, &addr(m)).expect("offer");
        ingest(&mut peers, &addr(&me), &addr(m), &o.body).expect("ingest");
    }
    assert_eq!(peers.links.len(), 2);
    let (links, revs) = roster_update(&own, &peers, &roster_of(&[&me, &peer]));
    let members: Vec<&str> = links.iter().map(|l| l.member.as_str()).collect();
    assert_eq!(members, vec![addr(&me).as_str(), addr(&peer).as_str()]);
    assert!(revs.is_empty());
}

#[test]
fn the_update_never_carries_a_revoked_link_and_stays_within_the_daemon_caps() {
    let me = seed(0x41);
    let peer = seed(0x51);
    let wallet = seed(0x0b);
    let mut own = own_store(&me, &[(&seed(0x42), 0, "Mine")]);
    // A peer link that our OWN store has a revocation for is never sent.
    let peer_dev = seed(0x52);
    let mut peers = PeerLinkStore {
        links: vec![link(&peer, &peer_dev, &wallet, 0, "Theirs", 1_790_000_000)],
        revocations: vec![],
    };
    own.revocations
        .push(sign_revocation(&peer, &addr(&peer_dev), 1_790_000_200).expect("rev"));
    let (links, _) = roster_update(&own, &peers, &roster_of(&[&me, &peer]));
    assert!(links.iter().all(|l| l.device != addr(&peer_dev)));

    // Caps: at most MAX_LINKS links and MAX_LINKS revocations, newest revocations first.
    peers.links.clear();
    for i in 0..(crate::device_link::MAX_LINKS as u16 + 10) {
        let mut r = [0x77u8; 32];
        r[30] = (i >> 8) as u8;
        r[31] = (i & 0xff) as u8 | 1;
        // Stored revocations were verified at ingest; the update builder only selects them.
        peers.revocations.push(RevocationWire {
            member: addr(&peer),
            device: addr(&hex::encode(r)),
            revoked_at: 1_790_000_000 + u64::from(i),
            member_sig: String::new(),
        });
        let mut d = [0x78u8; 32];
        d[30] = (i >> 8) as u8;
        d[31] = (i & 0xff) as u8 | 1;
        peers
            .links
            .push(link(&peer, &hex::encode(d), &wallet, u32::from(i), "Box", 1_790_000_000));
    }
    let (links, revs) = roster_update(&own, &peers, &roster_of(&[&me, &peer]));
    assert_eq!(links[0].member, addr(&me), "own links are kept first");
    assert!(links.len() <= crate::device_link::MAX_LINKS);
    assert!(revs.len() <= crate::device_link::MAX_LINKS);
    assert!(
        revs.windows(2).all(|w| w[0].revoked_at >= w[1].revoked_at),
        "newest revocations first"
    );
}

#[test]
fn a_full_roster_update_fits_the_daemons_ipc_line() {
    // The worst case core can send: MAX_LINKS links with the longest labels, MAX_LINKS revocations,
    // and a 700-member roster. The daemon reads at most 64 KiB per line.
    let wallet = seed(0x0b);
    let member = seed(0x21);
    let label = "W".repeat(crate::device_link::MAX_LABEL_LEN);
    let links: Vec<DeviceLinkWire> = (0..crate::device_link::MAX_LINKS)
        .map(|i| {
            let mut b = [0x66u8; 32];
            b[31] = i as u8 | 1;
            link(&member, &hex::encode(b), &wallet, i as u32, &label, 1_790_000_000)
        })
        .collect();
    let revocations: Vec<RevocationWire> = (0..crate::device_link::MAX_LINKS)
        .map(|i| {
            let mut b = [0x55u8; 32];
            b[31] = i as u8 | 1;
            sign_revocation(&member, &addr(&hex::encode(b)), 1_790_000_000).expect("rev")
        })
        .collect();
    // A large group: the roster alone is about 36 KiB, so the device part must be trimmed to fit.
    let mut roster: Vec<(String, String)> = (0..700u32)
        .map(|i| (format!("0x{:040x}", i + 1), "member".to_string()))
        .collect();
    roster.push((format!("0x{}", addr(&member)), "member".to_string()));
    let peers = PeerLinkStore { links, revocations };
    let (links, revocations) = roster_update(&DeviceLinkStore::default(), &peers, &roster);
    assert!(!links.is_empty() && !revocations.is_empty(), "the update is trimmed, not emptied");
    let line = serde_json::to_string(&serde_json::json!({
        "op": "setRoster", "group": "g".repeat(64), "roster": roster,
        "devices": links, "revocations": revocations,
    }))
    .expect("json");
    assert!(
        line.len() < 64 * 1024,
        "a full update is {} bytes; the daemon's line cap is 65536",
        line.len()
    );
}
