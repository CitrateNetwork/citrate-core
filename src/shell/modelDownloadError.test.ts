// The first-run model step must show WHY a download failed. A Tauri command rejects with the
// Rust error as a plain string (not an Error), and the step used to replace it with
// "download failed" / "unknown", hiding the cause (e.g. "model download is not a GGUF file").
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";

afterEach(() => {
  vi.restoreAllMocks();
});

const settle = () => new Promise((r) => setTimeout(r, 0));

describe("model download failure shows the real cause", () => {
  it("a string rejection from the download command is the error shown", async () => {
    vi.spyOn(bridge.model, "status").mockResolvedValue({ state: "notPresent" });
    vi.spyOn(bridge.model, "download").mockRejectedValue("model download is not a GGUF file (bad magic)");
    const toast = vi.spyOn(store, "toast").mockImplementation(() => {});
    store.startModelDownload();
    await settle();
    expect(store.state.modelState).toBe("error");
    expect(store.state.modelError).toBe("model download is not a GGUF file (bad magic)");
    expect(toast.mock.calls.at(-1)?.[0]).toContain("model download is not a GGUF file (bad magic)");
  });

  it("a string rejection from the verify command is the error shown", async () => {
    vi.spyOn(bridge.model, "status").mockResolvedValue({ state: "notPresent" });
    vi.spyOn(bridge.model, "verify").mockRejectedValue("model checksum mismatch — file quarantined (not ready)");
    vi.spyOn(store, "toast").mockImplementation(() => {});
    await store.verifyModel();
    expect(store.state.modelState).toBe("error");
    expect(store.state.modelError).toBe("model checksum mismatch — file quarantined (not ready)");
  });
});
