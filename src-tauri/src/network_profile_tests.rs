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
        "genesisProfile",
        "nodeNetwork",
        "rpcUrl",
        "explorerUrl",
        "faucetUrl",
        "bootnodes",
        "addressBook",
        "addressBookSha256",
        "minChainRev",
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
            assert_eq!(missing.len(), 11);
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
        (
            "addressBookSha256",
            Box::new(|p| p.address_book_sha256 = Some("ab".repeat(31))),
        ),
        (
            "addressBookSha256",
            Box::new(|p| p.address_book_sha256 = Some("AB".repeat(32))),
        ),
        (
            "addressBookSha256",
            Box::new(|p| p.address_book_sha256 = Some("0".repeat(64))),
        ),
        (
            "minChainRev",
            Box::new(|p| p.min_chain_rev = Some("c384fc98".into())),
        ),
        (
            "minChainRev",
            Box::new(|p| p.min_chain_rev = Some("0".repeat(40))),
        ),
        (
            "minChainRev",
            Box::new(|p| p.min_chain_rev = Some("g".repeat(40))),
        ),
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some("r9999".into())),
        ),
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some(String::new())),
        ),
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some("R1006".into())),
        ),
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some("r1006\"\nx = 1".into())),
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
        t.contains(&format!("genesis_profile = \"{GENESIS_PROFILE_PLACEHOLDER}\"")),
        "genesis profile placeholder"
    );
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

// ---- r1006 (RC52 D-2): the rerolled testnet the 0.5.2 RC targets ----

const R1006: &str = "r1006";
const FIXTURE_GENESIS: &str = "0x6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a";

fn r1006() -> &'static NetworkProfile {
    profile(R1006).expect("the r1006 profile exists")
}

/// r1006 with the reroll values filled by test fixtures (what `scripts/fill-network-profile.sh`
/// writes after the reroll).
fn r1006_filled() -> NetworkProfile {
    let mut p = r1006().clone();
    p.genesis_hash = Some(FIXTURE_GENESIS.into());
    p.address_book_sha256 = Some("5c".repeat(32));
    p.min_chain_rev = Some("1f".repeat(20));
    p.pending.clear();
    p
}

/// r1006 as shipped before the reroll fill: the three reroll values unset, each with a reason.
fn r1006_unfilled() -> NetworkProfile {
    let mut p = r1006().clone();
    p.genesis_hash = None;
    p.address_book_sha256 = None;
    p.min_chain_rev = None;
    for k in ["genesisHash", "addressBookSha256", "minChainRev"] {
        p.pending.insert(k.into(), "set at the reroll".into());
    }
    p
}

fn signs(gate: &crate::rpc::ExpectedNetwork, scope: crate::rpc::SignatureScope) -> bool {
    crate::rpc::with_added_network_gate(gate.clone(), || {
        crate::rpc::check_wallet_signing(scope)
    })
    .is_ok()
}

/// Every value known before the reroll is set: chain id 40204 (the reroll keeps it), the
/// existing fleet's RPC, explorer and faucet, and its bootnodes (citrate-chain
/// node/config/testnet-beta.toml; the Noise keys stay stable across a reroll; owner decision:
/// boot1-3 + rpc.citrate.ai). Since the RC52 fill the reroll values are set too (next test).
#[test]
fn r1006_has_the_values_known_today_and_only_the_reroll_values_unset() {
    let p = r1006_unfilled();
    assert_eq!(p.chain_id, Some(40204));
    assert_eq!(p.node_network.as_deref(), Some("testnet"));
    assert_eq!(p.rpc_url.as_deref(), Some("https://rpc.citrate.ai"));
    assert_eq!(p.explorer_url.as_deref(), Some("https://explorer.citrate.ai"));
    assert_eq!(p.faucet_url.as_deref(), Some("https://faucet.citrate.ai"));
    assert_eq!(p.address_book.as_deref(), Some("addresses/40204.json"));
    let nodes = p.bootnodes.clone().expect("bootnodes set");
    assert_eq!(nodes.len(), 4);
    assert!(nodes.iter().all(|b| is_bootnode(b)), "{nodes:?}");
    let missing: Vec<String> = p.missing().into_iter().map(|(k, _)| k).collect();
    assert_eq!(missing, ["genesisHash", "addressBookSha256", "minChainRev"]);
}

