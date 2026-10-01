// HUP-S1.6 — TEST-ONLY sample tier report (imported by the tier slice / TierPanel tests; never by
// app code). A 32 GB Apple Silicon machine, shaped exactly like the Rust `TierReport`.
import type { TierReport } from "../../bridge/domains";

export function sampleReport(over: Partial<TierReport> = {}): TierReport {
  const profiles = [
    { tier: "T0" as const, modelHint: "Gemma 4 E4B (Q4)", modelMatch: ["gemma4e4b"], ctxTokens: 16384 },
    { tier: "T1" as const, modelHint: "Qwen 3.6 27B (Q4) or Gemma 4 26B", modelMatch: ["qwen3627b", "gemma426b"], ctxTokens: 32768 },
    { tier: "T2" as const, modelHint: "Qwen 3.6 35B-A3B", modelMatch: ["qwen3635ba3b"], ctxTokens: 65536 },
  ];
  return {
    facts: { os: "macos", arch: "aarch64", totalRamBytes: 32 * 2 ** 30, unifiedMemory: true, gpuVramBytes: null, diskFreeBytes: 200 * 2 ** 30 },
    recommendation: {
      ...profiles[2],
      rationale: ["32 GB unified memory (shared with the GPU)", "24 GB or more usable → T2"],
      guided: false,
      usableBytes: 25 * 2 ** 30,
    },
    overrideTier: null,
    effective: "T2",
    profiles,
    ...over,
  };
}
