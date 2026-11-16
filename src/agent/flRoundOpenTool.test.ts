// 0.5.4 Hermes x FL WP-H5: fl_round_open_propose. The r1007 state goes through the store's real
// handleTool with the shipped address book; the 0.6.0 states (test-only: WorkMarket pinned, the gate
// open, a binding) drive the module. Drafts and bindings here are TEST-ONLY fixtures.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { decodeFunctionData, toFunctionSelector, type Hex } from "viem";
import realBook from "../../src-tauri/addresses/40204.json";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { runFlRoundOpen, type FlRoundOpenDeps } from "./flRoundOpenTool";
import { readFlNetworkState, type FlReads } from "../fl/networkState";
import { OPEN_NEEDS_BINDING, OPEN_SIGNATURE, WORK_MARKET_OPEN_ABI, saveRoundDraft } from "../fl/roundDraft";
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
  roleIn: async () => "admin",
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

  it("refuses in openIntent's order: gate dark with its reason, then no binding, then a bad draft (past the money and role checks)", async () => {
    const propose = vi.fn();
    expect((await runFlRoundOpen(deps({ networkState: dark, propose }))).result).toMatch(/opens with payouts after the security review\. WorkMarket is not pinned on this network/);
    expect((await runFlRoundOpen(deps({ binding: async () => null, propose }))).result).toContain(OPEN_NEEDS_BINDING);
    expect((await runFlRoundOpen(deps({ loadDraft: () => ({ ...FILLED_DRAFT, maxWorkers: 2 }), propose }))).result).toMatch(/At least three workers/);
    expect(propose).not.toHaveBeenCalled();
  });

  it("only an owner or admin of the draft's group on the live roster may open it (WorkMarket does not check)", async () => {
    const propose = vi.fn(async () => ({ proposed: true }));
    expect((await runFlRoundOpen(deps({ roleIn: async () => "member", propose }))).result).toMatch(/Your role in this group is member\. Only owners and admins can open a round/);
    expect((await runFlRoundOpen(deps({ roleIn: async () => null, propose }))).result).toMatch(/not on this group's live roster/);
    expect((await runFlRoundOpen(deps({ roleIn: async () => { throw new Error("comms daemon not running"); }, propose }))).result).toMatch(/couldn't read the group's roster.*comms daemon not running/);
    expect(propose).not.toHaveBeenCalled();
    await runFlRoundOpen(deps({ roleIn: async () => "owner", propose }));
    expect(propose).toHaveBeenCalledTimes(1);
  });

  it("through handleTool checks the member's own role on the live roster", async () => {
    vi.spyOn(store, "flNetworkState").mockImplementation(payoutsOpenNetwork);
    vi.spyOn(store, "flRoundBinding").mockResolvedValue(ROUND_BINDING);
    store.setState({ commsAddr: "0xME" });
    const roster = (role: string) => [{ id: FILLED_DRAFT.groupId, name: "g", owner: "o", kind: "channel" as const, members: [{ address: "0xme", role }] }] as never;
    saveRoundDraft({ ...FILLED_DRAFT });
    const propose = vi.spyOn(store, "proposeContractCall").mockResolvedValue({ proposed: true });
    vi.spyOn(bridge.groups, "list").mockResolvedValue(roster("member"));
    expect(await store.handleTool(call, "m1", noop)).toMatch(/Your role in this group is member/);
    expect(propose).not.toHaveBeenCalled();
    vi.spyOn(bridge.groups, "list").mockResolvedValue(roster("admin"));
    expect(await store.handleTool(call, "m1", noop)).toMatch(/waiting in your wallet review/);
    expect(propose).toHaveBeenCalledTimes(1);
    store.setState({ commsAddr: "" });
  });

  it("PAY-19: refuses a Hermes-sourced draft and empty money fields, whatever amounts it carries", async () => {
    const propose = vi.fn();
    expect((await runFlRoundOpen(deps({ loadDraft: () => ({ ...FILLED_DRAFT, source: "hermes" }), propose }))).result).toMatch(/handed in by Hermes, and Hermes may never set money/);
    expect((await runFlRoundOpen(deps({ loadDraft: () => ({ ...FILLED_DRAFT, budgetSalt: "", joinBondSalt: " " }), propose }))).result).toBe(
      "The draft has no budget, join bond yet. Only the member types amounts, on Groups › Rounds › Open a round. Nothing was proposed.",
    );
    expect(propose).not.toHaveBeenCalled();
  });

  it("PAY-19 keys on source, never draftedBy: a Hermes draft the member filled in and saved opens", async () => {
    const propose = vi.fn(async () => ({ proposed: true }));
    await runFlRoundOpen(deps({ loadDraft: () => ({ ...FILLED_DRAFT, source: "member", draftedBy: "hermes" }), propose }));
    expect(propose).toHaveBeenCalledTimes(1);
    const before = propose.mock.calls.length;
    expect((await runFlRoundOpen(deps({ loadDraft: () => ({ ...FILLED_DRAFT, source: "hermes", draftedBy: "hermes" }), propose }))).result).toMatch(/Hermes may never set money/);
    expect(propose.mock.calls.length).toBe(before);
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
