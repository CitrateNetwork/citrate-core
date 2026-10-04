// HUP-S6.7 / US-6.3 AC2 — the contract_view agent tool through the real store entry point, with
// the bridge spied at its boundary. Read-only: no approval gate, no ceremony, no write.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { encodeAbiParameters } from "viem";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ToolCall } from "../agent/harness";

const call = (args: Record<string, unknown>): ToolCall => ({ id: "c1", name: "contract_view", arguments: JSON.stringify(args) });
const noop = () => {};
const ADDR = "0x" + "cd".repeat(20);

beforeEach(() => store.setState({ chatMsgs: [] }));
afterEach(() => vi.restoreAllMocks());

describe("contract_view", () => {
  it("runs an eth_call through core's contract_view_call and asks for no approval", async () => {
    const view = vi.spyOn(bridge.contracts, "viewCall").mockResolvedValue(encodeAbiParameters([{ type: "uint256" }], [42n]));
    const src = vi.spyOn(bridge.contracts, "verifiedSource");
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(
      call({ address: ADDR, function: "totalMinted", abi_fragment: "function totalMinted() view returns (uint256)" }),
      "m1",
      noop,
    );
    expect(view).toHaveBeenCalledWith("citrate", ADDR, expect.stringMatching(/^0x[0-9a-f]{8}$/));
    expect(src).not.toHaveBeenCalled();
    expect(sig).not.toHaveBeenCalled();
    expect(out).toContain("<<<UNTRUSTED");
    expect(out).toContain("42");
  });

  it("a write function is refused before core is asked anything", async () => {
    const view = vi.spyOn(bridge.contracts, "viewCall");
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call({ address: ADDR, function: "mint", abi_fragment: "function mint(uint256) payable" }), "m1", noop);
    expect(out).toMatch(/not a view function/);
    expect(view).not.toHaveBeenCalled();
    expect(sig).not.toHaveBeenCalled();
  });
});
