// Bridge impl — Store v1 (K5a), TAURI. Rust `store_catalog.rs`: the catalog is fetched and
// verified against the pinned catalog key (empty until the key ceremony, so every catalog call
// answers "no_key" before any network or disk access). Installs arrive paused; removal is one call.
import { listen } from "@tauri-apps/api/event";
import { invoke } from "./invoke";
import type { StoreCatalogView, StoreDomain, StoreInstalled, StoreOutcome, StoreStaged } from "../domains";

export const tauriStore: StoreDomain = {
  catalog() {
    return invoke<StoreCatalogView>("store_catalog");
  },
  stage(listingId: string) {
    return invoke<StoreOutcome<StoreStaged>>("store_stage", { listingId });
  },
  async install(stageId: string, onProgress?: (pct: number) => void) {
    const unlisten = onProgress
      ? await listen<{ stageId: string; pct: number }>("store://install-progress", (ev) => {
          if (ev.payload?.stageId === stageId && typeof ev.payload.pct === "number") onProgress(ev.payload.pct);
        })
      : null;
    try {
      return await invoke<StoreOutcome<StoreInstalled>>("store_install", {
        stageId,
        nowMs: Date.now(),
        offsetMin: -new Date().getTimezoneOffset(),
      });
    } finally {
      unlisten?.();
    }
  },
  installed() {
    return invoke<StoreInstalled[]>("store_installed", { nowMs: Date.now() });
  },
  remove(listingId: string) {
    return invoke<StoreOutcome<string>>("store_remove", { listingId });
  },
};
