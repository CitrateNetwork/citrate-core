// =====================================================================
// citrate-core: Hermes's round draft read (0.5.4, Hermes x FL WP-H5, gap analysis §5.3)
//
//   fl_round_draft_read   the Open a round draft on this device, and what still blocks it   (read)
//
// Data source (Rule 7): the board 07 draft store, src/fl/roundDraft.ts (`loadRoundDraft`,
// `leasesCovered`, `draftProblems`), the same draft the member edits on Groups › Rounds › Open a
// round. This tool only reads it. Writing a draft is lane 3's `round_draft_propose` card hand-off
// (#725), the one write path: the member presses "Review in Open a round" themselves.
//
// The draft can hold another member's text when it came from a group card, so the free-text
// fields reach the model fenced; the computed lines (leases covered, what is missing) are core's.
// =====================================================================
import { draftProblems, leasesCovered, loadRoundDraft, type RoundDraft } from "../fl/roundDraft";
import { fenceUntrusted } from "./untrusted";

export const FL_ROUND_DRAFT_READ_TOOL = "fl_round_draft_read";

export interface FlRoundDraftReadDeps {
  load: () => RoundDraft | null;
}

export const defaultDraftReadDeps: FlRoundDraftReadDeps = { load: loadRoundDraft };

/** A draft as Hermes reads it: every field, what the budget covers, and what still blocks it. */
export function formatDraft(d: RoundDraft): string {
  const leases = leasesCovered(d.budgetSalt, d.rateSalt);
  const problems = draftProblems(d);
  const fields = {
    group: d.groupId ?? "not chosen",
    template: d.template,
    dataClass: d.dataClass || "not named",
    budgetSalt: d.budgetSalt,
    rateSaltPerUnit: d.rateSalt,
    maxWorkers: d.maxWorkers,
    deadline: d.deadline || "not set",
    whoMayJoin: d.who,
    coordinator: d.coordinator ?? "none chosen",
    ...(d.draftedFor ? { draftedFor: d.draftedFor } : {}),
  };
  return [
    `Round draft on this device (${d.source === "hermes" ? "handed in from a Hermes card" : "written by the member"}${d.savedAt ? `, saved ${new Date(d.savedAt).toISOString()}` : ""}) [this device, the Open a round draft]:`,
    fenceUntrusted("the round draft's fields", fields),
    `The budget covers ${leases === null ? "an unknown number of" : leases} leases at that rate.`,
    problems.length ? `Still needed before it can be opened: ${problems.join("; ")}.` : "Every field is filled in.",
    "Nothing is escrowed by a draft. To change it, propose a new draft with round_draft_propose, or the member edits it on Groups › Rounds › Open a round. Opening the round is the member's own HIC-1 escrow signature; Hermes cannot sign it.",
  ].join("\n");
}

export function runFlRoundDraftRead(deps: FlRoundDraftReadDeps = defaultDraftReadDeps): string {
  let d: RoundDraft | null;
  try {
    d = deps.load();
  } catch (e) {
    return "couldn't read the round draft on this device: " + (e instanceof Error ? e.message : String(e));
  }
  return d ? formatDraft(d) : "There is no round draft on this device yet.";
}
