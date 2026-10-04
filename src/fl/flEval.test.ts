// HUP-S9.4 (n5) — the in-app eval run: both arms through core, then core decides. Written
// red-first (flEval.ts did not exist). The domain is a typed in-test double; production uses core.
import { describe, it, expect, vi } from "vitest";
import type { FlAdapterGateRecord, FlEvalArm, FlRoundsDomain } from "../bridge/domains";
import { runInAppEval, type EvalDatasets } from "./flEval";
import type { InjectionCase, ToolcallTask } from "../agent/eval/runner";

const SHA = "e".repeat(64);

const datasets: EvalDatasets = {
  toolcall: {
    version: "toolcall-v1",
    tasks: [
      { id: "t1", prompt: "Show my wallet balance", expect: { tool: null } } as unknown as ToolcallTask,
      { id: "t2", prompt: "hello", expect: { tool: null } } as unknown as ToolcallTask,
    ],
  },
  injection: { version: "injection-v1", cases: [] as InjectionCase[] },
};

function rec(verdict: "ACCEPT" | "REJECT"): FlAdapterGateRecord {
  return {
    adapterSha256: SHA,
    adapterPath: "/data/adapters/eval/" + SHA + ".gguf",
    baseModel: "m",
    decidedAtMs: 1,
    decision: { verdict, reasons: [], metrics: [], compositeBase: 0.5, compositeCandidate: 0.6 },
  };
}

type EvalDomain = Pick<FlRoundsDomain, "evalBegin" | "evalComplete" | "evalFinish" | "evalEnd">;

function domain(over: Partial<EvalDomain> = {}): EvalDomain & { calls: FlEvalArm[] } {
  const calls: FlEvalArm[] = [];
  return {
    calls,
    evalBegin: vi.fn(async () => ({ sessionId: "s1", adapterSha256: SHA, model: "m", startedAtMs: 1, baseCalls: 0, candidateCalls: 0 })),
    evalComplete: vi.fn(async (_id: string, arm: FlEvalArm) => {
      calls.push(arm);
      return JSON.stringify({ role: "assistant", content: "Hello.", tool_calls: [] });
    }),
    evalFinish: vi.fn(async () => rec("ACCEPT")),
    evalEnd: vi.fn(async () => {}),
    ...over,
  };
}

describe("runInAppEval", () => {
  it("runs the base arm, then the candidate arm, through core, and lets core decide", async () => {
    const d = domain();
    const progress: string[] = [];
    const out = await runInAppEval(d, "/x/cand.gguf", SHA, (p) => progress.push(`${p.arm}:${p.done}/${p.total}`), datasets);
    expect(out.decision.verdict).toBe("ACCEPT");
    expect(d.evalBegin).toHaveBeenCalledWith("/x/cand.gguf", SHA);
    expect(d.calls).toEqual(["base", "base", "candidate", "candidate"]);
    expect(progress).toEqual(["base:1/2", "base:2/2", "candidate:1/2", "candidate:2/2"]);
    const [id, baseJson, candJson] = (d.evalFinish as ReturnType<typeof vi.fn>).mock.calls[0] as [string, string, string];
    expect(id).toBe("s1");
    const base = JSON.parse(baseJson);
    const cand = JSON.parse(candJson);
    // Both scorecards name the served model and the item count core must have answered.
    expect(base.model).toBe("m");
    expect(base.n).toBe(2);
    expect(cand.n).toBe(2);
    // The base run is never stamped; core stamps the candidate with the session's adapter.
    expect(base.adapterSha256).toBeUndefined();
    expect(d.evalEnd).not.toHaveBeenCalled();
  });

  it("sends the runner's messages and tools to core as JSON", async () => {
    const d = domain();
    await runInAppEval(d, "/x/cand.gguf", SHA, undefined, datasets);
    const [, , msgs, tools] = (d.evalComplete as ReturnType<typeof vi.fn>).mock.calls[0] as [string, string, string, string];
    expect(Array.isArray(JSON.parse(msgs))).toBe(true);
    expect(JSON.parse(tools).length).toBeGreaterThan(0);
  });

  it("ends the run in core when a request fails, and reports the failure", async () => {
    const d = domain({
      evalComplete: vi.fn(async () => {
        throw new Error("the local model answered HTTP 500");
      }),
    });
    await expect(runInAppEval(d, "/x/cand.gguf", SHA, undefined, datasets)).rejects.toThrow(/HTTP 500/);
    expect(d.evalEnd).toHaveBeenCalledWith("s1");
    expect(d.evalFinish).not.toHaveBeenCalled();
  });

  it("ends the run when core refuses the scorecards", async () => {
    const d = domain({
      evalFinish: vi.fn(async () => {
        throw new Error("the base scorecard reports 2 items but core answered 1 base requests");
      }),
    });
    await expect(runInAppEval(d, "/x/cand.gguf", SHA, undefined, datasets)).rejects.toThrow(/core answered/);
    expect(d.evalEnd).toHaveBeenCalledWith("s1");
  });

  it("treats a reply that is not a message object as a transport error, not a model verdict", async () => {
    const d = domain({ evalComplete: vi.fn(async () => "[]") });
    await expect(runInAppEval(d, "/x/cand.gguf", SHA, undefined, datasets)).rejects.toThrow(/message/);
    expect(d.evalEnd).toHaveBeenCalled();
  });

  it("does not start when core refuses to begin", async () => {
    const d = domain({
      evalBegin: vi.fn(async () => {
        throw new Error("start the local model first; the eval runs on it");
      }),
    });
    await expect(runInAppEval(d, "/x/cand.gguf", SHA, undefined, datasets)).rejects.toThrow(/start the local model/);
    expect(d.evalComplete).not.toHaveBeenCalled();
    expect(d.evalEnd).not.toHaveBeenCalled();
  });
});

describe("shippedDatasets", () => {
  it("loads the same tool-call and injection sets the CLI runs", async () => {
    const { shippedDatasets } = await import("./flEval");
    const ds = await shippedDatasets();
    expect(ds.toolcall.version).toBe("toolcall-v1");
    expect(ds.injection.version).toBe("injection-v1");
    expect(ds.toolcall.tasks.length + ds.injection.cases.length).toBe(88);
  });
});
