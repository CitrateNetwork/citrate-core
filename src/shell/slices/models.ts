// =====================================================================
// citrate-core — models slice (CX-S1.6, lane s1)
//
// The one state primitive for the Models surface: the locally-present models, the last search
// results, which model is active, and transient busy/error flags. Actions call the bridge
// (bridge.modelsCatalog → the S1.4 resolver + S1.5 download/select) and fold the result into
// slice state. Errors are CAUGHT into `error` (honest surface message), never thrown at render.
//
// Owned entirely by lane s1 — separate from the legacy `class Store`, so no shared-state race.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type { ModelDescriptor } from "../../bridge/domains";

export type ModelSourceId = "hf" | "github";

export interface ModelsState {
  /** Locally-present, verified models (bridge.modelsCatalog.local). */
  local: ModelDescriptor[];
  /** Results of the last catalog search. */
  results: ModelDescriptor[];
  /** The id of the active local model, or null if unknown/none selected. */
  activeId: string | null;
  /** A search is in flight. */
  searching: boolean;
  /** The id currently downloading, or null. One at a time keeps the UX legible. */
  downloadingId: string | null;
  /** The id currently being switched to, or null. */
  selectingId: string | null;
  /** local() isn't wired/available yet — a pending WIRE, not a user-facing error. */
  localPending: boolean;
  /** The last user-facing error (from any action), or null when clear. */
  error: string | null;
}

const initial: ModelsState = {
  local: [],
  results: [],
  activeId: null,
  searching: false,
  downloadingId: null,
  selectingId: null,
  localPending: false,
  error: null,
};

export const modelsSlice = createSlice<ModelsState>(initial);

const message = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === "string" ? e : String(e);

/** Load the locally-present verified models. Honest-empty on a not-wired/sim bridge. */
export async function refreshLocalModels(): Promise<void> {
  try {
    const local = await bridge.modelsCatalog.local();
    modelsSlice.set({ local, localPending: false, error: null });
  } catch {
    // local() is not wired yet (CX-S1 resolver pending) — this is a pending WIRE, not a
    // user-facing error. Keep the surface calm and honest; search/download/select still work.
    modelsSlice.set({ localPending: true });
  }
}

/** Search a source for downloadable models. Clears results first so stale hits never linger. */
export async function searchModels(source: ModelSourceId, query: string): Promise<void> {
  const q = query.trim();
  if (q === "") {
    modelsSlice.set({ results: [], error: null });
    return;
  }
  modelsSlice.set({ searching: true, error: null });
  try {
    const results = await bridge.modelsCatalog.search(source, q);
    modelsSlice.set({ results, searching: false });
  } catch (e) {
    modelsSlice.set({ results: [], searching: false, error: message(e) });
  }
}

/** Download + verify a catalog model by id, then refresh the local list so it appears. */
export async function downloadModel(id: string): Promise<void> {
  if (modelsSlice.get().downloadingId) return; // one at a time
  modelsSlice.set({ downloadingId: id, error: null });
  try {
    await bridge.modelsCatalog.download(id);
    modelsSlice.set({ downloadingId: null });
    await refreshLocalModels();
  } catch (e) {
    modelsSlice.set({ downloadingId: null, error: message(e) });
  }
}

/** Switch the active model (restarts llama-server -m). Sets activeId only on success. */
export async function selectModel(id: string): Promise<void> {
  modelsSlice.set({ selectingId: id, error: null });
  try {
    await bridge.modelsCatalog.select(id);
    modelsSlice.set({ selectingId: null, activeId: id });
  } catch (e) {
    modelsSlice.set({ selectingId: null, error: message(e) });
  }
}
