// K6 invariants P1 and P3 (Codex P2 round 3 on core#363): with an active network profile that
// is not ready, the REAL app modules load (chain.ts, wagmi.ts with the ceremony connector, App),
// the wagmi config builds without throwing, every read is refused without a request being sent,
// and the network gate renders the Update required notice.
//
// Nothing here mocks wagmi or the gate: only the embedded profiles file is replaced, with the
// real file whose `active` names the real not-yet-set `genesis-2026-12` profile.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../../src-tauri/network/profiles.json", async (importOriginal) => {
  const real = (await importOriginal()) as { default: { active: string } };
  return { default: { ...real.default, active: "genesis-2026-12" } };
});

const fetchSpy = vi.fn(async () => {
  throw new Error("no request may be sent while the network is not set");
});

describe("app startup with a network profile that is not ready (K6 P1, P3)", () => {
  beforeEach(() => {
    fetchSpy.mockClear();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("the active profile really is the not-ready one", async () => {
    const { ACTIVE_PROFILE_ID, ACTIVE_PROFILE, isReady } = await import("../network/profile");
    expect(ACTIVE_PROFILE_ID).toBe("genesis-2026-12");
    expect(isReady(ACTIVE_PROFILE)).toBe(false);
    const { NETWORK_READY, citrate } = await import("../chain");
    expect(NETWORK_READY).toBe(false);
    expect(citrate.id).toBe(0);
    expect(citrate.rpcUrls.default.http).toEqual([]);
  });

  it("the real wagmi config (with the ceremony connector) builds without throwing", async () => {
    const { wagmiConfig } = await import("../wagmi");
    expect(wagmiConfig.connectors.map((c) => c.id)).toContain("citrateCeremony");
  });

  it("every read through the wagmi client and the connector is refused and nothing is sent", async () => {
    const { wagmiConfig } = await import("../wagmi");
    const { NetworkNotSetError } = await import("../chain");
    const client = wagmiConfig.getClient();
    await expect(client.request({ method: "eth_blockNumber" })).rejects.toThrow(/no network set/);
    await expect(
      client.request({ method: "eth_getBlockByNumber", params: ["0x0", false] }),
    ).rejects.toThrow(/no network set/);
    const connector = wagmiConfig.connectors.find((c) => c.id === "citrateCeremony")!;
    const provider = (await connector.getProvider()) as {
      request: (a: { method: string; params?: unknown[] }) => Promise<unknown>;
    };
    const err = await provider.request({ method: "eth_blockNumber" }).catch((e: unknown) => e);
    expect(String((err as Error)?.message ?? err)).toMatch(/no network set/);
    expect(new NetworkNotSetError("x").name).toBe("NetworkNotSetError");
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("the gate inside the real WagmiProvider renders the Update required notice", async () => {
    const { wagmiConfig } = await import("../wagmi");
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
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain("Update required");
    expect(html).toContain("does not have the network it should connect to set");
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("the App module imports (the path that threw before the gate could render)", async () => {
    const mod = await import("../App");
    expect(typeof mod.default).toBe("function");
  });
});
