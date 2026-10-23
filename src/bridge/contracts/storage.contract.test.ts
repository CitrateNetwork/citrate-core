// CX bridge contract — storage (C-17). Pins the frozen shape (CX-S0.2). See models.contract for rationale.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { bridge } from "../index";

// Mock the invoke boundary so the tauri impl can be exercised in the vitest env (CX-S2.3).
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
// K4: backup/restore subscribe to the Rust progress event for the duration of the call.
const { listenMock, unlistenMock } = vi.hoisted(() => ({ listenMock: vi.fn(), unlistenMock: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
import { tauriStorage } from "../tauri/storage";

describe("CX bridge contract — storage (frozen CX-S0.2)", () => {
  it("exposes the storage domain with its frozen methods", () => {
    expect(bridge.storage).toBeDefined();
    for (const m of ["add", "pinLocal", "list", "retrieve", "unpin", "pinParams", "backupExport", "backupVerify", "backupRestore"] as const) {
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

  it("pinLocal → storage_pin_local with { cid } (K4: never the bond ceremony)", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await tauriStorage.pinLocal("bafy1");
    expect(invokeMock).toHaveBeenCalledWith("storage_pin_local", { cid: "bafy1" });
    expect(invokeMock).not.toHaveBeenCalledWith("storage_pin", expect.anything());
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

describe("K4 — tauri storage: V3 parameters and verified backup", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    unlistenMock.mockReset();
    listenMock.mockResolvedValue(unlistenMock);
  });

  it("pinParams → storage_pin_params", async () => {
    invokeMock.mockResolvedValueOnce({ contract: "0xf5", bondWei: null, rewardWei: null, rounds: null, bondsLive: false, source: "s", errors: [] });
    const p = await tauriStorage.pinParams();
    expect(invokeMock).toHaveBeenCalledWith("storage_pin_params");
    expect(p.bondsLive).toBe(false);
  });

  it("backupExport → storage_backup_export with { dest, parts } and streams progress, then unlistens", async () => {
    const seen: unknown[] = [];
    listenMock.mockImplementationOnce(async (_ev: string, cb: (e: { payload: unknown }) => void) => {
      cb({ payload: { op: "export", phase: "files", done: 1, total: 2, item: "files/bafy1" } });
      cb({ payload: "not a progress object" });
      return unlistenMock;
    });
    invokeMock.mockResolvedValueOnce({ folder: "/b/citrate-backup-1" });
    await tauriStorage.backupExport("/b", ["files", "memory"], (p) => seen.push(p));
    expect(listenMock).toHaveBeenCalledWith("storage://backup-progress", expect.any(Function));
    expect(invokeMock).toHaveBeenCalledWith("storage_backup_export", { dest: "/b", parts: ["files", "memory"] });
    expect(seen).toEqual([{ op: "export", phase: "files", done: 1, total: 2, item: "files/bafy1" }]);
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("backupRestore unlistens even when Rust refuses", async () => {
    invokeMock.mockRejectedValueOnce(new Error("the backup does not match its manifest (1 problem)"));
    await expect(tauriStorage.backupRestore("/b/x", ["memory"], () => {})).rejects.toThrow(/does not match/);
    expect(invokeMock).toHaveBeenCalledWith("storage_backup_restore", { folder: "/b/x", parts: ["memory"] });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("backupVerify → storage_backup_verify with { folder }", async () => {
    invokeMock.mockResolvedValueOnce({ ok: true });
    await tauriStorage.backupVerify("/b/x");
    expect(invokeMock).toHaveBeenCalledWith("storage_backup_verify", { folder: "/b/x" });
  });

  it("sim rejects backup and parameter reads with a plain reason (no fabricated values)", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.storage.pinParams()).rejects.toThrow(/desktop app/);
      await expect(bridge.storage.backupExport("/x", ["files"])).rejects.toThrow(/desktop app/);
    }
  });
});
