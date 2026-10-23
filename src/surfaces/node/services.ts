// =====================================================================
// citrate-core — Node › Services: the nine supervised processes (core plan K1/K3, SPEC-001)
//
// There is no registry snapshot command yet (K1, SC1/SC2), so each row is read through the status
// command its service already has. Data sources (Rule 7), one per row:
//   Node         node_status.state            (node.rs map_state)
//   Node agent   agent_status.state           (agent.rs)
//   Memory       memory_status.state          (memory.rs)
//   Llama chat   model_serve_status.state     (serve.rs)
//   Llama embed  none yet: no status command  (embed_serve.rs stops with Hermes)
//   IPFS         ipfs_status (a bool: running or not)
//   Hermes       hermes_status.state          (hermes.rs)
//   Comms        none yet for the process; comms_relay_status (groups.relayStatus) is relay health only
//   Cluster      none yet: cluster_status reads a group, not the daemon
// Phase follows SPEC-001: stopped/starting/running/restarting/failed map onto Stopped, Starting,
// Alive, Backoff, Failed; the D-12 phases (Stopping, Quarantined) pass through when a DTO carries
// them. Readiness (D-12) is read only from a `readiness` field; none of today's DTOs has one, so it
// shows "—". `healthy` is the supervisor's Running, not a probe, and is never shown as Ready.
// =====================================================================

export type ServiceId = "node" | "agent" | "memory" | "llama" | "embed" | "ipfs" | "hermes" | "comms" | "cluster";
export type Phase = "stopped" | "starting" | "alive" | "backoff" | "failed" | "stopping" | "quarantined";
export type Readiness = "notAssessed" | "awaiting" | "ready" | "stale" | "unavailable";

export const PHASE_LABEL: Record<Phase, string> = {
  stopped: "Stopped",
  starting: "Starting",
  alive: "Alive",
  backoff: "Backoff",
  failed: "Failed",
  stopping: "Stopping",
  quarantined: "Quarantined",
};

export const READINESS_LABEL: Record<Readiness, string> = {
  notAssessed: "Not assessed",
  awaiting: "Awaiting",
  ready: "Ready",
  stale: "Stale",
  unavailable: "Unavailable",
};

export interface ServiceDef {
  id: ServiceId;
  name: string;
  desc: string;
  /** Where the row's phase comes from, or why there is none. */
  statusSource: string;
  /** The commands Restart runs, or null when there is no stop/start pair. */
  restartSource: string | null;
  /** Where the log comes from, or null when no log read exists yet (SC4). */
  logSource: string | null;
  module: string;
}

export const SERVICES: readonly ServiceDef[] = [
  { id: "node", name: "Node", desc: "citrate-node, the full node for 40204", statusSource: "node_status.state", restartSource: "node_stop, node_start", logSource: "node_logs (the supervisor's ring of stdout and stderr)", module: "node.rs" },
  { id: "agent", name: "Node agent", desc: "node-agent, the node's local control API", statusSource: "agent_status.state", restartSource: "agent_stop, agent_start", logSource: null, module: "agent.rs" },
  { id: "memory", name: "Memory", desc: "the memory graph daemon on a local socket", statusSource: "memory_status.state", restartSource: "memory_stop, memory_start", logSource: null, module: "memory.rs" },
  { id: "llama", name: "Llama chat", desc: "llama-server with the local chat model", statusSource: "model_serve_status.state", restartSource: "model_serve_stop, model_serve_start", logSource: null, module: "serve.rs" },
  { id: "embed", name: "Llama embed", desc: "the embedding server for memory and Hermes", statusSource: "none yet: no status command for the embedding server (SC2)", restartSource: null, logSource: null, module: "embed_serve.rs" },
  { id: "ipfs", name: "IPFS", desc: "bundled kubo for files and pins", statusSource: "ipfs_status (running: yes or no)", restartSource: "ipfs_stop, ipfs_start", logSource: null, module: "ipfs.rs" },
  { id: "hermes", name: "Hermes", desc: "the Hermes agent sidecar", statusSource: "hermes_status.state", restartSource: "hermes_stop, hermes_start", logSource: null, module: "hermes.rs" },
  { id: "comms", name: "Comms", desc: "the group messaging daemon", statusSource: "none yet for the process; comms_relay_status (groups.relayStatus) is relay health only", restartSource: null, logSource: null, module: "comms.rs" },
  { id: "cluster", name: "Cluster", desc: "co-pinning and linked devices", statusSource: "none yet: cluster_status reads a group, not the daemon", restartSource: null, logSource: null, module: "cluster.rs" },
];

