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
//! intentional stop — records a crash and schedules a backed-off restart. Time is
//! taken from an injected [`Clock`] so tests are deterministic (no wall-clock
//! races — the B1.1-F-4 lesson).
//!
//! ## Acknowledged controls (SCL-S2, SC1)
//! Controls and the monitor share one lifecycle cell per service (`Mutex` +
//! `Condvar`, std only, no async runtime: D-4). A control commits the latest
//! intent (Start or Stop) with a new intent sequence and raises one coalescing
//! wake bit; there is no message queue. The monitor is the only writer of process
//! state and re-reads the intent at bounded points. A Stop wins in every phase and
//! a repeated Stop joins the pending one, so it can never respawn the child.
//! [`Supervisor::request_stop`] and [`Supervisor::request_start`] return a
//! [`ControlResult`]; `Accepted` is not completion: the [`Receipt`] says whether a
//! Stop was `Complete` or `Incomplete`. [`Supervisor::lifecycle`] returns the
//! D-12 [`LifecycleSnapshot`] (phase with `stopping` and `quarantined`, a separate
//! readiness field, counters as decimal strings).
//!
//! ## Readiness and retry credit (SCL-S3, SC2)
//! Readiness is separate from liveness. A service with a [`HealthCheck`] follows the
//! [`ReadinessProfile::Probed`] rule: a new incarnation is `Awaiting`, a fresh success of that
//! incarnation makes it `Ready`, a failed or timed-out sample in the start grace makes it
//! `Unavailable`, and `Ready` older than two probe intervals is `Stale`. Only attested health
//! resets its retry budget: successes spanning `healthy_after` with no gap. The grace and
//! elapsed time never do (bug (b)). A service without a probe is
//! [`ReadinessProfile::LivenessOnly`]: always `NotAssessed`, never `Ready`, and its budget resets
//! after `healthy_after` of continuous running. All of it is measured on the monotonic clock;
//! the injected [`Clock`] stamps crash records only. One probe worker runs at a time per service:
//! a worker that outlives its timeout or its incarnation keeps the slot until it really ends,
//! and its result is discarded. Bounding and quarantining such a worker is SC3 (SCL-S4).
//!
//! Only a member Start ([`Supervisor::request_start`]) gives a fresh retry budget. An automated
//! start or restart ([`Supervisor::request_auto_start`], [`Supervisor::request_auto_restart`])
//! carries the budget over and never revives a `Failed` service (INV-17).
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
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::thread::JoinHandle;
use std::time::Duration;

// ---------------------------------------------------------------------------
// externalBin resolution — the ONE correct place to find a bundled sidecar.
// ---------------------------------------------------------------------------

/// File names to try for the bundled sidecar `name`: `<name><exe_suffix>` first,
/// then the bare `name`. `exe_suffix` is `std::env::consts::EXE_SUFFIX`: ".exe"
/// on Windows (the externalBin is `<name>.exe` on disk), "" on macOS/Linux,
/// where this is just `[name]`.
pub fn sidecar_candidate_names(name: &str, exe_suffix: &str) -> Vec<String> {
    let suffixed = format!("{name}{exe_suffix}");
    if suffixed == name {
        vec![name.to_string()]
    } else {
        vec![suffixed, name.to_string()]
    }
}

/// Resolve a bundled Tauri `externalBin` sidecar by its bundled name.
///
/// Tauri v2 installs `externalBin` **next to the app's main executable**
/// (`Contents/MacOS/<name>` on macOS), NOT into the resource dir. The per-domain
/// resolvers joined the name onto `resource_dir()` (`Contents/Resources/`),
/// which does not exist in a packaged bundle, so `start()` failed its
/// `bin.exists()` guard with `BinaryNotFound` — masked in the UI as
/// "supervisor unavailable". This only ever bit the packaged DMG; dev uses the
/// `CITRATE_*_BIN` env override and never reaches here.
///
/// Prefer the executable's directory; fall back to the resource dir for
/// robustness across Tauri layouts; return an honest error that lists every
/// path tried (Rule 1 — never hide why it failed).
pub fn resolve_external_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    name: &str,
) -> std::result::Result<PathBuf, String> {
    use tauri::Manager;
    let mut tried: Vec<String> = Vec::new();
    // Try the platform executable suffix first, then the bare name. On Windows
    // the bundled externalBin is `<name>.exe` (EXE_SUFFIX = ".exe"); on
    // macOS/Linux EXE_SUFFIX is "" so this collapses to the bare name and the
    // Unix layout is unchanged. Without the suffixed candidate the packaged
    // Windows app cannot find any sidecar (`<name>` has no extension on disk)
    // and panics in the setup hook.
    let names = sidecar_candidate_names(name, std::env::consts::EXE_SUFFIX);
    // 1) Next to the current executable (Contents/MacOS/<name>) — the real
    //    externalBin home in a packaged app.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for n in &names {
                let cand = dir.join(n);
                if cand.exists() {
                    return Ok(cand);
                }
                tried.push(cand.display().to_string());
            }
        }
    }
    // 2) Resource dir fallback (Contents/Resources/<name>).
    if let Ok(res) = app.path().resource_dir() {
        for n in &names {
            let cand = res.join(n);
            if cand.exists() {
                return Ok(cand);
            }
            tried.push(cand.display().to_string());
        }
    }
    Err(format!(
        "bundled sidecar '{name}' not found (tried: {})",
        tried.join(", ")
    ))
}

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
    /// A startup GRACE window (Docker's `--start-period`): for this long AFTER a
    /// (re)spawn, a failing OR wedged probe is NOT counted as unhealthy — the
    /// child is presumed still starting up, so no restart is triggered. Once the
    /// grace has elapsed a failing probe restarts the child as before. A probe
    /// that SUCCEEDS during the grace is honoured immediately (the child is up).
    /// `Duration::ZERO` disables the grace (every probe counts from the start),
    /// which is the pre-existing behaviour for sidecars that come up instantly.
    /// This exists because a slow-cold-starting child (e.g. `llama-server` mmap-
    /// ing a multi-GB GGUF + building the Metal graph) takes far longer than one
    /// probe interval to bind its port, and without a grace the first probe kills
    /// it mid-load and it crash-loops to terminal `Failed` before it can serve.
    pub grace: Duration,
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
            .field("grace", &self.grace)
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
    /// Optional hooks for the processes the child itself starts (see [`ChildWatch`]).
    pub child_watch: Option<ChildWatch>,
}

/// Hooks for the processes a supervised child starts on its own (SCL-S0.3).
///
/// The supervisor never signals those processes; it gives the caller the two moments when it can
/// act on facts it observed itself:
///
/// - `observe(pid)` runs on the monitor thread with the child's pid while the child is this
///   process's own **unreaped** child, so `pid` names that child and no other process (an exited
///   child keeps its id until it is reaped). It runs every `interval` while the child runs, and
///   right before each stop signal (SIGTERM, and SIGKILL when the grace runs out).
/// - `ended()` runs on the monitor thread after the child has ended and been reaped, on every path
///   (stop, shutdown, crash, health failure, kill after the grace), before the supervisor reports
///   the stop as done or starts a new child.
#[derive(Clone)]
pub struct ChildWatch {
    pub interval: Duration,
    pub observe: Arc<dyn Fn(u32) + Send + Sync>,
    pub ended: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for ChildWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildWatch")
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
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
            .field("child_watch", &self.child_watch)
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
            child_watch: None,
        }
    }

    /// The bytes this descriptor holds: name, binary path, every argument, every environment
    /// key and value, and the working directory. Bounded by [`MAX_DESCRIPTOR_BYTES`].
    pub fn descriptor_bytes(&self) -> usize {
        let args: usize = self.args.iter().map(String::len).sum();
        let env: usize = self.env.iter().map(|(k, v)| k.len() + v.len()).sum();
        let workdir = self.workdir.as_ref().map_or(0, |w| w.as_os_str().len());
        self.name
            .len()
            .saturating_add(self.bin.as_os_str().len())
            .saturating_add(args)
            .saturating_add(env)
            .saturating_add(workdir)
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
        // On Windows a console sidecar (ipfs, mem-mcp, the node, …) spawned from a
        // GUI app pops a visible empty console window per child. CREATE_NO_WINDOW
        // suppresses it; stdout/stderr are already piped above, so no output is
        // lost (the log ring still gets every line). cfg-gated: a no-op on Unix.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
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
// SCL-S2 (SC1): lifecycle identity, acknowledged controls, receipts.
// ---------------------------------------------------------------------------

/// The most bytes a service descriptor (name, binary path, argv, environment, working
/// directory) may hold. A larger descriptor is rejected before anything is spawned
/// (SPEC-001 "Ownership, intent and generations": 64 KiB).
pub const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

/// How long a control or a snapshot read may wait for the lifecycle cell before it answers
/// `Busy` instead of blocking on the monitor (SPEC-001: bounded try-lock, never wait on an owner).
const CONTROL_LOCK_BUDGET: Duration = Duration::from_millis(5);

/// After a stop's teardown, how long the monitor waits for the child's log readers to reach
/// end of file before it reports the stop `Incomplete` and quarantines the service.
const READER_DRAIN: Duration = Duration::from_millis(250);

/// The most publication names one `Complete` scope lists.
const MAX_SCOPE_PUBLICATIONS: usize = 16;

/// While quarantined, how often the monitor re-checks whether the held work has ended.
const QUARANTINE_POLL: Duration = Duration::from_millis(50);

include!("supervisor_deadline.rs");

/// Distinguishes supervisor instances within one app run; never reused. The same id names
/// the instance in its tokens and in the deadline overlay.
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// This application run's identity. Ephemeral: it orders controls and receipts within one run
/// and is never a credential or a persistent replay id.
fn app_epoch() -> u64 {
    static EPOCH: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(|| SystemClock.now_unix_ms().max(1))
}

/// What the latest accepted control asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Desired {
    Running,
    Stopped,
}

/// The lifecycle phase (D-12). `Alive` proves process existence only; readiness is separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// No owned resources from the completed control.
    Stopped,
    /// A spawn is admitted and has not been published yet.
    Starting,
    /// A child exists.
    Alive,
    /// The child ended unexpectedly; a retry is scheduled.
    Backoff,
    /// The retry budget is spent; a fresh explicit Start is needed.
    Failed,
    /// A Stop is accepted and its teardown has not settled.
    Stopping,
    /// A Stop settled `Incomplete`: owned work is still running. Starts are refused until it
    /// really ends.
    Quarantined,
}

/// Readiness, independent of the phase (D-12, SCL-S3). `Ready` is claimed only from a fresh
/// success of the current incarnation's probe; a service without a probe is `NotAssessed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Readiness {
    /// The service has no configured probe: liveness only, readiness is not assessed.
    NotAssessed,
    /// A live incarnation that has not answered its probe yet (a cold load).
    Awaiting,
    /// The current incarnation answered its probe, and the answer is fresh.
    Ready,
    /// The last success is older than the freshness limit (two probe intervals).
    Stale,
    /// No live incarnation, or its last sample failed or timed out.
    Unavailable,
}

/// Which readiness and retry-credit rule a service follows (D-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReadinessProfile {
    /// A configured probe attests readiness, and only attested health resets the retry budget
    /// (llama chat and embed, Hermes, the Comms and Cluster connect probes).
    Probed,
    /// No probe (node, node agent, memory, IPFS): readiness is `NotAssessed`, and the retry
    /// budget resets after `healthy_after` of continuous running.
    LivenessOnly,
}

