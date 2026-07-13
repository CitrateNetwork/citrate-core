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

// CORE-A2 A2.5 — sim custody: simulates lock/unlock UI STATE ONLY. It holds no
// real secret and stores nothing; the real vault lives in the Tauri/Rust path.
describe("sim adapter — custody UI state only (no real secret, stores nothing)", () => {
  it("status reflects sim lock state and reads autolock from host config", async () => {
    const { host } = fakeHost({ autolock: 15 } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const st = await bridge.custody.status();
    expect(st.unlocked).toBe(false);
    expect(st.autolockMins).toBe(15);
    // No OS keyring on the web shim — honest, matches config.keyringStatus.
    expect(st.keyringStatus).toBe("unavailable");
  });

  it("unlock then lock flips UI state; no secret is stored anywhere", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    await bridge.custody.unlock("anything");
    expect((await bridge.custody.status()).unlocked).toBe(true);
    await bridge.custody.lock();
    expect((await bridge.custody.status()).unlocked).toBe(false);
  });

  it("listSlots returns no real slots in sim (metadata only, never bytes)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    expect(await bridge.custody.listSlots()).toEqual([]);
  });
});

// CORE-A3 A3.3 — sim auth derives the claim-derived AuthStatus from the live
// persona/AppState (the entitlement engine input) and carries NO token.
describe("sim adapter — auth persona claim (A3.3 sim half)", () => {
  it("status is signed-out before S1 completes (onboarding gate)", async () => {
    const { host } = fakeHost({ stage: "s1", s1: "idle", persona: "p1" } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const st = await bridge.auth.status();
    expect(st.signedIn).toBe(false);
  });

  it("status reflects the persona entitlement once signed in; never a token", async () => {
    const { host } = fakeHost({ stage: "done", persona: "p4", tier: "enterprise", org: "BA-7" } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const st = await bridge.auth.status();
    expect(st.signedIn).toBe(true);
    expect(st.tier).toBe("enterprise");
    expect(st.org).toBe("BA-7");
    expect(st.role).toBe("org-seat");
    // No token material anywhere in the sim status either (ADV-8).
    expect(JSON.stringify(st)).not.toContain("token");
  });

  it("logout + kycStart are inert no-ops in the shim (no real token to leak)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    await expect(bridge.auth.logout()).resolves.toBeUndefined();
    await expect(bridge.auth.kycStart()).resolves.toBeUndefined();
  });
});

// CORE-B1.2 — sim signing is the STATE MACHINE ONLY (no real key, no real sig).
// The dev shim models request→approve→reject (single-use, raw-ack gate, explicit
// id) so the dev UI behaves like the real ceremony, and returns a CLEARLY-FAKE
// signature that is never presented as live. The real vault-gated signer is the
// Tauri/Rust path (B1.2 cargo tests).
describe("sim adapter — signing ceremony state machine (dev shim)", () => {
  it("request returns a decoded PENDING view; approve consumes it (single-use)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const view = await bridge.signing.request({ origin: "https://app.citrate.ai", kind: "personal_sign", chainId: 40204, raw: "0x68656c6c6f" });
    expect(view.origin).toBe("https://app.citrate.ai"); // TRUE origin, verbatim
    expect(view.decoded.action).toContain("Sign message");
    expect(view.requiresRawAck).toBe(false);
    const sig = await bridge.signing.approve(view.id, false);
    // Honest: the shim holds no key — the sim signature is clearly labeled, never
    // a real one.
    expect(sig.sigHex).toContain("sim");
    // Single-use: a replay errors.
    await expect(bridge.signing.approve(view.id, false)).rejects.toBeTruthy();
  });

  it("undecodable calldata is raw-ack gated in the shim too", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const view = await bridge.signing.request({ origin: "agent", kind: "transaction", chainId: 40204, raw: "0x02f86b" });
    expect(view.requiresRawAck).toBe(true);
    await expect(bridge.signing.approve(view.id, false)).rejects.toBeTruthy();
    const view2 = await bridge.signing.request({ origin: "agent", kind: "transaction", chainId: 40204, raw: "0x02f86b" });
    await expect(bridge.signing.approve(view2.id, true)).resolves.toBeTruthy();
  });

  it("reject consumes without a signature; unknown id errors", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const view = await bridge.signing.request({ origin: "o", kind: "personal_sign", chainId: 40204, raw: "0x68656c6c6f" });
    await expect(bridge.signing.reject(view.id)).resolves.toBeUndefined();
    await expect(bridge.signing.reject(view.id)).rejects.toBeTruthy();
  });
});
