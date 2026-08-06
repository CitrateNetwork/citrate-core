//! citrate-core — NodeDomain: the real citrate-node under the SidecarSupervisor
//! (CORE-C1.1). @rule8 · encrypted data dir + keyring storage key.
//!
//! This wires the `NodeDomain { status(); start(); stop() }` bridge seam to a
//! REAL `citrate` node spawned through [`crate::supervisor::Supervisor`]:
//!
//! - **`start`** mints/loads a 32-byte storage master key from the OS keyring
//!   (the A2 custody keyring pattern — never on disk in clear), then spawns the
//!   bundled `citrate` binary under the supervisor with an explicit `--data-dir`
//!   and the storage key handed in ONLY via the `CITRATE_STORAGE_KEY` env var.
//!   The node opens its RocksDB encrypted-at-rest (AES-256-GCM per value) with
//!   that key, so a raw-disk grep of the data dir yields ciphertext.
//! - **`status`** reads the node's REAL sync state from its local JSON-RPC
//!   (`eth_blockNumber` → height, `net_peerCount` → peers) — no fabricated
//!   numbers (Rule 1). When the node is not running or its RPC is not yet up,
//!   status reflects the supervisor state honestly (Off/Starting/Backoff/…).
//! - **`stop`** releases the supervisor: SIGTERM → grace → SIGKILL, no orphan.
//!
//! ## @rule8 — where the storage key lives
//! The key is minted with `OsRng` on first `start`, stored under keyring account
//! [`KEYRING_NODE_STORAGE_ACCOUNT`] in service `ai.citrate.core` (the same
//! service the custody vault uses), and is held in memory only as a
//! [`Zeroizing`] hex string for the duration of the spawn call. It is passed to
//! the child through an env var (argv would leak it to `ps`); the child converts
//! it back to 32 raw bytes. The plaintext key is NEVER written to the data dir
//! or any config file — only the encrypted DB and the wrong-key-detecting
//! `encryption.meta` commitment land on disk.
//!
//! ## C1.0b-1 — node restart policy (decided here)
//! A citrate-node that legitimately restarts (config reload, transient network
//! blip, an OS OOM-kill under memory pressure) must NEVER be driven to a
//! permanent `Failed` state, while a genuinely broken node (bad binary, corrupt
//! data dir, instant crash-loop) must still hit the fork-bomb bound. The
//! supervisor's F-1 sliding-window design already does exactly this: the
//! consecutive-failure counter resets to 0 once a respawned child stays
//! `Running` for `healthy_after`. We therefore tune, for the node specifically,
//! a LONGER `healthy_after` (`NODE_HEALTHY_AFTER`, 90s) than the supervisor
//! default (30s): initial-sync + peer-discovery + genesis-open can take tens of
//! seconds, so 30s risked counting a slow-but-healthy boot as "not yet healthy"
//! and letting the counter climb across legitimate restarts. 90s comfortably
//! exceeds a healthy node's cold-start, so any node that reaches steady state
//! resets its failure budget; a node that cannot stay up 90s is genuinely
//! broken and is still bounded by `max_retries`. We keep the default bounded
//! exponential backoff + `max_retries` (the fork-bomb cap). No change to the
//! supervisor state machine or the TLA+ model is required — this is a config
//! tuning of an existing, already-modelled parameter, not a new transition.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use zeroize::Zeroizing;

