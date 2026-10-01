// HUP-S1.6 bridge contract — tier (hardware tier recommendation + persisted override).
import { describe, it, expect, vi, beforeEach } from "vitest";
import { bridge } from "../index";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriTier } from "../tauri/tier";

describe("bridge contract — tier (HUP-S1.6)", () => {
  it("exposes recommend + setOverride", () => {
    expect(typeof bridge.tier.recommend).toBe("function");
    expect(typeof bridge.tier.setOverride).toBe("function");
  });

  it("sim mode never fabricates hardware (Rule 1): recommend is null, override needs the app", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.tier.recommend()).toBeNull();
      await expect(bridge.tier.setOverride("T1")).rejects.toThrow(/desktop app/);
    }
  });
});

describe("tauri tier invokes the registered commands", () => {
  beforeEach(() => invokeMock.mockReset());

  it("recommend → tier_recommend (no args)", async () => {
    invokeMock.mockResolvedValueOnce(null);
    await tauriTier.recommend();
    expect(invokeMock).toHaveBeenCalledWith("tier_recommend");
  });

  it("setOverride → tier_set_override with { tier } (null clears)", async () => {
    invokeMock.mockResolvedValueOnce("T2");
    expect(await tauriTier.setOverride("T2")).toBe("T2");
    expect(invokeMock).toHaveBeenCalledWith("tier_set_override", { tier: "T2" });
    invokeMock.mockResolvedValueOnce(null);
    await tauriTier.setOverride(null);
    expect(invokeMock).toHaveBeenLastCalledWith("tier_set_override", { tier: null });
  });
});
