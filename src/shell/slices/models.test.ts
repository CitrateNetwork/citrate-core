// HUP-S0.3 — interrupted catalog downloads survive an app restart as "Resume" rows.
import { describe, it, expect, vi, afterEach } from "vitest";
import { bridge } from "../../bridge";
import { modelsSlice, refreshLocalModels, downloadModel } from "./models";

afterEach(() => {
  vi.restoreAllMocks();
  modelsSlice.set({ partials: [], downloadingId: null, downloadPct: null, error: null });
});

describe("HUP-S0.3 resumable downloads", () => {
  it("refreshLocalModels also loads the interrupted downloads", async () => {
    vi.spyOn(bridge.modelsCatalog, "local").mockResolvedValue([]);
    vi.spyOn(bridge.modelsCatalog, "partials").mockResolvedValue([
      { id: "hf:org/repo/q.gguf", file: "q.gguf", downloadedBytes: 10, totalBytes: 40, pct: 25 },
    ]);
    await refreshLocalModels();
    expect(modelsSlice.get().partials).toHaveLength(1);
    expect(modelsSlice.get().partials[0].pct).toBe(25);
  });

  it("a failed partials read is an honest empty list, never an error banner", async () => {
    vi.spyOn(bridge.modelsCatalog, "local").mockResolvedValue([]);
    vi.spyOn(bridge.modelsCatalog, "partials").mockRejectedValue(new Error("unwired"));
    await refreshLocalModels();
    expect(modelsSlice.get().partials).toEqual([]);
    expect(modelsSlice.get().error).toBeNull();
  });

  it("resuming = downloading the same id; the finished model leaves the resume list", async () => {
    const partials = vi
      .spyOn(bridge.modelsCatalog, "partials")
      .mockResolvedValueOnce([{ id: "hf:x", file: "x.gguf", downloadedBytes: 1, totalBytes: 2, pct: 50 }])
      .mockResolvedValue([]);
    vi.spyOn(bridge.modelsCatalog, "local").mockResolvedValue([]);
    const dl = vi.spyOn(bridge.modelsCatalog, "download").mockResolvedValue(undefined);
    await refreshLocalModels();
    expect(modelsSlice.get().partials).toHaveLength(1);
    await downloadModel("hf:x");
    expect(dl).toHaveBeenCalledWith("hf:x", expect.any(Function));
    expect(partials).toHaveBeenCalledTimes(2);
    expect(modelsSlice.get().partials).toEqual([]);
  });
});

describe("HUP-S0.3b gated Hugging Face repos", () => {
  it("surfaces the backend's gated-repo message verbatim (where to add the token)", async () => {
    // The Rust side (hf_auth.rs FetchError::Gated) owns this text; the slice must not mangle it.
    const gated =
      "This model needs a Hugging Face token with access. Add it in Settings › Connections " +
      "(connect Hugging Face), and accept the model's terms on its Hugging Face page if it asks.";
    vi.spyOn(bridge.modelsCatalog, "download").mockRejectedValue(gated);
    vi.spyOn(bridge.modelsCatalog, "partials").mockResolvedValue([]);
    await downloadModel("hf:org/gated/m.gguf");
    expect(modelsSlice.get().error).toBe(gated);
    expect(modelsSlice.get().downloadingId).toBeNull();
  });
});
