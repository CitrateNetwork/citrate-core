//! citrate-core — the services registry and its snapshot (K1a-2, RC52; Node › Services).
//!
//! One registry of the nine processes this app supervises, each with its owner module,
//! prerequisites, probe, ready signal and log budget, and one read command, `services_snapshot`,
//! that Node › Services renders.
//!
//! Data source (Rule 7): each row is read from the service's own kit [`Supervisor`]
//! ([`Supervisor::observe`]: the SPEC-001 lifecycle cell, the last phase transition with the time
//! it happened on the supervisor's clock, the restart count and the last crash). Nothing here
//! invents a state:
//! - a service with no supervisor in its slot is `notSupervised` (never started in this session,
//!   or stopped through its owner, which releases the supervisor);
//! - a slot or cell that stays contended past the read budget is `busy`, with no phase claimed.
//!
//! Readiness is reported separately from the phase (SC2). Until SCL-S3 lands every supervisor
//! reports `notAssessed`, and that is what is shown.
//!
//! The registry is read-only. Start and stop stay with each service's owner path (the
//! `controls` flag marks the services whose owner path Node › Services offers).
//!
//! CORE-D4 adds `sidecars_health`: the same rows plus, from the same supervisor read, the retry
//! counters, the slow retry after `Failed` (CORE-D3), the last N captured log lines (scrubbed with
//! [`crate::telemetry::scrub`]: home path, key shapes, hex blobs, emails) and, for the node, the
//! liveness note (CORE-D2). CORE-D3 also adds the alert watch: when a sidecar becomes `Failed`, or
//! comes back from it, the main window is sent a `sidecars://alert` event.

use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use crate::supervisor::{
    Clock, LogLine, Observation, Phase, PhaseTransition, Readiness, RetryCounters, SlowRetry,
    Supervisor, SystemClock,
};

/// How long a snapshot read waits for a service's slot before it reports `busy` (SPEC-001: a
/// cached read never waits on an owner).
const SLOT_READ_BUDGET: Duration = Duration::from_millis(5);

/// The nine supervised services, in the order Node › Services lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceId {
    Node,
    NodeAgent,
    Memory,
    LlamaChat,
    LlamaEmbed,
    Ipfs,
    Hermes,
    Comms,
    Cluster,
}

/// One registry row: what the service is and who owns it. Static: it describes the code in this
/// build, not the running process.
#[derive(Debug)]
pub struct ServiceDescriptor {
    pub id: ServiceId,
    pub label: &'static str,
    /// The module that starts, stops and owns the service's supervisor.
    pub owner: &'static str,
    /// What the owner checks before it spawns the service.
    pub prerequisites: &'static [&'static str],
    /// What the supervisor checks while the child runs.
    pub probe: &'static str,
    /// What readiness will be judged on (SC2). The snapshot's `readiness` says what it is now.
    pub ready_signal: &'static str,
    /// The command that reads this service's captured log lines, if one exists.
    pub logs_command: Option<&'static str>,
    /// Whether Node › Services offers start and stop through the owner's path.
    pub controls: bool,
}

const PROCESS_ONLY: &str = "Process exit only: the supervisor restarts it if it exits";
const PROCESS_ONLY_READY: &str =
    "None: a process-only service reads Not assessed while it is Alive";

