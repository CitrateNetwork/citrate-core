// =====================================================================
// citrate-core — the daemon runner, main window (HUP-S10.3, US-10.3 AC2)
//
// Every tick (30 s) it asks Rust which daemons are due and inside their budget
// (`daemons_claim_due`), runs each claimed turn one at a time on the LOCAL model, and reports the
// end back (`daemons_finish_run`). Rust owns schedules, budgets and the ledger; this file owns a
// run's life:
//   - over budget: the token meter passes the run's allowance -> the run is stopped ("over_budget")
//   - deadline: a run longer than `runTimeoutMs` is stopped ("timed_out")
//   - pause: pausing a running daemon stops its run ("stopped"); a claim of this tick that has
//     not started yet is never run once its daemon is paused, or after "pause all" (reported
//     "stopped" with 0 tokens)
//   - anything else that throws -> "failed", with the reason as the note
// Nothing is claimed while no local model can run, so a daemon never falls back to a paid gateway
// (spend budget 0). How a turn runs (the provider, the HIC rules for tool calls) is the store's
// `runDaemonTurn`; this file is kept free of the store so its paths are testable.
// =====================================================================
import { TurnStopped } from "../agent/harness";
import { TokenMeter } from "./tokenMeter";
import type { Claim, DaemonRunEntry, DaemonsApi, RunOutcome } from "./api";

/** Run one claimed turn; resolve with the reply text. Must stop when `signal` aborts. */
export type DaemonTurn = (claim: Claim, signal: AbortSignal, meter: TokenMeter) => Promise<string>;

export interface RunnerDeps {
  api: DaemonsApi;
  now(): number;
  offsetMinutes(): number;
  /** Whether a run may start now (the local model is serving); `why` is shown when not. */
  canRun(): { ok: true } | { ok: false; why: string };
  runTurn: DaemonTurn;
  /** Called whenever the runner's state changes (the slice re-reads the list). */
  onChange(): void;
  /** Longest a run may take. PENDING OWNER SIGN-OFF (placeholder: 10 minutes in the app). */
  runTimeoutMs: number;
  /** HUP-S10.3: a run that started and ended (charged in Rust and in the run log). The app writes
   *  it into the journal. Not called for a claim held back before it started. */
  onFinished?(run: Pick<DaemonRunEntry, "daemonId" | "runId" | "name" | "outcome" | "tokens" | "tokenSource" | "endedMs">): void;
}

export interface RunnerState {
  running: { daemonId: string; runId: string; name: string; startedMs: number; tokensAllowed: number } | null;
  /** Why nothing was claimed on the last tick, if a run could not start. */
  blockedReason: string | null;
  error: string | null;
}

export interface DaemonRunner {
  tick(): Promise<void>;
  /** Pause a daemon; stops its run if one is going. */
  pause(id: string): Promise<void>;
  /** Before a daemon is removed: pause it, stop its run and any claim of it still queued in this
   *  tick, and resolve only once each of those has ended and its end was recorded. */
  cancel(id: string): Promise<void>;
  /** Stop the running run (the Activity monitor's Stop for a daemon run) without pausing. */
  stopRunning(): void;
  /** "Pause all": stop the running run and every claim of this tick that has not started yet. */
  stopAll(): void;
  state(): RunnerState;
  lastReply(id: string): string | null;
}

type StopReason = "budget" | "timeout" | "paused" | "stopped";

const NOTES: Record<StopReason, { outcome: RunOutcome; note: string }> = {
  budget: { outcome: "over_budget", note: "stopped: the run reached its token allowance" },
  timeout: { outcome: "timed_out", note: "stopped: the run passed its time limit" },
  paused: { outcome: "stopped", note: "stopped: you paused this daemon" },
  stopped: { outcome: "stopped", note: "stopped by you" },
};

const firstLine = (s: string) => s.split("\n").map((l) => l.trim()).find((l) => l.length > 0) ?? "";

