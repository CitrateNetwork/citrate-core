// CX bridge contract — agentHarness (C-22). Pins the frozen shape (CX-S0.2).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — agentHarness (frozen CX-S0.2)", () => {
  it("exposes the agentHarness domain with its frozen methods", () => {
    expect(bridge.agentHarness).toBeDefined();
    for (const m of ["start", "status", "skills", "runSkill", "pendingApprovals", "stop"] as const) {
      expect(typeof bridge.agentHarness[m]).toBe("function");
    }
  });
  it("sim is honest — not running, no skills, no approvals (Rule 1)", async () => {
    if (bridge.mode === "sim") {
      const s = await bridge.agentHarness.status();
      expect(s.running).toBe(false);
      expect(await bridge.agentHarness.skills()).toEqual([]);
    }
  });
  it("agentHarness is distinct from the legacy node-agent `agent` domain", () => {
    expect(bridge.agent).toBeDefined(); // legacy GPU-market agent still present
    expect(bridge.agentHarness).not.toBe(bridge.agent);
  });
});
