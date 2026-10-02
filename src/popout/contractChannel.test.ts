// HUP-S6.7 — the Contract reader's request channel: the pop-out asks, the main window checks
// every request again and answers; malformed traffic is dropped or refused, never half-run.
import { describe, expect, it, vi } from "vitest";
import type { BridgeTransport } from "./bridge";
import { checkArgs, createContractClient, createContractHost, parseRequest, parseResponse, type ContractOps } from "./contractChannel";

const ADDR = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";

function bus() {
  const listeners = new Map<string, ((p: unknown) => void)[]>();
  const sent: { from: string; to: string; payload: unknown }[] = [];
  const transport = (self: string): BridgeTransport => ({
    async send(to, payload) {
      sent.push({ from: self, to, payload });
      for (const f of listeners.get(to) ?? []) f(JSON.parse(JSON.stringify(payload)));
    },
    async listen(handler) {
      const arr = listeners.get(self) ?? [];
      arr.push(handler);
      listeners.set(self, arr);
      return () => listeners.set(self, (listeners.get(self) ?? []).filter((f) => f !== handler));
    },
  });
  return { transport, sent };
}

function ops(over: Partial<ContractOps> = {}): ContractOps {
  return {
    initial: vi.fn(async () => null),
    source: vi.fn(async () => ({ status: "unverified", isContract: true, codeSize: 10, contractName: null, compilerVersion: null, abi: null, source: null, note: null }) as const),
    codeSize: vi.fn(async () => 42),
    view: vi.fn(async () => "0x01"),
    write: vi.fn(async () => ({ proposed: true }) as const),
    explain: vi.fn(async () => ({ text: "It mints.", by: "local model" })),
    ...over,
  };
}

describe("HUP-S6.7 contract channel validation", () => {
  it("checks every op's arguments", () => {
    expect(checkArgs("source", { address: ADDR })).toEqual({ address: ADDR });
    expect(checkArgs("source", { address: "0x12" })).toBeNull();
    expect(checkArgs("view", { target: "citrate", address: ADDR, calldata: "0x06fdde03" })).not.toBeNull();
    expect(checkArgs("view", { target: "citrate", address: ADDR, calldata: "0x06" })).toBeNull();
    expect(checkArgs("view", { target: "x".repeat(201), address: ADDR, calldata: "0x06fdde03" })).toBeNull();
    expect(checkArgs("write", { address: ADDR, calldata: "0xa0712d68", valueWei: "0", label: "mint" })).not.toBeNull();
    expect(checkArgs("write", { address: ADDR, calldata: "0xa0712d68", valueWei: "-1", label: "mint" })).toBeNull();
    expect(checkArgs("write", { address: ADDR, calldata: "0xa0712d68", valueWei: "1e18", label: "mint" })).toBeNull();
    expect(checkArgs("explain", { prompt: "" })).toBeNull();
    expect(checkArgs("explain", { prompt: "x".repeat(40_001) })).toBeNull();
    expect(checkArgs("explain", "prompt")).toBeNull();
    expect(checkArgs("initial", {})).toEqual({});
  });

  it("drops messages that are not requests or responses", () => {
    expect(parseRequest({ v: 1, type: "contract.request", id: "a1", op: "sign", args: {} })).toBeNull();
    expect(parseRequest({ v: 2, type: "contract.request", id: "a1", op: "view", args: {} })).toBeNull();
    expect(parseRequest({ v: 1, type: "contract.request", id: "../x", op: "view", args: {} })).toBeNull();
    expect(parseRequest({ v: 1, type: "monitor.stop" })).toBeNull();
    expect(parseResponse({ v: 1, type: "contract.response", id: "a1", ok: "yes" })).toBeNull();
  });
});

describe("HUP-S6.7 contract channel round trips", () => {
  it("a request reaches the main window's op and the answer comes back", async () => {
    const b = bus();
    const o = ops();
    const host = await createContractHost(b.transport("main"), o);
    const client = await createContractClient(b.transport("popout-contract"));
    await expect(client.call("codeSize", { target: "citrate", address: ADDR })).resolves.toBe(42);
    expect(o.codeSize).toHaveBeenCalledWith({ target: "citrate", address: ADDR });
    await expect(client.call("view", { target: "citrate", address: ADDR, calldata: "0x06fdde03" })).resolves.toBe("0x01");
    host.close();
    client.close();
  });

  it("an op's failure comes back as the error message", async () => {
    const b = bus();
    const host = await createContractHost(b.transport("main"), ops({ view: vi.fn(async () => { throw new Error("the node refused: execution reverted"); }) }));
    const client = await createContractClient(b.transport("popout-contract"));
    await expect(client.call("view", { target: "citrate", address: ADDR, calldata: "0x06fdde03" })).rejects.toThrow(/execution reverted/);
    host.close();
    client.close();
  });

  it("the main window refuses malformed arguments without running the op", async () => {
    const b = bus();
    const o = ops();
    const host = await createContractHost(b.transport("main"), o);
    const client = await createContractClient(b.transport("popout-contract"));
    await expect(client.call("write", { address: ADDR, calldata: "0xzz", valueWei: "0", label: "x" })).rejects.toThrow(/malformed/);
    expect(o.write).not.toHaveBeenCalled();
    host.close();
    client.close();
  });

  it("the main window can point the reader at an address", async () => {
    const b = bus();
    const host = await createContractHost(b.transport("main"), ops());
    const seen: string[] = [];
    const client = await createContractClient(b.transport("popout-contract"), (a, t) => seen.push(`${a}@${t}`));
    await host.focus(ADDR);
    expect(seen).toEqual([`${ADDR}@citrate`]);
    host.close();
    client.close();
  });

  it("closing the reader rejects what is still waiting; an unanswered call times out", async () => {
    const b = bus();
    const quiet = await createContractClient(b.transport("popout-contract"), undefined, 20);
    await expect(quiet.call("source", { address: ADDR })).rejects.toThrow(/did not answer/);
    const p = quiet.call("source", { address: ADDR });
    quiet.close();
    await expect(p).rejects.toThrow(/closed/);
  });
});
