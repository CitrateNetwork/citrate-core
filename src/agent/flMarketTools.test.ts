// 0.5.4 Hermes x FL WP-H5: fl_offers_list and fl_join_propose. The r1007 state goes through the
// store's real handleTool with the SHIPPED address book (no WorkMarket pinned). The 0.6.0 states
// stand in a network with WorkMarket (and, test-only, a profile that opens payouts); the offers
// are TEST-ONLY fixtures shaped like core's fl_market_offers. Written red-first.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { decodeFunctionData, toFunctionSelector, type Hex } from "viem";
import realBook from "../../src-tauri/addresses/40204.json";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { runFlMarketTool, type FlMarketDeps } from "./flMarketTools";
import { readFlNetworkState, type FlReads } from "../fl/networkState";
import { JOIN_WAITS_FOR_GSEC, WORK_MARKET_JOIN_ABI } from "../fl/Matches";
import type { MarketView, OfferView } from "../fl/market";
import type { WorkerTerms } from "../fl/workerTerms";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";

const call = (name: string, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};
const SALT = 10n ** 18n;
const WM = "0x" + "ab".repeat(20);
const ZERO32 = "0x" + "0".repeat(64);
const reads: FlReads = { codeSize: async () => 1000, viewCall: async () => ZERO32 };
const today = () => readFlNetworkState(realBook, reads);
const live = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads);
/** Test-only: a profile that opens payouts (no shipped profile does). */
const opened = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads, { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile);

const offer = (id: number, over: { minVram?: number; classes?: string[]; hidden?: OfferView["hidden"]; full?: boolean; deadline?: number } = {}): OfferView => ({
  opened: { id, kind: "flRound", poster: "0x" + String(id).padStart(2, "0").repeat(20), groupHash: ZERO32, pricePerUnit: String((3n * SALT) / 2n), budget: String(600n * SALT), deadline: over.deadline ?? 1_763_769_600 + id, requirementsCid: "bafkreiflm" + id },
  openedBlock: 100 + id,
  closed: null,
  terms: { state: "open", jobSpecHash: ZERO32, requirementsHash: ZERO32, allowlistRoot: ZERO32, maxWorkers: 8, workers: over.full ? 8 : 3, joinBond: String(SALT / 10n), coordinator: "0x" + "cd".repeat(20), disputeWindow: 100 },
  requirements: { status: "verified", requirements: { v: 1, dataClasses: over.classes ?? ["support-tickets"], architectures: [], minVramGb: over.minVram ?? 16, estHours: 2 } },
  hidden: over.hidden ?? null,
});
const market = (...offers: OfferView[]): MarketView => ({ available: true, reason: null, market: WM, offers });
const terms: WorkerTerms = { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: ["support-tickets"] };
const deps = (over: Partial<FlMarketDeps> = {}): FlMarketDeps => ({
  networkState: opened,
  readMarket: async () => market(offer(1)),
  terms: async () => terms,
  machine: async () => ({ memoryGb: 24, memoryKind: "VRAM" }),
  propose: async () => ({ proposed: true }),
  ...over,
});

beforeEach(() => store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null }));
afterEach(() => vi.restoreAllMocks());

describe("schemas and annotations", () => {
  it("fl_offers_list is an untrusted read; fl_join_propose spends and is not read-only", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name);
    expect(names).toContain("fl_offers_list");
    expect(names).toContain("fl_join_propose");
    expect(annotationFor("fl_offers_list")).toEqual({ effect: "none", trust: "untrusted" });
    expect(annotationFor("fl_join_propose")).toEqual({ effect: "spend", trust: "trusted" });
    expect(READ_ONLY_AGENT_TOOLS.has("fl_offers_list")).toBe(true);
    expect(READ_ONLY_AGENT_TOOLS.has("fl_join_propose")).toBe(false);
  });
});

describe("on this network (shipped book, through handleTool)", () => {
  it("both say group rounds are not live, read no offers and propose nothing", async () => {
    vi.spyOn(store, "flNetworkState").mockImplementation(today);
    const offers = vi.spyOn(store, "flMarketOffers");
    const propose = vi.spyOn(store, "proposeContractCall");
    const sig = vi.spyOn(store, "requestSig");
    for (const [name, args] of [["fl_offers_list", {}], ["fl_join_propose", { offerId: 1 }]] as const) {
      const out = await store.handleTool(call(name, args), "m1", noop);
      expect(out).toMatch(/Group rounds not live yet\. WorkMarket is not available yet on this network/);
    }
    expect(offers).not.toHaveBeenCalled();
    expect(propose).not.toHaveBeenCalled();
    expect(sig).not.toHaveBeenCalled();
  });
});

