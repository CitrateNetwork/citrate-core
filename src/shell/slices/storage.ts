// =====================================================================
// citrate-core — storage slice (CX-S2.3, lane s2)
//
// State for the Files surface: the pin list, transient busy/error flags. Actions call
// bridge.storage (the S2.1 kubo seam) and fold results in. Errors are CAUGHT into `error`,
// never thrown at render. S2.3 pins LOCALLY (to this node's kubo) — the network-wide
// ceremony-gated SALT bond is S2.2, blocked on the chain CommD fix (FINDING_PIN_COMMD_BOND).
//
// Owned entirely by lane s2 — separate from the legacy `class Store`, so no shared-state race.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type { PinRow } from "../../bridge/domains";

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
}

const initial: StorageState = {
  pins: [],
  adding: false,
  busyCid: null,
  lastRetrievedPath: null,
  kuboDown: false,
  error: null,
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

/** Pin a CID LOCALLY (to this node). Records the local marker as the bond until S2.2 lands. */
export async function localPin(cid: string): Promise<void> {
  storageSlice.set({ busyCid: cid, error: null });
  try {
    await bridge.storage.pin(cid, LOCAL_PIN_MARKER);
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
