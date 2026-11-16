// =====================================================================
// citrate-core — a verified Hermes round draft from group chat, handed to Rounds › Open a round.
//
// Board 07 (src/fl/roundDraft.ts, `setRoundDraft`) owns the draft store. This module maps the chat's
// draft onto it and calls `setRoundDraft(draft, "hermes")`, so "Review draft" opens Open a round
// prefilled; the member still checks every field and signs (HIC-1). A value Hermes did not propose
// is left empty for the member to fill, never a default that would read as Hermes's.
//
// roundDraft.ts lands with board 07 (#696). Until a build carries it, the glob below finds no module
// and Review only opens the screen; nothing is written in its place, so there is one draft store.
// =====================================================================
import type { RoundDraft as ChatDraft } from "./groupEnvelope";

/** The board 07 draft fields this hand-off fills (structurally `Partial<RoundDraft>` there). */
export interface OpenRoundFields {
  groupId: string;
  template: "tune";
  dataClass: string;
  budgetSalt: string;
  rateSalt: string;
  maxWorkers?: number;
  deadline: string;
  who: "members";
  draftedFor: string;
}

const dec = (n: number | null) => (n === null ? "" : n.toLocaleString("en-US", { maximumFractionDigits: 18, useGrouping: false }));

/** The chat draft as Open a round's fields. */
export function toOpenRound(d: ChatDraft, groupId: string, draftedFor: string): OpenRoundFields {
  return {
    groupId,
    template: "tune",
    // The chat draft carries a title, not a data class; the member names the data class.
    dataClass: "",
    budgetSalt: dec(d.budgetSalt),
    rateSalt: dec(d.rateSalt),
    ...(d.workers !== null ? { maxWorkers: d.workers } : {}),
    deadline: d.deadline ?? "",
    who: "members",
    draftedFor,
  };
}

type SetRoundDraft = (d: OpenRoundFields, source: "hermes") => void;
const boards = import.meta.glob<{ setRoundDraft?: SetRoundDraft }>("../../fl/roundDraft.ts");

/** Hand the draft to Open a round when this build has it. True when it was handed over. */
export async function handOffDraft(d: ChatDraft, groupId: string, draftedFor: string, load = boards): Promise<boolean> {
  const mod = Object.values(load)[0];
  if (!mod) return false;
  try {
    const m = await mod();
    if (typeof m.setRoundDraft !== "function") return false;
    m.setRoundDraft(toOpenRound(d, groupId, draftedFor), "hermes");
    return true;
  } catch {
    return false;
  }
}