use crate::custody::{Keyring, OsKeyring};
use crate::rpc::{HttpTransport, RpcClient};
use crate::supervisor::{
    BackoffPolicy, LogLine, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// OS keyring account for the node's 32-byte storage-at-rest master key. Lives
/// in the same service (`ai.citrate.core`) as the custody vault master, under a
/// distinct account so the two never collide.
const KEYRING_NODE_STORAGE_ACCOUNT: &str = "node-storage-key";

/// Length of the storage master key (32 bytes = AES-256).
const STORAGE_KEY_LEN: usize = 32;

/// C1.0b-1: the node's sustained-healthy window (see the module doc). Longer
/// than the supervisor default so a slow-but-healthy cold start is not mistaken
/// for a failure, and legitimate restarts always reset the failure budget.
const NODE_HEALTHY_AFTER: Duration = Duration::from_secs(90);

/// The default local RPC URL the spawned node serves (from the bundled testnet
/// config: `[rpc] listen_addr = "0.0.0.0:8545"`, reachable on loopback). Kept
/// distinct from the remote `rpc.citrate.ai` used by the wallet ceremony — this
/// one points at OUR local node's status surface.
const NODE_LOCAL_RPC_URL: &str = "http://127.0.0.1:8545";

/// The env var the node reads its 32-byte storage key from (hex). Matches the
/// node-side `at_rest_encryption_from_env` (citrate-chain node/src/main.rs).
const NODE_STORAGE_KEY_ENV: &str = "CITRATE_STORAGE_KEY";

/// CONSENSUS-CRITICAL launch env the fleet producer runs (DGX_NODE_SYNC_WEDGE_RESPONSE
/// 2026-07-22). VALIDATOR-S1 / §R' epoch-reward + validator-registry snapshot are OFF
/// by default (`citrate-chain node/src/main.rs:2506`: activates only on
/// `CITRATE_VALIDATOR_REGISTRY` + `_ACTIVATION_HEIGHT`). Without these the app node
/// computes a DIFFERENT state root than the fleet once §R' bites (first divergence at
/// block 2,580, after activation-2000) and the receive-path root check rejects the
/// block — the observed sync wedge. These MUST match the fleet producer's systemd env
/// (`rpc-1` 142.93.58.145) exactly.
const NODE_BLOCK_V2_ENV: &str = "CITRATE_BLOCK_V2";
const NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV: &str = "CITRATE_VALIDATOR_ACTIVATION_HEIGHT";
const NODE_VALIDATOR_REGISTRY_ENV: &str = "CITRATE_VALIDATOR_REGISTRY";
/// v2 execute-on-receive is DEFAULT ON since the 2026-07-21 SRP reroll; set explicitly
/// for parity with the fleet + clarity.
const NODE_BLOCK_V2_VALUE: &str = "1";
/// The fleet's validator activation height (fleet systemd env).
const NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE: &str = "2000";
/// The live ValidatorRegistry on chain 40204 — canonical
/// `citrate-chain/contracts/addresses/40204.json` (`ValidatorRegistry`), the same
/// address the fleet producer runs.
/// From the generated 40204 book (`crate::addresses`) — the SAME source
/// `grant_status` reads, so the node env and the app's reads cannot disagree.
fn node_validator_registry_value() -> &'static str {
    crate::addresses::validator_registry()
}
/// SYNC-S1 D3 (`citrate-chain node/src/dag_prune.rs`): bound the in-memory DAG
/// store. D1 removed the Θ(N²) blue-ancestry retention that OOM-killed followers
/// in the 9k–15k range (our node froze at 14840); D3 caps the remaining O(N)
/// growth (~16 KB/block → a desktop follower runs out near 150k blocks). Pruning
/// is opt-in and a no-op unless this env is set, so the app owns it — a long-lived
/// desktop follower must set it or it re-hits the memory wall. The value is a
/// retain window in blocks; the node clamps to a 1_000 floor (10× MAX_REORG_DEPTH),
/// and `block_serve` reads the CHAIN store, so pruning the DAG store never affects
/// what this node can serve to peers. 10_000 = 100× the deepest revertible reorg.
/// ⛔ **DELIBERATELY NOT SET.** Retained as a named constant so the
/// `debug_assert!` in `build_spec` can name it, and so anyone tempted to re-add
/// it lands on this comment first.
///
/// Setting this is what made a fresh node unable to cold-sync past block 54,600
/// on 40204 — the bug escalated as chain PR #144. The reasoning below (bound the
/// DAG store or a desktop follower OOMs) is CORRECT in general and WRONG on this
/// chain, for one specific reason:
///
///   block 54,600  `0xb3b1ee47…`  mergeParentHashes: []
///   block 54,601  `0xa267188c…`  mergeParentHashes: ["0x175fdf2b…"]
///   `0x175fdf2b…` is at **height 32**
///
/// Block 54,601 merges a parent 54,569 blocks below itself — an artefact of the
/// 2026-07-27 concurrent-producer fork, produced before any rule bounded
/// merge-parent depth. With a 10,000 retain window at applied height 54,600 the
/// pruning point is 44,600, so height 32 is deleted and admission of 54,601 fails
/// `MissingParent` FOREVER: blocks stored, applied head frozen, the same range
/// re-imported every ~2 s. No fleet node sets this, which is the only reason the
/// fleet was unaffected.
///
/// Proven both ways by chain PR #145 (`node/src/dag_prune.rs`):
/// `no_pruning_admits_the_deep_merge_parent_that_wedges_a_pruned_node` passes,
/// `merge_block_referencing_a_pruned_parent_is_rejected_not_scored` reproduces
/// the failure. The two differ only in whether a prune pass ran.
///
/// **The memory concern is real and now unmitigated**: an unpruned desktop
/// follower still grows ~16 KB/block and will approach the wall the original
/// comment warned about. Accepted deliberately — a node that cannot sync at all
/// is strictly worse than one that syncs and needs an occasional restart.
///
/// **Before re-enabling**, one of:
///   * chain-side: the bounded blue-set walk from
///     `handoffs/PRUNE_MERGE_PARENT_BOUND_SPEC.md` lands (MP-DEPTH #138 is now
///     active past height 100,000, which was its precondition); or
///   * a retain window proven larger than the deepest merge in pre-100,000
///     history. 60,000 would cover the one known anomaly, but an EXHAUSTIVE scan
///     is required first — the 28-height sample that found 54,601 is not proof
///     that it is the only one, and a block at height 99,000 merging genesis
///     would need a 99,000 window.
const NODE_DAG_PRUNE_RETAIN_ENV: &str = "CITRATE_DAG_PRUNE_RETAIN";

/// Node-side errors surfaced to the bridge as strings.
#[derive(Debug)]
pub enum NodeError {
    /// The OS keyring is unreachable (fail closed — never spawn without a key).
    Keyring(String),
    /// The bundled node binary could not be located.
    BinaryNotFound(String),
    /// The supervisor refused to start the node (thread/spawn failure).
    Spawn(String),
    /// The node is already running (idempotent-start guard).
    AlreadyRunning,
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Keyring(m) => write!(f, "node keyring error: {m}"),
            NodeError::BinaryNotFound(m) => write!(f, "node binary not found: {m}"),
            NodeError::Spawn(m) => write!(f, "node spawn error: {m}"),
            NodeError::AlreadyRunning => write!(f, "node already running"),
        }
    }
}

impl std::error::Error for NodeError {}

type Result<T> = std::result::Result<T, NodeError>;

