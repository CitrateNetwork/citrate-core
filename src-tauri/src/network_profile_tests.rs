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

/// #511: a send record written before entries named a genesis belongs to r1005. That genesis is
/// the `alpha-r1005` profile's, also after the active profile moves to a new genesis.
#[test]
fn the_send_record_legacy_genesis_is_the_r1005_profile() {
    assert_eq!(
        profile("alpha-r1005").unwrap().genesis_hash.as_deref(),
        Some(citrate_core_kit::submitted_log::LEGACY_GENESIS)
    );
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

/// K6: the signing gate is set only from a fully ready profile. The active profile gives a set
/// gate; the next-genesis profile (every value null) would give one that refuses every signature.
#[test]
fn the_signing_gate_is_set_only_from_a_ready_profile() {
    let gate = active_gate();
    assert!(gate.is_set(), "the active profile is ready: {gate:?}");
    let r = active().unwrap().ready().unwrap();
    assert_eq!(gate.chain_id, Some(r.chain_id));
    assert_eq!(gate.genesis_hash.as_deref(), Some(r.genesis_hash.as_str()));
    let next = profile("genesis-2026-12").unwrap();
    assert!(next.ready().is_err());
    assert!(!next.expected_network().is_set());
    assert!(crate::rpc::with_added_network_gate(next.expected_network(), || {
        crate::rpc::check_wallet_signing(crate::rpc::SignatureScope::Unbound)
    })
    .is_err());
}

/// K6: a profile with its chain id and genesis set but anything else not set is not fully set,
/// so its gate refuses every signature.
#[test]
fn a_partly_set_profile_gives_a_gate_that_refuses() {
    let json = PROFILES_JSON.replace(
        "\"addressBook\": \"addresses/40204.json\"",
        "\"addressBook\": null",
    );
    assert_ne!(json, PROFILES_JSON, "the replacement hit the active profile");
    let (active_id, all) = parse(&json).expect("parses");
    let p = all.iter().find(|p| p.id == active_id).expect("active");
    assert!(p.chain_id.is_some() && p.genesis_hash.is_some());
    assert!(!p.expected_network().is_set());
}

/// The `cfg` names a build script's output sets (`cargo:rustc-cfg=…` or `cargo::rustc-cfg=…`),
/// value included (`name` or `name="value"`).
fn k6_emitted_cfgs(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|l| {
            l.strip_prefix("cargo::rustc-cfg=")
                .or_else(|| l.strip_prefix("cargo:rustc-cfg="))
        })
        .map(|c| c.trim().to_string())
        .collect()
}

/// K6 (MAC round 6 on core#363): the key-access scans in kit read `#[cfg(test)]` as test-only.
/// That holds only while no build script sets `cfg(test)` for a non-test build. kit has no build
/// script (pinned in kit). This reads what the app's build script actually emitted (Cargo keeps
/// its stdout beside OUT_DIR, so the answer comes from the build, not from reading the script),
/// and pins the script's source so a conditional emission cannot be added unnoticed.
#[test]
fn k6_app_build_script_sets_no_test_cfg() {
    let sample = concat!(
        "cargo:rerun-if-changed=x\n",
        "cargo:rustc-cfg=desktop\n",
        "cargo::rustc-cfg=test\n",
        "cargo:rustc-cfg=feature=\"x\"\n",
        "cargo:rustc-check-cfg=cfg(test)\n",
        "cargo:rustc-env=A=cargo:rustc-cfg=test\n",
    );
    assert_eq!(
        k6_emitted_cfgs(sample),
        vec!["desktop", "test", "feature=\"x\""],
        "both directive spellings; check-cfg and env lines set no cfg"
    );
    let out = std::path::Path::new(env!("OUT_DIR")).join("../output");
    let emitted = std::fs::read_to_string(&out)
        .unwrap_or_else(|e| panic!("read build script output {}: {e}", out.display()));
    let cfgs = k6_emitted_cfgs(&emitted);
    assert!(
        cfgs.iter().any(|c| c == "desktop"),
        "positive control: tauri_build's `desktop` cfg is seen ({cfgs:?})"
    );
    assert!(
        !cfgs.iter().any(|c| c == "test"),
        "the app build script set cfg(test) for every build: {cfgs:?}"
    );
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
    let src = std::fs::read_to_string(&script).expect("read src-tauri/build.rs");
    let squashed: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(
        squashed, "fnmain(){tauri_build::build()}",
        "src-tauri/build.rs changed; re-check that it sets no cfg(test), then update this pin"
    );
}
