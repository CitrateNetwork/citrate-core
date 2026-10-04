// HUP-S9.4 — Hermes's two round tools through the store's gated handlers. fl_round_plan is a
// read; fl_round_start always stops at the member's explicit decision (HIC-1) and only then asks
// core to record the start for that exact plan. Written red-first.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { AGENT_TOOLS, AGENT_SYSTEM_PROMPT, type ToolCall } from "../agent/harness";
import { annotationFor } from "../agent/toolAnnotations";
import { livePlan, PLAN_HASH } from "./fixtures/plan";

const call = (name: string, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};
const HIC = { hic: "required" as const, hicReason: "this session read untrusted content" };

beforeEach(() => {
  store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null });
});
afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ queue: [], walletReview: null });
});

describe("tool schemas and annotations", () => {
  it("offers fl_round_plan (read) and fl_round_start (write) with reviewed annotations", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name);
    expect(names).toContain("fl_round_plan");
    expect(names).toContain("fl_round_start");
    expect(annotationFor("fl_round_plan")).toEqual({ effect: "none", trust: "trusted" });
    expect(annotationFor("fl_round_start")).toEqual({ effect: "write", trust: "trusted" });
    const start = AGENT_TOOLS.find((t) => t.function.name === "fl_round_start")!;
    expect(start.function.parameters.required).toEqual(["planHash"]);
  });

  it("the system prompt tells Hermes it proposes rounds and the member decides", () => {
    expect(AGENT_SYSTEM_PROMPT).toContain("fl_round_plan");
    expect(AGENT_SYSTEM_PROMPT).toMatch(/fl_round_start/);
  });
});

describe("fl_round_plan", () => {
  it("returns core's plan as plain words with the plan hash, and asks for nothing", async () => {
    const plan = vi.spyOn(bridge.flRounds, "plan").mockResolvedValue(livePlan());
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call("fl_round_plan", { requires: "probe", loraRank: 4 }), "m1", noop);
    expect(plan).toHaveBeenCalledWith({ requires: "probe", loraRank: 4, maxTrajectories: 500, leaseHours: 6 });
    expect(sig).not.toHaveBeenCalled();
    expect(out).toContain(PLAN_HASH);
    expect(out).toContain("nothing is paid");
  });

  it("says so plainly when core cannot plan", async () => {
    vi.spyOn(bridge.flRounds, "plan").mockRejectedValue(new Error("the LoRA rank must be a power of two from 1 to 64"));
    const out = await store.handleTool(call("fl_round_plan", { loraRank: 3 }), "m1", noop);
    expect(out).toMatch(/couldn't plan/i);
    expect(out).toContain("power of two");
  });
});

describe("fl_round_start (HIC-1)", () => {
  it("shows the plan on an approval card and starts nothing when the member declines", async () => {
    vi.spyOn(bridge.flRounds, "lookupPlan").mockResolvedValue(livePlan());
    const start = vi.spyOn(bridge.flRounds, "start");
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    const out = await store.handleTool(call("fl_round_start", { planHash: PLAN_HASH }), "m1", noop);
    expect(sig).toHaveBeenCalledTimes(1);
    const spec = sig.mock.calls[0][0];
    expect(spec.title).toMatch(/federated training round/i);
    expect(spec.card?.kind).toBe("fields");
    expect(start).not.toHaveBeenCalled();
    expect(out).toMatch(/declined/);
  });

  it("starts exactly that plan after approval and reports that no training ran", async () => {
    vi.spyOn(bridge.flRounds, "lookupPlan").mockResolvedValue(livePlan());
    const start = vi
      .spyOn(bridge.flRounds, "start")
      .mockResolvedValue({ planHash: PLAN_HASH, coordinatorUrl: "https://c", authorizedAtMs: 1, trainingStarted: false, note: "No training has started." });
    vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    const out = await store.handleTool(call("fl_round_start", { planHash: PLAN_HASH }), "m1", noop);
    expect(start).toHaveBeenCalledWith(PLAN_HASH);
    expect(out).toContain("No training has started.");
  });

  it("carries a hic:required reason on its one card instead of a second card", async () => {
    vi.spyOn(bridge.flRounds, "lookupPlan").mockResolvedValue(livePlan());
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("fl_round_start", { planHash: PLAN_HASH }), "m1", noop, HIC);
    expect(sig).toHaveBeenCalledTimes(1);
    expect(sig.mock.calls[0][0].hic?.reason).toContain("untrusted");
  });

  it("refuses an unknown plan hash without asking", async () => {
    vi.spyOn(bridge.flRounds, "lookupPlan").mockRejectedValue(new Error("no plan with that hash on this device; plan the round first"));
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call("fl_round_start", { planHash: "f".repeat(64) }), "m1", noop);
    expect(sig).not.toHaveBeenCalled();
    expect(out).toContain("plan the round first");
  });
});

describe("fl_round_plan with a named round (fan-out 6)", () => {
  it("offers a roundId parameter and passes it to core", async () => {
    const tool = AGENT_TOOLS.find((t) => t.function.name === "fl_round_plan")!;
    expect(Object.keys(tool.function.parameters.properties)).toContain("roundId");
    const round = "0x" + "ab".repeat(32);
    const plan = vi.spyOn(bridge.flRounds, "plan").mockResolvedValue(livePlan());
    await store.handleTool(call("fl_round_plan", { roundId: round }), "m1", noop);
    expect(plan).toHaveBeenCalledWith({ requires: "federated", loraRank: 8, maxTrajectories: 500, leaseHours: 6, roundId: round });
  });
});

describe("fl_round_start for a named round (fan-out 6)", () => {
  it("names the round and its consent on the one approval card", async () => {
    const round = "0x" + "cd".repeat(32);
    vi.spyOn(bridge.flRounds, "lookupPlan").mockResolvedValue(livePlan({ proposal: { requires: "federated", loraRank: 8, maxTrajectories: 500, leaseHours: 6, roundId: round } }));
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("fl_round_start", { planHash: PLAN_HASH }), "m1", noop);
    expect(sig).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(sig.mock.calls[0][0].card)).toContain(round);
    expect(JSON.stringify(sig.mock.calls[0][0].card)).toContain("consent for this round only");
  });
});
