// CX bridge impl — modelsCatalog (C-16), TAURI. Owned by lane s1 (CX-S1) after S0.
//
// S0.2 stub: honest `Unavailable` (Rule 1) — an unwired domain SAYS so, it never shows
// fabricated data. CX-S1 replaces each throw with a real invoke over the HF Hub / GitHub
// Releases resolver + the runtime-selectable llama-server (-m), copying this shape exactly.
import type { ModelsCatalogDomain } from "../domains";
import { Unavailable } from "../types";

export const tauriModelsCatalog: ModelsCatalogDomain = {
  async local() {
    throw new Unavailable("modelsCatalog", "local");
  },
  async search() {
    throw new Unavailable("modelsCatalog", "search");
  },
  async download() {
    throw new Unavailable("modelsCatalog", "download");
  },
  async select() {
    throw new Unavailable("modelsCatalog", "select");
  },
};