const PHASES: Record<string, Phase> = {
  stopped: "stopped",
  starting: "starting",
  running: "alive",
  alive: "alive",
  restarting: "backoff",
  backoff: "backoff",
  failed: "failed",
  stopping: "stopping",
  quarantined: "quarantined",
};

/** SPEC-001 phase from a supervisor state string; null when it isn't one we know. */
export function phaseFrom(state: unknown): Phase | null {
  if (typeof state !== "string") return null;
  return PHASES[state.trim().toLowerCase()] ?? null;
}

const READINESS: Record<string, Readiness> = {
  notassessed: "notAssessed",
  not_assessed: "notAssessed",
  awaiting: "awaiting",
  ready: "ready",
  stale: "stale",
  unavailable: "unavailable",
};

/** D-12 readiness from a DTO's `readiness` field; null when absent or unknown. */
export function readinessFrom(v: unknown): Readiness | null {
  if (typeof v !== "string") return null;
  return READINESS[v.trim().toLowerCase()] ?? null;
}

/** A status DTO as it crosses the bridge: a supervisor state, and readiness once SC2 adds it. */
export interface StatusDto {
  state?: unknown;
  readiness?: unknown;
}

/** The status reads behind the table, one per service that has one. */
export interface ServiceReads {
  node(): Promise<StatusDto>;
  agent(): Promise<StatusDto>;
  memory(): Promise<StatusDto>;
  llama(): Promise<StatusDto>;
  ipfs(): Promise<boolean>;
  hermes(): Promise<StatusDto>;
  commsRelay(): Promise<string>;
}

export interface ServiceRow {
  id: ServiceId;
  /** null = unknown (no status command, or the read failed). */
  phase: Phase | null;
  /** null = not provided by the DTO ("—"). */
  readiness: Readiness | null;
  /** Why the row reads as it does, when that isn't plain from the phase. */
  note: string;
  /** The read's failure, when it failed. */
  error?: string;
}

const errText = (e: unknown): string => (e instanceof Error ? e.message : typeof e === "string" && e.trim() ? e : "no answer");

const fromDto = (id: ServiceId, dto: StatusDto): ServiceRow => {
  const phase = phaseFrom(dto.state);
  return {
    id,
    phase,
    readiness: readinessFrom(dto.readiness),
    note: phase ? "" : "the status read returned a state this page doesn't know: " + String(dto.state),
  };
};

