// =====================================================================
// citrate-core: the member's own rounds and payouts, read from WorkMarket and FederatedRoundLedger
//
// Built on workMarketReads.ts. Callers read only once the shipped address book pins WorkMarket
// and its code is on chain; on r1007 nothing here runs. What the chain does not hold is said,
// never filled in (NOT_ON_CHAIN):
//  - leases inside a round are the coordinator's (its /v1/status counts), not on chain;
//  - your units and amount are leaves of the coordinator's posted payout list, not on chain (the
//    contract holds only the list's root);
//  - the Belnap counts are not in the round's on-chain record.
// Shared by the FL boards (Live round, Payouts) and Hermes's FL tools.
// =====================================================================
import { getFrlRound, myOffers, pullBalance, settledFor, type ChainOffer, type ChainRound, type ViewCall } from "./workMarketReads";

export const NOT_ON_CHAIN = {
  leases: "leases are the coordinator's, not on chain",
  units: "your units are in the coordinator's payout list, which the app does not read yet",
  belnap: "the Belnap counts are not in the round's on-chain record",
  training: "the training worker is not bundled in this release",
};

/** The round states the FL design names (board 06). */
export type RoundStateWord =
  | "pending"
  | "running"
  | "verifying"
  | "accepted"
  | "rejected"
  | "disputed"
  | "expired"
  /** Closed with no live payout list: refunded to the poster (for example after a dispute), so
   *  nobody is paid from it. Never "accepted". */
  | "closed";

/** A round's state from its WorkMarket offer and, for an FL round, its FRL record. */
export function liveStateOf(o: ChainOffer, r: ChainRound | null): RoundStateWord {
  switch (o.state) {
    case "open":
      return "pending";
    case "running":
      if (r?.status === "committed") return "verifying";
      if (r?.status === "accepted") return "accepted";
      if (r?.status === "rejected") return "rejected";
      return "running";
    case "payoutPosted":
      return "accepted";
    case "closed":
      // Closed after a settled payout keeps its payout root; closed after a refund (a dispute
      // upheld, or the poster reclaiming) has none and paid nobody from a payout list.
      if (r?.status === "rejected") return "rejected";
      return /^0x0{64}$/i.test(o.payoutRoot) ? "closed" : "accepted";
    case "disputed":
      return "disputed";
    case "expired":
      return "expired";
  }
}

/** Budget split into paid, reserved for posted units, and refundable (wei). */
export function escrowOf(o: ChainOffer): { budgetWei: bigint; paidWei: bigint; reservedWei: bigint; refundableWei: bigint } {
  const owedToWorkers = o.payoutUnits * o.pricePerUnit;
  // Once the poster swept the unsettled reserve, nothing is held back for workers any more.
  const reserved = !o.reserveSwept && owedToWorkers > o.paid ? owedToWorkers - o.paid : 0n;
  const left = o.budget - o.paid - o.refunded - reserved;
  return { budgetWei: o.budget, paidWei: o.paid, reservedWei: reserved, refundableWei: left > 0n ? left : 0n };
}

/**
 * Where this member's payout on an offer stands. Mirrors every precondition of WorkMarket.settle
 * (citrate-chain contracts/src/work/WorkMarket.sol), in its order, so "claimable" is said only
 * when a claim with a valid leaf would not revert:
 *   state is PayoutPosted or Closed; a live payout root; now >= the dispute end (the later of
 *   payoutPostedAt + disputeWindow and answerGraceUntil); no open challenges; the reserve not
 *   swept; joined; not already settled.
 * The leaf itself (units, proof) is the coordinator's and is not checked here.
 */
export type PayoutStatusWord =
  /** You already settled this offer. */
  | "claimed"
  /** The payout list is under dispute; the payout is frozen. */
  | "disputed"
  /** The offer is PayoutPosted or Closed but no payout list is live (withdrawn by a dispute). */
  | "no-payout-list"
  /** Inside the dispute window (or the grace after an answered challenge). */
  | "waiting"
  /** A challenge against the payout list awaits an answer. */
  | "challenge-open"
  /** The poster took back the unsettled reserve; nothing is left to settle. */
  | "reserve-swept"
  /** This wallet did not join the offer. */
  | "not-joined"
  | "claimable";

export const PAYOUT_STATUS_WORDS: Record<PayoutStatusWord, string> = {
  claimed: "Claimed",
  disputed: "Disputed, frozen",
  "no-payout-list": "No payout list is live",
  waiting: "Waiting for the dispute window",
  "challenge-open": "A challenge to the payout list is open",
  "reserve-swept": "The unsettled reserve was returned to the poster",
  "not-joined": "You did not join this round",
  claimable: "Claimable",
};

/** The dispute window's end: the later of payoutPostedAt + disputeWindow and answerGraceUntil. */
export function disputeEnd(o: ChainOffer): number {
  return Math.max(o.payoutPostedAt + o.disputeWindow, o.answerGraceUntil);
}

const ZERO_ROOT = /^0x0{64}$/i;

/** Where this member's payout stands, or null before a payout list was ever posted. */
export function payoutStatusOf(o: ChainOffer, slot: { joined: boolean; settled: boolean }, nowSecs: number): PayoutStatusWord | null {
  if (slot.settled) return "claimed";
  if (o.state === "disputed") return "disputed";
  if (o.state !== "payoutPosted" && o.state !== "closed") return null;
  if (ZERO_ROOT.test(o.payoutRoot)) return "no-payout-list";
  if (nowSecs < disputeEnd(o)) return "waiting";
  if (o.openChallenges !== 0) return "challenge-open";
  if (o.reserveSwept) return "reserve-swept";
  if (!slot.joined) return "not-joined";
  return "claimable";
}

export interface MyWork {
  offers: ChainOffer[];
  rounds: Record<number, ChainRound | null>;
  settled: Record<number, boolean>;
  pullWei: bigint;
}

/** The contract addresses to read, from the shipped address book (with code on chain). */
export interface MyWorkAddresses {
  workMarket: string;
  /** FederatedRoundLedger, or null when it is not on this network. */
  frl: string | null;
}

/** The offers this wallet joined (newest first), their FRL rounds, settlement and pull balance. */
export async function readMyWork(a: MyWorkAddresses, wallet: string, call: ViewCall): Promise<MyWork> {
  if (!/^0x[0-9a-fA-F]{40}$/.test(wallet)) throw new Error("no wallet on this device yet");
  const offers = await myOffers(call, a.workMarket, wallet);
  const rounds: Record<number, ChainRound | null> = {};
  const settled: Record<number, boolean> = {};
  for (const o of offers) {
    rounds[o.id] = o.kind === "flRound" && a.frl && !/^0x0{64}$/.test(o.bindRef) ? await getFrlRound(call, a.frl, o.bindRef) : null;
    settled[o.id] = await settledFor(call, a.workMarket, o.id, wallet);
  }
  return { offers, rounds, settled, pullWei: await pullBalance(call, a.workMarket, wallet) };
}

/** The round to show: the newest one still in progress, else the newest. */
export function currentOffer(offers: ChainOffer[]): ChainOffer | null {
  return offers.find((o) => o.state === "open" || o.state === "running" || o.state === "payoutPosted" || o.state === "disputed") ?? offers[0] ?? null;
}