/// The registry. One row per supervised service; `services_tests` checks that every
/// `Supervisor::start` in core belongs to exactly one of these owners.
pub const REGISTRY: [ServiceDescriptor; 9] = [
    ServiceDescriptor {
        id: ServiceId::Node,
        label: "Node",
        owner: "src-tauri/src/node.rs",
        prerequisites: &[
            "the node binary is bundled",
            "startup cleanup has finished",
            "no other process holds the chain database or a node port",
            "the storage key is read from the keyring",
        ],
        probe: PROCESS_ONLY,
        ready_signal: PROCESS_ONLY_READY,
        logs_command: Some("node_logs"),
        controls: true,
    },
    ServiceDescriptor {
        id: ServiceId::NodeAgent,
        label: "Node agent",
        owner: "src-tauri/src/agent.rs",
        prerequisites: &[
            "the node-agent binary is bundled",
            "a session bearer is written to its owner-only file",
        ],
        probe: PROCESS_ONLY,
        ready_signal: PROCESS_ONLY_READY,
        logs_command: None,
        controls: false,
    },
    ServiceDescriptor {
        id: ServiceId::Memory,
        label: "Memory",
        owner: "src-tauri/src/memory.rs",
        prerequisites: &[
            "the memory daemon binary is bundled",
            "no knowledge import, backup or restore holds the memory store",
            "the store key is read from the keyring",
        ],
        probe: PROCESS_ONLY,
        ready_signal: PROCESS_ONLY_READY,
        logs_command: None,
        controls: true,
    },
    ServiceDescriptor {
        id: ServiceId::LlamaChat,
        label: "Llama chat",
        owner: "src-tauri/src/serve.rs",
        prerequisites: &[
            "the local model is verified",
            "the llama-server binary is bundled",
            "its loopback port is free",
        ],
        probe: "HTTP GET /health on its loopback port; a failing probe restarts it",
        ready_signal: "/health answers 200",
        logs_command: None,
        controls: false,
    },
    ServiceDescriptor {
        id: ServiceId::LlamaEmbed,
        label: "Llama embed",
        owner: "src-tauri/src/embed_serve.rs",
        prerequisites: &[
            "Hermes is starting (it starts this server)",
            "the bundled embedding model matches its pin",
            "the llama-server binary is bundled",
            "its loopback port is free",
        ],
        probe: "HTTP GET /health on its loopback port; a failing probe restarts it",
        ready_signal: "/health answers 200",
        logs_command: None,
        controls: false,
    },
    ServiceDescriptor {
        id: ServiceId::Ipfs,
        label: "IPFS",
        owner: "src-tauri/src/ipfs.rs",
        prerequisites: &[
            "the kubo binary is bundled",
            "the local IPFS repository is initialized",
        ],
        probe: PROCESS_ONLY,
        ready_signal: PROCESS_ONLY_READY,
        logs_command: None,
        controls: false,
    },
    ServiceDescriptor {
        id: ServiceId::Hermes,
        label: "Hermes",
        owner: "src-tauri/src/hermes.rs",
        prerequisites: &[
            "the Hermes sidecar binary is bundled",
            "its control port is free",
            "a session bearer is written to its owner-only file",
        ],
        probe: "HTTP GET /health on its control address; a failing probe restarts it",
        ready_signal: "/health answers 200",
        logs_command: None,
        controls: true,
    },
    ServiceDescriptor {
        id: ServiceId::Comms,
        label: "Comms",
        owner: "src-tauri/src/comms.rs",
        prerequisites: &[
            "the comms identity is provisioned",
            "the comms member daemon binary is bundled",
        ],
        probe: "Connects to its local socket; a failing probe restarts it",
        ready_signal: "its local socket accepts a connection",
        logs_command: None,
        controls: false,
    },
    ServiceDescriptor {
        id: ServiceId::Cluster,
        label: "Cluster",
        owner: "src-tauri/src/cluster.rs",
        prerequisites: &[
            "this machine's mesh identity is configured",
            "the cluster daemon binary is bundled",
        ],
        probe: "Connects to its local socket; a failing probe restarts it",
        ready_signal: "its local socket accepts a connection",
        logs_command: None,
        controls: false,
    },
];

/// Whether a row carries a read of the service's supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ObservationKind {
    /// The supervisor was read; `phase` and the fields after it are its values.
    Observed,
    /// No supervisor holds this service now. Nothing about a process is claimed.
    NotSupervised,
    /// The read did not get the slot or the cell within its budget. Nothing is claimed.
    Busy,
}

/// The last crash, without its stderr tail (which can carry paths and is not for this view).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashView {
    pub at_unix_ms: u64,
    pub exit: String,
    pub restart_attempt: u32,
}

/// One row of the snapshot: the registry descriptor plus what the supervisor reported.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEntry {
    pub id: ServiceId,
    pub label: &'static str,
    pub owner: &'static str,
    pub prerequisites: &'static [&'static str],
    pub probe: &'static str,
    pub ready_signal: &'static str,
    /// Lines the supervisor keeps for this service ([`crate::supervisor::LOG_RING_CAPACITY`]).
    pub log_budget_lines: usize,
    pub logs_command: Option<&'static str>,
    pub controls: bool,
    pub observation: ObservationKind,
    pub phase: Option<Phase>,
    pub readiness: Option<Readiness>,
    pub last_transition: Option<PhaseTransition>,
    pub restarts: Option<u32>,
    pub last_crash: Option<CrashView>,
    pub quarantine: Option<String>,
}

