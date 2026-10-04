// Bridge impl — escalation router (HUP-S1.5), TAURI. Every command runs in Rust `escalation.rs`:
// endpoint keys are sealed in the OS keyring there and never come back across invoke; the daily
// spend budget, the quote and the run (write-ahead reservation, then the sidecar) all live in core.
// The registry route (escalation_registry.rs) quotes from the on-chain InferenceRouter and pays with
// an x402 authorization the member approves in the SignatureCeremony (HIC-1).
import { invoke } from "./invoke";
import type {
  EscalationBudget,
  EscalationDomain,
  EscalationEndpoint,
  EscalationEndpointInput,
  EscalationQuote,
  EscalationRegistryQuote,
  EscalationRegistryRun,
  EscalationRegistryRunRecord,
  EscalationRegistryStatus,
  EscalationRun,
} from "../domains";
import type { CeremonyView } from "../types";

export const tauriEscalation: EscalationDomain = {
  endpoints() {
    return invoke<EscalationEndpoint[]>("escalation_endpoints");
  },
  addEndpoint(input: EscalationEndpointInput, apiKey: string) {
    return invoke<EscalationEndpoint>("escalation_endpoint_add", { input, apiKey });
  },
  removeEndpoint(id: string) {
    return invoke<void>("escalation_endpoint_remove", { id });
  },
  budget() {
    return invoke<EscalationBudget>("escalation_budget");
  },
  setBudget(capMicros: number) {
    return invoke<EscalationBudget>("escalation_budget_set", { capMicros });
  },
  quote(endpointId: string, prompt: string, system?: string | null, maxTokens?: number | null) {
    return invoke<EscalationQuote>("escalation_quote", { endpointId, prompt, system: system ?? null, maxTokens: maxTokens ?? null });
  },
  run(quoteId: string, shownCostMicros: number, confirmed: boolean, tainted: boolean) {
    return invoke<EscalationRun>("escalation_run", { quoteId, shownCostMicros, confirmed, tainted });
  },
  registryStatus() {
    return invoke<EscalationRegistryStatus>("escalation_registry_status");
  },
  registryQuote(modelHash: string, prompt: string, system?: string | null, maxTokens?: number | null) {
    return invoke<EscalationRegistryQuote>("escalation_registry_quote", { modelHash, prompt, system: system ?? null, maxTokens: maxTokens ?? null });
  },
  registryRequest(quoteId: string, shownPriceBaseUnits: string) {
    return invoke<CeremonyView>("escalation_registry_request", { quoteId, shownPriceBaseUnits });
  },
  registryRun(quoteId: string, signature: string) {
    return invoke<EscalationRegistryRun>("escalation_registry_run", { quoteId, signature });
  },
  registryHistory() {
    return invoke<EscalationRegistryRunRecord[]>("escalation_registry_history");
  },
};
