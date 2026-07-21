//! citrate-core — SidecarSupervisor (CORE-C1.0). @rule8-adjacent · spawns OS
//! processes.
//!
//! The reusable native process-supervision primitive that Phase C's sidecars
//! (citrate-node, node-agent, and later mcp_serve / llama-server) run under.
//! C1.0 is the STANDALONE primitive: no external binary is wired here (that is
//! C1.1/C1.2). Its status API is shaped so a future `NodeDomain` adapter maps
//! straight onto it (state / pid / restarts / last_crash).
//!
//! ## Security model (why this is process-spawn-sensitive)
//! - **Injection-proof by construction.** A child is spawned via
//!   [`std::process::Command`] with an explicit program path + an argument
//!   VECTOR. There is NO `sh -c`, no shell, and no string interpolation
//!   anywhere on the spawn path, so shell metacharacters in an arg (`; rm -rf`,
//!   `$(...)`, backticks) are delivered to the child as literal `argv` and are
//!   never interpreted by a shell. The [`SidecarSpec`] type has no field that
//!   can smuggle a shell in.
//! - **No orphans.** Every supervised child is killed on supervisor teardown
//!   (explicit `stop` or `Drop`), so app-exit leaves no stray process.
//! - **No fork-bomb.** A child that exits instantly does NOT trigger a tight
//!   restart loop: restarts use bounded exponential backoff with a max-retry
//!   cap; once the cap is hit the supervisor enters a terminal `Failed` state
//!   and stops respawning.
//!
//! ## Design (matches the repo's blocking-thread + Mutex pattern)
//! Each [`Supervisor`] owns a monitor thread (like `rpc.rs`'s spawned blocking
//! thread) that spawns the child, waits for exit, and — unless the exit was an
//! intentional stop — records a crash and schedules a backed-off restart. Shared
//! state (the [`SupervisorStatus`] + a control channel) is a `Mutex`/`Arc`
//! (like `custody.rs`). Time is taken from an injected [`Clock`] so tests are
//! deterministic (no wall-clock races — the B1.1-F-4 lesson).
//!
//! ## Platform note (honesty)
//! Graceful stop is SIGTERM → wait → SIGKILL on Unix (macOS + Linux), via a
//! direct `libc::kill` (libc is already in the tree transitively; no new crate).
//! On Windows (OUT of beta scope per O-4) there is no SIGTERM: the fallback is a
//! hard `Child::kill` (equivalent to SIGKILL), so graceful shutdown is
//! best-effort there. This is cfg-gated and called out as a caveat.

// C1.0 is the standalone primitive: the supervisor is consumed by C1.1/C1.2 and
// exercised by its own tests. Some constructor/status surface is only reached by
// those later wirings + the tests until the NodeDomain adapter lands.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Clock seam — injected so backoff/crash timestamps are deterministic in tests
// (no reliance on wall-clock races; the B1.1-F-4 lesson).
// ---------------------------------------------------------------------------

/// A monotonic-ish wall-clock source, injectable for deterministic tests. The
/// library NEVER calls `SystemTime::now()` inline on the crash/backoff path; it
/// takes the timestamp from here. Production uses [`SystemClock`]; tests use a
/// fake that returns a controlled sequence.
pub trait Clock: Send + Sync {
    /// Current wall-clock time in unix milliseconds (for crash records).
    fn now_unix_ms(&self) -> u64;
}

/// The real system clock. Clamps a pre-epoch clock to 0 (same convention as
/// `custody::now_unix_ms`).
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Backoff policy — the anti-fork-bomb bound.
// ---------------------------------------------------------------------------

/// Bounded exponential-backoff restart policy. A child that crashes is respawned
/// after `base * multiplier^(attempt-1)` (capped at `max_delay`); after
/// `max_retries` CONSECUTIVE failed restarts the supervisor gives up and enters
/// the terminal [`SupervisorState::Failed`] state. The cap + max-retries are
/// what stop a crash-looping (or instantly-exiting) child from fork-bombing the
/// host — remove either and an instant-exit child would spin unbounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackoffPolicy {
    /// The first restart delay.
    pub base_delay: Duration,
    /// Delay multiplier per consecutive failure (>= 1). 2 = classic doubling.
    pub multiplier: u32,
    /// The maximum single restart delay (the cap). Bounds the exponential so a
    /// long-lived crash loop does not schedule absurd delays.
    pub max_delay: Duration,
    /// Maximum CONSECUTIVE restart attempts before giving up (terminal Failed).
    /// This is the hard fork-bomb bound: attempts are finite.
    pub max_retries: u32,
}

impl BackoffPolicy {
    /// A sane default: 500ms base, doubling, capped at 30s, 8 retries.
    pub fn new() -> Self {
        BackoffPolicy {
            base_delay: Duration::from_millis(500),
            multiplier: 2,
            max_delay: Duration::from_secs(30),
            max_retries: 8,
        }
    }

    /// The delay before the `attempt`-th consecutive restart (1-indexed). Uses
    /// saturating arithmetic so a large attempt count can never overflow into a
    /// tiny delay — it saturates at `max_delay`. Returns `max_delay` for
    /// attempt 0 defensively (callers pass >= 1).
    pub fn delay_for(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return self.max_delay.min(self.base_delay);
        }
        // base * multiplier^(attempt-1), saturating.
        let mut delay = self.base_delay;
        for _ in 1..attempt {
            delay = delay.saturating_mul(self.multiplier).min(self.max_delay);
            if delay >= self.max_delay {
                break;
            }
        }
        delay.min(self.max_delay)
    }
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Health check (optional) — a failing check is treated like a crash.
// ---------------------------------------------------------------------------

