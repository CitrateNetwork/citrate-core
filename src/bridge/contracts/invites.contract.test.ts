// CX bridge contract — group invites (INVITE-S2 self-admit + CONNECT-S1 claim-back).
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — invites", () => {
  it("exposes the invites domain with its methods", () => {
    expect(bridge.invites).toBeDefined();
    for (const m of [
      "create", "redeem", "list", "verifyConsume", "revoke",
      "submitClaim", "pollClaims", "referralLog", "exportReferralLog",
    ] as const) {
      expect(typeof bridge.invites[m]).toBe("function");
    }
  });
  it("sim is honest — empty lists, verifyConsume rejects, self-admit/create need the desktop (Rule 1)", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.invites.list("grp")).toEqual([]);
      expect(await bridge.invites.verifyConsume("grp", "tok")).toBe(false);
      expect(await bridge.invites.referralLog()).toEqual([]);
      expect(await bridge.invites.exportReferralLog()).toBe("[]");
      // No relay/daemon in the web preview: minting and self-admit throw honestly, never a fake success.
      await expect(bridge.invites.create("grp", "@x")).rejects.toThrow();
      await expect(bridge.invites.redeem("citrate://invite?g=grp&t=tok")).rejects.toThrow();
    }
  });
});
