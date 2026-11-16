// 0.5.4 Hermes x FL WP-H10: the payout tools, shipped dark behind core's Rust payouts gate (#736).
// The r1007 and dark states go through the store's real handleTool; the open state (test-only:
// no shipped profile opens payouts) drives the module with core's shapes. Offers here are
// TEST-ONLY fixtures. Written red-first.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import realBook from "../../src-tauri/addresses/40204.json";
import { store } from "../shell/store";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { CLAIM_NEEDS_LEAF, DISPUTE_NEEDS_RECEIPT, runFlPayoutTool, type FlPayoutDeps, type FlPayoutToolName } from "./flPayoutTools";
import { readFlNetworkState, type FlReads } from "../fl/networkState";
import type { MyWork } from "../fl/myWork";
import { WITHDRAW_SIGNATURE, withdrawCalldata, type ChainOffer } from "../fl/workMarketReads";
import type { PayoutsStatus } from "../fl/payoutsStatus";
import { payoutsOpenNetwork } from "../fl/fixtures/market";

const call = (name: FlPayoutToolName, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};
const SALT = 10n ** 18n;
const WM = "0x" + "ab".repeat(20);
const ZERO32 = "0x" + "0".repeat(64);
const ROOT = "0x" + "77".repeat(32);
const ME = "0x" + "5b".repeat(20);
const reads: FlReads = { codeSize: async () => 1000, viewCall: async () => ZERO32 };
const today = () => readFlNetworkState(realBook, reads);
const withWm = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads);
const DARK_REASON = 'FL payouts are not open on network "r1007" (payoutsOpen is false)';
const status = (open: boolean, reason: string | null = open ? null : DARK_REASON): PayoutsStatus => ({ network: "r1007", payoutsOpenFlag: open, open, reason, contracts: [] });

const chainOffer = (id: number, over: Partial<ChainOffer> = {}): ChainOffer => ({
  id, kind: "flRound", state: "payoutPosted", poster: "0x" + "11".repeat(20), bindRef: ZERO32, groupHash: ZERO32,
  pricePerUnit: (3n * SALT) / 2n, budget: 600n * SALT, paid: 0n, refunded: 0n, deadline: 1_700_000_000, disputeWindow: 86_400,
  maxWorkers: 8, workers: 3, joinBond: SALT / 10n, coordinator: "0x" + "cd".repeat(20), payoutPostedAt: 1_700_000_000, payoutUnits: 0n,
  payoutRoot: ROOT, answerGraceUntil: 0, openChallenges: 0, reserveSwept: false, ...over,
});
const work = (offers: ChainOffer[], settled: number[] = [], pullWei = 0n): MyWork => ({
  offers, rounds: {}, settled: Object.fromEntries(offers.map((o) => [o.id, settled.includes(o.id)])), pullWei,
});
const deps = (w: MyWork, over: Partial<FlPayoutDeps> = {}): FlPayoutDeps => ({
  networkState: payoutsOpenNetwork,
  payoutsStatus: async () => status(true),
  wallet: () => ME,
  call: async () => { throw new Error("not called: readMyWork is stood in"); },
  chainNow: async () => 1_700_000_000 + 86_400 + 10,
  propose: async () => ({ proposed: true }),
  readMyWork: async () => w,
  pullBalance: async () => w.pullWei,
  ...over,
});
const ALL: FlPayoutToolName[] = ["fl_payouts_list", "fl_withdraw_propose", "fl_claim_propose", "fl_dispute_propose"];

beforeEach(() => store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null }));
afterEach(() => vi.restoreAllMocks());

describe("schemas and annotations", () => {
  it("list, claim and dispute are reviewed reads (no card in this release); withdraw spends", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name);
    for (const n of ALL) expect(names).toContain(n);
    for (const n of ["fl_payouts_list", "fl_claim_propose", "fl_dispute_propose"]) {
      expect(READ_ONLY_AGENT_TOOLS.has(n)).toBe(true);
      expect(annotationFor(n)!.effect).toBe("none");
    }
    expect(READ_ONLY_AGENT_TOOLS.has("fl_withdraw_propose")).toBe(false);
    expect(annotationFor("fl_withdraw_propose")).toEqual({ effect: "spend", trust: "trusted" });
  });
});

