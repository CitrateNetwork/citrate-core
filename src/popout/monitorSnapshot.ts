// =====================================================================
// citrate-core — Activity monitor snapshot (HUP-S7.6, US-7.4)
//
// Data sources (Rule 7):
//   - turn: the turn activity slice, written by the real send path (store.sendChat)
//   - model: the ModelRouter's active choice (same resolution as the chat header)
//   - tier: the hardware tier report (bridge.tier.recommend, a local probe)
//   - context window: the local llama-server's --ctx-size, read from Rust (popout_monitor_facts)
//   - spend: local inference is free; gateway metering is not wired into the app yet
//   - daemons (HUP-S10.3): Rust daemons.rs via daemons_list (status, today's ledger, next run) and
//     the runner's state (why runs are held). Daemon tokens are measured from the model server's
//     usage when every call of a run reported it, otherwise estimated (characters / 4).
//   - runs (HUP-S2.2): command runs (shell_run and the toolchain tools) from their tool results,
//     recorded by the sidecar provider into the turn activity slice
//   - workers (HUP-S1.9): the agent sidecar's worker processes, read from Rust (hermes_workers →
//     the sidecar's GET /workers, i.e. its own process supervisor)
//   - decide (HUP-S5.3): the decide() slot's per-backend metering, read from Rust
//     (hermes_decide_stats → the sidecar's GET /decide/stats: every metered decision plus each task
//     outcome recorded through POST /decide/outcomes)
//   - usage (HUP-S7.6, US-7.4 AC1): the model server's own report for the latest model call of the
//     turn (core's `citrate_usage` on the in-app loop, the sidecar's `usage` event), giving context
//     used (prompt + completion tokens) and tokens per second (llama-server `timings.predicted_ms`)
//   - plan, approvals, verifier verdicts (HUP-S7.6): the turn activity slice, from the sidecar's
//     `plan` and `verifier` events, the store's approval cards and the sidecar's held commands
// Where the app has no real number the field is null and the monitor says "unknown" with the
// reason. Usage is null until the model server reports it for this turn; it is never estimated.
// =====================================================================
import {
  planStepStates,
  tokensPerSecond,
  type ApprovalRow,
  type RunRow,
  type ToolRow,
  type TurnActivity,
  type TurnPhase,
  type UsageReport,
  type VerifierRow,
} from "../shell/slices/turnActivity";
import type { DaemonsView } from "../daemons/api";

export type ProviderClass = "local" | "gateway" | "demo" | "unknown";

/** HUP-S1.9 — one of the agent sidecar's worker processes, as Rust reports it (hermes_workers). */
export interface WorkerRow {
  /** "toolchain" | "browser". */
  kind: string;
  /** "starting" | "running" | "restarting" | "failed" | "stopped" | "off" | "not_built". */
  state: string;
  healthy: boolean | null;
  pid: number | null;
  restarts: number | null;
  lastExit: string | null;
  lastError: string | null;
  runningSinceMs: number | null;
  detail: string | null;
}

/** HUP-S5.3: one decide() backend's metering, as Rust reports it (hermes_decide_stats). */
export interface DecideBackendRow {
  /** "local" | "jev". */
  backend: string;
  decisions: number;
  errors: number;
  p50Ms: number | null;
  p95Ms: number | null;
  meanConfidence: number | null;
  /** Bytes sent off the machine (Jev request bodies). */
  egressBytes: number;
  tasksAttempted: number;
  tasksSucceeded: number;
  /** Succeeded / attempted in basis points; null before any task outcome. */
  taskSuccessBps: number | null;
}

/** HUP-S5.3: the decide() metering report Rust returns; null = Hermes is not running. */
export interface DecideMetering {
  jevEnabled: boolean;
  jevOrigins: number;
  jevNonWeb: boolean;
  logging: boolean;
  backends: DecideBackendRow[];
}

/** HUP-S5.3: what the monitor shows for decide(). rows null = could not be read. */
export interface DecideSection {
  rows: DecideBackendRow[] | null;
  jevEnabled: boolean;
  note: string;
}