/// What `services_snapshot` returns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicesSnapshot {
    pub taken_at_unix_ms: u64,
    pub services: Vec<ServiceEntry>,
}

/// Where a service's supervisor slot is, as the snapshot found it.
pub(crate) enum SlotRead<'a> {
    /// The owner does not exist in this session (never built), so no supervisor exists.
    NoOwner,
    /// The owner's own slot is held (it is being built or started), so nothing is claimed.
    OwnerBusy,
    /// The owner's supervisor slot.
    Slot(&'a Mutex<Option<Supervisor>>),
}

/// The row for `desc`, read from the owner's supervisor slot.
pub(crate) fn entry(desc: &'static ServiceDescriptor, slot: SlotRead<'_>) -> ServiceEntry {
    read_slot(desc, slot, |_, _| ()).0
}

/// Read `desc`'s supervisor once: its row, and whatever `extra` takes from the same supervisor and
/// observation while the slot is held (nothing when there is no supervisor to read).
fn read_slot<T>(
    desc: &'static ServiceDescriptor,
    slot: SlotRead<'_>,
    extra: impl FnOnce(&Supervisor, &Observation) -> T,
) -> (ServiceEntry, Option<T>) {
    let slot = match slot {
        SlotRead::NoOwner => return (unobserved(desc, ObservationKind::NotSupervised), None),
        SlotRead::OwnerBusy => return (unobserved(desc, ObservationKind::Busy), None),
        SlotRead::Slot(slot) => slot,
    };
    let Some(guard) = lock_bounded(slot, SLOT_READ_BUDGET) else {
        return (unobserved(desc, ObservationKind::Busy), None);
    };
    let Some(sup) = guard.as_ref() else {
        return (unobserved(desc, ObservationKind::NotSupervised), None);
    };
    let Some(obs) = sup.observe() else {
        return (unobserved(desc, ObservationKind::Busy), None);
    };
    let more = extra(sup, &obs);
    drop(guard);
    (observed_entry(desc, obs), Some(more))
}

fn observed_entry(desc: &'static ServiceDescriptor, obs: Observation) -> ServiceEntry {
    ServiceEntry {
        observation: ObservationKind::Observed,
        phase: Some(obs.lifecycle.phase),
        readiness: Some(obs.lifecycle.readiness),
        last_transition: Some(obs.last_transition),
        restarts: Some(obs.lifecycle.restarts),
        last_crash: obs.last_crash.map(|c| CrashView {
            at_unix_ms: c.at_unix_ms,
            exit: c.exit,
            restart_attempt: c.restart_attempt,
        }),
        quarantine: obs.lifecycle.quarantine,
        ..unobserved(desc, ObservationKind::Observed)
    }
}

fn unobserved(desc: &'static ServiceDescriptor, observation: ObservationKind) -> ServiceEntry {
    ServiceEntry {
        id: desc.id,
        label: desc.label,
        owner: desc.owner,
        prerequisites: desc.prerequisites,
        probe: desc.probe,
        ready_signal: desc.ready_signal,
        log_budget_lines: crate::supervisor::LOG_RING_CAPACITY,
        logs_command: desc.logs_command,
        controls: desc.controls,
        observation,
        phase: None,
        readiness: None,
        last_transition: None,
        restarts: None,
        last_crash: None,
        quarantine: None,
    }
}

