// K6 network switch: the network profiles, their fail-closed "not set" state, and the
// tripwires that keep every other copy of a network value equal to the active profile.

use super::*;

const NEXT: &str = "genesis-2026-12";

fn ready_alpha() -> ReadyNetwork {
    active_ready().expect("the active profile is ready")
}

/// A release build must target a network whose every value is set. Flipping `active` to a
/// profile that is not set yet fails here, in CI, before a build can ship.
#[test]
fn the_active_profile_is_ready() {
    let net = ready_alpha();
    assert_eq!(net.id, active_id().unwrap());
    assert!(!net.bootnodes.is_empty());
}

#[test]
fn the_embedded_profiles_parse_and_the_active_one_exists() {
    let all = profiles().expect("profiles.json parses");
    assert!(all.len() >= 2, "the live network and the next genesis");
    assert!(all.iter().any(|p| p.id == active_id().unwrap()));
    assert!(matches!(
        profile("no-such-network"),
        Err(ProfileError::UnknownProfile(_))
    ));
    assert!(matches!(parse("{"), Err(ProfileError::Malformed(_))));
    // An unknown key is refused, so a typo cannot silently drop a value.
    let typo = r#"{"active":"a","profiles":{"a":{"label":"x","chainId":1,"genesisHas":"0x"}}}"#;
    assert!(matches!(parse(typo), Err(ProfileError::Malformed(_))));
}

/// The next genesis is defined but not set: every value is null with a reason, and nothing about
/// it looks like a real value.
#[test]
fn the_next_genesis_profile_is_explicitly_not_set() {
    let p = profile(NEXT).expect("the next-genesis profile exists");
    let missing = p.missing();
    let names: Vec<&str> = missing.iter().map(|(k, _)| k.as_str()).collect();
    for field in [
        "chainId",
        "genesisHash",
        "nodeNetwork",
        "rpcUrl",
        "explorerUrl",
        "faucetUrl",
        "bootnodes",
        "addressBook",
    ] {
        assert!(
            names.contains(&field),
            "{field} must be unset until decided: {names:?}"
        );
    }
    for (field, why) in &missing {
        assert!(
            why != "not recorded" && !why.is_empty(),
            "{field} is unset with no reason recorded in `pending`"
        );
    }
    match p.ready() {
        Err(ProfileError::NotSet { profile, missing }) => {
            assert_eq!(profile, NEXT);
            assert_eq!(missing.len(), 8);
        }
        other => panic!("a not-set profile must not be ready: {other:?}"),
    }
    let gate = p.expected_network();
    assert_eq!(gate.chain_id, None);
    assert_eq!(gate.genesis_hash, None);
}

/// Every profile: a null value has a reason, and a reason is only given for a null value (a
/// stale reason next to a real value would mislead).
#[test]
fn pending_reasons_match_exactly_the_unset_values() {
    for p in profiles().unwrap() {
        let unset: Vec<String> = p.missing().into_iter().map(|(k, _)| k).collect();
        let reasons: Vec<String> = p.pending.keys().cloned().collect();
        let mut a = unset.clone();
        a.sort();
        let mut b = reasons.clone();
        b.sort();
        assert_eq!(
            a, b,
            "profile {}: unset {unset:?} vs reasons {reasons:?}",
            p.id
        );
    }
}

