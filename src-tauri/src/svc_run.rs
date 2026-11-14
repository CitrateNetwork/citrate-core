//! V60 SVC-1: the service process, `citrate-core --svc` (see [`crate::svc`]).
//!
//! [`svc_main`] resolves the app data dir without an app handle, runs the same startup orphan
//! sweep the app runs, binds the socket, starts kubo and the node, and then loops:
//! - every [`TICK`] it starts what is wanted and not running (a node start held by a locked
//!   keyring or by another holder of the chain database is retried here, never crash-looped),
//!   and runs the mining gate exactly as the app's watchdog does;
//! - a drain (CORE-D6, before an update) holds the app-wide spawn gate, stops both and waits for
//!   the children to exit. While drained, a replaced executable makes the service exit
//!   [`EXIT_RESTART`] so the unit manager starts the new one; a drain that outlives
//!   [`DRAIN_MAX`] with the old executable is released (the update did not happen);
//! - SIGTERM or SIGINT (the unit manager's stop) drains and exits 0.
//!
//! The node's storage key comes from the OS keyring as in-process. When the keyring is not
//! reachable (Linux before the member logs in: spike finding 1) the start is held with that
//! reason and retried every tick; there is no plaintext fallback.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use citrate_core_kit::supervisor::SpawnGate;

use crate::ipfs::IpfsManager;
use crate::node::NodeManager;
use crate::svc::{SvcDrain, SvcHello, SvcPaths, SvcStatus, EXIT_RESTART, PROTOCOL_VERSION};
use crate::svc_server::{Control, SvcHost, SvcServer};

/// How often the service starts what is wanted and runs the mining gate (the app's watchdog
/// period).
pub const TICK: Duration = Duration::from_secs(30);
/// The longest a drain is kept with an unchanged executable.
pub const DRAIN_MAX: Duration = Duration::from_secs(15 * 60);
/// What a start refused during a drain says.
pub const DRAIN_HOLD: &str = "the background service is drained for an update";

