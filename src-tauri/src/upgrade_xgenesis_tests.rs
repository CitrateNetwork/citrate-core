//! 0.5.1 on r1005 -> 0.5.2 on a new genesis (RC52 G2, K6 network switch).
//!
//! The same-genesis upgrade is covered by `upgrade_v051_tests`. Here a 0.5.1 install whose node
//! data belongs to the r1005 testnet (fixtures/upgrade-v0.5.1/node-r1005) is started by this build
//! targeting a different genesis, through `NodeManager::start`, the path the app takes. The new
//! profile is the real `genesis-2026-12` entry filled with test-only values (K6f fills it for
//! real), so it goes through the same `ready()` checks as a release profile.
//!
//! What must hold (UPGRADE_051_052 §2, K6_NETWORK_SWITCH §3.3, core#363 M1-M4):
//! - the r1005 chain database leaves the node dir (deleted today; archived once D-K6-1 is
//!   decided), the previous `node.toml` is set aside byte for byte, a new one is written from the
//!   profile, and the marker names the new genesis with the committing migration's nonce;
//! - the wallet vault, keychain slots, invites, pins, Store/models, daemons, Hermes settings and
//!   node keys are byte-identical afterwards, and the submitted-tx record reads as uncertain;
//! - nothing is dropped silently: the member is told the chain was reset and where the previous
//!   node settings were kept, also when the switch finishes on a later launch.
//!
//! The post-upgrade app data dir and keychain are compared with
//! fixtures/upgrade-v0.5.1/golden/xgenesis-post-upgrade.manifest.

use crate::upgrade_v051_tests::{copy_dir, read};
use base64::Engine;
use citrate_core_kit::custody::{CustodyError, CustodyVault, Keyring};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The r1005 genesis every 0.5.1 node marker names (`alpha-r1005` in network/profiles.json).
const R1005: &str = "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1";
/// Test-only genesis for the new network. Not a real block hash.
const NEW_GENESIS: &str = "0x2026120100000000000000000000000000000000000000000000000000000c52";
/// Test-only bootnodes for the new network (`.invalid` never resolves).
const NEW_BOOTNODES: [&str; 2] = [
    "noise_a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1@boot1.rehearsal.invalid:30303",
    "noise_b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2@seq.rehearsal.invalid:30303",
];
/// Where a switch from r1005 sets the previous `node.toml` aside (first 10 characters).
const ASIDE: &str = "node.toml.0x1dcfc490";

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/upgrade-v0.5.1")
}

fn golden_path() -> PathBuf {
    fixture_root().join("golden/xgenesis-post-upgrade.manifest")
}

/// A scratch root for one test: `app/` is the app data dir, `rt/` the key runtime dir.
fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "citrate-upgrade-xgenesis-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("rt")).unwrap_or_else(|e| panic!("mkdir: {e}"));
    d
}

/// The 0.5.1 app data dir with its node dir on r1005.
fn install_v051_on_r1005(root: &Path) -> PathBuf {
    let app = root.join("app");
    copy_dir(&fixture_root().join("app-data"), &app);
    std::fs::remove_dir_all(app.join("node")).unwrap_or_else(|e| panic!("rm node: {e}"));
    copy_dir(&fixture_root().join("node-r1005"), &app.join("node"));
    app
}

#[derive(Clone, Default)]
struct MapKeyring(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);
impl MapKeyring {
    fn from_fixture(name: &str) -> Self {
        let m: serde_json::Value = serde_json::from_slice(&read(&fixture_root().join(name)))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let kr = MapKeyring::default();
        for (account, v) in m["keyring"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: no keyring entries"))
        {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(v.as_str().unwrap_or_default())
                .unwrap_or_else(|e| panic!("{account}: {e}"));
            kr.0.lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(account.clone(), bytes);
        }
        kr
    }
    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}
impl Keyring for MapKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(account)
            .cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), CustodyError> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), CustodyError> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(account);
        Ok(())
    }
}

