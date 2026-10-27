// CX bridge contract — group invites (D-12: one flow, request and approve).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — invites", () => {
  it("exposes exactly the request-and-approve methods", () => {
    expect(Object.keys(bridge.invites).sort()).toEqual(
      ["approve", "create", "decline", "dismiss", "list", "myRequests", "preview", "request", "requests", "revoke"],
    );
  });
  it("sim is honest — nothing is made, sent or approved without the desktop app (Rule 1)", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.invites.list("grp")).toEqual([]);
      expect(await bridge.invites.myRequests()).toEqual([]);
      await expect(bridge.invites.create("grp", "", "")).rejects.toThrow(/desktop app/);
      await expect(bridge.invites.requests("grp")).rejects.toThrow(/desktop app/);
      await expect(bridge.invites.approve("grp", "id", "0xa")).rejects.toThrow(/desktop app/);
      await expect(bridge.invites.request("citrate://invite?g=grp")).rejects.toThrow(/desktop app/);
    }
  });
});
