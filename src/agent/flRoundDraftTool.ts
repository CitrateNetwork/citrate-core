// =====================================================================
// citrate-core: Hermes's round draft tool (0.5.4, Hermes x FL WP-H5, gap analysis §5.3)
//
//   fl_round_draft   read, or fill in, the Open a round draft on this device   (draft only, no SALT)
//
// Data source (Rule 7): the board 07 draft store, src/fl/roundDraft.ts (`loadRoundDraft`,
// `setRoundDraft`, `leasesCovered`, `draftProblems`), the same draft the member edits on
// Groups › Rounds › Open a round. The group id is checked against the member's comms groups
// (bridge.groups.list, the comms daemon).
//
// A draft is not a round: saving one escrows nothing, publishes nothing and signs nothing. Opening
// the round stays the member's HIC-1 escrow signature on the board, which Hermes cannot press. So
// a Hermes draft needs no card, with one exception: a draft the member wrote themselves is never
// replaced without their Approve. Every save is marked "Drafted by Hermes" on the board and toasted.
// =====================================================================
import type { ApprovalCard } from "./approvalCards";
import { fieldsCard } from "./approvalCards";
import type { ToolAnnotation } from "./toolAnnotations";
import type { CerSpec } from "../shell/state";
import { BLANK_DRAFT, draftProblems, leasesCovered, loadRoundDraft, setRoundDraft, type RoundDraft, type RoundTemplate, type WhoMayJoin } from "../fl/roundDraft";

export const FL_ROUND_DRAFT_TOOL = "fl_round_draft";

export interface FlRoundDraftDeps {
  load: () => RoundDraft | null;
  save: (d: Partial<RoundDraft>, source: RoundDraft["source"]) => void;
  /** The member's comms group ids; throws when the daemon cannot be read. */
  groupIds: () => Promise<string[]>;
  ask: (spec: CerSpec, card: ApprovalCard) => Promise<string>;
  toast: (m: string) => void;
}

export const defaultDraftStore = { load: loadRoundDraft, save: setRoundDraft };

export interface FlRoundDraftOutcome {
  status: string;
  result: string;
}

const TEMPLATES: readonly RoundTemplate[] = ["tune", "eval", "data"];
const WHO: readonly WhoMayJoin[] = ["members", "group", "anyone"];
const SALT = /^\d{1,12}(\.\d{1,18})?$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

const FIELDS = ["groupId", "template", "dataClass", "budgetSalt", "rateSalt", "maxWorkers", "deadline", "who", "coordinator"] as const;

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The draft fields in a tool call, checked one by one. Unknown values are refused, never coerced. */
export function draftFieldsOf(args: Record<string, unknown>): { fields: Partial<RoundDraft>; refused: string[] } {
  const fields: Partial<RoundDraft> = {};
  const refused: string[] = [];
  const s = (k: string) => (typeof args[k] === "string" ? (args[k] as string).trim() : args[k] === undefined ? undefined : String(args[k]).trim());
  const groupId = s("groupId");
  if (groupId !== undefined) groupId ? (fields.groupId = groupId) : refused.push("groupId is empty");
  const template = s("template");
  if (template !== undefined) (TEMPLATES as readonly string[]).includes(template) ? (fields.template = template as RoundTemplate) : refused.push(`template must be one of ${TEMPLATES.join(", ")}`);
  const dataClass = s("dataClass");
  if (dataClass !== undefined) dataClass.length <= 120 ? (fields.dataClass = dataClass) : refused.push("dataClass is longer than 120 characters");
  for (const k of ["budgetSalt", "rateSalt"] as const) {
    const v = s(k);
    if (v !== undefined) SALT.test(v) ? (fields[k] = v) : refused.push(`${k} must be a SALT amount like 600 or 1.50`);
  }
  if (args.maxWorkers !== undefined) {
    const n = typeof args.maxWorkers === "number" ? args.maxWorkers : Number(args.maxWorkers);
    Number.isInteger(n) && n >= 1 && n <= 10_000 ? (fields.maxWorkers = n) : refused.push("maxWorkers must be a whole number from 1 to 10000");
  }
  const deadline = s("deadline");
  if (deadline !== undefined) DATE.test(deadline) && Number.isFinite(Date.parse(deadline + "T00:00:00Z")) ? (fields.deadline = deadline) : refused.push("deadline must be a date like 2026-12-01");
  const who = s("who");
  if (who !== undefined) (WHO as readonly string[]).includes(who) ? (fields.who = who as WhoMayJoin) : refused.push(`who must be one of ${WHO.join(", ")}`);
  const coordinator = s("coordinator");
  if (coordinator !== undefined) {
    if (coordinator === "" || coordinator === "none") fields.coordinator = null;
    else if (/^https?:\/\/[^\s]+$/.test(coordinator) && coordinator.length <= 300) fields.coordinator = coordinator;
    else refused.push("coordinator must be an http(s) URL, or none");
  }
  const draftedFor = s("draftedFor");
  if (draftedFor) fields.draftedFor = draftedFor.slice(0, 80);
  return { fields, refused };
}