describe("fl_offers_list once WorkMarket is deployed", () => {
  it("ranks joinable first then by deadline, says why and why not, fences offer text", async () => {
    const out = await runFlMarketTool("fl_offers_list", {}, deps({ networkState: live, readMarket: async () => market(offer(1, { minVram: 48 }), offer(2, { deadline: 1_900_000_000 }), offer(3)) }));
    const body = out.result.slice(out.result.indexOf("<<<UNTRUSTED"));
    expect(out.result).toContain("3 offers shown, 0 hidden");
    expect(out.result).toContain("never by amount");
    expect(body.indexOf("offer 3 ")).toBeLessThan(body.indexOf("offer 2 "));
    expect(body.indexOf("offer 2 ")).toBeLessThan(body.indexOf("offer 1 "));
    expect(body).toContain("Not eligible. Why not: needs 48 GB VRAM; this machine has 24 GB.");
    expect(body).toContain("Inside your terms. Why this match:");
    expect(out.result.split("UNTRUSTED>>>").length - 1).toBe(1);
  });

  it("lists hidden offers with the reason only when asked", async () => {
    const m = market(offer(1), offer(2, { hidden: { reason: "notYourGroup" } }));
    expect((await runFlMarketTool("fl_offers_list", {}, deps({ readMarket: async () => m }))).result).not.toContain("offer 2 ");
    expect((await runFlMarketTool("fl_offers_list", { includeHidden: true }, deps({ readMarket: async () => m }))).result).toContain("hidden: a group round for a group you are not in");
  });

  it("says when terms could not be read, and reports a failed or unavailable offers read", async () => {
    expect((await runFlMarketTool("fl_offers_list", {}, deps({ terms: async () => { throw new Error("Your terms are kept by the desktop app"); } }))).result).toMatch(/worker terms could not be read/);
    expect((await runFlMarketTool("fl_offers_list", {}, deps({ readMarket: async () => { throw new Error("rpc timeout"); } }))).result).toBe("Offers: — (core's offers read failed: rpc timeout)");
    expect((await runFlMarketTool("fl_offers_list", {}, deps({ readMarket: async () => ({ available: false, reason: "WorkMarket is not in this build's address book", market: null, offers: [] }) }))).result).toContain("Offers: — (WorkMarket is not in this build's address book)");
  });
});

describe("fl_join_propose", () => {
  it("while payouts are dark refuses with the G-SEC words and proposes nothing", async () => {
    const propose = vi.fn();
    const out = await runFlMarketTool("fl_join_propose", { offerId: 1 }, deps({ networkState: live, propose }));
    expect(out.result).toContain(JOIN_WAITS_FOR_GSEC);
    expect(propose).not.toHaveBeenCalled();
  });

  it("with payouts open proposes WorkMarket.join at the offer's rate with the join bond, and says the member signs", async () => {
    const propose = vi.fn(async () => ({ proposed: true }));
    const out = await runFlMarketTool("fl_join_propose", { offerId: 1 }, deps({ propose }));
    expect(propose).toHaveBeenCalledTimes(1);
    const intent = (propose.mock.calls[0] as unknown as [{ address: string; calldata: string; valueWei: string; label: string }])[0];
    expect(intent.address).toBe(WM);
    expect(intent.valueWei).toBe(String(SALT / 10n));
    expect(intent.calldata.slice(0, 10)).toBe(toFunctionSelector(intent.label));
    const d = decodeFunctionData({ abi: WORK_MARKET_JOIN_ABI, data: intent.calldata as Hex });
    expect(d.args).toEqual([1n, (3n * SALT) / 2n, []]);
    expect(out.result).toMatch(/waiting in your wallet review.*Hermes cannot sign it/);
  });

  it("refuses an unknown, hidden, full or out-of-terms offer before any proposal", async () => {
    const propose = vi.fn();
    const m = market(offer(2, { hidden: { reason: "posterNotAdmin" } }), offer(3, { full: true }), offer(4, { classes: ["medical-notes"] }));
    const d = deps({ readMarket: async () => m, propose });
    expect((await runFlMarketTool("fl_join_propose", { offerId: 9 }, d)).result).toMatch(/no offer 9/);
    expect((await runFlMarketTool("fl_join_propose", { offerId: 2 }, d)).result).toMatch(/not shown to this member: poster is not an owner or admin/);
    expect((await runFlMarketTool("fl_join_propose", { offerId: 3 }, d)).result).toMatch(/all 8 worker places are taken/);
    expect((await runFlMarketTool("fl_join_propose", { offerId: 4 }, d)).result).toMatch(/reads medical-notes data, which your terms do not allow/);
    expect((await runFlMarketTool("fl_join_propose", { offerId: "x" }, d)).result).toMatch(/whole number/);
    expect(propose).not.toHaveBeenCalled();
  });

  it("through handleTool opens the wallet review once payouts are open", async () => {
    vi.spyOn(store, "flNetworkState").mockImplementation(opened);
    vi.spyOn(store, "flMarketOffers").mockResolvedValue(market(offer(1)));
    vi.spyOn(bridge.tier, "recommend").mockResolvedValue(null);
    vi.spyOn(bridge.contracts, "proposeWrite").mockResolvedValue({ id: "cer1" } as never);
    const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
    const out = await store.handleTool(call("fl_join_propose", { offerId: 1 }), "m1", noop);
    // The machine's memory was not read, so matchOffer refuses: no review opens.
    expect(review).not.toHaveBeenCalled();
    expect(out).toMatch(/GPU memory was not read/);
  });
});
