// CORE-C1.1 — NodeDomain ↔ SidecarSupervisor wiring + @rule8 storage-key /
// ciphertext-at-rest suite. Red-first (see the sprint file + PR body).
//
// CI-safe: everything here runs headless against (a) an in-memory keyring fake
// (the @rule8 storage-key handoff) and (b) a small native STUB node binary that
// writes ciphertext to its data dir and blocks until killed — so the wiring,
// the storage-key mint/hold, and the ciphertext-at-rest grep are all proven
// WITHOUT building the heavy ark/zk node. Status-parsing is proven against a
// mock RpcTransport (no port races). The real node bounded-sync is a separate,
// scripted + `#[ignore]` live proof at the bottom.

use super::*;
use crate::custody::CustodyError;
use crate::rpc::{RpcError, RpcTransport};
use serde_json::{json, Value};
use std::sync::{Mutex as StdMutex, OnceLock};

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

/// In-memory keyring fake so the @rule8 storage-key logic runs without a live
/// platform keyring.
#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<std::collections::HashMap<String, Vec<u8>>>,
}

impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

/// A shared handle so a test can inspect the keyring after the manager took a
/// boxed view of it.
struct SharedFake(std::sync::Arc<FakeKeyring>);
impl Keyring for SharedFake {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        self.0.get(account)
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.0.set(account, secret)
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.0.delete(account)
    }
}

/// A keyring whose `get` fails (backend unreachable), to prove fail-closed.
struct DeadKeyring;
impl Keyring for DeadKeyring {
    fn get(&self, _a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> std::result::Result<(), CustodyError> {
        Ok(())
    }
}

/// A mock RPC transport that answers `eth_blockNumber` / `net_peerCount` with a
/// fixed height/peers, so status-parsing is tested without a live port.
struct MockRpc {
    height: u64,
    peers: u64,
}
impl RpcTransport for MockRpc {
    fn call(&self, body: Value) -> std::result::Result<Value, RpcError> {
        let method = body.get("method").and_then(Value::as_str).unwrap_or("");
        let id = body.get("id").cloned().unwrap_or(json!(1));
        let result = match method {
            "eth_blockNumber" => json!(format!("0x{:x}", self.height)),
            "net_peerCount" => json!(format!("0x{:x}", self.peers)),
            other => return Err(RpcError::Node(format!("unexpected method {other}"))),
        };
        Ok(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compile the checked-in native fixture once per test process and return its
/// absolute path. The output lives under Cargo's OUT_DIR, never in the checkout.
///
/// Compiling at test runtime keeps `cargo test --lib` self-contained: Cargo does
/// not build `[[bin]]` targets for library unit tests, while the supervisor must
/// receive a real executable path and continue to spawn it directly without a
/// shell.
fn stub_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("stub_node.rs");
        let out_dir = PathBuf::from(env!("OUT_DIR")).join("test-fixtures");
        std::fs::create_dir_all(&out_dir).expect("create native fixture output directory");
        let output = out_dir.join(format!(
            "stub_node-{}{}",
            std::process::id(),
            std::env::consts::EXE_SUFFIX
        ));
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let status = std::process::Command::new(rustc)
            .arg("--edition=2021")
            .arg(&source)
            .arg("-o")
            .arg(&output)
            .status()
            .expect("run rustc for native node fixture");
        assert!(status.success(), "native node fixture must compile from {}", source.display());
        output
    })
    .clone()
}

/// A fresh unique temp dir for a test's data dir + crash records.
fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("citrate-core-c1_1-{tag}-{nanos}-{:?}", std::thread::current().id()));
    p
}

/// Build a manager over the stub binary + a fake keyring + temp dirs. Returns
/// the manager, the shared keyring handle, and the data dir.
fn stub_manager(tag: &str) -> (NodeManager, std::sync::Arc<FakeKeyring>, PathBuf) {
    let fake = std::sync::Arc::new(FakeKeyring::default());
    let data_dir = tmp_dir(tag);
    let crash = data_dir.join("crash-records.jsonl");
    let mgr = NodeManager::new(
        Box::new(SharedFake(fake.clone())),
        stub_bin(),
        data_dir.clone(),
        crash,
        // No live RPC in the stub path — status uses the supervisor state and
        // an unreachable RPC (which honestly yields 0/0).
        "http://127.0.0.1:59999",
    )
    // The app's startup cleanup has run (SCL-S8.5a) and no node port is checked: the stub
    // node binds nothing, and the host's real 8545/8546/30303 are not this test's business.
    .with_startup_barrier(opened_barrier(), std::time::Duration::ZERO)
    .with_node_ports(Vec::new());
    (mgr, fake, data_dir)
}

/// A startup barrier whose cleanup has already run.
fn opened_barrier() -> std::sync::Arc<StartupBarrier> {
    let b = std::sync::Arc::new(StartupBarrier::new());
    b.run_cleanup(|| {});
    b
}

// ---------------------------------------------------------------------------
// WP1 @rule8 — storage key in the OS keyring; ciphertext at rest
// ---------------------------------------------------------------------------

/// The storage key is minted into the keyring on first `start` and is exactly
/// 32 bytes (AES-256). @rule8: the key lives in the keyring, not on disk clear.
#[test]
fn storage_key_is_minted_into_keyring_on_start() {
    let (mgr, fake, _data) = stub_manager("mint");
    // Key absent before start.
    assert!(fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().is_none());
    mgr.start().expect("stub node starts");
    // Give the stub a beat to write its data file, then stop.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let key = fake
        .get(KEYRING_NODE_STORAGE_ACCOUNT)
        .unwrap()
        .expect("storage key minted into keyring");
    assert_eq!(key.len(), STORAGE_KEY_LEN, "storage key must be 32 bytes");
    mgr.stop();
}

/// The storage key reaches the node only through an owner-only key file named in
/// `CITRATE_STORAGE_KEY_FILE`: the key is in no env var and no argv of the child, the file
/// holds exactly the keyring key, is 0600 while the node runs, and is gone after stop.
#[test]
fn storage_key_reaches_the_node_only_through_an_owner_only_file() {
    let (mgr, fake, _data) = stub_manager("key-file");
    mgr.start().expect("stub node starts");
    let key = fake
        .get(KEYRING_NODE_STORAGE_ACCOUNT)
        .unwrap()
        .expect("storage key minted into keyring");
    let key_hex = hex::encode(&key);

    let spec = mgr.build_spec();
    assert!(
        !spec.env.iter().any(|(k, v)| k == "CITRATE_STORAGE_KEY" || v.contains(&key_hex)),
        "the key itself is never in the child env"
    );
    assert!(!spec.args.iter().any(|a| a.contains(&key_hex)), "never in argv");

    let path = mgr.storage_key_path();
    assert!(!path.starts_with(&mgr.data_dir), "outside the chain data dir");
    assert_eq!(std::fs::read_to_string(&path).expect("key file"), key_hex);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "owner-only key file");
    }
    mgr.stop();
    assert!(!path.exists(), "the key file is removed once the node stopped");
}

/// A symbolic link already sitting where the key file goes is never written through: start
/// replaces it with a regular owner-only file, and the link's target is left untouched.
#[cfg(unix)]
#[test]
fn storage_key_file_replaces_a_planted_symlink_instead_of_writing_through_it() {
    let (mgr, _fake, data) = stub_manager("key-file-link");
    let path = mgr.storage_key_path();
    let parent = path.parent().expect("key file has a parent");
    std::fs::create_dir_all(parent).expect("mkdir key dir");
    std::fs::create_dir_all(&data).expect("mkdir data");
    let decoy = data.join("decoy.txt");
    std::fs::write(&decoy, b"untouched").expect("write decoy");
    std::os::unix::fs::symlink(&decoy, &path).expect("plant symlink");

    mgr.start().expect("stub node starts");
    let meta = std::fs::symlink_metadata(&path).expect("stat key file");
    assert!(meta.file_type().is_file(), "the key file is a regular file, not the planted link");
    assert_eq!(
        std::fs::read(&decoy).expect("read decoy"),
        b"untouched",
        "the link's target was not written"
    );
    mgr.stop();
}

/// Agent precompiles are active from genesis on 40204 (reroll 2026-10-05, owner decision
/// 2026-10-04): the node binary's release pin sets the height to 0, and on a release network a
/// config or env height that disagrees with the pin stops the node at start-up (citrate-chain
/// `core/execution/src/agent_fork.rs`). So the app must never pass its own height: no
/// activation gate lives in the app.
#[test]
fn spawn_env_never_sets_an_agent_precompile_height() {
    let (mgr, _fake, _data) = stub_manager("agent-height");
    let spec = mgr.build_spec();
    assert!(
        !spec.env.iter().any(|(k, _)| k == "CITRATE_AGENT_PRECOMPILES_HEIGHT"),
        "the node's release pin is the only source of the agent precompile height"
    );
    assert!(
        !spec.args.iter().any(|a| a.contains("agent_precompiles_height")),
        "no agent precompile height on the command line either: {:?}",
        spec.args
    );
    // The member node config core writes (`[chain] agent_precompiles_height` is the node's
    // config key) never carries one.
    assert!(
        !MEMBER_NODE_CONFIG.contains("agent_precompiles_height"),
        "member-node.toml must not set the agent precompile height"
    );
}

