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

use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use crate::supervisor::{Clock, Phase, PhaseTransition, Readiness, Supervisor, SystemClock};

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
    let slot = match slot {
        SlotRead::NoOwner => return unobserved(desc, ObservationKind::NotSupervised),
        SlotRead::OwnerBusy => return unobserved(desc, ObservationKind::Busy),
        SlotRead::Slot(slot) => slot,
    };
    let Some(guard) = lock_bounded(slot, SLOT_READ_BUDGET) else {
        return unobserved(desc, ObservationKind::Busy);
    };
    let Some(sup) = guard.as_ref() else {
        return unobserved(desc, ObservationKind::NotSupervised);
    };
    let Some(obs) = sup.observe() else {
        return unobserved(desc, ObservationKind::Busy);
    };
    drop(guard);
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

/// Blocking body of [`services_snapshot`]: finds each owner's slot without starting anything.
pub fn services_snapshot_sync(app: &tauri::AppHandle) -> ServicesSnapshot {
    use tauri::Manager;
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
    snapshot_with(&SystemClock, |id| match id {
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
    })
}

/// **services_snapshot** — every supervised service's registry row and its supervisor's state
/// (K1a-2). Read-only: it never starts, stops or builds a service.
#[tauri::command]
pub async fn services_snapshot(app_h: tauri::AppHandle) -> Result<ServicesSnapshot, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || Ok(services_snapshot_sync(&app_h))).await
}

#[cfg(test)]
mod tests {
    include!("services_tests.rs");
}
