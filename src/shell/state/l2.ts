// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L2 (HIC, sandbox and deploy: the terminal tool switch).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L2 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import type { PersistScope } from "../netScope";

export interface L2State {
  /** Let Hermes run terminal commands (the sidecar's `shell_run` tool). Off by default for new
   * installs (owner decision 2026-10-19, superseding 2026-10-04); an install that stored a value
   * keeps it. Every command still needs the member's approval, runs in the OS sandbox, and only in
   * folders the member shared. Core passes it to the sidecar (hermes_terminal.rs). */
  hermesTerminal: boolean;
  /** Marker of the retired 2026-10-04 migration (terminal commands switched on once). Kept so
   * stored states still read; nothing acts on it any more. */
  hermesTerminalDefaultApplied: boolean;
}

export function freshL2(_pid: string): L2State {
  return {
    hermesTerminal: false,
    hermesTerminalDefaultApplied: true,
  };
}

const D = "device" as const;
export const L2_PERSIST_SCOPE = {
  hermesTerminal: D, hermesTerminalDefaultApplied: D,
} satisfies Partial<Record<keyof L2State, PersistScope>>;
