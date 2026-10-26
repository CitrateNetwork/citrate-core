// Test-only builders for Settings › Models states (D-11). Shapes mirror Rust `providers_ipc.rs` views.
import type { ProviderStatus, ProviderUsage } from "../../../bridge/domains";

export const DAY = 86_400_000;
export const NOW = 20_752 * DAY + 3_600_000; // 2026-10-26 01:00 UTC

export function vendor(over: Partial<ProviderStatus> = {}): ProviderStatus {
  return {
    id: "openai",
    name: "OpenAI",
    kind: "vendor",
    verified: true,
    baseUrl: "https://api.openai.com/v1",
    host: "api.openai.com",
    adapter: "openai-compatible",
    keyConfigured: false,
    keySetMs: null,
    keyPage: "https://platform.openai.com/api-keys",
    revokePage: "https://platform.openai.com/api-keys",
    confirmed: true,
    halt: null,
    caps: { dailyTokens: 1_000_000, monthlyTokens: 10_000_000, dailyMicros: 5_000_000, monthlyMicros: 50_000_000 },
    approvedModels: [],
    listedModels: [],
    prices: {},
    ...over,
  };
}

export function usage(over: Partial<ProviderUsage> = {}): ProviderUsage {
  const zero = { calls: 0, tokens: 0, spendMicros: 0, unpricedCalls: 0 };
  return { id: "openai", today: zero, month: zero, unsettledCalls: 0, caps: vendor().caps, ...over };
}

