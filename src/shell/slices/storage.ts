// =====================================================================
// citrate-core — storage slice (CX-S2.3, lane s2)
//
// State for the Files surface: the pin list, transient busy/error flags. Actions call
// bridge.storage (the S2.1 kubo seam) and fold results in. Errors are CAUGHT into `error`,
// never thrown at render. S2.3 pins LOCALLY (to this node's kubo) — the network-wide
// ceremony-gated SALT bond is S2.2, blocked on the chain CommD fix (FINDING_PIN_COMMD_BOND).
// K4: every pin is LOCAL ("Local pin · no bond", via `pinLocal`); the IPFSIncentivesV3 parameters
// are read for display only; backup/restore state (progress, reports, refusals) lives here too.
//
// Owned entirely by lane s2 — separate from the legacy `class Store`, so no shared-state race.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type {
  BackupExportReport,
  BackupPart,
  BackupProgress,
  BackupRestoreReport,
  BackupVerifyReport,
  PinParams,
  PinRow,
} from "../../bridge/domains";

/** The marker recorded as the "bond" for a locally-pinned file until real on-chain bonds land. */
export const LOCAL_PIN_MARKER = "local";

export interface StorageState {
  /** The pinning file store (index ∪ live pins), newest first. */
  pins: PinRow[];
  /** A file add is in flight (drag-drop / add). */
  adding: boolean;
  /** The cid of a per-row action in flight (pin/unpin/retrieve), or null. */
  busyCid: string | null;
  /** The last retrieved file's local path (for a "saved to…" confirmation), or null. */
  lastRetrievedPath: string | null;
  /** The IPFS (kubo) daemon is unreachable — a backend OUTAGE, shown as an honest card, not a raw dump. */
  kuboDown: boolean;
  /** The last user-facing error, or null when clear. */
  error: string | null;
  /** K4: the IPFSIncentivesV3 parameters, or null before the first read. */
  pinParams: PinParams | null;
  /** K4: why the parameter read failed as a whole (bridge or RPC unreachable), or null. */
  pinParamsError: string | null;
  /** K4: the backup operation in flight, or null. */
  backupOp: "export" | "verify" | "restore" | null;
  /** K4: the latest progress step of the running export/restore. */
  backupProgress: BackupProgress | null;
  /** K4: the last export's report. */
  lastExport: BackupExportReport | null;
  /** K4: the folder chosen for restore and its verification. */
  restoreCheck: BackupVerifyReport | null;
  /** K4: the last restore's report. */
  lastRestore: BackupRestoreReport | null;
  /** K4: the last backup/restore error (a restore refusal is one), or null. */
  backupError: string | null;
}

const initial: StorageState = {
  pins: [],
  adding: false,
  busyCid: null,
  lastRetrievedPath: null,
  kuboDown: false,
  error: null,
  pinParams: null,
  pinParamsError: null,
  backupOp: null,
  backupProgress: null,
  lastExport: null,
  restoreCheck: null,
  lastRestore: null,
  backupError: null,
};

export const storageSlice = createSlice<StorageState>(initial);

const message = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === "string" ? e : String(e);

// A kubo/IPFS transport failure (daemon down, 500, refused) is a backend outage — the surface
// shows an honest "unreachable · retry" card for it, never a raw 500/stack. Other errors keep
// their specific text. Case-insensitive match on the shapes kubo failures actually produce.
const isKuboDown = (msg: string): boolean =>
  /\b500\b|transport|connection refused|econnrefused|fetch failed|:5001|refused|unreachable|timed?\s*out|network error|failed to fetch|kubo|ipfs/i.test(msg);

/** Load the pinning file store. Honest-empty on a sim/down-daemon bridge. */
export async function refreshPins(): Promise<void> {
  try {
    const pins = await bridge.storage.list();
    storageSlice.set({ pins, error: null, kuboDown: false });
  } catch (e) {
    const m = message(e);
    storageSlice.set({ error: m, kuboDown: isKuboDown(m) });
  }
}

/** Add a local file (by path) to IPFS, then refresh the list so it appears. */
export async function addFile(path: string): Promise<void> {
  storageSlice.set({ adding: true, error: null });
  try {
    await bridge.storage.add(path);
    storageSlice.set({ adding: false });
    await refreshPins();
  } catch (e) {
    const m = message(e);
    storageSlice.set({ adding: false, error: m, kuboDown: isKuboDown(m) });
  }
}

