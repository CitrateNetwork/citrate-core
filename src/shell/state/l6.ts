// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L6 (Connections and marketplace: connections, the AI provider route).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L6 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import type { PersistScope } from "../netScope";

export interface L6State {
  connections: Record<string, boolean>;
  /**
   * CORE-AI1 (@rule8) — the DEFAULT AI provider route id (a non-secret id like
   * "openai" / "gateway" / "custom"). The PROVIDER KEY is NOT here and NOT
   * persisted: it lives sealed in the OS keyring (Rust), never in AppState or
   * localStorage. `aiKeys` was REMOVED — the webview holds no key (invariant 2).
   * Whether a route is actually usable is read live via bridge.chat.providerStatus.
   */
  aiDefault: string;
  aiEdit: string | null;
}

export function freshL6(_pid: string): L6State {
  return {
    connections: {},
    // AI provider KEYS live in the OS keyring (Rust), never in AppState. Only the
    // non-secret default route id is kept here (AI1, invariant 2).
    aiDefault: "gateway",
    aiEdit: null,
  };
}

const D = "device" as const;
export const L6_PERSIST_SCOPE = {
  // NOTE: `aiKeys` is REMOVED (AI1) — provider keys live in the OS keyring, never
  // localStorage (invariant 2). Only the non-secret `aiDefault` route id persists.
  connections: D, aiDefault: D,
} satisfies Partial<Record<keyof L6State, PersistScope>>;
