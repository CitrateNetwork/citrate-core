// CORE-A1 / CORE-A2 — the contract lookup and decoding tools through the real store entry point,
// with the bridge spied at its boundary. All read-only: no approval gate, no ceremony, no write.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import { READ_ONLY_AGENT_TOOLS, type ToolCall } from "../agent/harness";

const call = (name: string, args: Record<string, unknown>): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};
const ADDR = "0x" + "cd".repeat(20);
const TOOLS = ["address_book_lookup", "get_code", "identify_contract", "decode_calldata", "decode_log", "explain_tx"];

beforeEach(() => store.setState({ chatMsgs: [] }));
afterEach(() => vi.restoreAllMocks());

describe("CORE-A1 / CORE-A2 contract tools", () => {
  it("are on the reviewed read-only list", () => {
    for (const t of TOOLS) expect(READ_ONLY_AGENT_TOOLS.has(t), t).toBe(true);
  });

  it("address_book_lookup calls core with the query and asks for no approval", async () => {
    const spy = vi.spyOn(bridge.contracts, "addressBookLookup").mockResolvedValue({
      query: "wrapped salt", kind: "name", chainId: 40204, genesisHash: "0x" + "a3".repeat(32),
      book: [{ name: "WrappedSALT", address: ADDR, section: "contracts", abiContract: "src/WrappedSALT.sol:WrappedSALT", proxy: false }],
      registries: [], note: "1 address book entry matched.",
    });
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call("address_book_lookup", { query: "wrapped salt" }), "m1", noop);
    expect(spy).toHaveBeenCalledWith("wrapped salt");
    expect(sig).not.toHaveBeenCalled();
    expect(out).toContain(`WrappedSALT = ${ADDR}`);
  });

  it("get_code and identify_contract reach core; a failure is reported as unknown", async () => {
    vi.spyOn(bridge.contracts, "getCode").mockRejectedValue(new Error("the RPC could not be reached"));
    const out = await store.handleTool(call("get_code", { address: ADDR }), "m1", noop);
    expect(out).toMatch(/could not be read \(the RPC could not be reached\)/);
    expect(out).toMatch(/unknown/);
    vi.spyOn(bridge.contracts, "identify").mockRejectedValue(new Error("down"));
    expect(await store.handleTool(call("identify_contract", { address: ADDR }), "m1", noop)).toMatch(/stays unknown/);
  });

  it("decode_calldata uses core's ABI resolution and signature subset", async () => {
    const resolve = vi.spyOn(bridge.contracts, "abiResolve").mockResolvedValue({
      address: ADDR, source: "none", abiAddress: null, contractName: null, abi: null, proxy: null, bookNames: [], note: "none",
    });
    const sigs = vi.spyOn(bridge.contracts, "signatureLookup").mockResolvedValue({ functions: { "0xdeadbeef": [] }, events: {}, errors: {} });
    const out = await store.handleTool(call("decode_calldata", { data: "0xdeadbeef", address: ADDR }), "m1", noop);
    expect(resolve).toHaveBeenCalledWith(ADDR);
    expect(sigs).toHaveBeenCalledWith(["0xdeadbeef"], []);
    expect(out).toMatch(/unknown selector/);
  });

  it("explain_tx fetches the tx through core", async () => {
    const spy = vi.spyOn(bridge.contracts, "txFetch").mockResolvedValue({
      hash: "0x" + "ab".repeat(32), found: false, from: null, to: null, valueWei: null, input: null, inputTruncated: false,
      nonce: null, blockNumber: null, status: "pending", gasUsed: null, contractAddress: null, logs: [], logsTruncated: false, names: {},
    });
    const out = await store.handleTool(call("explain_tx", { hash: "0x" + "ab".repeat(32) }), "m1", noop);
    expect(spy).toHaveBeenCalled();
    expect(out).toMatch(/has no transaction/);
  });
});
