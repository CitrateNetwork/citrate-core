// HUP-S6.4 — contract_deploy through the store: the review carries the gate record that core
// checked; a refusal reaches the agent as the honest reason and opens no review.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ToolCall } from "../agent/harness";
import { GATE_ITEM_IDS, type DeployGateRecord } from "../agent/deployGate";

const call = (name: string, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};
const H = "0x" + "aa".repeat(32);
const gate: DeployGateRecord = {
  initcodeHash: H,
  bindingHash: "0x" + "bb".repeat(32),
  compiler: { solcVersion: "0.8.28", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
  verdict: "READY",
  items: GATE_ITEM_IDS.map((id) => ({ id, label: id, pass: true, reason: "ok", evidence: { counts: {}, outputSha256: null, durationMs: null, toolVersion: null } })),
  evaluatedAtMs: 1,
};
const view = { id: "cer1", origin: "local-user", kind: "transaction", chainId: 40204, decoded: { action: "contract creation", cost: "", destination: "" }, requiresRawAck: false };
const REFUSAL = "Deploy refused: the D-4 deploy gate is NOT READY for bytecode " + H + ". Failing: Aderyn: aderyn is not installed (a missing tool is a fail, never a pass).";

beforeEach(() => store.setState({ chatMsgs: [], queue: [], walletReview: null }));
afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ walletReview: null });
});

describe("Feature: the agent's contract_deploy honours the D-4 gate", () => {
  it("Given a READY gate, then the wallet review carries the gate record", async () => {
    vi.spyOn(bridge.contracts, "deploy").mockResolvedValue({ ...view, gate } as never);
    const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
    await store.handleTool(call("contract_deploy", { bytecodeHex: "0x6000" }), "m1", noop);
    expect(review.mock.calls[0][5]?.deployGate).toEqual(gate);
  });

  it("Given core refuses (NOT READY), then no review opens and the agent gets the refusal with the failing items", async () => {
    vi.spyOn(bridge.contracts, "deploy").mockRejectedValue(new Error(REFUSAL));
    const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
    const out = await store.handleTool(call("contract_deploy", { bytecodeHex: "0x6000" }), "m1", noop);
    expect(review).not.toHaveBeenCalled();
    expect(out).toContain("NOT READY");
    expect(out).toContain("Aderyn");
    expect(out).toMatch(/do not retry|explain/i);
  });
});

describe("Feature: the Contracts tab deploy honours the D-4 gate", () => {
  it("Given a READY gate, then deployContract opens the review with the gate record and reports ok", async () => {
    vi.spyOn(bridge.contracts, "deploy").mockResolvedValue({ ...view, gate } as never);
    const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
    const r = await store.deployContract({ bytecodeHex: "0x6000" });
    expect(r.ok).toBe(true);
    expect(review.mock.calls[0][5]?.deployGate).toEqual(gate);
  });

  it("Given a refusal, then deployContract returns the gate lookup so the verdict card can show why", async () => {
    vi.spyOn(bridge.contracts, "deploy").mockRejectedValue(new Error(REFUSAL));
    const lookup = vi.spyOn(bridge.contracts, "gateLookup").mockResolvedValue({ initcodeHash: H, record: { ...gate, verdict: "NOT_READY" } });
    const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
    const r = await store.deployContract({ bytecodeHex: "0x6000", constructorArgsHex: "0x01" });
    expect(review).not.toHaveBeenCalled();
    expect(r.ok).toBe(false);
    expect(lookup).toHaveBeenCalledWith("0x6000", "0x01");
    expect(r.error).toContain("NOT READY");
    expect(r.gate?.initcodeHash).toBe(H);
    expect(r.gate?.record?.verdict).toBe("NOT_READY");
  });

  it("Given a refusal and the lookup also fails, then deployContract still returns the honest error", async () => {
    vi.spyOn(bridge.contracts, "deploy").mockRejectedValue(new Error(REFUSAL));
    vi.spyOn(bridge.contracts, "gateLookup").mockRejectedValue(new Error("no node"));
    const r = await store.deployContract({ bytecodeHex: "0x6000" });
    expect(r.ok).toBe(false);
    expect(r.error).toContain("NOT READY");
    expect(r.gate).toBeNull();
  });
});
