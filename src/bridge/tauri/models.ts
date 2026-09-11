// CX bridge impl — modelsCatalog (C-16), TAURI. Owned by lane s1 (CX-S1).
//
// S1.6: real invokes over the S1.4 catalog resolver + the S1.5 runtime-selectable llama-server.
// The command NAMES are the ones frozen in lib.rs (S0.3); arg keys are camelCase matching the
// Rust command params. Descriptors cross the boundary already shaped as the ModelDescriptor DTO
// (Rust serializes camelCase), so nothing is reshaped here.
import { listen } from "@tauri-apps/api/event";
import { invoke } from "./invoke";
import type { ModelDescriptor, ModelsCatalogDomain, RegisterModelInput, RegistryModel } from "../domains";

export const tauriModelsCatalog: ModelsCatalogDomain = {
  local() {
    return invoke<ModelDescriptor[]>("model_catalog_local");
  },
  search(source, query) {
    return invoke<ModelDescriptor[]>("model_catalog_search", { source, query });
  },
  async download(id, onProgress) {
    // Subscribe to byte-progress events for THIS id before invoking; the Rust command emits
    // `model://download-progress` on each whole-percent change. Always unlisten when done.
    const unlisten = onProgress
      ? await listen<{ id: string; pct: number }>("model://download-progress", (ev) => {
          if (ev.payload?.id === id && typeof ev.payload.pct === "number") onProgress(ev.payload.pct);
        })
      : null;
    try {
      await invoke("model_catalog_download", { id });
    } finally {
      unlisten?.();
    }
  },
  async select(id) {
    await invoke("model_catalog_select", { id });
  },
  registry() {
    return invoke<RegistryModel[]>("models_registry_list");
  },
  async register(input: RegisterModelInput) {
    // camelCase arg keys — Tauri v2 maps them to the Rust command's snake_case params.
    await invoke("models_registry_register", {
      name: input.name,
      framework: input.framework,
      version: input.version,
      ipfsCid: input.ipfsCid,
      sizeBytes: input.sizeBytes,
      inferencePrice: input.inferencePrice,
      description: input.description,
      license: input.license,
      tags: input.tags,
    });
  },
};
