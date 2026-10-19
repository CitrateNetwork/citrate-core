// =====================================================================
// citrate-core — Store slice (K5a)
//
// State for the Store surface: the signed catalog view, the open listing, the install sheet and
// what the Store installed. Every action calls bridge.store (Rust `store_catalog.rs`) and folds
// the answer in; nothing here invents a listing or a verification. A thrown bridge error becomes
// `error` (the honest banner), never a render crash.
//
// Install sheet phases: staging (verifying signature and hash) → review (the member reads the
// declared permissions and budget) → installing (after the member approves) → done (installed
// paused) | refused (with the core's reason code).
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type { StoreCatalogView, StoreInstalled, StoreRefusal, StoreStaged } from "../../bridge/domains";
import { daemonsApi } from "../../daemons/api";

export type StoreSheetPhase = "staging" | "review" | "installing" | "done" | "refused";

export interface StoreSheet {
  listingId: string;
  phase: StoreSheetPhase;
  staged: StoreStaged | null;
  refusal: StoreRefusal | null;
  installed: StoreInstalled | null;
  /** Download progress while a model installs, 0–100, or null. */
  pct: number | null;
}

export interface StoreState {
  catalog: StoreCatalogView | null;
  loading: boolean;
  /** A bridge call threw (not a refusal): the real error text. */
  error: string | null;
  installed: StoreInstalled[];
  /** The listing page that is open, or null for the list. */
  openId: string | null;
  sheet: StoreSheet | null;
  /** The listing being removed, or null. */
  removing: string | null;
  /** The last removal / start result, shown once. */
  notice: string | null;
}

const initial: StoreState = {
  catalog: null,
  loading: false,
  error: null,
  installed: [],
  openId: null,
  sheet: null,
  removing: null,
  notice: null,
};

export const storeSlice = createSlice<StoreState>(initial);

const msg = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** Fetch and verify the catalog, and read what is installed. */
export async function refreshStore(): Promise<void> {
  storeSlice.set({ loading: true, error: null });
  try {
    const [catalog, installed] = await Promise.all([bridge.store.catalog(), bridge.store.installed()]);
    storeSlice.set({ catalog, installed, loading: false });
  } catch (e) {
    storeSlice.set({ loading: false, error: msg(e) });
  }
}

export function openListing(id: string | null): void {
  storeSlice.set({ openId: id, notice: null });
}

/** Open the install sheet: core verifies the signature (and a daemon's hash) and stages it. */
export async function beginInstall(listingId: string): Promise<void> {
  storeSlice.set({
    sheet: { listingId, phase: "staging", staged: null, refusal: null, installed: null, pct: null },
  });
  try {
    const out = await bridge.store.stage(listingId);
    storeSlice.set((s) =>
      s.sheet?.listingId !== listingId
        ? {}
        : {
            sheet: out.ok
              ? { ...s.sheet, phase: "review", staged: out.ok }
              : { ...s.sheet, phase: "refused", refusal: out.refused ?? { code: "install_failed", message: "No answer from the Store" } },
          },
    );
  } catch (e) {
    storeSlice.set((s) => ({
      sheet: s.sheet && { ...s.sheet, phase: "refused", refusal: { code: "install_failed", message: msg(e) } },
    }));
  }
}

/** The member approved the sheet (HIC). Core installs the listing PAUSED. */
export async function approveInstall(): Promise<void> {
  const sheet = storeSlice.get().sheet;
  if (!sheet || sheet.phase !== "review" || !sheet.staged) return;
  const stageId = sheet.staged.stageId;
  storeSlice.set({ sheet: { ...sheet, phase: "installing", pct: null } });
  try {
    const out = await bridge.store.install(stageId, (pct) =>
      storeSlice.set((s) => (s.sheet?.staged?.stageId === stageId ? { sheet: { ...s.sheet, pct } } : {})),
    );
    storeSlice.set((s) =>
      s.sheet?.staged?.stageId !== stageId
        ? {}
        : {
            sheet: out.ok
              ? { ...s.sheet, phase: "done", installed: out.ok }
              : { ...s.sheet, phase: "refused", refusal: out.refused ?? { code: "install_failed", message: "No answer from the Store" } },
          },
    );
    if (out.ok) await refreshStore();
  } catch (e) {
    storeSlice.set((s) => ({
      sheet: s.sheet && { ...s.sheet, phase: "refused", refusal: { code: "install_failed", message: msg(e) } },
    }));
  }
}

export function closeSheet(): void {
  storeSlice.set({ sheet: null });
}

/** One call: core removes the daemon or the model file and forgets the install. */
export async function removeInstall(listingId: string): Promise<void> {
  // K5-F: never ask core to remove a listing whose install is still running here (core refuses
  // it too, with `install_running`).
  const sheet = storeSlice.get().sheet;
  if (sheet?.listingId === listingId && (sheet.phase === "installing" || sheet.phase === "staging")) {
    storeSlice.set({ notice: "It is still installing. Remove it when the install has finished." });
    return;
  }
  storeSlice.set({ removing: listingId, notice: null });
  try {
    const out = await bridge.store.remove(listingId);
    storeSlice.set({ removing: null, notice: out.ok ?? out.refused?.message ?? null });
    await refreshStore();
  } catch (e) {
    storeSlice.set({ removing: null, notice: msg(e) });
  }
}

/** Start a daemon the Store installed paused (the daemon book's own pause switch). */
export async function startInstalledDaemon(daemonId: string): Promise<void> {
  const api = daemonsApi();
  if (!api) {
    storeSlice.set({ notice: "Daemons run in the desktop app" });
    return;
  }
  try {
    await api.setPaused(daemonId, false, Date.now());
    storeSlice.set({ notice: "Started. It runs on its schedule inside its budget." });
    await refreshStore();
  } catch (e) {
    storeSlice.set({ notice: msg(e) });
  }
}
