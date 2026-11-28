// L7-WP01: the link vocabulary and `classify`. Written red-first: the module body was absent, so
// these failed to compile, then brought green.

use super::*;

const FIXTURE: &str = include_str!("fixtures/net_status_v1.json");

fn fixture() -> serde_json::Value {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

fn names<T: Serialize>(all: &[T]) -> Vec<String> {
    all.iter()
        .map(|v| {
            serde_json::to_value(v)
                .expect("serializes")
                .as_str()
                .expect("a string")
                .to_string()
        })
        .collect()
}

fn fixture_list(key: &str) -> Vec<String> {
    fixture()[key]
        .as_array()
        .expect("a list")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect()
}

fn keys(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().expect("an object").keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn fixture_json() {
    // The names on the wire are exactly the fixture's, in its order (TS reads the same file).
    assert_eq!(names(&LinkKind::ALL), fixture_list("linkKinds"));
    assert_eq!(names(&LinkSecurity::ALL), fixture_list("linkSecurity"));
    assert_eq!(fixture()["pqSuite"].as_str(), Some(PQ_SUITE));
    assert_eq!(fixture()["version"].as_u64(), Some(1));

    let ns = &fixture()["netStatus"];
    // Every enum-typed field parses into the Rust enum.
    let _: LinkKind = serde_json::from_value(ns["link"].clone()).expect("link kind");
    let _: LinkSecurity =
        serde_json::from_value(ns["meshLinkSecurity"].clone()).expect("link security");
    // The embedded existing shapes are the ones core serializes today.
    let ts = serde_json::to_value(crate::fleet_tailscale::TailscaleReport::not_installed())
        .expect("serializes");
    assert_eq!(keys(&ns["tailscale"]), keys(&ts));
    let d = crate::cluster_mesh::decide(None, false, true, false);
    let mesh = serde_json::to_value(crate::cluster_mesh::status_for(&d, false, true, false))
        .expect("serializes");
    assert_eq!(keys(&ns["mesh"]), keys(&mesh));
    // The NetStatus field set (section 3.2).
    let want = [
        "link",
        "mesh",
        "meshLinkSecurity",
        "note",
        "pairingPort",
        "privateAddrs",
        "tailscale",
        "tailscaleVia",
        "warp",
    ];
    assert_eq!(keys(ns), want);
}

#[test]
fn names_round_trip() {
    for k in LinkKind::ALL {
        let s = serde_json::to_string(&k).expect("serializes");
        assert_eq!(serde_json::from_str::<LinkKind>(&s).expect("parses"), k);
    }
    for v in LinkSecurity::ALL {
        let s = serde_json::to_string(&v).expect("serializes");
        assert_eq!(serde_json::from_str::<LinkSecurity>(&s).expect("parses"), v);
    }
    assert!(serde_json::from_str::<LinkSecurity>("\"quantum-safe\"").is_err());
}

#[test]
fn classical_tunnel_never_classifies_as_pq() {
    let not_pq = [
        PqState::Missing,
        PqState::Refused("peer has no pq-session protocol".into()),
    ];
    // A refusal keeps the peer's reason (shown per link once the daemon reports it, WP12).
    assert!(matches!(&not_pq[1], PqState::Refused(r) if r.contains("pq-session")));
    for link in LinkKind::ALL {
        for pq in &not_pq {
            assert_ne!(
                classify(link, pq),
                LinkSecurity::PqHybridE2e,
                "{link:?} {pq:?}"
            );
        }
    }
    for link in [
        LinkKind::TailnetOwn,
        LinkKind::TailnetManaged,
        LinkKind::Warp,
        LinkKind::Lan,
    ] {
        assert_eq!(
            classify(link, &PqState::Missing),
            LinkSecurity::ClassicalE2e
        );
    }
}

#[test]
fn pq_only_when_session_established_with_the_expected_suite() {
    let ok = PqState::Established {
        suite: PQ_SUITE.to_string(),
    };
    assert_eq!(
        classify(LinkKind::TailnetOwn, &ok),
        LinkSecurity::PqHybridE2e
    );
    assert_eq!(classify(LinkKind::Lan, &ok), LinkSecurity::PqHybridE2e);
    for other in [
        "",
        "X25519-HKDF-SHA256-AES128GCM",
        "x25519mlkem768-hkdf-sha384-aes256gcm",
        "X25519MLKEM768-HKDF-SHA384-AES256GCM ",
    ] {
        let s = PqState::Established {
            suite: other.to_string(),
        };
        assert_eq!(
            classify(LinkKind::TailnetOwn, &s),
            LinkSecurity::ClassicalE2e,
            "{other:?}"
        );
    }
}

#[test]
fn no_link_protects_nothing() {
    let ok = PqState::Established {
        suite: PQ_SUITE.to_string(),
    };
    assert_eq!(classify(LinkKind::None, &ok), LinkSecurity::None);
    assert_eq!(
        classify(LinkKind::None, &PqState::Missing),
        LinkSecurity::None
    );
}
