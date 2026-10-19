// =====================================================================
// K2 (0.5.2): what the running Hermes started with, for the settings it reads only at start.
//
// Web search (hermes_web.rs) and learning from sessions (fl_trajectories.rs) are saved for the next
// Hermes start; core reports `appliesOnRestart` but not the running sidecar's own values, and its
// status carries no start time. So the app keeps, for this session, the value Hermes is running
// with: the saved value the first time the app sees it, or the saved value just before the first
// write (every writer of these two settings records it here before writing). A confirmed restart
// replaces it with what is saved now. It is app-wide state, not a component's, so leaving and
// reopening a panel does not lose a pending restart.
//
// Every app path that restarts Hermes calls refreshHermesRunningFromSaved once the new Hermes is
// up: a Start confirmed running (startRuntime: the Agent page's Start, the toolchain card's restart
// and the Controls restart), and the terminal switch's automatic restart (store.setHermesTerminal
// and the launch sync, when core reports `restarted`).
//
// Limit: a restart core makes on its own (the supervisor restarting a crashed sidecar) is not
// reported to the app, so the notice can outlive it. That errs toward warning. A sidecar report of
// its effective settings would replace this.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import { invoke } from "../../bridge/tauri/invoke";
import type { HermesWebStatus } from "../../agent/webSearch";

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

/**
 * Hermes has just (re)started, so it read the saved settings: record those as what it runs with.
 * A value that cannot be read (no desktop app, core error) becomes unknown rather than a guess; the
 * next read or write fills it in. Never throws.
 */
export async function refreshHermesRunningFromSaved(): Promise<void> {
  const [web, learn] = await Promise.all([
    invoke<HermesWebStatus>("hermes_web_settings_get").then(
      (st) => st.settings.searchEnabled,
      () => null,
    ),
    bridge.flRounds.trajectoryStatus().then(
      (st) => st.settings.enabled,
      () => null,
    ),
  ]);
  hermesRunningSlice.set({ webSearch: web, learn });
}
