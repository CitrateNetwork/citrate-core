// =====================================================================
// citrate-core: federated rounds, the shared front-end flow (HUP-S9.4)
//
// One start path for both callers (Hermes's fl_round_start tool and the Train surface): show the
// plan core built on an approval card, and only after the member's explicit Approve (HIC-1, no
// signature, no wallet) ask core to record the start for that exact plan hash. Core re-reads the
// coordinator and refuses if anything changed. Plans, explanations and refusals all come from
// core (src-tauri/src/fl_rounds.rs); nothing here invents a number.
// =====================================================================
import type { CerSpec } from "../shell/state";
import type { FlAdapterGateRecord, FlCapability, FlRoundPlan, FlRoundProposal, FlStartReceipt } from "../bridge/domains";

/** Conservative defaults, pending owner sign-off. Must match RoundProposal::default in core. */
export const DEFAULT_PROPOSAL: FlRoundProposal = {
  requires: "federated",
  loraRank: 8,
  maxTrajectories: 500,
  leaseHours: 6,
};

const CAPS: readonly FlCapability[] = ["probe", "federated", "h01"];

function num(v: unknown, fallback: number): number {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  if (typeof v === "string" && v.trim() !== "" && Number.isFinite(Number(v))) return Number(v);
  return fallback;
}

/**
 * A proposal from a tool call's arguments. Missing values take the defaults; numbers are passed
 * through as given so core's validation refuses a bad one rather than this quietly fixing it.
 */
export function proposalFromToolArgs(args: Record<string, unknown>): FlRoundProposal {
  const req = typeof args.requires === "string" && (CAPS as readonly string[]).includes(args.requires) ? (args.requires as FlCapability) : DEFAULT_PROPOSAL.requires;
  return {
    requires: req,
    loraRank: num(args.loraRank, DEFAULT_PROPOSAL.loraRank),
    maxTrajectories: num(args.maxTrajectories, DEFAULT_PROPOSAL.maxTrajectories),
    leaseHours: num(args.leaseHours, DEFAULT_PROPOSAL.leaseHours),
  };
}

function coordinatorLine(plan: FlRoundPlan): string {
  const c = plan.coordinator;
  if (c.state === "notConfigured") return "not configured";
  if (c.state === "unreachable") return `${c.url} (could not be read: ${c.reason})`;
  return `${c.url} (${c.status.settlement} settlement)`;
}

/** The approval card for starting a round. Chainless: no transaction, no signature. */
export function flStartCerSpec(plan: FlRoundPlan, origin: string): CerSpec {
  return {
    origin,
    requester: origin + " · federated round",
    title: "Join a federated training round",
    chainless: true,
    rows: [
      { k: "Coordinator", v: coordinatorLine(plan) },
      { k: "Data", v: plan.explain.data },
      { k: "Compute", v: plan.explain.compute },
      { k: "Reward", v: plan.explain.reward },
      { k: "Privacy", v: plan.explain.privacy },
      { k: "Plan", v: plan.planHash.slice(0, 16) + "…" },
    ],
    cost: "none, no chain transaction",
    sponsor: "your explicit decision (HIC-1)",
    sponsorColor: "var(--tx-3)",
  };
}

export interface StartDeps {
  requestSig: (spec: CerSpec) => Promise<string>;
  start: (planHash: string) => Promise<FlStartReceipt>;
}

export interface StartOutcome {
  status: "blocked" | "declined" | "approved" | "error";
  message: string;
  receipt?: FlStartReceipt;
}

/**
 * Ask the member about this exact plan, then (only on Approve) ask core to record the start.
 * `decorate` lets a caller attach its approval card / HIC reason to the one ceremony.
 */
export async function approveAndStartRound(
  deps: StartDeps,
  plan: FlRoundPlan,
  origin: string,
  decorate: (spec: CerSpec) => CerSpec = (s) => s,
): Promise<StartOutcome> {
  if (!plan.canStart) {
    return { status: "blocked", message: "This round cannot start: " + plan.blockers.join(" ") };
  }
  const r = await deps.requestSig(decorate(flStartCerSpec(plan, origin)));
  if (r !== "approved") {
    return { status: "declined", message: "The member declined joining the round; nothing was started." };
  }
  try {
    const receipt = await deps.start(plan.planHash);
    return { status: "approved", receipt, message: receipt.note };
  } catch (e) {
    return { status: "error", message: "Core refused the start: " + (e instanceof Error ? e.message : String(e)) };
  }
}

/** What Hermes reads after fl_round_plan: core's words, the hash, and the next step. */
export function formatPlanForAgent(plan: FlRoundPlan): string {
  const lines = [
    `Round plan ${plan.planHash}`,
    `Status: ${plan.explain.status}`,
    `Data: ${plan.explain.data}`,
    `Compute: ${plan.explain.compute}`,
    `Reward: ${plan.explain.reward}`,
    `Privacy: ${plan.explain.privacy}`,
  ];
  if (plan.canStart) {
    lines.push(
      `This plan can start. Explain it to the member in plain words, and if they want to join, call fl_round_start with planHash "${plan.planHash}". The member decides on an approval card; joining records their approval, and this build has no device training worker, so no training runs yet.`,
    );
  } else {
    lines.push("This round cannot start now. Tell the member why:", ...plan.blockers.map((b) => "- " + b));
  }
  return lines.join("\n");
}

const f3 = (v: number | null) => (v === null ? "n/a" : v.toFixed(3));

/** The eval-gate record as a headline plus one line per metric and per reason. */
export function gateSummary(rec: FlAdapterGateRecord): { headline: string; lines: string[] } {
  const d = rec.decision;
  const headline =
    d.verdict === "ACCEPT"
      ? `Passed the eval gate (score ${f3(d.compositeBase)} to ${f3(d.compositeCandidate)})`
      : `Rejected by the eval gate (score ${f3(d.compositeBase)} to ${f3(d.compositeCandidate)})`;
  const lines = d.metrics.map((m) => {
    const delta = m.improvement === null ? "" : ` (${m.improvement >= 0 ? "+" : ""}${m.improvement.toFixed(3)})`;
    return `${m.metric} ${f3(m.base)} to ${f3(m.candidate)}${delta}`;
  });
  return { headline, lines: [...lines, ...d.reasons] };
}