/// The real `genesis-2026-12` profile with every pending value filled by test-only values.
fn new_network() -> crate::network_profile::ReadyNetwork {
    let mut p = crate::network_profile::profile("genesis-2026-12")
        .unwrap_or_else(|e| panic!("genesis-2026-12 profile: {e:?}"))
        .clone();
    p.chain_id = Some(40204);
    p.genesis_hash = Some(NEW_GENESIS.to_string());
    p.node_network = Some("rehearsal".to_string());
    p.rpc_url = Some("https://rpc.rehearsal.invalid".to_string());
    p.explorer_url = Some("https://explorer.rehearsal.invalid".to_string());
    p.faucet_url = Some("https://faucet.rehearsal.invalid".to_string());
    p.bootnodes = Some(NEW_BOOTNODES.iter().map(|s| s.to_string()).collect());
    p.address_book = Some("addresses/40204.json".to_string());
    p.pending.clear();
    p.ready()
        .unwrap_or_else(|e| panic!("the filled profile must be ready: {e:?}"))
}

/// The node manager the app builds for this install (node.rs `build_node_state`), except that
/// the binary is a file that is not executable: the start runs every step up to the spawn, and no
/// process ever writes into the dir under test.
fn manager(
    root: &Path,
    app: &Path,
    node_keyring: &MapKeyring,
    network: Result<crate::network_profile::ReadyNetwork, crate::network_profile::ProfileError>,
) -> crate::node::NodeManager {
    let bin = root.join("not-a-node");
    std::fs::write(&bin, b"not an executable").unwrap_or_else(|e| panic!("bin: {e}"));
    let barrier = Arc::new(crate::startup_barrier::StartupBarrier::new());
    barrier.run_cleanup(|| {});
    crate::node::NodeManager::new(
        Box::new(node_keyring.clone()),
        bin,
        app.join("node"),
        root.join("crash-records.jsonl"),
        "http://127.0.0.1:59999",
    )
    .with_network(network)
    .with_startup_barrier(barrier, std::time::Duration::ZERO)
    .with_node_ports(Vec::new())
    .with_key_runtime_dir(Some(root.join("rt")))
}

/// Start for the new network, then stop. The migration runs before the spawn; the "binary"
/// cannot be executed, so the supervisor never runs a process in the dir (it may report the
/// spawn failure at once or retry it until the stop).
fn start_for_new_network(mgr: &crate::node::NodeManager) {
    match mgr.start() {
        Ok(()) | Err(crate::node::NodeError::Spawn(_)) => {}
        other => panic!("expected the migration to complete before the spawn: {other:?}"),
    }
    mgr.stop();
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

/// Files whose bytes depend on the run (a random migration nonce, the absolute data dir); the
/// manifest records their presence and the tests check their content.
const RUN_DEPENDENT: &[&str] = &["node/chain-genesis", "node/node.toml"];

/// One line per file in `app` (`file <path> sha256:<hex>` or `file <path> present`) and per
/// keychain slot (`keychain <service>/<account> sha256:<hex>`), sorted.
fn manifest(app: &Path, keychains: &[(&str, &MapKeyring)]) -> String {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for name in names(dir) {
            let p = dir.join(&name);
            if p.is_dir() {
                walk(base, &p, out);
                continue;
            }
            let rel = p
                .strip_prefix(base)
                .unwrap_or_else(|e| panic!("{e}"))
                .to_string_lossy()
                .replace('\\', "/");
            if RUN_DEPENDENT.contains(&rel.as_str()) {
                out.push(format!("file {rel} present"));
            } else {
                out.push(format!("file {rel} sha256:{}", sha256_hex(&read(&p))));
            }
        }
    }
    let mut lines = Vec::new();
    walk(app, app, &mut lines);
    for (service, kr) in keychains {
        for (account, v) in kr.snapshot() {
            lines.push(format!(
                "keychain {service}/{account} sha256:{}",
                sha256_hex(&v)
            ));
        }
    }
    lines.sort();
    lines.join("\n") + "\n"
}

