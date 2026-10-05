// =====================================================================
// citrate-core — turn activity slice (HUP-S7.6, US-7.4)
//
// The live record of the agent turn in progress, for the Activity monitor pop-out. It is written
// only from the real send path (`store.sendChat` callbacks and the provider's step reports), so
// every value here traces to something that happened. Nothing is estimated: a turn with no step
// reports keeps `step: null`, and the monitor shows that as unknown (Rule 1).
// =====================================================================
import { createSlice } from "./createSlice";
import type { ChatStatus, TurnActivityEvent } from "../../agent/harness";

export type TurnPhase = "thinking" | "streaming" | "tool";

export interface ToolRow {
  id: string;
  name: string;
  /** `abandoned`: the turn was stopped while this call was still running. */
  state: "running" | "done" | "failed" | "abandoned";
  startedAt: number;
  endedAt: number | null;
}

/** HUP-S2.2 (US-2.2 AC3) — one command run (shell_run or a toolchain tool), from its tool result. */
export interface RunRow {
  callId: string;
  tool: string;
  /** The run's status as the sidecar reported it ("completed", "timed_out", "refused", "failed",
   *  "not_installed"), or "declined" when the member said no. */
  status: string;
  summary: string;
  exitCode: number | null;
  durationMs: number | null;
  timedOut: boolean;
  /** The OS sandbox summary the run reported, or null when it reported none. */
  sandbox: string | null;
  /** When core received the result (ms since epoch). */
  at: number;
}

/** HUP-S7.6 (US-7.4 AC1) — the model server's report for the latest model call of the turn. */
export interface UsageReport {
  promptTokens: number;
  completionTokens: number;
  /** Generation time of the completion in ms, or null when the server does not report it. */
  generationMs: number | null;
  /** Model calls this turn that reported usage. */
  calls: number;
}

/** HUP-S7.6 — one approval this turn asked the member for, and how it was decided. */
export interface ApprovalRow {
  callId: string;
  tool: string;
  state: "pending" | "approved" | "declined" | "failed";
  at: number;
}

/** HUP-S7.6 — one verifier verdict on a workflow step attempt. */
export interface VerifierRow {
  step: string;
  name: string;
  passed: boolean;
  detail: string;
  at: number;
}

export interface TurnActivity {
  state: "idle" | "running" | "stopping";
  /** The provider the turn started on (`ChatProvider.kind`), kept for the whole turn. */
  providerKind: string | null;
  providerLabel: string | null;
  phase: TurnPhase | null;
  currentTool: string | null;
  /** The highest step the loop reported, or null when the provider reports none. */
  step: number | null;
  startedAt: number | null;
  endedAt: number | null;
  tools: ToolRow[];
  /** HUP-S2.2: the command runs of this turn (most recent last). */
  runs: RunRow[];
  outcome: "answered" | "failed" | "stopped" | null;
  /** HUP-S7.6: the latest model call's reported usage, or null when none was reported. */
  usage?: UsageReport | null;
  /** HUP-S7.6: a workflow run's step ids, or a chat turn's plan rows (one per model step that
   *  asked for tools); null before any plan is reported. */
  plan?: string[] | null;
  /** Where `plan` came from: a workflow's `plan` event, or the chat turn's tool calls. Absent from
   *  an older sender = workflow. */
  planSource?: "workflow" | "chat";
  /** HUP-S7.6: approvals asked this turn (most recent last). */
  approvals?: ApprovalRow[];
  /** HUP-S7.6: verifier verdicts this turn (most recent last). */
  verifiers?: VerifierRow[];
}

/** The monitor lists at most this many tool calls of the current turn (the most recent ones). */
export const MAX_TOOL_ROWS = 20;
/** The monitor lists at most this many command runs of the current turn (the most recent ones). */
export const MAX_RUN_ROWS = 20;
/** At most this many approvals, verifier verdicts and plan steps are kept per turn. */
export const MAX_APPROVAL_ROWS = 20;
export const MAX_VERIFIER_ROWS = 40;
export const MAX_PLAN_STEPS = 50;
/** Longest verifier detail kept, in characters. */
export const MAX_VERIFIER_DETAIL = 240;

