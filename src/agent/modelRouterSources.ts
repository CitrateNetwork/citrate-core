// Hermes ModelRouter — live source adapters (P0 / WP0.2).
//
// Maps the THREE real model sources onto the pure router's EnumerateInput:
//   - local    → bridge.modelsCatalog.local() (downloaded + verified GGUFs) → ready
//   - gateway  → the always-serve terminal: a configured gateway, or the on-device demo
//                backstop when no key is sealed (so INV-Router-2's fallback is always ready)
//   - registry → on-chain-registry models NOT yet local (NOT ready — must pull+verify)
//
// NOTE (WP0.2b, tracked): the on-chain model registry currently has only a WRITE path
// (storage.rs register_model_calldata); there is no READ command yet, so `registry` is
// empty until a `models_registry_list` chain-read lands. Local + gateway are wired to live
// data now. The mapping is pure so it is unit-testable without a bridge.
import type { ModelDescriptor, AiProviderStatus, RegistryModel } from "../bridge/domains";
import { enumerateChoices, type EnumerateInput, type ModelChoice } from "./modelRouter";

/** Map on-chain ModelRegistry models (WP0.2b) onto the router's downloadable third-source
 *  input: id = the modelHash, label = the human name. They are NOT-ready (the router marks
 *  them "download to use") until also present + verified locally. */
export function registryModelsToChoiceInput(models: RegistryModel[]): { id: string; label: string }[] {
  return models.map((m) => ({ id: m.id, label: m.name || m.id }));
}

/** A human label for a local model: the GGUF filename, else the repo, else the id. */
export function modelLabel(m: ModelDescriptor): string {
  return m.file || m.repo || m.id;
}

/** Is a real gateway provider configured (a sealed key)? Used for the picker LABEL only —
 *  the terminal is always serve-able either way (demo backstops an unconfigured gateway),
 *  so this never makes the fallback not-ready (that would break INV-Router-2). */
export function gatewayConfigured(providers: AiProviderStatus[]): boolean {
  return providers.some((p) => p.configured);
}

/** Build the pure router input from the live sources. `gatewayReady` is always true: the
 *  gateway/demo terminal always serves, so the send path's fallback is never a dead end. */
export function liveEnumerateInput(
  local: ModelDescriptor[],
  registry: { id: string; label: string }[] = [],
): EnumerateInput {
  return {
    local: local.map((m) => ({ id: m.id, label: modelLabel(m) })),
    registry,
    gatewayReady: true,
  };
}

/** Convenience: the full choice list from the live local set (+ optional registry). */
export function choicesFromSources(
  local: ModelDescriptor[],
  registry: { id: string; label: string }[] = [],
): ModelChoice[] {
  return enumerateChoices(liveEnumerateInput(local, registry));
}
