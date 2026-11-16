// =====================================================================
// citrate-core: Hermes's round read tools (0.5.4, Hermes x FL WP-H5, gap analysis §5.3)
//
//   fl_round_status   one round's state, each field tagged by where it came from   (read)
//   fl_lease_explain  why a round (and so a lease in it) was rejected              (read)
//
// Data sources (Rule 7), each named next to its value in the answer:
//  - which FL contracts this network has: the shipped address book plus eth_getCode
//    (src/fl/networkState.ts readFlNetworkState, the same read the FL boards make);
//  - FederatedRoundLedger.getRound(roundId): deployed on r1007, so this half is live now;
//  - WorkMarket.getOffer(id) and the round its bindRef names: read only once the address book
//    pins WorkMarket and its code is on chain (src/fl/workMarketReads.ts, src/fl/myWork.ts,
//    shared with the Live round board). On r1007 it is not pinned, and the answer says
//    "Group rounds not live yet" from that data, with no app update needed when it lands;
//  - the training coordinator's own `/v1/status` counts, through core's fl_round_plan read,
//    labelled "coordinator-reported" because they are not on chain.
//
// What no source holds is said, never filled in (NOT_ON_CHAIN): per-lease receipts are the
// coordinator's and core does not read them; the Belnap counts are not in the on-chain record.
// =====================================================================
import { NOT_AVAILABLE, ROUNDS_NOT_LIVE, contractWords, type FlNetworkState } from "../fl/networkState";
import { escrowOf, liveStateOf, NOT_ON_CHAIN } from "../fl/myWork";
import { getFrlRound, getOffer, type ChainOffer, type ChainRound, type ViewCall } from "../fl/workMarketReads";
import { saltText } from "../fl/format";
import type { FlCoordinatorView } from "../bridge/domains";

export const FL_ROUND_TOOL_NAMES = ["fl_round_status", "fl_lease_explain"] as const;
export type FlRoundToolName = (typeof FL_ROUND_TOOL_NAMES)[number];

export function isFlRoundTool(name: string): name is FlRoundToolName {
  return (FL_ROUND_TOOL_NAMES as readonly string[]).includes(name);
}

export interface FlRoundDeps {
  /** The FL network state from the address book and eth_getCode. */
  networkState: () => Promise<FlNetworkState>;
  /** eth_call on the active network (core's contract_view_call). */
  call: ViewCall;
  /** The configured coordinator's view, or null when none is configured. Throws when core cannot say. */
  coordinator: () => Promise<FlCoordinatorView | null>;
}

const ROUND_ID = /^0x[0-9a-f]{64}$/;
const ZERO32 = /^0x0{64}$/;

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** What each FederatedRoundLedger fault means (citrate-chain contracts/src/FederatedRoundLedger.sol). */
export const FAULT_WORDS: Record<ChainRound["fault"], string> = {
  none: "no fault is recorded",
  "output mismatch": "the ledger re-ran the Belnap aggregation (precompile 0x0110) on a committed input and got a different output than the coordinator committed",
  "row mismatch": "a participant's row inside a committed input is not the row that participant's committed update holds",
  "rule violation": "a committed input broke the cluster's rules: its shape, weights, confidences or thresholds",
  "participant order": "the participants were not in strictly ascending order, so one worker was counted twice",
};

function roundIdOf(v: unknown): string | null {
  const s = typeof v === "string" ? v.trim().toLowerCase() : "";
  return ROUND_ID.test(s) ? s : null;
}

function offerIdOf(v: unknown): number | null {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  return Number.isInteger(n) && n >= 1 && n <= Number.MAX_SAFE_INTEGER ? n : null;
}

/** The round ledger's record as lines, each tagged by source. */
export function formatFrlRound(roundId: string, r: ChainRound): string[] {
  const src = "[FederatedRoundLedger.getRound, on chain]";
  if (r.status === "none") return [`FederatedRoundLedger holds no round with id ${roundId} on this network. ${src}`];
  const lines = [`Round ${roundId}: ${r.status} ${src}`, `- round ordinal in its cluster: ${r.ordinal} ${src}`, `- devices that took part: ${r.participants} ${src}`];
  if (r.status === "committed") lines.push(`- challenge window: accepted after block ${r.deadlineBlock} unless a challenge proves it wrong first ${src}`);
  if (r.status === "accepted") lines.push(`- accepted: no challenge proved it wrong before block ${r.deadlineBlock} ${src}`);
  if (r.status === "rejected") lines.push(`- rejected: ${r.fault}, meaning ${FAULT_WORDS[r.fault]}. A rejected round never pays. ${src}`);
  lines.push(`- merged adapter hash: ${ZERO32.test(r.adapterHash) ? "none recorded" : r.adapterHash} ${src}`);
  return lines;
}

