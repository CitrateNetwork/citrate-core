// 0.5.4 Hermes x FL WP-H10: fl_payouts_list, shipped dark. The r1007 and dark states go through the
// store's real handleTool; the open state (test-only: no shipped profile opens payouts) drives the
// module with core's shapes. Offers here are TEST-ONLY fixtures. Written red-first.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import realBook from "../../src-tauri/addresses/40204.json";
import { store } from "../shell/store";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { runFlPayoutTool, type FlPayoutDeps } from "./flPayoutTools";
import { readFlNetworkState, type FlReads } from "../fl/networkState";
import type { MyWork } from "../fl/myWork";
import type { ChainOffer } from "../fl/workMarketReads";
import { payoutsOpenNetwork } from "../fl/fixtures/market";

const call = (args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name: "fl_payouts_list", arguments: JSON.stringify(args) });
const noop = () => {};
const SALT = 10n ** 18n;
const WM = "0x" + "ab".repeat(20);
const ZERO32 = "0x" + "0".repeat(64);
const ROOT = "0x" + "77".repeat(32);
const ME = "0x" + "5b".repeat(20);
const reads: FlReads = { codeSize: async () => 1000, viewCall: async () => ZERO32 };
const today = () => readFlNetworkState(realBook, reads);
const dark = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads);

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
  wallet: () => ME,
  call: async () => { throw new Error("not called: readMyWork is stood in"); },
  chainNow: async () => 1_700_000_000 + 86_400 + 10,
  readMyWork: async () => w,
  ...over,
});

beforeEach(() => store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null }));
afterEach(() => vi.restoreAllMocks());

describe("fl_payouts_list", () => {
  it("is a reviewed trusted read", () => {
    expect(AGENT_TOOLS.map((t) => t.function.name)).toContain("fl_payouts_list");
    expect(READ_ONLY_AGENT_TOOLS.has("fl_payouts_list")).toBe(true);
    expect(annotationFor("fl_payouts_list")).toEqual({ effect: "none", trust: "trusted" });
  });

  it("on this network says WorkMarket is not available and reads nothing (handleTool)", async () => {
    vi.spyOn(store, "flNetworkState").mockImplementation(today);
    const out = await store.handleTool(call(), "m1", noop);
    expect(out).toMatch(/^Payouts: none to read\. WorkMarket is not available yet on this network/);
  });

  it("with WorkMarket but payouts dark says the required words and reads nothing (handleTool)", async () => {
    vi.spyOn(store, "flNetworkState").mockImplementation(dark);
    const out = await store.handleTool(call(), "m1", noop);
    expect(out).toBe("Payouts: Payouts open after the security review. [address book, eth_getCode and the network profile]");
  });

  it("once open lists each joined offer's status from every settle precondition, units as unknown", async () => {
    const w = work(
      [
        chainOffer(1),
        chainOffer(2, { openChallenges: 1 }),
        chainOffer(3),
        chainOffer(4, { payoutPostedAt: 1_700_000_000 + 86_400 }),
        chainOffer(5, { state: "running", payoutRoot: ZERO32 }),
      ],
      [3],
      2n * SALT,
    );
    const out = await runFlPayoutTool("fl_payouts_list", {}, deps(w));
    expect(out).toContain("Pull balance, ready to withdraw: 2");
    expect(out).toMatch(/- offer 1: Claimable; units × 1\.5\d* SALT per unit = — \(your units are in the coordinator's payout list/);
    expect(out).toContain("- offer 2: A challenge to the payout list is open");
    expect(out).toContain("- offer 3: Claimed");
    expect(out).toContain("- offer 4: Waiting for the dispute window");
    expect(out).not.toContain("offer 5"); // no payout list posted yet
    expect(out).toMatch(/Hermes cannot claim, withdraw or dispute for you/);
  });

  it("without the chain's time never says Claimable", async () => {
    const out = await runFlPayoutTool("fl_payouts_list", {}, deps(work([chainOffer(1)]), { chainNow: async () => { throw new Error("rpc down"); } }));
    expect(out).not.toContain("Claimable");
    expect(out).toContain("the chain's time was not read");
  });

  it("a failed read is unknown with its reason", async () => {
    const out = await runFlPayoutTool("fl_payouts_list", {}, deps(work([]), { readMyWork: async () => { throw new Error("no wallet on this device yet"); } }));
    expect(out).toBe("Payouts: — (no wallet on this device yet)");
  });
});
