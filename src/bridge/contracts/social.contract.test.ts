// CX bridge contract — social identity (Connections · social discovery). ADR-2026-08-30.
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — social (ADR-2026-08-30)", () => {
  it("exposes the social domain with its methods", () => {
    expect(bridge.social).toBeDefined();
    for (const m of ["status", "start", "bindingChallenge", "verify", "setVisibility", "disconnect"] as const) {
      expect(typeof bridge.social[m]).toBe("function");
    }
  });

  it("status() is honest-empty — no fabricated links (Rule 1)", async () => {
    expect(await bridge.social.status()).toEqual([]);
  });

  it("sim linking honestly requires the desktop app (never a fake link)", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.social.start("discord")).rejects.toThrow(/desktop app/i);
    }
  });
});