impl ReadinessProfile {
    /// Readiness while no incarnation is live, or after a sample ended the success window.
    fn not_ready(self) -> Readiness {
        match self {
            ReadinessProfile::Probed => Readiness::Unavailable,
            ReadinessProfile::LivenessOnly => Readiness::NotAssessed,
        }
    }

    /// Readiness of a newly published incarnation.
    fn on_spawn(self) -> Readiness {
        match self {
            ReadinessProfile::Probed => Readiness::Awaiting,
            ReadinessProfile::LivenessOnly => Readiness::NotAssessed,
        }
    }
}

/// `Ready` is shown only while its last success is at most `limit` old (INV-13); an older or
/// missing success shows `Stale`. Every other value is shown as stored.
fn shown_readiness(stored: Readiness, last_ok_age: Option<Duration>, limit: Duration) -> Readiness {
    match (stored, last_ok_age) {
        (Readiness::Ready, Some(age)) if age <= limit => Readiness::Ready,
        (Readiness::Ready, _) => Readiness::Stale,
        (other, _) => other,
    }
}

/// The retry counters (SCL-S3.3). `max_retries` is the number of retries after the first failed
/// incarnation, so a budget allows `1 + max_retries` failures before `Failed`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryCounters {
    /// Spawns admitted to the OS call, retries included.
    #[serde(serialize_with = "decimal_u64")]
    pub attempts: u64,
    /// Retries for the same Start intent whose spawn returned a child.
    #[serde(serialize_with = "decimal_u64")]
    pub respawns: u64,
    /// Failures that began a budget: the first failure after a fresh budget or a credit.
    #[serde(serialize_with = "decimal_u64")]
    pub initial_failures: u64,
    /// Failures counted against the current budget.
    #[serde(serialize_with = "decimal_u32")]
    pub consecutive_failures: u32,
    /// Retries allowed after the first failure.
    #[serde(serialize_with = "decimal_u32")]
    pub max_retries: u32,
    /// Times health credit reset the budget (attested for Probed, elapsed for LivenessOnly).
    #[serde(serialize_with = "decimal_u64")]
    pub health_credits: u64,
}

/// Identifies one accepted control: this app run plus the service's intent sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlToken {
    pub app_epoch: u64,
    /// The supervisor instance that issued it (unique within the app run, never reused), so
    /// equal sequences from two services, or from a replacement supervisor, never collide.
    pub instance: u64,
    pub intent_sequence: u64,
}

/// The answer to a control call. It says what was committed, never what has finished:
/// `Accepted` is not `Complete`; read the [`Receipt`] for that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlResult {
    /// A new intent was committed (or the call joined the pending one with the same aim).
    Accepted(ControlToken),
    /// Nothing to do: the service already is what was asked.
    AlreadySatisfied(LifecycleSnapshot),
    /// The supervisor is shutting down; no new start is admitted.
    RejectedClosing,
    /// The lifecycle cell was contended; this call committed nothing.
    Busy,
    /// Owned work from an earlier stop is still running; no start until it ends.
    Quarantined(String),
    /// The intent was committed, but the monitor that acts on it is not running, so its outcome
    /// cannot be observed. Never a success.
    ObservationUnknown(ControlToken, String),
    /// Deadline admission is unavailable: the deadline publisher could not be created or has
    /// ended. Nothing was committed. Never retried by creating another publisher.
    Unavailable(String),
    /// An automated start or restart was refused: the service is `Failed` or not running, and
    /// only a member Start revives it with a fresh retry budget. Nothing was committed.
    RequiresMemberStart(String),
}

/// What became of an accepted control.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Receipt {
    /// Accepted, not settled yet.
    Pending,
    /// A Start: its child was spawned and published.
    Started {
        #[serde(serialize_with = "decimal_u64")]
        incarnation: u64,
    },
    /// A Start: the retry budget ran out before a child stayed up.
    Failed { reason: String },
    /// A Stop: everything named in `scope` was observed finished.
    Complete { scope: Vec<String> },
    /// A Stop: the named resources were still running when the stop settled.
    Incomplete {
        remaining: Vec<String>,
        reason: String,
    },
    /// A later control replaced this one before it settled.
    Superseded,
    /// Not retained (an older token, or one from another app run).
    Expired,
    /// The control's deadline passed before it settled. Final: a later completion updates the
    /// lifecycle snapshot but never turns this into `Complete`.
    DeadlineExpired,
    /// The cell could not be read in bounded time; nothing is claimed.
    Unknown { reason: String },
}

/// A cached copy of a service's lifecycle cell (D-12). Counters cross to JavaScript as decimal
/// strings and must be compared numerically (BigInt), never as strings.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleSnapshot {
    pub name: String,
    pub phase: Phase,
    pub readiness: Readiness,
    pub desired: Desired,
    /// This application run.
    #[serde(serialize_with = "decimal_u64")]
    pub app_epoch: u64,
    /// This supervisor instance (unique within the app run, never reused).
    #[serde(serialize_with = "decimal_u64")]
    pub instance: u64,
    /// Bumped by every distinct accepted Start or Stop.
    #[serde(serialize_with = "decimal_u64")]
    pub intent_sequence: u64,
    /// Committed service configuration. One per supervisor until replacement lands (SC5, SC6).
    #[serde(serialize_with = "decimal_u64")]
    pub generation: u64,
    /// Bumped by every spawn, retries included.
    #[serde(serialize_with = "decimal_u64")]
    pub incarnation: u64,
    /// Bumped by every published change of this cell.
    #[serde(serialize_with = "decimal_u64")]
    pub revision: u64,
    /// Legacy recorded-failure counter (the old `restarts` field), kept during migration.
    #[serde(serialize_with = "decimal_u32")]
    pub restarts: u32,
    pub pid: Option<u32>,
    /// Why the service is quarantined, when it is.
    pub quarantine: Option<String>,
    /// Admitted spawn and publication tickets not yet resolved. A Stop settles only after they
    /// are (or names them as remaining).
    pub open_tickets: u32,
    /// The readiness rule this service follows (D-6).
    pub readiness_profile: ReadinessProfile,
    /// Milliseconds since the current incarnation's last fresh probe success, on the monotonic
    /// clock. `None` when there is none.
    #[serde(serialize_with = "decimal_opt_u64")]
    pub readiness_age_ms: Option<u64>,
    /// Retry counters (SCL-S3.3). `restarts` above stays the legacy recorded-failure counter.
    pub counters: RetryCounters,
}

fn decimal_u64<S: serde::Serializer>(v: &u64, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&v.to_string())
}

fn decimal_u32<S: serde::Serializer>(v: &u32, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&v.to_string())
}