/// An optional liveness probe run periodically while the child is Running. A
/// failing probe is treated as an unhealthy child and triggers a restart (the
/// child is stopped and respawned under the same backoff bound), so a wedged-
/// but-alive process is recovered. The probe is a boxed closure so a caller can
/// supply any check (TCP connect, HTTP 200, a pidfile) without this module
/// depending on a transport.
pub struct HealthCheck {
    /// How often to run the probe while Running. This interval is ALSO the
    /// probe's timeout: the probe runs off the monitor thread (F-2), and a probe
    /// that has not answered within `interval` is treated as unhealthy so a
    /// wedged probe recovers the child instead of stalling supervision.
    pub interval: Duration,
    /// The probe: returns `true` if the child is healthy. `Arc` (not `Box`) so
    /// the runner can hand it to a dedicated probe thread — the probe NEVER runs
    /// inline on the monitor thread, so even a blocking/hanging probe cannot
    /// stall crash detection or teardown (F-2).
    pub probe: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl std::fmt::Debug for HealthCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HealthCheck")
            .field("interval", &self.interval)
            .field("probe", &"<fn>")
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SidecarSpec — the typed, shell-free spawn descriptor.
// ---------------------------------------------------------------------------

/// A typed description of a sidecar process to supervise. There is deliberately
/// NO "command string" / "shell" field — the binary is named by an explicit
/// path and its arguments by a `Vec<String>`, so the spawn path can never route
/// through a shell. This is the structural guarantee behind injection-proofing.
pub struct SidecarSpec {
    /// A human name for status/log/crash-record correlation.
    pub name: String,
    /// The binary to execute, by explicit path. Spawned directly — never via a
    /// shell. (C1.1 constrains this to a bundled/allowlisted dir.)
    pub bin: PathBuf,
    /// Arguments, one element per argv slot. Passed to `Command::args` verbatim;
    /// shell metacharacters here are literal and never interpreted.
    pub args: Vec<String>,
    /// Extra environment for the child, as explicit key/value pairs.
    pub env: Vec<(String, String)>,
    /// Working directory for the child, if any.
    pub workdir: Option<PathBuf>,
    /// Optional periodic liveness probe (see [`HealthCheck`]).
    pub health_check: Option<HealthCheck>,
}

impl std::fmt::Debug for SidecarSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidecarSpec")
            .field("name", &self.name)
            .field("bin", &self.bin)
            .field("args", &self.args)
            .field(
                "env_keys",
                &self.env.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            )
            .field("workdir", &self.workdir)
            .field("has_health_check", &self.health_check.is_some())
            .finish()
    }
}

impl SidecarSpec {
    /// Build a spec for `name` running `bin` with `args`. Env/workdir/health
    /// default to empty; use the builder-ish setters or set fields directly.
    pub fn new(name: impl Into<String>, bin: impl Into<PathBuf>, args: Vec<String>) -> Self {
        SidecarSpec {
            name: name.into(),
            bin: bin.into(),
            args,
            env: Vec::new(),
            workdir: None,
            health_check: None,
        }
    }

    /// Build the `std::process::Command` for this spec. **This is the single
    /// spawn-construction site**, and it is shell-free by construction:
    /// `Command::new(path)` + `.args(vec)` never invokes a shell, so argv is
    /// delivered literally. BOTH stdout and stderr are piped so the supervisor's
    /// reader threads can stream every line into the bounded log ring (Q-A.2:
    /// the node logs to these streams and the UI must show REAL output — a GUI
    /// child's inherited stdout is otherwise dropped). stdin is null.
    fn to_command(&self) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.args(&self.args);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &self.workdir {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd
    }
}

// ---------------------------------------------------------------------------
// Status API — shaped so a NodeDomain adapter maps straight onto it.
// ---------------------------------------------------------------------------

/// The supervision state machine. A future `NodeDomain.status().state` maps onto
/// this: `Off`→"stopped", `Starting`→"starting", `Running`→"running",
/// `Backoff`→"restarting", `Failed`→"failed".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorState {
    /// Not started, or intentionally stopped. No child.
    Off,
    /// A child is being spawned (transient).
    Starting,
    /// A child is alive (and, if a health check exists, was last seen healthy).
    Running,
    /// The child crashed; a restart is scheduled at `next_retry_ms` (unix ms).
    Backoff { next_retry_ms: u64 },
    /// The child crash-looped past `max_retries`; the supervisor gave up. No
    /// child, no further restarts (the terminal fork-bomb stop).
    Failed,
}

/// A single crash record — written to the crash-record file on each unexpected
/// exit. Timestamp comes from the injected [`Clock`] (not an inline
/// `SystemTime::now`), so tests are deterministic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CrashRecord {
    /// The sidecar name (for multi-sidecar crash logs).
    pub name: String,
    /// Wall-clock crash time, unix ms (from the injected clock).
    pub at_unix_ms: u64,
    /// The child's exit status, rendered (e.g. "exit code: 1", "signal: 9").
    pub exit: String,
    /// A BOUNDED tail of the child's stderr (last [`STDERR_TAIL_BYTES`] bytes),
    /// lossy-UTF8. Bounded so a chatty child cannot blow up the record file.
    pub stderr_tail: String,
    /// Which consecutive-restart attempt this crash triggered (1-indexed). 0
    /// means the FIRST run crashed (before any restart).
    pub restart_attempt: u32,
}