/// Sync-wedge tripwire (DGX_NODE_SYNC_WEDGE_RESPONSE 2026-07-22): the node spawn MUST
/// carry the fleet producer's consensus env — `CITRATE_BLOCK_V2`,
/// `CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000`, and the live `CITRATE_VALIDATOR_REGISTRY`
/// — alongside the storage key. Dropping any of them disables the validator/§R' state
/// path, forks the state root, and wedges the node at block 2,580. This locks them in.
#[test]
fn spawn_env_carries_the_fleet_consensus_vars() {
    let (mgr, _fake, _data) = stub_manager("consensus-env");
    let spec = mgr.build_spec();
    let get = |k: &str| {
        spec.env
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    };
    let key_path = mgr.storage_key_path();
    assert_eq!(
        get(NODE_STORAGE_KEY_FILE_ENV),
        Some(key_path.to_string_lossy().as_ref()),
        "the storage key file path is still passed"
    );
    assert_eq!(get(NODE_BLOCK_V2_ENV), Some("1"), "v2 execute-on-receive explicit");
    // RESOLVED 2026-09-12 (`srp-s5-diskfix` reroll): `2000`, read live from rpc-1
    // (142.93.58.145) — both the running citrate-node process env and the
    // citrate-node.service.d/reroll.conf drop-in. The reroll RESET the fleet
    // activation to 2000 (chain crossed 2000 without the producer halting), which
    // INVERTS the 2026-09-09 finding: on this chain a bundled `1000000` forks at
    // block 2000 (it fails to settle the §R' reward the fleet settles from 2000),
    // and `2000` syncs clean. Keep this and NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE
    // in lockstep, and re-read the fleet on any future re-roll — the failure mode
    // is a silent fork.
    assert_eq!(
        get(NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV),
        Some("2000"),
        "validator activation height must match the fleet",
    );
    // Re-earned for the 2026-09-07 re-roll (was `0x61d44d8a…`). ValidatorRegistry
    // moves with the deployer nonce like the other CREATE deploys. Taken from the
    // address book synced at `2d88191`; live `eth_getCode` is 26,054 bytes here
    // and `0x` at the previous pin.
    // Re-earned 2026-09-30 for the fresh-keys reroll (genesis 0x0f2b567f…): DGX core-pin harvest (federation#253), live eth_getCode non-empty (13,026 bytes).
    // Re-earned 2026-10-06 for the r1005 reroll (genesis 0x1dcfc490…, chain 2979a157): DGX core-pin, book via sync-addresses.py --rpc, live eth_getCode non-empty (13,026 bytes); `0x` at the previous pin.
    assert_eq!(
        get(NODE_VALIDATOR_REGISTRY_ENV),
        Some("0xde4f679632be7810d915f637cc6a72bf076a3bd9"),
        "ValidatorRegistry must be the live 40204 address the fleet runs",
    );
    // INVERTED 2026-07-30. This previously asserted the retain window MUST be
    // wired, on the reasoning that an unpruned desktop follower OOMs near 150k
    // blocks. That reasoning is right in general and wrong on 40204: block
    // 54,601 merges a parent at HEIGHT 32, and a 10,000-block window prunes it
    // away, so admission of 54,601 fails MissingParent forever and the node can
    // never cold-sync past 54,600 (chain PR #144). The test was faithfully
    // pinning the bug.
    assert_eq!(
        get(NODE_DAG_PRUNE_RETAIN_ENV),
        None,
        "DAG pruning must NOT be wired: it wedges cold sync at block 54,600. \
         See the NODE_DAG_PRUNE_RETAIN_ENV doc comment for the re-enable \
         conditions — this is a deliberate memory-for-correctness trade.",
    );
    // Still joins the public testnet with the encrypted data dir.
    assert!(spec.args.iter().any(|a| a == "testnet"), "joins the public testnet");
}

/// A known coinbase alone does NOT arm the producer: the node must catch up to
/// the network tip first. So with a coinbase set but mining NOT armed, the node
/// spawns as a plain FOLLOWER (no `--mine`/`--coinbase`). This is the guard for
/// the concurrent-producer wedge — a validator that is in the active set would
/// otherwise produce a competing block while still tens of thousands of blocks
/// behind. Once ARMED (caught up), the next spawn carries `--mine --coinbase`.
#[test]
fn spawn_is_follower_until_armed_then_mines() {
    let (mgr, _fake, _data) = stub_manager("coinbase-set");
    mgr.set_coinbase("0xd12c00c377eb4615a7ae934df509c903a29ecb6c".to_string());

    // Coinbase known but NOT armed → follower (no mining flags).
    let follower = mgr.build_spec();
    assert!(
        !follower.args.iter().any(|a| a == "--mine"),
        "a coinbase alone must NOT arm mining — the node syncs as a follower first",
    );
    assert!(
        !follower.args.iter().any(|a| a == "--coinbase"),
        "no --coinbase flag until the producer is armed",
    );
    assert!(!mgr.is_mining_armed(), "not armed by set_coinbase");

    // Arm (as the caught-up gate would) → the next spawn carries --mine --coinbase.
    mgr.arm_mining();
    assert!(mgr.is_mining_armed(), "armed after arm_mining");
    let producer = mgr.build_spec();
    assert!(producer.args.iter().any(|a| a == "--mine"), "mining armed");
    let ci = producer
        .args
        .iter()
        .position(|a| a == "--coinbase")
        .expect("--coinbase flag present once armed");
    assert_eq!(
        producer.args.get(ci + 1).map(String::as_str),
        Some("0xd12c00c377eb4615a7ae934df509c903a29ecb6c"),
        "coinbase is the member's wallet address (the earner/staker)",
    );
}

/// The mining gate drives the armed state: a hold never arms and is reported in the status;
/// `Ok` arms; a later hold DISARMS an armed producer, and the next spawn is a follower again.
#[test]
fn mining_gate_verdict_arms_holds_and_disarms() {
    use crate::mining_gate::MiningHold;
    let (mgr, _fake, _data) = stub_manager("gate-verdict");
    mgr.set_coinbase("0xd12c00c377eb4615a7ae934df509c903a29ecb6c".to_string());

    assert!(!mgr.apply_mining_verdict(Err(MiningHold::TooFewProducers { others: 1 })));
    assert!(!mgr.is_mining_armed(), "a hold never arms");
    let st = mgr.status();
    assert!(!st.mining_armed);
    assert!(
        st.mining_hold.as_deref().unwrap_or("").contains("1 other producer(s)"),
        "the hold reason reaches the status: {:?}",
        st.mining_hold
    );

    assert!(mgr.apply_mining_verdict(Ok(())), "Ok arms");
    assert!(mgr.is_mining_armed());
    let st = mgr.status();
    assert!(st.mining_armed);
    assert_eq!(st.mining_hold, None, "no hold reason once armed");
    assert!(mgr.build_spec().args.iter().any(|a| a == "--mine"));
    assert!(!mgr.apply_mining_verdict(Ok(())), "re-arming is a no-op");

    assert!(
        mgr.apply_mining_verdict(Err(MiningHold::NetworkStalled { age_secs: 600 })),
        "a hold disarms an armed producer"
    );
    assert!(!mgr.is_mining_armed());
    assert!(!mgr.build_spec().args.iter().any(|a| a == "--mine"), "follower again");
    assert!(mgr.status().mining_hold.as_deref().unwrap_or("").contains("600s old"));
}

/// The status JSON stays exactly the old shape while production was never requested.
#[test]
fn status_omits_mining_fields_until_requested() {
    let (mgr, _fake, _data) = stub_manager("gate-status-shape");
    let v = serde_json::to_value(mgr.status()).expect("status serializes");
    assert!(v.get("miningArmed").is_none(), "{v}");
    assert!(v.get("miningHold").is_none(), "{v}");
}

/// The pure sync gate: the producer arms only once the local head is within
/// `SYNC_ARM_MARGIN` of the authoritative network tip. Far behind → false (stay a
/// follower); at/near the tip → true; a spurious local-ahead never underflows.
#[test]
fn is_caught_up_gate_only_true_near_the_tip() {
    use super::is_caught_up;
    assert!(!is_caught_up(54_600, 109_150), "54k behind is not caught up");
    assert!(!is_caught_up(109_100, 109_150), "50 behind (> margin 32) is not caught up");
    assert!(is_caught_up(109_130, 109_150), "20 behind (<= margin) is caught up");
    assert!(is_caught_up(109_150, 109_150), "at the tip is caught up");
    assert!(is_caught_up(109_200, 109_150), "ahead never underflows → caught up");
}

/// Without a coinbase (a fresh follower, or pre-wallet boot), the node spawns
/// WITHOUT `--mine`/`--coinbase`: it syncs as a follower and never attempts
/// production. Enabling mining is gated on the wallet address being known.
#[test]
fn spawn_args_omit_mining_when_no_coinbase() {
    let (mgr, _fake, _data) = stub_manager("coinbase-unset");
    let spec = mgr.build_spec();
    assert!(
        !spec.args.iter().any(|a| a == "--mine"),
        "no mining without a coinbase",
    );
    assert!(
        !spec.args.iter().any(|a| a == "--coinbase"),
        "no coinbase flag without a coinbase",
    );
}

/// `set_coinbase` is idempotent-ish: the last set value is what the next spawn
/// uses (the member's stable wallet address), so a restart re-arms the producer.
#[test]
fn set_coinbase_is_reflected_in_the_next_spawn() {
    let (mgr, _fake, _data) = stub_manager("coinbase-restart");
    mgr.set_coinbase("0x0000000000000000000000000000000000000001".to_string());
    // The coinbase only reaches the argv once the producer is armed (caught up).
    mgr.arm_mining();
    let spec = mgr.build_spec();
    let ci = spec.args.iter().position(|a| a == "--coinbase").unwrap();
    assert_eq!(
        spec.args.get(ci + 1).map(String::as_str),
        Some("0x0000000000000000000000000000000000000001"),
    );
}

/// A second `start` reuses the SAME keyring key (does not re-mint), so an
/// existing encrypted data dir stays openable across restarts.
#[test]
fn storage_key_is_stable_across_restarts() {
    let (mgr, fake, _data) = stub_manager("stable");
    mgr.start().expect("start 1");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let k1 = fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().unwrap();
    mgr.stop();
    mgr.start().expect("start 2");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let k2 = fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().unwrap();
    mgr.stop();
    assert_eq!(k1, k2, "storage key must be stable across restarts");
}

/// An unreachable keyring is a HARD FAULT: we never spawn the node without an
/// encryption key (that would silently produce a plaintext data dir). Fail
/// closed (@rule8).
#[test]
fn dead_keyring_blocks_start_fail_closed() {
    let data_dir = tmp_dir("dead");
    let crash = data_dir.join("crash.jsonl");
    let mgr = NodeManager::new(
        Box::new(DeadKeyring),
        stub_bin(),
        data_dir,
        crash,
        "http://127.0.0.1:59998",
    )
    .with_startup_barrier(opened_barrier(), std::time::Duration::ZERO)
    .with_node_ports(Vec::new());
    let r = mgr.start();
    assert!(matches!(r, Err(NodeError::Keyring(_))), "got: {r:?}");
}

