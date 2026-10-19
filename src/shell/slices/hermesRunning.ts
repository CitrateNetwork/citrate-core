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
// Hermes reads the saved values AT START, so every app path that starts or restarts Hermes runs
// inside withHermesStart: it marks a start in flight (the writers of these two settings hold their
// writes until it ends), takes the saved values BEFORE the start, and the caller commits that
// snapshot only once the start is confirmed. Paths: startRuntime (the Agent page's Start, the
// toolchain card's restart and the Controls restart via restartHermesConfirmed) and the terminal
// switch's automatic restart (store.setHermesTerminal and the launch sync).
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

/** What the running Hermes started with (null = not known yet this session), and how many starts
 *  or restarts are in flight (the writers hold their writes while it is above zero). */
export interface HermesRunningState {
  webSearch: boolean | null;
  learn: boolean | null;
  starting: number;
}

export type HermesSavedSnapshot = Pick<HermesRunningState, HermesStartSetting>;

export const hermesRunningSlice = createSlice<HermesRunningState>({ webSearch: null, learn: null, starting: 0 });

/** True while Hermes is starting or restarting: writes to its start-time settings wait. */
export function hermesStarting(): boolean {
  return hermesRunningSlice.get().starting > 0;
}

/** Said by a writer that holds its write during a start. */
export const HELD_DURING_START = "Hermes is starting. This setting can be changed once it is up.";

/** Record the value Hermes is running with, if not already known: call with the SAVED value when
 *  first reading it, and before every write with the value saved just before that write. */
export function noteRunning(key: HermesStartSetting, saved: boolean): void {
  if (hermesRunningSlice.get()[key] === null) hermesRunningSlice.set({ [key]: saved } as Partial<HermesRunningState>);
}

/** A start was confirmed: Hermes runs with the snapshot taken before it started. */
export function confirmRunning(snapshot: Partial<HermesSavedSnapshot>): void {
  hermesRunningSlice.set(snapshot);
}

/** The saved values now. A value that cannot be read (no desktop app, core error) is unknown,
 *  never a guess. Never throws. */
export async function readSavedForHermes(): Promise<HermesSavedSnapshot> {
  const [webSearch, learn] = await Promise.all([
    invoke<HermesWebStatus>("hermes_web_settings_get").then(
      (st) => st.settings.searchEnabled,
      () => null,
    ),
    bridge.flRounds.trajectoryStatus().then(
      (st) => st.settings.enabled,
      () => null,
    ),
  ]);
  return { webSearch, learn };
}

/**
 * Run a Hermes start or restart: mark it in flight first (so no writer changes the saved values
 * from here on), snapshot the saved values (what Hermes will read when it starts), then run `f`
 * with the snapshot. `f` commits it with confirmRunning only once the start is confirmed.
 * `read` is the snapshot source (the app's saved settings by default).
 */
export async function withHermesStart<T>(f: (snapshot: HermesSavedSnapshot) => Promise<T>, read: () => Promise<HermesSavedSnapshot> = readSavedForHermes): Promise<T> {
  hermesRunningSlice.set((st) => ({ starting: st.starting + 1 }));
  try {
    const snapshot = await read();
    return await f(snapshot);
  } finally {
    hermesRunningSlice.set((st) => ({ starting: Math.max(0, st.starting - 1) }));
  }
}
