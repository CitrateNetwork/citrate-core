// 0.5.4 Hermes x FL WP-H5: fl_round_status and fl_lease_explain. The "today" tests go through the
// store's real handleTool with the SHIPPED address book (no WorkMarket pinned) and core's chain
// reads spied at the bridge. The 0.6.0 tests drive the module with a fake eth_call answering
// ABI-encoded returns; those offers and rounds are TEST-ONLY fixtures. Written red-first.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { decodeFunctionData, encodeFunctionResult, toFunctionSelector, type Hex } from "viem";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { runFlRoundTool, type FlRoundDeps } from "./flRoundTools";
import { FRL_ABI, WORK_MARKET_ABI, type ViewCall } from "../fl/workMarketReads";
import type { ContractReading, FlNetworkState } from "../fl/networkState";
import type { FlOverview } from "../bridge/domains";
import { livePlan } from "../fl/fixtures/plan";

const flOverview = (over: Partial<FlOverview> = {}): FlOverview => ({
  config: { url: null, source: "none", settingsUrl: null, note: null },
  starts: [], gates: [], activeAdapter: null, rememberedAdapter: null, restoreError: null,
  consentedRounds: [], consentFile: null, consentError: null, storeError: null, ...over,
});
const call = (name: string, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};
const SALT = 10n ** 18n;
const WM = "0x" + "ab".repeat(20);
const FRL = "0x25fc02fbfad9589d0dde84f858d0962bfa0253c5";
const Z32 = ("0x" + "0".repeat(64)) as Hex;
const ROUND = ("0x" + "3e".repeat(32)) as Hex;

const roundTuple = (status: number, fault = 0, over: Record<string, unknown> = {}) => ({
  clusterId: Z32, ordinal: 4n, configHash: Z32, participantsRoot: Z32, participants: 3, nValues: 0n, inputRoot: Z32, outputRoot: Z32, chunks: 1,
  adapterHash: ("0x" + "ad".repeat(32)) as Hex, deadline: 1_234_567n, status, fault, faultChunk: 0, ...over,
});
const offerTuple = (state: number) => ({
  kind: 0, state, poster: ("0x" + "11".repeat(20)) as Hex, verifier: ("0x" + "22".repeat(20)) as Hex, bindRef: ROUND, jobSpecHash: Z32, requirementsHash: Z32, groupHash: Z32, allowlistRoot: Z32,
  pricePerUnit: (3n * SALT) / 2n, budget: 600n * SALT, paid: 0n, refunded: 0n, maxUnits: 400n, deadline: 1_763_769_600n, disputeWindow: 86_400, maxWorkers: 8, workers: 3, joinBondsReleased: 0,
  joinBond: SALT / 10n, coordinator: ("0x" + "cd".repeat(20)) as Hex, coordinatorBond: 100n * SALT, disputeAdmin: ("0x" + "0".repeat(40)) as Hex, payoutRoot: Z32, leavesRoot: Z32, digest: Z32,
  payoutPostedAt: 0n, payoutUnits: 0n, settledUnits: 0n, coordinatorBondHeld: false, subjectiveDisputeUsed: false, reserveSwept: false, payoutEpoch: 0, openChallenges: 0, payoutDepth: 0,
  answerGraceUntil: 0n, resultEpoch: 0, challengeBondsHeld: 0n,
});

/** A fake eth_call: FRL getRound answers `round`; WorkMarket getOffer answers `offer`. */
function chain(round: ReturnType<typeof roundTuple>, offer?: ReturnType<typeof offerTuple>): ViewCall {
  return async (address, data) => {
    if (address === FRL && data.startsWith(toFunctionSelector("getRound(bytes32)"))) return encodeFunctionResult({ abi: FRL_ABI, functionName: "getRound", result: round });
    if (address === WM && offer) {
      const d = decodeFunctionData({ abi: WORK_MARKET_ABI, data: data as Hex });
      if (d.functionName === "getOffer") return encodeFunctionResult({ abi: WORK_MARKET_ABI, functionName: "getOffer", result: offer });
    }
    throw new Error("unexpected call " + address + " " + data.slice(0, 10));
  };
}

