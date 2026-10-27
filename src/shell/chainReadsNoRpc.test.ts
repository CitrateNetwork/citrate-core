// The chain reads behind the validator and producer figures (C1-V) must not touch the
// transport while a module is being imported. A build whose chain has no RPC url (an incomplete
// network profile, K6) has to load every module, so the app can show its "not ready" state; the
// reads it cannot make are refused one by one, with a reason, and nothing is sent.
//
// Only `../chain` is replaced (a chain with an empty RPC list); chainTruth and the store are real.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../chain", async () => {
  const { defineChain } = await import("viem");
  return {
    citrate: defineChain({
      id: 0,
      name: "Citrate",
      nativeCurrency: { name: "SALT", symbol: "SALT", decimals: 18 },
      rpcUrls: { default: { http: [] } },
    }),
  };
});

const fetchSpy = vi.fn(async () => {
  throw new Error("no request may be sent when the chain has no RPC url");
});

describe("chain reads with no RPC url (import never constructs a transport)", () => {
  beforeEach(() => {
    fetchSpy.mockClear();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("createChainReads() does not throw when the chain has no RPC url", async () => {
    const { createChainReads } = await import("./chainTruth");
    expect(() => createChainReads()).not.toThrow();
  });

  it("the store module imports (its singleton builds the chain reads)", async () => {
    const loaded = import("./store").then((m) => m.store.chainReads);
    await expect(loaded).resolves.toBeDefined();
  });

  it("every read is refused with a reason, and nothing is sent", async () => {
    const { createChainReads } = await import("./chainTruth");
    let reads: ReturnType<typeof createChainReads> | undefined;
    expect(() => (reads = createChainReads())).not.toThrow();
    if (!reads) return;
    const key = ("0x" + "11".repeat(32)) as `0x${string}`;
    const results = await Promise.allSettled([
      reads.validatorActivation(key),
      reads.minStakeWei(),
      reads.blockSeconds(),
      reads.producerSample(),
    ]);
    for (const r of results) {
      expect(r.status).toBe("rejected");
      expect(String((r as PromiseRejectedResult).reason?.message)).toMatch(/no RPC url is set/);
    }
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
