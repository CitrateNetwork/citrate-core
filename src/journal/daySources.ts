// =====================================================================
// HUP-S10.4 (US-10.4 AC1) — read the day's local sources for the journal summary.
//
// Data sources (Rule 7), all on this device:
//   - metering: `hermes_metering_daily` (the Hermes sidecar's daily report from its metering log)
//   - daemons:  `daemon_runs_between` (core's daemon run log in the same metering folder)
//   - memory:   the personal memory tenant's most recent nodes (bridge.memory.recall)
// Each source is read on its own: one that fails is reported with its reason and the others still
// count. In the web preview none is read (the summary says the metering needs the desktop app).
// Nothing here leaves the machine (AC3).
// =====================================================================
import type { DailyResponse } from "./meteringView";
import type { DaemonRunLog } from "../daemons/api";
import type { MemoryResult } from "../bridge/domains";
import type { DaySources, SourceRead } from "./dailyEntry";

export interface DaySourceDeps {
  mode: string;
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
  recallPersonal(): Promise<MemoryResult>;
}

/** Facts read from the personal tenant for the summary. */
export const MEMORY_RECALL_BUDGET = 5;

const why = (e: unknown) => (e instanceof Error ? e.message : String(e));

async function read<T>(f: () => Promise<T>): Promise<SourceRead<T>> {
  try {
    return { ok: true, value: await f() };
  } catch (e) {
    return { ok: false, why: why(e) };
  }
}

/** [start, end) of a `YYYY-MM-DD` UTC day in ms. */
export function utcDayBounds(day: string): [number, number] {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) throw new Error(`day must be YYYY-MM-DD, got "${day}"`);
  const start = Date.parse(day + "T00:00:00Z");
  if (!Number.isFinite(start)) throw new Error(`not a date: ${day}`);
  return [start, start + 86_400_000];
}

export async function loadDaySources(deps: DaySourceDeps, day: string): Promise<DaySources> {
  if (deps.mode !== "tauri") return { metering: null, daemonRuns: null, memory: null };
  const [from, to] = utcDayBounds(day);
  const [metering, daemonRuns, memory] = await Promise.all([
    read(() => deps.invoke<DailyResponse>("hermes_metering_daily", { day })),
    read(() => deps.invoke<DaemonRunLog>("daemon_runs_between", { fromMs: from, toMs: to })),
    read(async () => (await deps.recallPersonal()).hits),
  ]);
  return { metering, daemonRuns, memory };
}
