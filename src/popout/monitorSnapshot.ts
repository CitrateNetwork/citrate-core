// =====================================================================
// citrate-core — Activity monitor snapshot (HUP-S7.6, US-7.4)
//
// Data sources (Rule 7):
//   - turn: the turn activity slice, written by the real send path (store.sendChat)
//   - model: the ModelRouter's active choice (same resolution as the chat header)
//   - tier: the hardware tier report (bridge.tier.recommend, a local probe)
//   - context window: the local llama-server's --ctx-size, read from Rust (popout_monitor_facts)
//   - spend: local inference is free; gateway metering is not wired into the app yet
//   - workers (HUP-S1.9): the agent sidecar's worker processes, read from Rust (hermes_workers →
//     the sidecar's GET /workers, i.e. its own process supervisor)
// Where the app has no real number the field is null and the monitor says "unknown" with the
// reason. Token usage is null for every provider today: no provider reports usage to the app.
// =====================================================================
import { type ToolRow, type TurnActivity, type TurnPhase } from "../shell/slices/turnActivity";

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
    why: string;
  };
  spend: { amount: number | null; unit: string; note: string };
  /** HUP-S1.9: null rows = could not be read (unknown); [] = Hermes is not running. */
  workers: { rows: WorkerRow[] | null; note: string };
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
  now: number;
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

export function contextFor(kind: string, localCtxTokens: number | null): MonitorSnapshot["context"] {
  const usedNote = "the model server does not report token usage to the app yet";
  if (providerClass(kind) === "local") {
    return localCtxTokens !== null
      ? { usedTokens: null, windowTokens: localCtxTokens, usedNote, windowNote: "local llama-server context window" }
      : { usedTokens: null, windowTokens: null, usedNote, windowNote: "the local context window could not be read" };
  }
  if (providerClass(kind) === "gateway") {
    return { usedTokens: null, windowTokens: null, usedNote, windowNote: "the gateway does not report its context window" };
  }
  return { usedTokens: null, windowTokens: null, usedNote, windowNote: "no model context for this provider" };
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
    context: contextFor(kind, i.localCtxTokens),
    turn: {
      state: a.state,
      phase: a.phase,
      currentTool: a.currentTool,
      step: a.step,
      startedAt: a.startedAt,
      endedAt: a.endedAt,
      outcome: a.outcome,
      tools: a.tools,
      why: waitingReason(a),
    },
    spend: spendFor(kind),
    workers: workersFor(i.workers),
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
  if (!isObj(spend) || !numOrNull(spend.amount) || typeof spend.unit !== "string" || typeof spend.note !== "string") return false;
  const { workers } = v;
  if (!isObj(workers) || typeof workers.note !== "string") return false;
  if (!(workers.rows === null || (Array.isArray(workers.rows) && workers.rows.every(isWorkerRow)))) return false;
  return true;
}