/** A draft as Hermes reads it: every field, what the budget covers, and what still blocks it. */
export function formatDraft(d: RoundDraft): string {
  const leases = leasesCovered(d.budgetSalt, d.rateSalt);
  const problems = draftProblems(d);
  return [
    `Round draft on this device (${d.source === "hermes" ? "drafted by Hermes" : "written by the member"}${d.savedAt ? `, saved ${new Date(d.savedAt).toISOString()}` : ""}):`,
    `- group: ${d.groupId ?? "not chosen"}`,
    `- template: ${d.template}`,
    `- data class: ${d.dataClass || "not named"}`,
    `- budget: ${d.budgetSalt} SALT at ${d.rateSalt} SALT per unit, which covers ${leases === null ? "an unknown number of" : leases} leases`,
    `- most workers: ${d.maxWorkers}`,
    `- deadline: ${d.deadline || "not set"}`,
    `- who may join: ${d.who}`,
    `- coordinator: ${d.coordinator ?? "none chosen"}`,
    problems.length ? `Still needed before it can be opened: ${problems.join("; ")}.` : "Every field is filled in.",
    "Nothing is escrowed by a draft. Opening the round is the member's own HIC-1 escrow signature on Groups › Rounds › Open a round; Hermes cannot sign it.",
  ].join("\n");
}

function sameDraft(a: RoundDraft, b: RoundDraft): boolean {
  return FIELDS.every((k) => a[k] === b[k]);
}

export async function runFlRoundDraft(args: Record<string, unknown>, ann: ToolAnnotation | null, deps: FlRoundDraftDeps): Promise<FlRoundDraftOutcome> {
  const current = deps.load();
  const { fields, refused } = draftFieldsOf(args);
  if (refused.length) return { status: "ok", result: `Nothing was saved: ${refused.join("; ")}.` };
  const changes = FIELDS.filter((k) => k in fields);
  if (changes.length === 0) {
    return { status: "ok", result: current ? formatDraft(current) : "There is no round draft on this device yet. Give the fields to draft one." };
  }
  if (fields.groupId) {
    let ids: string[];
    try {
      ids = await deps.groupIds();
    } catch (e) {
      return { status: "ok", result: "couldn't read the member's groups, so nothing was saved: " + msg(e) };
    }
    if (!ids.includes(fields.groupId)) return { status: "ok", result: `The member is not in a group with id ${fields.groupId}. Nothing was saved; groups_list names their groups.` };
  }
  const base = current ?? BLANK_DRAFT;
  const next: RoundDraft = { ...base, ...fields, source: "hermes" };
  if (current && current.source === "member" && !sameDraft(current, next)) {
    const rows = changes.map((k) => ({ k, v: `${String(current[k] ?? "none")} to ${String(next[k] ?? "none")}` }));
    const spec: CerSpec = {
      origin: "chat agent",
      requester: "chat agent · round draft",
      title: "Replace your round draft",
      chainless: true,
      rows,
      cost: "none, a draft escrows nothing",
      sponsor: "your explicit decision (HIC-1)",
      sponsorColor: "var(--tx-3)",
    };
    const r = await deps.ask(spec, fieldsCard(FL_ROUND_DRAFT_TOOL, ann, Object.fromEntries(rows.map((x) => [x.k, x.v])), "change the round draft you wrote"));
    if (r !== "approved") return { status: r, result: "The member kept their own draft; nothing was changed." };
  }
  try {
    deps.save(next, "hermes");
  } catch (e) {
    return { status: "error", result: "couldn't save the draft on this device: " + msg(e) };
  }
  deps.toast("Hermes saved a round draft. Check every field on Groups › Rounds › Open a round.");
  const saved = deps.load() ?? next;
  return { status: "ok", result: "Saved the draft.\n" + formatDraft(saved) };
}
