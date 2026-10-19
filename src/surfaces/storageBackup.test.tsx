// K4 — Backup and restore UI states: progress, the export report, a verified folder, and a refused
// restore (manifest mismatch). Rust does the work; these pin what the member is shown.
import { describe, it, expect, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { StorageBackup } from "./StorageBackup";
import { storageSlice } from "../shell/slices/storage";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";
import type { BackupVerifyReport } from "../bridge/domains";

const stubStore = { toast: () => {} } as unknown as Store;
const render = () => renderToStaticMarkup(<StorageBackup store={stubStore} s={freshState("p1")} />);

const OK_CHECK: BackupVerifyReport = {
  folder: "/Users/m/Backups/citrate-backup-20261019-120000",
  ok: true,
  createdAt: 1_792_411_200,
  parts: ["files", "memory"],
  fileCount: 7,
  totalBytes: 4_200_000,
  mismatches: [],
  problem: null,
};

afterEach(() => {
  storageSlice.set({ backupOp: null, backupProgress: null, lastExport: null, restoreCheck: null, lastRestore: null, backupError: null });
});

describe("K4 Backup — export", () => {
  it("explains what is exported and that the memory store is not encrypted by the app", () => {
    const html = render();
    expect(html).toContain("Export a backup");
    expect(html).toContain("SHA-256 of every file");
    expect(html).toContain("isn&#x27;t encrypted by the app");
  });

  it("shows progress while exporting", () => {
    storageSlice.set({ backupOp: "export", backupProgress: { op: "export", phase: "files", done: 1, total: 2, item: "files/bafy1" } });
    const html = render();
    expect(html).toContain("Exporting · Files 1 of 2 · 50%");
    expect(html).toContain("files/bafy1");
  });

  it("reports the written folder, counts, the manifest digest and anything skipped", () => {
    storageSlice.set({
      lastExport: {
        folder: "/Users/m/Backups/citrate-backup-20261019-120000",
        createdAt: 1,
        parts: ["files", "memory"],
        files: 2,
        memoryFiles: 5,
        totalBytes: 4_200_000,
        entriesSha256: "ab".repeat(32),
        skipped: [{ item: "bafygone", reason: "not pinned on this node" }],
        daemonRestarted: true,
        notes: [],
      },
    });
    const html = render();
    expect(html).toContain("Backup written and checked against its manifest.");
    expect(html).toContain("citrate-backup-20261019-120000");
    expect(html).toContain("2 files · 5 memory store files · 4 MB");
    expect(html).toContain("ab".repeat(32));
    expect(html).toContain("skipped bafygone: not pinned on this node");
  });
});

describe("K4 Backup — restore", () => {
  it("a matching folder shows what it holds and offers Restore", () => {
    storageSlice.set({ restoreCheck: OK_CHECK });
    const html = render();
    expect(html).toContain("Matches its manifest: 7 files, 4 MB");
    expect(html).toContain("data-testid=\"restore-check-ok\"");
    expect(html).toContain("Restore ");
    expect(html).not.toContain("Restore refused");
  });

  it("a mismatching folder is refused, each problem named, and no Restore button", () => {
    storageSlice.set({
      restoreCheck: {
        ...OK_CHECK,
        ok: false,
        mismatches: [
          { kind: "hashDiffers", path: "memory/store.bge.memdag/000123.sst" },
          { kind: "missing", path: "files/bafy1" },
          { kind: "unlisted", path: "memory/planted.bin" },
        ],
      },
    });
    const html = render();
    expect(html).toContain("Restore refused: this folder doesn&#x27;t match its manifest.");
    expect(html).toContain("memory/store.bge.memdag/000123.sst: content differs from the manifest (sha256)");
    expect(html).toContain("files/bafy1: missing from the folder");
    expect(html).toContain("memory/planted.bin: not listed in the manifest");
    expect(html).not.toContain("data-testid=\"restore-check-ok\"");
  });

  it("an unusable manifest is refused with its reason", () => {
    storageSlice.set({ restoreCheck: { ...OK_CHECK, ok: false, problem: "the backup manifest can't be used: no citrate-backup.json in that folder" } });
    const html = render();
    expect(html).toContain("Restore refused");
    expect(html).toContain("no citrate-backup.json in that folder");
  });

  it("a refusal from Rust at restore time is shown as a refusal", () => {
    storageSlice.set({ backupError: "the backup does not match its manifest (1 problem): memory/a: content differs" });
    const html = render();
    expect(html).toContain("Restore refused.");
    expect(html).toContain("memory/a: content differs");
  });

  it("the restore report says where the previous memory store was kept and any CID change", () => {
    storageSlice.set({
      lastRestore: {
        folder: OK_CHECK.folder,
        parts: ["files", "memory"],
        skipped: [],
        filesRestored: 2,
        cidsChanged: [{ expected: "QmOld", actual: "bafyNew" }],
        memoryFilesRestored: 5,
        previousMemoryKeptAt: "/data/memory.before-restore-20261019-120500",
        daemonRestarted: true,
        notes: [],
      },
    });
    const html = render();
    expect(html).toContain("Restored Files and Memory and checked.");
    expect(html).toContain("2 files · 5 memory store files");
    expect(html).toContain("memory.before-restore-20261019-120500");
    expect(html).toContain("QmOld came back as bafyNew");
  });

  it("the restore report lists only what was restored and names each skipped part with its reason", () => {
    storageSlice.set({
      lastRestore: {
        folder: OK_CHECK.folder,
        parts: ["files"],
        skipped: [{ item: "memory", reason: "not in this backup (the export skipped it: there is no memory store on this machine yet)" }],
        filesRestored: 1,
        cidsChanged: [],
        memoryFilesRestored: 0,
        previousMemoryKeptAt: null,
        daemonRestarted: false,
        notes: [],
      },
    });
    const html = render();
    expect(html).toContain("Restored Files and checked.");
    expect(html).not.toContain("Restored Files and Memory");
    expect(html).toContain("Memory not restored: not in this backup (the export skipped it: there is no memory store on this machine yet)");
    expect(html).not.toContain("memory store file");
  });

  it("the export report names a skipped part instead of counting it as exported", () => {
    storageSlice.set({
      lastExport: {
        folder: "/b/citrate-backup-1",
        createdAt: 1,
        parts: ["files"],
        files: 1,
        memoryFiles: 0,
        totalBytes: 10,
        entriesSha256: "cd".repeat(32),
        skipped: [{ item: "memory", reason: "there is no memory store on this machine yet" }],
        daemonRestarted: false,
        notes: [],
      },
    });
    const html = render();
    expect(html).toContain("Backed up Files.");
    expect(html).toContain("Memory not backed up: there is no memory store on this machine yet");
    expect(html).not.toContain("memory store file");
  });

  it("a mode problem is named in the refusal", () => {
    storageSlice.set({
      restoreCheck: { ...OK_CHECK, ok: false, mismatches: [{ kind: "badMode", path: "memory/x" }] },
    });
    expect(render()).toContain("memory/x: not an allowed file mode");
  });

  it("in the web preview it says backup needs the desktop app", () => {
    const html = render();
    expect(html).toContain("Backup and restore need the desktop app");
  });
});
