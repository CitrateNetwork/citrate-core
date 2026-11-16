// 0.5.4 Hermes x FL WP-H5: fl_round_open_propose. The r1007 state goes through the store's real
// handleTool with the shipped address book; the 0.6.0 states (test-only: WorkMarket pinned, the gate
// open, a binding) drive the module. Drafts and bindings here are TEST-ONLY fixtures.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { decodeFunctionData, toFunctionSelector, type Hex } from "viem";
import realBook from "../../src-tauri/addresses/40204.json";
import { store } from "../shell/store";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { runFlRoundOpen, type FlRoundOpenDeps } from "./flRoundOpenTool";
import { readFlNetworkState, type FlReads } from "../fl/networkState";
import { BLANK_DRAFT, OPEN_NEEDS_BINDING, OPEN_SIGNATURE, WORK_MARKET_OPEN_ABI, saveRoundDraft } from "../fl/roundDraft";
import { FILLED_DRAFT, ROUND_BINDING, payoutsOpenNetwork } from "../fl/fixtures/market";

const call: ToolCall = { id: "c1", name: "fl_round_open_propose", arguments: "{}" };
const noop = () => {};
const WM = "0x" + "ab".repeat(20);
const ZERO32 = "0x" + "0".repeat(64);
const reads: FlReads = { codeSize: async () => 1000, viewCall: async () => ZERO32 };
const dark = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, { ...reads, payoutsStatus: async () => ({ open: false, reason: "WorkMarket is not pinned on this network" }) });
const deps = (over: Partial<FlRoundOpenDeps> = {}): FlRoundOpenDeps => ({
  loadDraft: () => ({ ...FILLED_DRAFT }),
  networkState: payoutsOpenNetwork,
  binding: async () => ROUND_BINDING,
  propose: async () => ({ proposed: true }),
  ...over,
});

beforeEach(() => {
  localStorage.clear();
  store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null });
});
afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("fl_round_open_propose", () => {
  it("is a reviewed spend, not read-only", () => {
    expect(AGENT_TOOLS.map((t) => t.function.name)).toContain("fl_round_open_propose");
    expect(READ_ONLY_AGENT_TOOLS.has("fl_round_open_propose")).toBe(false);
    expect(annotationFor("fl_round_open_propose")).toEqual({ effect: "spend", trust: "trusted" });
  });

  it("on this network says group rounds are not live, keeps the draft and proposes nothing (handleTool)", async () => {
    saveRoundDraft({ ...FILLED_DRAFT });
    const propose = vi.spyOn(store, "proposeContractCall");
    const out = await store.handleTool(call, "m1", noop);
    expect(out).toMatch(/Group rounds not live yet\. WorkMarket is not available yet on this network/);
    expect(out).toContain("The draft stays saved");
    expect(propose).not.toHaveBeenCalled();
  });

  it("with no draft says how to make one", async () => {
    expect((await runFlRoundOpen(deps({ loadDraft: () => null }))).result).toMatch(/no round draft on this device.*round_draft_propose/);
  });

  it("refuses in openIntent's order: gate dark with its reason, then no binding, then a bad draft", async () => {
    const propose = vi.fn();
    expect((await runFlRoundOpen(deps({ networkState: dark, propose }))).result).toMatch(/opens with payouts after the security review\. WorkMarket is not pinned on this network/);
    expect((await runFlRoundOpen(deps({ binding: async () => null, propose }))).result).toContain(OPEN_NEEDS_BINDING);
    expect((await runFlRoundOpen(deps({ loadDraft: () => ({ ...BLANK_DRAFT }), propose }))).result).toMatch(/Choose a group/);
    expect(propose).not.toHaveBeenCalled();
  });

  it("with everything in place proposes WorkMarket.open escrowing the budget; the member signs", async () => {
    const propose = vi.fn(async () => ({ proposed: true }));
    const out = await runFlRoundOpen(deps({ propose }));
    const intent = (propose.mock.calls[0] as unknown as [{ address: string; calldata: string; valueWei: string; label: string }])[0];
    expect(intent.address).toBe(WM);
    expect(intent.label).toBe(OPEN_SIGNATURE);
    expect(intent.valueWei).toBe(String(600n * 10n ** 18n));
    expect(intent.calldata.slice(0, 10)).toBe(toFunctionSelector(OPEN_SIGNATURE));
    expect(decodeFunctionData({ abi: WORK_MARKET_OPEN_ABI, data: intent.calldata as Hex }).args[0]).toMatchObject({ requirementsCid: "bafkreifixture", maxWorkers: 8 });
    expect(out.result).toMatch(/600(\.0+)? SALT escrowed.*HIC-1.*Hermes cannot sign it/);
  });

  it("reports core's refusal before the card in its own words", async () => {
    const out = await runFlRoundOpen(deps({ propose: async () => { throw new Error("FL payouts are not open on network \"r1007\", so this WorkMarket open call is not signed"); } }));
    expect(out.result).toBe("Opening the round was not proposed: FL payouts are not open on network \"r1007\", so this WorkMarket open call is not signed");
  });

  it("the store has no binding source yet, so it is null", async () => {
    expect(await store.flRoundBinding()).toBeNull();
  });
});