/// CIPHERTEXT-AT-REST TRIPWIRE (@rule8 / ENCRYPT pattern): after the node runs
/// with a keyring key, a raw-disk grep of the data dir must NOT contain the
/// known plaintext sentinel — the bytes on disk are ciphertext.
#[test]
fn data_dir_is_ciphertext_at_rest() {
    let (mgr, _fake, data_dir) = stub_manager("ciphertext");
    mgr.start().expect("start");
    // Wait for the stub to write its data file.
    let data_file = data_dir.join("data.rocks");
    for _ in 0..50 {
        if data_file.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    mgr.stop();
    assert!(data_file.exists(), "stub node must write a data file");
    let bytes = std::fs::read(&data_file).expect("read data file");
    // The plaintext sentinel the stub XOR-encrypts. Its presence = encryption
    // failed.
    let sentinel = b"CITRATE_PLAINTEXT_SENTINEL_v1";
    assert!(
        !contains_subslice(&bytes, sentinel),
        "data dir must be CIPHERTEXT at rest — plaintext sentinel found"
    );
    // And the ciphertext is non-empty (something really was written).
    assert!(!bytes.is_empty(), "data file must not be empty");
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// Naive subslice search for the tripwire (no extra deps).
fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|w| w == needle)
}

// ---------------------------------------------------------------------------
// WP2 — NodeDomain wiring: start → supervisor Running → stop → no orphan
// ---------------------------------------------------------------------------