export const IDLE_ACTIVITY: TurnActivity = {
  state: "idle",
  providerKind: null,
  providerLabel: null,
  phase: null,
  currentTool: null,
  step: null,
  startedAt: null,
  endedAt: null,
  tools: [],
  runs: [],
  outcome: null,
  usage: null,
  plan: null,
  planSource: "workflow",
  approvals: [],
  verifiers: [],
};

export const turnActivity = createSlice<TurnActivity>(IDLE_ACTIVITY);

const live = (): boolean => turnActivity.get().state !== "idle";

export function beginTurn(providerKind: string, providerLabel: string, now: number = Date.now()): void {
  turnActivity.set({ ...IDLE_ACTIVITY, state: "running", providerKind, providerLabel, phase: "thinking", startedAt: now });
}

/** Record the provider's status. `done`/`error` are not phases; `endTurn` closes the turn. */
export function notePhase(status: ChatStatus): void {
  if (!live()) return;
  if (status === "thinking" || status === "streaming" || status === "tool") {
    turnActivity.set((s) => ({ phase: status, currentTool: status === "tool" ? s.currentTool : null }));
  }
}

export function noteStep(step: number): void {
  if (!live() || !Number.isFinite(step)) return;
  turnActivity.set((s) => ({ step: s.step === null ? step : Math.max(s.step, step) }));
}

export function toolStarted(id: string, name: string, now: number = Date.now()): void {
  if (!live()) return;
  turnActivity.set((s) => ({
    phase: "tool",
    currentTool: name,
    tools: s.tools.concat([{ id, name, state: "running", startedAt: now, endedAt: null }]).slice(-MAX_TOOL_ROWS),
  }));
}

export function toolFinished(id: string, ok: boolean, now: number = Date.now()): void {
  if (!live()) return;
  turnActivity.set((s) => {
    let hit = false;
    const tools = s.tools.map((t) => {
      if (hit || t.id !== id || t.state !== "running") return t;
      hit = true;
      return { ...t, state: ok ? ("done" as const) : ("failed" as const), endedAt: now };
    });
    return { tools, currentTool: tools.some((t) => t.state === "running") ? s.currentTool : null };
  });
}

export function markStopping(): void {
  if (turnActivity.get().state !== "running") return;
  turnActivity.set({ state: "stopping" });
}

export function endTurn(outcome: "answered" | "failed" | "stopped", now: number = Date.now()): void {
  if (!live()) return;
  turnActivity.set((s) => ({
    state: "idle",
    phase: null,
    currentTool: null,
    endedAt: now,
    outcome,
    tools: s.tools.map((t) => (t.state === "running" ? { ...t, state: "abandoned" as const, endedAt: now } : t)),
  }));
}

/**
 * HUP-S2.2 (US-2.2 AC3) — record one command run. Not gated on a live turn: a run reported while a
 * stopped turn drains still happened, so it is kept until the next turn starts.
 */
export function commandRan(ev: Extract<TurnActivityEvent, { kind: "command_run" }>, now: number = Date.now()): void {
  const row: RunRow = {
    callId: ev.callId,
    tool: ev.tool,
    status: ev.status,
    summary: ev.summary,
    exitCode: ev.exitCode,
    durationMs: ev.durationMs,
    timedOut: ev.timedOut,
    sandbox: ev.sandbox,
    at: now,
  };
  turnActivity.set((s) => ({ runs: (s.runs ?? []).concat([row]).slice(-MAX_RUN_ROWS) }));
}

/**
 * HUP-S7.6 (US-7.4 AC1) — record the token usage the model server reported for one model call.
 * Only reported numbers are kept; a call without a report leaves the last one in place and the
 * monitor still says where its number came from.
 */