const deployed = (address: string): ContractReading => ({ kind: "deployed", address, codeSize: 100, rounds: "not-applicable" });
function net(workMarket: ContractReading): FlNetworkState {
  return {
    contracts: {
      FederatedRoundLedger: deployed(FRL),
      PatronageLedger: deployed("0x" + "41".repeat(20)),
      LoRAFactory: deployed("0x" + "62".repeat(20)),
      WorkMarket: workMarket,
      FLRoundVerifier: { kind: "not-in-book" },
      DatasetRegistry: { kind: "not-in-book" },
    },
    phase: workMarket.kind === "deployed" ? "rounds-live" : "today",
    payouts: workMarket.kind === "deployed" ? "dark" : "not-deployed",
    networkLabel: "Test network",
  };
}
const deps = (over: Partial<FlRoundDeps> = {}): FlRoundDeps => ({
  networkState: async () => net({ kind: "not-in-book" }),
  call: chain(roundTuple(1)),
  coordinator: async () => null,
  ...over,
});

beforeEach(() => {
  store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null });
});
afterEach(() => vi.restoreAllMocks());

describe("schemas and annotations", () => {
  it("offers both as reviewed reads", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name);
    for (const n of ["fl_round_status", "fl_lease_explain"]) {
      expect(names).toContain(n);
      expect(READ_ONLY_AGENT_TOOLS.has(n)).toBe(true);
      expect(annotationFor(n)).toEqual({ effect: "none", trust: "trusted" });
    }
  });
});

describe("fl_round_status on this network (shipped book, through handleTool)", () => {
  it("says group rounds are not live from the book, and reads a round record from the live FRL", async () => {
    vi.spyOn(bridge.contracts, "codeSize").mockResolvedValue(100);
    const view = vi.spyOn(bridge.contracts, "viewCall").mockImplementation(async (_t, address, data) => {
      if (address === FRL) return encodeFunctionResult({ abi: FRL_ABI, functionName: "getRound", result: roundTuple(1) });
      if (data.startsWith(toFunctionSelector("totalUnits()"))) return "0x" + "0".repeat(64);
      throw new Error("not read on this network: " + address);
    });
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview());
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call("fl_round_status", { offerId: 7, roundId: ROUND }), "m1", noop);
    expect(sig).not.toHaveBeenCalled();
    expect(out).toMatch(/Group rounds not live yet\. WorkMarket is not available yet on this network/);
    expect(out).toContain(`Round ${ROUND}: committed [FederatedRoundLedger.getRound, on chain]`);
    expect(out).toContain("accepted after block 1234567 unless a challenge proves it wrong");
    expect(out).toMatch(/Coordinator: none is configured/);
    // WorkMarket is never read: it is not in the shipped book.
    expect(view.mock.calls.every(([, a]) => a !== WM)).toBe(true);
  });

  it("with no ids lists the contracts in the required words", async () => {
    vi.spyOn(bridge.contracts, "codeSize").mockResolvedValue(100);
    vi.spyOn(bridge.contracts, "viewCall").mockResolvedValue("0x" + "0".repeat(64));
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview());
    const out = await store.handleTool(call("fl_round_status"), "m1", noop);
    expect(out).toContain("- FederatedRoundLedger: deployed, no rounds");
    expect(out).toContain("- WorkMarket: Not available yet on this network");
    expect(out).toContain("- DatasetRegistry: Not available yet on this network");
    expect(out).toMatch(/Group rounds not live yet: rounds reach members only through WorkMarket offers/);
  });

  it("an unreadable chain is unknown with its reason, never zero", async () => {
    vi.spyOn(bridge.contracts, "codeSize").mockRejectedValue(new Error("rpc timeout"));
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview());
    const out = await store.handleTool(call("fl_round_status", { roundId: ROUND }), "m1", noop);
    expect(out).toContain(`Round ${ROUND}: — FederatedRoundLedger is —`);
    expect(out).toContain("rpc timeout");
  });

  it("labels the coordinator's counts as its own", async () => {
    vi.spyOn(bridge.contracts, "codeSize").mockResolvedValue(100);
    vi.spyOn(bridge.contracts, "viewCall").mockResolvedValue("0x" + "0".repeat(64));
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ config: { url: "https://coordinator.example.org", source: "settings", settingsUrl: null, note: null } }));
    vi.spyOn(bridge.flRounds, "plan").mockResolvedValue(livePlan());
    const out = await store.handleTool(call("fl_round_status"), "m1", noop);
    expect(out).toContain("- leases: 3 pending, 1 leased, 0 done, 0 quarantined; 4 workers [coordinator-reported, its /v1/status, not on chain]");
    expect(out).toContain("the Belnap counts are not in the round's on-chain record");
  });

  it("refuses malformed ids without reading", async () => {
    const cs = vi.spyOn(bridge.contracts, "codeSize");
    expect(await store.handleTool(call("fl_round_status", { roundId: "0x12" }), "m1", noop)).toMatch(/64 hex/);
    expect(await store.handleTool(call("fl_round_status", { offerId: -1 }), "m1", noop)).toMatch(/whole number/);
    expect(cs).not.toHaveBeenCalled();
  });
});