export interface MonitorSnapshot {
  /** When the main window built this snapshot (ms since epoch). */
  at: number;
  model: { label: string; id: string | null };
  provider: { kind: string; class: ProviderClass; label: string };
  /** The tier in effect, or null when the hardware probe has not answered. */
  tier: string | null;
  context: { usedTokens: number | null; windowTokens: number | null; usedNote: string; windowNote: string };
  turn: {
    state: TurnActivity["state"];
    phase: TurnPhase | null;
    currentTool: string | null;
    step: number | null;
    startedAt: number | null;
    endedAt: number | null;
    outcome: TurnActivity["outcome"];
    tools: ToolRow[];
    /** HUP-S2.2: this turn's command runs. Absent from an older sender = none. */
    runs?: RunRow[];
    /** HUP-S7.6: a workflow run's steps with their verifier state; null = a chat turn (no plan).
     *  Absent from an older sender = no plan. */
    plan?: { step: string; state: "not checked yet" | "passed" | "failed" }[] | null;
    /** HUP-S7.6: approvals asked this turn. Absent from an older sender = none. */
    approvals?: ApprovalRow[];
    /** HUP-S7.6: verifier verdicts this turn. Absent from an older sender = none. */
    verifiers?: VerifierRow[];
    why: string;
  };
  /** HUP-S7.6: generation speed of the latest model call. Absent from an older sender = unknown. */
  speed?: { tokensPerSecond: number | null; note: string };
  spend: { amount: number | null; unit: string; note: string };
  /** HUP-S1.9: null rows = could not be read (unknown); [] = Hermes is not running. */
  workers: { rows: WorkerRow[] | null; note: string };
  /** HUP-S10.3 — scheduled daemons. */
  daemons: DaemonsSection;
  /** HUP-S5.3: decide() metering per backend. Absent from an older sender = not shown. */
  decide?: DecideSection;
}

/** HUP-S10.3 — one daemon as the monitor shows it. */
export interface DaemonRow {
  id: string;
  name: string;
  status: string;
  paused: boolean;
  running: boolean;
  runsToday: number;
  maxRuns: number;
  /** Tokens charged today: measured from the model server when every call of a run reported
   *  usage, otherwise estimated (characters / 4). */
  tokensToday: number;
  /** HUP-S10.3: "measured" | "estimated" for the last run; null before any run (absent from an
   *  older sender = unknown). */
  lastTokenSource?: string | null;
  maxTokens: number;
  nextRunAt: number | null;
  lastOutcome: string | null;
  lastNote: string | null;
}

export interface DaemonsSection {
  allPaused: boolean;
  /** Why the runner is holding runs (e.g. the local model is not serving), or null. */
  blocked: string | null;
  error: string | null;
  rows: DaemonRow[];
}

export const NO_DAEMONS: DaemonsSection = { allPaused: false, blocked: null, error: null, rows: [] };

/** Build the monitor's daemon section from the list Rust reported and the runner's state. */
export function daemonsSection(view: DaemonsView | null, runner: { blocked: string | null; error: string | null }): DaemonsSection {
  return {
    allPaused: view?.allPaused ?? false,
    blocked: runner.blocked,
    error: runner.error,
    rows: (view?.daemons ?? []).map((d) => ({
      id: d.id,
      name: d.name,
      status: d.status,
      paused: d.paused,
      running: d.running,
      runsToday: d.runsToday,
      maxRuns: d.budget.maxRunsPerDay,
      tokensToday: d.tokensToday,
      maxTokens: d.budget.maxTokensPerDay,
      nextRunAt: d.nextRunMs,
      lastOutcome: d.lastOutcome,
      lastNote: d.lastNote,
      lastTokenSource: d.lastTokenSource ?? null,
    })),
  };
}

