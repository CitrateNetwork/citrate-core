// CORE-A1 A1.5 — sim adapter contract. The sim adapter must delegate to the
// bound host's live state (the prototype Store) so the 1:1 UI is preserved.
import { describe, it, expect } from "vitest";
import { createSimBridge, type SimHost } from "./sim";
import type { AppState } from "../shell/state";
import { DEFAULT_APP_CONFIG } from "./types";

function fakeHost(overrides: Partial<AppState> = {}): { host: SimHost; state: AppState } {
  const state = {
    ...DEFAULT_APP_CONFIG,
    liquid: 12.5,
    selfStake: 2500,
    hasGrant: true,
    claimable: 9.41,
    walletAddr: "0xabc",
    activity: [{ id: "a1", kind: "Send", amount: "-1", hash: "0xdead", ts: 1 }],
    node: "validating",
    peers: 23,
    height: 131234,
    syncPct: 100,
    chatBackend: "gateway",
    entitlement: "active",
    tier: "pilot",
    org: null,
    connections: { github: true },
    ...overrides,
  } as unknown as AppState;
  const host: SimHost = {
    getState: () => state,
    patch: (u) => Object.assign(state, u),
  };
  return { host, state };
}

describe("sim adapter contract (delegates to the host Store)", () => {
  it("config.read reflects the live host state", async () => {
    const { host } = fakeHost({ net: "local", telemetry: true } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const cfg = await bridge.config.read();
    expect(cfg.net).toBe("local");
    expect(cfg.telemetry).toBe(true);
    expect(cfg.autolock).toBe(DEFAULT_APP_CONFIG.autolock);
  });

  it("config.write patches the host and returns the merged config", async () => {
    const { host, state } = fakeHost();
    const bridge = createSimBridge(host);
    const merged = await bridge.config.write({ autolock: 15, channel: "beta" });
    expect(merged.autolock).toBe(15);
    expect(merged.channel).toBe("beta");
    // the patch landed on the live state (Store delegation)
    expect((state as unknown as { autolock: number }).autolock).toBe(15);
  });

  it("wallet.balances is computed from live host state (grant + self-stake)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const b = await bridge.wallet.balances();
    expect(b.staked).toBe(32000 + 2500);
    expect(b.liquid).toBe(12.5);
    expect(b.address).toBe("0xabc");
  });

  it("node.status mirrors the live sim node fields", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const st = await bridge.node.status();
    expect(st.state).toBe("validating");
    expect(st.peers).toBe(23);
    expect(st.height).toBe(131234);
  });

  it("keyring status is honestly 'unavailable' on the web shim", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    expect(await bridge.config.keyringStatus()).toBe("unavailable");
  });
});
