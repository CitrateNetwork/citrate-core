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
export type RoundStateWord = "pending" | "running" | "verifying" | "accepted" | "rejected" | "disputed" | "expired";

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
    case "closed":
      return "accepted";
    case "disputed":
      return "disputed";
    case "expired":
      return "expired";
  }
}

/** Budget split into paid, reserved for posted units, and refundable (wei). */
export function escrowOf(o: ChainOffer): { budgetWei: bigint; paidWei: bigint; reservedWei: bigint; refundableWei: bigint } {
  const owedToWorkers = o.payoutUnits * o.pricePerUnit;
  const reserved = owedToWorkers > o.paid ? owedToWorkers - o.paid : 0n;
  const left = o.budget - o.paid - o.refunded - reserved;
  return { budgetWei: o.budget, paidWei: o.paid, reservedWei: reserved, refundableWei: left > 0n ? left : 0n };
}

export type PayoutStatusWord = "waiting" | "claimable" | "claimed" | "disputed";

/** Where this member's payout on an offer stands, or null before a payout list is posted. */
export function payoutStatusOf(o: ChainOffer, settled: boolean, nowSecs: number): PayoutStatusWord | null {
  if (o.state !== "payoutPosted" && o.state !== "disputed" && o.state !== "closed") return null;
  if (settled) return "claimed";
  if (o.state === "disputed") return "disputed";
  return nowSecs < o.payoutPostedAt + o.disputeWindow ? "waiting" : "claimable";
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