fn golden_lines() -> String {
    let text = String::from_utf8(read(&golden_path())).unwrap_or_else(|e| panic!("{e}"));
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

#[test]
fn a_v051_install_on_r1005_switches_to_the_new_genesis_and_keeps_everything_else() {
    let root = scratch("switch");
    let app = install_v051_on_r1005(&root);
    let node = app.join("node");
    let custody_kr = MapKeyring::from_fixture("custody.keyring.json");
    let node_kr = MapKeyring::from_fixture("node-r1005.keyring.json");
    let custody_before = custody_kr.snapshot();
    let node_key_before = node_kr.snapshot();
    let net = new_network();
    // The fixture is an r1005 install: its marker names the alpha profile's genesis.
    assert_eq!(
        crate::node_genesis::read_marker_record(&node)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .map(|m| m.genesis),
        Some(R1005.to_string())
    );
    assert_eq!(
        crate::network_profile::profile("alpha-r1005")
            .unwrap_or_else(|e| panic!("{e:?}"))
            .genesis_hash
            .as_deref(),
        Some(R1005)
    );
    assert_ne!(net.genesis_hash, R1005);

    let mgr = manager(&root, &app, &node_kr, Ok(net));
    assert!(mgr.status().notice.is_none(), "no notice before the switch");
    start_for_new_network(&mgr);

    // Chain database: every r1005 RocksDB file is gone (D-K6-1 open: deleted, not archived).
    let db: Vec<String> = names(&fixture_root().join("node-r1005"))
        .into_iter()
        .filter(|n| crate::node_genesis::is_chain_db_entry(n))
        .collect();
    assert_eq!(db.len(), 9, "the fixture holds a full RocksDB file set");
    for f in &db {
        assert!(
            !node.join(f).exists(),
            "r1005 chain file {f} must not survive"
        );
    }
    // M1/M2: the marker is the commit, carrying the migration nonce; the intent is cleaned up.
    let marker = crate::node_genesis::read_marker_record(&node)
        .unwrap_or_else(|e| panic!("marker: {e:?}"))
        .unwrap_or_else(|| panic!("no marker after the switch"));
    assert_eq!(marker.genesis, NEW_GENESIS);
    assert!(
        marker.nonce.is_some(),
        "a migration commit carries its nonce"
    );
    assert!(crate::node_genesis::read_intent(&node)
        .unwrap_or_else(|e| panic!("intent: {e:?}"))
        .is_none());
    // The previous node.toml (with the member's own edit) is set aside byte for byte.
    assert_eq!(
        read(&node.join(ASIDE)),
        read(&fixture_root().join("node-r1005/node.toml")),
        "the r1005 node.toml is kept as {ASIDE}"
    );
    // ... and the new one is rendered from the new profile, never the r1005 one.
    let toml = String::from_utf8(read(&node.join("node.toml"))).unwrap_or_else(|e| panic!("{e}"));
    for b in NEW_BOOTNODES {
        assert!(toml.contains(b), "new bootnode {b} in node.toml");
    }
    let alpha = crate::network_profile::profile("alpha-r1005")
        .unwrap_or_else(|e| panic!("{e:?}"))
        .bootnodes
        .clone()
        .unwrap_or_default();
    assert!(!alpha.is_empty());
    for b in &alpha {
        assert!(
            !toml.contains(b.as_str()),
            "r1005 bootnode {b} left in node.toml"
        );
    }
    assert!(toml.contains(&node.to_string_lossy().replace('\\', "\\\\")));
    // Node keys and the rest of the node dir are kept, byte for byte.
    for f in [
        "proposer.key",
        "noise.key",
        "peer.id",
        "encryption.meta",
        "crash-records.jsonl",
    ] {
        assert_eq!(
            read(&node.join(f)),
            read(&fixture_root().join("node-r1005").join(f)),
            "{f} is kept"
        );
    }

    // Keychain: no slot changed or was re-minted.
    assert_eq!(custody_kr.snapshot(), custody_before, "custody slots");
    assert_eq!(node_kr.snapshot(), node_key_before, "node storage key slot");

    // The wallet opens from the post-switch app data dir and derives the 0.5.1 address.
    let meta: serde_json::Value =
        serde_json::from_slice(&read(&fixture_root().join("custody.keyring.json")))
            .unwrap_or_else(|e| panic!("{e}"));
    let vault = CustodyVault::new(Box::new(custody_kr.clone()), app.join("custody.enc"), 30);
    vault
        .ensure_auto_unlocked()
        .unwrap_or_else(|e| panic!("the 0.5.1 vault must open after the switch: {e}"));
    let info = citrate_core_kit::wallet::address(&vault).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(Some(info.address.as_str()), meta["walletAddress"].as_str());

    // The submitted-tx record a 0.5.2 launch opens next to the vault is uncertain (0.5.1 kept
    // none), so a first send error after the switch is "outcome unknown", never "not sent".
    let log = citrate_core_kit::submitted_log::SubmittedTxLog::open(app.join("submitted-tx.json"));
    assert!(log.is_uncertain());

    // Loaders this build uses read the 0.5.1 files after the switch.
    assert!(!crate::hermes_terminal::enabled(&app.join("hermes")));
    let book = crate::daemons::load_book(&app.join("daemons.json"))
        .unwrap_or_else(|e| panic!("daemons: {e}"));
    assert_eq!(book.list(1_792_300_000_000, -420).len(), 1);
    let invites: Vec<crate::invites::PendingInvite> =
        serde_json::from_slice(&read(&app.join("invites/pending.json")))
            .unwrap_or_else(|e| panic!("invites: {e}"));
    assert_eq!(invites.len(), 2);

    // Golden: every file in the app data dir and every keychain slot after the switch.
    let got = manifest(
        &app,
        &[
            ("ai.citrate.core.custody", &custody_kr),
            ("ai.citrate.core", &node_kr),
        ],
    );
    if std::env::var_os("CITRATE_WRITE_XGENESIS_GOLDEN").is_some() {
        let header =
            "# RC52 G2: the app data dir and keychain of a 0.5.1 install on r1005 after a \
                      0.5.2 start on a new genesis.\n# Written by upgrade_xgenesis_tests with \
                      CITRATE_WRITE_XGENESIS_GOLDEN=1; `present` = run-dependent bytes, checked \
                      by the test.\n";
        std::fs::write(golden_path(), format!("{header}{got}"))
            .unwrap_or_else(|e| panic!("write golden: {e}"));
    }
    assert_eq!(
        got,
        golden_lines(),
        "post-upgrade manifest differs from {}",
        golden_path().display()
    );
    // The member is told, and told where the previous node settings are.
    let notice = mgr
        .status()
        .notice
        .unwrap_or_else(|| panic!("a switch must be reported to the member"));
    assert!(
        notice.starts_with(crate::node_genesis::CHAIN_RESET_NOTICE),
        "{notice}"
    );
    let aside = node.join(ASIDE).to_string_lossy().to_string();
    assert!(
        notice.contains(&aside),
        "the notice names where the previous node settings were kept ({aside}): {notice}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_switch_interrupted_after_the_config_was_set_aside_still_reports_it_on_the_next_launch() {
    // M3: the first launch stops after setting node.toml aside (a crash there). The next launch
    // finishes the switch; the member must still learn where the previous settings went.
    let root = scratch("crash-retire");
    let app = install_v051_on_r1005(&root);
    let node = app.join("node");
    let node_kr = MapKeyring::from_fixture("node-r1005.keyring.json");
    let first = manager(&root, &app, &node_kr, Ok(new_network()));
    crate::node_genesis::test_hooks::STOP_AFTER
        .with(|s| s.set(Some(crate::node_genesis::Step::ConfigRetired)));
    let crashed = first.start();
    crate::node_genesis::test_hooks::STOP_AFTER.with(|s| s.set(None));
    assert!(
        matches!(crashed, Err(crate::node::NodeError::Genesis(_))),
        "{crashed:?}"
    );
    assert!(node.join(ASIDE).exists(), "set aside before the crash");
    assert!(
        crate::node_genesis::read_intent(&node)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .is_some(),
        "uncommitted"
    );

    let next = manager(&root, &app, &node_kr, Ok(new_network()));
    start_for_new_network(&next);
    let marker = crate::node_genesis::read_marker_record(&node)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .unwrap_or_else(|| panic!("no marker"));
    assert_eq!(marker.genesis, NEW_GENESIS);
    assert_eq!(
        read(&node.join(ASIDE)),
        read(&fixture_root().join("node-r1005/node.toml"))
    );
    let notice = next
        .status()
        .notice
        .unwrap_or_else(|| panic!("the finishing launch reports the switch"));
    let aside = node.join(ASIDE).to_string_lossy().to_string();
    assert!(
        notice.contains(&aside),
        "the finishing launch still names {aside}: {notice}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_launch_after_a_completed_switch_keeps_the_dir_and_reports_nothing() {
    let root = scratch("relaunch");
    let app = install_v051_on_r1005(&root);
    let node = app.join("node");
    let node_kr = MapKeyring::from_fixture("node-r1005.keyring.json");
    start_for_new_network(&manager(&root, &app, &node_kr, Ok(new_network())));
    assert_eq!(
        crate::node_genesis::read_marker_record(&node)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .map(|m| m.genesis),
        Some(NEW_GENESIS.to_string()),
        "the first launch switched"
    );
    assert!(node.join(ASIDE).exists());
    let before: Vec<(String, Vec<u8>)> = names(&node)
        .into_iter()
        .map(|n| {
            let b = read(&node.join(&n));
            (n, b)
        })
        .collect();

    let again = manager(&root, &app, &node_kr, Ok(new_network()));
    start_for_new_network(&again);
    assert!(
        again.status().notice.is_none(),
        "a kept dir is not reported again"
    );
    let after: Vec<(String, Vec<u8>)> = names(&node)
        .into_iter()
        .map(|n| {
            let b = read(&node.join(&n));
            (n, b)
        })
        .collect();
    assert_eq!(after, before, "nothing in the switched node dir changes");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_build_whose_new_genesis_is_not_set_leaves_the_r1005_install_untouched() {
    // P2/P5: the real, unset genesis-2026-12 profile. No migration step runs.
    let root = scratch("unset");
    let app = install_v051_on_r1005(&root);
    let node_kr = MapKeyring::from_fixture("node-r1005.keyring.json");
    let custody_kr = MapKeyring::from_fixture("custody.keyring.json");
    let unset = crate::network_profile::profile("genesis-2026-12")
        .unwrap_or_else(|e| panic!("{e:?}"))
        .ready();
    assert!(unset.is_err(), "genesis-2026-12 is not set in this build");
    let before = manifest(
        &app,
        &[
            ("ai.citrate.core.custody", &custody_kr),
            ("ai.citrate.core", &node_kr),
        ],
    );
    let mgr = manager(&root, &app, &node_kr, unset);
    assert!(matches!(
        mgr.start(),
        Err(crate::node::NodeError::Network(_))
    ));
    assert!(mgr.status().notice.is_none());
    let after = manifest(
        &app,
        &[
            ("ai.citrate.core.custody", &custody_kr),
            ("ai.citrate.core", &node_kr),
        ],
    );
    assert_eq!(after, before, "an unset profile touches nothing");
    assert_eq!(
        read(&app.join("node/chain-genesis")),
        read(&fixture_root().join("node-r1005/chain-genesis"))
    );
    let _ = std::fs::remove_dir_all(&root);
}
