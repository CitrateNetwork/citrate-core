// =====================================================================
// citrate-core — daemons slice (HUP-S10.3)
//
// The daemon list as Rust last reported it (`daemons_list`), plus the runner's live state. The
// Daemons card, the Activity monitor and the `daemons.summary` widget query all read this.
// =====================================================================
import { createSlice } from "../shell/slices/createSlice";
import type { DaemonsView } from "./api";
import type { RunnerState } from "./runner";

export interface DaemonsState {
  view: DaemonsView | null;
  loaded: boolean;
  error: string | null;
  runner: RunnerState;
  /** The last full reply per daemon, this session only (the ledger keeps a one-line note). */
  replies: Record<string, string>;
}

export const daemonsSlice = createSlice<DaemonsState>({
  view: null,
  loaded: false,
  error: null,
  runner: { running: null, blockedReason: null, error: null },
  replies: {},
});

/** Counts for the `daemons.summary` widget query and the monitor. */
export function daemonsSummary(view: DaemonsView | null): { allPaused: boolean; total: number; running: number; paused: number; budgetUsedUp: number } {
  const ds = view?.daemons ?? [];
  return {
    allPaused: view?.allPaused ?? false,
    total: ds.length,
    running: ds.filter((d) => d.running).length,
    paused: ds.filter((d) => d.paused).length,
    budgetUsedUp: ds.filter((d) => d.status === "budget used up today").length,
  };
}
