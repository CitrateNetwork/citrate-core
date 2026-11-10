// K6 invariants P1 and P3: the read transport for a profile that is not ready is built without
// throwing and refuses every request without sending it; a ready profile keeps viem http().
import { describe, it, expect, vi, afterEach } from "vitest";
import { defineChain, http } from "viem";
import { readTransport, NetworkNotSetError, NETWORK_READY, citrate } from "./chain";

const noRpcChain = defineChain({
  id: 0,
  name: "Citrate",
  nativeCurrency: { name: "SALT", symbol: "SALT", decimals: 18 },
  rpcUrls: { default: { http: [] } },
});

describe("readTransport (K6)", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("the bug it replaces: viem http() throws at setup on a chain with no RPC url", () => {
    expect(() => http()({ chain: noRpcChain })).toThrow();
  });

  it("not ready: builds without throwing and refuses every request, sending nothing", async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const t = readTransport(undefined, false)({ chain: noRpcChain });
    for (const method of ["eth_blockNumber", "eth_chainId", "eth_getBlockByNumber", "eth_sendRawTransaction"]) {
      const err = await t.request({ method } as never).catch((e: unknown) => e);
      expect(err).toBeInstanceOf(Error);
      expect(String((err as Error).message)).toContain(`so ${method} was not sent`);
    }
    // An explicit url does not bypass it either (Settings' RPC probe).
    const t2 = readTransport("https://rpc.citrate.ai", false)({ chain: noRpcChain });
    const err2 = await t2.request({ method: "eth_blockNumber" } as never).catch((e: unknown) => e);
    // viem wraps a custom transport's error; the cause is ours.
    expect((err2 as { cause?: unknown }).cause).toBeInstanceOf(NetworkNotSetError);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("ready (the active build profile): viem http transport over the profile's RPC", () => {
    expect(NETWORK_READY).toBe(true);
    const t = readTransport()({ chain: citrate });
    expect(t.config.type).toBe("http");
    expect(t.value?.url).toBe(citrate.rpcUrls.default.http[0]);
  });
});
