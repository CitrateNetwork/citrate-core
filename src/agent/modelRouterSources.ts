// Hermes ModelRouter — live source adapters (P0 / WP0.2).
//
// Maps the FOUR real model sources onto the pure router's EnumerateInput:
//   - local    → bridge.modelsCatalog.local() (downloaded + verified GGUFs) → ready
//   - cloud    → bridge.providers.status() (D-11 BYOK): every model of every provider connected in
//                Settings › Models (a stored key, an adapter, no halt, a confirmed custom URL).
//                Choosing one stores it in core as the preferred model; the Hermes turn then runs
//                through the core model proxy (the key never leaves core).
//   - gateway  → the always-serve terminal: a configured gateway, or the on-device demo
//                backstop when no key is sealed (so INV-Router-2's fallback is always ready)
//   - registry → on-chain-registry models NOT yet local (NOT ready — must pull+verify)
//
// NOTE (WP0.2b, tracked): the on-chain model registry currently has only a WRITE path
// (storage.rs register_model_calldata); there is no READ command yet, so `registry` is
// empty until a `models_registry_list` chain-read lands. Local + gateway are wired to live
// data now. The mapping is pure so it is unit-testable without a bridge.
import type { ModelDescriptor, AiProviderStatus, ChatRoute, ProviderStatus, RegistryModel } from "../bridge/domains";
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

/** One selectable cloud model (D-11). */
export interface CloudModelOption {
  providerId: string;
  providerName: string;
  model: string;
  /** A custom endpoint the member added (never a governed vendor). */
  unverified: boolean;
}

/** Most models listed per provider (a vendor's list can run to hundreds). */
export const MAX_MODELS_PER_PROVIDER = 60;

/** The cloud models Hermes can run on: providers with a stored key, an adapter in this build, no
 *  halt, and (custom endpoints) the member's confirmation. Cognition is a delegate, never a chat
 *  model; the Citrate gateway is the router's terminal choice, not a BYOK provider. */
export function cloudModelOptions(statuses: readonly ProviderStatus[]): CloudModelOption[] {
  const out: CloudModelOption[] = [];
  for (const p of statuses) {
    if (!p.keyConfigured || !p.adapter || p.halt || p.kind === "delegate" || p.kind === "gateway") continue;
    if (p.kind === "custom" && !p.confirmed) continue;
    const models = [...new Set([...p.approvedModels, ...p.listedModels])].slice(0, MAX_MODELS_PER_PROVIDER);
    for (const model of models) out.push({ providerId: p.id, providerName: p.name, model, unverified: p.kind === "custom" });
  }
  return out;
}

/** Why a connected cloud model cannot take a Hermes turn now (null = it can). */
export const LOCAL_ONLY_CLOUD_NOTE = "Local model only is on";

/** D-11: the router's cloud input from core's provider status. Under Local model only the models
 *  are listed but not ready (core refuses a cloud call there too), so the send path never uses one. */
export function cloudChoiceInput(statuses: readonly ProviderStatus[], route: ChatRoute = "ask_first"): NonNullable<EnumerateInput["cloud"]> {
  const localOnly = route === "local_only";
  return cloudModelOptions(statuses).map((o) => ({
    providerId: o.providerId,
    model: o.model,
    label: `${o.providerName} · ${o.model}${o.unverified ? " (unverified)" : ""}`,
    ready: !localOnly,
    ...(localOnly ? { note: LOCAL_ONLY_CLOUD_NOTE } : {}),
  }));
}

/** Build the pure router input from the live sources. `gatewayReady` is always true: the
 *  gateway/demo terminal always serves, so the send path's fallback is never a dead end. */
export function liveEnumerateInput(
  local: ModelDescriptor[],
  registry: { id: string; label: string }[] = [],
  cloud: NonNullable<EnumerateInput["cloud"]> = [],
): EnumerateInput {
  return {
    local: local.map((m) => ({ id: m.id, label: modelLabel(m) })),
    cloud,
    registry,
    gatewayReady: true,
  };
}

/** Convenience: the full choice list from the live local set (+ cloud, + optional registry). */
export function choicesFromSources(
  local: ModelDescriptor[],
  registry: { id: string; label: string }[] = [],
  cloud: NonNullable<EnumerateInput["cloud"]> = [],
): ModelChoice[] {
  return enumerateChoices(liveEnumerateInput(local, registry, cloud));
}

/** The one router list every surface shows (the chat picker, the Activity monitor, widgets): the
 *  models slice's local + registry models and the connected providers' cloud models. */
export function routerChoicesFor(
  models: { local: ModelDescriptor[]; registry: RegistryModel[]; cloud: readonly ProviderStatus[] },
  route: ChatRoute = "ask_first",
): ModelChoice[] {
  return choicesFromSources(models.local, registryModelsToChoiceInput(models.registry), cloudChoiceInput(models.cloud, route));
}