fn lock_bounded<T>(m: &Mutex<T>, budget: Duration) -> Option<MutexGuard<'_, T>> {
    let deadline = std::time::Instant::now() + budget;
    loop {
        match m.try_lock() {
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

/// The snapshot, given a way to find each service's supervisor slot.
pub(crate) fn snapshot_with<'a>(
    clock: &dyn Clock,
    slot_of: impl Fn(ServiceId) -> SlotRead<'a>,
) -> ServicesSnapshot {
    let services = REGISTRY
        .iter()
        .map(|desc| entry(desc, slot_of(desc.id)))
        .collect();
    ServicesSnapshot {
        taken_at_unix_ms: clock.now_unix_ms(),
        services,
    }
}

/// Finds each owner's supervisor slot without starting or building anything, binds the finder to
/// `$slot_of` and evaluates `$body` with it (a macro: the finder borrows the app's managed states,
/// which live only in this scope).
macro_rules! with_slots {
    ($app:expr, |$slot_of:ident| $body:expr) => {{
        use tauri::Manager;
        let app = $app;
        let node = app.try_state::<crate::node::NodeState>();
        let agent = app.try_state::<crate::agent::AgentState>();
        let memory = app.try_state::<crate::memory::MemoryState>();
        let serve = app.try_state::<crate::serve::ServeState>();
        let ipfs = app.try_state::<crate::ipfs::IpfsState>();
        let hermes = crate::hermes::existing_manager();
        let comms = crate::comms::existing_manager();
        let cluster = crate::cluster::existing_manager();
        fn slot<'a>(s: Option<&'a Mutex<Option<Supervisor>>>) -> SlotRead<'a> {
            s.map_or(SlotRead::NoOwner, SlotRead::Slot)
        }
        let $slot_of = |id: ServiceId| match id {
            ServiceId::Node => slot(node.as_ref().map(|s| s.inner().0.supervisor_slot())),
            ServiceId::NodeAgent => slot(agent.as_ref().map(|s| s.inner().0.supervisor_slot())),
            ServiceId::Memory => slot(memory.as_ref().map(|s| s.inner().0.supervisor_slot())),
            ServiceId::LlamaChat => slot(serve.as_ref().map(|s| s.inner().0.supervisor_slot())),
            ServiceId::LlamaEmbed => slot(
                hermes
                    .and_then(|h| h.embed_server())
                    .map(|e| e.supervisor_slot()),
            ),
            ServiceId::Ipfs => slot(ipfs.as_ref().map(|s| s.inner().0.supervisor_slot())),
            ServiceId::Hermes => slot(hermes.map(|h| h.supervisor_slot())),
            ServiceId::Comms => slot(comms.map(|c| c.supervisor_slot())),
            ServiceId::Cluster => match &cluster {
                Ok(c) => slot(c.as_deref().map(|c| c.supervisor_slot())),
                Err(crate::cluster::SlotHeld) => SlotRead::OwnerBusy,
            },
        };
        $body
    }};
}

/// Blocking body of [`services_snapshot`]: finds each owner's slot without starting anything.
pub fn services_snapshot_sync(app: &tauri::AppHandle) -> ServicesSnapshot {
    with_slots!(app, |slot_of| snapshot_with(&SystemClock, slot_of))
}

/// **services_snapshot** — every supervised service's registry row and its supervisor's state
/// (K1a-2). Read-only: it never starts, stops or builds a service.
#[tauri::command]
pub async fn services_snapshot(app_h: tauri::AppHandle) -> Result<ServicesSnapshot, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || Ok(services_snapshot_sync(&app_h))).await
}

// ---------------------------------------------------------------------------
// CORE-D3 — the slow retry period every sidecar's supervisor is given.
// ---------------------------------------------------------------------------

/// Overrides the slow retry period, in seconds (`0` or `off` turns it off).
pub const SLOW_RETRY_SECS_ENV: &str = "CITRATE_SIDECAR_SLOW_RETRY_SECS";
/// After a sidecar spent its retry budget, it is tried again this often (one attempt each time).
pub const DEFAULT_SLOW_RETRY: Duration = Duration::from_secs(10 * 60);
/// The shortest period an override may set.
pub const MIN_SLOW_RETRY: Duration = Duration::from_secs(60);

/// The slow retry period from `get` (the environment in production): the default, an override of
/// at least [`MIN_SLOW_RETRY`], or `None` when it is turned off.
pub(crate) fn slow_retry_from(get: impl Fn(&str) -> Option<String>) -> Option<Duration> {
    let Some(v) = get(SLOW_RETRY_SECS_ENV) else {
        return Some(DEFAULT_SLOW_RETRY);
    };
    let v = v.trim().to_ascii_lowercase();
    if v == "0" || v == "off" {
        return None;
    }
    match v.parse::<u64>().map(Duration::from_secs) {
        Ok(d) if d >= MIN_SLOW_RETRY => Some(d),
        _ => Some(DEFAULT_SLOW_RETRY),
    }
}

/// The slow retry every registry owner sets on its supervisor (`config.slow_retry`).
pub(crate) fn slow_retry_every() -> Option<Duration> {
    slow_retry_from(|k| std::env::var(k).ok())
}

// ---------------------------------------------------------------------------
// CORE-D4 — `sidecars_health`.
// ---------------------------------------------------------------------------