/// The bridge status shape — mirrors `NodeDomain.status()` in domains.ts:
/// `{ state, peers, height, syncPct }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeStatus {
    /// "stopped" | "starting" | "running" | "restarting" | "failed" — the
    /// supervisor state mapped to the bridge vocabulary.
    pub state: String,
    /// Live peer count from `net_peerCount` (0 when RPC not reachable yet).
    pub peers: u64,
    /// Head height from `eth_blockNumber` (0 when RPC not reachable yet).
    pub height: u64,
    /// Bounded sync-progress percentage in [0, 100]. Honest: when we cannot
    /// determine a target height over the local RPC we report progress as a
    /// function of the observed head only if a target is known, else 0 while
    /// `starting`/unsynced and 100 once the node reports itself not syncing.
    #[serde(rename = "syncPct")]
    pub sync_pct: f64,
}

/// Map a [`SupervisorState`] to the bridge state vocabulary the surface renders.
fn map_state(state: &SupervisorState) -> &'static str {
    match state {
        SupervisorState::Off => "stopped",
        SupervisorState::Starting => "starting",
        SupervisorState::Running => "running",
        SupervisorState::Backoff { .. } => "restarting",
        SupervisorState::Failed => "failed",
    }
}

/// The node manager: owns the keyring seam, the resolved binary + data dir, the
/// local RPC url, and (while running) a live [`Supervisor`]. Managed as Tauri
/// state; `status`/`start`/`stop` are called from the invoke commands.
pub struct NodeManager {
    keyring: Box<dyn Keyring>,
    /// The bundled node binary path (resolved from the Tauri resource dir or a
    /// dev/stub override).
    bin: PathBuf,
    /// The node's encrypted data dir (inside the app data dir).
    data_dir: PathBuf,
    /// Where crash records are appended.
    crash_record_path: PathBuf,
    /// The local RPC url to poll for real height/peers.
    rpc_url: String,
    /// The live supervisor, present only while the node is running.
    sup: Mutex<Option<Supervisor>>,
    /// W1.5 — the coinbase (reward/staker) address the node mines to, when known.
    /// This is the member's own embedded-wallet address (key-safe: the node derives
    /// its ed25519 proposer key from this *public* address and never holds the
    /// wallet's private key). `None` until the wallet is unlocked/available, in
    /// which case the node spawns as a plain follower (no `--mine`).
    coinbase: Mutex<Option<String>>,
    /// Whether the block producer is ARMED. A member node must NOT produce while
    /// it is still catching up: a node tens of thousands of blocks behind that
    /// starts producing builds its own competing block at the fork it is syncing
    /// through (the 2026-07-27 concurrent-producer fork at 54600), whose state no
    /// peer reproduces — its producer fork-choice then diverges from its own
    /// applied tip and the node wedges instead of following the canonical chain.
    /// So `--mine` is gated on this flag (NOT merely on a known coinbase): the
    /// node spawns as a plain FOLLOWER and is only armed once it has caught up to
    /// the network tip (see [`Self::arm_mining`]). Off by default.
    mining_armed: AtomicBool,
}

impl NodeManager {
    /// Build a manager over an explicit keyring, binary path, data dir, and RPC
    /// url. Production uses [`NodeManager::from_app`]; tests inject a fake
    /// keyring + a stub binary + a temp data dir + a stub RPC url.
    pub fn new(
        keyring: Box<dyn Keyring>,
        bin: PathBuf,
        data_dir: PathBuf,
        crash_record_path: PathBuf,
        rpc_url: impl Into<String>,
    ) -> Self {
        NodeManager {
            keyring,
            bin,
            data_dir,
            crash_record_path,
            rpc_url: rpc_url.into(),
            sup: Mutex::new(None),
            coinbase: Mutex::new(None),
            mining_armed: AtomicBool::new(false),
        }
    }

    /// W1.5 — set the coinbase (the member's wallet address) the node mines to.
    /// The next spawn (start / supervised restart) arms the block producer with
    /// `--mine --coinbase <addr>`. Idempotent: the last value set wins. Setting it
    /// while the node is already running takes effect on the next restart — callers
    /// that want it live now should stop+start.
    pub fn set_coinbase(&self, addr: String) {
        *self.coinbase.lock().unwrap_or_else(|e| e.into_inner()) = Some(addr);
    }

