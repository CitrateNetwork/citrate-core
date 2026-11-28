// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L7 (Networking: the fleet pairing link).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L7 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import type { PersistScope } from "../netScope";

export interface L7State {
  /**
   * HUP-S8.2 — a `citrate://pair?…` fleet pairing link opened from a QR code or another app
   * (transient, not persisted). The Cluster surface's wizard fills it in; nothing pairs until the
   * member presses "Pair". Null when none is pending.
   */
  pendingPairLink: string | null;
}

export function freshL7(_pid: string): L7State {
  return {
    pendingPairLink: null,
  };
}

export const L7_PERSIST_SCOPE = {} satisfies Partial<Record<keyof L7State, PersistScope>>;
