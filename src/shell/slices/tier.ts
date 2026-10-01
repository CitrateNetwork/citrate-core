// =====================================================================
// citrate-core — tier slice (HUP-S1.6, US-1.6)
//
// The machine's hardware tier: the local recommendation from `bridge.tier.recommend()` (Rust
// `tier.rs` — a local probe, no network), the user's persisted override, and which model files
// belong to the tier in effect. A failed or unavailable probe is an honest `null` + message,
// never a fabricated tier (Rule 1).
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type { TierId, TierProfile, TierReport } from "../../bridge/domains";

export interface TierState {
  /** The last report, or null (not probed yet, web preview, or the probe failed). */
  report: TierReport | null;
  /** The probe has returned (successfully or not). */
  loaded: boolean;
  /** An override save is in flight. */
  saving: boolean;
  error: string | null;
}

export const tierSlice = createSlice<TierState>({ report: null, loaded: false, saving: false, error: null });

const message = (e: unknown): string => (e instanceof Error ? e.message : typeof e === "string" ? e : String(e));

/** Probe this machine (once per call) and store the report. */
export async function refreshTier(): Promise<void> {
  try {
    const report = await bridge.tier.recommend();
    tierSlice.set({ report, loaded: true, error: null });
  } catch (e) {
    tierSlice.set({ report: null, loaded: true, error: message(e) });
  }
}

/** Persist the user's choice (null = follow the recommendation). On failure the previous
 *  choice stays and the error is surfaced. */
export async function setTierOverride(tier: TierId | null): Promise<void> {
  tierSlice.set({ saving: true, error: null });
  try {
    const stored = await bridge.tier.setOverride(tier);
    tierSlice.set((s) => ({
      saving: false,
      report: s.report
        ? { ...s.report, overrideTier: stored, effective: stored ?? s.report.recommendation.tier }
        : s.report,
    }));
  } catch (e) {
    tierSlice.set({ saving: false, error: message(e) });
  }
}

/** The profile (model hint + ctx) of the tier in effect. */
export function effectiveProfile(report: TierReport | null): TierProfile | null {
  if (!report) return null;
  return report.profiles.find((p) => p.tier === report.effective) ?? null;
}

/** Lowercase ASCII alphanumerics only — identical to Rust `tier::normalize_model_key`. */
export function normalizeModelKey(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]/g, "");
}

/** True when `file` belongs to the model family of the tier in effect. */
export function isRecommendedModel(file: string, report: TierReport | null): boolean {
  const p = effectiveProfile(report);
  if (!p) return false;
  const key = normalizeModelKey(file);
  return p.modelMatch.some((m) => key.includes(m));
}