/// One captured line from the supervised child's stdout or stderr. The node
/// logs progress to these streams (e.g. `citrate_network::sync: Validated and
/// imported 32/32 blocks (height 1540-1571)`); the supervisor pipes both streams
/// and keeps a bounded ring of the most recent lines so the UI can show REAL
/// node output in a packaged build (Q-A.2/Q-B.2), never a fabricated template.
/// serde camelCase so `stream`/`line`/`ts` map straight onto the bridge shape.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// Wall-clock capture time, unix ms (from the injected clock).
    pub ts: u64,
    /// Which stream the line came from: `"out"` (stdout) or `"err"` (stderr).
    pub stream: String,
    /// The line text (one child stdout/stderr line, trailing newline stripped).
    pub line: String,
}

/// Max lines retained in the per-process log ring buffer (drop-oldest). Bounds
/// memory: a chatty node cannot grow the buffer without bound — the oldest line
/// is evicted once the cap is reached (mirrors the bounded crash-record tail).
pub const LOG_RING_CAPACITY: usize = 500;

/// A bounded, drop-oldest ring of the most recent captured [`LogLine`]s. Held on
/// the supervisor's shared state so a reader thread pushes and a poll (the
/// `node_logs` command) snapshots it. Bounded at [`LOG_RING_CAPACITY`] so it can
/// never grow without bound (the anti-bloat invariant, like the stderr tail).
#[derive(Debug, Default)]
pub struct LogRing {
    lines: Mutex<VecDeque<LogLine>>,
}

impl LogRing {
    fn new() -> Self {
        LogRing {
            lines: Mutex::new(VecDeque::with_capacity(LOG_RING_CAPACITY)),
        }
    }

    /// Push one line, evicting the oldest if the ring is at capacity.
    fn push(&self, line: LogLine) {
        let mut q = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() >= LOG_RING_CAPACITY {
            q.pop_front();
        }
        q.push_back(line);
    }

