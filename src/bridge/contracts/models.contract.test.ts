// CX bridge contract test — modelsCatalog (C-16). Pins the FROZEN interface shape (CX-S0.2).
// If a feature branch changes this DTO/method surface, this fails at merge and forces a
// serialized spine-PR instead of a silent divergence that would race another lane (01 §5.4).
import { describe, it, expect, vi, beforeEach } from "vitest";
import { bridge } from "../index";

// Mock the invoke boundary so the tauri impl can be exercised in the vitest env (CX-S1.6).
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriModelsCatalog } from "../tauri/models";

describe("CX bridge contract — modelsCatalog (frozen CX-S0.2)", () => {
  it("exposes the modelsCatalog domain with exactly its frozen methods", () => {
    expect(bridge.modelsCatalog).toBeDefined();
    for (const m of ["local", "search", "download", "select"] as const) {
      expect(typeof bridge.modelsCatalog[m]).toBe("function");
    }
  });

  it("sim mode is honest-empty — no fabricated catalog (Rule 1)", async () => {
    // The vitest env runs the bridge in sim mode; the tauri impl throws Unavailable instead.
    if (bridge.mode === "sim") {
      expect(await bridge.modelsCatalog.local()).toEqual([]);
      expect(await bridge.modelsCatalog.search("hf", "q")).toEqual([]);
      await expect(bridge.modelsCatalog.download("x")).resolves.toBeUndefined();
      await expect(bridge.modelsCatalog.select("x")).resolves.toBeUndefined();
    }
  });

  it("the composed bridge still carries every legacy domain (no regression from the CX spread)", () => {
    for (const d of ["config", "wallet", "node", "model", "memory", "chat", "comms", "connections"] as const) {
      expect(bridge[d]).toBeDefined();
    }
  });
});

describe("CX bridge — tauri modelsCatalog invokes the frozen commands (CX-S1.6)", () => {
  beforeEach(() => invokeMock.mockReset());

  it("local → model_catalog_local (no args)", async () => {
    invokeMock.mockResolvedValueOnce([]);
    await tauriModelsCatalog.local();
    expect(invokeMock).toHaveBeenCalledWith("model_catalog_local");
  });

  it("search → model_catalog_search with { source, query }", async () => {
    invokeMock.mockResolvedValueOnce([]);
    await tauriModelsCatalog.search("hf", "gemma");
    expect(invokeMock).toHaveBeenCalledWith("model_catalog_search", { source: "hf", query: "gemma" });
  });

  it("download → model_catalog_download with { id }", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await tauriModelsCatalog.download("hf:acme/cool-gguf/m.gguf");
    expect(invokeMock).toHaveBeenCalledWith("model_catalog_download", { id: "hf:acme/cool-gguf/m.gguf" });
  });

  it("select → model_catalog_select with { id }", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await tauriModelsCatalog.select("github:owner/repo/m.gguf@v1.0");
    expect(invokeMock).toHaveBeenCalledWith("model_catalog_select", { id: "github:owner/repo/m.gguf@v1.0" });
  });
});