describe("fl_round_status once WorkMarket is deployed (test-only chain)", () => {
  it("reads the offer and the round its bindRef names, each tagged by source", async () => {
    const out = await runFlRoundTool("fl_round_status", { offerId: 3 }, deps({ networkState: async () => net(deployed(WM)), call: chain(roundTuple(1), offerTuple(1)) }));
    expect(out).toContain("Offer 3 (federated round): verifying [WorkMarket.getOffer, on chain]");
    expect(out).toContain("workers: 3 of at most 8");
    expect(out).toMatch(/escrow: budget 600(\.00)? SALT/);
    expect(out).toContain(`Round ${ROUND}: committed [FederatedRoundLedger.getRound, on chain]`);
  });

  it("a failed WorkMarket read is unknown with its reason", async () => {
    const out = await runFlRoundTool("fl_round_status", { offerId: 3 }, deps({ networkState: async () => net(deployed(WM)), call: async () => { throw new Error("execution reverted"); } }));
    expect(out).toContain("Offer 3: — (the WorkMarket read failed: execution reverted)");
  });
});

describe("fl_lease_explain", () => {
  it("names a rejected round's fault in plain words and says a lease cannot be singled out", async () => {
    const out = await runFlRoundTool("fl_lease_explain", { roundId: ROUND, leaseIndex: 2 }, deps({ call: chain(roundTuple(2, 2)) }));
    expect(out).toContain("was rejected: row mismatch");
    expect(out).toContain("not the row that participant's committed update holds");
    expect(out).toContain("a rejected round never pays");
    expect(out).toMatch(/Lease 2: its receipt is the coordinator's/);
  });

  it("says a committed round has not been rejected, and an accepted one has no fault", async () => {
    expect(await runFlRoundTool("fl_lease_explain", { roundId: ROUND }, deps({ call: chain(roundTuple(1)) }))).toMatch(/has not been rejected/);
    expect(await runFlRoundTool("fl_lease_explain", { roundId: ROUND }, deps({ call: chain(roundTuple(3)) }))).toMatch(/accepted; no fault was proved/);
    expect(await runFlRoundTool("fl_lease_explain", { roundId: ROUND }, deps({ call: chain(roundTuple(0)) }))).toMatch(/holds no round with id/);
  });

  it("refuses a bad round id or lease index", async () => {
    expect(await runFlRoundTool("fl_lease_explain", {}, deps())).toMatch(/needs the round id/);
    expect(await runFlRoundTool("fl_lease_explain", { roundId: ROUND, leaseIndex: 1.5 }, deps())).toMatch(/lease index/);
  });
});