fn decimal_opt_u64<S: serde::Serializer>(
    v: &Option<u64>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    match v {
        Some(n) => s.serialize_str(&n.to_string()),
        None => s.serialize_none(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentKind {
    Start,
    Stop,
}

/// Who asked for a Start: a member (fresh retry budget) or an automated caller (budget carried).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartOrigin {
    Member,
    Automated,
}

const FAILED_NEEDS_MEMBER: &str =
    "the retry budget is spent; only a member Start revives a Failed service";

/// The lifecycle cell: the one latest intent, the coalescing wake bit, the identity counters,
/// the legacy status view and at most one pending and one settled receipt. No queue.
struct Cell {
    status: SupervisorStatus,
    phase: Phase,
    desired: Desired,
    intent_sequence: u64,
    generation: u64,
    incarnation: u64,
    revision: u64,
    wake: bool,
    closing: bool,
    quarantine: Option<String>,
    active: Option<(ControlToken, IntentKind)>,
    settled: Option<(ControlToken, Receipt)>,
    /// This supervisor's id in the deadline overlay, and the publisher it arms.
    owner: u64,
    deadlines: Arc<DeadlinePublisher>,
    /// A spawn admitted and not yet published or torn down (SCL-S2.3).
    spawn_ticket: bool,
    /// Admitted credential publications not yet committed or released:
    /// (ticket id, intent sequence it was admitted under, name).
    publications: Vec<(u64, u64, String)>,
    /// Publications released while a Stop was pending, for that Stop's `Complete` scope
    /// (bounded at [`MAX_SCOPE_PUBLICATIONS`]).
    resolved_publications: Vec<String>,
    next_ticket: u64,
    /// Set once shutdown has finished: no monitor is left to reconcile a quarantine.
    closed: bool,
    /// Log readers still running when the monitor exited. Checked (and joined) by whoever next
    /// touches the cell after shutdown: a ticket release or a snapshot read.
    closed_readers: Vec<Reader>,
    /// This supervisor instance, carried in every token it issues.
    instance: u64,
    /// The readiness rule (fixed by whether the spec has a probe) and the current incarnation's
    /// readiness as the monitor last wrote it. Shown only while the phase is `Alive`.
    profile: ReadinessProfile,
    readiness: Readiness,
    /// The current incarnation's last fresh probe success (monotonic), and how old it may get
    /// before `Ready` is shown `Stale` (two probe intervals).
    last_ok: Option<std::time::Instant>,
    fresh_limit: Duration,
    /// Retry counters. `consecutive_failures` is the budget the monitor counts against.
    counters: RetryCounters,
    /// Bumped by every member Start ([`Supervisor::request_start`]). The monitor gives a fresh
    /// retry budget when it changes, and only then (INV-17).
    member_starts: u64,
}

impl Cell {
    /// Readiness as shown: the profile's not-ready value unless an incarnation is `Alive`, and
    /// `Ready` only while fresh.
    fn shown_readiness(&self) -> Readiness {
        if self.phase != Phase::Alive {
            return self.profile.not_ready();
        }
        shown_readiness(
            self.readiness,
            self.last_ok.map(|t| t.elapsed()),
            self.fresh_limit,
        )
    }

    fn snapshot(&self, epoch: u64) -> LifecycleSnapshot {
        let alive = self.phase == Phase::Alive;
        LifecycleSnapshot {
            name: self.status.name.clone(),
            phase: self.phase,
            readiness: self.shown_readiness(),
            desired: self.desired,
            app_epoch: epoch,
            instance: self.instance,
            intent_sequence: self.intent_sequence,
            generation: self.generation,
            incarnation: self.incarnation,
            revision: self.revision,
            restarts: self.status.restarts,
            pid: self.status.pid,
            quarantine: self.quarantine.clone(),
            open_tickets: u32::try_from(self.publications.len())
                .unwrap_or(u32::MAX)
                .saturating_add(u32::from(self.spawn_ticket)),
            readiness_profile: self.profile,
            readiness_age_ms: self
                .last_ok
                .filter(|_| alive)
                .map(|t| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)),
            counters: self.counters,
        }
    }

    /// After shutdown, reconcile a quarantine without a monitor: join the readers that have
    /// ended since, and once no reader and no ticket is left, the service is Stopped.
    fn reconcile_after_close(&mut self) {
        if !self.closed || self.quarantine.is_none() {
            return;
        }
        if !self.closed_readers.is_empty() {
            let _still = drain_readers(&mut self.closed_readers, Duration::ZERO);
        }
        if self.closed_readers.is_empty() && self.publications.is_empty() && !self.spawn_ticket {
            self.quarantine = None;
            self.phase = Phase::Stopped;
            self.touch();
        }
    }

    /// Publish a legacy state and its phase. A committed Stop owns the phase until it settles
    /// (`Stopping`, then `Stopped` or `Quarantined`); a late state write from the monitor does
    /// not overwrite it.
    fn apply_state(&mut self, state: SupervisorState) {
        let stop_pending = self.desired == Desired::Stopped
            && matches!(self.phase, Phase::Stopping | Phase::Quarantined);
        self.status.state = state.clone();
        self.touch();
        if stop_pending {
            return;
        }
        self.phase = match state {
            SupervisorState::Off => Phase::Stopped,
            SupervisorState::Starting => Phase::Starting,
            SupervisorState::Running => Phase::Alive,
            SupervisorState::Backoff { .. } => Phase::Backoff,
            SupervisorState::Failed => Phase::Failed,
        };
    }

    fn touch(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    /// Commit a new latest intent: supersede the pending receipt, bump the sequence, raise the
    /// wake bit. Returns the new token.
    fn commit(&mut self, epoch: u64, desired: Desired, kind: IntentKind) -> ControlToken {
        if let Some((old, _)) = self.active.take() {
            // DeadlineExpired is final: a control whose deadline already passed stays expired
            // when a newer intent replaces it.
            let expired =
                self.deadlines
                    .expired_at_settle(self.owner, old.app_epoch, old.intent_sequence);
            self.deadlines
                .release(self.owner, old.app_epoch, old.intent_sequence);
            let receipt = if expired {
                Receipt::DeadlineExpired
            } else {
                Receipt::Superseded
            };
            self.settled = Some((old, receipt));
        }
        self.intent_sequence = self.intent_sequence.saturating_add(1);
        self.desired = desired;
        self.resolved_publications.clear();
        let token = ControlToken {
            app_epoch: epoch,
            instance: self.instance,
            intent_sequence: self.intent_sequence,
        };
        self.active = Some((token, kind));
        self.wake = true;
        self.touch();
        token
    }

    /// The answer to a Stop when a Stop is already the latest intent. It joins a pending Stop;
    /// it is `AlreadySatisfied` only when the service is Stopped and owns nothing; while owned
    /// work from that Stop is still running it is `Quarantined`, never "already satisfied".
    fn repeated_stop(&self, epoch: u64) -> ControlResult {
        if let Some((token, IntentKind::Stop)) = self.active {
            return ControlResult::Accepted(token);
        }
        if let Some(reason) = &self.quarantine {
            return ControlResult::Quarantined(reason.clone());
        }
        if self.phase == Phase::Stopped && !self.spawn_ticket && self.publications.is_empty() {
            return ControlResult::AlreadySatisfied(self.snapshot(epoch));
        }
        ControlResult::Busy
    }

    /// Settle the pending receipt if it belongs to intent `seq` and is of `kind`.
    fn settle(&mut self, seq: u64, kind: IntentKind, receipt: Receipt) {
        if let Some((token, k)) = self.active {
            if token.intent_sequence == seq && k == kind {
                // A settlement after the control's deadline is reported as expired, whatever was
                // observed: the caller was promised an answer by then and did not get one.
                let expired = self
                    .deadlines
                    .expired_at_settle(self.owner, token.app_epoch, seq);
                self.deadlines.release(self.owner, token.app_epoch, seq);
                let receipt = if expired {
                    Receipt::DeadlineExpired
                } else {
                    receipt
                };
                self.active = None;
                self.settled = Some((token, receipt));
                self.touch();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Config for the supervisor.
// ---------------------------------------------------------------------------

/// How long to wait after SIGTERM before escalating to SIGKILL on a graceful
/// stop. Bounded so `stop()` always returns promptly. Public so a one-shot child
/// that is not supervised (see [`terminate_unsupervised`]) stops within the same
/// grace on app quit.
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(5);

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
    /// The deadline publisher for deadline-bound controls. `None` uses the app-wide
    /// [`DeadlinePublisher::global`].
    pub deadlines: Option<Arc<DeadlinePublisher>>,
    /// Runs on the monitor after a spawn is admitted and before the OS spawn call. A
    /// fault-injection seam: the lifecycle fixtures hold a spawn "in flight" with it while they
    /// commit a Stop or shutdown. `None` in production.
    pub spawn_barrier: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Runs on the monitor while it marks the service Failed, right after the Failed phase is
    /// visible to controls. A fault-injection seam: the lifecycle fixtures hold the monitor
    /// there while a Start races the transition. `None` in production.
    pub failed_barrier: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Runs on the monitor in a Start's turn after it has read the intent and before it applies
    /// the retry-budget rule. A fault-injection seam: the lifecycle fixtures hold the monitor
    /// there while a member Start joins an automated one. `None` in production.
    pub budget_barrier: Option<Arc<dyn Fn() + Send + Sync>>,
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
            deadlines: None,
            spawn_barrier: None,
            failed_barrier: None,
            budget_barrier: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared state between the controller handle and the monitor thread.
// ---------------------------------------------------------------------------

/// Shared between the controller handle and the monitor. The monitor is the only writer of
/// process state; controls only commit intent into the cell and raise its wake bit.
struct Shared {
    cell: Mutex<Cell>,
    /// Signalled whenever the wake bit is raised. One per service; it carries no payload.
    wake: Condvar,
    /// The injected clock, shared so the backoff wait can anchor `Backoff { next_retry_ms }`
    /// on the same time source as crash records.
    clock: Arc<dyn Clock>,
    /// The bounded ring of the most recent captured stdout/stderr lines. Reader threads push;
    /// the `logs()` accessor snapshots it. Survives restarts.
    logs: LogRing,
    epoch: u64,
    /// See [`SupervisorConfig::failed_barrier`].
    failed_barrier: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Shared {
    /// The cell, for the monitor's short critical sections. Poison-tolerant.
    fn lock(&self) -> MutexGuard<'_, Cell> {
        self.cell.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The cell, or `None` if it stays contended for [`CONTROL_LOCK_BUDGET`]. Controls and
    /// cached reads use this, so they never wait on the monitor.
    fn lock_bounded(&self) -> Option<MutexGuard<'_, Cell>> {
        let deadline = std::time::Instant::now() + CONTROL_LOCK_BUDGET;
        loop {
            match self.cell.try_lock() {
                Ok(g) => return Some(g),
                Err(TryLockError::Poisoned(p)) => return Some(p.into_inner()),
                Err(TryLockError::WouldBlock) => {
                    if std::time::Instant::now() >= deadline {
                        return None;
                    }
                    std::thread::yield_now();
                }
            }
        }
    }

    fn set_state(&self, state: SupervisorState) {
        self.lock().apply_state(state);
    }

    fn snapshot(&self) -> SupervisorStatus {
        self.lock().status.clone()
    }

    /// The current intent: its sequence, what it asks for, whether shutdown has begun, and the
    /// member-Start count (a change means a fresh retry budget).
    fn intent(&self) -> (u64, Desired, bool, u64) {
        let c = self.lock();
        (c.intent_sequence, c.desired, c.closing, c.member_starts)
    }

    /// Whether a newer intent than `seq` was committed, or shutdown began.
    fn intent_changed(&self, seq: u64) -> bool {
        let c = self.lock();
        c.closing || c.intent_sequence != seq
    }

    /// Wait for the wake bit (or `timeout`), then clear it. Spurious wakeups are harmless: the
    /// monitor re-reads the intent after every return.
    fn wait_wake(&self, timeout: Option<Duration>) {
        let mut c = self.lock();
        if !c.wake {
            c = match timeout {
                None => self.wake.wait(c).unwrap_or_else(|e| e.into_inner()),
                Some(t) => {
                    self.wake
                        .wait_timeout(c, t)
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
            };
        }
        c.wake = false;
    }

    fn notify(&self) {
        self.wake.notify_all();
    }
}

// ---------------------------------------------------------------------------
// The Supervisor handle.
// ---------------------------------------------------------------------------

/// The controller handle for a supervised sidecar. Owns the monitor thread's join handle.
/// Dropping it (or calling [`Supervisor::stop`] / [`Supervisor::shutdown`]) tears the child
/// down: no orphans.
pub struct Supervisor {
    shared: Arc<Shared>,
    monitor: Option<JoinHandle<()>>,
    name: String,
    /// Hard cap on how long teardown blocks joining the monitor thread (F-2).
    join_timeout: Duration,
    /// This supervisor's id in the deadline overlay.
    owner: u64,
    deadlines: Arc<DeadlinePublisher>,
    /// This supervisor instance (see [`ControlToken::instance`]).
    instance: u64,
}

impl Supervisor {
    /// Spawn the sidecar and start supervising it. Returns immediately; the monitor thread does
    /// the spawning, waiting and restarting. The initial status is `Starting` and the first
    /// Start intent (sequence 1) is pending. Returns `Err` if the descriptor is larger than
    /// [`MAX_DESCRIPTOR_BYTES`] (`InvalidInput`, nothing spawned) or if the OS refuses the
    /// monitor thread. No `.expect`, so the caller decides (F-2 LOW).
    pub fn start(config: SupervisorConfig) -> std::io::Result<Self> {
        let bytes = config.spec.descriptor_bytes();
        if bytes > MAX_DESCRIPTOR_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "service descriptor for '{}' is {bytes} bytes; the limit is {MAX_DESCRIPTOR_BYTES}",
                    config.spec.name
                ),
            ));
        }
        let name = config.spec.name.clone();
        let join_timeout = config.join_timeout;
        let epoch = app_epoch();
        let instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let owner = instance;
        // The deadline publisher is reserved before any control is admitted. If it could not be
        // created, deadline-bound controls report Unavailable; nothing replaces it.
        let deadlines = config
            .deadlines
            .clone()
            .unwrap_or_else(DeadlinePublisher::global);
        let first = ControlToken {
            app_epoch: epoch,
            instance,
            intent_sequence: 1,
        };
        let profile = if config.spec.health_check.is_some() {
            ReadinessProfile::Probed
        } else {
            ReadinessProfile::LivenessOnly
        };
        // SPEC-001: the initial freshness limit is two configured probe intervals.
        let fresh_limit = config
            .spec
            .health_check
            .as_ref()
            .map_or(Duration::ZERO, |hc| hc.interval.saturating_mul(2));
        let shared = Arc::new(Shared {
            cell: Mutex::new(Cell {
                status: SupervisorStatus {
                    name: name.clone(),
                    state: SupervisorState::Starting,
                    pid: None,
                    restarts: 0,
                    last_crash: None,
                },
                phase: Phase::Starting,
                desired: Desired::Running,
                intent_sequence: 1,
                generation: 1,
                incarnation: 0,
                revision: 1,
                wake: false,
                closing: false,
                quarantine: None,
                active: Some((first, IntentKind::Start)),
                settled: None,
                owner,
                deadlines: deadlines.clone(),
                spawn_ticket: false,
                publications: Vec::new(),
                resolved_publications: Vec::new(),
                next_ticket: 1,
                closed: false,
                closed_readers: Vec::new(),
                instance,
                profile,
                readiness: profile.not_ready(),
                last_ok: None,
                fresh_limit,
                counters: RetryCounters {
                    max_retries: config.backoff.max_retries,
                    ..RetryCounters::default()
                },
                member_starts: 1,
            }),
            wake: Condvar::new(),
            clock: config.clock.clone(),
            logs: LogRing::new(),
            epoch,
            failed_barrier: config.failed_barrier.clone(),
        });
        let monitor_shared = shared.clone();
        let monitor = std::thread::Builder::new()
            .name(format!("supervisor:{name}"))
            .spawn(move || run_monitor(config, monitor_shared))?;
        Ok(Supervisor {
            shared,
            monitor: Some(monitor),
            name,
            join_timeout,
            owner,
            deadlines,
            instance,
        })
    }

    /// This supervisor's id in the deadline overlay (see [`DeadlinePublisher::record_state`]).
    pub fn owner_id(&self) -> u64 {
        self.owner
    }

    /// Commit a Stop that must settle within `within` (SCL-S2.4). The absolute monotonic
    /// deadline is armed here, in the admission path, before the monitor is woken and before any
    /// teardown work. If it passes before the Stop settles, the receipt is `DeadlineExpired`,
    /// even if the publisher never ran, and stays so after a late completion.
    ///
    /// - `Unavailable` if the deadline publisher could not be created or has ended (nothing is
    ///   committed, and no replacement publisher is created).
    /// - `Busy` if the cell is contended or every deadline record is in use (nothing committed).
    /// - A call while a Stop is pending joins it: a plain pending Stop gets this deadline, and a
    ///   pending deadline is moved earlier if this one is tighter, never later (a later call
    ///   never restarts the budget). Otherwise as [`Supervisor::request_stop`].
    pub fn request_stop_within(&self, within: Duration) -> ControlResult {
        if let Some(reason) = self.deadlines.unavailable() {
            return ControlResult::Unavailable(reason);
        }
        let Some(mut c) = self.shared.lock_bounded() else {
            return ControlResult::Busy;
        };
        if c.closing {
            return ControlResult::RejectedClosing;
        }
        if c.desired == Desired::Stopped {
            // Joining a pending Stop (plain or with a deadline) applies this deadline to it if
            // it is the tighter one.
            if let Some((token, IntentKind::Stop)) = c.active {
                if !self.monitor_running() {
                    // Nothing could settle the Stop or free a record: arm none.
                    drop(c);
                    return self.observed(token);
                }
                if !self.deadlines.arm_or_tighten(
                    self.owner,
                    token.app_epoch,
                    token.intent_sequence,
                    within,
                ) {
                    return ControlResult::Busy;
                }
                drop(c);
                return self.observed(token);
            }
            let joined = c.repeated_stop(self.shared.epoch);
            drop(c);
            return match joined {
                ControlResult::Accepted(token) => self.observed(token),
                other => other,
            };
        }
        let seq = c.intent_sequence.saturating_add(1);
        // With the monitor gone nothing would ever settle (and so free) a deadline record: the
        // Stop is committed without one and reported ObservationUnknown.
        let monitor_alive = self.monitor_running();
        if monitor_alive
            && !self
                .deadlines
                .arm(self.owner, self.shared.epoch, seq, within)
        {
            return ControlResult::Busy;
        }
        let token = c.commit(self.shared.epoch, Desired::Stopped, IntentKind::Stop);
        if c.phase != Phase::Quarantined {
            c.phase = Phase::Stopping;
        }
        drop(c);
        self.shared.notify();
        self.observed(token)
    }

    /// Admit a credential publication (for example a generation's bearer or seed file) under the
    /// current intent (SCL-S2.3). Admission is refused unless the service is meant to be running
    /// and not shutting down or quarantined. Write the file only after admission, then
    /// [`PublicationTicket::commit`] it: only a ticket whose intent is still current may become
    /// the active reference. A Stop settles only after every admitted ticket is resolved, or
    /// names it as remaining.
    pub fn admit_publication(
        &self,
        what: &str,
    ) -> std::result::Result<PublicationTicket, PublicationRefused> {
        let Some(mut c) = self.shared.lock_bounded() else {
            return Err(PublicationRefused::Busy);
        };
        if c.closing {
            return Err(PublicationRefused::Closing);
        }
        if let Some(reason) = &c.quarantine {
            return Err(PublicationRefused::Quarantined(reason.clone()));
        }
        if c.desired != Desired::Running {
            return Err(PublicationRefused::NotCurrent);
        }
        let id = c.next_ticket;
        c.next_ticket = c.next_ticket.saturating_add(1);
        let name: String = what.chars().take(64).collect();
        let seq = c.intent_sequence;
        c.publications.push((id, seq, name.clone()));
        c.touch();
        Ok(PublicationTicket {
            shared: self.shared.clone(),
            id,
            seq: c.intent_sequence,
            name,
            open: true,
        })
    }

    /// A cloneable, read-only status snapshot (the legacy view). Safe to poll from the Tauri
    /// runtime while the monitor thread runs (takes the lock briefly, clones).
    pub fn status(&self) -> SupervisorStatus {
        self.shared.snapshot()
    }

    /// The lifecycle cell (D-12): phase, readiness, intent and identity counters. A bounded
    /// read: `None` means the cell was contended and nothing is claimed.
    ///
    /// Not purely an observation after shutdown: with no monitor left, this read reconciles a
    /// quarantine. It joins the log readers that have ended since shutdown, and once no reader
    /// and no ticket is left it moves the phase from `Quarantined` to `Stopped` (bumping the
    /// revision) before returning the snapshot. Before shutdown it changes nothing.
    pub fn lifecycle(&self) -> Option<LifecycleSnapshot> {
        self.shared.lock_bounded().map(|mut c| {
            // After shutdown, a read is the only thing left that can observe late completion.
            c.reconcile_after_close();
            c.snapshot(self.shared.epoch)
        })
    }

    /// A snapshot of the recent captured stdout/stderr lines (oldest→newest, bounded at
    /// [`LOG_RING_CAPACITY`]). Safe to poll from the Tauri runtime; the `node_logs` command
    /// folds this into the Node LOG panel so a packaged build shows REAL node output.
    pub fn logs(&self) -> Vec<LogLine> {
        self.shared.logs.snapshot()
    }

    /// The sidecar name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the monitor thread is still running. It ends only on shutdown, or if a caller's
    /// hook panicked on it.
    pub fn monitor_running(&self) -> bool {
        self.monitor.as_ref().is_some_and(|h| !h.is_finished())
    }

    /// Commit a Stop intent (SC1). Non-blocking and bounded: it never waits for the teardown.
    ///
    /// - A Stop on a stopped service is `AlreadySatisfied`; a Stop while one is pending joins
    ///   it (`Accepted` with the same token). A repeated Stop never respawns and never starts a
    ///   second teardown.
    /// - Otherwise the Stop wins in every phase (Starting, Alive, Backoff, Failed): the monitor
    ///   tears down what it owns and admits no further spawn for the earlier intent.
    /// - `Accepted` is not `Complete`: read [`Supervisor::receipt`].
    pub fn request_stop(&self) -> ControlResult {
        let Some(mut c) = self.shared.lock_bounded() else {
            return ControlResult::Busy;
        };
        if c.closing {
            return ControlResult::RejectedClosing;
        }
        if c.desired == Desired::Stopped {
            let joined = c.repeated_stop(self.shared.epoch);
            drop(c);
            // Joining a pending Stop is only Accepted while a monitor can settle it.
            return match joined {
                ControlResult::Accepted(token) => self.observed(token),
                other => other,
            };
        }
        let token = c.commit(self.shared.epoch, Desired::Stopped, IntentKind::Stop);
        if c.phase != Phase::Quarantined {
            c.phase = Phase::Stopping;
        }
        drop(c);
        self.shared.notify();
        self.observed(token)
    }

    /// Commit a member Start intent (SC1, SC2): a person asked for the service (a Start button,
    /// a setting the member turned on). Non-blocking and bounded.
    ///
    /// - Refused as `RejectedClosing` after shutdown began, and as `Quarantined` while owned
    ///   work from an earlier stop is still running.
    /// - A Start on a live service is `AlreadySatisfied`; one while a Start is pending joins it.
    /// - After a Stop (or after `Failed`) it commits a new intent: the monitor finishes any
    ///   teardown first, then spawns exactly one new incarnation with a fresh retry budget. This
    ///   is the only control that gives a fresh budget; automated callers use
    ///   [`Supervisor::request_auto_start`] and [`Supervisor::request_auto_restart`].
    pub fn request_start(&self) -> ControlResult {
        self.start_intent(StartOrigin::Member)
    }

    /// Commit an automated Start (SC2): no member asked (auto-start at launch, a lazy start from
    /// another service or a tool). Like a member Start, except that it carries the retry budget
    /// over (failures before an earlier Stop still count) and never revives a `Failed` service:
    /// that is `RequiresMemberStart`. If the carried budget is already spent, the Start settles
    /// `Failed` without spawning.
    pub fn request_auto_start(&self) -> ControlResult {
        self.start_intent(StartOrigin::Automated)
    }

    /// Commit an automated restart (SC2): an internal stop then start of a running service, as
    /// one new Start intent (for example the node's mining arm and disarm, or a watchdog). The
    /// current incarnation is torn down (not counted as a failure) and a new one is spawned
    /// under the same retry budget. Only for a service that is meant to be running and is
    /// `Starting`, `Alive` or in `Backoff`; otherwise `RequiresMemberStart`. A restart while a
    /// Start is still pending joins it.
    pub fn request_auto_restart(&self) -> ControlResult {
        let Some(mut c) = self.shared.lock_bounded() else {
            return ControlResult::Busy;
        };
        if c.closing {
            return ControlResult::RejectedClosing;
        }
        if let Some(reason) = &c.quarantine {
            return ControlResult::Quarantined(reason.clone());
        }
        if c.desired != Desired::Running {
            return ControlResult::RequiresMemberStart(
                "the service is not meant to be running; an automated restart does not start it"
                    .to_string(),
            );
        }
        if c.phase == Phase::Failed {
            return ControlResult::RequiresMemberStart(FAILED_NEEDS_MEMBER.to_string());
        }
        if let Some((token, IntentKind::Start)) = c.active {
            drop(c);
            return self.observed(token);
        }
        let token = c.commit(self.shared.epoch, Desired::Running, IntentKind::Start);
        if c.phase == Phase::Alive {
            c.phase = Phase::Stopping;
        }
        drop(c);
        self.shared.notify();
        self.observed(token)
    }

    /// Whether the current incarnation is `Ready`: it answered its probe, and the answer is
    /// fresh. Always false for a service without a probe (it is `NotAssessed`) and false when
    /// the cell could not be read in bounded time. Process existence is [`Supervisor::status`].
    pub fn is_ready(&self) -> bool {
        self.lifecycle()
            .is_some_and(|l| l.readiness == Readiness::Ready)
    }

    fn start_intent(&self, origin: StartOrigin) -> ControlResult {
        let Some(mut c) = self.shared.lock_bounded() else {
            return ControlResult::Busy;
        };
        if c.closing {
            return ControlResult::RejectedClosing;
        }
        if let Some(reason) = &c.quarantine {
            return ControlResult::Quarantined(reason.clone());
        }
        // A pending Start is joined in every phase (the phase can still read Failed until the
        // monitor picks the new intent up), so identical concurrent Starts never supersede each
        // other.
        if let Some((token, IntentKind::Start)) = c.active {
            drop(c);
            // Joining a pending Start is only Accepted while a monitor can settle it.
            return self.observed(token);
        }
        if c.desired == Desired::Running && c.phase != Phase::Failed {
            return ControlResult::AlreadySatisfied(c.snapshot(self.shared.epoch));
        }
        if origin == StartOrigin::Automated && c.phase == Phase::Failed {
            return ControlResult::RequiresMemberStart(FAILED_NEEDS_MEMBER.to_string());
        }
        if origin == StartOrigin::Member {
            c.member_starts = c.member_starts.saturating_add(1);
        }
        let token = c.commit(self.shared.epoch, Desired::Running, IntentKind::Start);
        drop(c);
        self.shared.notify();
        self.observed(token)
    }

    /// `Accepted(token)` if the monitor is there to act on the committed intent, otherwise
    /// `ObservationUnknown`: the intent stays committed and visible, nothing is claimed.
    fn observed(&self, token: ControlToken) -> ControlResult {
        if self.monitor_running() {
            ControlResult::Accepted(token)
        } else {
            ControlResult::ObservationUnknown(
                token,
                "the service monitor is not running; the intent is recorded but its outcome cannot be observed"
                    .to_string(),
            )
        }
    }

    /// What became of the control `token`. A bounded read that never waits on the monitor.
    /// Only the pending and the most recently settled receipt are kept; older tokens and tokens
    /// from another app run are `Expired`.
    pub fn receipt(&self, token: ControlToken) -> Receipt {
        // A token from another app run or another supervisor instance is never answered here,
        // even when its sequence matches one of ours.
        if token.app_epoch != self.shared.epoch || token.instance != self.instance {
            return Receipt::Expired;
        }
        // Compare the monotonic deadline directly, without any lock, so expiry is reported even
        // when the publisher has not run and the cell is busy.
        match self
            .deadlines
            .expired(self.owner, token.app_epoch, token.intent_sequence)
        {
            Some(true) => return Receipt::DeadlineExpired,
            Some(false) => {}
            None => {
                return Receipt::Unknown {
                    reason: "the deadline record was being rewritten".to_string(),
                }
            }
        }
        let Some(c) = self.shared.lock_bounded() else {
            return Receipt::Unknown {
                reason: "the lifecycle cell is busy".to_string(),
            };
        };
        match (&c.active, &c.settled) {
            (Some((t, _)), _) if *t == token => Receipt::Pending,
            (_, Some((t, r))) if *t == token => r.clone(),
            _ => Receipt::Expired,
        }
    }

    /// Gracefully stop the child (SIGTERM → grace → SIGKILL) and leave the supervisor `Off`
    /// with NO restart. An intentional stop is NOT a crash: no crash record, no respawn.
    ///
    /// Legacy form of [`Supervisor::request_stop`]: it commits the Stop, then blocks until the
    /// receipt settles (or a 10 s bound). Calling it again on a stopped supervisor is a no-op;
    /// it never respawns the child (bug (a), SCL-S2.2). New callers should use
    /// `request_stop` and read the receipt, which distinguishes `Complete` from `Incomplete`.
    pub fn stop(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let token = loop {
            match self.request_stop() {
                ControlResult::Accepted(t) => break t,
                ControlResult::Busy if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                // Already stopped, shutting down, monitor gone or still busy at the bound:
                // nothing this call can wait for.
                _ => return,
            }
        };
        while std::time::Instant::now() < deadline {
            match self.receipt(token) {
                Receipt::Pending | Receipt::Unknown { .. } => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return,
            }
        }
    }

    /// Tear the supervisor down entirely: stop the child and join the monitor thread. Called by
    /// `Drop`; exposed so callers can join deterministically. After this every Start is
    /// `RejectedClosing`.
    ///
    /// The join is BOUNDED by `join_timeout` (F-2): a `JoinHandle` has no timed join in std, so
    /// we poll `is_finished()` up to the bound and only `join()` once it has finished. If the
    /// monitor has not finished within the bound we DETACH rather than block teardown forever.
    /// The detached monitor holds no supervisor lock and owns no live child at that point (it
    /// terminates the child before exiting), so this is safe.
    pub fn shutdown(&mut self) {
        {
            let mut c = self.shared.lock();
            if !c.closing {
                c.closing = true;
                c.wake = true;
                c.touch();
            }
        }
        self.shared.notify();
        if let Some(handle) = self.monitor.take() {
            let deadline = std::time::Instant::now() + self.join_timeout;
            while !handle.is_finished() {
                if std::time::Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = handle.join();
        }
    }

    /// Poll the status until `pred` holds or `timeout` elapses. Test/helper convenience; uses a
    /// short sleep between polls. Returns the final snapshot.
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

/// Why a publication was not admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationRefused {
    /// The service is not meant to be running (a Stop is committed), or a newer intent replaced
    /// the ticket's own: the file must not become the active reference.
    NotCurrent,
    /// Shutdown has begun.
    Closing,
    /// Owned work from an earlier stop is still running.
    Quarantined(String),
    /// The lifecycle cell was contended; nothing was admitted.
    Busy,
}

/// An admitted credential publication (SCL-S2.3). While it is open the service's Stop cannot
/// settle `Complete`. Dropping it releases it: say so only once nothing it wrote remains.
pub struct PublicationTicket {
    shared: Arc<Shared>,
    id: u64,
    seq: u64,
    name: String,
    open: bool,
}

impl std::fmt::Debug for PublicationTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicationTicket")
            .field("id", &self.id)
            .field("intent_sequence", &self.seq)
            .field("name", &self.name)
            .finish()
    }
}

impl PublicationTicket {
    /// Make the published file the active reference, if the ticket's intent is still current.
    /// Otherwise the ticket stays open as a [`StaleTicket`]: remove what it wrote, then call
    /// [`StaleTicket::cleaned`]. Dropping the `StaleTicket` instead keeps the publication open.
    pub fn commit(mut self) -> std::result::Result<(), StaleTicket> {
        let current = {
            let c = self.shared.lock();
            !c.closing && c.desired == Desired::Running && c.intent_sequence == self.seq
        };
        if current {
            self.release();
            Ok(())
        } else {
            self.open = false;
            Err(StaleTicket {
                shared: self.shared.clone(),
                id: self.id,
                name: std::mem::take(&mut self.name),
            })
        }
    }

    fn release(&mut self) {
        if self.open {
            self.open = false;
            release_publication(&self.shared, self.id);
        }
    }
}

impl Drop for PublicationTicket {
    fn drop(&mut self) {
        self.release();
    }
}

/// A publication whose intent was superseded before it committed. It still holds the service's
/// Stop open until the caller has removed what it wrote and said so with [`StaleTicket::cleaned`].
/// Dropping it without that (an ignored error, or an unwind) releases nothing: the file may
/// still be on disk, so the Stop stays `Incomplete` naming the publication and the service stays
/// Quarantined.
#[must_use = "remove what the ticket wrote, then call cleaned(); dropping it keeps the Stop open"]
pub struct StaleTicket {
    shared: Arc<Shared>,
    id: u64,
    name: String,
}

impl std::fmt::Debug for StaleTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaleTicket")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

impl StaleTicket {
    /// What the ticket was admitted for.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The stale file is gone: release the ticket.
    pub fn cleaned(self) {
        release_publication(&self.shared, self.id);
    }
}

fn release_publication(shared: &Shared, id: u64) {
    {
        let mut c = shared.lock();
        let stop_pending = matches!(c.active, Some((_, IntentKind::Stop)));
        if let Some(pos) = c.publications.iter().position(|(t, _, _)| *t == id) {
            let (_, _, name) = c.publications.remove(pos);
            if stop_pending && c.resolved_publications.len() < MAX_SCOPE_PUBLICATIONS {
                c.resolved_publications.push(format!("publication:{name}"));
            }
        }
        // After shutdown no monitor reconciles a quarantine: the release does, re-checking the
        // readers itself.
        c.reconcile_after_close();
        c.wake = true;
        c.touch();
    }
    shared.notify();
}

impl Drop for Supervisor {
    /// No-orphan guarantee: dropping the supervisor tells the monitor to stop the child and
    /// joins it, so the child is dead before the process exits.
    fn drop(&mut self) {
        self.shutdown();
        // Nothing can settle this supervisor's controls any more: free its deadline records.
        self.deadlines.release_owner(self.owner);
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

/// One log reader thread, named for the receipt that reports it.
struct Reader {
    name: &'static str,
    handle: JoinHandle<()>,
}

/// Take the child's piped stdout + stderr and spawn one reader thread per stream that reads
/// lines and pushes each into the shared log ring. The monitor keeps the handles: a stop is
/// `Complete` only once both readers have reached end of file (SC1). A reader never blocks
/// teardown itself; it ends with EOF when the last holder of the pipe closes it, which for an
/// ordinary child is the child's own exit. A descendant that inherited the pipe can keep it
/// open, and then the stop reports the reader as remaining. They hold only an `Arc<Shared>`
/// (no supervisor lock is held across a blocking read). Best-effort: if the OS refuses a reader
/// thread the stream is simply not captured.
fn spawn_log_readers(child: &mut Child, name: &str, shared: &Arc<Shared>) -> Vec<Reader> {
    use std::io::{BufRead, BufReader};
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let shared = shared.clone();
        if let Ok(handle) = std::thread::Builder::new()
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
            })
        {
            readers.push(Reader {
                name: "reader:stdout",
                handle,
            });
        }
    }
    if let Some(err) = child.stderr.take() {
        let shared = shared.clone();
        if let Ok(handle) = std::thread::Builder::new()
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
            })
        {
            readers.push(Reader {
                name: "reader:stderr",
                handle,
            });
        }
    }
    readers
}

