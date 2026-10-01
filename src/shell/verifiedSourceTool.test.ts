// HUP-S4.3 — the get_verified_source agent tool through the real store entry point, with the
// bridge spied at its boundary. Read-only: no approval gate, no ceremony, no write.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ToolCall } from "../agent/harness";

const call = (args: Record<string, unknown>): ToolCall => ({ id: "c1", name: "get_verified_source", arguments: JSON.stringify(args) });
const noop = () => {};
const ADDR = "0x" + "cd".repeat(20);

beforeEach(() => store.setState({ chatMsgs: [] }));
afterEach(() => vi.restoreAllMocks());

describe("get_verified_source", () => {
  it("calls core with the address and returns the fenced verified source", async () => {
    const spy = vi.spyOn(bridge.contracts, "verifiedSource").mockResolvedValue({
      address: ADDR, status: "verified", verified: true, matchType: "full", contractName: "A.sol:A",
      compilerVersion: "v0.8.26", sourceHash: null, source: "contract A {}", sourceTruncated: false,
      abi: [], verifiedAt: null, note: "n",
    });
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call({ address: ADDR }), "m1", noop);
    expect(spy).toHaveBeenCalledWith(ADDR);
    expect(sig).not.toHaveBeenCalled();
    expect(out).toContain("<<<UNTRUSTED");
    expect(out).toContain("contract A {}");
  });

  it("a malformed address is refused without calling core", async () => {
    const spy = vi.spyOn(bridge.contracts, "verifiedSource");
    const out = await store.handleTool(call({ address: "0x12" }), "m1", noop);
    expect(spy).not.toHaveBeenCalled();
    expect(out).toMatch(/address/i);
  });

  it("a failed lookup is reported honestly, never as 'not verified'", async () => {
    vi.spyOn(bridge.contracts, "verifiedSource").mockRejectedValue(new Error("CitrateScan answered HTTP 502"));
    const out = await store.handleTool(call({ address: ADDR }), "m1", noop);
    expect(out).toMatch(/unavailable/i);
    expect(out).toContain("502");
    expect(out).not.toMatch(/not verified/i);
  });
});