/** A WorkMarket offer as lines, each tagged by source. */
export function formatOffer(o: ChainOffer, r: ChainRound | null): string[] {
  const src = "[WorkMarket.getOffer, on chain]";
  const e = escrowOf(o);
  return [
    `Offer ${o.id} (${o.kind === "flRound" ? "federated round" : "task"}): ${liveStateOf(o, r)} ${src}`,
    `- posted by ${o.poster}; coordinator ${o.coordinator} ${src}`,
    `- workers: ${o.workers} of at most ${o.maxWorkers} ${src}`,
    `- rate: ${saltText(o.pricePerUnit)} per unit; join bond ${saltText(o.joinBond)} ${src}`,
    `- escrow: budget ${saltText(e.budgetWei)}, paid ${saltText(e.paidWei)}, reserved for posted units ${saltText(e.reservedWei)}, refundable ${saltText(e.refundableWei)} ${src}`,
    `- deadline: ${new Date(o.deadline * 1000).toISOString()} ${src}`,
  ];
}

/** The coordinator's own counts, labelled as not on chain. */
async function coordinatorLines(deps: FlRoundDeps): Promise<string[]> {
  let c: FlCoordinatorView | null;
  try {
    c = await deps.coordinator();
  } catch (e) {
    return [`Coordinator: — (core could not read the coordinator setting: ${msg(e)})`];
  }
  if (!c || c.state === "notConfigured") return [`Coordinator: none is configured on this device, so there are no lease counts. ${NOT_ON_CHAIN.leases}.`];
  if (c.state === "unreachable") return [`Coordinator ${c.url}: — (it did not answer: ${c.reason})`];
  const s = c.status;
  const tag = "[coordinator-reported, its /v1/status, not on chain]";
  return [
    `Coordinator ${c.url}: ${s.phase}, settlement ${s.settlement} ${tag}`,
    `- leases: ${s.pending} pending, ${s.leased} leased, ${s.done} done, ${s.quarantined} quarantined; ${s.workers} workers ${tag}`,
  ];
}

