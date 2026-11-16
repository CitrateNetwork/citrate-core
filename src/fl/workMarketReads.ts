// =====================================================================
// citrate-core: on-chain reads of WorkMarket and FederatedRoundLedger for the FL boards
//
// Data sources (Rule 7): `eth_call` through core's contract_view_call on the active network, at
// the addresses the shipped book pins (networkState.ts). Nothing here is read until the book pins
// WorkMarket and its code is on chain, so on r1007 these functions are never called; once the
// book pins it they read real state with no other change:
//  - WorkMarket.nextOfferId(), getOffer(id), joined(id, you), settled(id, you), balanceOf(you)
//  - FederatedRoundLedger.getRound(bindRef) for an FL offer's round (bindRef is the FRL round id)
// Shapes from citrate-chain contracts/src/work/WorkMarket.sol (Offer, 38 static words) and
// contracts/src/FederatedRoundLedger.sol (Round).
// =====================================================================
import { decodeFunctionResult, encodeFunctionData, type Hex } from "viem";

const OFFER_COMPONENTS = [
  { name: "kind", type: "uint8" },
  { name: "state", type: "uint8" },
  { name: "poster", type: "address" },
  { name: "verifier", type: "address" },
  { name: "bindRef", type: "bytes32" },
  { name: "jobSpecHash", type: "bytes32" },
  { name: "requirementsHash", type: "bytes32" },
  { name: "groupHash", type: "bytes32" },
  { name: "allowlistRoot", type: "bytes32" },
  { name: "pricePerUnit", type: "uint128" },
  { name: "budget", type: "uint128" },
  { name: "paid", type: "uint128" },
  { name: "refunded", type: "uint128" },
  { name: "maxUnits", type: "uint64" },
  { name: "deadline", type: "uint64" },
  { name: "disputeWindow", type: "uint32" },
  { name: "maxWorkers", type: "uint32" },
  { name: "workers", type: "uint32" },
  { name: "joinBondsReleased", type: "uint32" },
  { name: "joinBond", type: "uint128" },
  { name: "coordinator", type: "address" },
  { name: "coordinatorBond", type: "uint128" },
  { name: "disputeAdmin", type: "address" },
  { name: "payoutRoot", type: "bytes32" },
  { name: "leavesRoot", type: "bytes32" },
  { name: "digest", type: "bytes32" },
  { name: "payoutPostedAt", type: "uint64" },
  { name: "payoutUnits", type: "uint64" },
  { name: "settledUnits", type: "uint64" },
  { name: "coordinatorBondHeld", type: "bool" },
  { name: "subjectiveDisputeUsed", type: "bool" },
  { name: "reserveSwept", type: "bool" },
  { name: "payoutEpoch", type: "uint32" },
  { name: "openChallenges", type: "uint32" },
  { name: "payoutDepth", type: "uint8" },
  { name: "answerGraceUntil", type: "uint64" },
  { name: "resultEpoch", type: "uint32" },
  { name: "challengeBondsHeld", type: "uint128" },
] as const;

export const WORK_MARKET_ABI = [
  { type: "function", name: "nextOfferId", stateMutability: "view", inputs: [], outputs: [{ name: "", type: "uint256" }] },
  { type: "function", name: "getOffer", stateMutability: "view", inputs: [{ name: "id", type: "uint256" }], outputs: [{ name: "", type: "tuple", components: OFFER_COMPONENTS }] },
  { type: "function", name: "joined", stateMutability: "view", inputs: [{ name: "id", type: "uint256" }, { name: "worker", type: "address" }], outputs: [{ name: "", type: "bool" }] },
  { type: "function", name: "settled", stateMutability: "view", inputs: [{ name: "id", type: "uint256" }, { name: "worker", type: "address" }], outputs: [{ name: "", type: "bool" }] },
  { type: "function", name: "balanceOf", stateMutability: "view", inputs: [{ name: "a", type: "address" }], outputs: [{ name: "", type: "uint256" }] },
  { type: "function", name: "withdraw", stateMutability: "nonpayable", inputs: [], outputs: [] },
] as const;

export const FRL_ABI = [
  {
    type: "function",
    name: "getRound",
    stateMutability: "view",
    inputs: [{ name: "roundId", type: "bytes32" }],
    outputs: [
      {
        name: "",
        type: "tuple",
        components: [
          { name: "clusterId", type: "bytes32" },
          { name: "ordinal", type: "uint64" },
          { name: "configHash", type: "bytes32" },
          { name: "participantsRoot", type: "bytes32" },
          { name: "participants", type: "uint16" },
          { name: "nValues", type: "uint64" },
          { name: "inputRoot", type: "bytes32" },
          { name: "outputRoot", type: "bytes32" },
          { name: "chunks", type: "uint32" },
          { name: "adapterHash", type: "bytes32" },
          { name: "deadline", type: "uint64" },
          { name: "status", type: "uint8" },
          { name: "fault", type: "uint8" },
          { name: "faultChunk", type: "uint32" },
        ],
      },
    ],
  },
] as const;

