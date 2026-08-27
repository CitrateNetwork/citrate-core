// CX bridge impl — modelsCatalog (C-16), TAURI. Owned by lane s1 (CX-S1).
//
// S1.6: real invokes over the S1.4 catalog resolver + the S1.5 runtime-selectable llama-server.
// The command NAMES are the ones frozen in lib.rs (S0.3); arg keys are camelCase matching the
// Rust command params. Descriptors cross the boundary already shaped as the ModelDescriptor DTO
// (Rust serializes camelCase), so nothing is reshaped here.
import { invoke } from "@tauri-apps/api/core";
import type { ModelDescriptor, ModelsCatalogDomain } from "../domains";

export const tauriModelsCatalog: ModelsCatalogDomain = {
  local() {
    return invoke<ModelDescriptor[]>("model_catalog_local");
  },
  search(source, query) {
    return invoke<ModelDescriptor[]>("model_catalog_search", { source, query });
  },
  async download(id) {
    await invoke("model_catalog_download", { id });
  },
  async select(id) {
    await invoke("model_catalog_select", { id });
  },
};
