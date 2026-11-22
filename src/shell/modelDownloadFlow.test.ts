// The store side of the onboarding model download: a pause is shown as paused (not an error), a
// second start attaches to the running download instead of failing it, the smaller model pauses
// the default first, and a download that verified is not hashed again.
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";

afterEach(() => {
  vi.restoreAllMocks();
});

const settle = () => new Promise((r) => setTimeout(r, 0));

describe("model download flow", () => {
  it("a paused download reads paused, with its bytes, and is not an error", async () => {
    vi.spyOn(bridge.model, "download").mockRejectedValue("The download is paused. Resume continues where it stopped.");
    vi.spyOn(bridge.model, "status").mockResolvedValue({ state: "paused", downloadedBytes: 1000, totalBytes: 4000, pct: 25 });
    const toast = vi.spyOn(store, "toast").mockImplementation(() => {});
    store.startModelDownload();
    await settle();
    await settle();
    expect(store.state.modelState).toBe("paused");
    expect(store.state.modelDownloadedBytes).toBe(1000);
    expect(store.state.modelError).toBeNull();
    expect(toast).not.toHaveBeenCalled();
  });

  it("a second start while one runs attaches to it: only the newest outcome is shown", async () => {
    let finishFirst: () => void = () => {};
    const dl = vi
      .spyOn(bridge.model, "download")
      .mockImplementationOnce(() => new Promise<void>((r) => (finishFirst = r)))
      .mockResolvedValueOnce(undefined);
    vi.spyOn(bridge.model, "status").mockResolvedValue({ state: "ready" });
    const verify = vi.spyOn(bridge.model, "verify").mockResolvedValue(undefined);
    vi.spyOn(store, "toast").mockImplementation(() => {});
    vi.spyOn(store, "serveLocalAndRoute").mockResolvedValue(undefined as never);
    store.startModelDownload();
    store.startModelDownload(); // "Retry" / a relaunch while the first runs
    await settle();
    await settle();
    finishFirst();
    await settle();
    expect(dl).toHaveBeenCalledTimes(2);
    expect(store.state.modelState).toBe("ready");
    expect(store.state.modelError).toBeNull();
    expect(verify).not.toHaveBeenCalled(); // the download call verified; no second hash
  });

  it("choosing the smaller model pauses the running default, then downloads the small one", async () => {
    const calls: string[] = [];
    vi.spyOn(bridge.model, "download").mockImplementation(async (m?: "default" | "small") => {
      calls.push(`download:${m ?? "chosen"}`);
      if (!m) await new Promise(() => {}); // the default keeps running
    });
    vi.spyOn(bridge.model, "pause").mockImplementation(async () => {
      calls.push("pause");
    });
    // The backend records the new choice; options() reads it back.
    vi.spyOn(bridge.model, "options").mockResolvedValue({ chosen: "small", options: [], freeBytes: null, marginBytes: 0 });
    vi.spyOn(bridge.model, "status").mockResolvedValue({ state: "ready" });
    vi.spyOn(store, "toast").mockImplementation(() => {});
    vi.spyOn(store, "serveLocalAndRoute").mockResolvedValue(undefined as never);
    store.startModelDownload();
    store.chooseSmallerModel();
    await settle();
    await settle();
    expect(calls, JSON.stringify(calls)).toEqual(["download:chosen", "pause", "download:small"]);
    expect(store.state.modelChoice).toBe("small");
  });
});
