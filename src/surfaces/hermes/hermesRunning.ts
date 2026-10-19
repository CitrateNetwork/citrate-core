// =====================================================================
// K2 (0.5.2): what the running Hermes started with, for the settings it reads only at start.
//
// Web search (hermes_web.rs) and learning from sessions (fl_trajectories.rs) are saved for the next
// Hermes start; core reports `appliesOnRestart` but not the running sidecar's own values, and its
// status carries no start time. So the app keeps, for this session, the value Hermes is running
// with: the saved value the first time the app sees it, or the saved value just before the first
// write (every writer of these two settings records it here before writing). A confirmed restart
// (restartHermesConfirmed) replaces it with what is saved now. It is app-wide state, not a
// component's, so leaving and reopening a panel does not lose a pending restart.
//
// Limits: the app does not see restarts made outside Hermes › Controls (for example Stop and
// Start on the Agent page), so the notice can outlive such a restart until a restart from Controls
// confirms it. That errs toward warning. A sidecar report of its effective settings would replace
// this.
// =====================================================================
import { createSlice } from "../../shell/slices/createSlice";

export type HermesStartSetting = "webSearch" | "learn";

/** null = not known yet this session. */
export type HermesRunningState = Record<HermesStartSetting, boolean | null>;

export const hermesRunningSlice = createSlice<HermesRunningState>({ webSearch: null, learn: null });

/** Record the value Hermes is running with, if not already known: call with the SAVED value when
 *  first reading it, and before every write with the value saved just before that write. */
export function noteRunning(key: HermesStartSetting, saved: boolean): void {
  if (hermesRunningSlice.get()[key] === null) hermesRunningSlice.set({ [key]: saved } as Partial<HermesRunningState>);
}

/** A restart was confirmed: Hermes now runs with what is saved. */
export function confirmRunning(saved: Partial<HermesRunningState>): void {
  hermesRunningSlice.set(saved);
}
