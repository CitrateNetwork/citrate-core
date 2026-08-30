// CX bridge contract — group claimable invites (ADR-2026-08-30 D4).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — invites (ADR D4)", () => {
  it("exposes the invites domain with its methods", () => {
    expect(bridge.invites).toBeDefined();
    for (const m of ["create", "list", "verifyConsume", "revoke"] as const) {
      expect(typeof bridge.invites[m]).toBe("function");
    }
  });
  it("sim is honest — no pending invites, verifyConsume rejects (Rule 1)", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.invites.list("grp")).toEqual([]);
      expect(await bridge.invites.verifyConsume("grp", "tok")).toBe(false);
    }
  });
});
