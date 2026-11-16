// The shared on-chain reads (workMarketReads.ts, myWork.ts), against a fake eth_call that answers
// with ABI-encoded WorkMarket and FederatedRoundLedger returns. The offers here are TEST-ONLY
// fixtures; on r1007 these reads never run (no WorkMarket in the shipped book).
import { describe, expect, it } from "vitest";
import { decodeFunctionData, encodeFunctionResult, toFunctionSelector, type Hex } from "viem";
import { currentOffer, disputeEnd, escrowOf, liveStateOf, payoutStatusOf, readMyWork } from "./myWork";
import { FRL_ABI, getOffer, WITHDRAW_SIGNATURE, withdrawCalldata, WORK_MARKET_ABI, type ChainOffer, type ViewCall } from "./workMarketReads";

const SALT = 10n ** 18n;
const WM = "0x" + "ab".repeat(20);
const FRL = "0x25fc02fbfad9589d0dde84f858d0962bfa0253c5";
const ME = "0x" + "5b".repeat(20);
const Z32 = ("0x" + "0".repeat(64)) as Hex;
const ROUND = ("0x" + "3e".repeat(32)) as Hex;

const offerTuple = (state: number, over: Record<string, unknown> = {}) => ({
  kind: 0, state, poster: ("0x" + "11".repeat(20)) as Hex, verifier: ("0x" + "22".repeat(20)) as Hex, bindRef: ROUND, jobSpecHash: Z32, requirementsHash: Z32, groupHash: Z32, allowlistRoot: Z32,
  pricePerUnit: (3n * SALT) / 2n, budget: 600n * SALT, paid: 0n, refunded: 0n, maxUnits: 400n, deadline: 1_763_769_600n, disputeWindow: 86_400, maxWorkers: 8, workers: 3, joinBondsReleased: 0,
  joinBond: SALT / 10n, coordinator: ("0x" + "cd".repeat(20)) as Hex, coordinatorBond: 100n * SALT, disputeAdmin: ("0x" + "0".repeat(40)) as Hex, payoutRoot: Z32, leavesRoot: Z32, digest: Z32,
  payoutPostedAt: 0n, payoutUnits: 0n, settledUnits: 0n, coordinatorBondHeld: false, subjectiveDisputeUsed: false, reserveSwept: false, payoutEpoch: 0, openChallenges: 0, payoutDepth: 0,
  answerGraceUntil: 0n, resultEpoch: 0, challengeBondsHeld: 0n, ...over,
});

function chain(offers: Record<number, ReturnType<typeof offerTuple>>, joined: number[], settled: number[] = [], pull = 0n): ViewCall {
  return async (to, data) => {
    if (to === WM) {
      const d = decodeFunctionData({ abi: WORK_MARKET_ABI, data: data as Hex });
      switch (d.functionName) {
        case "nextOfferId":
          return encodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "nextOfferId", result: BigInt(Math.max(...Object.keys(offers).map(Number))) });
        case "getOffer":
          return encodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "getOffer", result: offers[Number(d.args[0])] });
        case "joined":
          return encodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "joined", result: joined.includes(Number(d.args[0])) });
        case "settled":
          return encodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "settled", result: settled.includes(Number(d.args[0])) });
        case "balanceOf":
          return encodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "balanceOf", result: pull });
      }
    }
    if (to === FRL) {
      return encodeFunctionResult({ abi: FRL_ABI, functionName: "getRound", result: { clusterId: Z32, ordinal: 4n, configHash: Z32, participantsRoot: Z32, participants: 5, nValues: 10n, inputRoot: Z32, outputRoot: Z32, chunks: 1, adapterHash: ("0x" + "a7".repeat(32)) as Hex, deadline: 1_209_911n, status: 1, fault: 0, faultChunk: 0 } });
    }
    throw new Error("unexpected call to " + to);
  };
}

describe("WorkMarket and FRL reads", () => {
  it("decodes getOffer's 38 words", async () => {
    const o = await getOffer(chain({ 14: offerTuple(1) }, []), WM, 14);
    expect(o).toMatchObject({ id: 14, kind: "flRound", state: "running", pricePerUnit: (3n * SALT) / 2n, budget: 600n * SALT, maxWorkers: 8, workers: 3, bindRef: ROUND });
  });
  it("finds the offers this wallet joined, newest first, with their FRL rounds, settlement and pull balance", async () => {
    const call = chain({ 12: offerTuple(4), 13: offerTuple(1), 14: offerTuple(2, { payoutPostedAt: 1_000n, payoutUnits: 7n }) }, [12, 14], [12], 3n * SALT);
    const w = await readMyWork({ workMarket: WM, frl: FRL }, ME, call);
    expect(w.offers.map((o) => o.id)).toEqual([14, 12]);
    expect(w.settled).toEqual({ 14: false, 12: true });
    expect(w.rounds[14]).toMatchObject({ status: "committed", deadlineBlock: 1_209_911, participants: 5 });
    expect(w.pullWei).toBe(3n * SALT);
  });
  it("without FRL the rounds are null; without a wallet nothing is read", async () => {
    const w = await readMyWork({ workMarket: WM, frl: null }, ME, chain({ 14: offerTuple(1) }, [14]));
    expect(w.rounds[14]).toBeNull();
    await expect(readMyWork({ workMarket: WM, frl: FRL }, "", chain({ 1: offerTuple(0) }, []))).rejects.toThrow("no wallet");
  });
  it("withdraw is WorkMarket.withdraw()", () => {
    expect(withdrawCalldata()).toBe(toFunctionSelector(WITHDRAW_SIGNATURE));
  });
});