export function createDaemonRunner(deps: RunnerDeps): DaemonRunner {
  let ticking = false;
  let current: { claim: Claim; ac: AbortController; reason: StopReason | null } | null = null;
  const st: RunnerState = { running: null, blockedReason: null, error: null };
  const replies = new Map<string, string>();
  // Claims of the current tick that must not start: daemons paused mid-tick, or all after "pause all".
  const held = new Set<string>();
  let holdAll = false;
  // Claims of the current tick that have not ended yet (daemon id -> count), and who waits on them.
  const unsettled = new Map<string, number>();
  const waiters = new Map<string, (() => void)[]>();
  const settle = (id: string) => {
    const n = (unsettled.get(id) ?? 1) - 1;
    if (n > 0) {
      unsettled.set(id, n);
      return;
    }
    unsettled.delete(id);
    for (const w of waiters.get(id) ?? []) w();
    waiters.delete(id);
  };

  const stop = (reason: StopReason) => {
    if (!current || current.ac.signal.aborted) return;
    current.reason = reason;
    current.ac.abort();
  };

  async function holdBack(claim: Claim): Promise<void> {
    try {
      await deps.api.finishRun(claim.daemonId, claim.runId, 0, "stopped", "stopped: you paused this daemon before its run started", deps.now());
    } catch (e) {
      st.error = "could not record a daemon run's end: " + (e instanceof Error ? e.message : String(e));
    }
    deps.onChange();
  }

  async function runOne(claim: Claim): Promise<void> {
    if (holdAll || held.has(claim.daemonId)) return holdBack(claim);
    const ac = new AbortController();
    current = { claim, ac, reason: null };
    st.running = { daemonId: claim.daemonId, runId: claim.runId, name: claim.name, startedMs: deps.now(), tokensAllowed: claim.tokensAllowed };
    deps.onChange();
    const meter = new TokenMeter(claim.tokensAllowed, () => stop("budget"));
    const timer = setTimeout(() => stop("timeout"), deps.runTimeoutMs);
    let outcome: RunOutcome = "answered";
    let note = "";
    try {
      const reply = await deps.runTurn(claim, ac.signal, meter);
      if (current?.reason) {
        ({ outcome, note } = NOTES[current.reason]);
      } else {
        replies.set(claim.daemonId, reply);
        note = firstLine(reply);
      }
    } catch (e) {
      const reason = current?.reason ?? null;
      if (reason) ({ outcome, note } = NOTES[reason]);
      else if (e instanceof TurnStopped) ({ outcome, note } = NOTES.stopped);
      else {
        outcome = "failed";
        note = e instanceof Error ? e.message : String(e);
      }
    } finally {
      clearTimeout(timer);
    }
    const tokens = meter.tokens();
    const tokenSource = meter.source();
    const endedMs = deps.now();
    let recorded = false;
    try {
      await deps.api.finishRun(claim.daemonId, claim.runId, tokens, outcome, note, endedMs, tokenSource);
      recorded = true;
    } catch (e) {
      st.error = "could not record a daemon run's end: " + (e instanceof Error ? e.message : String(e));
    }
    // A run core did not accept (for example its daemon was removed) is not written anywhere else.
    if (recorded) {
      try {
        deps.onFinished?.({ daemonId: claim.daemonId, runId: claim.runId, name: claim.name, outcome, tokens, tokenSource, endedMs });
      } catch (e) {
        st.error = "could not write a daemon run to the journal: " + (e instanceof Error ? e.message : String(e));
      }
    }
    current = null;
    st.running = null;
    deps.onChange();
  }

  return {
    async tick() {
      if (ticking) return;
      ticking = true;
      try {
        const can = deps.canRun();
        if (!can.ok) {
          st.blockedReason = can.why;
          deps.onChange();
          return;
        }
        st.blockedReason = null;
        let claims: Claim[];
        try {
          claims = await deps.api.claimDue(deps.now(), deps.offsetMinutes());
          st.error = null;
        } catch (e) {
          st.error = e instanceof Error ? e.message : String(e);
          deps.onChange();
          return;
        }
        for (const c of claims) unsettled.set(c.daemonId, (unsettled.get(c.daemonId) ?? 0) + 1);
        for (const c of claims) {
          try {
            await runOne(c);
          } finally {
            settle(c.daemonId);
          }
        }
      } finally {
        ticking = false;
        held.clear();
        holdAll = false;
      }
    },
    async pause(id) {
      await deps.api.setPaused(id, true, deps.now());
      if (ticking) held.add(id);
      if (current?.claim.daemonId === id) stop("paused");
      deps.onChange();
    },
    async cancel(id) {
      await deps.api.setPaused(id, true, deps.now());
      const done = unsettled.has(id) ? new Promise<void>((res) => waiters.set(id, [...(waiters.get(id) ?? []), res])) : null;
      if (ticking) held.add(id);
      if (current?.claim.daemonId === id) stop("paused");
      deps.onChange();
      if (done) await done;
    },
    stopRunning() {
      stop("stopped");
    },
    stopAll() {
      if (ticking) holdAll = true;
      stop("stopped");
    },
    state: () => ({ ...st, running: st.running ? { ...st.running } : null }),
    lastReply: (id) => replies.get(id) ?? null,
  };
}