/** Pin a CID LOCALLY (to this node): "Local pin · no bond". Never opens a bond ceremony. */
export async function localPin(cid: string): Promise<void> {
  storageSlice.set({ busyCid: cid, error: null });
  try {
    await bridge.storage.pinLocal(cid);
    storageSlice.set({ busyCid: null });
    await refreshPins();
  } catch (e) {
    const m = message(e);
    storageSlice.set({ busyCid: null, error: m, kuboDown: isKuboDown(m) });
  }
}

/** Release a CID's pin, then refresh. */
export async function unpinCid(cid: string): Promise<void> {
  storageSlice.set({ busyCid: cid, error: null });
  try {
    await bridge.storage.unpin(cid);
    storageSlice.set({ busyCid: null });
    await refreshPins();
  } catch (e) {
    const m = message(e);
    storageSlice.set({ busyCid: null, error: m, kuboDown: isKuboDown(m) });
  }
}

/** Retrieve a CID's bytes to a local file; records the path for a confirmation. */
export async function retrieveCid(cid: string): Promise<void> {
  storageSlice.set({ busyCid: cid, error: null });
  try {
    const { path } = await bridge.storage.retrieve(cid);
    storageSlice.set({ busyCid: null, lastRetrievedPath: path });
  } catch (e) {
    const m = message(e);
    storageSlice.set({ busyCid: null, error: m, kuboDown: isKuboDown(m) });
  }
}

/** K4 — start the bundled IPFS (kubo) daemon under the supervisor, then re-list. The daemon is
 *  otherwise started alongside the node; this lets Files work without starting the node. */
export async function startFileStore(): Promise<void> {
  try {
    await bridge.node.startIpfs();
    await refreshPins();
  } catch (e) {
    const m = message(e);
    storageSlice.set({ error: m, kuboDown: true });
  }
}

/** K4 — read the IPFSIncentivesV3 BOND / REWARD / ROUNDS parameters (display only). */
export async function refreshPinParams(): Promise<void> {
  try {
    const pinParams = await bridge.storage.pinParams();
    storageSlice.set({ pinParams, pinParamsError: null });
  } catch (e) {
    storageSlice.set({ pinParams: null, pinParamsError: message(e) });
  }
}

const onBackupProgress = (p: BackupProgress) => storageSlice.set({ backupProgress: p });

/** K4 — export the chosen parts into a new verified backup folder inside `dest`. */
export async function exportBackup(dest: string, parts: BackupPart[]): Promise<void> {
  if (storageSlice.get().backupOp) return;
  storageSlice.set({ backupOp: "export", backupProgress: null, backupError: null, lastExport: null });
  try {
    const lastExport = await bridge.storage.backupExport(dest, parts, onBackupProgress);
    storageSlice.set({ backupOp: null, lastExport });
  } catch (e) {
    storageSlice.set({ backupOp: null, backupError: message(e) });
  }
}

/** K4 — check a chosen backup folder against its manifest before anything is restored. */
export async function checkBackup(folder: string): Promise<void> {
  if (storageSlice.get().backupOp) return;
  storageSlice.set({ backupOp: "verify", backupError: null, restoreCheck: null, lastRestore: null, backupProgress: null });
  try {
    const restoreCheck = await bridge.storage.backupVerify(folder);
    storageSlice.set({ backupOp: null, restoreCheck });
  } catch (e) {
    storageSlice.set({ backupOp: null, backupError: message(e) });
  }
}

/** K4 — restore from the checked folder. Rust re-verifies and refuses on any mismatch. */
export async function restoreBackup(folder: string, parts: BackupPart[]): Promise<void> {
  if (storageSlice.get().backupOp) return;
  storageSlice.set({ backupOp: "restore", backupProgress: null, backupError: null, lastRestore: null });
  try {
    const lastRestore = await bridge.storage.backupRestore(folder, parts, onBackupProgress);
    storageSlice.set({ backupOp: null, lastRestore });
    if (parts.includes("files")) await refreshPins();
  } catch (e) {
    storageSlice.set({ backupOp: null, backupError: message(e) });
  }
}