export interface MonitorInputs {
  activity: TurnActivity;
  /** The active provider's kind and label (used when no turn is running). */
  providerKind: string;
  providerLabel: string;
  modelLabel: string;
  modelId: string | null;
  tier: string | null;
  /** The local server's context window from Rust, or null when it could not be read. */
  localCtxTokens: number | null;
  /** HUP-S1.9: the sidecar's worker processes; null or absent = not read. */
  workers?: WorkerRow[] | null;
  /** HUP-S5.3: the decide() metering. undefined = not read, null = Hermes is not running, "error" = the read failed. */
  decide?: DecideMetering | null | "error";
  now: number;
  /** HUP-S10.3 — the daemon section; absent = no daemons. */
  daemons?: DaemonsSection;
}

/** `ChatProvider.kind` → where its inference runs. */
export function providerClass(kind: string): ProviderClass {
  if (kind === "local" || kind === "sidecar") return "local";
  if (kind === "real" || kind === "agent") return "gateway";
  if (kind === "demo") return "demo";
  return "unknown";
}

export function spendFor(kind: string): MonitorSnapshot["spend"] {
  switch (providerClass(kind)) {
    case "local":
      return { amount: 0, unit: "SALT", note: "local model on this machine, no charge" };
    case "demo":
      return { amount: 0, unit: "SALT", note: "built-in demo agent, no model call" };
    case "gateway":
      return { amount: null, unit: "SALT", note: "gateway spend is not metered in the app yet" };
    default:
      return { amount: null, unit: "SALT", note: "this provider does not report spend" };
  }
}

/** The context used by the latest model call: prompt plus completion, as the server reported. */
export function usedFrom(usage: UsageReport | null | undefined): { usedTokens: number | null; usedNote: string } {
  if (!usage) return { usedTokens: null, usedNote: "the model server has not reported token usage for this turn" };
  return {
    usedTokens: usage.promptTokens + usage.completionTokens,
    usedNote: `measured: the model server's report for its last call (${usage.promptTokens} prompt + ${usage.completionTokens} written)`,
  };
}

export function contextFor(kind: string, localCtxTokens: number | null, usage: UsageReport | null = null): MonitorSnapshot["context"] {
  const { usedTokens, usedNote } = usedFrom(usage);
  if (providerClass(kind) === "local") {
    return localCtxTokens !== null
      ? { usedTokens, windowTokens: localCtxTokens, usedNote, windowNote: "local llama-server context window" }
      : { usedTokens, windowTokens: null, usedNote, windowNote: "the local context window could not be read" };
  }
  if (providerClass(kind) === "gateway") {
    return { usedTokens, windowTokens: null, usedNote, windowNote: "the gateway does not report its context window" };
  }
  return { usedTokens, windowTokens: null, usedNote, windowNote: "no model context for this provider" };
}

/** HUP-S7.6 — tokens per second of the latest model call, or why it is unknown. */
export function speedFor(usage: UsageReport | null | undefined): NonNullable<MonitorSnapshot["speed"]> {
  const tps = tokensPerSecond(usage);
  if (tps !== null) return { tokensPerSecond: tps, note: "measured: written tokens over the model server's generation time" };
  if (usage) return { tokensPerSecond: null, note: "the model server reported tokens but not its generation time" };
  return { tokensPerSecond: null, note: "the model server has not reported a generation time for this turn" };
}

const WORKER_STATE_TEXT: Record<string, string> = {
  starting: "starting",
  running: "running",
  restarting: "restarting",
  failed: "failed",
  stopped: "stopped",
  off: "off",
  not_built: "not built yet",
};

/** HUP-S1.9 — one worker's state in words, with its restart history and how it last ended. */
export function workerLine(w: WorkerRow): string {
  let line = WORKER_STATE_TEXT[w.state] ?? w.state;
  if (w.state === "running" && w.healthy === false) line += ", not answering health checks";
  const n = w.restarts ?? 0;
  const why = [w.lastExit, w.state === "failed" ? w.lastError : null].filter((x): x is string => !!x);
  if (n > 0) {
    line += `, restarted ${n} ${n === 1 ? "time" : "times"}`;
    if (why.length) line += ` (last exit: ${why.join("; ")})`;
  } else if (w.state === "failed" && why.length) {
    line += ` (${why.join("; ")})`;
  }
  if (w.detail && (w.state === "not_built" || w.state === "off")) line += ` (${w.detail})`;
  return line;
}