describe("state words", () => {
  const o = (state: ChainOffer["state"], over: Partial<ChainOffer> = {}): ChainOffer => ({ id: 14, kind: "flRound", state, poster: "0x" + "11".repeat(20), bindRef: ROUND, groupHash: Z32, pricePerUnit: (3n * SALT) / 2n, budget: 600n * SALT, paid: 0n, refunded: 0n, deadline: 1, disputeWindow: 100, maxWorkers: 8, workers: 3, joinBond: 0n, coordinator: "0x" + "cd".repeat(20), payoutPostedAt: 0, payoutUnits: 0n, payoutRoot: Z32, answerGraceUntil: 0, openChallenges: 0, reserveSwept: false, ...over });
  const r = (status: "committed" | "accepted" | "rejected") => ({ status, fault: status === "rejected" ? ("output mismatch" as const) : ("none" as const), deadlineBlock: 1, participants: 5, adapterHash: Z32, ordinal: 4 });
  it("maps offer and FRL state to the design's round states", () => {
    expect(liveStateOf(o("open"), null)).toBe("pending");
    expect(liveStateOf(o("running"), null)).toBe("running");
    expect(liveStateOf(o("running"), r("committed"))).toBe("verifying");
    expect(liveStateOf(o("running"), r("rejected"))).toBe("rejected");
    expect(liveStateOf(o("payoutPosted"), r("accepted"))).toBe("accepted");
    expect(liveStateOf(o("disputed"), null)).toBe("disputed");
    expect(liveStateOf(o("expired"), null)).toBe("expired");
  });
  it("escrow: reserved for posted units, the rest refundable", () => {
    expect(escrowOf(o("payoutPosted", { payoutUnits: 7n }))).toEqual({ budgetWei: 600n * SALT, paidWei: 0n, reservedWei: (21n * SALT) / 2n, refundableWei: 600n * SALT - (21n * SALT) / 2n });
  });
  it("payouts: claimable only when every settle precondition holds", () => {
    const live = "0x" + "77".repeat(32);
    const posted = o("payoutPosted", { payoutPostedAt: 1_000, disputeWindow: 100, payoutRoot: live });
    const me = { joined: true, settled: false };
    expect(payoutStatusOf(posted, me, 1_200)).toBe("claimable");
    expect(payoutStatusOf({ ...posted, state: "closed" }, me, 1_200)).toBe("claimable");
  });
  // Red tests, one per WorkMarket.settle precondition: each makes "claimable" false.
  const live = "0x" + "77".repeat(32);
  const posted = o("payoutPosted", { payoutPostedAt: 1_000, disputeWindow: 100, payoutRoot: live });
  const me = { joined: true, settled: false };
  it("settle: state must be PayoutPosted or Closed (Running has no payout; Disputed is frozen)", () => {
    expect(payoutStatusOf({ ...posted, state: "running" }, me, 1_200)).toBeNull();
    expect(payoutStatusOf({ ...posted, state: "disputed" }, me, 1_200)).toBe("disputed");
  });
  it("settle: a live payout root (a withdrawn list is not claimable)", () => {
    expect(payoutStatusOf({ ...posted, payoutRoot: Z32 }, me, 1_200)).toBe("no-payout-list");
  });
  it("settle: the dispute window has ended", () => {
    expect(payoutStatusOf(posted, me, 1_099)).toBe("waiting");
    expect(payoutStatusOf(posted, me, 1_100)).toBe("claimable");
  });
  it("settle: the grace after an answered challenge has ended (answerGraceUntil)", () => {
    expect(payoutStatusOf({ ...posted, answerGraceUntil: 5_000 }, me, 1_200)).toBe("waiting");
    expect(payoutStatusOf({ ...posted, answerGraceUntil: 5_000 }, me, 5_000)).toBe("claimable");
  });
  it("settle: no open challenges", () => {
    expect(payoutStatusOf({ ...posted, openChallenges: 1 }, me, 1_200)).toBe("challenge-open");
  });
  it("settle: the reserve is not swept", () => {
    expect(payoutStatusOf({ ...posted, reserveSwept: true }, me, 1_200)).toBe("reserve-swept");
  });
  it("settle: joined", () => {
    expect(payoutStatusOf(posted, { joined: false, settled: false }, 1_200)).toBe("not-joined");
  });
  it("settle: not already settled", () => {
    expect(payoutStatusOf(posted, { joined: true, settled: true }, 1_200)).toBe("claimed");
  });
  it("getOffer carries every settle precondition", async () => {
    const x = await getOffer(chain({ 9: offerTuple(2, { payoutRoot: live as Hex, answerGraceUntil: 77n, openChallenges: 2, reserveSwept: true }) }, []), WM, 9);
    expect(x).toMatchObject({ payoutRoot: live, answerGraceUntil: 77, openChallenges: 2, reserveSwept: true });
    expect(disputeEnd({ ...posted, answerGraceUntil: 5_000 })).toBe(5_000);
  });
  it("the current round is the newest still in progress", () => {
    expect(currentOffer([o("closed", { id: 20 }), o("running", { id: 18 })])!.id).toBe(18);
    expect(currentOffer([o("closed", { id: 20 })])!.id).toBe(20);
    expect(currentOffer([])).toBeNull();
  });
});
