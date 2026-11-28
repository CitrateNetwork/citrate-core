// L7-WP01 characterization suite (lanes/L7_NETWORKING.md section 4, L7-WP01): today's networking
// behavior at core main, pinned before any L7 work package changes it. Each test passes on the code
// as it ships. A later L7 WP that changes one of these behaviors replaces the test with a stricter
// one in the same PR and names it (WP02: the two pairing-key tests; WP03: the ephemeral port;
// WP10: the all-interfaces listen; WP11: the gate text). Never deleted without that replacement.
// LQ runs this module on every merge (ci/characterization.d/l7.json, suite L7-CH-1).

use crate::cluster_mesh::{
    decide, MeshSource, DEFAULT_LISTEN, MULTI_GROUP_DAEMON, TRANSPORT_SIGNED_OFF,
};
use crate::fleet_pairing::{PairError, PairIssuer};

const NOW: u64 = 1_790_000_000;

fn hints() -> Vec<String> {
    vec!["192.168.1.20:41234".to_string()]
}

#[test]
fn char_mesh_needs_operator_env_today() {
    // N1-N3: with no CITRATE_CLUSTER_LISTEN this build runs the in-process transport only.
    let d = decide(None, TRANSPORT_SIGNED_OFF, MULTI_GROUP_DAEMON, false);
    assert_eq!(d.source, MeshSource::Off);
    assert_eq!(d.listen, None);
    let d = decide(Some("  "), TRANSPORT_SIGNED_OFF, MULTI_GROUP_DAEMON, false);
    assert_eq!(d.source, MeshSource::Off);
    // The operator's env turns it on, whatever the build says.
    let d = decide(
        Some("/ip4/192.168.1.5/tcp/4211"),
        TRANSPORT_SIGNED_OFF,
        MULTI_GROUP_DAEMON,
        true,
    );
    assert_eq!(d.source, MeshSource::Operator);
    assert_eq!(d.listen.as_deref(), Some("/ip4/192.168.1.5/tcp/4211"));
}

#[test]
fn char_default_listen_is_all_interfaces_4211() {
    // N3: pinned so WP10 (private-address listen) has to change it on purpose.
    assert_eq!(DEFAULT_LISTEN, "/ip4/0.0.0.0/tcp/4211");
}

#[test]
fn char_pairing_listener_binds_an_ephemeral_port() {
    // N8: every interface, a port the OS picks, a new one per listener. WP03 replaces this with a
    // fixed, documented port on private addresses.
    let a = crate::fleet::bind_pairing_listener().expect("binds");
    let b = crate::fleet::bind_pairing_listener().expect("binds");
    let (aa, ba) = (a.local_addr().expect("addr"), b.local_addr().expect("addr"));
    assert!(aa.ip().is_unspecified(), "{aa}");
    assert_ne!(aa.port(), 0);
    assert_ne!(aa.port(), ba.port());
}

#[test]
fn char_pairing_key_is_new_each_run() {
    // N9: before L7-WP02 the production issuer was `PairIssuer::new_random()`, once per app run.
    // Since WP02 production uses `PairIssuer::from_keyring` and `new_random` is the in-memory
    // fallback only; the stricter replacement is fleet_pairing_tests
    // `the_pairing_key_is_the_same_after_restart`. This pin still holds for the fallback.
    assert_ne!(
        PairIssuer::new_random().public_hex(),
        PairIssuer::new_random().public_hex()
    );
}

