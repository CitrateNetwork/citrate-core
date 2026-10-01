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
//     the runner's state (why runs are held). Daemon tokens are estimated (characters / 4).
// Where the app has no real number the field is null and the monitor says "unknown" with the
// reason. Token usage is null for every provider today: no provider reports usage to the app.
// =====================================================================
import { type ToolRow, type TurnActivity, type TurnPhase } from "../shell/slices/turnActivity";
import type { DaemonsView } from "../daemons/api";

export type ProviderClass = "local" | "gateway" | "demo" | "unknown";

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
  /** HUP-S10.3 — scheduled daemons. */
  daemons: DaemonsSection;
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
  /** Estimated (characters / 4): no provider reports usage yet. */
  tokensToday: number;
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
    daemons: i.daemons ?? NO_DAEMONS,
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
    strOrNull(v.lastNote)
  );
}

function isDaemonsSection(v: unknown): v is DaemonsSection {
  return isObj(v) && typeof v.allPaused === "boolean" && strOrNull(v.blocked) && strOrNull(v.error) && Array.isArray(v.rows) && v.rows.every(isDaemonRow);
}
