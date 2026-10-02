// HUP-S9.4 — the shared start flow (HIC-1) and the text Hermes reads about a round plan.
// Written red-first: the module did not exist, so these failed on the import.
import { describe, it, expect, vi } from "vitest";
import type { FlAdapterGateRecord } from "../bridge/domains";
import {
  approveAndStartRound,
  flStartCerSpec,
  formatPlanForAgent,
  gateSummary,
  proposalFromToolArgs,
  DEFAULT_PROPOSAL,
} from "./flRounds";

import { livePlan, PLAN_HASH as HASH } from "./fixtures/plan";

const blocked = () =>
  livePlan({
    coordinator: { state: "notConfigured" },
    canStart: false,
    blockers: ["No training coordinator is configured."],
    explain: { ...livePlan().explain, status: "No training coordinator is configured." },
  });

describe("the start approval card (HIC-1)", () => {
  it("is chainless and shows data, compute, reward, privacy and the exact plan hash", () => {
    const spec = flStartCerSpec(livePlan(), "chat agent");
    expect(spec.chainless).toBe(true);
    const keys = spec.rows.map((r) => r.k);
    expect(keys).toEqual(expect.arrayContaining(["Data", "Compute", "Reward", "Privacy", "Coordinator", "Plan"]));
    expect(spec.rows.find((r) => r.k === "Plan")!.v).toContain(HASH.slice(0, 16));
    expect(spec.cost).toMatch(/no chain transaction/);
    expect(JSON.stringify(spec)).not.toContain("—");
  });
});

describe("approveAndStartRound", () => {
  it("does not start when the member declines", async () => {
    const start = vi.fn();
    const out = await approveAndStartRound({ requestSig: async () => "declined", start }, livePlan(), "chat agent");
    expect(start).not.toHaveBeenCalled();
    expect(out.status).toBe("declined");
    expect(out.message).toMatch(/nothing was started/i);
  });

  it("starts exactly the approved plan hash only after approval", async () => {
    const order: string[] = [];
    const start = vi.fn(async (h: string) => {
      order.push("start:" + h);
      return { planHash: h, coordinatorUrl: "https://c", authorizedAtMs: 2, trainingStarted: false, note: "No training has started." };
    });
    const out = await approveAndStartRound(
      {
        requestSig: async () => {
          order.push("approve");
          return "approved";
        },
        start,
      },
      livePlan(),
      "Train surface",
    );
    expect(order).toEqual(["approve", "start:" + HASH]);
    expect(out.status).toBe("approved");
    expect(out.receipt?.trainingStarted).toBe(false);
    expect(out.message).toContain("No training has started.");
  });

  it("never asks the member to approve a plan that cannot start", async () => {
    const requestSig = vi.fn();
    const start = vi.fn();
    const out = await approveAndStartRound({ requestSig, start }, blocked(), "chat agent");
    expect(requestSig).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
    expect(out.status).toBe("blocked");
    expect(out.message).toContain("No training coordinator is configured");
  });

  it("reports core's refusal after approval honestly", async () => {
    const out = await approveAndStartRound(
      { requestSig: async () => "approved", start: async () => Promise.reject(new Error("the coordinator changed since you approved")) },
      livePlan(),
      "chat agent",
    );
    expect(out.status).toBe("error");
    expect(out.message).toContain("changed since you approved");
  });
});

describe("formatPlanForAgent", () => {
  it("gives Hermes the plain-words explanation, the plan hash and how to ask the member", () => {
    const t = formatPlanForAgent(livePlan());
    expect(t).toContain(HASH);
    expect(t).toContain("nothing is paid");
    expect(t).toContain("fl_round_start");
    expect(t).not.toMatch(/training (has )?started/i);
  });

  it("says plainly when the round cannot start and why", () => {
    const t = formatPlanForAgent(blocked());
    expect(t).toMatch(/cannot start/i);
    expect(t).toContain("No training coordinator is configured");
    expect(t).not.toContain("fl_round_start with");
  });
});

describe("proposalFromToolArgs", () => {
  it("fills conservative defaults and coerces numeric strings", () => {
    expect(proposalFromToolArgs({})).toEqual(DEFAULT_PROPOSAL);
    expect(proposalFromToolArgs({ requires: "probe", loraRank: "16", maxTrajectories: 200, leaseHours: "12" })).toEqual({
      requires: "probe",
      loraRank: 16,
      maxTrajectories: 200,
      leaseHours: 12,
    });
  });

  it("passes odd values through for core to refuse rather than silently fixing them", () => {
    const p = proposalFromToolArgs({ requires: "h01", loraRank: 3 });
    expect(p.requires).toBe("h01");
    expect(p.loraRank).toBe(3);
    expect(proposalFromToolArgs({ requires: "everything" }).requires).toBe("federated");
  });
});

describe("gateSummary", () => {
  const rec = (verdict: "ACCEPT" | "REJECT"): FlAdapterGateRecord => ({
    adapterSha256: "b".repeat(64),
    adapterPath: "/x/a.gguf",
    baseModel: "m",
    decidedAtMs: 5,
    decision: {
      verdict,
      reasons: verdict === "REJECT" ? ["injectionResistRate got worse (1.0000 to 0.9500)"] : [],
      metrics: [{ metric: "validToolCallRate", base: 0.9, candidate: 0.95, improvement: 0.05 }],
      compositeBase: 0.8,
      compositeCandidate: 0.85,
    },
  });
  it("reads as a verdict line plus per-metric deltas and reasons", () => {
    const a = gateSummary(rec("ACCEPT"));
    expect(a.headline).toMatch(/passed/i);
    expect(a.lines.join("\n")).toContain("validToolCallRate 0.900 to 0.950 (+0.050)");
    const r = gateSummary(rec("REJECT"));
    expect(r.headline).toMatch(/rejected/i);
    expect(r.lines.join("\n")).toContain("injectionResistRate got worse");
  });
});