    /// The currently-configured coinbase address, if any.
    pub fn coinbase(&self) -> Option<String> {
        self.coinbase
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Whether the block producer is currently armed (see [`Self::build_spec`]).
    pub fn is_mining_armed(&self) -> bool {
        self.mining_armed.load(Ordering::Relaxed)
    }

    /// Arm the block producer and, if the node is running, restart it so the next
    /// spawn carries `--mine --coinbase`. Idempotent — arming an already-armed
    /// node is a no-op (returns without touching the supervisor). Callers MUST
    /// gate this on being caught up to the network tip ([`Self::arm_mining_if_synced`]);
    /// arming while behind reintroduces the concurrent-producer wedge this guards.
    pub fn arm_mining(&self) {
        if self.mining_armed.swap(true, Ordering::Relaxed) {
            return; // already armed — do not churn the supervisor
        }
        let running = self.sup.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if running {
            self.stop();
            // Best-effort respawn under the mining spec; a failed restart leaves
            // the node stopped (the supervisor status reports it honestly).
            let _ = self.start();
        }
    }

    /// Arm the producer IFF the node has caught up to the network tip. Reads the
    /// local head (this node's RPC) and the AUTHORITATIVE network tip (the public
    /// citrate RPC) — deliberately NOT the local node's own `eth_syncing`, which
    /// reports "synced" the instant the node produces its own block, the very bug
    /// this guards. Arms only when a coinbase is known and it is not already
    /// armed. Best-effort: any RPC error → no-arm (stays a follower), never panics.
    /// Returns whether it armed on this call.
    pub fn arm_mining_if_synced(&self) -> bool {
        if self.is_mining_armed() || self.coinbase().is_none() {
            return false;
        }
        let (Some(local), Some(tip)) = (self.head_height(), remote_network_tip()) else {
            return false;
        };
        if is_caught_up(local, tip) {
            self.arm_mining();
            return true;
        }
        false
    }

    /// W1.1 (WP-11) — the node's ed25519 proposer pubkey, read from the secret
    /// `proposer.key` the node minted in its data dir. Honest error until the node
    /// has started once (the key is minted on first run). This is the registered
    /// consensus identity, NOT derivable from the coinbase.
    pub fn proposer_pubkey(&self) -> std::result::Result<String, String> {
        crate::validator::read_proposer_pubkey(&self.data_dir)
    }

    /// W1.3 — sign the validator registration with the node's `proposer.key`,
    /// returning `(proposer_pubkey, ed25519_sig)`. The seed never leaves the
    /// validator module (read + zeroized there). Honest error before the node has
    /// minted its key.
    pub fn sign_registration(
        &self,
        chain_id: u64,
        registry: &[u8; 20],
        staker: &[u8; 20],
        nonce: u64,
    ) -> std::result::Result<([u8; 32], [u8; 64]), String> {
        crate::validator::sign_registration_from_data_dir(
            &self.data_dir,
            chain_id,
            registry,
            staker,
            nonce,
        )
    }

    /// Load the 32-byte storage key from the OS keyring, minting it on first use
    /// (@rule8). Returned as a hex `Zeroizing<String>` ready to hand to the child
    /// via env; the raw bytes are zeroized on drop. An unreachable keyring is a
    /// hard fault — we NEVER spawn the node without an encryption key (that would
    /// silently produce a plaintext data dir).
    fn storage_key_hex(&self) -> Result<Zeroizing<String>> {
        // Read an existing key, if present.
        match self
            .keyring
            .get(KEYRING_NODE_STORAGE_ACCOUNT)
            .map_err(|e| NodeError::Keyring(e.to_string()))?
        {
            Some(bytes) => {
                if bytes.len() != STORAGE_KEY_LEN {
                    return Err(NodeError::Keyring(format!(
                        "stored node storage key has wrong length {}",
                        bytes.len()
                    )));
                }
                let hex = Zeroizing::new(hex::encode(&bytes));
                // scrub the transient copy from the keyring read
                let mut bytes = bytes;
                use zeroize::Zeroize;
                bytes.zeroize();
                Ok(hex)
            }
            None => {
                // Mint a fresh 32-byte key with a CSPRNG and persist it.
                use rand::RngCore;
                let mut key = Zeroizing::new([0u8; STORAGE_KEY_LEN]);
                rand::thread_rng().fill_bytes(key.as_mut());
                self.keyring
                    .set(KEYRING_NODE_STORAGE_ACCOUNT, key.as_ref())
                    .map_err(|e| NodeError::Keyring(e.to_string()))?;
                Ok(Zeroizing::new(hex::encode(key.as_ref())))
            }
        }
    }

    /// Build the [`SidecarSpec`] for the node: explicit binary, `--data-dir`,
    /// and the storage key in the env (NOT argv — argv would leak the key to a
    /// `ps` listing). `--network testnet` joins the public testnet.
    fn build_spec(&self, storage_key_hex: &str) -> SidecarSpec {
        let mut args = vec![
            "--network".to_string(),
            "testnet".to_string(),
            "--data-dir".to_string(),
            self.data_dir.to_string_lossy().to_string(),
        ];
        // Pass our config EXPLICITLY. Without `--config` the node walks its
        // fallback chain ($CITRATE_CONFIG → ~/.citrate/node.toml →
        // /etc/citrate/node.toml → empty default), and `bootstrap_nodes`
        // defaults to `[]` — so a fresh install has nothing to dial and sits at
        // height 0 with 0 peers forever (verified 2026-08-04 on a clean data
        // dir). Developer machines masked this because ~/.citrate/node.toml
        // exists there; `--config` also stops US inheriting a developer's file,
        // which could point a member at the wrong chain entirely.
        //
        // Best-effort: if the config cannot be written we still launch, because
        // a node that starts and reports "0 peers" is far easier to diagnose
        // than one that refuses to start at all. `ensure_node_config` logs.
        if let Ok(cfg) = ensure_node_config(&self.data_dir) {
            args.push("--config".to_string());
            args.push(cfg.to_string_lossy().to_string());
        }
        // W1.5 — arm the block producer ONLY when (a) the member's coinbase is
        // known AND (b) mining has been ARMED (the node has caught up to the
        // network tip; see [`Self::arm_mining`] + the `mining_armed` field doc).
        // The node's own active-set self-gating ("proposer not in the active set
        // at minStake") is NOT sufficient here: a registered validator IS in the
        // active set, so eligibility alone would let it produce while still tens
        // of thousands of blocks behind — which is exactly the wedge (a competing
        // block at the 54600 concurrent-producer fork, state no peer reproduces).
        // Until armed the node spawns as a plain FOLLOWER (omits both flags) and
        // just follows the canonical chain to the tip. `--mine` only ever forces
        // mining ON (citrate-chain node/src/main.rs) — there is no CLI off-switch —
        // so gating the flag here is the only lever the app has.
        if self.mining_armed.load(Ordering::Relaxed) {
            if let Some(coinbase) = self.coinbase() {
                args.push("--mine".to_string());
                args.push("--coinbase".to_string());
                args.push(coinbase);
            }
        }
        let mut spec = SidecarSpec::new("citrate-node", self.bin.clone(), args);
        spec.env = vec![
            (NODE_STORAGE_KEY_ENV.to_string(), storage_key_hex.to_string()),
            // The node's `tracing` output is piped (not a TTY) into the log tail the
            // UI renders; emit PLAIN text so ANSI colour escapes don't surface as
            // unrenderable boxes. `NO_COLOR` (https://no-color.org) is honoured by
            // tracing-subscriber + most Rust log stacks. The UI also strips ANSI
            // defensively (src/surfaces/Node.tsx), but killing it at the source is
            // the real fix.
            ("NO_COLOR".to_string(), "1".to_string()),
            ("CLICOLOR".to_string(), "0".to_string()),
            // CONSENSUS-CRITICAL: reproduce the fleet producer's validator/§R' state
            // path or the node forks the state root and wedges (see the const docs +
            // DGX_NODE_SYNC_WEDGE_RESPONSE_2026-07-22).
            (NODE_BLOCK_V2_ENV.to_string(), NODE_BLOCK_V2_VALUE.to_string()),
            (
                NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV.to_string(),
                NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE.to_string(),
            ),
            (
                NODE_VALIDATOR_REGISTRY_ENV.to_string(),
                node_validator_registry_value().to_string(),
            ),
        ];
        // DAG pruning is deliberately NOT set — see `NODE_DAG_PRUNE_RETAIN_ENV`.
        debug_assert!(
            !spec.env.iter().any(|(k, _)| k == NODE_DAG_PRUNE_RETAIN_ENV),
            "DAG pruning must not be enabled: it wedges cold sync at block 54,600"
        );
        spec
    }

    /// Start the node under the supervisor. Idempotent-ish: returns
    /// `AlreadyRunning` if a supervisor is already live. @rule8: the storage key
    /// is minted/loaded from the keyring and handed to the child via env.
    pub fn start(&self) -> Result<()> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(NodeError::AlreadyRunning);
        }
        if !self.bin.exists() {
            return Err(NodeError::BinaryNotFound(self.bin.display().to_string()));
        }
        let key_hex = self.storage_key_hex()?;
        let spec = self.build_spec(&key_hex);
        // C1.0b-1: default fork-bomb-bounded backoff, but a node-tuned
        // sustained-healthy window (see the module doc).
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = NODE_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| NodeError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        Ok(())
    }

    /// Stop the node: graceful supervisor release (SIGTERM → grace → SIGKILL),
    /// dropping the supervisor so no orphan remains. Idempotent — stopping an
    /// already-stopped node is a no-op.
    pub fn stop(&self) {
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(sup) = sup {
            sup.stop();
            // Dropping the supervisor joins the monitor thread (no orphan).
            drop(sup);
        }
    }

    /// Read the node's real status: supervisor state + (when running) live
    /// height/peers from the local RPC. NEVER fabricates numbers — if the RPC is
    /// not reachable yet the counts are 0 and the state reflects that honestly.
    pub fn status(&self) -> NodeStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        // Only poll the RPC when the supervisor thinks the child is Running.
        let running = matches!(sup_state, Some(SupervisorState::Running));
        let (height, peers) = if running {
            self.poll_rpc().unwrap_or((0, 0))
        } else {
            (0, 0)
        };
        let sync_pct = if running && height > 0 { 100.0 } else { 0.0 };
        NodeStatus {
            state: state.to_string(),
            peers,
            height,
            sync_pct,
        }
    }

    /// Q-A.2/Q-B.2 — the REAL recent node log lines, streamed from the
    /// supervised child's stdout+stderr into the supervisor's bounded ring. Newest
    /// lines are last; the ring is capped (`LOG_RING_CAPACITY`), so this returns at
    /// most that many. When the node is not running (no supervisor) it honestly
    /// returns an EMPTY list — never a fabricated template (Rule 1). This is what
    /// fills the Node LOG panel in a packaged build (it was permanently empty
    /// because stdout was inherited-and-dropped in the GUI process).
    pub fn logs(&self) -> Vec<LogLine> {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(sup) => sup.logs(),
            None => Vec::new(),
        }
    }

    /// Poll the node's local RPC for `(height, peers)`. Returns `None` on any
    /// transport error (RPC not up yet / node still booting) so the caller
    /// reports 0/0 honestly rather than a stale or fabricated value.
    fn poll_rpc(&self) -> Option<(u64, u64)> {
        let client = RpcClient::with_transport(HttpTransport::new(self.rpc_url.clone()));
        let height = client.block_number().ok()?;
        let peers = client.peer_count().unwrap_or(0);
        Some((height, peers))
    }

    /// This node's local head height (`eth_blockNumber` over its own RPC), or
    /// `None` if the RPC is not up. The arm gate reads this as the "how far have I
    /// caught up" side of the comparison.
    fn head_height(&self) -> Option<u64> {
        RpcClient::with_transport(HttpTransport::new(self.rpc_url.clone()))
            .block_number()
            .ok()
    }
}

