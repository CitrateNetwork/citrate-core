// citrate-core: bridge domain `services`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ---- services: the K1a-2 registry snapshot (Rust `services.rs`, `services_snapshot`) ----
// Each row is read from that service's kit supervisor (SPEC-001 lifecycle cell). `observation`
// says whether a supervisor was read at all; `phase` and everything after it are null unless it
// was. Readiness is its own axis (SC2): Alive never implies Ready.
export type ServiceId = "node" | "nodeAgent" | "memory" | "llamaChat" | "llamaEmbed" | "ipfs" | "hermes" | "comms" | "cluster";
export type ServicePhase = "stopped" | "starting" | "alive" | "backoff" | "failed" | "stopping" | "quarantined";
export type ServiceReadiness = "notAssessed" | "awaiting" | "ready" | "stale" | "unavailable";
export interface ServiceEntry {
  id: ServiceId;
  label: string;
  owner: string;
  prerequisites: string[];
  probe: string;
  readySignal: string;
  logBudgetLines: number;
  logsCommand: string | null;
  controls: boolean;
  observation: "observed" | "notSupervised" | "busy";
  phase: ServicePhase | null;
  readiness: ServiceReadiness | null;
  lastTransition: { from: ServicePhase | null; to: ServicePhase; atUnixMs: number } | null;
  restarts: number | null;
  lastCrash: { atUnixMs: number; exit: string; restartAttempt: number } | null;
  quarantine: string | null;
}
export interface ServicesSnapshot {
  takenAtUnixMs: number;
  services: ServiceEntry[];
}
// ---- CORE-D4: `sidecars_health` (Rust `services.rs`). One row per sidecar, each read from that
// sidecar's kit supervisor: the registry row above plus its retry counters, the slow retry after
// Failed (CORE-D3), the last N captured log lines (scrubbed in Rust: home path, key shapes, hex
// blobs) and, for the node, the liveness note (CORE-D2). Counters cross as decimal strings.
export interface SidecarRetryCounters {
  attempts: string;
  respawns: string;
  initialFailures: string;
  consecutiveFailures: string;
  maxRetries: string;
  healthCredits: string;
}
export interface SidecarSlowRetry {
  everyMs: string;
  nextAtUnixMs: string;
  attempts: string;
}
export interface SidecarLogLine {
  ts: number;
  stream: "out" | "err" | string;
  line: string;
}
/** CORE-D5: a sidecar's log file on disk (path scrubbed in Rust: the home folder reads `~`). */
export interface SidecarLogFile {
  path: string;
  writeFailures: number;
  lastError: string | null;
}
export interface SidecarHealth extends ServiceEntry {
  counters: SidecarRetryCounters | null;
  slowRetry: SidecarSlowRetry | null;
  logTail: SidecarLogLine[];
  logLinesKept: number | null;
  note: string | null;
  /** CORE-D5: unset by a build without log files; null when no supervisor keeps one. */
  logFile?: SidecarLogFile | null;
}
/** CORE-D5: what `diagnostics_logs_export` wrote. Mirrors Rust `sidecar_logs::DiagnosticsExport`. */
export interface DiagnosticsExport {
  path: string;
  bytes: number;
  sidecars: string[];
  logLines: number;
}
export interface SidecarsHealth {
  takenAtUnixMs: number;
  services: SidecarHealth[];
}
/** The `sidecars://alert` event: a sidecar became Failed, or came back from Failed. */
export interface SidecarAlert {
  id: ServiceId;
  label: string;
  kind: "failed" | "recovered";
  reason: string | null;
  nextRetryAtUnixMs: number | null;
  atUnixMs: number;
}
export interface ServicesDomain {
  snapshot(): Promise<ServicesSnapshot>;
  /** CORE-D4: every sidecar's health row, with its last `lines` log lines (Rust clamps it). */
  health(lines?: number): Promise<SidecarsHealth>;
  /** CORE-D3: subscribe to sidecar alerts; resolves to the unsubscribe function. */
  onAlert(cb: (alert: SidecarAlert) => void): Promise<() => void>;
  /** CORE-D5: write the scrubbed diagnostics bundle (health, sidecar logs, crash records, panics)
   *  to a `.txt` file the member chose. Local only; nothing is sent. */
  exportDiagnostics(path: string): Promise<DiagnosticsExport>;
}