export function workersFor(rows: WorkerRow[] | null | undefined): MonitorSnapshot["workers"] {
  if (rows === null || rows === undefined) {
    return { rows: null, note: "worker status could not be read" };
  }
  if (rows.length === 0) {
    return { rows: [], note: "Hermes is not running, so no worker processes are running" };
  }
  return { rows, note: "each worker is a separate process; a crash restarts it without stopping Hermes" };
}

/** HUP-S5.3: the monitor's decide() section from what Rust reported. */
export function decideFor(m: DecideMetering | null | "error" | undefined): DecideSection {
  if (m === undefined || m === "error") {
    return { rows: null, jevEnabled: false, note: "decision metering could not be read" };
  }
  if (m === null) {
    return { rows: [], jevEnabled: false, note: "Hermes is not running, so no decisions are being made" };
  }
  const where = m.jevEnabled
    ? `local model on this machine, and Jev (TypeSafe) for ${m.jevOrigins} origin${m.jevOrigins === 1 ? "" : "s"}, which sends the page snapshot off this machine`
    : "local model on this machine only; Jev is off";
  const rows = m.backends.filter((b) => b.decisions > 0 || b.tasksAttempted > 0);
  return {
    rows,
    jevEnabled: m.jevEnabled,
    note: rows.length === 0 ? `No decisions yet (${where}).` : `Measured by the sidecar for this session of Hermes (${where}).`,
  };
}

/** HUP-S5.3: one backend's numbers in words. Only measured numbers are shown. */
export function decideBackendLine(b: DecideBackendRow): string {
  const parts = [`${b.decisions} ${b.decisions === 1 ? "decision" : "decisions"}`];
  if (b.errors > 0) parts.push(`${b.errors} failed`);
  if (b.p50Ms !== null && b.p95Ms !== null) parts.push(`median ${b.p50Ms} ms, p95 ${b.p95Ms} ms`);
  if (b.tasksAttempted > 0) {
    const pct = b.taskSuccessBps !== null ? ` (${(b.taskSuccessBps / 100).toFixed(1)}%)` : "";
    parts.push(`tasks ${b.tasksSucceeded} of ${b.tasksAttempted} succeeded${pct}`);
  } else {
    parts.push("no task outcomes recorded");
  }
  if (b.meanConfidence !== null) parts.push(`mean confidence ${b.meanConfidence.toFixed(2)}`);
  if (b.egressBytes > 0) parts.push(`${b.egressBytes.toLocaleString("en-US")} bytes sent off this machine`);
  return parts.join(", ");
}

/** The one-line "why am I waiting" answer for the current turn. */
export function waitingReason(a: Pick<TurnActivity, "state" | "phase" | "currentTool">): string {
  if (a.state === "idle") return "Idle: nothing is running.";
  if (a.state === "stopping") return "Stopping: ending the turn.";
  switch (a.phase) {
    case "tool":
      return a.currentTool
        ? `Running a tool: ${a.currentTool}. A tool that changes anything waits for your approval in the main window.`
        : "Running a tool.";
    case "streaming":
      return "Writing the answer.";
    default:
      return "Waiting on the model: it is reading the conversation and deciding the next step.";
  }
}

