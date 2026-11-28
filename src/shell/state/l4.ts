// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L4 (Conversations: the chat thread, the Agent surface, groups and invites).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L4 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import { newThreadId } from "../slices/chatHistory";
import type { ChatMsg } from "../state";
import type { PersistScope } from "../netScope";

export interface L4State {
  /**
   * rc.5 FIX 5: a one-shot request for the Agent surface to open a tab (the chat's "Open toolchain
   * settings" asks for "contracts"). The Agent surface consumes and clears it. Not persisted.
   */
  agentTabRequest: "overview" | "contracts" | null;
  chatMsgs: ChatMsg[];
  /** 0.5.4 — the id the current thread is kept under in the chat history (slices/chatHistory.ts).
   *  Not persisted: a launch starts a new thread; the earlier ones stay in the history. */
  chatThreadId: string;
  /**
   * This device's COMMS address (from `bridge.groups.selfAddress()`) — the device-sealed
   * comms key that group rosters are keyed on, which is NOT the wallet. Set during
   * `refreshPeople`; the durable identity for "is this me in the roster" + "can I manage
   * this group" (issue #55 — using the wallet here demoted owners on reload). Empty until
   * a real selfAddress() read lands (never fabricated).
   */
  commsAddr: string;
  /** 0.5.4 — the member collapsed the Hermes chat's right column (widgets, daemons, transactions,
   *  tutorials). Off by default. Persisted. */
  hermesRailCollapsed: boolean;
  /**
   * D-12 — an invite link (`citrate://invite?…`) the OS handed the app or the member pasted, waiting
   * on the "Join <group>?" screen (Groups and onboarding). Nothing is sent until the member presses
   * Join. Transient, not persisted.
   */
  inviteLink: string | null;
}

export function freshL4(_pid: string): L4State {
  return {
    agentTabRequest: null,
    chatMsgs: [],
    chatThreadId: newThreadId(),
    commsAddr: "",
    hermesRailCollapsed: false,
    inviteLink: null,
  };
}

const D = "device" as const;
export const L4_PERSIST_SCOPE = {
  commsAddr: D,
  // 0.5.4: the member's layout choice (collapsed Hermes right column).
  hermesRailCollapsed: D,
} satisfies Partial<Record<keyof L4State, PersistScope>>;
