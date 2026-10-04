// Bridge impl — escalation router (HUP-S1.5), TAURI. Every command runs in Rust `escalation.rs`:
// endpoint keys are sealed in the OS keyring there and never come back across invoke; the daily
// spend budget, the quote and the run (write-ahead reservation, then the sidecar) all live in core.
import { invoke } from "./invoke";
import type {
  EscalationBudget,
  EscalationConfirmation,
  EscalationDomain,
  EscalationEndpoint,
  EscalationEndpointInput,
  EscalationQuote,
  EscalationRegistryMine,
  EscalationRegistryQuote,
  EscalationRegistryResult,
  EscalationRegistryStatus,
  EscalationRun,
} from "../domains";

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
  confirmPrepare(quoteId: string, shownCostMicros: number) {
    return invoke<EscalationConfirmation>("escalation_confirm_prepare", { quoteId, shownCostMicros });
  },
  run(quoteId: string, shownCostMicros: number, confirmId: string | null, tainted: boolean) {
    return invoke<EscalationRun>("escalation_run", { quoteId, shownCostMicros, confirmId, tainted });
  },
  registryStatus() {
    return invoke<EscalationRegistryStatus>("escalation_registry_status");
  },
  // Registry route (InferenceRouter, native SALT, HIC-1 per request): `inference_router.rs`.
  registryQuote(modelHash: string, input: string, maxPriceWei: string) {
    return invoke<EscalationRegistryQuote>("escalation_registry_quote", { modelHash, input, maxPriceWei });
  },
  registryRequest(modelHash: string, input: string, maxPriceWei: string, shownMaxPriceWei: string) {
    return invoke<void>("escalation_registry_request", { modelHash, input, maxPriceWei, shownMaxPriceWei });
  },
  registryResult(requestId: number) {
    return invoke<EscalationRegistryResult>("escalation_registry_result", { requestId });
  },
  registryMine() {
    return invoke<EscalationRegistryMine>("escalation_registry_mine");
  },
  registryClaimRefund() {
    return invoke<void>("escalation_registry_claim_refund");
  },
  registryExpire(requestId: number) {
    return invoke<void>("escalation_registry_expire", { requestId });
  },
};