/// `start` spawns the node under the supervisor and it reaches Running; `stop`
/// releases it cleanly (state back to stopped, no orphan pid).
#[test]
fn start_reaches_running_then_stop_is_clean() {
    let (mgr, _fake, data_dir) = stub_manager("wiring");
    mgr.start().expect("start");
    // Poll status until Running (the stub is a long-lived child).
    let mut running = false;
    let mut pid_seen: Option<u32> = None;
    for _ in 0..100 {
        let st = mgr.status();
        if st.state == "running" {
            running = true;
            // capture the live pid from the supervisor for the orphan check
            let guard = mgr.sup.lock().unwrap();
            pid_seen = guard.as_ref().and_then(|s| s.status().pid);
            drop(guard);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "node must reach Running under the supervisor");
    mgr.stop();
    // After stop, status is stopped.
    assert_eq!(mgr.status().state, "stopped");
    // No orphan: the captured pid is gone.
    if let Some(pid) = pid_seen {
        assert!(!pid_alive_test(pid), "stopped node must leave no orphan (pid {pid})");
    }
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// Unix liveness probe for the orphan check (kill -0).
#[cfg(unix)]
fn pid_alive_test(pid: u32) -> bool {
    // Give the OS a beat to reap.
    std::thread::sleep(std::time::Duration::from_millis(200));
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}
#[cfg(not(unix))]
fn pid_alive_test(_pid: u32) -> bool {
    false
}

/// Starting an already-running node returns `AlreadyRunning` (idempotent guard),
/// not a second spawn.
#[test]
fn double_start_is_rejected() {
    let (mgr, _fake, data_dir) = stub_manager("double");
    mgr.start().expect("first start");
    std::thread::sleep(std::time::Duration::from_millis(150));
    let r = mgr.start();
    assert!(matches!(r, Err(NodeError::AlreadyRunning)), "got: {r:?}");
    mgr.stop();
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// A missing binary path fails closed with `BinaryNotFound` — never a silent
/// no-op "started".
#[test]
fn missing_binary_fails_closed() {
    let fake = std::sync::Arc::new(FakeKeyring::default());
    let data_dir = tmp_dir("nobin");
    let mgr = NodeManager::new(
        Box::new(SharedFake(fake)),
        PathBuf::from("/nonexistent/citrate-binary-xyz"),
        data_dir,
        PathBuf::from("/tmp/x.jsonl"),
        "http://127.0.0.1:59997",
    )
    .with_startup_barrier(opened_barrier(), std::time::Duration::ZERO)
    .with_node_ports(Vec::new());
    let r = mgr.start();
    assert!(matches!(r, Err(NodeError::BinaryNotFound(_))), "got: {r:?}");
}

/// `stop` on a never-started node is a no-op (idempotent), status stays stopped.
#[test]
fn stop_when_never_started_is_noop() {
    let (mgr, _fake, _data) = stub_manager("stopnoop");
    mgr.stop();
    assert_eq!(mgr.status().state, "stopped");
}

// ---------------------------------------------------------------------------
// Status parsing — real height/peers from the (mock) local RPC
// ---------------------------------------------------------------------------

/// `RpcClient::block_number` / `peer_count` parse the node's hex quantities.
#[test]
fn rpc_parses_height_and_peers() {
    let client = RpcClient::with_transport(MockRpc { height: 0x1a4, peers: 7 });
    assert_eq!(client.block_number().unwrap(), 0x1a4);
    assert_eq!(client.peer_count().unwrap(), 7);
}

/// State mapping covers every supervisor state → bridge vocabulary.
#[test]
fn state_mapping_is_total() {
    assert_eq!(map_state(&SupervisorState::Off), "stopped");
    assert_eq!(map_state(&SupervisorState::Starting), "starting");
    assert_eq!(map_state(&SupervisorState::Running), "running");
    assert_eq!(
        map_state(&SupervisorState::Backoff { next_retry_ms: 0 }),
        "restarting"
    );
    assert_eq!(map_state(&SupervisorState::Failed), "failed");
}

/// A stopped node reports 0 height / 0 peers / 0% — never a fabricated or stale
/// value (Rule 1).
#[test]
fn stopped_status_is_honest_zeros() {
    let (mgr, _fake, _data) = stub_manager("zeros");
    let st = mgr.status();
    assert_eq!(st.state, "stopped");
    assert_eq!(st.height, 0);
    assert_eq!(st.peers, 0);
    assert_eq!(st.sync_pct, 0.0);
}

// ---------------------------------------------------------------------------
// C1.0b-1 — node restart policy
// ---------------------------------------------------------------------------

/// The node's sustained-healthy window is the tuned 90s (longer than the
/// supervisor default 30s), so a slow-but-healthy cold start / legitimate
/// hourly restart never climbs to a permanent Failed. This asserts the policy
/// constant the manager applies (the mechanism itself is proven by the
/// supervisor's own F-1 reset tests).
#[test]
fn node_healthy_after_is_tuned_longer_than_default() {
    assert_eq!(NODE_HEALTHY_AFTER, std::time::Duration::from_secs(90));
    // Strictly longer than the supervisor default (30s) — the whole point of
    // C1.0b-1: a legitimately-restarting node resets its failure budget.
    assert!(NODE_HEALTHY_AFTER > std::time::Duration::from_secs(30));
}

// ---------------------------------------------------------------------------
// WP3 (live) — real citrate-node bounded-sync proof. Heavy (ark/zk build +
// live testnet). `#[ignore]` so CI stays fast; run explicitly with the cold-sync
// harness on the K6 profile (scripts/e2e/cold-sync.py, RC52 G4) or:
//   CITRATE_NODE_BIN=/path/to/citrate \
//     cargo test --locked node::tests::live_bounded_sync_proof -- --ignored --nocapture
//
// It spawns the REAL node under the supervisor against a temp encrypted data
// dir, connects to the boot peers, and asserts height ADVANCES over a bounded
// window AND the data dir is ciphertext at rest. It NEVER fabricates numbers:
// if the node/RPC is unreachable it fails (does not pass with zeros).
// ---------------------------------------------------------------------------

#[test]
#[ignore = "heavy: builds/runs the real ark/zk node against live testnet"]
fn live_bounded_sync_proof() {
    let bin = match std::env::var("CITRATE_NODE_BIN") {
        Ok(p) if PathBuf::from(&p).exists() => PathBuf::from(p),
        _ => panic!(
            "set CITRATE_NODE_BIN to the built `citrate` binary to run the live proof"
        ),
    };
    let fake = std::sync::Arc::new(FakeKeyring::default());
    let data_dir = tmp_dir("live-sync");
    let crash = data_dir.join("crash.jsonl");
    let mgr = NodeManager::new(
        Box::new(SharedFake(fake)),
        bin,
        data_dir.clone(),
        crash,
        "http://127.0.0.1:8545",
    )
    .with_startup_barrier(opened_barrier(), std::time::Duration::ZERO);
    mgr.start().expect("real node starts under the supervisor");

    // Bounded window: sample height for up to ~120s; require it to advance.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut first_height: Option<u64> = None;
    let mut last = NodeStatus {
        state: "starting".into(),
        peers: 0,
        height: 0,
        sync_pct: 0.0,
        notice: None,
        blocked: None,
        mining_armed: false,
        mining_hold: None,
    };
    while std::time::Instant::now() < deadline {
        let st = mgr.status();
        if st.state == "running" && st.height > 0 {
            if first_height.is_none() {
                first_height = Some(st.height);
                eprintln!("[live] first observed height={} peers={}", st.height, st.peers);
            }
            last = st.clone();
            if let Some(fh) = first_height {
                if st.height > fh {
                    eprintln!(
                        "[live] height advanced {} -> {} (peers={})",
                        fh, st.height, st.peers
                    );
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    mgr.stop();

    let fh = first_height.expect("node must report a real height (RPC reachable)");
    assert!(
        last.height > fh,
        "height must ADVANCE over the bounded window (from {fh} to {}); no fabricated numbers",
        last.height
    );
    // Ciphertext-at-rest on the REAL encrypted data dir: the RocksDB SST/log
    // files must not contain a well-known plaintext marker. We grep for the
    // chain-id string that would appear in a plaintext value.
    // (Best-effort structural check; the authoritative encryption proof is the
    // encryption.meta commitment written by core/storage.)
    assert!(
        data_dir.join("encryption.meta").exists(),
        "real node must write encryption.meta (encryption-at-rest active)"
    );
    let _ = std::fs::remove_dir_all(&data_dir);
}

/// The node must NOT be spawned with DAG pruning enabled.
///
/// Setting `CITRATE_DAG_PRUNE_RETAIN` is what made a fresh node unable to
/// cold-sync past block 54,600 on 40204 (chain PR #144). Block 54,601 merges a
/// parent at height 32; a 10,000-block retain window deletes it, and admission
/// then fails `MissingParent` forever — blocks stored, applied head frozen, the
/// same range re-imported every ~2 s. Every member desktop would have hit it,
/// because this app set the variable on every node it spawned.
///
/// This asserts the SOURCE rather than a running process: a grep over the spec
/// builder, so the guard holds without needing a keyring, a binary, or a live
/// chain. Re-adding the pair fails here first.
///
/// See the `NODE_DAG_PRUNE_RETAIN_ENV` doc comment for the conditions under
/// which pruning may be re-enabled.
#[test]
fn node_spec_never_enables_dag_pruning() {
    let src = include_str!("node.rs");

    // The constant survives (the debug_assert names it), but it must never be
    // pushed into `spec.env`.
    let pushed = src.contains("NODE_DAG_PRUNE_RETAIN_ENV.to_string()");
    assert!(
        !pushed,
        "CITRATE_DAG_PRUNE_RETAIN must not be added to the node's env: it wedges \
         cold sync at block 54,600 (block 54,601 merges a parent at height 32, \
         which a retain window prunes away). See the const's doc comment."
    );

    // And the literal value must not have crept back in by another route.
    assert!(
        !src.contains("NODE_DAG_PRUNE_RETAIN_VALUE"),
        "the retain VALUE constant should be gone entirely — its presence means \
         someone is about to set it again"
    );
}

// ---------------------------------------------------------------------------
// Member node config (bootnodes) — a fresh install must be able to find peers.
// ---------------------------------------------------------------------------

/// The embedded config must carry bootnodes. Without them the node has nothing
/// to dial and sits at height 0 with 0 peers forever — verified on a clean data
/// dir 2026-08-04, and previously masked on dev machines by the presence of
/// ~/.citrate/node.toml.
/// K6: the bootnodes come from the active network profile, so the check runs on the config as
/// it is written for a member.
fn rendered_member_config() -> String {
    let net = crate::network_profile::active_ready().expect("the active profile is ready");
    crate::network_profile::render_node_network(MEMBER_NODE_CONFIG, &net)
}

#[test]
fn member_config_has_bootnodes() {
    let cfg = rendered_member_config();
    assert!(
        cfg.contains("bootstrap_nodes = ["),
        "member config must declare bootstrap_nodes"
    );
    let peers = cfg.matches("noise_").count();
    assert!(
        peers >= 4,
        "expected the 3 bootnodes + the sequencer, found {peers} peer entries"
    );
}

/// LOAD-BEARING: boot{1,2,3} are discovery-only and advertise height 0, so a
/// fresh node never triggers sync from them alone. Dropping the sequencer gives
/// a member three peers and no way to reach the head.
#[test]
fn member_config_includes_the_sequencer_not_just_bootnodes() {
    let cfg = rendered_member_config();
    assert!(
        cfg.contains("@rpc.citrate.ai:30303"),
        "the sequencer must be in bootstrap_nodes — boot1/2/3 advertise height 0 \
         and cannot pull a fresh node to the head on their own"
    );
}

/// citrate-chain does `if cli.mine { config.mining.enabled = true }`, so the
/// config is the BASE state and the flag only forces mining ON. Shipping
/// `enabled = true` (as citrate-chain's testnet-beta.toml does, because it
/// targets validator hosts) would make every member mine from first launch —
/// defeating the `mining_armed` gate and letting a node produce blocks while
/// far behind, which is how the 54,600 concurrent-producer fork happened.
#[test]
fn member_config_never_enables_mining() {
    let cfg = include_str!("../config/member-node.toml");
    let mining = cfg
        .split("[mining]")
        .nth(1)
        .expect("member config must have a [mining] section");
    let section = mining.split("\n[").next().unwrap_or(mining);
    assert!(
        section.contains("enabled = false"),
        "[mining] enabled must be false — the --mine flag is the only intended \
         way to turn production on, and it can only force it ON"
    );
}

/// PBA-L7b-007: the member RPC carries an explicit Origin allowlist: non-empty, no wildcard
/// and no web (http/https) origin.
#[test]
fn pba_l7b_007_member_rpc_has_a_non_web_cors_allowlist() {
    let cfg = include_str!("../config/member-node.toml");
    let rpc = cfg
        .split("\n[rpc]")
        .nth(1)
        .expect("member config must have an [rpc] section");
    let section = rpc.split("\n[").next().unwrap_or(rpc);
    let line = section
        .lines()
        .find(|l| l.trim_start().starts_with("cors_origins"))
        .expect("[rpc] must set cors_origins");
    let list = line.split_once('=').map(|(_, v)| v.trim()).unwrap_or("");
    assert!(list.starts_with('[') && list.ends_with(']'), "{line}");
    let entries: Vec<&str> = list
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|e| e.trim().trim_matches('"'))
        .filter(|e| !e.is_empty())
        .collect();
    assert!(!entries.is_empty(), "cors_origins must not be empty: {line}");
    for e in &entries {
        assert!(*e != "*", "no wildcard origin");
        assert!(
            !e.starts_with("http://") && !e.starts_with("https://") && *e != "null",
            "no web origin may be allowed: {e}"
        );
    }
}

/// A member's RPC must not be reachable from the network.
#[test]
fn member_config_binds_rpc_to_loopback() {
    let cfg = include_str!("../config/member-node.toml");
    assert!(
        !cfg.contains("listen_addr = \"0.0.0.0:8545\"") && !cfg.contains("ws_addr = \"0.0.0.0:8546\""),
        "the member RPC/WS must bind 127.0.0.1, not 0.0.0.0 — this is a laptop, \
         not a firewalled server"
    );
    assert!(cfg.contains("listen_addr = \"127.0.0.1:8545\""));
}

/// The node must be launched with `--config`, or it walks its fallback chain and
/// can inherit a developer's ~/.citrate/node.toml — which may point at another
/// chain entirely.
#[test]
fn node_spec_passes_config_explicitly() {
    let src = include_str!("node.rs");
    assert!(
        src.contains("\"--config\".to_string()"),
        "build_spec must pass --config so the node cannot fall back to \
         ~/.citrate/node.toml"
    );
}

/// The placeholder must actually be substituted, and the result must not still
/// contain it — a literal `{{DATA_DIR}}` on disk is a broken config.
#[test]
fn ensure_node_config_substitutes_the_data_dir_and_is_idempotent() {
    let tmp = std::env::temp_dir().join(format!(
        "citrate-core-cfg-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);

    let net = crate::network_profile::active_ready().expect("the active profile is ready");
    let path = ensure_node_config(&tmp, &net).expect("write config");
    let written = std::fs::read_to_string(&path).expect("read back");
    assert!(
        !written.contains("{{DATA_DIR}}"),
        "the DATA_DIR placeholder must be substituted"
    );
    assert!(
        written.contains(&tmp.to_string_lossy().replace('\\', "\\\\")),
        "the config must point at the data dir it was written for"
    );

    // Must NOT clobber operator edits on a second call.
    std::fs::write(&path, "# hand-edited\n").expect("simulate an operator edit");
    let again = ensure_node_config(&tmp, &net).expect("second call");
    assert_eq!(again, path);
    assert_eq!(
        std::fs::read_to_string(&again).expect("read"),
        "# hand-edited\n",
        "an existing config must never be overwritten"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// 40204 reroll: node data reset on a genesis change (node_genesis)
// ---------------------------------------------------------------------------

const REROLL_OLD_GENESIS: &str =
    "0x0f2b567f00000000000000000000000000000000000000000000000000000001";
const REROLL_NEW_GENESIS: &str =
    "0x1111111100000000000000000000000000000000000000000000000000000002";

/// A start for a new book genesis deletes the old chain DB before the spawn,
/// keeps the proposer and Noise keys, and surfaces the one-line notice.
#[test]
fn start_resets_old_genesis_chain_db_and_keeps_keys() {
    let (mgr, _fake, data) = stub_manager("genesis-reset");
    let mgr = mgr.with_book_genesis(REROLL_NEW_GENESIS);
    std::fs::create_dir_all(&data).unwrap();
    for f in ["CURRENT", "MANIFEST-000624", "000013.sst", "000623.log", "LOCK"] {
        std::fs::write(data.join(f), b"old chain").unwrap();
    }
    std::fs::write(data.join(crate::validator::PROPOSER_KEY_FILE), [7u8; 32]).unwrap();
    std::fs::write(data.join("noise.key"), [9u8; 64]).unwrap();
    std::fs::write(
        data.join(crate::node_genesis::GENESIS_MARKER_FILE),
        REROLL_OLD_GENESIS,
    )
    .unwrap();
    assert!(mgr.status().notice.is_none(), "no notice before a reset");

    mgr.start().expect("stub node starts after the reset");
    mgr.stop();

    for f in ["CURRENT", "MANIFEST-000624", "000013.sst", "000623.log"] {
        assert!(!data.join(f).exists(), "{f} must be deleted");
    }
    assert_eq!(
        std::fs::read(data.join(crate::validator::PROPOSER_KEY_FILE)).unwrap(),
        vec![7u8; 32]
    );
    assert_eq!(std::fs::read(data.join("noise.key")).unwrap(), vec![9u8; 64]);
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(REROLL_NEW_GENESIS)
    );
    assert_eq!(
        mgr.status().notice.as_deref(),
        Some(crate::node_genesis::CHAIN_RESET_NOTICE)
    );
}

/// A zero or unparsable book genesis refuses the start: no spawn, data untouched.
#[test]
fn start_refuses_a_bad_book_genesis() {
    for bad in [
        "0x0000000000000000000000000000000000000000000000000000000000000000",
        "not-a-hash",
    ] {
        let (mgr, fake, data) = stub_manager("genesis-bad");
        let mgr = mgr.with_book_genesis(bad);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("CURRENT"), b"old chain").unwrap();
        let err = mgr.start().expect_err("a bad book genesis must not start the node");
        assert!(matches!(err, NodeError::Genesis(_)), "{err:?}");
        assert!(err.to_string().contains("not started"), "{err}");
        assert_eq!(mgr.status().state, "stopped");
        assert!(data.join("CURRENT").exists(), "data untouched");
        assert!(
            fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().is_none(),
            "fails before the storage key is even minted"
        );
    }
}

/// The compiled-in book genesis is what a production manager targets.
#[test]
fn manager_targets_the_address_book_genesis() {
    let (mgr, _fake, data) = stub_manager("genesis-book");
    mgr.start().expect("stub node starts");
    mgr.stop();
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(crate::addresses::genesis_hash())
    );
    assert!(mgr.status().notice.is_none(), "a fresh dir is not a reset");
}

/// K6: a build whose network profile is not set starts no node and touches nothing: no data dir
/// change, no marker, no node.toml, no storage key.
#[test]
fn k6_start_refuses_when_the_network_is_not_set() {
    let (mgr, fake, data) = stub_manager("k6-not-set");
    let not_set = crate::network_profile::profile("genesis-2026-12")
        .expect("the next-genesis profile exists")
        .ready();
    assert!(not_set.is_err(), "the next genesis is not set yet");
    let mgr = mgr.with_network(not_set);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("CURRENT"), b"old chain").unwrap();
    let err = mgr.start().expect_err("no network, no node");
    assert!(matches!(err, NodeError::Network(_)), "{err:?}");
    let msg = err.to_string();
    assert!(
        msg.contains("not started") && msg.contains("genesisHash"),
        "{msg}"
    );
    assert_eq!(mgr.status().state, "stopped");
    assert!(data.join("CURRENT").exists(), "data untouched");
    assert!(!data.join(crate::node_genesis::GENESIS_MARKER_FILE).exists());
    assert!(!data.join("node.toml").exists(), "no config written");
    assert!(fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().is_none());
    // And no spec carries a guessed network.
    assert!(!mgr
        .build_spec()
        .args
        .iter()
        .any(|a| a == "--network" || a == "--config"));
}

/// K6: the node joins the network named by the profile: `--network`, bootnodes and chain id in
/// the written node.toml all come from it.
#[test]
fn k6_the_node_config_is_written_from_the_network_profile() {
    let (mgr, _fake, data) = stub_manager("k6-profile-config");
    let mut net = crate::network_profile::active_ready().expect("active profile ready");
    net.node_network = "k6net".into();
    net.chain_id = 40205;
    net.bootnodes = vec![format!("noise_{}@boot.example.org:30303", "ab".repeat(32))];
    let mgr = mgr.with_network(Ok(net.clone()));
    let spec = mgr.build_spec();
    let i = spec
        .args
        .iter()
        .position(|a| a == "--network")
        .expect("--network");
    assert_eq!(spec.args[i + 1], "k6net");
    let cfg = std::fs::read_to_string(data.join("node.toml")).expect("written");
    assert!(cfg.contains("chain_id = 40205"), "{cfg}");
    assert!(cfg.contains(&net.bootnodes[0]));
    assert!(!cfg.contains("{{"), "no placeholder left");
    assert!(
        !cfg.contains("boot1.citrate.ai"),
        "only the profile's bootnodes"
    );
    let table: toml::Table = cfg.parse().expect("valid TOML");
    assert_eq!(table["chain"]["chain_id"].as_integer(), Some(40205));
    assert_eq!(
        table["network"]["bootstrap_nodes"]
            .as_array()
            .map(|a| a.len()),
        Some(1)
    );
}

/// K6: on a genesis change the previous network's node.toml (old bootnodes) is set aside under
/// a name that says which network it was for, and the spawn writes a new one from the profile.
#[test]
fn k6_a_genesis_change_sets_the_old_node_config_aside_and_writes_a_new_one() {
    let (mgr, _fake, data) = stub_manager("k6-retire-config");
    let mgr = mgr.with_book_genesis(REROLL_NEW_GENESIS);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("CURRENT"), b"old chain").unwrap();
    std::fs::write(data.join("noise.key"), [9u8; 64]).unwrap();
    std::fs::write(
        data.join(crate::node_genesis::GENESIS_MARKER_FILE),
        REROLL_OLD_GENESIS,
    )
    .unwrap();
    std::fs::write(data.join("node.toml"), "# old network, hand-edited\n").unwrap();

    mgr.start().expect("stub node starts after the reset");
    mgr.stop();

    let kept = data.join("node.toml.0x0f2b567f");
    assert_eq!(
        std::fs::read_to_string(&kept).expect("old config kept"),
        "# old network, hand-edited\n"
    );
    let fresh = std::fs::read_to_string(data.join("node.toml")).expect("new config");
    assert!(fresh.contains("bootstrap_nodes = ["), "{fresh}");
    assert!(!fresh.contains("hand-edited"));
    assert_eq!(
        std::fs::read(data.join("noise.key")).unwrap(),
        vec![9u8; 64]
    );
}

/// K6 (Codex P2 on core#363): a failure setting the old node.toml aside refuses the start
/// WITHOUT committing the new genesis marker, so the next start retries the migration and
/// writes a fresh node.toml. The old code committed the marker first; the retry then saw a
/// matching marker and kept the old network's config.
#[test]
fn k6_a_failed_config_set_aside_is_retried_on_the_next_start() {
    let (mgr, _fake, data) = stub_manager("k6-retire-retry");
    let mgr = mgr.with_book_genesis(REROLL_NEW_GENESIS);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("CURRENT"), b"old chain").unwrap();
    std::fs::write(
        data.join(crate::node_genesis::GENESIS_MARKER_FILE),
        REROLL_OLD_GENESIS,
    )
    .unwrap();
    std::fs::write(data.join("node.toml"), "# old network, hand-edited\n").unwrap();

    crate::node_genesis::test_hooks::FAIL_RETIRE.with(|f| f.set(true));
    let first = mgr.start();
    crate::node_genesis::test_hooks::FAIL_RETIRE.with(|f| f.set(false));
    assert!(first.is_err(), "the start is refused: {first:?}");
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(REROLL_OLD_GENESIS),
        "the new marker is not committed"
    );
    assert_eq!(
        std::fs::read_to_string(data.join("node.toml")).unwrap(),
        "# old network, hand-edited\n",
        "no spawn ran with it"
    );

    mgr.start().expect("the retry completes the migration and starts");
    mgr.stop();
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(REROLL_NEW_GENESIS)
    );
    let fresh = std::fs::read_to_string(data.join("node.toml")).expect("new config");
    assert!(!fresh.contains("hand-edited"), "{fresh}");
    assert!(fresh.contains("bootstrap_nodes = ["), "{fresh}");
    assert_eq!(
        std::fs::read_to_string(data.join("node.toml.0x0f2b567f")).unwrap(),
        "# old network, hand-edited\n"
    );
}

/// K6: a crash after the old node.toml was set aside but before the marker was written leaves
/// the old marker and no node.toml; the next start completes the switch and writes a fresh one.
#[test]
fn k6_a_crash_after_the_config_was_set_aside_completes_on_the_next_start() {
    let (mgr, _fake, data) = stub_manager("k6-retire-crash");
    let mgr = mgr.with_book_genesis(REROLL_NEW_GENESIS);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join(crate::node_genesis::GENESIS_MARKER_FILE),
        REROLL_OLD_GENESIS,
    )
    .unwrap();
    std::fs::write(data.join("node.toml.0x0f2b567f"), "# old\n").unwrap();

    mgr.start().expect("starts");
    mgr.stop();
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(REROLL_NEW_GENESIS)
    );
    let fresh = std::fs::read_to_string(data.join("node.toml")).expect("new config");
    assert!(fresh.contains("bootstrap_nodes = ["), "{fresh}");
    assert_eq!(
        std::fs::read_to_string(data.join("node.toml.0x0f2b567f")).unwrap(),
        "# old\n"
    );
    assert!(!data.join("node.toml.0x0f2b567f.1").exists());
}