/// The authoritative network tip: `eth_blockNumber` on the public citrate RPC
/// (`rpc.citrate.ai`). Read from the network, NOT from this node's own view, so
/// a wedged/self-mining local node cannot fool the arm gate into thinking it has
/// caught up. `None` on any transport error (→ no-arm, stay a follower).
fn remote_network_tip() -> Option<u64> {
    crate::rpc::RpcClient::citrate().block_number().ok()
}

/// The block margin within which the local node counts as "caught up" to the
/// network tip. At 40204's 2.0s block time this is ~1 minute of slack: small
/// enough that a producer armed here is genuinely at the head, large enough to
/// absorb the tip advancing a few blocks during the check itself.
const SYNC_ARM_MARGIN: u64 = 32;

/// Pure gate: is `local` height within [`SYNC_ARM_MARGIN`] of the network `tip`?
/// Extracted as a free function so the arm decision is unit-testable without any
/// RPC. Saturating so a (spurious) `local > tip` can never underflow.
pub fn is_caught_up(local: u64, tip: u64) -> bool {
    local.saturating_add(SYNC_ARM_MARGIN) >= tip
}

/// Managed Tauri state: the process-wide node manager.
pub struct NodeState(pub NodeManager);

/// Resolve the bundled `citrate` sidecar binary path.
///
/// D-C1-1: the node is bundled as a Tauri `externalBin` (`citrate`), which Tauri
/// installs into the app's resource dir as `citrate` (Tauri strips the target
/// triple suffix at bundle time). In dev, and for the stub-node CI path, an
/// override env var `CITRATE_NODE_BIN` points directly at a binary so the app
/// (and the tests) can run without a full bundle.
fn resolve_node_bin<R: Runtime>(app: &AppHandle<R>) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CITRATE_NODE_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "CITRATE_NODE_BIN set but not found: {}",
            path.display()
        ));
    }
    // Bundled externalBin: installed next to the main executable (Contents/MacOS).
    crate::supervisor::resolve_external_bin(app, "citrate")
}

