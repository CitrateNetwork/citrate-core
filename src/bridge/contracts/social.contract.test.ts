// CX bridge contract — social identity (Connections · social discovery). ADR-2026-08-30.
import { describe, it, expect } from "vitest";
import { bridge } from "../index";

describe("CX bridge contract — social (ADR-2026-08-30)", () => {
  it("exposes the social domain with its methods", () => {
    expect(bridge.social).toBeDefined();
    for (const m of [
      "status", "start", "verifyRequest", "verifyApprove", "verifyForget", "setVisibility",
      "disconnect", "resolve", "exportBinding", "ingestBinding",
      "directoryPublishRequest", "directoryPublishApprove", "directoryUnpublishRequest",
      "directoryUnpublishApprove", "directoryForget", "directoryLookup", "directorySearch",
    ] as const) {
      expect(typeof bridge.social[m]).toBe("function");
    }
  });

  it("status() + resolve() are honest-empty; sim ingest never accepts a fabricated binding (Rule 1)", async () => {
    expect(await bridge.social.status()).toEqual([]);
    expect(await bridge.social.resolve(["0xabc"])).toEqual([]);
    if (bridge.mode === "sim") {
      expect(await bridge.social.exportBinding("discord")).toBeNull();
      expect(await bridge.social.ingestBinding("0xabc", { network: "discord", handle: "x", address: "0xabc", nonce: "n", signature: "0x" })).toBe(false);
      // #61 — the directory never fabricates a resolution in the web preview (D-7 / Rule 1).
      expect(await bridge.social.directoryLookup("x", "someone")).toBeNull();
      expect(await bridge.social.directorySearch("x", "some")).toEqual([]);
    }
  });

  it("sim linking honestly requires the desktop app (never a fake link)", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.social.start("discord")).rejects.toThrow(/desktop app/i);
    }
  });
});
