// =====================================================================
// citrate-core: the FL facts in Hermes's live snapshot (0.5.4, Hermes x FL WP-H12, gap §7.7)
//
// So Hermes says "not available on this network" from data, not memory. Data sources (Rule 7):
// the address book this build ships (src-tauri/addresses/40204.json) and the active network
// profile (payoutsOpen, G-SEC). Book only: no chain read happens at session start, so a pinned
// contract is "pinned in the address book" and Hermes reads its state with a tool.
// =====================================================================
import { FL_CONTRACTS, NOT_AVAILABLE, NO_TRAINING_YET, PAYOUTS_DARK, ROUNDS_NOT_LIVE, bookOnlyState, payoutsEnabledBy, type FlBook } from "./networkState";
import type { NetworkProfile } from "../network/profile";
import { FL_TOOLS_IN_BUILD } from "../agent/flPrompt";

export const PINNED = "pinned in the address book (read its state with a tool)";

export interface FlSnapshot {
  source: string;
  contracts: Record<string, string>;
  groupRounds: string;
  payouts: string;
  training: string;
  envelope: string;
}

export function flSnapshot(book: FlBook, profile: NetworkProfile | null, tools: ReadonlySet<string> = FL_TOOLS_IN_BUILD): FlSnapshot {
  const st = bookOnlyState(book, profile);
  const contracts: Record<string, string> = {};
  for (const n of FL_CONTRACTS) contracts[n] = st.contracts[n].kind === "not-in-book" ? NOT_AVAILABLE : PINNED;
  const work = st.contracts.WorkMarket.kind !== "not-in-book";
  return {
    source: "the address book this build ships and the network profile",
    contracts,
    groupRounds: work ? "WorkMarket is pinned: read offers and rounds with tools" : ROUNDS_NOT_LIVE,
    payouts: !work ? `${NOT_AVAILABLE} (WorkMarket)` : payoutsEnabledBy(profile) ? "opened by the network profile; read them with a tool" : PAYOUTS_DARK,
    training: NO_TRAINING_YET,
    envelope: tools.has("fl_envelope_read") ? "read it with fl_envelope_read; never state caps from memory" : "HIC-2 envelopes are not in this build: every FL action is HIC-1",
  };
}