#[test]
fn ready_refuses_values_that_only_look_set() {
    let base = active().unwrap().clone();
    type Mutate = Box<dyn Fn(&mut NetworkProfile)>;
    let cases: Vec<(&str, Mutate)> = vec![
        ("chainId", Box::new(|p| p.chain_id = Some(0))),
        (
            "genesisHash",
            Box::new(|p| p.genesis_hash = Some(format!("0x{}", "0".repeat(64)))),
        ),
        (
            "genesisHash",
            Box::new(|p| p.genesis_hash = Some("0x1234".into())),
        ),
        (
            "genesisHash",
            Box::new(|p| p.genesis_hash = Some(String::new())),
        ),
        (
            "nodeNetwork",
            Box::new(|p| p.node_network = Some(String::new())),
        ),
        (
            "nodeNetwork",
            Box::new(|p| p.node_network = Some("test net".into())),
        ),
        (
            "rpcUrl",
            Box::new(|p| p.rpc_url = Some("http://rpc.citrate.ai".into())),
        ),
        ("rpcUrl", Box::new(|p| p.rpc_url = Some("https://".into()))),
        (
            "explorerUrl",
            Box::new(|p| p.explorer_url = Some(String::new())),
        ),
        (
            "faucetUrl",
            Box::new(|p| p.faucet_url = Some("ftp://x".into())),
        ),
        ("bootnodes", Box::new(|p| p.bootnodes = Some(Vec::new()))),
        (
            "bootnodes",
            Box::new(|p| p.bootnodes = Some(vec!["boot1.citrate.ai:30303".into()])),
        ),
        (
            "bootnodes",
            Box::new(|p| p.bootnodes = Some(vec![format!("noise_{}@h\"x:30303", "a".repeat(64))])),
        ),
        (
            "addressBook",
            Box::new(|p| p.address_book = Some(String::new())),
        ),
    ];
    for (field, mutate) in cases {
        let mut p = base.clone();
        mutate(&mut p);
        match p.ready() {
            Err(ProfileError::Invalid { field: f, .. }) => assert_eq!(f, field),
            other => panic!("{field}: expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn bootnode_form_is_checked() {
    let key = "ab".repeat(32);
    assert!(is_bootnode(&format!("noise_{key}@boot1.citrate.ai:30303")));
    assert!(!is_bootnode(&format!("noise_{key}@boot1.citrate.ai:0")));
    assert!(!is_bootnode(&format!("noise_{key}@:30303")));
    assert!(!is_bootnode(&format!("noise_{}@h:30303", "ab".repeat(31))));
    assert!(!is_bootnode(&format!("{key}@h:30303")));
    assert!(!is_bootnode(&format!("noise_{key}@h")));
}

#[test]
fn the_gate_for_the_active_profile_carries_its_chain_id_and_genesis() {
    let net = ready_alpha();
    let gate = active().unwrap().expected_network();
    assert_eq!(gate.chain_id, Some(net.chain_id));
    assert_eq!(
        gate.genesis_hash.as_deref(),
        Some(net.genesis_hash.as_str())
    );
    assert_eq!(gate.name, net.id);
}

#[test]
fn not_set_error_names_the_missing_values() {
    let e = profile(NEXT).unwrap().ready().unwrap_err().to_string();
    assert!(
        e.contains(NEXT) && e.contains("genesisHash") && e.contains("bootnodes"),
        "{e}"
    );
    assert!(e.contains("nothing is synced or signed"), "{e}");
}

// ---- rendering the member node config ----

#[test]
fn the_template_has_no_network_values_of_its_own() {
    let t = include_str!("../config/member-node.toml");
    assert!(t.contains(CHAIN_ID_PLACEHOLDER), "chain id placeholder");
    assert!(
        t.contains(BOOTNODES_PLACEHOLDER),
        "bootnode placeholder line"
    );
    assert_eq!(
        t.matches("noise_").count(),
        0,
        "no bootnode outside the profile"
    );
    assert!(!t.contains("40204"), "no chain id outside the profile");
    // The template stays valid TOML, so tools that read ports from it keep working.
    t.parse::<toml::Table>().expect("template is valid TOML");
}

#[test]
fn rendering_fills_every_network_value() {
    let net = ready_alpha();
    let out = render_node_network(include_str!("../config/member-node.toml"), &net);
    assert!(!out.contains("{{CHAIN_ID}}") && !out.contains("{{BOOTSTRAP_NODES}}"));
    let t: toml::Table = out
        .replace("{{DATA_DIR}}", "/tmp/x")
        .parse()
        .expect("rendered config is valid TOML");
    assert_eq!(
        t["chain"]["chain_id"].as_integer(),
        Some(i64::try_from(net.chain_id).unwrap())
    );
    let nodes: Vec<&str> = t["network"]["bootstrap_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        nodes,
        net.bootnodes.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

// ---- tripwires: every other copy equals the active profile ----

/// The address book the build embeds is the one the profile names, for the same chain and
/// genesis. A new genesis means a new book AND a new profile; they cannot drift apart.
#[test]
fn the_address_book_matches_the_active_profile() {
    let net = ready_alpha();
    assert_eq!(net.address_book, "addresses/40204.json");
    assert!(
        include_str!("addresses.rs")
            .contains(&format!("include_str!(\"../{}\")", net.address_book)),
        "addresses.rs must embed the book the active profile names"
    );
    assert_eq!(crate::addresses::chain_id(), net.chain_id);
    assert_eq!(crate::addresses::genesis_hash(), net.genesis_hash);
}

#[test]
fn the_kit_rpc_constants_match_the_active_profile() {
    let net = ready_alpha();
    assert_eq!(crate::rpc::CITRATE_RPC_URL, net.rpc_url);
    assert_eq!(crate::rpc::CITRATE_CHAIN_ID, net.chain_id);
}

#[test]
fn the_explorer_and_faucet_urls_match_the_active_profile() {
    let net = ready_alpha();
    assert_eq!(crate::activity::EXPLORER_BASE, net.explorer_url);
    assert_eq!(crate::faucet::DEFAULT_FAUCET_URL, net.faucet_url);
}

/// The withdrawn-branch checks are per genesis; each must belong to a profile this build knows.
#[test]
fn every_withdrawn_branch_check_belongs_to_a_known_profile() {
    let known: Vec<String> = profiles()
        .unwrap()
        .iter()
        .filter_map(|p| p.genesis_hash.clone())
        .collect();
    for c in crate::node_withdrawn::FORK_CHECKS {
        assert!(known.iter().any(|g| g == c.genesis), "{}", c.label);
    }
}

/// The webview reads the same file (src/network/profile.ts), not a copy.
#[test]
fn the_webview_reads_the_same_profiles_file() {
    let ts = include_str!("../../src/network/profile.ts");
    assert!(
        ts.contains("src-tauri/network/profiles.json"),
        "profile.ts imports profiles.json"
    );
}
