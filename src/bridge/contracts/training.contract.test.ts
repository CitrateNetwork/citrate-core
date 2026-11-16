// CX bridge contract — training (C-21). Pins the frozen shape (CX-S0.2).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — training (frozen CX-S0.2)", () => {
  it("exposes the training domain's reads", () => {
    expect(bridge.training).toBeDefined();
    for (const m of ["status", "reward"] as const) {
      expect(typeof bridge.training[m]).toBe("function");
    }
  });
  it("V5: offers no start, contribute or claim (they were stubs that always failed)", () => {
    for (const m of ["start", "contribute", "claim"]) {
      expect((bridge.training as unknown as Record<string, unknown>)[m]).toBeUndefined();
    }
  });
  it("sim is honest-idle (no fabricated round/reward, Rule 1)", async () => {
    if (bridge.mode === "sim") {
      expect((await bridge.training.status("g")).phase).toBe("idle");
      expect((await bridge.training.reward("g")).salt).toBe("0");
    }
  });
});
