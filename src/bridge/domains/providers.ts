// citrate-core: bridge domain `providers`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ---- D-11 (RC52) — cloud model providers (ADR-2026-10-26). Mirrors Rust `providers_ipc.rs`. ----
// Keys go in one way (`setKey`) and never come back. Amounts are integer micro-USD; token counts are
// the vendor's own. Unknown is `null`, never zero.

/** `delegate` is Cognition (Devin): a key and a usage row, never a chat model. `gateway` appears in
 *  the usage summary only (its key is set in the chat presets). */
export type ProviderKind = "vendor" | "delegate" | "gateway" | "custom";
export interface ProviderCaps {
  dailyTokens: number | null;
  monthlyTokens: number | null;
  dailyMicros: number | null;
  monthlyMicros: number | null;
}
export interface ProviderPrice {
  inputMicrosPerMtok: number;
  outputMicrosPerMtok: number;
}
/** A model core knows for a provider: its price and where the price came from. */
export interface ProviderModelPrice {
  id: string;
  price: ProviderPrice | null;
  /** `member` (entered in Settings, wins), `published` (the vendor's cited price), or null (unknown). */
  source: "member" | "published" | null;
  /** The least core reserves against the caps before one call is sent (empty prompt, the adapter's
   *  output bound), micro-USD; null when the model has no price. */
  minReserveMicros: number | null;
}
export interface ProviderHalt {
  /** `member`, `burst`, `unknownModel`, `tokenSpike` or `guardUnreadable`. */
  kind: string;
  message: string;
  atMs: number;
}
export interface ProviderStatus {
  id: string;
  name: string;
  kind: ProviderKind;
  /** A governed vendor on its pinned host. Always false for a custom endpoint. */
  verified: boolean;
  baseUrl: string;
  host: string;
  /** `native` (a governed vendor's own API) or `openai-compatible` (a custom endpoint); null = not available in this build. */
  adapter: string | null;
  keyConfigured: boolean;
  keySetMs: number | null;
  keyPage: string | null;
  revokePage: string | null;
  /** Custom endpoints: whether the member confirmed this exact URL (calls are refused until then). */
  confirmed: boolean;
  halt: ProviderHalt | null;
  caps: ProviderCaps;
  approvedModels: string[];
  listedModels: string[];
  prices: Record<string, ProviderPrice>;
  /** Every known model with its price source. Under a spending cap an unpriced model is refused. */
  models: ProviderModelPrice[];
}
export interface ProviderPeriodUsage {
  calls: number;
  tokens: number;
  /** Only priced calls; `unpricedCalls` counts the rest (their spend is unknown, not zero). */
  spendMicros: number;
  unpricedCalls: number;
}
export interface ProviderUsage {
  id: string;
  today: ProviderPeriodUsage;
  month: ProviderPeriodUsage;
  unsettledCalls: number;
  caps: ProviderCaps;
}
/** One recent cloud call from the signed usage log. Metadata only: the log has no field for content. */
export interface ProviderRecentCall {
  atMs: number;
  provider: string;
  model: string;
  /** False for a custom (unverified) endpoint. */
  verified: boolean;
  /** `ok`, a coarse failure or refusal code, or null while unsettled (in flight, or cut off by a crash). */
  outcome: string | null;
  /** True when the call was refused before anything was sent. */
  refused: boolean;
  tokensIn: number | null;
  tokensOut: number | null;
  /** null = unknown (no price, or not reported), never zero. */
  costMicros: number | null;
}
export interface ProviderUsageSummary {
  /** The signed usage log verified; when false every cloud call is refused. */
  logIntact: boolean;
  logProblem: string | null;
  records: number;
  asOfMs: number;
  providers: ProviderUsage[];
  /** The latest calls, newest first (bounded in core). */
  recentCalls: ProviderRecentCall[];
  /** Set when this computer's clock reads well before the last record: every call is refused. */
  clockProblem: string | null;
}
export interface ProviderKeyTest {
  ok: boolean;
  listedModels: number;
}
export interface ProviderCustomEndpoint {
  id: string;
  label: string;
  baseUrl: string;
  models: string[];
  confirmed: boolean;
  verified: false;
}
export interface ProvidersDomain {
  status(): Promise<ProviderStatus[]>;
  usageSummary(): Promise<ProviderUsageSummary>;
  setCaps(providerId: string, caps: ProviderCaps): Promise<ProviderStatus>;
  setPrice(providerId: string, model: string, price: ProviderPrice | null): Promise<ProviderStatus>;
  halt(providerId: string): Promise<ProviderStatus>;
  resume(providerId: string): Promise<ProviderStatus>;
  /** One way: format-checked and tested live with the vendor before it is stored; never returned. */
  setKey(providerId: string, apiKey: string): Promise<ProviderKeyTest>;
  testKey(providerId: string): Promise<ProviderKeyTest>;
  removeKey(providerId: string): Promise<void>;
  addCustom(label: string, baseUrl: string, models: string[]): Promise<ProviderCustomEndpoint>;
  /** The member confirms by echoing the exact URL. */
  confirmCustom(id: string, baseUrl: string): Promise<ProviderCustomEndpoint>;
  removeCustom(id: string): Promise<void>;
  /** Only a broken log is reset (archived aside, a new chain started). */
  resetUsageLog(): Promise<ProviderUsageSummary>;
  /** D-11 (P-5) — the member's preferred cloud model for Hermes (null = the local model). */
  preferredGet(): Promise<ProviderPreferredView>;
  /** The provider must be connected and able to take calls; the model is approved for the anomaly
   *  halt. `null` clears it. */
  preferredSet(choice: ProviderPreferredModel | null): Promise<ProviderPreferredView>;
}
export interface ProviderPreferredModel {
  providerId: string;
  model: string;
}
export interface ProviderPreferredView {
  preferred: ProviderPreferredModel | null;
  /** The stored setting could not be read (Hermes then uses the local model). */
  loadError: string | null;
}