/// The member node config, compiled in.
///
/// Embedded rather than bundled as a Tauri resource so it cannot go missing at
/// runtime: a resource lookup that fails leaves the node with no bootnodes,
/// which is silent (0 peers) rather than loud. `include_str!` makes an absent
/// file a BUILD error instead.
const MEMBER_NODE_CONFIG: &str = include_str!("../config/member-node.toml");

/// The `{{DATA_DIR}}` placeholder in [`MEMBER_NODE_CONFIG`].
const DATA_DIR_PLACEHOLDER: &str = "{{DATA_DIR}}";

/// Write the member node config into `data_dir/node.toml` if it is not already
/// there, and return its path.
///
/// Does NOT overwrite an existing file: an operator who hand-edits their
/// bootnodes or ports must keep those edits across restarts and upgrades. The
/// cost is that a shipped bootnode change does not reach existing installs —
/// deliberate, since silently rewriting a user's config is worse. Delete the
/// file to regenerate.
fn ensure_node_config(data_dir: &std::path::Path) -> std::io::Result<PathBuf> {
    let path = data_dir.join("node.toml");
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(data_dir)?;
    // Escape for TOML basic strings: on Windows the path contains backslashes,
    // which would otherwise be read as escape sequences and yield a broken or
    // (worse) subtly wrong data_dir.
    let escaped = data_dir
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let rendered = MEMBER_NODE_CONFIG.replace(DATA_DIR_PLACEHOLDER, &escaped);
    std::fs::write(&path, rendered)?;
    Ok(path)
}

/// Build the managed node state from a live app handle: the real OS keyring, the
/// bundled binary, an encrypted `node` data dir inside the app data dir, and the
/// local RPC url.
pub fn build_node_state<R: Runtime>(app: &AppHandle<R>) -> std::result::Result<NodeState, String> {
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let data_dir = data_root.join("node");
    let crash_record_path = data_root.join("node").join("crash-records.jsonl");
    let bin = resolve_node_bin(app)?;
    Ok(NodeState(NodeManager::new(
        Box::new(OsKeyring),
        bin,
        data_dir,
        crash_record_path,
        NODE_LOCAL_RPC_URL,
    )))
}

// ---------------------------------------------------------------------------
// Tauri commands — the NodeDomain bridge surface. Return status / () only.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn node_status(state: State<'_, NodeState>) -> std::result::Result<NodeStatus, String> {
    Ok(state.0.status())
}

#[tauri::command]
pub fn node_start(
    state: State<'_, NodeState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<(), String> {
    // W1.5 — arm the producer with the member's own wallet as coinbase when the
    // vault is unlocked. Best-effort: a locked or absent wallet just starts a
    // plain follower; the coinbase can be set later and the node restarted.
    if let Ok(info) = crate::wallet::address_auto_unlocked(&custody.0) {
        state.0.set_coinbase(info.address);
    }
    state.0.start().map_err(|e| e.to_string())
}

/// W1.1 (WP-11) — the node's on-chain proposer identity: the `coinbase` (the
/// member's wallet address the node mines to) and the ed25519 `proposer_pubkey`
/// READ from the node's minted `proposer.key`. The pubkey is a persisted secret's
/// public half — the value registered in `ValidatorRegistry` and the key for
/// `validatorInfo`/reward reads — NOT derivable from the coinbase (WP-11). The
/// coinbase needs an unlocked vault; the pubkey needs the node to have started once
/// (mint-on-first-run) — either leg errors honestly.
#[derive(serde::Serialize)]
pub struct ProposerIdentity {
    pub coinbase: String,
    #[serde(rename = "proposerPubkey")]
    pub proposer_pubkey: String,
}

