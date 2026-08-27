// CX bridge contract — cluster (C-20). Pins the frozen shape (CX-S0.2).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — cluster (frozen CX-S0.2)", () => {
  it("exposes the cluster domain with its frozen methods", () => {
    expect(bridge.cluster).toBeDefined();
    for (const m of ["status", "join", "peers", "shareFile", "leave"] as const) {
      expect(typeof bridge.cluster[m]).toBe("function");
    }
  });
  it("sim is honest-empty (no fabricated peers, Rule 1)", async () => {
    if (bridge.mode === "sim") {
      const s = await bridge.cluster.status("g");
      expect(s.online).toBe(0);
      expect(await bridge.cluster.peers("g")).toEqual([]);
    }
  });
});
