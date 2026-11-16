// =====================================================================
// 0.5.4 Hermes × FL, WP-H4: the HIC-2 envelope tools (gap analysis §5.2, §4 V7 and V10).
//
//   fl_envelope_read     the envelope, what is left of it, whether it can authorize anything here (read)
//   fl_envelope_propose  turn an envelope on with caps (HIC-1 card), or off now (no card)
//
// Data sources (Rule 7):
//  - core's fl_envelope.rs `fl_envelope_status`: the sealed envelope (fl_work.rs, lane 1) rolled
//    to now, joins left, whether WorkMarket is in this build's address book, the fixed rules and
//    `authorizes` (false in this build). Its own `fl_envelope` scope, never the SIWE budget.
//  - core's fl_work.rs `fl_work_envelope_approve` / `fl_work_envelope_off` through
//    src/fl/authority.ts, which also moves the composer's authority badge.
//
// The caps are enforced in core (fl_envelope.rs `evaluate`), never by Hermes: the first join with
// a new group or poster always asks, the floor comes from the member's terms, and an offer's text
// has no way into the evaluation. On a network without WorkMarket the envelope authorizes nothing,
// and both tools say so. Auto-claim is not offered: payouts open after the security review.
// =====================================================================
import type { ApprovalCard } from "./approvalCards";
import { fieldsCard } from "./approvalCards";
import type { ToolAnnotation } from "./toolAnnotations";
import type { CerSpec } from "../shell/state";
import type { EnvelopeInput, EnvelopeRecord } from "../fl/authority";

export const FL_ENVELOPE_TOOL_NAMES = ["fl_envelope_read", "fl_envelope_propose"] as const;
export type FlEnvelopeToolName = (typeof FL_ENVELOPE_TOOL_NAMES)[number];

export function isFlEnvelopeTool(name: string): name is FlEnvelopeToolName {
  return (FL_ENVELOPE_TOOL_NAMES as readonly string[]).includes(name);
}

/** Core's caps, inclusive (fl_work.rs MAX_JOINS_PER_WEEK, MAX_HOURS_PER_ROUND). */
export const MAX_JOINS_PER_WEEK = 50;
export const MAX_HOURS_PER_ROUND = 72;

/** Mirrors Rust fl_envelope.rs `EnvelopeStatus`. */
export interface FlEnvelopeStatus {
  scope: "fl_envelope";
  envelope: EnvelopeRecord | null;
  joinsLeft: number | null;
  workMarketPinned: boolean;
  authorizes: boolean;
  note: string;
  rules: string[];
}

export interface FlEnvelopeApi {
  status(): Promise<FlEnvelopeStatus>;
  approve(input: EnvelopeInput): Promise<EnvelopeRecord>;
  off(): Promise<EnvelopeRecord | null>;
}

export interface FlEnvelopeDeps {
  api: FlEnvelopeApi;
  ask: (spec: CerSpec, card: ApprovalCard) => Promise<string>;
  toast: (text: string) => void;
}

export interface FlEnvelopeOutcome {
  status: string;
  result: string;
}

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

function whole(v: unknown): number | null {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  return Number.isInteger(n) ? n : null;
}

function enabledOf(v: unknown): boolean | null {
  if (v === true || v === "true") return true;
  if (v === false || v === "false") return false;
  return null;
}

/** The status in words Hermes can quote. */
export function statusWords(s: FlEnvelopeStatus): string {
  const e = s.envelope;
  const state = !e
    ? "No envelope is approved: every action asks the member (HIC-1)."
    : !e.on
      ? `The envelope "${e.name}" is off: every action asks the member (HIC-1).`
      : `The envelope "${e.name}" is on: ${s.joinsLeft ?? 0} of ${e.joinsPerWeek} joins left this week, up to ${e.maxHoursPerRound} machine hours per round.`;
  const reach = s.authorizes ? "" : ` It authorizes nothing yet: ${s.note}`;
  return `${state}${reach} Fixed rules, enforced by core: ${s.rules.join(" ")}`;
}

