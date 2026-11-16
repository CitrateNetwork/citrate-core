// The board views of the member's own rounds and payouts (myWorkViews.ts), from on-chain reads.
// TEST-ONLY offers; on r1007 nothing reads (no WorkMarket in the shipped book).
import { describe, expect, it } from "vitest";
import { liveRoundFrom, receiptFrom, WITHDRAW_WAITS_FOR_GSEC, withdrawIntent } from "./myWorkViews";
import { readFlNetworkState } from "./networkState";
import { WITHDRAW_SIGNATURE, withdrawCalldata } from "./workMarketReads";
import realBook from "../../src-tauri/addresses/40204.json";
import { NOT_ON_CHAIN } from "./myWork";
import type { ChainOffer, ChainRound } from "./workMarketReads";

const SALT = 10n ** 18n;
const Z32 = "0x" + "0".repeat(64);
const ROUND = "0x" + "3e".repeat(32);
const o = (state: ChainOffer["state"], over: Partial<ChainOffer> = {}): ChainOffer => ({ id: 14, kind: "flRound", state, poster: "0x" + "11".repeat(20), bindRef: ROUND, groupHash: Z32, pricePerUnit: (3n * SALT) / 2n, budget: 600n * SALT, paid: 0n, refunded: 0n, deadline: 1, disputeWindow: 100, maxWorkers: 8, workers: 3, joinBond: 0n, coordinator: "0x" + "cd".repeat(20), payoutPostedAt: 0, payoutUnits: 0n, payoutRoot: Z32, answerGraceUntil: 0, openChallenges: 0, reserveSwept: false, ...over });
const r = (status: ChainRound["status"]): ChainRound => ({ status, fault: status === "rejected" ? "output mismatch" : "none", faultChunk: 0, deadlineBlock: 1_209_911, participants: 5, adapterHash: "0x" + "a7".repeat(32), ordinal: 4 });

describe("Live round from the chain", () => {
  it("what the chain does not hold is said, never filled", () => {
    const lr = liveRoundFrom(o("running"), r("committed"), 1_206_611, "dark");
    expect(lr.state).toBe("verifying");
    expect(lr.leases).toBeNull();
    expect(lr.belnap).toBeNull();
    expect(lr.belnapReason).toBe(NOT_ON_CHAIN.belnap);
    expect(lr.clock).toEqual({ state: "open", endBlock: 1_209_911, head: 1_206_611, blockSeconds: null });
    expect(lr.contribution.find((c) => c.key === "training")!.detail).toBe(NOT_ON_CHAIN.training);
    expect(lr.ids.mergedHash).toBe("0x" + "a7".repeat(32));
    expect(lr.payout).toBeNull();
  });
  it("a refunded Closed offer is drawn as Closed, never Accepted", () => {
    const lr = liveRoundFrom(o("closed", { payoutRoot: Z32, refunded: 600n * SALT }), null, null, "dark");
    expect(lr.state).toBe("closed");
  });
  it("accepted closes the clock; rejected ends it early with the fault", () => {
    expect(liveRoundFrom(o("payoutPosted"), r("accepted"), null, "dark").clock.state).toBe("closed");
    const rej = liveRoundFrom(o("running"), r("rejected"), null, "dark");
    expect(rej.clock.state).toBe("ended-early");
    expect(rej.fault).toBe("output mismatch");
  });
});

describe("Payout receipts from the chain", () => {
  const live = "0x" + "77".repeat(32);
  const posted = o("payoutPosted", { payoutPostedAt: 1_000, disputeWindow: 100, payoutRoot: live });
  const me = { joined: true, settled: false };
  it("claimable only when settle would not revert; the window end includes the answer grace", () => {
    expect(receiptFrom(posted, me, 1_200)!.status).toBe("claimable");
    expect(receiptFrom({ ...posted, answerGraceUntil: 9_000 }, me, 1_200)).toMatchObject({ status: "waiting", disputeEndsAt: 9_000 });
    expect(receiptFrom({ ...posted, openChallenges: 1 }, me, 1_200)!.status).toBe("challenge-open");
    expect(receiptFrom({ ...posted, reserveSwept: true }, me, 1_200)!.status).toBe("reserve-swept");
    expect(receiptFrom({ ...posted, payoutRoot: Z32 }, me, 1_200)!.status).toBe("no-payout-list");
    expect(receiptFrom(posted, { joined: true, settled: true }, 1_200)!.status).toBe("claimed");
    expect(receiptFrom(o("running"), me, 1_200)).toBeNull();
  });
  it("before the chain's time is read (now 0), nothing is claimable", () => {
    expect(receiptFrom(posted, me, 0)!.status).toBe("waiting");
  });
  it("your units are not on chain", () => {
    expect(receiptFrom(posted, me, 1_200)!.line).toEqual({ round: "14", units: null, rateWei: (3n * SALT) / 2n, amountWei: null });
  });
});

describe("withdrawIntent fails closed on its own (#724 review P2)", () => {
  const WM = "0x" + "ab".repeat(20);
  const base = { codeSize: async () => 1000, viewCall: async () => Z32 };
  const book = { addresses: { ...realBook.addresses, WorkMarket: WM } };
  it("refuses while core's payouts gate is shut, with its reason, even with a balance", async () => {
    const st = await readFlNetworkState(book, { ...base, payoutsStatus: async () => ({ open: false, reason: "payouts are not open on r1007" }) });
    expect(() => withdrawIntent(st, 5n * SALT)).toThrow(`${WITHDRAW_WAITS_FOR_GSEC} payouts are not open on r1007`);
  });
  it("refuses when the gate was not read, and on r1007 (no WorkMarket)", async () => {
    const unread = await readFlNetworkState(book, { ...base, payoutsStatus: async () => Promise.reject(new Error("no such command")) });
    expect(() => withdrawIntent(unread, 5n * SALT)).toThrow(/payouts gate was not read/);
    const r1007 = await readFlNetworkState(realBook, base);
    expect(() => withdrawIntent(r1007, 5n * SALT)).toThrow(WITHDRAW_WAITS_FOR_GSEC);
  });
  it("with the gate open: refuses an unread or empty balance; else withdraw() to WorkMarket, value 0", async () => {
    const st = await readFlNetworkState(book, { ...base, payoutsStatus: async () => ({ open: true, reason: null }) });
    expect(() => withdrawIntent(st, null)).toThrow(/not read/);
    expect(() => withdrawIntent(st, 0n)).toThrow(/nothing/);
    expect(withdrawIntent(st, 5n * SALT)).toEqual({ address: WM, calldata: withdrawCalldata(), valueWei: "0", label: WITHDRAW_SIGNATURE });
  });
});
