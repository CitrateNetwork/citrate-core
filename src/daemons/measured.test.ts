// HUP-S10.3 (US-10.3 AC2) — a daemon run's tokens are MEASURED from the model server's usage when
// every model call reported it (estimated otherwise, and labelled so), each finished run is
// reported with its token source, and the app writes it into the journal.
import { describe, it, expect, vi } from "vitest";
import { TokenMeter } from "./tokenMeter";
import { runDaemonTurn, type DaemonTurnDeps } from "./turn";
import { createDaemonRunner, type RunnerDeps } from "./runner";
import type { Claim, DaemonsApi } from "./api";

const claim: Claim = { daemonId: "d1", runId: "r1", name: "Node digest", prompt: "Summarise my node.", tokensAllowed: 100_000, startedMs: 0 };
const ctx = { height: 1, peers: 2, finalityAge: 3, nodeState: "synced", staked: 0, liquid: 0, claimable: 0, earningsToday: 0, walletAddr: "0x0", tier: "T0" };

describe("TokenMeter: measured beats estimated only when every call reported", () => {
  it("sums each call's prompt and written tokens", () => {
    const m = new TokenMeter(10_000, () => undefined);
    m.begin(4000, 40);
    m.round();
    m.measured(900, 30);
    m.round();
    m.measured(960, 50);
    expect(m.source()).toBe("measured");
    expect(m.tokens()).toBe(1940);
  });

  it("falls back to the estimate while a call has no report, and says so", () => {
    const m = new TokenMeter(10_000, () => undefined);
    m.begin(400, 40);
    m.round();
    m.measured(900, 30);
    m.round(); // this call reported nothing
    expect(m.source()).toBe("estimated");
    expect(m.tokens()).toBe(Math.ceil((440 + 440) / 4));
  });

  it("a run with no reports at all is estimated", () => {
    const m = new TokenMeter(10_000, () => undefined);
    m.begin(400, 0);
    m.round();
    expect(m.source()).toBe("estimated");
  });

  it("a measured count past the allowance stops the run once", () => {
    const over = vi.fn();
    const m = new TokenMeter(1_000, over);
    m.begin(4, 4); // the estimate stays tiny
    m.round();
    m.measured(990, 20);
    m.measured(5, 5);
    expect(over).toHaveBeenCalledTimes(1);
  });

  it("ignores a nonsense report", () => {
    const m = new TokenMeter(1_000, () => undefined);
    m.round();
    m.measured(-1, 5);
    m.measured(Number.NaN, 5);
    expect(m.source()).toBe("estimated");
  });
});

describe("a daemon turn feeds the server's usage to its meter", () => {
  it("on the in-app loop, from core's citrate_usage", async () => {
    const replies = [
      JSON.stringify({ role: "assistant", content: null, tool_calls: [{ id: "c1", function: { name: "node_status", arguments: "{}" } }], citrate_usage: { prompt_tokens: 2000, completion_tokens: 15, generation_ms: 500 } }),
      JSON.stringify({ role: "assistant", content: "Height 1.", citrate_usage: { prompt_tokens: 2100, completion_tokens: 25 } }),
    ];
    const d: DaemonTurnDeps = {
      providerKind: "local",
      systemPrompt: () => "You are Hermes.",
      context: () => ctx,
      inferLocalTools: vi.fn(async () => replies.shift() ?? ""),
      sidecar: null,
      handleTool: async () => "{}",
    };
    const meter = new TokenMeter(100_000, () => undefined);
    await runDaemonTurn(claim, new AbortController().signal, meter, d);
    expect(meter.source()).toBe("measured");
    expect(meter.tokens()).toBe(2000 + 15 + 2100 + 25);
  });
});

describe("the runner reports the token source and the journal hears about each run", () => {
  function api() {
    const finished: unknown[][] = [];
    const a = {
      finished,
      list: vi.fn(async () => ({ allPaused: false, daemons: [] })),
      save: vi.fn(),
      setPaused: vi.fn(async () => undefined),
      setAllPaused: vi.fn(async () => undefined),
      remove: vi.fn(async () => undefined),
      claimDue: vi.fn(async () => [claim]),
      finishRun: vi.fn(async (...args: unknown[]) => {
        finished.push(args);
      }),
      runsBetween: vi.fn(),
    };
    return a as unknown as DaemonsApi & { finished: unknown[][] };
  }
  const deps = (a: DaemonsApi, over: Partial<RunnerDeps> = {}): RunnerDeps => ({
    api: a,
    now: () => 5_000,
    offsetMinutes: () => 0,
    canRun: () => ({ ok: true }),
    runTurn: async (_c, _s, meter) => {
      meter.round();
      meter.measured(700, 20);
      return "Height 1.";
    },
    onChange: () => undefined,
    runTimeoutMs: 1_000,
    ...over,
  });

  it("a measured run is charged as measured and written to the journal", async () => {
    const a = api();
    const onFinished = vi.fn();
    await createDaemonRunner(deps(a, { onFinished })).tick();
    expect(a.finished).toEqual([["d1", "r1", 720, "answered", "Height 1.", 5_000, "measured"]]);
    expect(onFinished).toHaveBeenCalledWith({ daemonId: "d1", runId: "r1", name: "Node digest", outcome: "answered", tokens: 720, tokenSource: "measured", endedMs: 5_000 });
  });

  it("an estimated failed run is reported as estimated", async () => {
    const a = api();
    const onFinished = vi.fn();
    await createDaemonRunner(
      deps(a, {
        onFinished,
        runTurn: async (_c, _s, meter) => {
          meter.begin(400, 0);
          meter.round();
          throw new Error("the local model stopped answering");
        },
      }),
    ).tick();
    expect(a.finished[0]).toEqual(["d1", "r1", 100, "failed", "the local model stopped answering", 5_000, "estimated"]);
    expect(onFinished.mock.calls[0][0]).toMatchObject({ outcome: "failed", tokenSource: "estimated" });
  });

  it("a journal write that throws is reported and does not lose the ledger entry", async () => {
    const a = api();
    const r = createDaemonRunner(deps(a, { onFinished: () => { throw new Error("disk full"); } }));
    await r.tick();
    expect(a.finished).toHaveLength(1);
    expect(r.state().error).toMatch(/journal: disk full/);
  });
});