export async function runFlEnvelopeTool(name: FlEnvelopeToolName, args: Record<string, unknown>, ann: ToolAnnotation | null, d: FlEnvelopeDeps): Promise<FlEnvelopeOutcome> {
  if (name === "fl_envelope_read") {
    try {
      return { status: "ok", result: statusWords(await d.api.status()) };
    } catch (e) {
      return { status: "ok", result: "The envelope could not be read: " + errText(e) };
    }
  }
  const enabled = enabledOf(args.enabled);
  if (enabled === null) return { status: "ok", result: "Say enabled true (propose an envelope) or false (turn it off now); nothing changed." };
  if (!enabled) {
    // Lowering authority needs no card.
    try {
      const e = await d.api.off();
      if (!e) return { status: "ok", result: "There is no envelope to turn off; every action already asks the member." };
      d.toast("Envelope turned off. Every action asks you again.");
      return { status: "ok", result: `Turned the envelope "${e.name}" off; every action asks the member again (HIC-1).` };
    } catch (e) {
      return { status: "ok", result: "The envelope was not turned off: " + errText(e) };
    }
  }
  const joinsPerWeek = whole(args.joinsPerWeek);
  const maxHoursPerRound = whole(args.maxHoursPerRound);
  const name0 = typeof args.name === "string" && args.name.trim() ? args.name.trim() : "Rounds within my terms";
  if (joinsPerWeek === null || joinsPerWeek < 1 || joinsPerWeek > MAX_JOINS_PER_WEEK) {
    return { status: "ok", result: `Joins per week must be a whole number from 1 to ${MAX_JOINS_PER_WEEK}; nothing was proposed.` };
  }
  if (maxHoursPerRound === null || maxHoursPerRound < 1 || maxHoursPerRound > MAX_HOURS_PER_ROUND) {
    return { status: "ok", result: `Machine hours per round must be a whole number from 1 to ${MAX_HOURS_PER_ROUND}; nothing was proposed.` };
  }
  let s: FlEnvelopeStatus;
  try {
    s = await d.api.status();
  } catch (e) {
    return { status: "ok", result: "The envelope could not be read, so nothing was proposed: " + errText(e) };
  }
  const rows = [
    { k: "Name", v: name0 },
    { k: "Joins per week", v: String(joinsPerWeek) },
    { k: "Machine hours per round", v: `up to ${maxHoursPerRound}` },
    { k: "Always asks you", v: "the first join with a new group or poster, an offer below your floor, and anything over these caps" },
    { k: "Never in the envelope", v: "opening a round, publishing a dataset, offboarding, disputes and claims" },
    { k: "On this network", v: s.authorizes ? "joins inside these caps go ahead without asking" : s.note },
    { k: "Turn off", v: "any time in Settings › Work, and it applies at once" },
  ];
  const r = await d.ask(
    {
      origin: "chat agent",
      requester: "dashboard agent · tool fl_envelope_propose",
      title: "Approve an HIC-2 envelope",
      chainless: true,
      rows,
      cost: "none, no chain transaction",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
    },
    fieldsCard("fl_envelope_propose", ann, { name: name0, joinsPerWeek, maxHoursPerRound }, `approve the envelope "${name0}"`),
  );
  if (r !== "approved") {
    return { status: r, result: r === "expired" ? "The approval expired because the run ended; no envelope changed." : "The member declined; no envelope changed." };
  }
  try {
    const e = await d.api.approve({ name: name0, joinsPerWeek, maxHoursPerRound });
    return {
      status: r,
      result: `The member approved the envelope "${e.name}": ${e.joinsPerWeek} joins per week, up to ${e.maxHoursPerRound} machine hours per round.${s.authorizes ? "" : " " + s.note}`,
    };
  } catch (e) {
    return { status: r, result: "The envelope was not saved: " + errText(e) };
  }
}