/// The node and kubo under the service.
pub struct NodeHost {
    node: NodeManager,
    ipfs: IpfsManager,
    paths: SvcPaths,
    started_unix_ms: u64,
    want_node: AtomicBool,
    want_ipfs: AtomicBool,
    drained_at: Mutex<Option<Instant>>,
    node_hold: Mutex<Option<String>>,
    gate: Arc<SpawnGate>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A coinbase is a 20-byte hex address with the `0x` prefix.
pub(crate) fn valid_coinbase(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

impl NodeHost {
    /// Over explicit managers. The persisted coinbase, if any, is handed to the node.
    pub fn new(
        node: NodeManager,
        ipfs: IpfsManager,
        paths: SvcPaths,
        gate: Arc<SpawnGate>,
    ) -> Self {
        if let Ok(raw) = std::fs::read_to_string(&paths.coinbase) {
            let c = raw.trim();
            if valid_coinbase(c) {
                node.set_coinbase(c.to_string());
            }
        }
        NodeHost {
            node,
            ipfs,
            paths,
            started_unix_ms: now_ms(),
            want_node: AtomicBool::new(true),
            want_ipfs: AtomicBool::new(true),
            drained_at: Mutex::new(None),
            node_hold: Mutex::new(None),
            gate,
        }
    }

    fn node_running(&self) -> bool {
        self.node
            .supervisor_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    fn ipfs_supervised(&self) -> bool {
        self.ipfs
            .supervisor_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Whether a drain is in effect, and since when.
    pub fn drained_since(&self) -> Option<Instant> {
        *self.drained_at.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_hold(&self, hold: Option<String>) {
        *self.node_hold.lock().unwrap_or_else(|e| e.into_inner()) = hold;
    }

    /// Start what is wanted and not running, then run the mining gate. Kubo first: the node
    /// reaches it on 127.0.0.1:5001 for artifacts (block production does not need it).
    pub fn tick(&self) {
        if self.drained_since().is_some() {
            return;
        }
        if self.want_ipfs.load(Ordering::SeqCst) && !self.ipfs_supervised() {
            if let Err(e) = self.ipfs.start() {
                eprintln!("[svc] kubo did not start: {e}");
            }
        }
        if self.want_node.load(Ordering::SeqCst) && !self.node_running() {
            match self.node.start() {
                Ok(()) => self.set_hold(None),
                Err(crate::node::NodeError::AlreadyRunning) => self.set_hold(None),
                Err(e) => {
                    let msg = e.to_string();
                    eprintln!("[svc] node start held: {msg}");
                    self.set_hold(Some(msg));
                }
            }
        }
        if self.node_running() {
            if self.node.arm_mining_if_synced() {
                eprintln!("[svc] block production armed by the mining gate");
            } else if self.node.recheck_mining() {
                eprintln!("[svc] block production disarmed by the mining gate");
            }
        }
    }

    /// Stop both through their owners (SIGTERM, then SIGKILL after the stop grace).
    fn stop_all(&self) {
        self.node.stop();
        self.ipfs.stop();
    }
}

impl SvcHost for NodeHost {
    fn hello(&self) -> SvcHello {
        SvcHello {
            protocol: PROTOCOL_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            started_unix_ms: self.started_unix_ms,
        }
    }

    fn status(&self) -> SvcStatus {
        SvcStatus {
            node: self.node.status(),
            ipfs_running: self.ipfs.running(),
            node_hold: self
                .node_hold
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            drained: self.drained_since().is_some(),
            coinbase: self.node.coinbase(),
        }
    }

    fn node_start(&self) -> Result<(), String> {
        if self.drained_since().is_some() {
            return Err(DRAIN_HOLD.to_string());
        }
        self.want_node.store(true, Ordering::SeqCst);
        match self.node.start() {
            Ok(()) | Err(crate::node::NodeError::AlreadyRunning) => {
                self.set_hold(None);
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                self.set_hold(Some(msg.clone()));
                Err(msg)
            }
        }
    }

    fn node_stop(&self) {
        // A member Stop is kept: the tick does not start the node again until a start.
        self.want_node.store(false, Ordering::SeqCst);
        self.node.stop();
    }

    fn ipfs_start(&self) -> Result<(), String> {
        if self.drained_since().is_some() {
            return Err(DRAIN_HOLD.to_string());
        }
        self.want_ipfs.store(true, Ordering::SeqCst);
        match self.ipfs.start() {
            Ok(()) | Err(crate::ipfs::IpfsError::AlreadyRunning) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn ipfs_stop(&self) {
        self.want_ipfs.store(false, Ordering::SeqCst);
        self.ipfs.stop();
    }

    fn set_coinbase(&self, coinbase: String) -> Result<(), String> {
        if !valid_coinbase(&coinbase) {
            return Err("the coinbase must be a 0x-prefixed 20-byte hex address".to_string());
        }
        citrate_core_kit::fsutil::write_secret_file(&self.paths.coinbase, coinbase.as_bytes())
            .map_err(|e| format!("cannot keep the coinbase: {}", e.kind()))?;
        self.node.set_coinbase(coinbase);
        Ok(())
    }

    fn arm_mining(&self) -> bool {
        self.node.arm_mining_if_synced()
    }

    fn drain(&self) -> SvcDrain {
        let t0 = Instant::now();
        self.gate.hold(DRAIN_HOLD);
        *self.drained_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        self.stop_all();
        let still_exiting = self
            .gate
            .wait_until_no_children(crate::update_drain::EXIT_WAIT);
        SvcDrain {
            still_exiting,
            took_ms: t0.elapsed().as_millis() as u64,
        }
    }

    fn resume(&self) {
        *self.drained_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.gate.release();
        self.tick();
    }
}

// ---------------------------------------------------------------------------
// The executable's identity: a drained service exits for a replaced executable.
// ---------------------------------------------------------------------------

/// (device, inode, mtime) of a file; an update writes a new file, so one of them changes.
pub(crate) fn file_identity(path: &Path) -> Option<(u64, u64, i64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .ok()
        .map(|m| (m.dev(), m.ino(), m.mtime()))
}

/// What the loop does about a drain this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainStep {
    Keep,
    /// The executable was replaced: exit so the unit manager starts the new one.
    Restart,
    /// The drain outlived [`DRAIN_MAX`] with the same executable: release it.
    Release,
}

pub(crate) fn drain_step(
    drained_for: Option<Duration>,
    identity_at_start: Option<(u64, u64, i64)>,
    identity_now: Option<(u64, u64, i64)>,
    drain_max: Duration,
) -> DrainStep {
    let Some(for_) = drained_for else {
        return DrainStep::Keep;
    };
    // A missing file mid-replacement is not "replaced" yet: wait for the new one.
    if let (Some(a), Some(b)) = (identity_at_start, identity_now) {
        if a != b {
            return DrainStep::Restart;
        }
    }
    if for_ >= drain_max {
        DrainStep::Release
    } else {
        DrainStep::Keep
    }
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

static SIGNALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_sig: libc::c_int) {
    SIGNALLED.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    let handler = on_signal as extern "C" fn(libc::c_int);
    // SAFETY: the handler only stores to a static atomic, which is async-signal-safe; the
    // handler address is a valid `extern "C" fn(c_int)` for the life of the process.
    #[allow(unsafe_code)]
    unsafe {
        libc::signal(libc::SIGTERM, handler as libc::sighandler_t);
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
    }
}

// ---------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------

/// Serve and supervise until a stop. Returns the process exit code.
pub fn run_service(
    paths: &SvcPaths,
    host: Arc<NodeHost>,
    tick: Duration,
    drain_max: Duration,
) -> i32 {
    // SIGTERM / SIGINT (the unit manager's stop) end the loop below; the exit drains.
    install_signal_handlers();
    let server = match SvcServer::bind(paths) {
        Ok(s) => s,
        Err(crate::svc_server::BindError::AlreadyRunning) => {
            eprintln!("[svc] another background service is running for this user; exiting");
            return 0;
        }
        Err(e) => {
            eprintln!("[svc] {e}");
            return 1;
        }
    };
    let control = Control::new();
    let stop = Arc::new(AtomicBool::new(false));
    let serving = {
        let (host, control, stop) = (host.clone(), control.clone(), stop.clone());
        let host: Arc<dyn SvcHost> = host;
        std::thread::Builder::new()
            .name("svc-socket".into())
            .spawn(move || server.serve(host, control, stop))
    };
    let serving = match serving {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[svc] cannot start the socket thread: {e}");
            return 1;
        }
    };
    let watched = crate::svc::exe_to_watch(
        std::env::var_os("APPIMAGE").map(PathBuf::from),
        std::env::current_exe().ok(),
    );
    let identity_at_start = watched.as_deref().and_then(file_identity);
    eprintln!(
        "[svc] serving on {} (pid {})",
        paths.socket.display(),
        std::process::id()
    );
    host.tick();
    let mut last_tick = Instant::now();
    let code = loop {
        if let Some(code) = control.wait(Duration::from_millis(250)) {
            break code;
        }
        if SIGNALLED.load(Ordering::SeqCst) {
            break 0;
        }
        let step = drain_step(
            host.drained_since().map(|t| t.elapsed()),
            identity_at_start,
            watched.as_deref().and_then(file_identity),
            drain_max,
        );
        match step {
            DrainStep::Restart => {
                eprintln!("[svc] the executable was replaced during the drain; restarting");
                break EXIT_RESTART;
            }
            DrainStep::Release => {
                eprintln!("[svc] the drain outlived its limit with the same executable; resuming");
                host.resume();
            }
            DrainStep::Keep => {}
        }
        if last_tick.elapsed() >= tick {
            host.tick();
            last_tick = Instant::now();
        }
    };
    // Every exit drains: the node first (its RocksDB), then kubo, then the socket goes.
    let report = host.drain();
    if !report.still_exiting.is_empty() {
        eprintln!(
            "[svc] still exiting at shutdown: {:?}",
            report.still_exiting
        );
    }
    stop.store(true, Ordering::SeqCst);
    let _ = serving.join();
    eprintln!("[svc] exit {code}");
    code
}

/// Resolve a bundled sidecar next to this executable (Contents/MacOS on macOS, the AppImage
/// mount's usr/bin, /usr/bin for the .deb), with the dev/test override env first.
fn sibling_bin(env: &str, name: &str) -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os(env).map(PathBuf::from) {
        if p.exists() {
            return Ok(p);
        }
        return Err(format!("{env} set but not found: {}", p.display()));
    }
    let exe = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "the executable has no directory".to_string())?;
    let p = dir.join(name);
    if p.exists() {
        Ok(p)
    } else {
        Err(format!("{name} not found next to {}", exe.display()))
    }
}

/// The log directory the app uses (`app_log_dir()`): `~/Library/Logs/<id>` on macOS,
/// `<data dir>/<id>/logs` elsewhere.
pub(crate) fn headless_log_dir(data_root: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Logs").join(crate::svc::APP_IDENTIFIER);
        }
    }
    data_root.join("logs")
}

/// `citrate-core --svc`. Never returns to the app start-up.
pub fn svc_main() -> i32 {
    let paths = match SvcPaths::headless() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[svc] {e}");
            return 1;
        }
    };
    crate::sidecar_logs::init(&headless_log_dir(&paths.data_root));
    let node_bin = match sibling_bin("CITRATE_NODE_BIN", "citrate") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[svc] {e}");
            return 1;
        }
    };
    let ipfs_bin = match sibling_bin("CITRATE_IPFS_BIN", "ipfs") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[svc] {e}");
            return 1;
        }
    };
    // The app's own startup cleanup (SCL-S0.1): leftovers of an earlier run whose parent is gone.
    // This process's children, and children of a live app instance, are never signalled.
    let main_exe = std::env::current_exe().ok();
    crate::startup_barrier::StartupBarrier::global().run_cleanup(|| {
        let _ = crate::owned_cleanup::cleanup(
            &[node_bin.clone(), ipfs_bin.clone()],
            main_exe.as_deref(),
        );
    });
    let node = NodeManager::new(
        // The node's at-rest storage key, in the same LEGACY keyring namespace as in-process
        // (repointing it would orphan the encrypted data dir).
        Box::new(crate::custody::OsKeyring::legacy()),
        node_bin,
        paths.data_root.join("node"),
        paths.data_root.join("node").join("crash-records.jsonl"),
        crate::node::NODE_LOCAL_RPC_URL,
    );
    let ipfs = IpfsManager::new(
        ipfs_bin,
        paths.data_root.join("ipfs"),
        paths.data_root.join("ipfs").join("crash-records.jsonl"),
    );
    let host = Arc::new(NodeHost::new(
        node,
        ipfs,
        paths.clone(),
        SpawnGate::global(),
    ));
    run_service(&paths, host, TICK, DRAIN_MAX)
}

#[cfg(test)]
#[path = "svc_run_tests.rs"]
mod tests;
