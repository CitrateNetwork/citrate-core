//! 0.5.2 on r1006 -> 0.5.3 on r1007 (CORE-N3, the RC52 G2 cross-genesis check with r1007).
//!
//! A 0.5.2 install whose node data belongs to r1006 (fixtures/upgrade-v0.5.2-r1006 over the 0.5.1
//! fixtures, see its README) is started by this build through `NodeManager::start`, the path the
//! app takes, on the real `r1007` profile filled with test-only values (the r1007 cut fills it for
//! real), so the start goes through the same `ready()` checks as a release profile.
//!
//! What must hold:
//! - the r1006 chain database leaves the node dir but is NOT deleted: it is set aside, byte for
//!   byte, in `node/chain.r1006.bak/`, named for the profile it came from;
//! - the previous `node.toml` (with the member's edit) is set aside, a new one is written from the
//!   r1007 profile (its bootnodes, its genesis profile, its pruned chain store);
//! - the wallet vault, keychain slots, IPFS pins, Store models, daemons, Hermes settings, invites
//!   and node keys are byte-identical afterwards, and the wallet opens with the same address;
//! - nothing is dropped silently: the member is told the chain was reset, where the previous node
//!   settings went and where the previous chain data went;
//! - while r1007 is not set in a build (today), the r1006 install is not touched at all.

use crate::upgrade_v051_tests::{copy_dir, read};
use crate::upgrade_xgenesis_tests::{
    fixture_root, manager, names, scratch, start_for_new_network, tree, MapKeyring,
};
use citrate_core_kit::custody::CustodyVault;
use std::path::{Path, PathBuf};

/// Block 0 of r1006 (`r1006` in network/profiles.json).
const R1006: &str = "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4";
/// Test-only block 0 for r1007. Not a real block hash.
const T_R1007_GENESIS: &str = "0x7e57000000000000000000000000000000000000000000000000000000001007";
/// Test-only r1007 bootnodes on the real hosts (keys are test-only).
const T_R1007_BOOTNODES: [&str; 4] = [
    "noise_b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1@boot1.citrate.ai:30303",
    "noise_b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2@boot2.citrate.ai:30303",
    "noise_b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3@boot3.citrate.ai:30303",
    "noise_c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4@rpc.citrate.ai:30303",
];
/// Where the r1006 chain data goes, and where the r1006 node.toml goes (first 10 characters).
const CHAIN_ASIDE: &str = "chain.r1006.bak";
const CONFIG_ASIDE: &str = "node.toml.0xa3651a27";

fn v052_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/upgrade-v0.5.2-r1006")
}

/// The 0.5.2 app data dir with its node dir on r1006.
fn install_v052_on_r1006(root: &Path) -> PathBuf {
    let app = root.join("app");
    copy_dir(&fixture_root().join("app-data"), &app);
    std::fs::remove_dir_all(app.join("node")).unwrap_or_else(|e| panic!("rm node: {e}"));
    copy_dir(&fixture_root().join("node-r1005"), &app.join("node"));
    for f in ["node.toml", "chain-genesis"] {
        std::fs::copy(
            v052_fixture().join("node").join(f),
            app.join("node").join(f),
        )
        .unwrap_or_else(|e| panic!("{f}: {e}"));
    }
    app
}

/// The real r1007 profile with its pending values filled by test-only values. It uses the
/// r1006 genesis profile because the bundled node's table has no r1007 entry before the cut;
/// what is checked here is the app's handling of the data dir, not the node's block 0.
fn r1007_filled() -> crate::network_profile::ReadyNetwork {
    let mut p = crate::network_profile::profile("r1007")
        .unwrap_or_else(|e| panic!("r1007 profile: {e:?}"))
        .clone();
    p.genesis_hash = Some(T_R1007_GENESIS.to_string());
    p.genesis_profile = Some("r1006".to_string());
    p.bootnodes = Some(T_R1007_BOOTNODES.iter().map(|s| s.to_string()).collect());
    p.address_book_sha256 = Some("7b".repeat(32));
    p.min_chain_rev = Some("7a".repeat(20));
    assert_eq!(
        p.missing(),
        Vec::<(String, String)>::new(),
        "the test fill leaves an r1007 value unset"
    );
    p.pending.clear();
    p.ready()
        .unwrap_or_else(|e| panic!("the filled r1007 profile must be ready: {e:?}"))
}

/// Files outside the node dir that the switch must not touch (wallet, pins, Store, Hermes, ...).
fn kept_app_files(app: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for n in names(dir) {
            let p = dir.join(&n);
            let rel = p
                .strip_prefix(base)
                .unwrap_or_else(|e| panic!("{e}"))
                .to_string_lossy()
                .replace('\\', "/");
            if rel == "node" {
                continue;
            }
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.push((rel, read(&p)));
            }
        }
    }
    let mut out = Vec::new();
    walk(app, app, &mut out);
    out
}

