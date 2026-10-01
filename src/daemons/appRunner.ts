// =====================================================================
// citrate-core — wires the daemon runner to the real app (HUP-S10.3)
//
// Desktop app only. The runner ticks every 30 s in the main window; runs go through
// `store.runDaemonTurn` (local model only, every effectful tool call needs the member's explicit
// decision). In the web preview nothing starts and the Daemons card says daemons need the app.
// =====================================================================
import { store } from "../shell/store";
import { daemonsApi, localOffsetMinutes, type DaemonInput } from "./api";
import { createDaemonRunner, type DaemonRunner } from "./runner";
import { daemonsSlice } from "./slice";

/** How often the runner asks Rust which daemons are due. */
export const DAEMON_TICK_MS = 30_000;
/** Longest one daemon run may take. PENDING OWNER SIGN-OFF (conservative placeholder). */
export const DAEMON_RUN_TIMEOUT_MS = 10 * 60_000;

let runner: DaemonRunner | null = null;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Re-read the list from Rust into the slice. */
export async function refreshDaemons(): Promise<void> {
  const api = daemonsApi();
  if (!api) {
    daemonsSlice.set({ view: null, loaded: true, error: null });
    return;
  }
  try {
    daemonsSlice.set({ view: await api.list(Date.now(), localOffsetMinutes()), loaded: true, error: null });
  } catch (e) {
    daemonsSlice.set({ loaded: true, error: message(e) });
  }
}

function publish(): void {
  if (!runner) return;
  const r = runner;
  daemonsSlice.set((s) => {
    const replies = { ...s.replies };
    for (const d of s.view?.daemons ?? []) {
      const reply = r.lastReply(d.id);
      if (reply !== null) replies[d.id] = reply;
    }
    return { runner: r.state(), replies };
  });
  void refreshDaemons();
}

/** Start the runner once (desktop app only). */
export function startDaemonRunner(): void {
  const api = daemonsApi();
  if (!api || runner) return;
  runner = createDaemonRunner({
    api,
    now: () => Date.now(),
    offsetMinutes: () => localOffsetMinutes(),
    canRun: () => store.daemonRunAvailability(),
    runTurn: (claim, signal, meter) => store.runDaemonTurn(claim, signal, meter),
    onChange: publish,
    runTimeoutMs: DAEMON_RUN_TIMEOUT_MS,
  });
  void refreshDaemons();
  const tick = () => void runner?.tick().catch((e) => daemonsSlice.set({ error: message(e) }));
  setInterval(tick, DAEMON_TICK_MS);
}

async function act(f: () => Promise<unknown>): Promise<string | null> {
  try {
    await f();
    await refreshDaemons();
    return null;
  } catch (e) {
    return message(e);
  }
}

/** Create or edit a daemon; resolves with an error message, or null on success. */
export function saveDaemon(input: DaemonInput): Promise<string | null> {
  const api = daemonsApi();
  if (!api) return Promise.resolve("daemons need the desktop app");
  return act(() => api.save(input, Date.now(), localOffsetMinutes()));
}

/** Pause (stopping a run in flight) or resume one daemon. */
export function setDaemonPaused(id: string, paused: boolean): Promise<string | null> {
  const api = daemonsApi();
  if (!api) return Promise.resolve("daemons need the desktop app");
  if (paused && runner) {
    const r = runner;
    return act(() => r.pause(id));
  }
  return act(() => api.setPaused(id, paused, Date.now()));
}

/** Pause (stopping any run in flight) or resume every daemon. */
export function setAllDaemonsPaused(paused: boolean): Promise<string | null> {
  const api = daemonsApi();
  if (!api) return Promise.resolve("daemons need the desktop app");
  return act(async () => {
    await api.setAllPaused(paused, Date.now());
    if (paused) runner?.stopRunning();
  });
}

export function deleteDaemon(id: string): Promise<string | null> {
  const api = daemonsApi();
  if (!api) return Promise.resolve("daemons need the desktop app");
  return act(async () => {
    if (runner?.state().running?.daemonId === id) await runner.pause(id);
    await api.remove(id);
  });
}

/** Stop the daemon run in flight (the Activity monitor's Stop for a daemon run). */
export function stopDaemonRun(): void {
  runner?.stopRunning();
}
