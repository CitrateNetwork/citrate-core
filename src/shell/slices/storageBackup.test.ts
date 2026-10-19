// K4 — storage slice: local pins never open a bond; backup/restore fold progress, reports and
// refusals into state; the parameter read keeps unknown as unknown.
import { describe, it, expect, vi, afterEach } from "vitest";
import { bridge } from "../../bridge";
import {
  storageSlice,
  localPin,
  refreshPinParams,
  exportBackup,
  checkBackup,
  restoreBackup,
} from "./storage";

afterEach(() => {
  vi.restoreAllMocks();
  storageSlice.set({
    pins: [],
    busyCid: null,
    error: null,
    kuboDown: false,
    pinParams: null,
    pinParamsError: null,
    backupOp: null,
    backupProgress: null,
    lastExport: null,
    restoreCheck: null,
    lastRestore: null,
    backupError: null,
  });
});

describe("K4 storage slice", () => {
  it("localPin keeps the CID on this node via pinLocal", async () => {
    const pin = vi.spyOn(bridge.storage, "pinLocal").mockResolvedValue(undefined);
    vi.spyOn(bridge.storage, "list").mockResolvedValue([]);
    await localPin("bafy1");
    expect(pin).toHaveBeenCalledWith("bafy1");
  });

  it("a failed parameter read is recorded as unavailable, not as zero values", async () => {
    vi.spyOn(bridge.storage, "pinParams").mockRejectedValue(new Error("rpc down"));
    await refreshPinParams();
    expect(storageSlice.get().pinParams).toBeNull();
    expect(storageSlice.get().pinParamsError).toBe("rpc down");
  });

  it("export streams progress into state and keeps the report", async () => {
    const seen: (string | undefined)[] = [];
    const unsub = storageSlice.subscribe(() => seen.push(storageSlice.get().backupProgress?.phase));
    vi.spyOn(bridge.storage, "backupExport").mockImplementation(async (_d, _p, on) => {
      on?.({ op: "export", phase: "files", done: 1, total: 1, item: "files/bafy1" });
      on?.({ op: "export", phase: "done", done: 1, total: 1, item: "/b/x" });
      return {
        folder: "/b/x",
        createdAt: 1,
        parts: ["files"],
        files: 1,
        memoryFiles: 0,
        totalBytes: 3,
        entriesSha256: "00",
        skipped: [],
        daemonRestarted: false,
        notes: [],
      };
    });
    await exportBackup("/b", ["files"]);
    unsub();
    expect(seen).toContain("files");
    expect(storageSlice.get().lastExport?.folder).toBe("/b/x");
    expect(storageSlice.get().backupOp).toBeNull();
  });

  it("a second operation while one runs is ignored", async () => {
    storageSlice.set({ backupOp: "export" });
    const exp = vi.spyOn(bridge.storage, "backupExport");
    await exportBackup("/b", ["files"]);
    expect(exp).not.toHaveBeenCalled();
  });

  it("a refused restore keeps the refusal and restores nothing into state", async () => {
    vi.spyOn(bridge.storage, "backupRestore").mockRejectedValue(
      new Error("the backup does not match its manifest (1 problem): memory/a: content differs"),
    );
    await restoreBackup("/b/x", ["memory"]);
    expect(storageSlice.get().lastRestore).toBeNull();
    expect(storageSlice.get().backupError).toMatch(/does not match its manifest/);
    expect(storageSlice.get().backupOp).toBeNull();
  });

  it("checkBackup stores the verification (mismatches included)", async () => {
    vi.spyOn(bridge.storage, "backupVerify").mockResolvedValue({
      folder: "/b/x",
      ok: false,
      createdAt: 1,
      parts: ["memory"],
      fileCount: 1,
      totalBytes: 1,
      mismatches: [{ kind: "missing", path: "memory/a" }],
      problem: null,
    });
    await checkBackup("/b/x");
    expect(storageSlice.get().restoreCheck?.ok).toBe(false);
    expect(storageSlice.get().restoreCheck?.mismatches).toHaveLength(1);
  });

  it("a files restore re-lists the pins", async () => {
    vi.spyOn(bridge.storage, "backupRestore").mockResolvedValue({
      folder: "/b/x",
      parts: ["files"],
      filesRestored: 1,
      cidsChanged: [],
      memoryFilesRestored: 0,
      previousMemoryKeptAt: null,
      daemonRestarted: false,
      notes: [],
    });
    const list = vi.spyOn(bridge.storage, "list").mockResolvedValue([]);
    await restoreBackup("/b/x", ["files"]);
    expect(list).toHaveBeenCalled();
    expect(storageSlice.get().lastRestore?.filesRestored).toBe(1);
  });
});
