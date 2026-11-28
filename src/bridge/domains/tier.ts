// citrate-core: bridge domain `tier`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ---- HUP-S1.6 — hardware tier (02_ARCHITECTURE §3, US-1.6). Mirrors Rust `tier.rs`. ----

export type TierId = "T0" | "T1" | "T2";

/** What this machine reported. `null` = could not be read — shown as unknown, never guessed. */
export interface HardwareFacts {
  os: string;
  arch: string;
  totalRamBytes: number | null;
  /** true on Apple Silicon (the GPU shares system memory); null = not known either way. */
  unifiedMemory: boolean | null;
  /** Largest dedicated GPU memory (NVIDIA only today); null = unknown / none. */
  gpuVramBytes: number | null;
  diskFreeBytes: number | null;
}

export interface TierProfile {
  tier: TierId;
  /** Display-only model family for the tier (S1.7 finalizes the picks). */
  modelHint: string;
  /** Normalized filename fragments identifying a matching model file. */
  modelMatch: string[];
  ctxTokens: number;
}

export interface TierRecommendation extends TierProfile {
  rationale: string[];
  /** T0: the guided / escalate tier. */
  guided: boolean;
  usableBytes: number | null;
}

export interface TierReport {
  facts: HardwareFacts;
  recommendation: TierRecommendation;
  /** The persisted user choice, or null when the recommendation applies. */
  overrideTier: TierId | null;
  /** The tier in effect (override ?? recommendation). */
  effective: TierId;
  profiles: TierProfile[];
}

export interface TierDomain {
  /** Probe this machine locally (no network) and recommend a tier. null = no hardware read is
   *  possible here (web preview) — never a fabricated machine. */
  recommend(): Promise<TierReport | null>;
  /** Persist the user's tier choice (null clears it). Stores only the tier id. */
  setOverride(tier: TierId | null): Promise<TierId | null>;
}
