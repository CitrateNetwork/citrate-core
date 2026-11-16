// =====================================================================
// citrate-core: the member's own rounds and payouts as the FL boards draw them
//
// Adapters from the shared on-chain reads (myWork.ts, workMarketReads.ts) to the board views:
// Live round (LiveRound) and Payouts (Receipt), and the hook that reads them once the network
// state shows WorkMarket deployed. On r1007 nothing here reads.
// =====================================================================
import { useEffect, useState } from "react";
import type { LiveRound } from "./LiveRound";
import { disputeEnd, escrowOf, liveStateOf, NOT_ON_CHAIN, payoutStatusOf, readMyWork, type MyWork } from "./myWork";
import type { FlNetworkState } from "./networkState";
import type { Receipt } from "./Payouts";
import { shortHex } from "./primitives";
import type { ChainOffer, ChainRound, ViewCall } from "./workMarketReads";

export function liveRoundFrom(o: ChainOffer, r: ChainRound | null, head: number | null, payouts: FlNetworkState["payouts"]): LiveRound {
  const clock =
    r?.status === "committed"
      ? { state: "open" as const, endBlock: r.deadlineBlock, head, blockSeconds: null }
      : r?.status === "accepted"
        ? { state: "closed" as const, endBlock: r.deadlineBlock, head, blockSeconds: null }
        : r?.status === "rejected"
          ? { state: "ended-early" as const, head, blockSeconds: null }
          : { state: "not-open" as const, head, blockSeconds: null };
  return {
    offerId: o.id,
    poster: shortHex(o.poster),
    state: liveStateOf(o, r),
    coordinatorAnswering: true,
    leases: null,
    maxWorkers: o.maxWorkers,
    clock,
    belnap: null,
    belnapReason: NOT_ON_CHAIN.belnap,
    escrow: escrowOf(o),
    contribution: [
      { key: "dataset", state: "pending" },
      { key: "training", state: "pending", detail: NOT_ON_CHAIN.training },
      { key: "update", state: "pending" },
      { key: "upload", state: "pending" },
      { key: "receipt", state: "pending" },
    ],
    fault: r?.status === "rejected" ? r.fault : undefined,
    rateWei: o.pricePerUnit,
    deadline: o.deadline,
    payouts,
    payout: null,
    ids: { frlRound: o.kind === "flRound" ? o.bindRef : null, mergedHash: r && !/^0x0{64}$/.test(r.adapterHash) ? r.adapterHash : null, coordinator: o.coordinator },
  };
}

/** A payout receipt for an offer whose payout list is posted, its status mirroring every
 *  WorkMarket.settle precondition (myWork.ts). Your units are not on chain. */
export function receiptFrom(o: ChainOffer, slot: { joined: boolean; settled: boolean }, nowSecs: number): Receipt | null {
  const status = payoutStatusOf(o, slot, nowSecs);
  if (!status) return null;
  return { line: { round: String(o.id), units: null, rateWei: o.pricePerUnit, amountWei: null }, status, disputeEndsAt: disputeEnd(o) };
}

export type MyWorkRead = { kind: "idle" } | { kind: "reading" } | { kind: "read"; work: MyWork } | { kind: "failed"; reason: string };

/** Read the member's work once WorkMarket is deployed (addresses from the shipped book). */
export function useMyWork(state: FlNetworkState, wallet: string, call: ViewCall): MyWorkRead {
  const [w, setW] = useState<MyWorkRead>({ kind: "idle" });
  const wm = state.contracts.WorkMarket;
  const frl = state.contracts.FederatedRoundLedger;
  const market = wm.kind === "deployed" ? wm.address : null;
  const ledger = frl.kind === "deployed" ? frl.address : null;
  useEffect(() => {
    if (!market) return;
    let on = true;
    setW({ kind: "reading" });
    readMyWork({ workMarket: market, frl: ledger }, wallet, call).then(
      (work) => on && setW({ kind: "read", work }),
      (e) => on && setW({ kind: "failed", reason: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      on = false;
    };
  }, [market, ledger, wallet]);
  return w;
}