export function usageReported(ev: Extract<TurnActivityEvent, { kind: "usage" }>): void {
  if (!live()) return;
  turnActivity.set((s) => ({
    usage: {
      promptTokens: ev.promptTokens,
      completionTokens: ev.completionTokens,
      generationMs: ev.generationMs,
      calls: (s.usage?.calls ?? 0) + 1,
    },
  }));
}

/** HUP-S7.6 — a workflow run's plan (step ids in order), or a chat turn's plan so far. */
export function planReported(steps: string[], source: "workflow" | "chat" = "workflow"): void {
  if (!live()) return;
  // A workflow's own plan is never replaced by chat rows from the same turn.
  if (source === "chat" && turnActivity.get().planSource === "workflow" && (turnActivity.get().plan ?? null) !== null) return;
  turnActivity.set({ plan: steps.slice(0, MAX_PLAN_STEPS), planSource: source });
}

/**
 * HUP-S7.6 — the state of each row of a chat turn's plan. A row is `done` once the model moved on
 * to a later step or the turn was answered; the latest row is `running` while the turn runs, and
 * `stopped` when the turn was stopped or failed before the model answered.
 */
export function chatPlanStates(
  plan: string[],
  turn: Pick<TurnActivity, "state" | "outcome">,
): { step: string; state: "running" | "done" | "stopped" }[] {
  return plan.map((step, i) => {
    if (i < plan.length - 1) return { step, state: "done" as const };
    if (turn.state !== "idle") return { step, state: "running" as const };
    return { step, state: turn.outcome === "answered" ? ("done" as const) : ("stopped" as const) };
  });
}

/** HUP-S7.6 — an approval was asked for (pending) or decided. A decision updates its pending row. */
export function approvalNoted(callId: string, tool: string, state: ApprovalRow["state"], now: number = Date.now()): void {
  if (!live()) return;
  turnActivity.set((s) => {
    const rows = s.approvals ?? [];
    const i = rows.findIndex((r) => r.callId === callId && r.state === "pending");
    if (state !== "pending" && i >= 0) {
      return { approvals: rows.map((r, j) => (j === i ? { ...r, state, at: now } : r)) };
    }
    return { approvals: rows.concat([{ callId, tool, state, at: now }]).slice(-MAX_APPROVAL_ROWS) };
  });
}

/** HUP-S7.6 — one verifier verdict on a workflow step attempt. */
export function verifierReported(ev: Extract<TurnActivityEvent, { kind: "verifier" }>, now: number = Date.now()): void {
  if (!live()) return;
  const row: VerifierRow = { step: ev.step, name: ev.name, passed: ev.passed, detail: ev.detail.slice(0, MAX_VERIFIER_DETAIL), at: now };
  turnActivity.set((s) => ({ verifiers: (s.verifiers ?? []).concat([row]).slice(-MAX_VERIFIER_ROWS) }));
}

/** HUP-S7.6 — the tokens per second of a usage report, or null when the server gave no time. */
export function tokensPerSecond(u: UsageReport | null | undefined): number | null {
  if (!u || u.generationMs === null || u.generationMs <= 0) return null;
  return Math.round((u.completionTokens / (u.generationMs / 1000)) * 10) / 10;
}

/** HUP-S7.6 — the state of each plan step from its latest verifier verdicts. */
export function planStepStates(plan: string[], verifiers: VerifierRow[]): { step: string; state: "not checked yet" | "passed" | "failed" }[] {
  return plan.map((step) => {
    const latest = new Map<string, boolean>();
    for (const v of verifiers) if (v.step === step) latest.set(v.name, v.passed);
    if (latest.size === 0) return { step, state: "not checked yet" as const };
    return { step, state: [...latest.values()].every(Boolean) ? ("passed" as const) : ("failed" as const) };
  });
}