async function status(args: Record<string, unknown>, deps: FlRoundDeps): Promise<string> {
  const rawRound = args.roundId;
  const rawOffer = args.offerId;
  const roundId = rawRound === undefined ? null : roundIdOf(rawRound);
  const offerId = rawOffer === undefined ? null : offerIdOf(rawOffer);
  if (rawRound !== undefined && !roundId) return "fl_round_status needs the round id as 0x followed by 64 hex characters. Nothing was read.";
  if (rawOffer !== undefined && offerId === null) return "fl_round_status needs the offer id as a whole number from 1. Nothing was read.";
  let net: FlNetworkState;
  try {
    net = await deps.networkState();
  } catch (e) {
    return "couldn't read which FL contracts this network has: " + msg(e);
  }
  const frl = net.contracts.FederatedRoundLedger;
  const wm = net.contracts.WorkMarket;
  const out: string[] = [];
  if (offerId !== null) {
    if (wm.kind === "deployed") {
      try {
        const o = await getOffer(deps.call, wm.address, offerId);
        const bound = o.kind === "flRound" && frl.kind === "deployed" && !ZERO32.test(o.bindRef) ? await getFrlRound(deps.call, frl.address, o.bindRef) : null;
        out.push(...formatOffer(o, bound));
        if (bound) out.push(...formatFrlRound(o.bindRef, bound));
      } catch (e) {
        out.push(`Offer ${offerId}: — (the WorkMarket read failed: ${msg(e)})`);
      }
    } else if (wm.kind === "unknown") {
      out.push(`Offer ${offerId}: — (whether WorkMarket is on this network is unknown: ${wm.reason})`);
    } else {
      out.push(`${ROUNDS_NOT_LIVE}. WorkMarket is ${NOT_AVAILABLE.toLowerCase()} [${net.networkLabel}, address book and eth_getCode], so there is no offer ${offerId} to read.`);
    }
  }
  if (roundId) {
    if (frl.kind === "deployed") {
      try {
        out.push(...formatFrlRound(roundId, await getFrlRound(deps.call, frl.address, roundId)));
      } catch (e) {
        out.push(`Round ${roundId}: — (the FederatedRoundLedger read failed: ${msg(e)})`);
      }
    } else {
      out.push(`Round ${roundId}: — FederatedRoundLedger is ${contractWords(frl).text}${contractWords(frl).unknown ? ` (${contractWords(frl).unknown})` : ""} on ${net.networkLabel}.`);
    }
  }
  if (offerId === null && !roundId) {
    const tag = "[address book and eth_getCode]";
    out.push(`FL contracts on ${net.networkLabel} ${tag}:`);
    for (const n of ["FederatedRoundLedger", "WorkMarket", "FLRoundVerifier", "DatasetRegistry", "PatronageLedger", "LoRAFactory"] as const) {
      const w = contractWords(net.contracts[n]);
      out.push(`- ${n}: ${w.text}${w.unknown ? ` (${w.unknown})` : ""}`);
    }
    if (net.phase === "today") out.push(`${ROUNDS_NOT_LIVE}: rounds reach members only through WorkMarket offers. FederatedRoundLedger is ${contractWords(frl).text}; give a round id to read one record.`);
    else if (net.phase === "rounds-live") out.push("Give an offer id or a round id to read one round.");
    else if (net.phaseReason) out.push(`Round availability is unknown: ${net.phaseReason}`);
  }
  out.push(...(await coordinatorLines(deps)));
  out.push(`Not shown: ${NOT_ON_CHAIN.belnap}; per-lease receipts are the coordinator's, and core does not read them.`);
  return out.join("\n");
}

async function leaseExplain(args: Record<string, unknown>, deps: FlRoundDeps): Promise<string> {
  const roundId = roundIdOf(args.roundId);
  if (!roundId) return "fl_lease_explain needs the round id as 0x followed by 64 hex characters. Nothing was read.";
  const lease = args.leaseIndex === undefined ? null : Number(args.leaseIndex);
  if (lease !== null && !(Number.isInteger(lease) && lease >= 0)) return "fl_lease_explain needs the lease index as a whole number from 0. Nothing was read.";
  let net: FlNetworkState;
  try {
    net = await deps.networkState();
  } catch (e) {
    return "couldn't read which FL contracts this network has: " + msg(e);
  }
  const frl = net.contracts.FederatedRoundLedger;
  const leaseNote =
    lease === null
      ? ""
      : `\nLease ${lease}: its receipt is the coordinator's, and core does not read per-lease receipts yet, so this can say why the round was rejected but not which lease caused it.`;
  if (frl.kind !== "deployed") {
    const w = contractWords(frl);
    return `Round ${roundId}: — FederatedRoundLedger is ${w.text}${w.unknown ? ` (${w.unknown})` : ""} on ${net.networkLabel}.` + leaseNote;
  }
  let r: ChainRound;
  try {
    r = await getFrlRound(deps.call, frl.address, roundId);
  } catch (e) {
    return `Round ${roundId}: — (the FederatedRoundLedger read failed: ${msg(e)})` + leaseNote;
  }
  const src = "[FederatedRoundLedger.getRound, on chain]";
  let why: string;
  if (r.status === "none") why = `FederatedRoundLedger holds no round with id ${roundId} on this network. ${src}`;
  else if (r.status === "rejected") why = `Round ${roundId} was rejected: ${r.fault}. That means ${FAULT_WORDS[r.fault]}. Someone proved it on chain with a challenge, and a rejected round never pays. ${src}`;
  else if (r.status === "committed") why = `Round ${roundId} has not been rejected. It is committed and accepted after block ${r.deadlineBlock} unless a challenge proves it wrong first. ${src}`;
  else why = `Round ${roundId} was accepted; no fault was proved. ${src}`;
  return why + leaseNote;
}

export async function runFlRoundTool(name: FlRoundToolName, args: Record<string, unknown>, deps: FlRoundDeps): Promise<string> {
  return name === "fl_round_status" ? status(args, deps) : leaseExplain(args, deps);
}
