// =====================================================================
// citrate-core — turn activity slice (HUP-S7.6, US-7.4)
//
// The live record of the agent turn in progress, for the Activity monitor pop-out. It is written
// only from the real send path (`store.sendChat` callbacks and the provider's step reports), so
// every value here traces to something that happened. Nothing is estimated: a turn with no step
// reports keeps `step: null`, and the monitor shows that as unknown (Rule 1).
// =====================================================================
import { createSlice } from "./createSlice";
import type { ChatStatus } from "../../agent/harness";

export type TurnPhase = "thinking" | "streaming" | "tool";

export interface ToolRow {
  id: string;
  name: string;
  /** `abandoned`: the turn was stopped while this call was still running. */
  state: "running" | "done" | "failed" | "abandoned";
  startedAt: number;
  endedAt: number | null;
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
  outcome: "answered" | "failed" | "stopped" | null;
}

/** The monitor lists at most this many tool calls of the current turn (the most recent ones). */
export const MAX_TOOL_ROWS = 20;

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
  outcome: null,
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