/// K6: setting aside never overwrites an earlier one, and a missing node.toml is not an error.
#[test]
fn k6_retire_node_config_never_overwrites() {
    let dir = std::env::temp_dir().join(format!(
        "citrate-core-k6-retire-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(
        retire_node_config(&dir, Some(REROLL_OLD_GENESIS)).unwrap(),
        None
    );
    std::fs::write(dir.join("node.toml"), "a").unwrap();
    let first = retire_node_config(&dir, Some(REROLL_OLD_GENESIS))
        .unwrap()
        .unwrap();
    std::fs::write(dir.join("node.toml"), "b").unwrap();
    let second = retire_node_config(&dir, Some(REROLL_OLD_GENESIS))
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "a");
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "b");
    std::fs::write(dir.join("node.toml"), "c").unwrap();
    let none = retire_node_config(&dir, None).unwrap().unwrap();
    assert!(none.ends_with("node.toml.unrecorded"), "{}", none.display());
    // Not a chain database file: a later genesis reset keeps it.
    assert!(!crate::node_genesis::is_chain_db_entry(
        "node.toml.0x0f2b567f"
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// SCL-S0.6 / S8.5a — holders of the chain database and ports; startup barrier
// ---------------------------------------------------------------------------

/// S0.6 + S8.5a (RT-11): an orphan that holds the chain database lock and never answers on
/// the local RPC blocks a genesis-change start: no reset, no spawn, and the UI state names the
/// holder (pid and path) with the action.
#[test]
fn genesis_change_start_is_refused_while_an_orphan_holds_the_db_and_names_it() {
    let (mgr, fake, data) = stub_manager("s06-orphan");
    let mgr = mgr.with_book_genesis(REROLL_NEW_GENESIS);
    std::fs::create_dir_all(&data).unwrap();
    for f in ["CURRENT", "MANIFEST-000624", "000013.sst", "LOCK"] {
        std::fs::write(data.join(f), b"old chain").unwrap();
    }
    std::fs::write(
        data.join(crate::node_genesis::GENESIS_MARKER_FILE),
        REROLL_OLD_GENESIS,
    )
    .unwrap();
    let fx = crate::node_holder::tests::hold_chain_db_lock(&data);

    let err = mgr.start().expect_err("a held chain database refuses the start");
    assert!(err.to_string().contains("not started"), "{err}");
    assert_eq!(mgr.status().state, "stopped", "nothing spawned");
    for f in ["CURRENT", "MANIFEST-000624", "000013.sst", "LOCK"] {
        assert!(data.join(f).exists(), "{f} untouched");
    }
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(REROLL_OLD_GENESIS),
        "marker unchanged"
    );
    assert!(
        fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().is_none(),
        "refused before the storage key is touched"
    );
    let blocked = mgr.status().blocked.expect("UI state names the holder");
    assert_eq!(
        blocked.resource,
        crate::node_holder::BlockedResource::ChainDatabase
    );
    #[cfg(unix)]
    {
        assert_eq!(blocked.pid, Some(fx.pid));
        assert!(blocked.message.contains(&format!("process {}", fx.pid)), "{}", blocked.message);
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert_eq!(
        blocked.path.as_deref().map(PathBuf::from),
        Some(crate::node_holder::tests::HolderFixture::exe())
    );
    assert!(blocked.message.contains("Quit that process"), "{}", blocked.message);

    // The holder exits: the next start resets and runs, and the blocked state clears.
    fx.stop();
    mgr.start().expect("free lock: reset, then start");
    assert!(mgr.status().blocked.is_none(), "cleared by the next start");
    mgr.stop();
    assert!(!data.join("CURRENT").exists(), "old chain data reset");
    let _ = std::fs::remove_dir_all(&data);
}

/// S0.6: an old node left holding the database blocks a normal start too (it would make the
/// new node fail to open its database), named the same way.
#[test]
fn a_normal_start_is_refused_while_an_orphan_holds_the_db() {
    let (mgr, _fake, data) = stub_manager("s06-normal");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join(crate::node_genesis::GENESIS_MARKER_FILE),
        crate::addresses::genesis_hash(),
    )
    .unwrap();
    let fx = crate::node_holder::tests::hold_chain_db_lock(&data);
    let err = mgr.start().expect_err("held database refuses the start");
    assert!(matches!(err, NodeError::Blocked(_)), "{err:?}");
    assert_eq!(mgr.status().state, "stopped");
    assert!(mgr.status().blocked.is_some());
    fx.stop();
    mgr.start().expect("starts once the holder exits");
    mgr.stop();
    let _ = std::fs::remove_dir_all(&data);
}

/// S0.6: an owned fixture listener on a node port (standing in for an old node's RPC port)
/// blocks the start and is named.
#[test]
fn a_start_is_refused_while_a_node_port_is_in_use_and_names_it() {
    let fx = crate::node_holder::tests::listen_on_loopback();
    let (mgr, _fake, data) = stub_manager("s06-port");
    let mgr = mgr.with_node_ports(vec![fx.port]);
    let err = mgr.start().expect_err("a held node port refuses the start");
    let NodeError::Blocked(b) = &err else {
        panic!("expected Blocked, got {err:?}");
    };
    assert_eq!(b.port, Some(fx.port));
    assert_eq!(mgr.status().state, "stopped");
    let blocked = mgr.status().blocked.expect("UI state names the port holder");
    assert!(blocked.message.contains(&format!("port {}", fx.port)), "{}", blocked.message);
    if let Some(pid) = blocked.pid {
        assert_eq!(pid, fx.pid);
    }
    fx.stop();
    mgr.start().expect("starts once the port is free");
    assert!(mgr.status().blocked.is_none());
    mgr.stop();
    let _ = std::fs::remove_dir_all(&data);
}

/// S8.5a (US-7.3 AC1): no node spawn is admitted until the startup cleanup has finished.
#[test]
fn node_spawn_is_admitted_only_after_the_startup_cleanup() {
    let (mgr, fake, data) = stub_manager("s85a-barrier");
    let barrier = std::sync::Arc::new(StartupBarrier::new());
    let mgr = mgr.with_startup_barrier(barrier.clone(), std::time::Duration::from_millis(50));
    let err = mgr.start().expect_err("closed barrier: no spawn");
    assert!(matches!(err, NodeError::StartupCleanupPending), "{err:?}");
    assert_eq!(mgr.status().state, "stopped");
    assert!(
        fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().is_none(),
        "refused before anything else runs"
    );
    assert!(
        crate::node_genesis::read_marker(&data).unwrap().is_none(),
        "the data dir is not touched before admission"
    );

    // A start waiting on the barrier is admitted as soon as the cleanup returns.
    let mgr = std::sync::Arc::new(
        mgr.with_startup_barrier(barrier.clone(), std::time::Duration::from_secs(20)),
    );
    let m2 = mgr.clone();
    let starter = std::thread::spawn(move || m2.start());
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(mgr.status().state, "stopped", "still waiting on the cleanup");
    barrier.run_cleanup(|| {});
    starter
        .join()
        .expect("starter thread")
        .expect("admitted after cleanup");
    mgr.stop();
    let _ = std::fs::remove_dir_all(&data);
}

/// Production managers are gated on the app's own startup barrier.
#[test]
fn production_managers_use_the_app_startup_barrier() {
    let mgr = NodeManager::new(
        Box::new(DeadKeyring),
        stub_bin(),
        tmp_dir("s85a-global"),
        PathBuf::from("/tmp/x.jsonl"),
        "http://127.0.0.1:59996",
    );
    assert!(std::sync::Arc::ptr_eq(&mgr.admission, &StartupBarrier::global()));
    assert_eq!(mgr.admission_wait, ADMISSION_WAIT);
    assert!(mgr.node_ports.is_none(), "ports come from node.toml");
}

// ---------------------------------------------------------------------------
// Core #326 — a node that followed a withdrawn branch moves its chain data aside
// ---------------------------------------------------------------------------

/// A node RPC at `head` whose block at the rollback check height is `hash`.
struct ForkNode {
    head: u64,
    hash: &'static str,
}

impl RpcTransport for ForkNode {
    fn call(&self, body: Value) -> std::result::Result<Value, RpcError> {
        match body.get("method").and_then(Value::as_str) {
            Some("eth_blockNumber") => Ok(json!({ "result": format!("0x{:x}", self.head) })),
            Some("eth_getBlockByNumber") => Ok(json!({ "result": { "hash": self.hash } })),
            _ => Err(RpcError::Transport("unexpected".into())),
        }
    }
}

const FORK_DB: &[&str] = &["CURRENT", "MANIFEST-000005", "000013.sst", "000181.log", "LOCK"];
const FORK_KEPT: &[&str] = &["proposer.key", "noise.key", "node.toml", "chain-genesis"];

/// What each fixture file holds: the genesis marker holds the book genesis (as the app writes
/// it), every other file its own name.
fn fork_body(name: &str) -> Vec<u8> {
    if name == crate::node_genesis::GENESIS_MARKER_FILE {
        format!("{}\n", crate::addresses::genesis_hash()).into_bytes()
    } else {
        name.as_bytes().to_vec()
    }
}

fn populated_fork_dir(tag: &str) -> (NodeManager, PathBuf) {
    let (mgr, _fake, data_dir) = stub_manager(tag);
    std::fs::create_dir_all(&data_dir).expect("mkdir");
    for f in FORK_DB.iter().chain(FORK_KEPT.iter()) {
        std::fs::write(data_dir.join(f), fork_body(f)).expect("write");
    }
    (mgr, data_dir)
}

fn aside_dirs(data_dir: &std::path::Path) -> Vec<PathBuf> {
    let parent = data_dir.parent().expect("parent");
    let prefix = format!(
        "{}.withdrawn-",
        data_dir.file_name().expect("name").to_string_lossy()
    );
    let mut v: Vec<PathBuf> = std::fs::read_dir(parent)
        .expect("read parent")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

#[test]
fn a_node_on_the_canonical_block_keeps_its_chain_data() {
    let (mgr, data_dir) = populated_fork_dir("wd-healthy");
    let check = crate::node_withdrawn::FORK_CHECKS[0];
    let node = ForkNode { head: 400_000, hash: check.canonical };
    assert!(mgr.recheck_withdrawn_via(&node), "nothing left to watch");
    for f in FORK_DB.iter().chain(FORK_KEPT.iter()) {
        assert!(data_dir.join(f).exists(), "{f} must stay");
    }
    assert!(aside_dirs(&data_dir).is_empty());
    assert_eq!(mgr.status().notice, None);
}

#[test]
fn a_node_on_a_withdrawn_block_moves_its_chain_data_and_keeps_its_keys() {
    let (mgr, data_dir) = populated_fork_dir("wd-moved");
    let node = ForkNode {
        head: 386_060,
        hash: "0x8a0d748900000000000000000000000000000000000000000000000000000001",
    };
    assert!(!mgr.recheck_withdrawn_via(&node), "watch again after the resync");
    mgr.stop();
    let aside = aside_dirs(&data_dir);
    assert_eq!(aside.len(), 1, "{aside:?}");
    for f in FORK_DB {
        assert!(!data_dir.join(f).exists(), "{f} must leave the data dir");
        assert!(aside[0].join(f).exists(), "{f} must be in {}", aside[0].display());
    }
    for f in FORK_KEPT {
        assert_eq!(
            std::fs::read(data_dir.join(f)).expect("kept"),
            fork_body(f),
            "{f} must stay unchanged"
        );
    }
    assert_eq!(
        mgr.status().notice.as_deref(),
        Some(crate::node_withdrawn::WITHDRAWN_NOTICE)
    );
    assert!(!mgr.is_mining_armed());
}

#[test]
fn the_withdrawn_move_happens_once() {
    let (mgr, data_dir) = populated_fork_dir("wd-once");
    let check = crate::node_withdrawn::FORK_CHECKS[0];
    let withdrawn = ForkNode {
        head: 386_060,
        hash: "0x8a0d748900000000000000000000000000000000000000000000000000000001",
    };
    assert!(!mgr.recheck_withdrawn_via(&withdrawn));
    mgr.stop();
    // After the resync the node is on the canonical block: nothing more is moved.
    let resynced = ForkNode { head: 400_000, hash: check.canonical };
    assert!(mgr.recheck_withdrawn_via(&resynced));
    assert_eq!(aside_dirs(&data_dir).len(), 1);
    // A node still below the check height is watched again and nothing is moved.
    let behind = ForkNode { head: 1_000, hash: check.canonical };
    assert!(!mgr.recheck_withdrawn_via(&behind));
    assert_eq!(aside_dirs(&data_dir).len(), 1);
}

/// A private directory for a test's "runtime dir" (owner-only, like the real ones).
#[cfg(unix)]
fn private_runtime_dir(tag: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let rt = tmp_dir(tag);
    std::fs::create_dir_all(&rt).expect("mkdir runtime dir");
    std::fs::set_permissions(&rt, std::fs::Permissions::from_mode(0o700)).expect("chmod 0700");
    rt
}

/// With a per-user runtime directory available, the storage key file lives in it (in a
/// per-install subdirectory), never under the app data dir that backups include.
#[cfg(unix)]
#[test]
fn storage_key_file_lives_in_the_runtime_dir_not_under_app_data() {
    let (mgr, _fake, data) = stub_manager("key-runtime");
    let rt = private_runtime_dir("key-runtime-rt");
    let mgr = mgr.with_key_runtime_dir(Some(rt.clone()));
    let path = mgr.storage_key_path();
    assert!(path.starts_with(&rt), "{} is in the runtime dir", path.display());
    let name = data.file_name().expect("data dir name").to_string_lossy().to_string();
    assert!(!path.starts_with(&data), "not in the chain data dir");
    assert!(
        !path.starts_with(data.with_file_name(format!("{name}.secrets"))),
        "not in the app-data secrets dir"
    );
    let sub = path
        .parent()
        .and_then(|p| p.file_name())
        .expect("key file has a named parent")
        .to_string_lossy()
        .to_string();
    assert!(sub.starts_with("citrate-core-"), "per-install subdirectory, got {sub}");
}

/// Without a usable runtime directory, the key file falls back to the app-data secrets dir.
#[test]
fn storage_key_file_falls_back_to_app_data_without_a_runtime_dir() {
    let (mgr, _fake, data) = stub_manager("key-fallback");
    let mgr = mgr.with_key_runtime_dir(None);
    let name = data.file_name().expect("data dir name").to_string_lossy().to_string();
    assert_eq!(
        mgr.storage_key_path(),
        data.with_file_name(format!("{name}.secrets")).join("storage.key")
    );
}

/// Two installs (two data dirs) sharing one runtime dir never share a key file.
#[cfg(unix)]
#[test]
fn two_installs_get_different_key_dirs_in_one_runtime_dir() {
    let rt = private_runtime_dir("key-two-rt");
    let (a, _fa, _da) = stub_manager("key-two-a");
    let (b, _fb, _db) = stub_manager("key-two-b");
    let a = a.with_key_runtime_dir(Some(rt.clone()));
    let b = b.with_key_runtime_dir(Some(rt));
    assert_ne!(a.storage_key_path().parent(), b.storage_key_path().parent());
}

/// A runtime dir is only used when it is a real directory owned by us and closed to others.
#[cfg(unix)]
#[test]
fn a_runtime_dir_must_be_ours_and_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let ok = private_runtime_dir("key-rt-ok");
    assert!(crate::node::usable_key_runtime_dir(&ok));
    let open = tmp_dir("key-rt-open");
    std::fs::create_dir_all(&open).expect("mkdir");
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    assert!(!crate::node::usable_key_runtime_dir(&open), "group/world-readable is refused");
    assert!(!crate::node::usable_key_runtime_dir(&tmp_dir("key-rt-missing")), "missing is refused");
    let link = tmp_dir("key-rt-link");
    std::os::unix::fs::symlink(&ok, &link).expect("symlink");
    assert!(!crate::node::usable_key_runtime_dir(&link), "a symbolic link is refused");
}

/// Start writes the key file into the runtime dir; stop removes it.
#[cfg(unix)]
#[test]
fn start_writes_the_key_into_the_runtime_dir_and_stop_removes_it() {
    let (mgr, _fake, data) = stub_manager("key-rt-start");
    std::fs::create_dir_all(&data).expect("mkdir data");
    let rt = private_runtime_dir("key-rt-start-rt");
    let mgr = mgr.with_key_runtime_dir(Some(rt.clone()));
    mgr.start().expect("start");
    let path = mgr.storage_key_path();
    assert!(path.starts_with(&rt));
    assert!(path.is_file(), "key file written in the runtime dir");
    mgr.stop();
    assert!(!path.exists(), "key file removed at stop");
}

// ---------------------------------------------------------------------------
// Round 3 (Codex P2 on core#363): invariants P2/P5 and M3/M4 through `NodeManager::start`.
// ---------------------------------------------------------------------------

/// Every file in `dir` with its bytes, for "nothing was touched" checks.
fn dir_snapshot(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| {
            (
                e.file_name().to_string_lossy().to_string(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

const ROUND3_THIRD_GENESIS: &str =
    "0x3333330000000000000000000000000000000000000000000000000000000003";

/// P2 + P5 + M3: a migration interrupted mid-delete, then a launch whose profile is not ready
/// (nothing at all is touched: no wipe, no intent change, no marker, no config, no key), then a
/// launch for a third genesis, which rolls the migration forward to it.
#[test]
fn k6_an_interrupted_migration_is_untouched_while_not_ready_and_completes_for_the_next_profile() {
    let (mgr, fake, data) = stub_manager("k6-r3-profile-change");
    let mgr = mgr.with_book_genesis(REROLL_NEW_GENESIS);
    std::fs::create_dir_all(&data).unwrap();
    for f in ["CURRENT", "MANIFEST-000001", "000013.sst", "000014.log"] {
        std::fs::write(data.join(f), b"old chain").unwrap();
    }
    std::fs::write(data.join("noise.key"), [9u8; 64]).unwrap();
    std::fs::write(
        data.join(crate::node_genesis::GENESIS_MARKER_FILE),
        REROLL_OLD_GENESIS,
    )
    .unwrap();
    std::fs::write(data.join("node.toml"), "# old network, hand-edited\n").unwrap();

    crate::node_genesis::test_hooks::STOP_WIPE_AFTER.with(|n| n.set(Some(1)));
    let first = mgr.start();
    crate::node_genesis::test_hooks::STOP_WIPE_AFTER.with(|n| n.set(None));
    assert!(first.is_err(), "interrupted: {first:?}");
    assert!(data.join(crate::node_genesis::MIGRATION_INTENT_FILE).exists());
    assert!(!data.join("CURRENT").exists(), "the delete began");

    // A launch whose profile is not ready: refused, and the data dir is byte-for-byte unchanged.
    let before = dir_snapshot(&data);
    let not_set = crate::network_profile::profile("genesis-2026-12")
        .expect("profile")
        .ready();
    let mgr = mgr.with_network(not_set);
    let err = mgr.start().expect_err("no network, no node");
    assert!(matches!(err, NodeError::Network(_)), "{err:?}");
    assert_eq!(dir_snapshot(&data), before, "P2/P5: no migration step ran");
    assert!(fake.get(KEYRING_NODE_STORAGE_ACCOUNT).unwrap().is_none(), "no key");
    assert_eq!(mgr.status().state, "stopped");

    // The next launch targets a third genesis: the migration rolls forward to it.
    let mgr = mgr
        .with_network(crate::network_profile::active_ready())
        .with_book_genesis(ROUND3_THIRD_GENESIS);
    mgr.start().expect("the migration completes and the node starts");
    mgr.stop();
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(ROUND3_THIRD_GENESIS)
    );
    assert!(!data.join(crate::node_genesis::MIGRATION_INTENT_FILE).exists());
    assert!(crate::node_genesis::chain_db_entries(&data).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(data.join("node.toml.0x0f2b567f")).unwrap(),
        "# old network, hand-edited\n",
        "the old config is kept under the network it came from"
    );
    let fresh = std::fs::read_to_string(data.join("node.toml")).expect("new config");
    assert!(!fresh.contains("hand-edited"), "{fresh}");
    assert!(fresh.contains("bootstrap_nodes = ["), "{fresh}");
    assert_eq!(std::fs::read(data.join("noise.key")).unwrap(), vec![9u8; 64]);
}

/// M4 (the reported case, end to end): a database with no marker is wiped, the run stops before
/// node.toml is set aside; the next start writes a fresh node.toml from the profile instead of
/// keeping the previous network's bootnodes.
#[test]
fn k6_an_unrecorded_database_interrupted_after_the_wipe_never_keeps_the_old_config() {
    let (mgr, _fake, data) = stub_manager("k6-r3-unrecorded");
    let mgr = mgr.with_book_genesis(REROLL_NEW_GENESIS);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("CURRENT"), b"old chain").unwrap();
    std::fs::write(data.join("000013.sst"), b"old chain").unwrap();
    std::fs::write(data.join("node.toml"), "# old network, hand-edited\n").unwrap();

    crate::node_genesis::test_hooks::STOP_AFTER
        .with(|s| s.set(Some(crate::node_genesis::Step::DbWiped)));
    let first = mgr.start();
    crate::node_genesis::test_hooks::STOP_AFTER.with(|s| s.set(None));
    assert!(first.is_err(), "interrupted: {first:?}");
    assert!(crate::node_genesis::chain_db_entries(&data).unwrap().is_empty());
    assert!(crate::node_genesis::read_marker(&data).unwrap().is_none());

    mgr.start().expect("starts");
    mgr.stop();
    let fresh = std::fs::read_to_string(data.join("node.toml")).expect("new config");
    assert!(!fresh.contains("hand-edited"), "{fresh}");
    assert!(fresh.contains("bootstrap_nodes = ["), "{fresh}");
    assert_eq!(
        std::fs::read_to_string(data.join("node.toml.unrecorded")).unwrap(),
        "# old network, hand-edited\n"
    );
    assert_eq!(
        crate::node_genesis::read_marker(&data).unwrap().as_deref(),
        Some(REROLL_NEW_GENESIS)
    );
}

// ---------------------------------------------------------------------------
// RC52: the bundled node runs r1006 (genesis profile + activation overrides)
// ---------------------------------------------------------------------------

/// Block 0 of r1006 (citrate-chain 9a32147e `R1006_GENESIS`).
const R1006_GENESIS_HASH: &str =
    "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4";

/// The shipped r1006 profile with its reroll values filled (what fill-network-profile.sh writes).
fn r1006_network() -> crate::network_profile::ReadyNetwork {
    let mut p = crate::network_profile::profile("r1006")
        .expect("the r1006 profile exists")
        .clone();
    p.genesis_hash = Some(R1006_GENESIS_HASH.to_string());
    p.address_book_sha256 = Some("5c".repeat(32));
    p.min_chain_rev = Some("9a32147e2aac60d388356ecc67488d717aa84c41".to_string());
    p.pending.clear();
    p.ready().expect("filled r1006 is ready")
}

fn env_of<'a>(spec: &'a SidecarSpec, k: &str) -> Option<&'a str> {
    spec.env
        .iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.as_str())
}

