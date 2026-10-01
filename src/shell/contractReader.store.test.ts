// HUP-S6.7 — the Contract reader's main-window actions: a write call stops at the Signature
// Ceremony review, and an explanation is a tool-less Hermes turn that refuses when no model is
// connected or Hermes is busy.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ChatProvider } from "../agent/harness";

const ADDR = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
const view = { id: "cer7", origin: "local-user:contract-reader", kind: "transaction", chainId: 40204, decoded: { action: "unrecognized call", cost: "", destination: ADDR }, requiresRawAck: true };

let savedProvider: ChatProvider | null = null;
beforeEach(() => {
  savedProvider = store.provider;
  store.setState({ walletReview: null, chatStatus: "ready" });
});
afterEach(() => {
  vi.restoreAllMocks();
  store.provider = savedProvider;
  store.setState({ walletReview: null });
});

describe("Feature: a Contract reader write goes through the ceremony", () => {
  it("Given a proposal, then the review opens with the decoded call and nothing is broadcast", async () => {
    const propose = vi.spyOn(bridge.contracts, "proposeWrite").mockResolvedValue(view as never);
    const broadcast = vi.spyOn(bridge.signing, "broadcast");
    await store.proposeContractCall({ address: ADDR, calldata: "0xa0712d68", valueWei: "0", label: "mint(uint256)" });
    expect(propose).toHaveBeenCalledWith(ADDR, "0xa0712d68", "0");
    const r = store.state.walletReview;
    expect(r?.kind).toBe("contract-call");
    expect(r?.label).toContain("mint(uint256)");
    expect(r?.view.id).toBe("cer7");
    expect(broadcast).not.toHaveBeenCalled();
  });

  it("Given core refuses (the estimate failed), then the error reaches the reader and no review opens", async () => {
    vi.spyOn(bridge.contracts, "proposeWrite").mockRejectedValue(new Error("the write was not proposed: execution reverted"));
    await expect(store.proposeContractCall({ address: ADDR, calldata: "0xa0712d68", valueWei: "0", label: "mint" })).rejects.toThrow(/not proposed/);
    expect(store.state.walletReview).toBeNull();
  });

  it("Given a review is already open, then a second proposal is refused", async () => {
    vi.spyOn(bridge.contracts, "proposeWrite").mockResolvedValue(view as never);
    await store.proposeContractCall({ address: ADDR, calldata: "0xa0712d68", valueWei: "0", label: "mint" });
    await expect(store.proposeContractCall({ address: ADDR, calldata: "0xa0712d68", valueWei: "0", label: "mint" })).rejects.toThrow(/already waiting/);
  });
});

describe("Feature: Hermes explains a contract function", () => {
  it("Given a model, then the explanation is its answer and every tool call is refused", async () => {
    let toolAnswer = "";
    store.provider = {
      kind: "local",
      label: "local model",
      send: async ({ messages, callbacks }) => {
        expect(messages).toEqual([{ role: "user", content: "explain mint" }]);
        toolAnswer = await callbacks.onToolCall({ id: "t1", name: "contract_deploy", arguments: "{}" });
        return { role: "assistant", content: "It mints tokens for a fee." };
      },
    };
    const r = await store.explainContract("explain mint");
    expect(r).toEqual({ text: "It mints tokens for a fee.", by: "local model" });
    expect(toolAnswer).toMatch(/not available/i);
  });

  it("Given only the demo provider, then it refuses honestly", async () => {
    store.provider = { kind: "demo", label: "demo", send: vi.fn() };
    await expect(store.explainContract("explain")).rejects.toThrow(/no model/i);
  });

  it("Given Hermes is mid-turn, then it refuses instead of interleaving", async () => {
    store.provider = { kind: "local", label: "local model", send: vi.fn() };
    store.setState({ chatStatus: "thinking" });
    await expect(store.explainContract("explain")).rejects.toThrow(/busy/i);
    store.setState({ chatStatus: "ready" });
  });
});
