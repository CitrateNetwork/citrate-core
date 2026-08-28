// CX bridge contract — groups (C-19). Pins the frozen shape (CX-S0.2).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — groups (frozen CX-S0.2)", () => {
  it("exposes the groups domain with its frozen methods", () => {
    expect(bridge.groups).toBeDefined();
    for (const m of ["create", "list", "join", "addMember", "roster", "assignRole", "offboard", "send", "messages"] as const) {
      expect(typeof bridge.groups[m]).toBe("function");
    }
  });
  it("sim is honest-empty (no fabricated rooms/messages, Rule 1)", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.groups.list()).toEqual([]);
      expect(await bridge.groups.messages("g")).toEqual([]);
    }
  });
});
