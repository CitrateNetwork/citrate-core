// HUP-S10.3 (US-10.3 AC2) — the daemon runner: it claims due runs from Rust (which owns schedules
// and budgets), runs each turn on the local model, stops a run at its token allowance, its deadline
// or a pause, and always reports the end back. These are the budget-exhaustion and HIC paths.
import { describe, it, expect, vi } from "vitest";
import { createDaemonRunner, type RunnerDeps, type DaemonTurn } from "./runner";
import { TokenMeter } from "./tokenMeter";
import { TurnStopped } from "../agent/harness";
import type { Claim, DaemonsApi, RunOutcome } from "./api";

const claim = (over: Partial<Claim> = {}): Claim => ({
  daemonId: "d1",
  runId: "r1",
  name: "Node digest",
  prompt: "Summarise my node.",
  tokensAllowed: 1_000,
  startedMs: 0,
  ...over,
});

type Finish = { id: string; runId: string; tokens: number; outcome: RunOutcome; note: string };

function fakeApi(claims: Claim[][]): DaemonsApi & { finished: Finish[]; claimed: number } {
  const finished: Finish[] = [];
  const api = {
    finished,
    claimed: 0,
    list: vi.fn(async () => ({ allPaused: false, daemons: [] })),
    save: vi.fn(),
    setPaused: vi.fn(async () => undefined),
    setAllPaused: vi.fn(async () => undefined),
    remove: vi.fn(async () => undefined),
    claimDue: vi.fn(async () => {
      api.claimed++;
      return claims.shift() ?? [];
    }),
    finishRun: vi.fn(async (id: string, runId: string, tokens: number, outcome: RunOutcome, note: string) => {
      finished.push({ id, runId, tokens, outcome, note });
    }),
  };
  return api as unknown as DaemonsApi & { finished: Finish[]; claimed: number };
}

function deps(api: DaemonsApi, turn: DaemonTurn, over: Partial<RunnerDeps> = {}): RunnerDeps {
  return {
    api,
    now: () => 0,
    offsetMinutes: () => 0,
    canRun: () => ({ ok: true }),
    runTurn: turn,
    onChange: () => undefined,
    runTimeoutMs: 1_000,
    ...over,
  };
}

describe("TokenMeter", () => {
  it("estimates tokens as characters over four, re-reading the context each round", () => {
    const m = new TokenMeter(10_000, () => undefined);
    m.begin(400, 40); // 400 chars of system prompt + tools, a 40-char task
    m.round(); // reads 440
    m.context(160); // a tool result
    m.round(); // reads 600
    m.output(200); // the answer
    expect(m.tokens()).toBe(Math.ceil((440 + 600 + 200) / 4));
  });

  it("counts one round even if the provider reports none", () => {
    const m = new TokenMeter(10_000, () => undefined);
    m.begin(400, 0);
    m.output(4);
    expect(m.tokens()).toBe(Math.ceil((400 + 4) / 4));
  });

  it("calls over-budget once, when the estimate passes the allowance", () => {
    const over = vi.fn();
    const m = new TokenMeter(100, over);
    m.begin(300, 0);
    m.round();
    expect(over).not.toHaveBeenCalled();
    m.output(200);
    m.output(200);
    expect(over).toHaveBeenCalledTimes(1);
  });
});