/// RC52 fill (scripts/fill-network-profile.sh): r1006 carries the reroll values and is the network
/// this build targets. Genesis: DGX fleet_agree + G4 cold-sync (fed#331), = citrate-chain
/// `R1006_GENESIS`; book: citrate-chain contracts/addresses/r1006/40204.json at a857443e; rev: the
/// node the fleet runs.
#[test]
fn r1006_is_filled_with_the_reroll_values_and_active() {
    let p = r1006();
    assert_eq!(p.genesis_hash.as_deref(), Some(R1006_GENESIS));
    assert_eq!(
        p.address_book_sha256.as_deref(),
        Some("724e3843f677588a6aac680554dd1f2db768d5c3ca2bea18bdc0c173c100b90b")
    );
    assert_eq!(
        p.min_chain_rev.as_deref(),
        Some("9a32147e2aac60d388356ecc67488d717aa84c41")
    );
    assert!(p.pending.is_empty(), "{:?}", p.pending);
    assert_eq!(active_id().unwrap(), R1006);
    let r = active_ready().expect("the active r1006 is ready");
    assert_eq!(r.genesis_hash, R1006_GENESIS);
    assert_eq!(r.node_genesis.name, "r1006");
    assert_eq!(crate::addresses::genesis_hash(), R1006_GENESIS);
}

/// Red first (K6, RC52): until the reroll values are filled, r1006 refuses every wallet
/// signature (unbound and on its own chain) and every RPC read.
#[test]
fn r1006_unfilled_refuses_every_signature() {
    let p = r1006_unfilled();
    match p.ready() {
        Err(ProfileError::NotSet { profile, missing }) => {
            assert_eq!(profile, R1006);
            assert_eq!(missing.len(), 3, "{missing:?}");
        }
        other => panic!("unfilled r1006 must not be ready: {other:?}"),
    }
    let gate = p.expected_network();
    assert!(!gate.is_set(), "{gate:?}");
    assert!(!signs(&gate, crate::rpc::SignatureScope::Unbound));
    assert!(!signs(&gate, crate::rpc::SignatureScope::Chain(40204)));
    assert!(crate::rpc::with_added_network_gate(gate, crate::rpc::check_rpc_allowed).is_err());
}

/// Each reroll value on its own keeps r1006 refusing: the genesis hash alone is not enough.
#[test]
fn r1006_refuses_while_any_one_reroll_value_is_unset() {
    type Unset = fn(&mut NetworkProfile);
    let cases: [(&str, Unset); 3] = [
        ("genesisHash", |p| p.genesis_hash = None),
        ("addressBookSha256", |p| p.address_book_sha256 = None),
        ("minChainRev", |p| p.min_chain_rev = None),
    ];
    for (field, unset) in cases {
        let mut p = r1006_filled();
        unset(&mut p);
        match p.ready() {
            Err(ProfileError::NotSet { missing, .. }) => {
                assert_eq!(missing.len(), 1, "{field}: {missing:?}");
                assert_eq!(missing[0].0, field);
            }
            other => panic!("{field} unset: expected NotSet, got {other:?}"),
        }
        let gate = p.expected_network();
        assert!(!signs(&gate, crate::rpc::SignatureScope::Chain(40204)), "{field}");
    }
}

/// Filled with fixture values, r1006 is ready and its gate signs on chain 40204 only.
#[test]
fn r1006_filled_with_fixture_values_signs_on_its_chain_only() {
    let p = r1006_filled();
    let r = p.ready().expect("filled r1006 is ready");
    assert_eq!(r.genesis_hash, FIXTURE_GENESIS);
    assert_eq!(r.min_chain_rev, "1f".repeat(20));
    let gate = p.expected_network();
    assert!(gate.is_set());
    assert_eq!(gate.chain_id, Some(40204));
    assert_eq!(gate.genesis_hash.as_deref(), Some(FIXTURE_GENESIS));
    assert!(signs(&gate, crate::rpc::SignatureScope::Unbound));
    assert!(signs(&gate, crate::rpc::SignatureScope::Chain(40204)));
    assert!(!signs(&gate, crate::rpc::SignatureScope::Chain(1)));
}

