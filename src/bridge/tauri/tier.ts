// Bridge impl — tier (HUP-S1.6), TAURI. `tier_recommend` probes this machine locally (Rust
// `tier.rs`: RAM, arch, unified memory, NVIDIA VRAM, free disk — no network) and returns the
// recommendation + rationale + stored override. `tier_set_override` persists only the tier id.
import { invoke } from "./invoke";
import type { TierDomain, TierId, TierReport } from "../domains";

export const tauriTier: TierDomain = {
  recommend() {
    return invoke<TierReport | null>("tier_recommend");
  },
  setOverride(tier: TierId | null) {
    return invoke<TierId | null>("tier_set_override", { tier });
  },
};