/// Join every finished reader and return the names of those still running. Waits up to
/// `window` for them to finish first.
fn drain_readers(readers: &mut Vec<Reader>, window: Duration) -> Vec<String> {
    let deadline = std::time::Instant::now() + window;
    while readers.iter().any(|r| !r.handle.is_finished()) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut remaining = Vec::new();
    let mut kept = Vec::new();
    for r in readers.drain(..) {
        if r.handle.is_finished() {
            let _ = r.handle.join();
        } else {
            remaining.push(r.name.to_string());
            kept.push(r);
        }
    }
    *readers = kept;
    remaining
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
/// path (the fork-bomb-bound negative control observes restart progress within
/// its bounded observation window).
fn drain_grace(shared: &Arc<Shared>) {
    let deadline = std::time::Instant::now() + Duration::from_millis(20);
    loop {
        if shared.logs.snapshot().iter().any(|l| l.stream == "err")
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
fn signal_pid(pid: u32, signal: i32) -> bool {
    let Some(target) = checked_pid(pid) else {
        return false;
    };
    // SAFETY: `kill(2)` with a positive pid and a signal number is a well-defined libc call
    // with no memory effects. A stale pid returns ESRCH, which the caller handles by
    // re-checking liveness.
    unsafe { libc::kill(target, signal) == 0 }
}

/// `pid` as a positive `pid_t`, or `None`. Zero would address the caller's own process group
/// and a value above `i32::MAX` would wrap to a negative pid (a group, or every process for
/// -1), so neither is ever passed to `kill(2)`.
#[cfg(unix)]
fn checked_pid(pid: u32) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok().filter(|p| *p > 0)
}

/// Whether `pid` is still alive on Unix (signal 0 = existence probe, sends no
/// signal). Used to confirm SIGTERM took effect before the grace timeout.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // SAFETY: `kill(p, 0)` on a positive pid performs the existence check without delivering a
    // signal.
    checked_pid(pid).is_some_and(|p| unsafe { libc::kill(p, 0) == 0 })
}

/// Gracefully terminate a child: SIGTERM, wait up to `grace` for exit, then
/// SIGKILL. On Windows (out of beta scope) there is no SIGTERM: fall back to a
/// hard kill. Reaps the child so no zombie remains. Returns once the child is
/// dead.
fn terminate_child(child: &mut Child, grace: Duration, watch: Option<&ChildWatch>) {
    #[cfg(unix)]
    {
        let pid = child.id();
        // Not reaped yet, so `pid` still names this child (see `ChildWatch`).
        if let Some(w) = watch {
            (w.observe)(pid);
        }
        let _ = signal_pid(pid, libc::SIGTERM);
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
        // Escalate: SIGKILL (cannot be caught) and reap. The child is still unreaped here (a reap
        // above returns early), so observe once more: its own children are still its children.
        if let Some(w) = watch {
            (w.observe)(pid);
        }
        let _ = signal_pid(pid, libc::SIGKILL);
        let _ = child.wait();
    }
    #[cfg(not(unix))]
    {
        // Windows fallback (O-4: out of beta scope): no SIGTERM. Hard-kill.
        let _ = grace;
        if let Some(w) = watch {
            (w.observe)(child.id());
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Gracefully terminate a one-shot child that is not under a [`Supervisor`]
/// (for example the knowledge importer): SIGTERM, wait up to `grace`, SIGKILL,
/// reap. A child that has already exited and been reaped is left alone (its pid
/// may have been reused), so this is safe to call more than once.
pub fn terminate_unsupervised(child: &mut Child, grace: Duration) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    terminate_child(child, grace, None);
}

/// The monitor's hold on the current child. Every normal path reaps the child before this goes
/// out of scope, so the drop finds it reaped and does nothing. If the monitor unwinds instead
/// (a `ChildWatch` hook panicked), the drop kills and reaps the child, so nothing outlives the
/// supervisor (the no-orphan guarantee) and no unreaped child is left behind.
struct OwnedChild(Child);

impl std::ops::Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // `Child` remembers a reaped status, so this never signals a pid that was reused.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// The monitor loop: the sole owner of the child and the sole writer of process state.
///
/// Every turn reads the latest intent from the cell (SC1):
/// - **Stop**: the child was already torn down where it was owned; the Stop is settled
///   (`Complete` once the log readers have ended, else `Incomplete` and Quarantined) and the
///   monitor idles until the next wake. It never spawns from here, whatever the phase, so a
///   repeated Stop cannot respawn (bug (a)).
/// - **Start**: spawn, supervise, and on an unexpected exit record a crash and retry under the
///   backoff bound. Each spawn is admitted against the intent it serves: a newer intent seen
///   before the spawn admits nothing, and one seen right after the OS call returns sends the new
///   child straight to teardown, unpublished.
/// - **Shutdown**: tear down, settle what is pending, exit.
fn run_monitor(config: SupervisorConfig, shared: Arc<Shared>) {
    let SupervisorConfig {
        spec,
        backoff,
        crash_record_path,
        stop_grace,
        healthy_after,
        join_timeout: _join_timeout,
        clock,
        deadlines: _deadlines,
        spawn_barrier,
        failed_barrier: _failed_barrier,
        budget_barrier,
    } = config;

    // The CONSECUTIVE-failure counter lives in the cell (`counters.consecutive_failures`) and
    // drives the backoff delay and the Failed cap. It resets on health credit (attested for a
    // probed service, elapsed for a liveness-only one: F-1, SC2) and on a member Start, never
    // on an automated one.
    let watch = spec.child_watch.clone();
    // After every end of a child (reaped): the caller's cleanup of what the child started.
    let ended = |w: &Option<ChildWatch>| {
        if let Some(w) = w {
            (w.ended)();
        }
    };
    // Log readers of every incarnation that have not been joined yet.
    let mut readers: Vec<Reader> = Vec::new();
    // The member-Start count the current retry budget belongs to.
    let mut budget_member: u64 = 1;
    // The service's one probe slot: a worker that outlives its timeout or its incarnation keeps
    // it until it really ends (one probe at a time per service; bounding it is SC3).
    let mut probe_slot: Option<ProbeRunner> = None;
    // The Start intent whose budget ran out; the monitor idles in Failed until a newer intent.
    let mut failed_seq: Option<u64> = None;
    // The Stop intent already settled; the monitor idles, re-checking a quarantine.
    let mut settled_stop: Option<u64> = None;
    // What was torn down since the last settled Stop: a published child ("child") and a spawn
    // admitted before the Stop that returned after it ("spawnTicket").
    let mut tore_down_child = false;
    let mut late_spawn = false;
    // The intent the last admitted spawn served. A different current intent is a new Start.
    let mut spawned_for: u64 = 0;

    loop {
        let (seq, desired, closing, member_starts) = shared.intent();
        if closing {
            finish_closing(&shared, &mut readers, tore_down_child);
            return;
        }

        if desired == Desired::Stopped {
            if settled_stop != Some(seq) {
                let torn = TornDown {
                    child: tore_down_child,
                    spawn_ticket: late_spawn,
                    watch: watch.is_some(),
                };
                if settle_stop(&shared, seq, &mut readers, torn) {
                    settled_stop = Some(seq);
                    tore_down_child = false;
                    late_spawn = false;
                }
                continue;
            }
            let (quarantined, publications_open) = {
                let c = shared.lock();
                (c.quarantine.is_some(), !c.publications.is_empty())
            };
            if quarantined
                && drain_readers(&mut readers, Duration::ZERO).is_empty()
                && !publications_open
            {
                // The held work really ended: ownership is reconciled. The receipt keeps what
                // was observed when the Stop settled; a fresh explicit Start is now admitted.
                let mut c = shared.lock();
                c.quarantine = None;
                c.phase = Phase::Stopped;
                c.touch();
                continue;
            }
            shared.wait_wake(quarantined.then_some(QUARANTINE_POLL));
            continue;
        }

        // desired == Running
        if failed_seq == Some(seq) {
            shared.wait_wake(None);
            continue;
        }
        // The first spawn for a new Start intent. However the previous incarnation ended (a
        // Stop's teardown that this Start superseded, a crash, or health failures that spent the
        // retry budget), its child is gone but its readers (and any process holding its output
        // pipe) may not be. Reconcile them before admitting the spawn; until then the service is
        // Quarantined and the Start pending.
        if spawned_for != seq {
            let mut remaining = drain_readers(&mut readers, READER_DRAIN);
            // Publications admitted under an earlier intent are the old incarnation's too.
            remaining.extend(
                shared
                    .lock()
                    .publications
                    .iter()
                    .filter(|(_, admitted, _)| *admitted < seq)
                    .map(|(_, _, n)| format!("publication:{n}")),
            );
            if !remaining.is_empty() {
                hold_start_for_cleanup(&shared, seq, &remaining);
                shared.wait_wake(Some(QUARANTINE_POLL));
                continue;
            }
            release_start_hold(&shared, seq);
            tore_down_child = false;
            late_spawn = false;
        }
        if let Some(barrier) = &budget_barrier {
            barrier();
        }
        // A member Start gives a fresh retry budget; an automated one carries it over.
        if budget_member != member_starts {
            budget_member = member_starts;
            reset_budget(&shared);
        }
        // An automated intent cannot buy more retries: with the budget spent, it settles Failed
        // without a spawn (INV-17).
        if consecutive_failures(&shared) > backoff.max_retries {
            enter_failed(
                &shared,
                seq,
                "the retry budget is spent; only a member Start gives a fresh one",
            );
            failed_seq = Some(seq);
            continue;
        }

        // --- spawn admission: only for the intent that is still current ---
        if !admit_spawn(&shared, seq) {
            continue;
        }
        let retry = spawned_for == seq;
        spawned_for = seq;
        if let Some(barrier) = &spawn_barrier {
            barrier();
        }
        count_attempt(&shared);
        let mut child = match spawn_child(&spec) {
            Ok(c) => OwnedChild(c),
            Err(e) => {
                end_spawn_ticket(&shared);
                // Spawn itself failed (bad path / EACCES). Treat as a crash so the same bounded
                // backoff applies — a bad binary path cannot fork-bomb either.
                let record = CrashRecord {
                    name: spec.name.clone(),
                    at_unix_ms: clock.now_unix_ms(),
                    exit: format!("spawn failed: {e}"),
                    stderr_tail: String::new(),
                    restart_attempt: consecutive_failures(&shared),
                };
                append_crash_record(&crash_record_path, &record);
                record_crash_status(&shared, &record);
                let failures = count_failure(&shared);
                if failures > backoff.max_retries {
                    enter_failed(&shared, seq, "the binary could not be started");
                    failed_seq = Some(seq);
                    continue;
                }
                backoff_wait(&shared, seq, backoff.delay_for(failures), clock.as_ref());
                continue;
            }
        };
        if retry {
            count_respawn(&shared);
        }

        // Stream stdout + stderr into the shared log ring (Q-A.2). Done BEFORE publishing so no
        // early line is missed. The crash tail is later derived from the ring's `err` lines.
        // Join readers of earlier incarnations that have ended, so a long crash-and-retry
        // history does not keep their handles around.
        let _still_reading = drain_readers(&mut readers, Duration::ZERO);
        readers.extend(spawn_log_readers(&mut child, &spec.name, &shared));

        // --- publication admission: a newer intent that arrived during the OS call wins ---
        let Some(incarnation) = publish_alive(&shared, seq, child.id()) else {
            // The spawn ticket was admitted before the newer intent: its child is still owned
            // and goes straight to teardown. The ticket closes only once the child is reaped.
            terminate_child(&mut child, stop_grace, watch.as_ref());
            ended(&watch);
            after_teardown(&shared);
            end_spawn_ticket(&shared);
            tore_down_child = true;
            late_spawn = true;
            continue;
        };

        // --- supervise: wait for exit OR a newer intent OR a health failure. Health credit is
        // applied inside, as soon as it is earned (SC2). ---
        let outcome = supervise_running(
            &mut child,
            &shared,
            Running {
                seq,
                incarnation,
                health: spec.health_check.as_ref(),
                watch: watch.as_ref(),
                healthy_after,
            },
            &mut probe_slot,
        );

        match outcome {
            RunOutcome::IntentChanged => {
                // A Stop (or shutdown): not a crash. No record, no restart.
                terminate_child(&mut child, stop_grace, watch.as_ref());
                ended(&watch);
                after_teardown(&shared);
                tore_down_child = true;
            }
            RunOutcome::Crashed { status } => {
                // Unexpected exit → crash record + backoff + restart. Reap first, then give the
                // stderr reader a brief window to drain the now-closed pipe so the ring holds the
                // final lines, then derive the bounded tail from the ring's `err` lines.
                let _ = child.wait(); // reap (already exited)
                ended(&watch);
                drain_grace(&shared);
                let stderr_tail = stderr_tail_from_ring(&shared);
                let record = CrashRecord {
                    name: spec.name.clone(),
                    at_unix_ms: clock.now_unix_ms(),
                    exit: render_exit(&status),
                    stderr_tail,
                    restart_attempt: consecutive_failures(&shared),
                };
                append_crash_record(&crash_record_path, &record);
                record_crash_status(&shared, &record);
                clear_pid(&shared);

                let failures = count_failure(&shared);
                if failures > backoff.max_retries {
                    // The fork-bomb bound: give up. Failed, no respawn for this intent.
                    enter_failed(
                        &shared,
                        seq,
                        "the child kept exiting; the retry budget is spent",
                    );
                    failed_seq = Some(seq);
                    continue;
                }
                backoff_wait(&shared, seq, backoff.delay_for(failures), clock.as_ref());
            }
            RunOutcome::Unhealthy => {
                // Health check failed: stop the (still-alive) child and treat it as a crash so
                // the same bounded backoff applies.
                terminate_child(&mut child, stop_grace, watch.as_ref());
                ended(&watch);
                let record = CrashRecord {
                    name: spec.name.clone(),
                    at_unix_ms: clock.now_unix_ms(),
                    exit: "health check failed".to_string(),
                    stderr_tail: String::new(),
                    restart_attempt: consecutive_failures(&shared),
                };
                append_crash_record(&crash_record_path, &record);
                record_crash_status(&shared, &record);
                clear_pid(&shared);

                let failures = count_failure(&shared);
                if failures > backoff.max_retries {
                    enter_failed(
                        &shared,
                        seq,
                        "the health check kept failing; the retry budget is spent",
                    );
                    failed_seq = Some(seq);
                    continue;
                }
                backoff_wait(&shared, seq, backoff.delay_for(failures), clock.as_ref());
            }
        }
    }
}

/// The failures counted against the current retry budget.
fn consecutive_failures(shared: &Shared) -> u32 {
    shared.lock().counters.consecutive_failures
}

/// Count one failure against the budget and return the new count. The first failure of a
/// budget is also an initial failure (SCL-S3.3).
fn count_failure(shared: &Shared) -> u32 {
    let mut c = shared.lock();
    if c.counters.consecutive_failures == 0 {
        c.counters.initial_failures = c.counters.initial_failures.saturating_add(1);
    }
    c.counters.consecutive_failures = c.counters.consecutive_failures.saturating_add(1);
    c.touch();
    c.counters.consecutive_failures
}

/// A member Start: a fresh retry budget.
fn reset_budget(shared: &Shared) {
    let mut c = shared.lock();
    c.counters.consecutive_failures = 0;
    c.touch();
}

/// Health credit (once per window): the budget resets.
fn grant_credit(shared: &Shared) {
    let mut c = shared.lock();
    c.counters.consecutive_failures = 0;
    c.counters.health_credits = c.counters.health_credits.saturating_add(1);
    c.touch();
}

fn count_attempt(shared: &Shared) {
    let mut c = shared.lock();
    c.counters.attempts = c.counters.attempts.saturating_add(1);
    c.touch();
}

fn count_respawn(shared: &Shared) {
    let mut c = shared.lock();
    c.counters.respawns = c.counters.respawns.saturating_add(1);
    c.touch();
}

/// The previous incarnation still holds `remaining`: quarantine the service while the accepted
/// Start for intent `seq` waits. Nothing is spawned meanwhile.
fn hold_start_for_cleanup(shared: &Shared, seq: u64, remaining: &[String]) {
    let mut c = shared.lock();
    if c.closing || c.intent_sequence != seq || c.quarantine.is_some() {
        return;
    }
    c.status.state = SupervisorState::Off;
    c.status.pid = None;
    c.phase = Phase::Quarantined;
    c.quarantine = Some(format!(
        "the previous incarnation is not cleaned up yet ({}): its output is still open, so \
         another process (for example one it started) may hold the pipe; the accepted Start \
         waits until it ends",
        remaining.join(", ")
    ));
    c.touch();
}

/// The previous incarnation is reconciled: lift the hold placed for intent `seq`.
fn release_start_hold(shared: &Shared, seq: u64) {
    let mut c = shared.lock();
    if c.intent_sequence == seq && c.desired == Desired::Running && c.quarantine.is_some() {
        c.quarantine = None;
        c.phase = Phase::Starting;
        c.touch();
    }
}

/// Spawn admission (SC1 ticket check): admit a spawn only while intent `seq` is still the
/// latest and shutdown has not begun, and publish `Starting` in the same critical section.
fn admit_spawn(shared: &Shared, seq: u64) -> bool {
    let mut c = shared.lock();
    if c.closing || c.intent_sequence != seq {
        return false;
    }
    c.status.state = SupervisorState::Starting;
    c.phase = Phase::Starting;
    c.spawn_ticket = true;
    c.touch();
    true
}

/// The admitted spawn is resolved: published, failed, or its late child reaped.
fn end_spawn_ticket(shared: &Shared) {
    let mut c = shared.lock();
    c.spawn_ticket = false;
    c.touch();
}

/// Count the new incarnation, then publish it `Alive` only if intent `seq` is still current,
/// with the profile's spawn readiness (`Awaiting` for a probed service). Settles a pending Start
/// receipt as `Started` and returns the incarnation. Returns `None` when a newer intent (or
/// shutdown) arrived while the OS call ran: the caller tears the child down without publishing
/// it.
fn publish_alive(shared: &Shared, seq: u64, pid: u32) -> Option<u64> {
    let mut c = shared.lock();
    c.incarnation = c.incarnation.saturating_add(1);
    c.touch();
    if c.closing || c.intent_sequence != seq {
        return None;
    }
    c.status.state = SupervisorState::Running;
    c.status.pid = Some(pid);
    c.phase = Phase::Alive;
    c.readiness = c.profile.on_spawn();
    c.last_ok = None;
    c.spawn_ticket = false;
    let incarnation = c.incarnation;
    c.settle(seq, IntentKind::Start, Receipt::Started { incarnation });
    Some(incarnation)
}

/// Write the readiness of incarnation `incarnation` (serving intent `seq`), with the time of its
/// last fresh success. A write for an incarnation or intent that is no longer current changes
/// nothing (INV-12).
fn set_readiness(
    shared: &Shared,
    seq: u64,
    incarnation: u64,
    readiness: Readiness,
    last_ok: Option<std::time::Instant>,
) {
    let mut c = shared.lock();
    if c.intent_sequence != seq || c.incarnation != incarnation || c.phase != Phase::Alive {
        return;
    }
    if c.readiness != readiness || c.last_ok != last_ok {
        c.readiness = readiness;
        c.last_ok = last_ok;
        c.touch();
    }
}

/// After a teardown (the child is reaped): the legacy view shows Off with no pid at once. The
/// phase stays `Stopping` until the Stop settles.
fn after_teardown(shared: &Shared) {
    let mut c = shared.lock();
    c.status.state = SupervisorState::Off;
    c.status.pid = None;
    c.touch();
}

/// The retry budget for intent `seq` is spent: Failed, and a pending Start receipt says why.
fn enter_failed(shared: &Shared, seq: u64, reason: &str) {
    // One critical section: a Start that lands after the Failed phase is visible must find the
    // failing intent already settled, so it commits a fresh intent instead of joining this one.
    {
        let mut c = shared.lock();
        c.apply_state(SupervisorState::Failed);
        c.settle(
            seq,
            IntentKind::Start,
            Receipt::Failed {
                reason: reason.to_string(),
            },
        );
    }
    if let Some(barrier) = &shared.failed_barrier {
        barrier();
    }
}

/// What a Stop's teardown resolved, for its `Complete` scope.
#[derive(Clone, Copy)]
struct TornDown {
    child: bool,
    spawn_ticket: bool,
    watch: bool,
}

/// Settle Stop intent `seq` after its teardown: `Complete` once every log reader has ended and
/// every admitted publication ticket is resolved, otherwise `Incomplete` naming what remains,
/// and the service is Quarantined. Returns false if a newer intent arrived meanwhile (the caller
/// handles that one instead).
fn settle_stop(shared: &Shared, seq: u64, readers: &mut Vec<Reader>, torn: TornDown) -> bool {
    let deadline = std::time::Instant::now() + READER_DRAIN;
    let (mut remaining, publications) = loop {
        let remaining = drain_readers(readers, Duration::ZERO);
        let publications: Vec<String> = shared
            .lock()
            .publications
            .iter()
            .map(|(_, _, n)| format!("publication:{n}"))
            .collect();
        if (remaining.is_empty() && publications.is_empty())
            || std::time::Instant::now() >= deadline
        {
            break (remaining, publications);
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut c = shared.lock();
    if c.closing || c.intent_sequence != seq {
        return false;
    }
    c.status.state = SupervisorState::Off;
    c.status.pid = None;
    if remaining.is_empty() && publications.is_empty() {
        let mut scope = Vec::new();
        if torn.child {
            scope.push("child".to_string());
            if torn.watch {
                scope.push("childWatch".to_string());
            }
        }
        if torn.spawn_ticket {
            scope.push("spawnTicket".to_string());
        }
        scope.push("readers".to_string());
        scope.append(&mut c.resolved_publications);
        c.phase = Phase::Stopped;
        c.quarantine = None;
        c.settle(seq, IntentKind::Stop, Receipt::Complete { scope });
    } else {
        let reason = if remaining.is_empty() {
            "a credential publication admitted before the Stop has not been resolved".to_string()
        } else {
            "the child ended but its output is still open: a log reader has not reached end of \
             file, so another process (for example one the child started) may still hold the pipe"
                .to_string()
        };
        remaining.extend(publications);
        c.phase = Phase::Quarantined;
        c.quarantine = Some(reason.clone());
        c.resolved_publications.clear();
        c.settle(
            seq,
            IntentKind::Stop,
            Receipt::Incomplete { remaining, reason },
        );
    }
    c.touch();
    true
}

/// Shutdown: the child is already torn down. Give its readers the same bounded window a Stop
/// does, settle what is pending, leave the legacy view Off, and let the monitor exit. Readers
/// still running are handed to the cell, so a later ticket release or snapshot read can
/// reconcile the quarantine without a monitor.
fn finish_closing(shared: &Shared, readers: &mut Vec<Reader>, tore_down_child: bool) {
    // The same bounded window a Stop gives its readers to reach end of file.
    let held_readers = drain_readers(readers, READER_DRAIN);
    let mut c = shared.lock();
    c.status.state = SupervisorState::Off;
    c.status.pid = None;
    // Open publication tickets are owned work too: shutdown never reports them done.
    let mut remaining = held_readers.clone();
    remaining.extend(
        c.publications
            .iter()
            .map(|(_, _, n)| format!("publication:{n}")),
    );
    c.closed = true;
    c.closed_readers.append(readers);
    if let Some((token, kind)) = c.active {
        let receipt = match kind {
            IntentKind::Start => Receipt::Superseded,
            IntentKind::Stop if remaining.is_empty() => {
                let mut scope = Vec::new();
                if tore_down_child {
                    scope.push("child".to_string());
                }
                scope.push("readers".to_string());
                scope.append(&mut c.resolved_publications);
                Receipt::Complete { scope }
            }
            IntentKind::Stop => Receipt::Incomplete {
                remaining: remaining.clone(),
                reason: "shutdown: owned work had not ended (a log reader still open, or a \
                         credential publication not yet resolved)"
                    .to_string(),
            },
        };
        c.settle(token.intent_sequence, kind, receipt);
    }
    if remaining.is_empty() {
        c.phase = Phase::Stopped;
        c.quarantine = None;
    } else {
        c.phase = Phase::Quarantined;
        if c.quarantine.is_none() {
            c.quarantine = Some(format!(
                "shutdown left owned work running: {}",
                remaining.join(", ")
            ));
        }
    }
    c.touch();
}

/// The result of a `supervise_running` wait.
enum RunOutcome {
    /// The child exited on its own (unexpected) → crash path.
    Crashed { status: std::process::ExitStatus },
    /// A newer intent (a Stop or a restart) was committed, or shutdown began.
    IntentChanged,
    /// Outside the start grace, a probe failed, timed out, or could not run for a whole interval.
    Unhealthy,
}

/// One probe worker. The probe closure runs on its OWN thread so a probe that blocks (or hangs
/// forever) cannot stall the monitor's crash detection or control handling (F-2). A worker is
/// tagged with the incarnation it samples; a result for any other incarnation, or one that
/// arrives after the worker's timeout, is discarded.
struct ProbeRunner {
    /// The probe's answer and the monotonic time it finished, taken on the worker thread.
    handle: JoinHandle<(bool, std::time::Instant)>,
    incarnation: u64,
    started: std::time::Instant,
    timed_out: bool,
}

impl ProbeRunner {
    /// Spawn the probe for `incarnation` on its own thread. `None` if the OS refuses the thread.
    fn spawn(probe: &Arc<dyn Fn() -> bool + Send + Sync>, incarnation: u64) -> Option<Self> {
        let p = probe.clone();
        std::thread::Builder::new()
            .name("supervisor:probe".into())
            .spawn(move || {
                let healthy = p();
                (healthy, std::time::Instant::now())
            })
            .ok()
            .map(|handle| ProbeRunner {
                handle,
                incarnation,
                started: std::time::Instant::now(),
                timed_out: false,
            })
    }

    /// Whether this worker's result still counts for `incarnation`.
    fn counts_for(&self, incarnation: u64) -> bool {
        self.incarnation == incarnation && !self.timed_out
    }
}

/// A finished probe worker, taken out of the slot.
struct Finished {
    /// The incarnation the worker sampled.
    incarnation: u64,
    /// The monitor had already timed it out (and handled that sample).
    timed_out: bool,
    /// Its answer came after its interval. A late answer never counts, however soon after its
    /// deadline it is collected.
    late: bool,
    /// Its answer. A panicked probe did not attest health (false).
    healthy: bool,
    /// When the worker finished, on the monotonic clock (taken on the worker thread). The
    /// sample's time, however late the monitor collects it.
    finished_at: std::time::Instant,
}

/// Take a finished worker out of the slot. `None` while it is still running.
fn take_finished(slot: &mut Option<ProbeRunner>, interval: Duration) -> Option<Finished> {
    if !slot.as_ref().is_some_and(|r| r.handle.is_finished()) {
        return None;
    }
    let runner = slot.take()?;
    let (healthy, late, finished_at) = match runner.handle.join() {
        Ok((healthy, done)) => (
            healthy,
            done.saturating_duration_since(runner.started) >= interval,
            done,
        ),
        Err(_) => (false, false, std::time::Instant::now()),
    };
    Some(Finished {
        incarnation: runner.incarnation,
        timed_out: runner.timed_out,
        late,
        healthy,
        finished_at,
    })
}

/// What `supervise_running` watches over: the intent and incarnation it serves, the probe and
/// child-watch hooks, and the credit window.
struct Running<'a> {
    seq: u64,
    incarnation: u64,
    health: Option<&'a HealthCheck>,
    watch: Option<&'a ChildWatch>,
    healthy_after: Duration,
}

/// The success window of a probed incarnation (SPEC-001 "Process state, readiness and
/// retries"): when its first success was, when the last one was, and whether this window has
/// already earned its one credit.
#[derive(Default)]
struct Window {
    first_ok: Option<std::time::Instant>,
    last_ok: Option<std::time::Instant>,
    credited: bool,
}

/// What one successful sample did to the success window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Success {
    /// Fresh evidence: the incarnation is Ready; `credit` if this sample completed the window's
    /// one credit.
    Fresh { credit: bool },
    /// Evidence already older than the freshness limit when it was collected: not Ready.
    Stale,
}

impl Window {
    /// Apply a successful sample taken at `at` and collected at `now`.
    fn record_success(
        &mut self,
        at: std::time::Instant,
        now: std::time::Instant,
        fresh_limit: Duration,
        healthy_after: Duration,
    ) -> Success {
        let _ = (at, fresh_limit);
        let first = *self.first_ok.get_or_insert(now);
        self.last_ok = Some(now);
        if !self.credited && now.duration_since(first) >= healthy_after {
            self.credited = true;
            return Success::Fresh { credit: true };
        }
        Success::Fresh { credit: false }
    }
}

/// Wait while the child is Running: poll for exit and for a newer intent, and (if configured)
/// run the health probe off-thread on its interval. Returns as soon as one of them ends the run.
///
/// Readiness and credit (SC2), all on the monotonic clock:
/// - Probed: a fresh success of this incarnation makes it `Ready`; successes that span
///   `healthy_after` with no gap over the freshness limit earn one credit, which resets the
///   retry budget at once. A failed or timed-out sample inside the start grace ends the window
///   (`Unavailable`, or still `Awaiting` if it never was ready); outside it the run is
///   `Unhealthy`. `Ready` older than the freshness limit becomes `Stale` and ends the window.
/// - Liveness-only: `NotAssessed`; running `healthy_after` without a break earns the credit.
///
/// `slot` is the service's one probe slot. A worker that times out keeps it until it really
/// ends; a new worker starts only once it is free. If the slot stays taken for a whole interval
/// past a sample's due time, this incarnation could not be sampled: inside the grace that ends
/// the window, outside it the run is `Unhealthy`.
fn supervise_running(
    child: &mut Child,
    shared: &Shared,
    run: Running<'_>,
    slot: &mut Option<ProbeRunner>,
) -> RunOutcome {
    use std::time::Instant;
    let poll = Duration::from_millis(20);
    let started = Instant::now();
    let mut next_watch = started;
    let mut window = Window::default();
    let mut shown = run
        .health
        .map_or(Readiness::NotAssessed, |_| Readiness::Awaiting);
    let mut next_probe = run.health.map(|hc| started + hc.interval);
    let (seq, inc) = (run.seq, run.incarnation);
    let publish = |readiness: Readiness, last_ok: Option<Instant>| {
        set_readiness(shared, seq, inc, readiness, last_ok);
    };

    loop {
        let now = Instant::now();
        // 1) a newer intent (Stop, restart) or shutdown, read from the cell at this bounded point.
        if shared.intent_changed(seq) {
            return RunOutcome::IntentChanged;
        }
        // 2) child exit (non-blocking reap).
        match child.try_wait() {
            Ok(Some(status)) => return RunOutcome::Crashed { status },
            Ok(None) => {
                // Still running and not reaped: `child.id()` names this child.
                if let Some(w) = run.watch {
                    if now >= next_watch {
                        next_watch = now + w.interval;
                        (w.observe)(child.id());
                    }
                }
            }
            // Cannot query the child; treat as crashed so we recover.
            Err(_) => {
                return RunOutcome::Crashed {
                    status: default_failed_status(),
                }
            }
        }
        let Some(hc) = run.health else {
            // 3a) liveness-only: continuous running earns the credit, once per incarnation.
            if !window.credited && now.duration_since(started) >= run.healthy_after {
                window.credited = true;
                grant_credit(shared);
            }
            std::thread::sleep(poll);
            continue;
        };
        // 3b) probed.
        let in_grace = now.duration_since(started) < hc.grace;
        let fresh_limit = hc.interval.saturating_mul(2);
        // A sample that ends the success window: tolerated inside the grace, a failure outside.
        macro_rules! sample_failed {
            () => {{
                if !in_grace {
                    return RunOutcome::Unhealthy;
                }
                window = Window::default();
                if shown != Readiness::Awaiting {
                    shown = Readiness::Unavailable;
                }
                publish(shown, None);
            }};
        }
        if let Some(done) = take_finished(slot, hc.interval) {
            // A result for another incarnation, or of a worker already timed out, is inert. A
            // late answer collected before its timeout was noticed is that timeout: it never
            // counts as a success.
            let counts = done.incarnation == inc && !done.timed_out;
            if counts && done.late {
                sample_failed!();
            } else if counts {
                if done.healthy {
                    match window.record_success(
                        done.finished_at,
                        now,
                        fresh_limit,
                        run.healthy_after,
                    ) {
                        Success::Fresh { credit } => {
                            shown = Readiness::Ready;
                            publish(shown, window.last_ok);
                            if credit {
                                grant_credit(shared);
                            }
                        }
                        Success::Stale => {
                            if shown == Readiness::Ready {
                                shown = Readiness::Stale;
                                publish(shown, None);
                            }
                        }
                    }
                } else {
                    sample_failed!();
                }
            }
        }
        if let Some(runner) = slot.as_mut() {
            if runner.counts_for(inc) && now.duration_since(runner.started) >= hc.interval {
                // The observer deadline passed: the result will not count, but the worker keeps
                // the slot until it really ends.
                runner.timed_out = true;
                sample_failed!();
            }
        }
        // Freshness: Ready whose last success is too old is Stale and ends the window.
        if shown == Readiness::Ready
            && window
                .last_ok
                .is_none_or(|t| now.duration_since(t) > fresh_limit)
        {
            window = Window::default();
            shown = Readiness::Stale;
            publish(shown, None);
        }
        if let Some(due) = next_probe {
            if now >= due {
                match slot.as_ref() {
                    None => {
                        next_probe = Some(now + hc.interval);
                        *slot = ProbeRunner::spawn(&hc.probe, inc);
                        if slot.is_none() {
                            // The OS refused the worker thread: no sample this interval.
                            sample_failed!();
                        }
                    }
                    // The slot is still taken (a timed-out worker, or one from an earlier
                    // incarnation) a whole interval after this sample was due.
                    Some(r) if !r.counts_for(inc) && now >= due + hc.interval => {
                        next_probe = Some(now);
                        sample_failed!();
                    }
                    Some(_) => {}
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

/// Sleep out the backoff delay for intent `seq`, waking early when a newer intent (a Stop, or
/// shutdown) is committed. Publishes `Backoff { next_retry_ms }` so a poller sees when the next
/// attempt is due. The wait is measured on the monotonic clock; a wakeup never restarts it.
/// The caller's loop then re-reads the intent: a Stop cancels the pending retry.
fn backoff_wait(shared: &Shared, seq: u64, delay: Duration, clock: &dyn Clock) {
    let next_retry_ms = clock.now_unix_ms().saturating_add(delay.as_millis() as u64);
    {
        let mut c = shared.lock();
        if c.closing || c.intent_sequence != seq {
            return;
        }
        c.status.state = SupervisorState::Backoff { next_retry_ms };
        c.phase = Phase::Backoff;
        c.touch();
    }
    let deadline = std::time::Instant::now() + delay;
    loop {
        if shared.intent_changed(seq) {
            return;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return;
        }
        shared.wait_wake(Some(deadline - now));
    }
}

/// Record a crash into the shared status (bumps `restarts`, sets `last_crash`).
fn record_crash_status(shared: &Shared, record: &CrashRecord) {
    let mut c = shared.lock();
    c.status.restarts = c.status.restarts.saturating_add(1);
    c.status.last_crash = Some(record.clone());
    c.touch();
}

/// Clear the live pid from the status (child is gone).
fn clear_pid(shared: &Shared) {
    let mut c = shared.lock();
    c.status.pid = None;
    c.touch();
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    include!("supervisor_tests.rs");

    #[cfg(windows)]
    include!("supervisor_windows_tests.rs");

    // The lifecycle cell (SC1): platform-neutral, so it runs on every OS lane.
    include!("supervisor_lifecycle_tests.rs");
}