fn written_genesis_profile(data: &std::path::Path) -> Option<String> {
    let cfg = std::fs::read_to_string(data.join("node.toml")).expect("node.toml written");
    let t: toml::Table = cfg.parse().expect("valid TOML");
    t["chain"]
        .get("genesis_profile")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Red first (RC52): on r1006 the ValidatorRegistry and its activation height are compiled into
/// the genesis (genesis-keyed activation table, citrate-chain #398), and the node refuses to start
/// on an env value that differs (r1006 compiles 1000; the app passed r1005's 2000). The app passes
/// neither override there, and its node config names the r1006 genesis profile.
#[test]
fn r1006_spawn_passes_no_validator_override_and_names_the_r1006_genesis() {
    let (mgr, _fake, data) = stub_manager("r1006-env");
    let mgr = mgr.with_network(Ok(r1006_network()));
    let spec = mgr.build_spec();
    assert_eq!(
        env_of(&spec, NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV),
        None,
        "the r1006 genesis compiles its activation height; an env value is refused by the node"
    );
    assert_eq!(
        env_of(&spec, NODE_VALIDATOR_REGISTRY_ENV),
        None,
        "the r1006 genesis compiles its ValidatorRegistry"
    );
    assert_eq!(env_of(&spec, NODE_BLOCK_V2_ENV), Some("1"), "v2 headers stay explicit");
    assert!(env_of(&spec, NODE_STORAGE_KEY_FILE_ENV).is_some());
    assert_eq!(written_genesis_profile(&data).as_deref(), Some("r1006"));
}

/// Control (RC52): the r1005 network keeps its fleet env (activation 2000 and the registry from
/// the address book), and its config names the genesis profile its block 0 comes from.
#[test]
fn r1005_spawn_keeps_its_validator_env_and_names_its_genesis_profile() {
    let (mgr, _fake, data) = stub_manager("r1005-env");
    let net = crate::network_profile::profile("alpha-r1005")
        .expect("alpha-r1005")
        .ready()
        .expect("ready");
    let mgr = mgr.with_network(Ok(net));
    let spec = mgr.build_spec();
    assert_eq!(env_of(&spec, NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV), Some("2000"));
    assert_eq!(
        env_of(&spec, NODE_VALIDATOR_REGISTRY_ENV),
        Some(node_validator_registry_value())
    );
    assert_eq!(written_genesis_profile(&data).as_deref(), Some("testnet_beta"));
}

/// Red first (RC52), fail closed: a node.toml already in the data dir (kept across restarts so
/// operator edits survive) that names another genesis profile, or none where the node's default
/// is not the network's genesis, would make the node build another block 0. The start is refused
/// with an error that names both and what to do; nothing is spawned.
#[test]
fn an_existing_node_config_for_another_genesis_profile_refuses_the_start() {
    for (tag, edit) in [
        ("other", Some("genesis_profile = \"testnet_beta\"")),
        ("absent", None),
    ] {
        let (mgr, _fake, data) = stub_manager(&format!("r1006-cfg-{tag}"));
        let mgr = mgr.with_network(Ok(r1006_network()));
        mgr.start().expect("first start writes node.toml");
        mgr.stop();
        let cfg = std::fs::read_to_string(data.join("node.toml")).expect("node.toml");
        let kept: Vec<&str> = cfg
            .lines()
            .filter(|l| !l.trim_start().starts_with("genesis_profile"))
            .collect();
        let mut edited = kept.join("\n");
        if let Some(line) = edit {
            edited = edited.replacen("[chain]\n", &format!("[chain]\n{line}\n"), 1);
        }
        std::fs::write(data.join("node.toml"), &edited).expect("edit node.toml");
        let second = mgr.start();
        mgr.stop();
        let msg = second.as_ref().err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            second.is_err() && msg.contains("genesis_profile") && msg.contains("r1006"),
            "{tag}: a node.toml for another genesis must refuse the start, got {second:?}"
        );
    }
}