describe("the gate, through handleTool", () => {
  it("on this network every tool says WorkMarket is not available and never asks core's gate", async () => {
    vi.spyOn(store, "flNetworkState").mockImplementation(today);
    const st = vi.spyOn(store, "flPayoutsStatus");
    const propose = vi.spyOn(store, "proposeContractCall");
    for (const n of ALL) expect(await store.handleTool(call(n, { offerId: 1 }), "m1", noop)).toMatch(/WorkMarket is not available yet on this network/);
    expect(st).not.toHaveBeenCalled();
    expect(propose).not.toHaveBeenCalled();
  });

  it("with WorkMarket, payouts dark comes from core's gate (not the profile flag) with its reason", async () => {
    vi.spyOn(store, "flNetworkState").mockImplementation(withWm);
    vi.spyOn(store, "flPayoutsStatus").mockResolvedValue(status(false, 'WorkMarket code hash 0xaa does not match the pinned 0xbb'));
    const propose = vi.spyOn(store, "proposeContractCall");
    for (const n of ALL) {
      const out = await store.handleTool(call(n, { offerId: 1 }), "m1", noop);
      expect(out).toContain("Payouts: Payouts open after the security review. WorkMarket code hash 0xaa does not match the pinned 0xbb [core fl_payouts_status, the Rust payouts gate]");
    }
    expect(propose).not.toHaveBeenCalled();
  });

  it("a status core cannot give is unknown with its reason, never open", async () => {
    const out = await runFlPayoutTool("fl_withdraw_propose", {}, deps(work([], [], SALT), { payoutsStatus: async () => { throw new Error("whether payouts are open is decided by the desktop app's core"); } }));
    expect(out.result).toMatch(/^Payouts: — \(core could not say whether payouts are open/);
  });
});

describe("fl_payouts_list once open", () => {
  it("lists each joined offer's status from every settle precondition, units as unknown", async () => {
    const w = work([chainOffer(1), chainOffer(2, { openChallenges: 1 }), chainOffer(3), chainOffer(4, { payoutPostedAt: 1_700_000_000 + 86_400 }), chainOffer(5, { state: "running", payoutRoot: ZERO32 })], [3], 2n * SALT);
    const out = (await runFlPayoutTool("fl_payouts_list", {}, deps(w))).result;
    expect(out).toContain("Pull balance, ready to withdraw: 2");
    expect(out).toMatch(/- offer 1: Claimable; units × 1\.5\d* SALT per unit = — \(your units are in the coordinator's payout list/);
    expect(out).toContain("- offer 2: A challenge to the payout list is open");
    expect(out).toContain("- offer 3: Claimed");
    expect(out).toContain("- offer 4: Waiting for the dispute window");
    expect(out).not.toContain("offer 5");
  });

  it("without the chain's time never says Claimable", async () => {
    const out = (await runFlPayoutTool("fl_payouts_list", {}, deps(work([chainOffer(1)]), { chainNow: async () => { throw new Error("rpc down"); } }))).result;
    expect(out).not.toContain("Claimable");
    expect(out).toContain("the chain's time was not read");
  });
});

describe("fl_withdraw_propose once open", () => {
  it("proposes WorkMarket.withdraw() for the pull balance; the member signs in the wallet review", async () => {
    const propose = vi.fn(async () => ({ proposed: true }));
    const out = await runFlPayoutTool("fl_withdraw_propose", {}, deps(work([], [], 3n * SALT), { propose }));
    expect(propose).toHaveBeenCalledWith({ address: WM, calldata: withdrawCalldata(), valueWei: "0", label: WITHDRAW_SIGNATURE });
    expect(out.result).toMatch(/waiting in your wallet review.*Hermes cannot sign it/);
  });

  it("proposes nothing without a wallet on this device", async () => {
    const propose = vi.fn();
    expect((await runFlPayoutTool("fl_withdraw_propose", {}, deps(work([], [], SALT), { propose, wallet: () => "" }))).result).toMatch(/no wallet on this device yet/);
    expect(propose).not.toHaveBeenCalled();
  });

  it("proposes nothing with a 0 balance, and reports core's refusal in its own words", async () => {
    const propose = vi.fn();
    expect((await runFlPayoutTool("fl_withdraw_propose", {}, deps(work([]), { propose }))).result).toMatch(/nothing to withdraw/);
    expect(propose).not.toHaveBeenCalled();
    const refused = async () => { throw new Error("FL payouts are not open on network \"r1007\", so this WorkMarket withdraw call is not signed"); };
    const out = await runFlPayoutTool("fl_withdraw_propose", {}, deps(work([], [], SALT), { propose: refused }));
    expect(out.result).toBe("The withdrawal was not proposed: FL payouts are not open on network \"r1007\", so this WorkMarket withdraw call is not signed");
  });
});

describe("fl_claim_propose and fl_dispute_propose are named-blocked", () => {
  it("claim: a line settle would revert on is refused with its status; a Claimable line names the missing leaf; nothing is proposed", async () => {
    const propose = vi.fn();
    const w = work([chainOffer(1), chainOffer(2, { openChallenges: 1 })]);
    expect((await runFlPayoutTool("fl_claim_propose", { offerId: 2 }, deps(w, { propose }))).result).toBe("Offer 2: A challenge to the payout list is open. A claim would be refused on chain now, so nothing was proposed.");
    expect((await runFlPayoutTool("fl_claim_propose", { offerId: 1 }, deps(w, { propose }))).result).toBe(`Offer 1: Claimable. ${CLAIM_NEEDS_LEAF} Nothing was proposed.`);
    expect((await runFlPayoutTool("fl_claim_propose", { offerId: 1 }, deps(w, { propose, chainNow: async () => { throw new Error("x"); } }))).result).toMatch(/chain's time was not read\)\. A claim would be refused/);
    expect((await runFlPayoutTool("fl_claim_propose", { offerId: 9 }, deps(w, { propose }))).result).toMatch(/did not join offer 9/);
    expect(propose).not.toHaveBeenCalled();
  });

  it("dispute names the missing signed receipt and proposes nothing", async () => {
    const propose = vi.fn();
    const out = await runFlPayoutTool("fl_dispute_propose", { offerId: 1, reason: "MISSING_LEAF" }, deps(work([chainOffer(1)]), { propose }));
    expect(out.result).toBe(`Offer 1: Claimable. ${DISPUTE_NEEDS_RECEIPT} Nothing was proposed.`);
    expect(propose).not.toHaveBeenCalled();
    expect((await runFlPayoutTool("fl_dispute_propose", {}, deps(work([])))).result).toMatch(/whole number/);
  });
});
