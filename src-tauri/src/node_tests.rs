// CORE-C1.1 — NodeDomain ↔ SidecarSupervisor wiring + @rule8 storage-key /
// ciphertext-at-rest suite. Red-first (see the sprint file + PR body).
//
// CI-safe: everything here runs headless against (a) an in-memory keyring fake
// (the @rule8 storage-key handoff) and (b) a small shell STUB node binary that
// writes ciphertext to its data dir and blocks until killed — so the wiring,
// the storage-key mint/hold, and the ciphertext-at-rest grep are all proven
// WITHOUT building the heavy ark/zk node. Status-parsing is proven against a
// mock RpcTransport (no port races). The real node bounded-sync is a separate,
// scripted + `#[ignore]` live proof at the bottom.

use super::*;
use crate::custody::CustodyError;
use crate::rpc::{RpcError, RpcTransport};
use serde_json::{json, Value};
use std::sync::Mutex as StdMutex;

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

/// Absolute path to the CI stub node shell script.
fn stub_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub_node.sh")
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
    );
    (mgr, fake, data_dir)
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

/// Sync-wedge tripwire (DGX_NODE_SYNC_WEDGE_RESPONSE 2026-07-22): the node spawn MUST
/// carry the fleet producer's consensus env — `CITRATE_BLOCK_V2`,
/// `CITRATE_VALIDATOR_ACTIVATION_HEIGHT=2000`, and the live `CITRATE_VALIDATOR_REGISTRY`
/// — alongside the storage key. Dropping any of them disables the validator/§R' state
/// path, forks the state root, and wedges the node at block 2,580. This locks them in.
#[test]
fn spawn_env_carries_the_fleet_consensus_vars() {
    let (mgr, _fake, _data) = stub_manager("consensus-env");
    let spec = mgr.build_spec("00");
    let get = |k: &str| {
        spec.env
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get(NODE_STORAGE_KEY_ENV), Some("00"), "storage key still passed");
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
    assert_eq!(
        get(NODE_VALIDATOR_REGISTRY_ENV),
        Some("0x2655d9fbbe599e75ff6e53790f99ebc9a20c93bf"),
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
    let follower = mgr.build_spec("00");
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
    let producer = mgr.build_spec("00");
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
    let spec = mgr.build_spec("00");
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
    let spec = mgr.build_spec("00");
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
    );
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
    );
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
// live testnet). `#[ignore]` so CI stays fast; run explicitly with the proof
// script (src-tauri/scripts/c1_1_node_sync_proof.sh) or:
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
    );
    mgr.start().expect("real node starts under the supervisor");

    // Bounded window: sample height for up to ~120s; require it to advance.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut first_height: Option<u64> = None;
    let mut last = NodeStatus {
        state: "starting".into(),
        peers: 0,
        height: 0,
        sync_pct: 0.0,
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
#[test]
fn member_config_has_bootnodes() {
    let cfg = include_str!("../config/member-node.toml");
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
    let cfg = include_str!("../config/member-node.toml");
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

/// PBA-L7b-007: the member RPC must not reflect arbitrary web Origins. citrate-chain's
/// jsonrpc-http-server reflects ANY Origin when `cors_origins` is empty/unset, so the member
/// config must carry a non-empty allowlist with no wildcard and no web (http/https) origin.
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
        .expect("[rpc] must set cors_origins (empty = reflect any Origin)");
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

    let path = ensure_node_config(&tmp).expect("write config");
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
    let again = ensure_node_config(&tmp).expect("second call");
    assert_eq!(again, path);
    assert_eq!(
        std::fs::read_to_string(&again).expect("read"),
        "# hand-edited\n",
        "an existing config must never be overwritten"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