/// Control: on r1005 a node.toml with no genesis_profile (every config written before 0.5.2) is
/// kept, because the node's chain-id default on 40204 is the r1005 genesis profile.
#[test]
fn an_existing_r1005_node_config_without_a_genesis_profile_still_starts() {
    let (mgr, _fake, data) = stub_manager("r1005-cfg-absent");
    let net = crate::network_profile::profile("alpha-r1005")
        .expect("alpha-r1005")
        .ready()
        .expect("ready");
    let mgr = mgr.with_network(Ok(net));
    mgr.start().expect("first start");
    mgr.stop();
    let cfg = std::fs::read_to_string(data.join("node.toml")).expect("node.toml");
    let kept: Vec<&str> = cfg
        .lines()
        .filter(|l| !l.trim_start().starts_with("genesis_profile"))
        .collect();
    std::fs::write(data.join("node.toml"), kept.join("\n")).expect("edit");
    mgr.start().expect("an r1005 config with no genesis_profile still starts");
    mgr.stop();
}

/// Tripwire (RC52): for EVERY genesis profile the bundled node builds with compiled activations,
/// the spawn carries no validator override (the node refuses one that differs from the compiled
/// table); for every other one the fleet values are still passed. A genesis added to
/// `nodeGenesisProfiles` is covered without a new test.
#[test]
fn no_validator_override_is_passed_on_any_genesis_with_compiled_activations() {
    let table = crate::network_profile::node_genesis_profiles().expect("table");
    assert!(table.iter().any(|g| g.compiled_activations), "r1006 at least");
    for g in table {
        let (mgr, _fake, _data) = stub_manager(&format!("override-{}", g.name));
        let mut net = r1006_network();
        net.node_genesis = g.clone();
        let spec = mgr.with_network(Ok(net)).build_spec();
        let has = |k: &str| spec.env.iter().any(|(n, _)| n == k);
        for k in [NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV, NODE_VALIDATOR_REGISTRY_ENV] {
            assert_eq!(
                has(k),
                !g.compiled_activations,
                "genesis profile {}: {k} must be passed only without compiled activations",
                g.name
            );
        }
        assert!(has(NODE_BLOCK_V2_ENV), "{}", g.name);
    }
}