/// The reroll is a new genesis: r1006 must never reuse r1005's block-0 hash.
#[test]
fn r1006_is_a_different_network_from_r1005() {
    let alpha = profile("alpha-r1005").unwrap();
    assert_ne!(r1006().genesis_hash, alpha.genesis_hash);
    assert_ne!(Some(FIXTURE_GENESIS.to_string()), alpha.genesis_hash);
}

/// The bundled node must contain the active network's minimum chain rev: build-sidecar.sh pins
/// the same commit (re-pinned together after the reroll).
#[test]
fn the_sidecar_min_chain_rev_is_the_active_profiles() {
    let net = ready_alpha();
    assert_eq!(net.min_chain_rev.len(), 40);
    let sidecar = include_str!("../../scripts/build-sidecar.sh");
    assert!(
        sidecar.contains(&format!("MIN_CHAIN_REV=\"{}\"", net.min_chain_rev)),
        "scripts/build-sidecar.sh MIN_CHAIN_REV must equal the active profile's minChainRev {}",
        net.min_chain_rev
    );
}

// ---- the genesis the bundled node builds (RC52: the 0.5.2 RC runs its node on r1006) ----

/// Block 0 of r1006 (citrate-chain 9a32147e `R1006_GENESIS`; DGX G4 cold-sync, fed#331).
const R1006_GENESIS: &str = "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4";