#[test]
fn char_used_tokens_are_forgotten_by_a_new_issuer() {
    // N9: the outstanding and used sets live in memory. Even with the same key, a new issuer (the
    // next app run) knows neither: a used link no longer reads "already used", and a link issued
    // before the restart is unknown. WP02 closes this with a persisted ledger for the production
    // issuer (`a_redeemed_token_stays_used_after_restart`,
    // `an_outstanding_token_survives_restart_and_redeems_once`); this pin still holds for an
    // in-memory issuer, which is what the keychain fallback is.
    let mut before = PairIssuer::from_seed([7u8; 32]);
    let (_c, used) = before.issue("Studio", None, hints(), NOW).expect("issues");
    let (_c, open) = before.issue("Studio", None, hints(), NOW).expect("issues");
    assert!(before.redeem(&used, NOW + 1).is_ok());
    assert_eq!(before.redeem(&used, NOW + 2), Err(PairError::AlreadyUsed));

    let mut after = PairIssuer::from_seed([7u8; 32]);
    assert_eq!(after.redeem(&used, NOW + 3), Err(PairError::UnknownToken));
    assert_eq!(after.redeem(&open, NOW + 3), Err(PairError::UnknownToken));
}

#[test]
fn char_hints_refuse_public_loopback_metadata() {
    // N7: a pairing link may only point at private, tailnet/CGNAT or link-local addresses.
    use crate::fleet::hint_ip_allowed;
    for no in [
        "8.8.8.8",
        "127.0.0.1",
        "169.254.169.254",
        "::1",
        "2001:4860:4860::8888",
    ] {
        assert!(!hint_ip_allowed(no.parse().expect("ip")), "{no}");
    }
    for yes in [
        "192.168.1.2",
        "10.0.0.3",
        "172.16.0.4",
        "100.100.1.1",
        "169.254.10.1",
        "fd00::1",
    ] {
        assert!(hint_ip_allowed(yes.parse().expect("ip")), "{yes}");
    }
}

#[test]
fn char_tailscale_only_runs_status_json() {
    // N10: the one argv core ever passes to the Tailscale CLI.
    assert_eq!(crate::fleet_tailscale::STATUS_ARGS, &["status", "--json"]);
}

/// The TS constant `MESH_NOT_AVAILABLE` in `AddMachineSheet.tsx`, read from the source.
fn ts_mesh_not_available(src: &str) -> String {
    let marker = "export const MESH_NOT_AVAILABLE =";
    let rest = &src[src.find(marker).expect("TS constant present") + marker.len()..];
    let open = rest.find('"').expect("opening quote") + 1;
    let len = rest[open..].find('"').expect("closing quote");
    rest[open..open + len].to_string()
}

#[test]
fn char_mesh_gate_text_matches_ts() {
    // N4, N5: mesh-only actions answer the same words in core and in the Add-a-machine sheet.
    let ts = ts_mesh_not_available(include_str!("../../../src/surfaces/AddMachineSheet.tsx"));
    assert_eq!(ts, crate::cluster::MESH_NOT_AVAILABLE);
    assert_eq!(
        crate::cluster::mesh_gate(false),
        Err(crate::cluster::MESH_NOT_AVAILABLE.to_string())
    );
    assert_eq!(crate::cluster::mesh_gate(true), Ok(()));
}

#[test]
fn char_suite_is_listed_for_lq() {
    // The lane registry and this file name the same tests, so LQ runs what is here.
    let me = include_str!("characterization_tests.rs");
    let reg: serde_json::Value =
        serde_json::from_str(include_str!("../../../ci/characterization.d/l7.json"))
            .expect("registry parses");
    let suite = reg["suites"]
        .as_array()
        .expect("suites")
        .iter()
        .find(|s| s["id"] == "L7-CH-1")
        .expect("suite L7-CH-1");
    assert_eq!(suite["filter"], "fleet::net::characterization_tests::");
    let mut listed: Vec<String> = suite["tests"]
        .as_array()
        .expect("tests")
        .iter()
        .map(|t| t.as_str().expect("name").to_string())
        .collect();
    let mut here: Vec<String> = me
        .lines()
        .filter_map(|l| l.strip_prefix("fn "))
        .filter(|l| l.starts_with("char_"))
        .filter_map(|l| l.split('(').next())
        .map(str::to_string)
        .collect();
    listed.sort();
    here.sort();
    assert_eq!(listed, here);
}
