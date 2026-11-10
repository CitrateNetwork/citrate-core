// K6 (RC52 D-2): a build whose active profile is the unfilled r1006 starts, sends nothing, and
// shows Update required. Only the embedded profiles file is replaced (the real file, r1006 active,
// with its three reroll values set back to null as before the RC52 fill); wagmi and the gate are
// the real modules.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../../src-tauri/network/profiles.json", async (importOriginal) => {
  const real = (await importOriginal()) as { default: { active: string; profiles: Record<string, Record<string, unknown>> } };
  const unset = { genesisHash: null, addressBookSha256: null, minChainRev: null };
  const pending = { genesisHash: "set at the reroll", addressBookSha256: "set at the reroll", minChainRev: "set at the reroll" };
  const r1006 = { ...real.default.profiles.r1006, ...unset, pending };
  return { default: { ...real.default, active: "r1006", profiles: { ...real.default.profiles, r1006 } } };
});

const fetchSpy = vi.fn(async () => {
  throw new Error("no request may be sent while the network is not set");
});

describe("app startup on the unfilled r1006 profile (K6)", () => {
  beforeEach(() => {
    fetchSpy.mockClear();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("the active profile is r1006 and it is not ready", async () => {
    const { ACTIVE_PROFILE_ID, ACTIVE_PROFILE, isReady, missingValues } = await import("../network/profile");
    expect(ACTIVE_PROFILE_ID).toBe("r1006");
    expect(isReady(ACTIVE_PROFILE)).toBe(false);
    expect(missingValues(ACTIVE_PROFILE!).map((m) => m.field)).toEqual([
      "genesisHash",
      "addressBookSha256",
      "minChainRev",
    ]);
    const { NETWORK_READY, citrate } = await import("../chain");
    expect(NETWORK_READY).toBe(false);
    expect(citrate.id).toBe(0);
  });

  it("reads are refused with nothing sent, and the gate renders Update required", async () => {
    const { wagmiConfig } = await import("../wagmi");
    await expect(wagmiConfig.getClient().request({ method: "eth_chainId" })).rejects.toThrow(/no network set/);
    const { WagmiProvider } = await import("wagmi");
    const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");
    const { NetworkCompatGate } = await import("./NetworkCompatGate");
    const html = renderToStaticMarkup(
      <WagmiProvider config={wagmiConfig} reconnectOnMount={false}>
        <QueryClientProvider client={new QueryClient()}>
          <NetworkCompatGate />
        </QueryClientProvider>
      </WagmiProvider>,
    );
    expect(html).toContain("Update required");
    expect(html).toContain("does not have the network it should connect to set");
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
