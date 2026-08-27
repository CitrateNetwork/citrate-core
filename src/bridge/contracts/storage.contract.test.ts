// CX bridge contract — storage (C-17). Pins the frozen shape (CX-S0.2). See models.contract for rationale.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { bridge } from "../index";

// Mock the invoke boundary so the tauri impl can be exercised in the vitest env (CX-S2.3).
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriStorage } from "../tauri/storage";

describe("CX bridge contract — storage (frozen CX-S0.2)", () => {
  it("exposes the storage domain with its frozen methods", () => {
    expect(bridge.storage).toBeDefined();
    for (const m of ["add", "pin", "list", "retrieve", "unpin"] as const) {
      expect(typeof bridge.storage[m]).toBe("function");
    }
  });
  it("sim is honest-empty (no fabricated pins, Rule 1)", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.storage.list()).toEqual([]);
    }
  });
});

describe("CX bridge — tauri storage invokes the frozen kubo-seam commands (CX-S2.3)", () => {
  beforeEach(() => invokeMock.mockReset());

  it("add → storage_add with { path }", async () => {
    invokeMock.mockResolvedValueOnce({ cid: "bafy1", sizeBytes: 12 });
    const out = await tauriStorage.add("/tmp/notes.txt");
    expect(invokeMock).toHaveBeenCalledWith("storage_add", { path: "/tmp/notes.txt" });
    expect(out).toEqual({ cid: "bafy1", sizeBytes: 12 });
  });

  it("pin → storage_pin with { cid, bondSalt }", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await tauriStorage.pin("bafy1", "local");
    expect(invokeMock).toHaveBeenCalledWith("storage_pin", { cid: "bafy1", bondSalt: "local" });
  });

  it("list → storage_list", async () => {
    invokeMock.mockResolvedValueOnce([]);
    await tauriStorage.list();
    expect(invokeMock).toHaveBeenCalledWith("storage_list");
  });

  it("retrieve → storage_retrieve with { cid } and wraps the path", async () => {
    invokeMock.mockResolvedValueOnce("/data/ipfs/retrieved/bafy1");
    const out = await tauriStorage.retrieve("bafy1");
    expect(invokeMock).toHaveBeenCalledWith("storage_retrieve", { cid: "bafy1" });
    expect(out).toEqual({ path: "/data/ipfs/retrieved/bafy1" });
  });

  it("unpin → storage_unpin with { cid }", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await tauriStorage.unpin("bafy1");
    expect(invokeMock).toHaveBeenCalledWith("storage_unpin", { cid: "bafy1" });
  });
});
