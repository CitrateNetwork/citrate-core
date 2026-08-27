// CX bridge contract — storage (C-17). Pins the frozen shape (CX-S0.2). See models.contract for rationale.
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — storage (frozen CX-S0.2)", () => {
  it("exposes the storage domain with its frozen methods", () => {
    expect(bridge.storage).toBeDefined();
    for (const m of ["add", "pin", "list", "retrieve", "unpin"] as const) {
      expect(typeof bridge.storage[m]).toBe("function");
    }
  });
  it("sim is honest-empty (no fabricated pins, Rule 1)", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.storage.list()).toEqual([]);
    }
  });
});