/// Log lines per sidecar when the caller does not say.
pub const HEALTH_LOG_LINES: usize = 40;
/// The most log lines per sidecar one read returns.
pub const MAX_HEALTH_LOG_LINES: usize = 200;

/// The tail length for a request: the default when unset, at most [`MAX_HEALTH_LOG_LINES`].
pub(crate) fn clamp_tail(asked: Option<u32>) -> usize {
    asked.map_or(HEALTH_LOG_LINES, |n| {
        usize::try_from(n)
            .unwrap_or(MAX_HEALTH_LOG_LINES)
            .min(MAX_HEALTH_LOG_LINES)
    })
}

/// One sidecar's health: its registry row plus what the same supervisor read reported.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarHealth {
    #[serde(flatten)]
    pub service: ServiceEntry,
    /// The retry counters of the lifecycle cell (attempts, failures in a row, the budget).
    pub counters: Option<RetryCounters>,
    /// While `Failed`: when the slow retry tries it next (CORE-D3).
    pub slow_retry: Option<SlowRetry>,
    /// The newest captured stdout/stderr lines, oldest first, each scrubbed.
    pub log_tail: Vec<LogLine>,
    /// How many lines the supervisor's ring holds now.
    pub log_lines_kept: Option<usize>,
    /// The owner's note for the member (the node's liveness note, CORE-D2).
    pub note: Option<String>,
    /// CORE-D5: the sidecar's log file on disk (its path, scrubbed, and any write failures).
    pub log_file: Option<citrate_core_kit::sidecar_log::LogFileStatus>,
}

/// What `sidecars_health` returns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarsHealth {
    pub taken_at_unix_ms: u64,
    pub services: Vec<SidecarHealth>,
}

/// The health row for `desc` with its last `tail` log lines, scrubbed against `home`.
pub(crate) fn health_entry(
    desc: &'static ServiceDescriptor,
    slot: SlotRead<'_>,
    tail: usize,
    home: &str,
) -> SidecarHealth {
    let (service, extra) = read_slot(desc, slot, |sup, obs| {
        let lines = sup.logs();
        let kept = lines.len();
        let log_tail: Vec<LogLine> = lines[kept.saturating_sub(tail)..]
            .iter()
            .map(|l| LogLine {
                ts: l.ts,
                stream: l.stream.clone(),
                line: crate::telemetry::scrub(&l.line, home),
            })
            .collect();
        let log_file = sup.log_file_status().map(|mut f| {
            f.path = crate::telemetry::scrub(&f.path, home);
            f.last_error = f.last_error.map(|e| crate::telemetry::scrub(&e, home));
            f
        });
        (
            obs.lifecycle.counters,
            obs.slow_retry,
            log_tail,
            kept,
            log_file,
        )
    });
    match extra {
        Some((counters, slow_retry, log_tail, kept, log_file)) => SidecarHealth {
            service,
            counters: Some(counters),
            slow_retry,
            log_tail,
            log_lines_kept: Some(kept),
            note: None,
            log_file,
        },
        None => SidecarHealth {
            service,
            counters: None,
            slow_retry: None,
            log_tail: Vec::new(),
            log_lines_kept: None,
            note: None,
            log_file: None,
        },
    }
}

/// Every sidecar's health row, given the slot finder and the owners' notes.
pub(crate) fn health_with<'a>(
    clock: &dyn Clock,
    tail: usize,
    home: &str,
    slot_of: impl Fn(ServiceId) -> SlotRead<'a>,
    note_of: impl Fn(ServiceId) -> Option<String>,
) -> SidecarsHealth {
    let services = REGISTRY
        .iter()
        .map(|desc| {
            let mut h = health_entry(desc, slot_of(desc.id), tail, home);
            h.note = note_of(desc.id);
            h
        })
        .collect();
    SidecarsHealth {
        taken_at_unix_ms: clock.now_unix_ms(),
        services,
    }
}

/// Blocking body of [`sidecars_health`].
pub fn sidecars_health_sync(app: &tauri::AppHandle, tail: usize) -> SidecarsHealth {
    use tauri::Manager;
    let home = std::env::var("HOME").unwrap_or_default();
    let node_note = app
        .try_state::<crate::node::NodeState>()
        .and_then(|s| s.inner().0.liveness_note());
    with_slots!(app, |slot_of| health_with(
        &SystemClock,
        tail,
        &home,
        slot_of,
        |id| if id == ServiceId::Node {
            node_note.clone()
        } else {
            None
        },
    ))
}

