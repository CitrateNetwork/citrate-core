// HUP-S8.3 — Tailscale detection (read-only) + connectivity guidance tests. Fixtures are
// synthetic (invented host names and addresses), shaped like `tailscale status --json`.
// Written red-first, then brought green.

use super::*;

const RUNNING: &str = r#"{
  "Version": "1.90.1-t1234",
  "BackendState": "Running",
  "TailscaleIPs": ["100.64.1.10", "fd7a:115c:a1e0::1"],
  "Self": {"HostName": "studio", "OS": "macOS", "TailscaleIPs": ["100.64.1.10", "fd7a:115c:a1e0::1"], "Online": true},
  "MagicDNSSuffix": "example.ts.net",
  "Peer": {
    "nodekey:aa": {"HostName": "linux-box", "OS": "linux", "TailscaleIPs": ["100.64.1.11"], "Online": true},
    "nodekey:bb": {"HostName": "old-laptop", "OS": "windows", "TailscaleIPs": ["100.64.1.12"], "Online": false}
  }
}"#;

#[test]
fn a_running_tailnet_is_parsed() {
    let r = parse_status(RUNNING);
    assert_eq!(r.state, TsState::Running);
    assert_eq!(r.version.as_deref(), Some("1.90.1-t1234"));
    assert_eq!(r.self_host.as_deref(), Some("studio"));
    assert_eq!(r.self_ips, vec!["100.64.1.10", "fd7a:115c:a1e0::1"]);
    assert_eq!(r.peers.len(), 2);
    // Sorted: online first, then by name.
    assert_eq!(r.peers[0].host_name, "linux-box");
    assert!(r.peers[0].online);
    assert_eq!(r.peers[1].host_name, "old-laptop");
    assert!(!r.peers[1].online);
    assert_eq!(tailnet_ipv4s(&r), vec!["100.64.1.10"]);
}

#[test]
fn backend_states_map_to_states() {
    for (b, want) in [
        ("NeedsLogin", TsState::NeedsLogin),
        ("NeedsMachineAuth", TsState::NeedsLogin),
        ("Stopped", TsState::Stopped),
        ("Starting", TsState::Starting),
        ("NoState", TsState::NotRunning),
        ("Weird", TsState::Unknown),
    ] {
        let r = parse_status(&format!(r#"{{"BackendState":"{b}"}}"#));
        assert_eq!(r.state, want, "{b}");
        assert!(r.peers.is_empty());
    }
}

#[test]
fn garbage_is_unknown_not_a_panic() {
    assert_eq!(parse_status("not json").state, TsState::Unknown);
    assert_eq!(parse_status("{}").state, TsState::Unknown);
    let r = parse_status(r#"{"BackendState":"Running","Peer":{"x":{"HostName":5}}}"#);
    assert_eq!(r.state, TsState::Running);
    assert!(r.peers.is_empty());
}

#[test]
fn cli_failures_are_classified() {
    assert_eq!(
        classify_cli_failure(
            "failed to connect to local tailscaled; it doesn't appear to be running"
        ),
        TsState::NotRunning
    );
    assert_eq!(classify_cli_failure("Logged out."), TsState::NeedsLogin);
    assert_eq!(classify_cli_failure("something else"), TsState::Unknown);
}

#[test]
fn candidate_paths_cover_path_and_the_os_install_location() {
    let mac = candidate_paths("macos", Some("/usr/bin:/opt/homebrew/bin"));
    assert!(mac.contains(&std::path::PathBuf::from("/opt/homebrew/bin/tailscale")));
    assert!(mac.contains(&std::path::PathBuf::from(
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale"
    )));
    let linux = candidate_paths("linux", None);
    assert!(linux.contains(&std::path::PathBuf::from("/usr/bin/tailscale")));
    let win = candidate_paths("windows", None);
    assert!(win
        .iter()
        .any(|p| p.to_string_lossy().ends_with("Tailscale\\tailscale.exe")));
}

#[test]
fn the_cli_is_only_ever_asked_for_status() {
    // Read-only by construction: the one argv this module runs.
    assert_eq!(STATUS_ARGS, &["status", "--json"]);
}

fn ids(g: &[GuidanceStep]) -> Vec<&str> {
    g.iter().map(|s| s.id.as_str()).collect()
}

#[test]
fn guidance_when_everything_is_reachable_is_empty() {
    let r = parse_status(RUNNING);
    let g = guidance(
        &r,
        &Reach {
            lan_peers: 1,
            unreachable: false,
        },
    );
    assert!(g.is_empty());
}

#[test]
fn guidance_suggests_installing_when_peers_are_unreachable_and_tailscale_is_absent() {
    let r = TailscaleReport::not_installed();
    let g = guidance(
        &r,
        &Reach {
            lan_peers: 0,
            unreachable: true,
        },
    );
    assert_eq!(ids(&g), vec!["same-network", "install"]);
    assert!(g[1]
        .url
        .as_deref()
        .is_some_and(|u| u.starts_with("https://tailscale.com/")));
}

#[test]
fn guidance_walks_through_start_and_login() {
    let mut r = parse_status(RUNNING);
    r.state = TsState::NotRunning;
    assert_eq!(
        ids(&guidance(
            &r,
            &Reach {
                lan_peers: 0,
                unreachable: true
            }
        )),
        vec!["same-network", "start"]
    );
    r.state = TsState::Stopped;
    assert_eq!(
        ids(&guidance(
            &r,
            &Reach {
                lan_peers: 0,
                unreachable: true
            }
        )),
        vec!["same-network", "start"]
    );
    r.state = TsState::NeedsLogin;
    assert_eq!(
        ids(&guidance(
            &r,
            &Reach {
                lan_peers: 0,
                unreachable: true
            }
        )),
        vec!["same-network", "login"]
    );
}

#[test]
fn guidance_when_running_points_at_the_tailnet_and_offline_peers() {
    let r = parse_status(RUNNING);
    let g = guidance(
        &r,
        &Reach {
            lan_peers: 0,
            unreachable: true,
        },
    );
    assert_eq!(ids(&g), vec!["same-tailnet", "peer-offline"]);
    assert!(g[1].text.contains("old-laptop"));
}

#[test]
fn guidance_never_tells_the_member_to_change_tailscale_settings_for_them() {
    let mut all = Vec::new();
    for st in [
        TsState::NotInstalled,
        TsState::NotRunning,
        TsState::NeedsLogin,
        TsState::Running,
    ] {
        let mut r = parse_status(RUNNING);
        r.state = st;
        all.extend(guidance(
            &r,
            &Reach {
                lan_peers: 0,
                unreachable: true,
            },
        ));
    }
    for s in &all {
        let t = s.text.to_lowercase();
        assert!(!t.contains("we will"), "{t}");
        assert!(!t.contains("hitl"), "{t}");
        assert!(!s.text.contains('\u{2014}'), "no em-dash: {}", s.text);
    }
}
