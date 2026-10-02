// =====================================================================
// citrate-core — daemons API (HUP-S10.3)
//
// The Rust `daemons_*` commands (src-tauri/src/daemons.rs), which own every schedule, budget and
// ledger. Desktop app only: in the web preview there is no Rust side, and `daemonsApi()` is null.
// =====================================================================
import { invoke } from "../bridge/tauri/invoke";
import { BRIDGE_MODE } from "../bridge/mode";

export interface Budget {
  maxRunsPerDay: number;
  maxTokensPerDay: number;
  maxTokensPerRun: number;
  /** Always "0" in this release. */
  maxSpendSalt: string;
}

export type RunOutcome = "answered" | "failed" | "stopped" | "timed_out" | "over_budget";

export interface DaemonView {
  id: string;
  name: string;
  prompt: string;
  schedule: string;
  budget: Budget;
  paused: boolean;
  status: "paused" | "running" | "budget used up today" | "scheduled" | "never runs";
  running: boolean;
  runsToday: number;
  tokensToday: number;
  skippedToday: number;
  spendTodaySalt: string;
  nextRunMs: number | null;
  lastRunMs: number | null;
  lastOutcome: string | null;
  lastNote: string | null;
}

export interface DaemonsView {
  allPaused: boolean;
  daemons: DaemonView[];
}

export interface DaemonInput {
  id?: string;
  name: string;
  prompt: string;
  schedule: string;
  budget?: Budget;
}

export interface Claim {
  daemonId: string;
  runId: string;
  name: string;
  prompt: string;
  tokensAllowed: number;
  startedMs: number;
}

export interface DaemonsApi {
  list(nowMs: number, offsetMin: number): Promise<DaemonsView>;
  save(input: DaemonInput, nowMs: number, offsetMin: number): Promise<DaemonView>;
  setPaused(id: string, paused: boolean, nowMs: number): Promise<void>;
  setAllPaused(paused: boolean, nowMs: number): Promise<void>;
  remove(id: string): Promise<void>;
  claimDue(nowMs: number, offsetMin: number): Promise<Claim[]>;
  finishRun(id: string, runId: string, tokensUsed: number, outcome: RunOutcome, note: string, nowMs: number): Promise<void>;
}

export const tauriDaemonsApi: DaemonsApi = {
  list: (nowMs, offsetMin) => invoke<DaemonsView>("daemons_list", { nowMs, offsetMin }),
  save: (input, nowMs, offsetMin) => invoke<DaemonView>("daemon_save", { input, nowMs, offsetMin }),
  setPaused: (id, paused, nowMs) => invoke<void>("daemon_set_paused", { id, paused, nowMs }),
  setAllPaused: (paused, nowMs) => invoke<void>("daemons_set_all_paused", { paused, nowMs }),
  remove: (id) => invoke<void>("daemon_delete", { id }),
  claimDue: (nowMs, offsetMin) => invoke<Claim[]>("daemons_claim_due", { nowMs, offsetMin }),
  finishRun: (id, runId, tokensUsed, outcome, note, nowMs) =>
    invoke<void>("daemons_finish_run", { id, runId, tokensUsed: Math.min(Math.max(0, Math.round(tokensUsed)), 0xffffffff), outcome, note, nowMs }),
};

/** The API in the desktop app; null in the web preview. */
export function daemonsApi(): DaemonsApi | null {
  return BRIDGE_MODE === "tauri" ? tauriDaemonsApi : null;
}

/** This machine's offset from UTC in minutes east (what Rust expects). */
export function localOffsetMinutes(d: Date = new Date()): number {
  return -d.getTimezoneOffset();
}

/** Default budget values, mirroring Rust. PENDING OWNER SIGN-OFF (conservative placeholders). */
export const DEFAULT_BUDGET: Budget = { maxRunsPerDay: 4, maxTokensPerDay: 20_000, maxTokensPerRun: 6_000, maxSpendSalt: "0" };
