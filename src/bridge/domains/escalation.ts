// citrate-core: bridge domain `escalation`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ---- HUP-S1.5 — the escalation router (US-1.5). Mirrors Rust `escalation.rs`. ----
// Amounts are integer micro-USD. Prices are member-entered (per 1M tokens) and unverified.

export interface EscalationEndpointInput {
  label: string;
  baseUrl: string;
  model: string;
  inputMicrosPerMtok: number;
  outputMicrosPerMtok: number;
}

/** A member endpoint. Never carries the key (it is sealed in the OS keyring by core). */
export interface EscalationEndpoint extends EscalationEndpointInput {
  id: string;
  /** "label · host" — what every price card names. */
  destination: string;
}

export type EscalationMode = "budget" | "confirmed";

export interface EscalationSpendRecord {
  escalationId: string;
  endpointId: string;
  destination: string;
  quotedMicros: number;
  chargedMicros: number;
  mode: EscalationMode;
  outcome: "answered" | "not_sent" | "failed" | "unknown";
  usageReported: boolean;
  exceededQuote: boolean;
  atMs: number;
}

export interface EscalationBudget {
  capMicros: number;
  usedMicros: number;
  remainingMicros: number;
  /** Member-confirmed (HIC-1) spend today, outside the cap. */
  confirmedMicros: number;
  periodStartMs: number;
  periodEndMs: number;
  maxCapMicros: number;
  /** The ledger file could not be read: every escalation asks until the cap is set again. */
  unreadable: boolean;
  history: EscalationSpendRecord[];
}

/** The price card shown before an escalation runs. */
export interface EscalationQuote {
  quoteId: string;
  endpointId: string;
  destination: string;
  model: string;
  costMicros: number;
  costLabel: string;
  withinBudget: boolean;
  /** K2: core will ask before this escalation runs whatever the budget (the member's route is
   *  ask first). Absent from an older core. */
  asksFirst?: boolean;
  remainingMicros: number;
  capMicros: number;
  maxTokens: number;
  promptBytes: number;
  expiresMs: number;
}

/** Core's one-shot confirmation id for one shown quote and price. */
export interface EscalationConfirmation {
  confirmId: string;
  quoteId: string;
  costMicros: number;
  expiresMs: number;
}

export interface EscalationRun {
  escalationId: string;
  content: string;
  destination: string;
  mode: EscalationMode;
  chargedMicros: number;
  chargedLabel: string;
  usageReported: boolean;
  exceededQuote: boolean;
  remainingMicros: number;
}

export interface EscalationRegistryStatus {
  /** The HIC-1 registry route can run (an InferenceRouter is pinned for 40204). */
  enabled: boolean;
  reason: string;
  missing: string[];
  /** The pinned InferenceRouter, when there is one. */
  router: string | null;
  /** How a registry request is paid: native SALT, one approval per request. */
  payment: "native-salt-hic1";
  /** Budgeted x402 payment (ADR B-2). Off in this build. */
  x402Enabled: boolean;
}

/** HUP-S1.5 registry quote. Mirrors Rust `inference_router::RegistryQuote`. Amounts are wei strings. */
export interface EscalationRegistryQuote {
  router: string;
  modelHash: string;
  maxPriceWei: string;
  maxPriceSalt: string;
  inputBytes: number;
  eligibleProviders: number;
  cheapestMinPriceWei: string | null;
  /** Always true: the router stores the input on chain. */
  inputIsPublic: boolean;
}

export type EscalationRouterStatus = "pending" | "processing" | "completed" | "failed" | "cancelled";

/** HUP-S1.5 one router request. Mirrors Rust `inference_router::RegistryResultView`. */
export interface EscalationRegistryResult {
  requestId: number;
  status: EscalationRouterStatus;
  modelHash: string;
  pricePaidWei: string;
  /** The provider's answer: untrusted data. */
  output: string | null;
  outputTruncated: boolean;
  outputIsHex: boolean;
}

/** HUP-S1.5 the member's router requests. Mirrors Rust `inference_router::RegistryMineView`. */
export interface EscalationRegistryMine {
  router: string;
  requestIds: number[];
  refundOwedWei: string;
}

export interface EscalationDomain {
  endpoints(): Promise<EscalationEndpoint[]>;
  /** The key goes to core once and is sealed in the OS keyring; it is never returned. */
  addEndpoint(input: EscalationEndpointInput, apiKey: string): Promise<EscalationEndpoint>;
  removeEndpoint(id: string): Promise<void>;
  budget(): Promise<EscalationBudget>;
  setBudget(capMicros: number): Promise<EscalationBudget>;
  quote(endpointId: string, prompt: string, system?: string | null, maxTokens?: number | null): Promise<EscalationQuote>;
  /**
   * The member's approval card is opening for a shown quote: core mints the one-shot confirmation
   * id a confirmed run needs. A run cannot be approved by a flag.
   */
  confirmPrepare(quoteId: string, shownCostMicros: number): Promise<EscalationConfirmation>;
  /**
   * Runs a quote the member was shown. `shownCostMicros` must equal the quote's price. `confirmId`
   * is the id from `confirmPrepare` once the member approved, else null (within budget only).
   */
  run(quoteId: string, shownCostMicros: number, confirmId: string | null, tainted: boolean): Promise<EscalationRun>;
  registryStatus(): Promise<EscalationRegistryStatus>;
  /** Quote a registry request from the model's live route (off until a router is pinned). */
  registryQuote(modelHash: string, input: string, maxPriceWei: string): Promise<EscalationRegistryQuote>;
  /** Raise the HIC-1 approval (a pending signature ceremony) for one registry request. */
  registryRequest(modelHash: string, input: string, maxPriceWei: string, shownMaxPriceWei: string): Promise<void>;
  registryResult(requestId: number): Promise<EscalationRegistryResult>;
  registryMine(): Promise<EscalationRegistryMine>;
  /** Raise the HIC-1 approval to withdraw the refund the router credited back. */
  registryClaimRefund(): Promise<void>;
  /** Raise the HIC-1 approval to expire one of your requests its provider never answered (after 1 hour). */
  registryExpire(requestId: number): Promise<void>;
}