/** Read every service through its own command. A failed read is that row's error; the others still read. */
export async function readServices(r: ServiceReads): Promise<ServiceRow[]> {
  const dtoRead = async (id: ServiceId, f: () => Promise<StatusDto>): Promise<ServiceRow> => {
    try {
      return fromDto(id, await f());
    } catch (e) {
      return { id, phase: null, readiness: null, note: "", error: errText(e) };
    }
  };
  const [node, agent, memory, llama, hermes] = await Promise.all([
    dtoRead("node", r.node),
    dtoRead("agent", r.agent),
    dtoRead("memory", r.memory),
    dtoRead("llama", r.llama),
    dtoRead("hermes", r.hermes),
  ]);
  let ipfs: ServiceRow;
  try {
    ipfs = (await r.ipfs())
      ? { id: "ipfs", phase: "alive", readiness: null, note: "" }
      : { id: "ipfs", phase: null, readiness: null, note: "ipfs_status says not running; it can't tell stopped, starting and failed apart" };
  } catch (e) {
    ipfs = { id: "ipfs", phase: null, readiness: null, note: "", error: errText(e) };
  }
  let comms: ServiceRow;
  try {
    comms = { id: "comms", phase: null, readiness: null, note: "process phase not exposed yet; relay health: " + (await r.commsRelay()) };
  } catch (e) {
    comms = { id: "comms", phase: null, readiness: null, note: "process phase not exposed yet", error: errText(e) };
  }
  const embed: ServiceRow = { id: "embed", phase: null, readiness: null, note: "no status command for the embedding server yet" };
  const cluster: ServiceRow = { id: "cluster", phase: null, readiness: null, note: "no process status command for the cluster daemon yet" };
  const byId: Record<ServiceId, ServiceRow> = { node, agent, memory, llama, embed, ipfs, hermes, comms, cluster };
  return SERVICES.map((d) => byId[d.id]);
}

/** A service's stop and start commands. */
export interface ServiceControl {
  stop(): Promise<void>;
  start(): Promise<void>;
}
export type RestartableId = "node" | "agent" | "memory" | "llama" | "ipfs" | "hermes";
export type ServiceControls = Record<RestartableId, ServiceControl>;

const isRestartable = (id: ServiceId): id is RestartableId => SERVICES.some((d) => d.id === id && d.restartSource != null);

/** Whether Restart (or Start) is allowed for a service in this phase, and why not. */
export function canRestart(id: ServiceId, phase: Phase | null): { ok: boolean; why: string } {
  if (!isRestartable(id)) return { ok: false, why: "No stop or start command for this service yet." };
  if (phase == null) return { ok: false, why: "Its state is unknown, so this page won't start a second process over one it can't see." };
  if (phase === "quarantined") return { ok: false, why: "Refused while the old process is still owned. Restart opens once cleanup finishes." };
  if (phase === "starting" || phase === "stopping") return { ok: false, why: "Wait until it has finished " + (phase === "starting" ? "starting" : "stopping") + "." };
  return { ok: true, why: "" };
}

export interface RestartOptions {
  pollMs: number;
  timeoutMs: number;
}
export const RESTART_DEFAULTS: RestartOptions = { pollMs: 500, timeoutMs: 15_000 };

/**
 * Restart one service with its own commands: stop, wait until its status reads stopped (or failed),
 * then start. A stopped or failed service is started without a stop. It never starts a second
 * process while the old one is still reported running.
 */
export async function restartService(
  id: RestartableId,
  controls: ServiceControls,
  status: () => Promise<Phase | null>,
  opts: RestartOptions = RESTART_DEFAULTS,
  current: Phase | null = "alive",
): Promise<{ ok: boolean; message: string }> {
  const c = controls[id];
  if (current !== "stopped" && current !== "failed") {
    try {
      await c.stop();
    } catch (e) {
      return { ok: false, message: "Stop failed: " + errText(e) };
    }
    const deadline = Date.now() + opts.timeoutMs;
    for (;;) {
      let p: Phase | null = null;
      try {
        p = await status();
      } catch {
        p = null;
      }
      if (p === "stopped" || p === "failed") break;
      if (Date.now() >= deadline) return { ok: false, message: "It is still reported running, so it wasn't started again." };
      await new Promise((r) => setTimeout(r, opts.pollMs));
    }
  }
  try {
    await c.start();
  } catch (e) {
    return { ok: false, message: "Start failed: " + errText(e) };
  }
  return { ok: true, message: "Started." };
}

/** The status read restart waits on, for one service. */
export function phaseReader(id: RestartableId, r: ServiceReads): () => Promise<Phase | null> {
  if (id === "ipfs") return async () => ((await r.ipfs()) ? "alive" : "stopped");
  return async () => phaseFrom((await r[id]()).state);
}