#[test]
fn a_v052_install_on_r1006_moves_to_r1007_with_its_chain_data_set_aside_and_nothing_lost() {
    let root = scratch("r1007-switch");
    let app = install_v052_on_r1006(&root);
    let node = app.join("node");
    let fixture_node = fixture_root().join("node-r1005");
    let custody_kr = MapKeyring::from_fixture("custody.keyring.json");
    let node_kr = MapKeyring::from_fixture("node-r1005.keyring.json");
    let custody_before = custody_kr.snapshot();
    let node_key_before = node_kr.snapshot();
    let app_before = kept_app_files(&app);
    assert!(
        app_before.iter().any(|(r, _)| r == "ipfs/pins.json")
            && app_before
                .iter()
                .any(|(r, _)| r == "hermes/terminal-settings.json")
            && app_before.iter().any(|(r, _)| r.starts_with("models/"))
            && app_before.iter().any(|(r, _)| r == "custody.enc"),
        "the fixture holds the wallet, pins, Store models and Hermes settings"
    );
    assert_eq!(
        crate::node_genesis::read_marker_record(&node)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .map(|m| m.genesis),
        Some(R1006.to_string()),
        "a 0.5.2 install on r1006"
    );

    let mgr = manager(&root, &app, &node_kr, Ok(r1007_filled()));
    start_for_new_network(&mgr);

    // Chain database: out of the node dir, kept byte for byte in chain.r1006.bak.
    let db: Vec<String> = names(&fixture_node)
        .into_iter()
        .filter(|n| crate::node_genesis::is_chain_db_entry(n))
        .collect();
    assert_eq!(db.len(), 9, "the fixture holds a full RocksDB file set");
    for f in &db {
        assert!(
            !node.join(f).exists(),
            "r1006 chain file {f} left in the node dir"
        );
        assert_eq!(
            read(&node.join(CHAIN_ASIDE).join(f)),
            read(&fixture_node.join(f)),
            "r1006 chain file {f} is kept in {CHAIN_ASIDE}"
        );
    }
    // The marker commits r1007; the intent is gone.
    let marker = crate::node_genesis::read_marker_record(&node)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .unwrap_or_else(|| panic!("no marker"));
    assert_eq!(marker.genesis, T_R1007_GENESIS);
    assert!(marker.nonce.is_some());
    assert!(crate::node_genesis::read_intent(&node)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .is_none());
    // node.toml: the 0.5.2 one (with the member edit) set aside, the new one from r1007.
    assert_eq!(
        read(&node.join(CONFIG_ASIDE)),
        read(&v052_fixture().join("node/node.toml"))
    );
    let toml = String::from_utf8(read(&node.join("node.toml"))).unwrap_or_else(|e| panic!("{e}"));
    for b in T_R1007_BOOTNODES {
        assert!(toml.contains(b), "r1007 bootnode {b} in node.toml");
    }
    for b in crate::network_profile::profile("r1006")
        .unwrap_or_else(|e| panic!("{e:?}"))
        .bootnodes
        .clone()
        .unwrap_or_default()
    {
        assert!(
            !toml.contains(b.as_str()),
            "r1006 bootnode {b} left in node.toml"
        );
    }
    let t: toml::Table = toml.parse().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t["storage"]["pruning"].as_bool(), Some(true), "CORE-N5");
    assert_eq!(t["storage"]["keep_blocks"].as_integer(), Some(10_000));
    // Node keys kept.
    for f in [
        "proposer.key",
        "noise.key",
        "peer.id",
        "encryption.meta",
        "crash-records.jsonl",
    ] {
        assert_eq!(
            read(&node.join(f)),
            read(&fixture_node.join(f)),
            "{f} is kept"
        );
    }
    // Everything outside the node dir is byte-identical; the keychain is unchanged.
    assert_eq!(
        kept_app_files(&app),
        app_before,
        "wallet, pins, Store, Hermes, invites, daemons"
    );
    assert_eq!(custody_kr.snapshot(), custody_before);
    assert_eq!(node_kr.snapshot(), node_key_before);
    let meta: serde_json::Value =
        serde_json::from_slice(&read(&fixture_root().join("custody.keyring.json")))
            .unwrap_or_else(|e| panic!("{e}"));
    let vault = CustodyVault::new(Box::new(custody_kr.clone()), app.join("custody.enc"), 30);
    vault
        .ensure_auto_unlocked()
        .unwrap_or_else(|e| panic!("the 0.5.2 vault must open after the switch: {e}"));
    let info = citrate_core_kit::wallet::address(&vault).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(Some(info.address.as_str()), meta["walletAddress"].as_str());
    assert!(
        !crate::hermes_terminal::enabled(&app.join("hermes")),
        "Hermes setting kept"
    );

    // The member is told, with both places named.
    let notice = mgr
        .status()
        .notice
        .unwrap_or_else(|| panic!("the switch must be reported"));
    assert!(
        notice.starts_with(crate::node_genesis::CHAIN_RESET_NOTICE),
        "{notice}"
    );
    for p in [node.join(CONFIG_ASIDE), node.join(CHAIN_ASIDE)] {
        let p = p.to_string_lossy().to_string();
        assert!(notice.contains(&p), "the notice names {p}: {notice}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn while_r1007_is_not_set_the_r1006_install_is_not_touched() {
    let root = scratch("r1007-unset");
    let app = install_v052_on_r1006(&root);
    let node_kr = MapKeyring::from_fixture("node-r1005.keyring.json");
    let unset = crate::network_profile::profile("r1007")
        .unwrap_or_else(|e| panic!("{e:?}"))
        .ready();
    assert!(unset.is_err(), "r1007 is not set in this build");
    let before = tree(&app.join("node"));
    let mgr = manager(&root, &app, &node_kr, unset);
    assert!(matches!(
        mgr.start(),
        Err(crate::node::NodeError::Network(_))
    ));
    let after = tree(&app.join("node"));
    assert_eq!(after, before, "an unset r1007 touches nothing");
    assert!(mgr.status().notice.is_none());
    let _ = std::fs::remove_dir_all(&root);
}
