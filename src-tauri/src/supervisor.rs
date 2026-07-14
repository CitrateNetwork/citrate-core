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
    /// How often to run the probe while Running.
    pub interval: Duration,
    /// The probe: returns `true` if the child is healthy. Must be cheap +
    /// non-blocking-ish (it runs on the monitor thread between waits).
    pub probe: Box<dyn Fn() -> bool + Send + Sync>,
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
    /// delivered literally. stderr is piped so the supervisor can capture a tail
    /// for the crash record; stdout is inherited; stdin is null.
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
        cmd.stdout(Stdio::inherit());
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
    /// The clock (inject a fake in tests).
    pub clock: Arc<dyn Clock>,
}

impl SupervisorConfig {
    /// A config with default backoff / grace / system clock.
    pub fn new(spec: SidecarSpec, crash_record_path: impl Into<PathBuf>) -> Self {
        SupervisorConfig {
            spec,
            backoff: BackoffPolicy::new(),
            crash_record_path: crash_record_path.into(),
            stop_grace: DEFAULT_STOP_GRACE,
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
    /// The live child's pid, mirrored out of the monitor for signalling from
    /// the controller side without reaching into the monitor's `Child`.
    /// (The monitor owns the `Child`; the controller signals by pid.)
    clock: Arc<dyn Clock>,
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
}

impl Supervisor {
    /// Spawn the sidecar and start supervising it. Returns immediately; the
    /// monitor thread does the spawning + waiting + restarting. The initial
    /// status is `Starting`.
    pub fn start(config: SupervisorConfig) -> Self {
        let name = config.spec.name.clone();
        let shared = Arc::new(Shared {
            status: Mutex::new(SupervisorStatus {
                name: name.clone(),
                state: SupervisorState::Starting,
                pid: None,
                restarts: 0,
                last_crash: None,
            }),
            clock: config.clock.clone(),
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let monitor_shared = shared.clone();
        let monitor = std::thread::Builder::new()
            .name(format!("supervisor:{name}"))
            .spawn(move || run_monitor(config, monitor_shared, rx))
            .expect("supervisor monitor thread must spawn");
        Supervisor {
            shared,
            control: tx,
            monitor: Some(monitor),
            name,
        }
    }

    /// A cloneable, read-only status snapshot. Safe to poll from the Tauri
    /// runtime while the monitor thread runs (takes the lock briefly, clones).
    pub fn status(&self) -> SupervisorStatus {
        self.shared.snapshot()
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
    pub fn shutdown(&mut self) {
        let _ = self.control.send(ControlMsg::Shutdown);
        if let Some(handle) = self.monitor.take() {
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

/// Drain a child's piped stderr into a bounded tail (last [`STDERR_TAIL_BYTES`]).
/// Reads to EOF (the child has exited by the time we call this), then keeps only
/// the trailing window so a chatty child cannot bloat the record.
fn read_stderr_tail(child: &mut Child) -> String {
    use std::io::Read;
    let mut buf = Vec::new();
    if let Some(mut err) = child.stderr.take() {
        // Bounded read: cap total bytes we retain. We read fully (stderr is a
        // pipe that the exited child has closed) but only keep the tail.
        let _ = err.read_to_end(&mut buf);
    }
    let start = buf.len().saturating_sub(STDERR_TAIL_BYTES);
    String::from_utf8_lossy(&buf[start..]).to_string()
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
        clock,
    } = config;

    // Consecutive-failure counter → drives the backoff delay + the Failed cap.
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

        // Publish Running + the live pid.
        {
            let mut s = shared.status.lock().unwrap_or_else(|e| e.into_inner());
            s.state = SupervisorState::Running;
            s.pid = Some(child.id());
        }

        // --- supervise: wait for exit OR a control message OR a health failure ---
        let outcome =
            supervise_running(&mut child, &control, spec.health_check.as_ref(), stop_grace);

        match outcome {
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
                // Unexpected exit → crash record + backoff + restart.
                let stderr_tail = read_stderr_tail(&mut child);
                let _ = child.wait(); // reap (already exited)
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
    /// A health-check probe failed.
    Unhealthy,
}

/// Wait while the child is Running: poll for exit, for a control message, and
/// (if configured) run the health probe on its interval. Returns as soon as one
/// of them fires. Uses short polling so a control message is handled promptly
/// without a blocking `wait()` that would ignore the channel.
fn supervise_running(
    child: &mut Child,
    control: &Receiver<ControlMsg>,
    health: Option<&HealthCheck>,
    _stop_grace: Duration,
) -> RunOutcome {
    let poll = Duration::from_millis(20);
    let mut since_probe = Duration::ZERO;
    loop {
        // 1) control message (non-blocking).
        match control.try_recv() {
            Ok(ControlMsg::Stop) => return RunOutcome::Stopped,
            Ok(ControlMsg::Shutdown) => return RunOutcome::Shutdown,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Controller dropped → treat as shutdown (no orphan).
                return RunOutcome::Shutdown;
            }
        }
        // 2) child exit (non-blocking reap).
        match child.try_wait() {
            Ok(Some(status)) => return RunOutcome::Crashed { status },
            Ok(None) => {}
            Err(_) => {
                // Cannot query the child; treat as crashed so we recover.
                return RunOutcome::Crashed {
                    status: default_failed_status(),
                };
            }
        }
        // 3) health probe on its interval.
        if let Some(hc) = health {
            since_probe += poll;
            if since_probe >= hc.interval {
                since_probe = Duration::ZERO;
                if !(hc.probe)() {
                    return RunOutcome::Unhealthy;
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