#[tauri::command]
pub fn node_proposer_identity(
    state: State<'_, NodeState>,
    custody: State<'_, crate::custody::CustodyState>,
) -> std::result::Result<ProposerIdentity, String> {
    let info = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let proposer_pubkey = state.0.proposer_pubkey()?;
    Ok(ProposerIdentity {
        coinbase: info.address,
        proposer_pubkey,
    })
}

/// The node's REAL validator earnings — `(total, claimable)` SALT wei from
/// `ValidatorRegistry.rewardsOf(proposerPubkey)` on 40204. The block subsidy accrues
/// HERE, keyed by the proposer pubkey, which is the correct source (W1.4 fix,
/// replacing the wrong `ContributionAccounting.claimable` read). A not-yet-registered
/// validator returns `(0, 0)` honestly — never a fabricated number (Rule 1).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorEarnings {
    pub total_wei: String,
    pub claimable_wei: String,
    #[serde(rename = "proposerPubkey")]
    pub proposer_pubkey: String,
}

#[tauri::command]
pub fn node_validator_earnings(
    state: State<'_, NodeState>,
) -> std::result::Result<ValidatorEarnings, String> {
    let pubkey_hex = state.0.proposer_pubkey()?;
    let raw = pubkey_hex.strip_prefix("0x").unwrap_or(&pubkey_hex);
    let bytes = hex::decode(raw).map_err(|e| format!("bad proposer pubkey hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("proposer pubkey is {} bytes, expected 32", bytes.len()));
    }
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&bytes);
    let rpc = crate::rpc::RpcClient::citrate();
    let (total, claimable) =
        crate::validator::read_validator_rewards(&rpc, node_validator_registry_value(), &pubkey)?;
    Ok(ValidatorEarnings {
        total_wei: total.to_string(),
        claimable_wei: claimable.to_string(),
        proposer_pubkey: pubkey_hex,
    })
}

/// W1.3 — the validator bond (SALT wei) the member sends as `msg.value` on
/// `registerValidator`. Matches the membership grant + `ValidatorRegistry.minStake`
/// (32,000 SALT). If minStake ever rises above this the register reverts (BadStake)
/// and the app surfaces the honest revert rather than a fabricated success.
const VALIDATOR_STAKE_WEI: u128 = 32_000u128 * 1_000_000_000_000_000_000u128;
/// Explicit gas for `registerValidator` (ed25519-verify precompile + storage
/// writes + possible eviction). A calldata tx MUST carry explicit gas or the
/// ceremony rejects it as undecodable.
const REGISTER_GAS: u64 = 600_000;

/// Build the `{from,to,value,data,gas,chainId}` JSON the ceremony's `Transaction`
/// intent consumes for `MemberBond.activate` (same shape as `staking::encode_stake_json`).
/// `to` is the member's bond CLONE; `value` is 0 — the clone forwards its OWN principal
/// (the 32k is already inside it), so the member EOA only pays gas.
fn encode_activate_json(from: &str, bond: &str, calldata: &[u8]) -> String {
    serde_json::json!({
        "from": from,
        "to": bond,
        "value": "0x0",
        "data": format!("0x{}", hex::encode(calldata)),
        "gas": format!("0x{REGISTER_GAS:x}"),
        "chainId": format!("0x{:x}", 40204u64),
    })
    .to_string()
}