/// The `[chain] genesis_profile` of the member node config rendered for `net` (None: not set).
fn rendered_genesis_profile(net: &ReadyNetwork) -> Option<String> {
    let out = render_node_network(include_str!("../config/member-node.toml"), net)
        .replace("{{DATA_DIR}}", "/tmp/x");
    let t: toml::Table = out.parse().expect("rendered config is valid TOML");
    t["chain"]
        .get("genesis_profile")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The embedded profiles file with `edit` applied to its parsed JSON.
fn profiles_json_with(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut v: serde_json::Value = serde_json::from_str(PROFILES_JSON).expect("profiles.json");
    edit(&mut v);
    v.to_string()
}

/// Red first (RC52): without `[chain] genesis_profile` the node builds the chain-id default
/// genesis, which on 40204 is r1005's (0x1dcfc490…), not r1006's. The node config the app
/// writes must name the genesis profile of the network it targets.
#[test]
fn the_node_config_names_the_genesis_profile_of_its_network() {
    let r1006 = r1006_filled()
        .ready()
        .expect("filled r1006 is ready");
    assert_eq!(
        rendered_genesis_profile(&r1006).as_deref(),
        Some("r1006"),
        "an r1006 node must be told to build the r1006 genesis"
    );
    let alpha = profile("alpha-r1005")
        .unwrap()
        .ready()
        .expect("alpha-r1005 is ready");
    assert_eq!(
        rendered_genesis_profile(&alpha).as_deref(),
        Some("testnet_beta"),
        "the r1005 network names the genesis profile its block 0 comes from"
    );
}

/// Red first (RC52), fail closed: a profile naming a genesis profile the bundled node does not
/// build (a typo, or a network whose genesis is not in this node) is not ready, so no node starts
/// and nothing is signed. An unset one is "not set". A known one is ready.
#[test]
fn a_genesis_profile_the_bundled_node_does_not_build_is_refused() {
    let fill = |v: &mut serde_json::Value, gp: serde_json::Value| {
        let p = &mut v["profiles"]["r1006"];
        p["genesisHash"] = R1006_GENESIS.into();
        p["addressBookSha256"] = "5c".repeat(32).into();
        p["minChainRev"] = "9a32147e2aac60d388356ecc67488d717aa84c41".into();
        p["genesisProfile"] = gp;
        p["pending"] = serde_json::json!({});
    };
    let r1006_of = |json: String| -> Result<ReadyNetwork, ProfileError> {
        let (_, ps) = parse(&json)?;
        ps.into_iter()
            .find(|p| p.id == "r1006")
            .ok_or_else(|| ProfileError::UnknownProfile("r1006".into()))?
            .ready()
    };
    let unknown = r1006_of(profiles_json_with(|v| fill(v, "r9999".into())));
    assert!(
        matches!(&unknown, Err(ProfileError::Invalid { field: "genesisProfile", .. })),
        "a genesis profile the node does not build must be refused: {unknown:?}"
    );
    let unset = r1006_of(profiles_json_with(|v| {
        fill(v, serde_json::Value::Null);
        v["profiles"]["r1006"]["pending"] =
            serde_json::json!({"genesisProfile": "set at the reroll"});
    }));
    assert!(
        matches!(&unset, Err(ProfileError::NotSet { missing, .. })
            if missing.iter().any(|(k, _)| k == "genesisProfile")),
        "an unset genesis profile is not set: {unset:?}"
    );
    let known = r1006_of(profiles_json_with(|v| fill(v, "r1006".into())));
    assert!(
        matches!(&known, Ok(r) if r.genesis_hash == R1006_GENESIS),
        "r1006 naming the r1006 genesis profile is ready: {known:?}"
    );
}

/// Tripwire (RC52): every shipped profile whose genesis is set names the genesis profile that
/// builds exactly that block 0, so the node can never be told to build another network's genesis
/// (alpha-r1005 → testnet_beta 0x1dcfc490…, r1006 → r1006 0xa3651a27… once filled).
#[test]
fn every_shipped_genesis_is_the_block_0_its_genesis_profile_builds() {
    let table = node_genesis_profiles().expect("nodeGenesisProfiles parses");
    assert!(!table.is_empty());
    for g in table {
        assert!(is_genesis_profile_name(&g.name), "{g:?}");
    }
    let mut checked = 0;
    for p in profiles().unwrap() {
        let (Some(gh), Some(gp)) = (&p.genesis_hash, &p.genesis_profile) else {
            continue;
        };
        let g = node_genesis(gp)
            .unwrap_or_else(|| panic!("profile {}: genesis profile {gp:?} is not in the table", p.id));
        assert_eq!(
            gh.to_ascii_lowercase(),
            g.genesis_hash,
            "profile {}: its genesis is not the block 0 genesis profile {gp} builds",
            p.id
        );
        checked += 1;
    }
    assert!(checked >= 1, "the active profile at least");
    let built = node_genesis("r1006").expect("the bundled node builds r1006");
    assert_eq!(built.genesis_hash, R1006_GENESIS);
    assert!(built.compiled_activations && !built.node_default);
    assert_eq!(r1006().genesis_profile.as_deref(), Some("r1006"));
}

/// The node genesis table is checked when it is read: a bad name or hash is a build defect.
#[test]
fn a_malformed_node_genesis_table_is_refused() {
    for (what, edit) in [
        ("a name the TOML string cannot carry as is", r#"{"r1006\"x": {"genesisHash": "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4", "nodeDefault": false, "compiledActivations": true}}"#),
        ("a hash that is not a block hash", r#"{"r1006": {"genesisHash": "0x1234", "nodeDefault": false, "compiledActivations": true}}"#),
        ("an unknown key", r#"{"r1006": {"genesisHash": "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4", "nodeDefault": false, "compiledActivations": true, "x": 1}}"#),
    ] {
        let json = profiles_json_with(|v| {
            v["nodeGenesisProfiles"] = serde_json::from_str(edit).expect("edit is JSON");
        });
        assert!(
            matches!(parse_node_genesis(&json), Err(ProfileError::Malformed(_))),
            "{what}"
        );
    }
    assert!(parse_node_genesis(PROFILES_JSON).is_ok());
}
