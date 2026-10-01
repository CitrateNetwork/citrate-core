// HUP-S1.6 — the tier slice: load the local recommendation, persist an override, and say which
// model files match the tier in effect.
import { describe, it, expect, vi, afterEach } from "vitest";
import { bridge } from "../../bridge";
import { sampleReport } from "./tierTestReport";
import { tierSlice, refreshTier, setTierOverride, effectiveProfile, isRecommendedModel, normalizeModelKey } from "./tier";

afterEach(() => {
  vi.restoreAllMocks();
  tierSlice.set({ report: null, loaded: false, saving: false, error: null });
});

describe("HUP-S1.6 tier slice", () => {
  it("refreshTier loads the real report", async () => {
    vi.spyOn(bridge.tier, "recommend").mockResolvedValue(sampleReport());
    await refreshTier();
    expect(tierSlice.get().report?.effective).toBe("T2");
    expect(tierSlice.get().loaded).toBe(true);
  });

  it("a failed probe is an honest null + message, never a made-up tier", async () => {
    vi.spyOn(bridge.tier, "recommend").mockRejectedValue(new Error("probe failed"));
    await refreshTier();
    expect(tierSlice.get().report).toBeNull();
    expect(tierSlice.get().error).toBe("probe failed");
    expect(tierSlice.get().loaded).toBe(true);
  });

  it("setTierOverride persists through the bridge and updates the effective tier", async () => {
    tierSlice.set({ report: sampleReport(), loaded: true });
    const set = vi.spyOn(bridge.tier, "setOverride").mockResolvedValue("T0");
    await setTierOverride("T0");
    expect(set).toHaveBeenCalledWith("T0");
    expect(tierSlice.get().report?.overrideTier).toBe("T0");
    expect(tierSlice.get().report?.effective).toBe("T0");
    expect(effectiveProfile(tierSlice.get().report)?.ctxTokens).toBe(16384);
  });

  it("clearing the override falls back to the recommendation", async () => {
    tierSlice.set({ report: sampleReport({ overrideTier: "T0", effective: "T0" }), loaded: true });
    vi.spyOn(bridge.tier, "setOverride").mockResolvedValue(null);
    await setTierOverride(null);
    expect(tierSlice.get().report?.effective).toBe("T2");
  });

  it("a failed save keeps the previous choice and surfaces the error", async () => {
    tierSlice.set({ report: sampleReport(), loaded: true });
    vi.spyOn(bridge.tier, "setOverride").mockRejectedValue(new Error("store locked"));
    await setTierOverride("T1");
    expect(tierSlice.get().report?.effective).toBe("T2");
    expect(tierSlice.get().error).toBe("store locked");
  });

  it("normalizes model filenames the same way the Rust side does", () => {
    expect(normalizeModelKey("Qwen3.6-27B-Q4_K_M.gguf")).toBe("qwen3627bq4kmgguf");
    expect(normalizeModelKey("gemma-4-E4B-it-Q4_0.gguf")).toBe("gemma4e4bitq40gguf");
  });

  it("isRecommendedModel matches only the effective tier's model family", () => {
    const rep = sampleReport();
    expect(isRecommendedModel("Qwen3.6-35B-A3B-Q4_K_M.gguf", rep)).toBe(true);
    expect(isRecommendedModel("Qwen3.6-27B-Q4_K_M.gguf", rep)).toBe(false);
    expect(isRecommendedModel("gemma-4-E4B-it-Q4_0.gguf", sampleReport({ effective: "T0" }))).toBe(true);
    expect(isRecommendedModel("anything.gguf", null)).toBe(false);
  });
});