    /// Snapshot the current lines oldest→newest (a clone, so the caller never
    /// holds the lock). The `node_logs` command returns this.
    pub fn snapshot(&self) -> Vec<LogLine> {
        self.lines
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

/// A snapshot of supervision status. Designed for the NodeDomain adapter:
/// `state` + `pid` + `restarts` + `last_crash` are exactly what an operations
/// surface renders. Cloneable so a poll from the Tauri runtime takes a snapshot
/// without holding the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorStatus {
    /// The sidecar name.
    pub name: String,
    /// The current state.
    pub state: SupervisorState,
    /// The OS pid of the live child, if one is running.
    pub pid: Option<u32>,
    /// Total restarts performed over this supervisor's lifetime (monotone).
    pub restarts: u32,
    /// The most recent crash record, if any crash has occurred.
    pub last_crash: Option<CrashRecord>,
}

// ---------------------------------------------------------------------------
// Config for the supervisor.
// ---------------------------------------------------------------------------

/// How long to wait after SIGTERM before escalating to SIGKILL on a graceful
/// stop. Bounded so `stop()` always returns promptly.
const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(5);

/// Default sustained-healthy window: once a respawned child has stayed `Running`
/// this long (measured on the injected [`Clock`]), the consecutive-failure
/// counter resets to 0 (see [`SupervisorConfig::healthy_after`] + the F-1 fix).
const DEFAULT_HEALTHY_AFTER: Duration = Duration::from_secs(30);

/// Hard upper bound on how long `Drop`/`stop` will block joining the monitor
/// thread, so a hung child or a wedged health probe can never hang teardown
/// forever (the F-2 liveness fix). The monitor's own SIGKILL escalation makes a
/// child die within `stop_grace`; this bounds the residual wait on the join.
const DEFAULT_JOIN_TIMEOUT: Duration = Duration::from_secs(15);

/// Max bytes of stderr retained for a crash record (the bounded tail).
pub const STDERR_TAIL_BYTES: usize = 4096;

/// Supervisor construction config.
pub struct SupervisorConfig {
    /// The process to supervise.
    pub spec: SidecarSpec,
    /// The restart backoff policy (the fork-bomb bound).
    pub backoff: BackoffPolicy,
    /// Where crash records are appended (JSON-lines). Its parent dir is created.
    pub crash_record_path: PathBuf,
    /// How long to wait for a graceful SIGTERM exit before SIGKILL.
    pub stop_grace: Duration,
    /// How long a respawned child must stay continuously `Running` before the
    /// consecutive-failure counter resets to 0 (the F-1 fix). A child that
    /// crash-loops faster than this NEVER resets, so the fork-bomb cap still
    /// bounds it. Measured on the injected [`Clock`].
    pub healthy_after: Duration,
    /// Hard cap on how long teardown (`stop`/`Drop`) blocks joining the monitor
    /// thread, so a hung child or wedged probe cannot hang teardown forever
    /// (the F-2 fix).
    pub join_timeout: Duration,
    /// The clock (inject a fake in tests).
    pub clock: Arc<dyn Clock>,
}

impl SupervisorConfig {
    /// A config with default backoff / grace / healthy-after / system clock.
    pub fn new(spec: SidecarSpec, crash_record_path: impl Into<PathBuf>) -> Self {
        SupervisorConfig {
            spec,
            backoff: BackoffPolicy::new(),
            crash_record_path: crash_record_path.into(),
            stop_grace: DEFAULT_STOP_GRACE,
            healthy_after: DEFAULT_HEALTHY_AFTER,
            join_timeout: DEFAULT_JOIN_TIMEOUT,
            clock: Arc::new(SystemClock),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal control messages (monitor thread <- controller).
// ---------------------------------------------------------------------------

enum ControlMsg {
    /// Ask the monitor to gracefully stop the child and go Off (no restart).
    Stop,
    /// Ask the monitor thread to exit its loop entirely (teardown).
    Shutdown,
}

/// Shared state between the controller handle and the monitor thread.
struct Shared {
    status: Mutex<SupervisorStatus>,
    /// The injected clock, shared so `backoff_or_control` can anchor
    /// `Backoff { next_retry_ms }` on the same time source as crash records.
    /// (The monitor OWNS the `Child` and does all signalling; the controller
    /// never signals a child by pid — it sends control messages instead.)
    clock: Arc<dyn Clock>,
    /// The bounded ring of the most recent captured stdout/stderr lines. Reader
    /// threads push; the `logs()` accessor snapshots it. SURVIVES restarts (it
    /// is not cleared on respawn) so a crash+restart keeps the tail visible.
    logs: LogRing,
}

impl Shared {
    fn set_state(&self, state: SupervisorState) {
        let mut s = self.status.lock().unwrap_or_else(|e| e.into_inner());
        s.state = state;
    }

    fn snapshot(&self) -> SupervisorStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

// ---------------------------------------------------------------------------
// The Supervisor handle.
// ---------------------------------------------------------------------------

/// The controller handle for a supervised sidecar. Owns the monitor thread's
/// join handle + a control channel. Dropping it (or calling [`Supervisor::stop`]
/// / [`Supervisor::shutdown`]) tears the child down — no orphans.
pub struct Supervisor {
    shared: Arc<Shared>,
    control: Sender<ControlMsg>,
    monitor: Option<JoinHandle<()>>,
    name: String,
    /// Hard cap on how long teardown blocks joining the monitor thread (F-2).
    join_timeout: Duration,
}

impl Supervisor {
    /// Spawn the sidecar and start supervising it. Returns immediately; the
    /// monitor thread does the spawning + waiting + restarting. The initial
    /// status is `Starting`. Returns `Err` if the OS refuses the monitor thread
    /// (e.g. thread exhaustion) — no `.expect`, so the caller decides (F-2 LOW).
    pub fn start(config: SupervisorConfig) -> std::io::Result<Self> {
        let name = config.spec.name.clone();
        let join_timeout = config.join_timeout;
        let shared = Arc::new(Shared {
            status: Mutex::new(SupervisorStatus {
                name: name.clone(),
                state: SupervisorState::Starting,
                pid: None,
                restarts: 0,
                last_crash: None,
            }),
            clock: config.clock.clone(),
            logs: LogRing::new(),
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let monitor_shared = shared.clone();
        let monitor = std::thread::Builder::new()
            .name(format!("supervisor:{name}"))
            .spawn(move || run_monitor(config, monitor_shared, rx))?;
        Ok(Supervisor {
            shared,
            control: tx,
            monitor: Some(monitor),
            name,
            join_timeout,
        })
    }

    /// A cloneable, read-only status snapshot. Safe to poll from the Tauri
    /// runtime while the monitor thread runs (takes the lock briefly, clones).
    pub fn status(&self) -> SupervisorStatus {
        self.shared.snapshot()
    }

    /// A snapshot of the recent captured stdout/stderr lines (oldest→newest,
    /// bounded at [`LOG_RING_CAPACITY`]). Safe to poll from the Tauri runtime;
    /// the `node_logs` command folds this into the Node LOG panel so a packaged
    /// build shows REAL node output (Q-A.2/Q-B.2), never a fabricated template.
    pub fn logs(&self) -> Vec<LogLine> {
        self.shared.logs.snapshot()
    }

    /// The sidecar name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gracefully stop the child (SIGTERM → grace → SIGKILL) and leave the
    /// supervisor `Off` with NO restart. An intentional stop is NOT a crash:
    /// the monitor marks it so and does not write a crash record or respawn.
    /// Idempotent — stopping an already-stopped supervisor is a no-op.
    pub fn stop(&self) {
        // Best-effort: if the monitor already exited, the send fails harmlessly.
        let _ = self.control.send(ControlMsg::Stop);
        // Block until the monitor acknowledges by reaching Off/Failed, bounded
        // by a generous timeout so a wedged child cannot hang the caller. The
        // monitor's own SIGKILL escalation guarantees the child dies within
        // grace.
        self.wait_until(
            |st| matches!(st, SupervisorState::Off | SupervisorState::Failed),
            Duration::from_secs(10),
        );
    }

    /// Tear the supervisor down entirely: stop the child and join the monitor
    /// thread. Called by `Drop`; exposed so callers can join deterministically.
    ///
    /// The join is BOUNDED by `join_timeout` (F-2): a `JoinHandle` has no timed
    /// join in std, so we poll `is_finished()` up to the bound and only `join()`
    /// once it has finished (an immediate, non-blocking join). If the monitor
    /// has not finished within the bound — which cannot happen from a hung child
    /// (SIGKILL escalation kills it within `stop_grace`) but could in principle
    /// from a wedged detached probe thread — we DETACH rather than block teardown
    /// forever. The detached monitor holds no supervisor lock and owns no live
    /// child at that point (it terminates the child before exiting), so this is
    /// safe: teardown returns and the process is not hung.
    pub fn shutdown(&mut self) {
        let _ = self.control.send(ControlMsg::Shutdown);
        if let Some(handle) = self.monitor.take() {
            let deadline = std::time::Instant::now() + self.join_timeout;
            while !handle.is_finished() {
                if std::time::Instant::now() >= deadline {
                    // Bound exceeded: detach (drop the handle) instead of an
                    // unbounded `join()`. No orphan/lock is leaked (see doc).
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = handle.join();
        }
    }

    /// Poll the status until `pred` holds or `timeout` elapses. Test/helper
    /// convenience; uses a short sleep between polls (the monitor updates status
    /// under the lock). Returns the final snapshot.
    pub fn wait_until(
        &self,
        pred: impl Fn(&SupervisorState) -> bool,
        timeout: Duration,
    ) -> SupervisorStatus {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let st = self.shared.snapshot();
            if pred(&st.state) {
                return st;
            }
            if std::time::Instant::now() >= deadline {
                return st;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Supervisor {
    /// No-orphan guarantee: dropping the supervisor tells the monitor to stop
    /// the child and joins it, so the child is dead before the process exits.
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// The monitor thread — the supervision loop.
// ---------------------------------------------------------------------------

/// Spawn the child from the spec, returning the `Child`. Single spawn site; see
/// [`SidecarSpec::to_command`] for the shell-free guarantee.
fn spawn_child(spec: &SidecarSpec) -> std::io::Result<Child> {
    spec.to_command().spawn()
}

/// Take the child's piped stdout + stderr and spawn one DETACHED reader thread
/// per stream that reads lines and pushes each into the shared log ring. The
/// threads are detached (not joined on teardown) so they can NEVER stall
/// shutdown (F-2 liveness): a reader blocked on a read simply unblocks with EOF
/// when the pipe closes as the child dies (SIGKILL escalation guarantees that),
/// then exits on its own. They hold only an `Arc<Shared>` (no supervisor lock is
/// held across a blocking read), so a lingering reader leaks nothing and blocks
/// nothing. Best-effort: if the OS refuses a reader thread the stream is simply
/// not captured (a missing log line is never a supervision failure).
fn spawn_log_readers(child: &mut Child, name: &str, shared: &Arc<Shared>) {
    use std::io::{BufRead, BufReader};
    if let Some(out) = child.stdout.take() {
        let shared = shared.clone();
        let _ = std::thread::Builder::new()
            .name(format!("supervisor:log-out:{name}"))
            .spawn(move || {
                let reader = BufReader::new(out);
                for line in reader.lines().map_while(std::result::Result::ok) {
                    shared.logs.push(LogLine {
                        ts: shared.clock.now_unix_ms(),
                        stream: "out".to_string(),
                        line,
                    });
                }
            });
    }
    if let Some(err) = child.stderr.take() {
        let shared = shared.clone();
        let _ = std::thread::Builder::new()
            .name(format!("supervisor:log-err:{name}"))
            .spawn(move || {
                let reader = BufReader::new(err);
                for line in reader.lines().map_while(std::result::Result::ok) {
                    shared.logs.push(LogLine {
                        ts: shared.clock.now_unix_ms(),
                        stream: "err".to_string(),
                        line,
                    });
                }
            });
    }
}

/// The BOUNDED stderr tail for a crash record, derived from the recent `err`
/// lines the reader thread captured into the ring (the reader thread now owns
/// the stderr pipe, so we no longer re-read `child.stderr`). Keeps only the
/// trailing [`STDERR_TAIL_BYTES`] so a chatty child cannot bloat the record —
/// the same anti-bloat bound as before, just sourced from the ring.
fn stderr_tail_from_ring(shared: &Arc<Shared>) -> String {
    let joined = shared
        .logs
        .snapshot()
        .into_iter()
        .filter(|l| l.stream == "err")
        .map(|l| l.line)
        .collect::<Vec<_>>()
        .join("\n");
    let bytes = joined.as_bytes();
    let start = bytes.len().saturating_sub(STDERR_TAIL_BYTES);
    // start is a byte index into a joined `\n`-delimited string; clamp to a char
    // boundary so the slice is valid UTF-8 (join output is UTF-8 by construction).
    let start = (start..bytes.len())
        .find(|&i| joined.is_char_boundary(i))
        .unwrap_or(joined.len());
    joined[start..].to_string()
}

/// Render a child exit status compactly for the crash record.
fn render_exit(status: &std::process::ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit code: {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return format!("signal: {sig}");
        }
    }
    "unknown exit".to_string()
}

/// Give the DETACHED stderr reader thread a brief window to drain the now-closed
/// pipe to EOF before we snapshot the ring for a crash record's tail. Adaptive +
/// bounded: it returns AS SOON AS an `err` line is present (the common case,
/// since a line-buffered pipe flushes on newline before the child exits), else
/// polls up to a small cap. Kept tiny so it does not slow the crash/restart hot
/// path (the fork-bomb-bound negative control churns many restarts in <400ms).
fn drain_grace(shared: &Arc<Shared>) {
    let deadline = std::time::Instant::now() + Duration::from_millis(20);
    loop {
        if shared
            .logs
            .snapshot()
            .iter()
            .any(|l| l.stream == "err")
            || std::time::Instant::now() >= deadline
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Append one crash record as a JSON line to the crash-record file. Best-effort:
/// a failure to write the record must NOT crash the monitor (we log to stderr
/// and continue supervising). Parent dir is created.
fn append_crash_record(path: &std::path::Path, record: &CrashRecord) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("supervisor: cannot create crash-record dir: {e}");
            return;
        }
    }
    let line = match serde_json::to_string(record) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("supervisor: cannot serialize crash record: {e}");
            return;
        }
    };
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                eprintln!("supervisor: cannot append crash record: {e}");
            }
        }
        Err(e) => eprintln!("supervisor: cannot open crash-record file: {e}"),
    }
}

/// Send `signal` to `pid` on Unix. Uses a direct `libc::kill`; `libc` is already
/// in the dependency tree (via tauri/tokio), so this adds no new crate. A failure
/// (e.g. the process already reaped) is ignored — the caller re-checks liveness.
#[cfg(unix)]
fn signal_pid(pid: u32, signal: i32) {
    // SAFETY: `kill(2)` with a pid and a signal number is a well-defined libc
    // call with no memory effects. A stale pid returns ESRCH which we ignore.
    unsafe {
        libc::kill(pid as libc::pid_t, signal);
    }
}

/// Whether `pid` is still alive on Unix (signal 0 = existence probe, sends no
/// signal). Used to confirm SIGTERM took effect before the grace timeout.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // SAFETY: `kill(pid, 0)` performs the permission/existence check without
    // delivering a signal. Returns 0 if the process exists and we may signal it.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Gracefully terminate a child: SIGTERM, wait up to `grace` for exit, then
/// SIGKILL. On Windows (out of beta scope) there is no SIGTERM: fall back to a
/// hard kill. Reaps the child so no zombie remains. Returns once the child is
/// dead.
fn terminate_child(child: &mut Child, grace: Duration) {
    #[cfg(unix)]
    {
        let pid = child.id();
        signal_pid(pid, libc::SIGTERM);
        let deadline = std::time::Instant::now() + grace;
        loop {
            // Non-blocking reap: if the child has exited, `try_wait` returns Some.
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(_) => break,
            }
            if std::time::Instant::now() >= deadline || !pid_alive(pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Escalate: SIGKILL (cannot be caught) and reap.
        signal_pid(pid, libc::SIGKILL);
        let _ = child.wait();
    }
    #[cfg(not(unix))]
    {
        // Windows fallback (O-4: out of beta scope): no SIGTERM. Hard-kill.
        let _ = grace;
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// The monitor loop. Owns the child. Spawns → waits → on unexpected exit records
/// a crash + backs off + respawns, bounded by the backoff policy. Reacts to
/// `Stop` (graceful stop, go Off, no restart) and `Shutdown` (stop + exit loop).
fn run_monitor(config: SupervisorConfig, shared: Arc<Shared>, control: Receiver<ControlMsg>) {
    let SupervisorConfig {
        spec,
        backoff,
        crash_record_path,
        stop_grace,
        healthy_after,
        join_timeout: _join_timeout,
        clock,
    } = config;

    // CONSECUTIVE-failure counter → drives the backoff delay + the Failed cap.
    // Reset to 0 once a respawned child stays Running for `healthy_after` (F-1),
    // so an intermittently-crashing-but-recovering child is never permanently
    // Failed — while a fast crash-loop (which never reaches `healthy_after`)
    // never resets and still hits the cap.
    let mut consecutive_failures: u32 = 0;

    loop {
        // --- spawn ---
        shared.set_state(SupervisorState::Starting);
        let mut child = match spawn_child(&spec) {
            Ok(c) => c,
            Err(e) => {
                // Spawn itself failed (bad path / EACCES). Treat as a crash so
                // the same bounded backoff applies — a bad binary path cannot
                // fork-bomb either.
                let record = CrashRecord {
                    name: spec.name.clone(),
                    at_unix_ms: clock.now_unix_ms(),
                    exit: format!("spawn failed: {e}"),
                    stderr_tail: String::new(),
                    restart_attempt: consecutive_failures,
                };
                append_crash_record(&crash_record_path, &record);
                record_crash_status(&shared, &record);
                consecutive_failures += 1;
                if consecutive_failures > backoff.max_retries {
                    shared.set_state(SupervisorState::Failed);
                    return;
                }
                if backoff_or_control(
                    &shared,
                    &control,
                    backoff.delay_for(consecutive_failures),
                    clock.as_ref(),
                ) == LoopSignal::Terminate
                {
                    return;
                }
                continue;
            }
        };

        // Stream stdout + stderr into the shared log ring via detached reader
        // threads (Q-A.2). Done BEFORE publishing Running so no early line is
        // missed. The threads own the pipes; the crash tail is later derived
        // from the ring's `err` lines (not a re-read of child.stderr).
        spawn_log_readers(&mut child, &spec.name, &shared);

        // Publish Running + the live pid.
        {
            let mut s = shared.status.lock().unwrap_or_else(|e| e.into_inner());
            s.state = SupervisorState::Running;
            s.pid = Some(child.id());
        }

        // --- supervise: wait for exit OR a control message OR a health failure ---
        let report = supervise_running(
            &mut child,
            &control,
            spec.health_check.as_ref(),
            healthy_after,
            clock.as_ref(),
        );
        // F-1: a run that stayed healthy long enough resets the consecutive
        // counter, so the NEXT crash starts a fresh backoff ramp and an
        // intermittently-crashing child is never permanently Failed.
        if report.sustained_healthy {
            consecutive_failures = 0;
        }

        match report.outcome {
            RunOutcome::Stopped => {
                // Intentional stop: NOT a crash. No record, no restart. Off.
                terminate_child(&mut child, stop_grace);
                clear_pid(&shared);
                shared.set_state(SupervisorState::Off);
                // Wait for a resume/shutdown; C1.0 has no resume, so we block on
                // the channel until Shutdown (or the handle drops → channel
                // closes). This keeps the monitor alive to own no child.
                match control.recv() {
                    Ok(ControlMsg::Shutdown) | Err(_) => return,
                    Ok(ControlMsg::Stop) => {
                        // Already stopped; stay Off, keep waiting.
                        continue;
                    }
                }
            }
            RunOutcome::Shutdown => {
                terminate_child(&mut child, stop_grace);
                clear_pid(&shared);
                shared.set_state(SupervisorState::Off);
                return;
            }
            RunOutcome::Crashed { status } => {
                // Unexpected exit → crash record + backoff + restart. Reap first,
                // then give the detached stderr reader a brief window to drain the
                // now-closed pipe to EOF so the ring holds the final lines, then
                // derive the bounded tail from the ring's `err` lines (the reader
                // owns the pipe now — we no longer re-read child.stderr).
                let _ = child.wait(); // reap (already exited)
                drain_grace(&shared);
                let stderr_tail = stderr_tail_from_ring(&shared);
                let record = CrashRecord {
                    name: spec.name.clone(),
                    at_unix_ms: clock.now_unix_ms(),
                    exit: render_exit(&status),
                    stderr_tail,
                    restart_attempt: consecutive_failures,
                };
                append_crash_record(&crash_record_path, &record);
                record_crash_status(&shared, &record);
                clear_pid(&shared);

                consecutive_failures += 1;
                if consecutive_failures > backoff.max_retries {
                    // The fork-bomb bound: give up. Terminal Failed, no respawn.
                    shared.set_state(SupervisorState::Failed);
                    return;
                }
                let delay = backoff.delay_for(consecutive_failures);
                if backoff_or_control(&shared, &control, delay, clock.as_ref())
                    == LoopSignal::Terminate
                {
                    return;
                }
                // else: loop → respawn.
            }
            RunOutcome::Unhealthy => {
                // Health check failed: stop the (still-alive) child and treat it
                // as a crash so the same bounded backoff applies.
                terminate_child(&mut child, stop_grace);
                let record = CrashRecord {
                    name: spec.name.clone(),
                    at_unix_ms: clock.now_unix_ms(),
                    exit: "health check failed".to_string(),
                    stderr_tail: String::new(),
                    restart_attempt: consecutive_failures,
                };
                append_crash_record(&crash_record_path, &record);
                record_crash_status(&shared, &record);
                clear_pid(&shared);

                consecutive_failures += 1;
                if consecutive_failures > backoff.max_retries {
                    shared.set_state(SupervisorState::Failed);
                    return;
                }
                let delay = backoff.delay_for(consecutive_failures);
                if backoff_or_control(&shared, &control, delay, clock.as_ref())
                    == LoopSignal::Terminate
                {
                    return;
                }
            }
        }
    }
}

/// The result of a `supervise_running` wait.
enum RunOutcome {
    /// The child exited on its own (unexpected) → crash path.
    Crashed { status: std::process::ExitStatus },
    /// A `Stop` control message arrived.
    Stopped,
    /// A `Shutdown` control message arrived.
    Shutdown,
    /// A health-check probe failed (returned `false`) or hung past its timeout.
    Unhealthy,
}

/// What `supervise_running` observed during one Running episode: how the episode
/// ended (`outcome`) AND whether the child stayed continuously Running long
/// enough (`>= healthy_after`, measured on the injected clock) to count as a
/// SUSTAINED-healthy run. `sustained_healthy` is the F-1 reset trigger — the
/// monitor resets `consecutive_failures` to 0 when it is set.
struct RunReport {
    outcome: RunOutcome,
    sustained_healthy: bool,
}

/// A non-blocking, single-shot health-probe runner. The probe closure runs on
/// its OWN thread so a probe that BLOCKS (or hangs forever) cannot stall the
/// monitor's crash-detection / control-message handling (the F-2 liveness fix).
/// The monitor polls `poll()` each loop; a probe that does not answer within the
/// health interval is treated as unhealthy (a wedged probe is not "healthy").
struct ProbeRunner {
    handle: Option<JoinHandle<bool>>,
}

impl ProbeRunner {
    /// Spawn the probe on its own thread. The closure is `Arc`-shared so the
    /// monitor keeps supervising while it runs. Returns `None` if the OS refuses
    /// the thread (treated by the caller as "no probe in flight").
    fn spawn(probe: &Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        let p = probe.clone();
        let handle = std::thread::Builder::new()
            .name("supervisor:probe".into())
            .spawn(move || p())
            .ok();
        ProbeRunner { handle }
    }

    /// Whether a probe is currently in flight.
    fn in_flight(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    /// If the probe thread has finished, join it and return its result; else
    /// `None` (still running). Non-blocking.
    fn poll(&mut self) -> Option<bool> {
        match &self.handle {
            Some(h) if h.is_finished() => {
                // The thread is done; join is immediate. A panicked probe is
                // treated as unhealthy (a probe that panicked did not attest
                // health).
                let h = self.handle.take().expect("checked Some");
                Some(h.join().unwrap_or(false))
            }
            _ => None,
        }
    }
}

/// Wait while the child is Running: poll for exit, for a control message, and
/// (if configured) run the health probe OFF-THREAD on its interval. Returns as
/// soon as one of them fires. Uses short polling so a control message is handled
/// promptly without a blocking `wait()` that would ignore the channel, and the
/// health probe never blocks this loop (F-2). Also measures how long the child
/// has stayed continuously Running (on the injected `clock`) to decide whether
/// the run was SUSTAINED-healthy (F-1 reset trigger).
fn supervise_running(
    child: &mut Child,
    control: &Receiver<ControlMsg>,
    health: Option<&HealthCheck>,
    healthy_after: Duration,
    clock: &dyn Clock,
) -> RunReport {
    let poll = Duration::from_millis(20);
    let mut since_probe = Duration::ZERO;
    let started_ms = clock.now_unix_ms();
    let healthy_after_ms = healthy_after.as_millis() as u64;
    let mut sustained_healthy = false;
    // In-flight off-thread probe + how long it has been running (for the timeout).
    let mut probe: Option<ProbeRunner> = None;
    let mut probe_elapsed = Duration::ZERO;
    // A probe must answer within its own interval; a probe that has not answered
    // by this bound is treated as unhealthy (wedged probe != healthy).
    let probe_timeout = health.map(|hc| hc.interval).unwrap_or(Duration::ZERO);

    // Helper: mark sustained-healthy once the child has been Running long enough.
    macro_rules! refresh_health {
        () => {
            if !sustained_healthy {
                let elapsed = clock.now_unix_ms().saturating_sub(started_ms);
                if elapsed >= healthy_after_ms {
                    sustained_healthy = true;
                }
            }
        };
    }

    loop {
        refresh_health!();
        // 1) control message (non-blocking).
        match control.try_recv() {
            Ok(ControlMsg::Stop) => {
                return RunReport {
                    outcome: RunOutcome::Stopped,
                    sustained_healthy,
                }
            }
            Ok(ControlMsg::Shutdown) => {
                return RunReport {
                    outcome: RunOutcome::Shutdown,
                    sustained_healthy,
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Controller dropped → treat as shutdown (no orphan).
                return RunReport {
                    outcome: RunOutcome::Shutdown,
                    sustained_healthy,
                };
            }
        }
        // 2) child exit (non-blocking reap).
        match child.try_wait() {
            Ok(Some(status)) => {
                return RunReport {
                    outcome: RunOutcome::Crashed { status },
                    sustained_healthy,
                }
            }
            Ok(None) => {}
            Err(_) => {
                // Cannot query the child; treat as crashed so we recover.
                return RunReport {
                    outcome: RunOutcome::Crashed {
                        status: default_failed_status(),
                    },
                    sustained_healthy,
                };
            }
        }
        // 3) health probe, run OFF-THREAD and bounded by a timeout so it can
        //    never stall this loop (F-2).
        if let Some(hc) = health {
            match &mut probe {
                // A probe is in flight: check for its result or a timeout.
                Some(runner) => {
                    if let Some(healthy) = runner.poll() {
                        probe = None;
                        probe_elapsed = Duration::ZERO;
                        if !healthy {
                            return RunReport {
                                outcome: RunOutcome::Unhealthy,
                                sustained_healthy,
                            };
                        }
                    } else {
                        probe_elapsed += poll;
                        if probe_elapsed >= probe_timeout {
                            // The probe is wedged past its interval — unhealthy.
                            // We DROP the runner (its detached thread may still
                            // be blocked, but it holds no supervisor lock and
                            // cannot block teardown). Restart recovers the child.
                            return RunReport {
                                outcome: RunOutcome::Unhealthy,
                                sustained_healthy,
                            };
                        }
                    }
                }
                // No probe in flight: start one when the interval elapses.
                None => {
                    since_probe += poll;
                    if since_probe >= hc.interval {
                        since_probe = Duration::ZERO;
                        probe = Some(ProbeRunner::spawn(&hc.probe));
                        probe_elapsed = Duration::ZERO;
                    }
                }
            }
        }
        std::thread::sleep(poll);
    }
}

/// A synthetic "failed" exit status for the can't-query-child edge (Unix: signal
/// SIGKILL; other: exit 1). Only used when `try_wait` itself errors.
fn default_failed_status() -> std::process::ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(9)
    }
    #[cfg(not(unix))]
    {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(1)
    }
}

/// A control signal from the backoff wait: keep looping, or terminate the
/// monitor.
#[derive(PartialEq, Eq)]
enum LoopSignal {
    Continue,
    Terminate,
}

/// Sleep out the backoff delay, but wake early on a control message. Publishes
/// `Backoff { next_retry_ms }` so a poller sees when the next attempt is due.
/// A `Stop` during backoff cancels the pending restart (Off, terminate the
/// monitor's restart intent by going Off and waiting); a `Shutdown` or a closed
/// channel terminates the monitor.
fn backoff_or_control(
    shared: &Arc<Shared>,
    control: &Receiver<ControlMsg>,
    delay: Duration,
    clock: &dyn Clock,
) -> LoopSignal {
    let next_retry_ms = clock.now_unix_ms().saturating_add(delay.as_millis() as u64);
    shared.set_state(SupervisorState::Backoff { next_retry_ms });
    match control.recv_timeout(delay) {
        // Timed out → time to retry: keep looping (respawn).
        Err(RecvTimeoutError::Timeout) => LoopSignal::Continue,
        // Stop during backoff: cancel the pending restart. Go Off and wait for a
        // shutdown (mirrors the Stopped arm — no orphan child exists here).
        Ok(ControlMsg::Stop) => {
            shared.set_state(SupervisorState::Off);
            match control.recv() {
                Ok(ControlMsg::Shutdown) | Err(_) => LoopSignal::Terminate,
                Ok(ControlMsg::Stop) => LoopSignal::Terminate,
            }
        }
        Ok(ControlMsg::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
            shared.set_state(SupervisorState::Off);
            LoopSignal::Terminate
        }
    }
}

/// Record a crash into the shared status (bumps `restarts`, sets `last_crash`).
fn record_crash_status(shared: &Arc<Shared>, record: &CrashRecord) {
    let mut s = shared.status.lock().unwrap_or_else(|e| e.into_inner());
    s.restarts = s.restarts.saturating_add(1);
    s.last_crash = Some(record.clone());
}

/// Clear the live pid from the status (child is gone).
fn clear_pid(shared: &Arc<Shared>) {
    let mut s = shared.status.lock().unwrap_or_else(|e| e.into_inner());
    s.pid = None;
}

#[cfg(test)]
mod tests {
    include!("supervisor_tests.rs");
}