/// **sidecars_health** — every sidecar's state, restart counters, last crash, slow retry and last
/// `lines` log lines (default [`HEALTH_LOG_LINES`], at most [`MAX_HEALTH_LOG_LINES`]), each read
/// from that sidecar's supervisor (CORE-D4). Read-only.
#[tauri::command]
pub async fn sidecars_health(
    app_h: tauri::AppHandle,
    lines: Option<u32>,
) -> Result<SidecarsHealth, String> {
    let tail = clamp_tail(lines);
    crate::blocking::off_main(move || Ok(sidecars_health_sync(&app_h, tail))).await
}

// ---------------------------------------------------------------------------
// CORE-D3 — the alert watch.
// ---------------------------------------------------------------------------

/// The event the main window is sent when a sidecar becomes `Failed` or recovers from it.
pub const ALERT_EVENT: &str = "sidecars://alert";
/// How often the alert watch reads the sidecars.
const ALERT_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AlertKind {
    /// It spent its retry budget.
    Failed,
    /// It is `Alive` again after an alert said it failed.
    Recovered,
}

/// One alert, as the main window receives it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarAlert {
    pub id: ServiceId,
    pub label: &'static str,
    pub kind: AlertKind,
    /// The last exit, when there is one.
    pub reason: Option<String>,
    /// When the slow retry tries it next, if one is scheduled.
    pub next_retry_at_unix_ms: Option<u64>,
    pub at_unix_ms: u64,
}

/// Which sidecars have an open `Failed` alert. One alert when a sidecar fails, one when it is
/// `Alive` again; a stop by the member closes it silently; a read that saw nothing changes nothing.
#[derive(Debug, Default)]
pub struct AlertTracker {
    failed_open: Vec<ServiceId>,
}

impl AlertTracker {
    /// The alerts that `rows`, read at `now_ms`, raise.
    pub fn update(&mut self, rows: &[SidecarHealth], now_ms: u64) -> Vec<SidecarAlert> {
        let mut out = Vec::new();
        for h in rows {
            let id = h.service.id;
            let open = self.failed_open.contains(&id);
            let phase = match h.service.observation {
                ObservationKind::Observed => h.service.phase,
                ObservationKind::NotSupervised => Some(Phase::Stopped),
                ObservationKind::Busy => None,
            };
            match phase {
                Some(Phase::Failed) if !open => {
                    self.failed_open.push(id);
                    out.push(SidecarAlert {
                        id,
                        label: h.service.label,
                        kind: AlertKind::Failed,
                        reason: h.service.last_crash.as_ref().map(|c| c.exit.clone()),
                        next_retry_at_unix_ms: h.slow_retry.map(|r| r.next_at_unix_ms),
                        at_unix_ms: now_ms,
                    });
                }
                Some(Phase::Alive) if open => {
                    self.failed_open.retain(|x| *x != id);
                    out.push(SidecarAlert {
                        id,
                        label: h.service.label,
                        kind: AlertKind::Recovered,
                        reason: None,
                        next_retry_at_unix_ms: None,
                        at_unix_ms: now_ms,
                    });
                }
                Some(Phase::Stopped) => self.failed_open.retain(|x| *x != id),
                _ => {}
            }
        }
        out
    }
}

/// Start the alert watch: every [`ALERT_EVERY`] it reads every sidecar (no log lines) and sends the
/// main window an [`ALERT_EVENT`] for each alert; each is also written to the app log.
pub fn spawn_alert_watch(app: tauri::AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("sidecar-alerts".into())
        .spawn(move || {
            use tauri::Emitter;
            let mut tracker = AlertTracker::default();
            loop {
                std::thread::sleep(ALERT_EVERY);
                let health = sidecars_health_sync(&app, 0);
                for a in tracker.update(&health.services, health.taken_at_unix_ms) {
                    eprintln!(
                        "[services] {} {:?}{}",
                        a.label,
                        a.kind,
                        a.reason
                            .as_deref()
                            .map(|r| format!(": {r}"))
                            .unwrap_or_default()
                    );
                    let _ = app.emit_to("main", ALERT_EVENT, &a);
                }
            }
        });
    if let Err(e) = spawned {
        eprintln!("[services] the sidecar alert watch did not start: {e}");
    }
}

#[cfg(test)]
mod tests {
    include!("services_tests.rs");
}
