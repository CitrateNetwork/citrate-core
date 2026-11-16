// Test fixture (0.5.4 WP-H9): eval gate records and an overview in the exact shapes core's
// fl_overview returns (src-tauri fl_rounds.rs, serde camelCase). Imported only by tests;
// production reads both from core.
import type { FlAdapterGateRecord, FlOverview } from "../../bridge/domains";

export const ADAPTER_SHA = "b".repeat(64);
export const ROUND_ID = "0x" + "c".repeat(64);

export function gateRecord(over: Partial<FlAdapterGateRecord> = {}, verdict: "ACCEPT" | "REJECT" = "ACCEPT"): FlAdapterGateRecord {
  return {
    adapterSha256: ADAPTER_SHA,
    adapterPath: "/tmp/adapter.gguf",
    baseModel: "gemma-4-E4B-it-Q4_0.gguf",
    decidedAtMs: 1_700_000_000_000,
    round: { roundId: ROUND_ID, recordDigest: "0x" + "d".repeat(64), adapterSha256: ADAPTER_SHA, chainId: 1, ledger: "0x" + "e".repeat(40), participants: 3 },
    decision: {
      verdict,
      reasons: verdict === "ACCEPT" ? [] : ["correct_tool_rate got worse (0.910 to 0.880)"],
      metrics: [
        { metric: "valid_tool_call_rate", base: 0.95, candidate: 0.97, improvement: 0.02 },
        { metric: "correct_tool_rate", base: 0.91, candidate: verdict === "ACCEPT" ? 0.93 : 0.88, improvement: verdict === "ACCEPT" ? 0.02 : -0.03 },
      ],
      compositeBase: 0.93,
      compositeCandidate: verdict === "ACCEPT" ? 0.95 : 0.925,
    },
    ...over,
  };
}

export function flOverview(over: Partial<FlOverview> = {}): FlOverview {
  return {
    config: { url: null, source: "none", settingsUrl: null, note: null },
    starts: [],
    gates: [],
    activeAdapter: null,
    rememberedAdapter: null,
    restoreError: null,
    consentedRounds: [],
    consentFile: null,
    consentError: null,
    storeError: null,
    ...over,
  };
}