export const OFFER_STATES = ["open", "running", "payoutPosted", "disputed", "closed", "expired"] as const;
export type OnChainOfferState = (typeof OFFER_STATES)[number];
export const FRL_STATUS = ["none", "committed", "rejected", "accepted"] as const;
export const FRL_FAULT = ["none", "output mismatch", "row mismatch", "rule violation", "participant order"] as const;

export interface ChainOffer {
  id: number;
  kind: "flRound" | "task";
  state: OnChainOfferState;
  poster: string;
  bindRef: string;
  groupHash: string;
  pricePerUnit: bigint;
  budget: bigint;
  paid: bigint;
  refunded: bigint;
  deadline: number;
  disputeWindow: number;
  maxWorkers: number;
  workers: number;
  joinBond: bigint;
  coordinator: string;
  payoutPostedAt: number;
  payoutUnits: bigint;
}

export interface ChainRound {
  status: (typeof FRL_STATUS)[number];
  fault: (typeof FRL_FAULT)[number];
  deadlineBlock: number;
  participants: number;
  adapterHash: string;
  ordinal: number;
}

/** The view call core makes (contract_view_call on the active network). */
export type ViewCall = (address: string, calldata: string) => Promise<string>;

export async function nextOfferId(call: ViewCall, market: string): Promise<number> {
  const out = await call(market, encodeFunctionData({ abi: WORK_MARKET_ABI, functionName: "nextOfferId" }));
  return Number(decodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "nextOfferId", data: out as Hex }));
}

export async function getOffer(call: ViewCall, market: string, id: number): Promise<ChainOffer> {
  const out = await call(market, encodeFunctionData({ abi: WORK_MARKET_ABI, functionName: "getOffer", args: [BigInt(id)] }));
  const o = decodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "getOffer", data: out as Hex });
  const state = OFFER_STATES[o.state];
  if (!state) throw new Error(`offer ${id}: unknown state ${o.state}`);
  return {
    id,
    kind: o.kind === 0 ? "flRound" : "task",
    state,
    poster: o.poster.toLowerCase(),
    bindRef: o.bindRef,
    groupHash: o.groupHash,
    pricePerUnit: o.pricePerUnit,
    budget: o.budget,
    paid: o.paid,
    refunded: o.refunded,
    deadline: Number(o.deadline),
    disputeWindow: o.disputeWindow,
    maxWorkers: o.maxWorkers,
    workers: o.workers,
    joinBond: o.joinBond,
    coordinator: o.coordinator.toLowerCase(),
    payoutPostedAt: Number(o.payoutPostedAt),
    payoutUnits: o.payoutUnits,
  };
}

async function boolCall(call: ViewCall, market: string, fn: "joined" | "settled", id: number, who: string): Promise<boolean> {
  const out = await call(market, encodeFunctionData({ abi: WORK_MARKET_ABI, functionName: fn, args: [BigInt(id), who as Hex] }));
  return decodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: fn, data: out as Hex });
}

export const joinedOffer = (call: ViewCall, market: string, id: number, who: string) => boolCall(call, market, "joined", id, who);
export const settledFor = (call: ViewCall, market: string, id: number, who: string) => boolCall(call, market, "settled", id, who);

/** The member's WorkMarket pull balance (wei). */
export async function pullBalance(call: ViewCall, market: string, who: string): Promise<bigint> {
  const out = await call(market, encodeFunctionData({ abi: WORK_MARKET_ABI, functionName: "balanceOf", args: [who as Hex] }));
  return decodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "balanceOf", data: out as Hex });
}

export const WITHDRAW_SIGNATURE = "withdraw()";
export const withdrawCalldata = () => encodeFunctionData({ abi: WORK_MARKET_ABI, functionName: "withdraw" });

export async function getFrlRound(call: ViewCall, frl: string, roundId: string): Promise<ChainRound> {
  const out = await call(frl, encodeFunctionData({ abi: FRL_ABI, functionName: "getRound", args: [roundId as Hex] }));
  const r = decodeFunctionResult({ abi: FRL_ABI, functionName: "getRound", data: out as Hex });
  return {
    status: FRL_STATUS[r.status] ?? "none",
    fault: FRL_FAULT[r.fault] ?? "none",
    deadlineBlock: Number(r.deadline),
    participants: r.participants,
    adapterHash: r.adapterHash,
    ordinal: Number(r.ordinal),
  };
}

/** How many of the newest offers the app scans for the member's own (bounded reads). */
export const SCAN_OFFERS = 200;

/** The offers this member joined, newest first, from the newest SCAN_OFFERS. */
export async function myOffers(call: ViewCall, market: string, who: string, scan = SCAN_OFFERS): Promise<ChainOffer[]> {
  const last = await nextOfferId(call, market);
  const out: ChainOffer[] = [];
  for (let id = last; id >= 1 && id > last - scan; id--) {
    if (await joinedOffer(call, market, id, who)) out.push(await getOffer(call, market, id));
  }
  return out;
}