describe("daemon runner", () => {
  it("runs a claimed turn and reports it answered with its estimated tokens and first line", async () => {
    const api = fakeApi([[claim()]]);
    const turn: DaemonTurn = async (_c, _s, meter) => {
      meter.begin(400, 20);
      meter.round();
      meter.output(80);
      return "Height 120,345, 7 peers.\nAll good.";
    };
    const r = createDaemonRunner(deps(api, turn));
    await r.tick();
    expect(api.finished).toEqual([{ id: "d1", runId: "r1", tokens: Math.ceil((420 + 80) / 4), outcome: "answered", note: "Height 120,345, 7 peers." }]);
    expect(r.lastReply("d1")).toBe("Height 120,345, 7 peers.\nAll good.");
  });

  it("claims nothing while no local model can run (spend stays zero)", async () => {
    const api = fakeApi([[claim()]]);
    const turn = vi.fn();
    const r = createDaemonRunner(deps(api, turn, { canRun: () => ({ ok: false, why: "daemons run only on the local model" }) }));
    await r.tick();
    expect(api.claimed).toBe(0);
    expect(turn).not.toHaveBeenCalled();
    expect(r.state().blockedReason).toBe("daemons run only on the local model");
  });

  it("stops a run at its token allowance and reports it over budget", async () => {
    const api = fakeApi([[claim({ tokensAllowed: 100 })]]);
    let toolRan = false;
    const turn: DaemonTurn = async (_c, signal, meter) => {
      meter.begin(300, 0);
      meter.round();
      meter.output(200); // 125 tokens > 100: the runner aborts
      if (signal.aborted) throw new TurnStopped();
      toolRan = true;
      return "late";
    };
    const r = createDaemonRunner(deps(api, turn));
    await r.tick();
    expect(toolRan).toBe(false);
    expect(api.finished[0].outcome).toBe("over_budget");
    expect(api.finished[0].tokens).toBe(125);
    expect(api.finished[0].note).toMatch(/token allowance/);
  });

  it("stops a run that passes its deadline and reports it timed out", async () => {
    vi.useFakeTimers();
    try {
      const api = fakeApi([[claim()]]);
      const turn: DaemonTurn = (_c, signal) =>
        new Promise((_, reject) => signal.addEventListener("abort", () => reject(new TurnStopped())));
      const r = createDaemonRunner(deps(api, turn, { runTimeoutMs: 5_000 }));
      const t = r.tick();
      await vi.advanceTimersByTimeAsync(5_001);
      await t;
      expect(api.finished[0].outcome).toBe("timed_out");
    } finally {
      vi.useRealTimers();
    }
  });

  it("pausing a running daemon stops its run and reports it stopped", async () => {
    const api = fakeApi([[claim()]]);
    let started!: () => void;
    const begun = new Promise<void>((res) => (started = res));
    const turn: DaemonTurn = (_c, signal) =>
      new Promise((_, reject) => {
        started();
        signal.addEventListener("abort", () => reject(new TurnStopped()));
      });
    const r = createDaemonRunner(deps(api, turn));
    const t = r.tick();
    await begun;
    expect(r.state().running?.daemonId).toBe("d1");
    await r.pause("d1");
    await t;
    expect(api.setPaused).toHaveBeenCalledWith("d1", true, 0);
    expect(api.finished[0].outcome).toBe("stopped");
    expect(api.finished[0].note).toMatch(/paused/);
    expect(r.state().running).toBeNull();
  });

  it("reports a failed turn honestly and keeps going with the next claim", async () => {
    const api = fakeApi([[claim(), claim({ daemonId: "d2", runId: "r2" })]]);
    const turn: DaemonTurn = async (c, _s, meter) => {
      meter.begin(40, 0);
      if (c.daemonId === "d1") throw new Error("the local model isn't running yet");
      return "ok";
    };
    const r = createDaemonRunner(deps(api, turn));
    await r.tick();
    expect(api.finished.map((f) => [f.id, f.outcome])).toEqual([["d1", "failed"], ["d2", "answered"]]);
    expect(api.finished[0].note).toContain("isn't running");
  });

  it("never runs two ticks at once", async () => {
    const api = fakeApi([[claim()], [claim({ runId: "r9" })]]);
    let release!: () => void;
    const gate = new Promise<void>((res) => (release = res));
    const turn: DaemonTurn = async () => {
      await gate;
      return "ok";
    };
    const r = createDaemonRunner(deps(api, turn));
    const a = r.tick();
    await r.tick(); // returns at once: a tick is in progress
    expect(api.claimed).toBe(1);
    release();
    await a;
  });

  it("a claim error is surfaced, not swallowed", async () => {
    const api = fakeApi([]);
    (api.claimDue as unknown as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("the daemons file could not be read"));
    const r = createDaemonRunner(deps(api, vi.fn()));
    await r.tick();
    expect(r.state().error).toContain("could not be read");
  });
});