/// W1.3 (bond-clone model, owner decision 2026-08-05 — reverses ADR-2026-07-27) —
/// activate the member's validator bond. The 32k already lives inside the member's
/// `MemberBond` clone (deployed + funded by `vault.grant` at payment). This reads the
/// live `bondOf(member)` escrow, signs the register digest — binding the CLONE as the
/// staker, because the clone (not the EOA) is what calls `registerValidator`, so the
/// registry sees `msg.sender = clone` — with the node's `proposer.key`, and submits
/// `MemberBond.activate(pubkey, sig)` TO the clone as a PENDING ceremony (Rule 3 — the
/// human approves; `sign_and_broadcast` signs the EIP-155 tx from the member EOA, which
/// is the clone's `onlyMember`). The tx carries NO value — the clone forwards its own
/// principal. Requires the node to have minted its key (started once); each leg errors
/// honestly (no deployed bond / already activated / underfunded).
#[tauri::command]
pub fn node_register_validator(
    state: State<'_, NodeState>,
    custody: State<'_, crate::custody::CustodyState>,
    ceremony: State<'_, crate::ceremony::CeremonyState>,
) -> std::result::Result<crate::ceremony::CeremonyView, String> {
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let registry = crate::validator::parse_address_20(node_validator_registry_value())?;
    let rpc = crate::rpc::RpcClient::citrate();

    // Honesty guard (Rule 1): activation can only bond a clone that is deployed AND
    // funded, and only once. Read the live grant status keyed on the member EOA (the
    // address that will send `activate`, and the one `bondOf` is derived from). A
    // member whose grant has not landed has no deployed bond; one who already activated
    // would revert `AlreadyActivated`. Fail here with a clear reason rather than mint a
    // doomed ceremony that looks like activation succeeded.
    let grant = crate::grant_status::read_grant_status(&rpc, &wallet.address)
        .map_err(|e| e.to_string())?;
    if !grant.bond_deployed {
        return Err(format!(
            "validator bond not ready: no MemberBond escrow is deployed for your wallet ({}). \
             The membership grant deploys and funds it at payment; if it hasn't arrived, the \
             treasury grant step is still pending — activation can't proceed until then.",
            wallet.address
        ));
    }
    if grant.has_validator {
        return Err(format!(
            "already a validator: your bond ({}) has already activated its proposer key.",
            grant.bond_address
        ));
    }
    // The clone forwards its principal to `registerValidator`; the funded principal
    // (set to the 32k at grant time) must meet the bond or `registerValidator` reverts.
    let principal: u128 = grant.attributed_principal_wei.parse().unwrap_or(0);
    if principal < VALIDATOR_STAKE_WEI {
        let salt = |w: u128| w / 1_000_000_000_000_000_000u128;
        return Err(format!(
            "validator bond underfunded: your MemberBond ({}) holds {} SALT of attributed \
             principal but activation needs {} (the 32k bond). The treasury grant funds this \
             automatically; if it hasn't fully landed, activation can't proceed yet.",
            grant.bond_address, salt(principal), salt(VALIDATOR_STAKE_WEI)
        ));
    }

    // The staker bound in the register digest is the CLONE (the `msg.sender` inside
    // `registry.registerValidator`), NOT the member EOA.
    let staker = crate::validator::parse_address_20(&grant.bond_address)?;
    // Live replay-guard nonce for the CLONE's registration digest (real read, never fabricated).
    let nonce =
        crate::validator::read_registration_nonce(&rpc, node_validator_registry_value(), &staker)?;
    // Sign the registration with the node's proposer key (seed stays in validator.rs).
    let (pubkey, sig) = state.0.sign_registration(40204, &registry, &staker, nonce)?;
    let calldata = crate::validator::member_bond_activate_calldata(&pubkey, &sig);
    // Sent FROM the member EOA (the clone's `onlyMember`) TO the clone, value 0.
    let raw = encode_activate_json(&wallet.address, &grant.bond_address, &calldata);
    let intent = crate::ceremony::SignatureIntent {
        origin: "local-user".to_string(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw,
    };
    Ok(ceremony.0.request(intent))
}

/// W1.x — arm the block producer, CONSENSUS-GATED on being synced. Delegates to
/// [`NodeManager::arm_mining_if_synced`], which arms only when a coinbase is known
/// AND the node has caught up to the network tip (arming while behind reintroduces
/// the concurrent-producer wedge that caused the 54600 fork). On arm the node
/// respawns with `--mine --coinbase`, and the chain node then MINTS `proposer.key`
/// — the ed25519 validator identity the bond activation needs. Without this the node
/// stays a plain follower forever, never mints the key, and the bond can never
/// activate. Idempotent + honest: returns `true` iff it armed on THIS call (already
/// armed, no coinbase, or not-yet-synced all return `false`, never a fabricated arm).
#[tauri::command]
pub fn node_arm_mining(state: State<'_, NodeState>) -> bool {
    state.0.arm_mining_if_synced()
}

#[tauri::command]
pub fn node_stop(state: State<'_, NodeState>) -> std::result::Result<(), String> {
    state.0.stop();
    Ok(())
}

/// Q-A.2/Q-B.2 — return the REAL recent node log lines (streamed stdout+stderr
/// from the supervised child's bounded ring). Honest empty when the node is not
/// running. The webview folds these into the Node LOG panel so a packaged build
/// shows live node output, never a fabricated template (Rule 1).
#[tauri::command]
pub fn node_logs(state: State<'_, NodeState>) -> std::result::Result<Vec<LogLine>, String> {
    Ok(state.0.logs())
}

#[cfg(test)]
mod tests {
    include!("node_tests.rs");
}

/// The most recent supervised-node crash, or `None` if it has never crashed.
#[derive(serde::Serialize)]
pub struct LastCrash {
    /// Human-readable cause, taken from the node's own stderr tail (falling back
    /// to the exit status when stderr was empty). Never a generic placeholder.
    pub reason: String,
    #[serde(rename = "atUnixMs")]
    pub at_unix_ms: u64,
}

/// **Command — node_last_crash.** Read the last line of `crash-records.jsonl`.
///
/// The supervisor restarts a crashed node with backoff and, at the cap, gives up
/// and writes a record — but nothing surfaced it, so a validator could sit down
/// indefinitely while the app looked idle. The watchdog uses this to tell the
/// member WHY their node stopped ("LOCK: Resource temporarily unavailable",
/// "No space left on device") instead of a generic failure line (Rule 1).
///
/// Read-only and best-effort: a missing/short/corrupt file is `None`, never an
/// error that would itself need explaining.
#[tauri::command]
pub fn node_last_crash<R: Runtime>(app: AppHandle<R>) -> Option<LastCrash> {
    let path = app.path().app_data_dir().ok()?.join("node").join("crash-records.jsonl");
    let contents = std::fs::read_to_string(path).ok()?;
    let last = contents.lines().rev().find(|l| !l.trim().is_empty())?;
    let v: serde_json::Value = serde_json::from_str(last).ok()?;
    let stderr_tail = v.get("stderr_tail").and_then(|x| x.as_str()).unwrap_or("").trim();
    let exit = v.get("exit").and_then(|x| x.as_str()).unwrap_or("").trim();
    // Prefer the node's own words; fall back to the exit status.
    let reason = if stderr_tail.is_empty() { exit } else { stderr_tail };
    if reason.is_empty() {
        return None;
    }
    // Keep it to one readable line for a toast.
    let reason: String = reason.lines().next().unwrap_or(reason).chars().take(180).collect();
    Some(LastCrash {
        reason,
        at_unix_ms: v.get("at_unix_ms").and_then(|x| x.as_u64()).unwrap_or(0),
    })
}
