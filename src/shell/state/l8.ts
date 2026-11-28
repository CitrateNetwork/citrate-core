// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L8 (Train together): no AppState field yet.
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L8 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import type { PersistScope } from "../netScope";

/** No AppState field belongs to L8 yet. */
export interface L8State {}

export function freshL8(_pid: string): L8State {
  return {};
}

export const L8_PERSIST_SCOPE = {} satisfies Partial<Record<keyof L8State, PersistScope>>;