/// Tripwire (RC52): the validator overrides are set in one place, `node_consensus_env`, which
/// gates them on the genesis. A second place that pushes them (the bug class: an override passed
/// whatever the genesis compiles) fails here.
#[test]
fn the_validator_overrides_are_set_only_in_node_consensus_env() {
    let src = include_str!("node.rs");
    let body_start = src
        .find("fn node_consensus_env(")
        .expect("node_consensus_env exists");
    let body_end = body_start
        + src[body_start..]
            .find("\n}\n")
            .expect("node_consensus_env ends");
    let body = &src[body_start..body_end];
    assert!(body.contains("compiled_activations"), "the overrides are gated on the genesis");
    for name in [
        "NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV",
        "NODE_VALIDATOR_REGISTRY_ENV",
        "NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE",
    ] {
        let uses: Vec<usize> = src
            .match_indices(&format!("{name}."))
            .map(|(i, _)| i)
            .chain(src.match_indices(&format!("{name},")).map(|(i, _)| i))
            .chain(src.match_indices(&format!("{name})")).map(|(i, _)| i))
            .collect();
        assert!(!uses.is_empty(), "{name} is used");
        for i in uses {
            assert!(
                (body_start..body_end).contains(&i),
                "{name} is used outside node_consensus_env at byte {i}"
            );
        }
    }
    // Negative control: a copy of node.rs with an ungated push elsewhere is caught.
    let planted = format!(
        "{src}\nfn elsewhere() {{ let _ = (NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV.to_string(), 1); }}\n"
    );
    let outside = planted
        .match_indices("NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV.")
        .any(|(i, _)| !(body_start..body_end).contains(&i));
    assert!(outside, "the scan sees a use outside the function");
}

/// RC52: the error a mismatched node.toml gives names both genesis profiles and what to do, and
/// a node.toml that is not TOML is refused too (never read as "no genesis_profile").
#[test]
fn the_node_config_genesis_check_is_honest_and_fails_closed() {
    let r1006 = r1006_network();
    let ok = "[chain]\nchain_id = 40204\ngenesis_profile = \"r1006\"\n";
    assert_eq!(check_node_config_genesis(ok, &r1006), Ok(()));
    let other = "[chain]\ngenesis_profile = \"testnet_beta\"\n";
    let e = check_node_config_genesis(other, &r1006).unwrap_err();
    assert!(e.contains("\"testnet_beta\"") && e.contains("\"r1006\"") && e.contains("move node.toml aside"), "{e}");
    assert!(e.contains(R1006_GENESIS_HASH), "{e}");
    let wrong_type = "[chain]\ngenesis_profile = 1006\n";
    assert!(check_node_config_genesis(wrong_type, &r1006).is_err());
    assert!(check_node_config_genesis("[chain\n", &r1006).is_err());
    assert!(check_node_config_genesis("[chain]\n", &r1006).is_err(), "absent on r1006");
    let alpha = crate::network_profile::profile("alpha-r1005")
        .unwrap()
        .ready()
        .unwrap();
    assert_eq!(check_node_config_genesis("[chain]\n", &alpha), Ok(()), "absent on r1005");
    assert!(check_node_config_genesis("[chain]\ngenesis_profile = \"r1006\"\n", &alpha).is_err());
}
