// CX bridge impl — storage (C-17), TAURI. Owned by lane s2 (CX-S2).
//
// S2.3: real invokes of the S2.1 kubo seam commands. K4: pins are LOCAL only (`storage_pin_local`,
// "Local pin · no bond"); the ceremony-gated on-chain bond (S2.2) is blocked on the chain team
// (docs/FINDING_PIN_COMMD_BOND_2026-08-26.md) and IPFSIncentivesV3 is deployed but dormant, so no
// surface opens a bond. `pinParams` reads the V3 parameters only. Backup/restore drive the K4
// `storage_backup_*` commands and stream their progress event.
import { listen } from "@tauri-apps/api/event";
import { invoke } from "./invoke";
import type {
  BackupExportReport,
  BackupProgress,
  BackupRestoreReport,
  BackupVerifyReport,
  PinParams,
  PinRow,
  StorageDomain,
} from "../domains";

/** Mirrors `storage_backup::PROGRESS_EVENT` in Rust. */
export const BACKUP_PROGRESS_EVENT = "storage://backup-progress";

/** Run `run` with the progress event subscribed; always unlisten, also when it rejects. */
async function withProgress<T>(onProgress: ((p: BackupProgress) => void) | undefined, run: () => Promise<T>): Promise<T> {
  const unlisten = onProgress
    ? await listen<BackupProgress>(BACKUP_PROGRESS_EVENT, (ev) => {
        if (ev.payload && typeof ev.payload === "object" && "phase" in ev.payload) onProgress(ev.payload);
      })
    : null;
  try {
    return await run();
  } finally {
    unlisten?.();
  }
}

export const tauriStorage: StorageDomain = {
  add(path) {
    return invoke<{ cid: string; sizeBytes: number }>("storage_add", { path });
  },
  async pinLocal(cid) {
    await invoke("storage_pin_local", { cid });
  },
  list() {
    return invoke<PinRow[]>("storage_list");
  },
  async retrieve(cid) {
    // The Rust command returns the on-disk path string; the domain shape wraps it.
    const path = await invoke<string>("storage_retrieve", { cid });
    return { path };
  },
  async unpin(cid) {
    await invoke("storage_unpin", { cid });
  },
  pinParams() {
    return invoke<PinParams>("storage_pin_params");
  },
  backupExport(dest, parts, onProgress) {
    return withProgress(onProgress, () => invoke<BackupExportReport>("storage_backup_export", { dest, parts }));
  },
  backupVerify(folder) {
    return invoke<BackupVerifyReport>("storage_backup_verify", { folder });
  },
  backupRestore(folder, parts, onProgress) {
    return withProgress(onProgress, () => invoke<BackupRestoreReport>("storage_backup_restore", { folder, parts }));
  },
};