export function formatElapsed(startedAt: number | null, now: number): string {
  if (startedAt === null) return "not started";
  const total = Math.max(0, Math.floor((now - startedAt) / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  if (h > 0) return `${h}h ${two(m)}m ${two(s)}s`;
  if (m > 0) return `${m}m ${two(s)}s`;
  return `${s}s`;
}

export function buildMonitorSnapshot(i: MonitorInputs): MonitorSnapshot {
  const a = i.activity;
  const running = a.state !== "idle" && a.providerKind !== null;
  const kind = running ? (a.providerKind as string) : i.providerKind;
  const label = running ? (a.providerLabel ?? i.providerLabel) : i.providerLabel;
  return {
    at: i.now,
    model: { label: i.modelLabel, id: i.modelId },
    provider: { kind, class: providerClass(kind), label },
    tier: i.tier,
    context: contextFor(kind, i.localCtxTokens, a.usage ?? null),
    speed: speedFor(a.usage),
    turn: {
      state: a.state,
      phase: a.phase,
      currentTool: a.currentTool,
      step: a.step,
      startedAt: a.startedAt,
      endedAt: a.endedAt,
      outcome: a.outcome,
      tools: a.tools,
      runs: a.runs ?? [],
      plan: a.plan ? planStepStates(a.plan, a.verifiers ?? []) : null,
      approvals: a.approvals ?? [],
      verifiers: a.verifiers ?? [],
      why: waitingReason(a),
    },
    spend: spendFor(kind),
    daemons: i.daemons ?? NO_DAEMONS,
    workers: workersFor(i.workers),
    decide: decideFor(i.decide),
  };
}

// ---- validation (the pop-out side checks every snapshot it receives) ----

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const numOrNull = (v: unknown) => v === null || (typeof v === "number" && Number.isFinite(v));
const strOrNull = (v: unknown) => v === null || typeof v === "string";

function isToolRow(v: unknown): v is ToolRow {
  return (
    isObj(v) &&
    typeof v.id === "string" &&
    typeof v.name === "string" &&
    ["running", "done", "failed", "abandoned"].includes(v.state as string) &&
    typeof v.startedAt === "number" &&
    numOrNull(v.endedAt)
  );
}

const boolOrNull = (v: unknown) => v === null || typeof v === "boolean";

function isRunRow(v: unknown): v is RunRow {
  return (
    isObj(v) &&
    typeof v.callId === "string" &&
    typeof v.tool === "string" &&
    typeof v.status === "string" &&
    typeof v.summary === "string" &&
    numOrNull(v.exitCode) &&
    numOrNull(v.durationMs) &&
    typeof v.timedOut === "boolean" &&
    strOrNull(v.sandbox) &&
    typeof v.at === "number"
  );
}

function isApprovalRow(v: unknown): v is ApprovalRow {
  return (
    isObj(v) &&
    typeof v.callId === "string" &&
    typeof v.tool === "string" &&
    ["pending", "approved", "declined", "failed"].includes(v.state as string) &&
    typeof v.at === "number"
  );
}

function isVerifierRow(v: unknown): v is VerifierRow {
  return isObj(v) && typeof v.step === "string" && typeof v.name === "string" && typeof v.passed === "boolean" && typeof v.detail === "string" && typeof v.at === "number";
}

function isPlanRow(v: unknown): boolean {
  return isObj(v) && typeof v.step === "string" && ["not checked yet", "passed", "failed"].includes(v.state as string);
}

function isWorkerRow(v: unknown): v is WorkerRow {
  return (
    isObj(v) &&
    typeof v.kind === "string" &&
    typeof v.state === "string" &&
    boolOrNull(v.healthy) &&
    numOrNull(v.pid) &&
    numOrNull(v.restarts) &&
    strOrNull(v.lastExit) &&
    strOrNull(v.lastError) &&
    numOrNull(v.runningSinceMs) &&
    strOrNull(v.detail)
  );
}

function isDecideRow(v: unknown): v is DecideBackendRow {
  return (
    isObj(v) &&
    typeof v.backend === "string" &&
    num(v.decisions) &&
    num(v.errors) &&
    numOrNull(v.p50Ms) &&
    numOrNull(v.p95Ms) &&
    numOrNull(v.meanConfidence) &&
    num(v.egressBytes) &&
    num(v.tasksAttempted) &&
    num(v.tasksSucceeded) &&
    numOrNull(v.taskSuccessBps)
  );
}

function isDecideSection(v: unknown): boolean {
  return (
    v === undefined ||
    (isObj(v) &&
      typeof v.note === "string" &&
      typeof v.jevEnabled === "boolean" &&
      (v.rows === null || (Array.isArray(v.rows) && v.rows.every(isDecideRow))))
  );
}

export function isMonitorSnapshot(v: unknown): v is MonitorSnapshot {
  if (!isObj(v) || typeof v.at !== "number") return false;
  const { model, provider, context, turn, spend } = v;
  if (!isObj(model) || typeof model.label !== "string" || !strOrNull(model.id)) return false;
  if (!isObj(provider) || typeof provider.kind !== "string" || typeof provider.label !== "string") return false;
  if (!["local", "gateway", "demo", "unknown"].includes(provider.class as string)) return false;
  if (!strOrNull(v.tier)) return false;
  if (!isObj(context) || !numOrNull(context.usedTokens) || !numOrNull(context.windowTokens)) return false;
  if (typeof context.usedNote !== "string" || typeof context.windowNote !== "string") return false;
  if (!isObj(turn) || !["idle", "running", "stopping"].includes(turn.state as string)) return false;
  if (!(turn.phase === null || ["thinking", "streaming", "tool"].includes(turn.phase as string))) return false;
  if (!strOrNull(turn.currentTool) || !numOrNull(turn.step) || !numOrNull(turn.startedAt) || !numOrNull(turn.endedAt)) return false;
  if (!(turn.outcome === null || ["answered", "failed", "stopped"].includes(turn.outcome as string))) return false;
  if (!Array.isArray(turn.tools) || !turn.tools.every(isToolRow) || typeof turn.why !== "string") return false;
  if (!(turn.runs === undefined || (Array.isArray(turn.runs) && turn.runs.every(isRunRow)))) return false;
  if (!(turn.plan === undefined || turn.plan === null || (Array.isArray(turn.plan) && turn.plan.every(isPlanRow)))) return false;
  if (!(turn.approvals === undefined || (Array.isArray(turn.approvals) && turn.approvals.every(isApprovalRow)))) return false;
  if (!(turn.verifiers === undefined || (Array.isArray(turn.verifiers) && turn.verifiers.every(isVerifierRow)))) return false;
  const { speed } = v;
  if (!(speed === undefined || (isObj(speed) && numOrNull(speed.tokensPerSecond) && typeof speed.note === "string"))) return false;
  if (!isObj(spend) || !numOrNull(spend.amount) || typeof spend.unit !== "string" || typeof spend.note !== "string") return false;
  const { workers } = v;
  if (!isObj(workers) || typeof workers.note !== "string") return false;
  if (!(workers.rows === null || (Array.isArray(workers.rows) && workers.rows.every(isWorkerRow)))) return false;
  if (!isDecideSection(v.decide)) return false;
  return isDaemonsSection(v.daemons);
}

const num = (v: unknown) => typeof v === "number" && Number.isFinite(v);

function isDaemonRow(v: unknown): v is DaemonRow {
  return (
    isObj(v) &&
    typeof v.id === "string" &&
    typeof v.name === "string" &&
    typeof v.status === "string" &&
    typeof v.paused === "boolean" &&
    typeof v.running === "boolean" &&
    num(v.runsToday) &&
    num(v.maxRuns) &&
    num(v.tokensToday) &&
    num(v.maxTokens) &&
    numOrNull(v.nextRunAt) &&
    strOrNull(v.lastOutcome) &&
    strOrNull(v.lastNote) &&
    (v.lastTokenSource === undefined || strOrNull(v.lastTokenSource))
  );
}

function isDaemonsSection(v: unknown): v is DaemonsSection {
  return isObj(v) && typeof v.allPaused === "boolean" && strOrNull(v.blocked) && strOrNull(v.error) && Array.isArray(v.rows) && v.rows.every(isDaemonRow);
}
