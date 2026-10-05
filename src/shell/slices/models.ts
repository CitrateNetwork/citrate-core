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
import type { ModelDeleted, ModelDeleteState, ModelDescriptor, PartialDownload, RegistryModel } from "../../bridge/domains";
import { refreshTier } from "./tier";

export type ModelSourceId = "hf" | "github";

export interface ModelsState {
  /** Locally-present, verified models (bridge.modelsCatalog.local). */
  local: ModelDescriptor[];
  /** HUP-S0.3 — interrupted catalog downloads that can be resumed after a restart. */
  partials: PartialDownload[];
  /** Results of the last catalog search. */
  results: ModelDescriptor[];
  /** Models registered on-chain in the ModelRegistry (Hermes WP0.2b). Not-yet-local. */
  registry: RegistryModel[];
  /** The id of the active local model, or null if unknown/none selected. */
  activeId: string | null;
  /** A search is in flight. */
  searching: boolean;
  /** The id currently downloading, or null. One at a time keeps the UX legible. */
  downloadingId: string | null;
  /** Download progress 0–100 for `downloadingId` (null when not downloading / unknown). */
  downloadPct: number | null;
  /** The id currently being switched to, or null. */
  selectingId: string | null;
  /** local() isn't wired/available yet — a pending WIRE, not a user-facing error. */
  localPending: boolean;
  /** Whether each downloaded model can be deleted now, by file name (from core). A model with no
   *  entry offers no Delete (web preview, or core could not say). */
  deleteStates: Record<string, ModelDeleteState>;
  /** The file being deleted, or null. One at a time. */
  deletingFile: string | null;
  /** The last user-facing error (from any action), or null when clear. */
  error: string | null;
}

const initial: ModelsState = {
  local: [],
  partials: [],
  results: [],
  registry: [],
  activeId: null,
  searching: false,
  downloadingId: null,
  downloadPct: null,
  selectingId: null,
  localPending: false,
  deleteStates: {},
  deletingFile: null,
  error: null,
};

export const modelsSlice = createSlice<ModelsState>(initial);

const message = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === "string" ? e : String(e);

/** Hermes WP0.2b — load the on-chain ModelRegistry models. Honest-empty on a sim/failed
 *  read (never fabricated); the router surfaces them as downloadable ("download to use"). */
export async function refreshRegistryModels(): Promise<void> {
  try {
    const registry = await bridge.modelsCatalog.registry();
    modelsSlice.set({ registry });
  } catch {
    modelsSlice.set({ registry: [] }); // unwired/failed read → honest empty, not a fake list
  }
}

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
  await refreshPartials();
  await refreshDeleteStates();
}

/** Ask core which downloaded models can be deleted now. A failure offers no Delete at all. */
export async function refreshDeleteStates(): Promise<void> {
  try {
    const list = await bridge.modelsCatalog.deleteStates();
    modelsSlice.set({ deleteStates: Object.fromEntries(list.map((s) => [s.file, s])) });
  } catch {
    modelsSlice.set({ deleteStates: {} });
  }
}

/** How a model row's Delete looks: hidden, enabled, or disabled with the reason as its tooltip. */
export interface DeleteControl {
  show: boolean;
  disabled: boolean;
  reason: string | null;
}

/** Pure: the Delete control for `file` from the slice state. Core's answer decides; while any
 *  delete, download or switch is running every Delete waits. */
export function deleteControl(st: ModelsState, file: string): DeleteControl {
  const d = st.deleteStates[file];
  if (!d) return { show: false, disabled: true, reason: null };
  if (!d.deletable) return { show: true, disabled: true, reason: d.reason || "This model cannot be deleted right now" };
  if (st.deletingFile) return { show: true, disabled: true, reason: "Another model is being deleted" };
  if (st.downloadingId) return { show: true, disabled: true, reason: "Wait for the download to finish" };
  if (st.selectingId) return { show: true, disabled: true, reason: "Wait for the model switch to finish" };
  return { show: true, disabled: false, reason: null };
}

/** Delete a downloaded model the member confirmed. On success the lists and the machine's free
 *  space are re-read and the result is returned; on a refusal the reason is the error and null
 *  is returned. */
export async function deleteLocalModel(file: string): Promise<ModelDeleted | null> {
  if (modelsSlice.get().deletingFile) return null; // one at a time
  modelsSlice.set({ deletingFile: file, error: null });
  try {
    const out = await bridge.modelsCatalog.deleteLocal(file);
    modelsSlice.set((s) => ({
      deletingFile: null,
      activeId: s.activeId === `local:${out.file}` ? null : s.activeId,
      local: s.local.filter((m) => m.file !== out.file),
    }));
    await refreshLocalModels();
    // The tier report carries the free disk space; read it again now that space was freed.
    await refreshTier();
    return out;
  } catch (e) {
    modelsSlice.set({ deletingFile: null, error: message(e) });
    await refreshDeleteStates();
    return null;
  }
}

/** HUP-S0.3 — load interrupted downloads. Honest-empty on failure (never an error banner). */
export async function refreshPartials(): Promise<void> {
  try {
    modelsSlice.set({ partials: await bridge.modelsCatalog.partials() });
  } catch {
    modelsSlice.set({ partials: [] });
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
  modelsSlice.set({ downloadingId: id, downloadPct: 0, error: null });
  try {
    await bridge.modelsCatalog.download(id, (pct) => {
      // Guard against a late event after another download started.
      if (modelsSlice.get().downloadingId === id) modelsSlice.set({ downloadPct: pct });
    });
    modelsSlice.set({ downloadingId: null, downloadPct: null });
    await refreshLocalModels();
  } catch (e) {
    modelsSlice.set({ downloadingId: null, downloadPct: null, error: message(e) });
    await refreshPartials(); // an interrupted download stays listed for a later resume
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
