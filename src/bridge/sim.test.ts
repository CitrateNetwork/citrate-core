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

  it("wallet.balances reports self-stake only (grant is layered by the store/UI)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const b = await bridge.wallet.balances();
    // `staked` = the SELF-stake (the chain fact); the vaulted 32k grant is an
    // entitlement the store adds via hasGrant, not part of this field.
    expect(b.staked).toBe(2500);
    expect(b.liquid).toBe(12.5);
    expect(b.address).toBe("0xabc");
  });

  // CORE WP2 — the sim withdraw path is HONEST: the web shim reaches no chain, so
  // requestWithdrawal/claimWithdrawal throw Unavailable (never a fabricated settle)
  // and pendingWithdrawals returns an empty queue (never fabricated rows — Rule 1).
  it("wallet.requestWithdrawal + claimWithdrawal are honestly Unavailable in sim", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const { isUnavailable } = await import("./types");
    await expect(bridge.wallet.requestWithdrawal("1000000000000000000")).rejects.toSatisfy((e: unknown) => isUnavailable(e));
    await expect(bridge.wallet.claimWithdrawal("1")).rejects.toSatisfy((e: unknown) => isUnavailable(e));
  });

  it("wallet.pendingWithdrawals returns an honest empty queue in sim (no chain)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    expect(await bridge.wallet.pendingWithdrawals()).toEqual([]);
  });

  // CORE item 4 — in sim the web shim reaches no indexer, so wallet.activity
  // echoes the host's prototype `s().activity` (the seed), never a fabricated
  // history. (In a Tauri build the adapter invokes the real CitrateScan read.)
  it("wallet.activity echoes the host prototype activity in sim (no indexer)", async () => {
    const { host, state } = fakeHost();
    const bridge = createSimBridge(host);
    const rows = await bridge.wallet.activity();
    expect(rows).toEqual(state.activity);
    expect(rows[0].hash).toBe("0xdead");
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

  // CORE-C2 — sim earnings: the prototype claimable rendered in the SAME wei
  // shape the real eth_call returns; NO fabricated per-source breakdown field.
  it("agent.earnings returns the single claimable (wei) + contract, no breakdown", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const e = await bridge.agent.earnings();
    // 9.41 SALT → wei string.
    expect(e.claimableWei).toBe("9410000000000000000");
    expect(e.walletAddress).toBe("0xabc");
    expect(e.contract).toBe("0xcdd2477387279c7d44a1053f44db5dac0fd8faef");
    expect(Object.keys(e).sort()).toEqual(["claimableWei", "contract", "walletAddress"]);
  });

  // CORE-C2-F-1 — the sim Claim button is HONEST: it mints a SIM ceremony (a
  // legible claimRewards() Call), NEVER a settled claim. The prototype UI can
  // render the ceremony, but approving it routes through sim signing.broadcast,
  // which THROWS (no key, no chain) — so no balance is ever fabricated. The old
  // faked `apply` (liquid += amt, "Claimed — balance updated from chain") is GONE.
  it("agent.claim mints a sim ceremony (no faked settlement); approving it honestly cannot broadcast", async () => {
    const { host, state } = fakeHost({ claimable: 9.41 } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const liquidBefore = (state as unknown as { liquid: number }).liquid;

    const res = await bridge.agent.claim();
    expect(res.kind).toBe("ceremony");
    if (res.kind !== "ceremony") throw new Error("expected a sim ceremony");
    // The sim shim does not decode tx calldata (honest — no real decoder), so the
    // claim ceremony is raw-ack gated "Unrecognized", NOT a fabricated settlement.
    // The origin is the verbatim agent origin (anti-spoof), and there is NO sig.
    expect(res.view.origin).toBe("agent:node-agent");
    expect("sigHex" in res.view).toBe(false);
    // Claiming did NOT mutate the balance (no fabricated settlement — Rule 1).
    expect((state as unknown as { liquid: number }).liquid).toBe(liquidBefore);
    // And the sim signer HONESTLY cannot settle it (no key/chain) — it throws
    // rather than fabricate a tx (raw-ack passed so the throw is the chain-reach
    // refusal, not the ack gate).
    await expect(bridge.signing.broadcast(res.view.id, true)).rejects.toBeTruthy();
    // The balance STILL did not change after the (failed) settle attempt.
    expect((state as unknown as { liquid: number }).liquid).toBe(liquidBefore);
  });

  // CORE-C2-F-1 — a zero prototype claimable → honest "nothing to claim", no
  // ceremony minted, no faked balance.
  it("agent.claim with 0 claimable returns honest 'nothing', mints no ceremony", async () => {
    const { host } = fakeHost({ claimable: 0 } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const res = await bridge.agent.claim();
    expect(res.kind).toBe("nothing");
    if (res.kind !== "nothing") throw new Error("expected nothing-to-claim");
    expect(res.claimableWei).toBe("0");
  });

  // CORE-D3.C — the sim membership.checkout is a guarded no-op: it opens no real
  // popup and settles nothing (the web-dev fake settle lives in the Store's
  // onS3Pay sim branch, not here). It resolves so the sim S3 flow proceeds.
  it("membership.checkout is a guarded no-op in sim (opens no real popup)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    await expect(bridge.membership.checkout()).resolves.toBeUndefined();
  });

  it("membership.entitlement still reflects the live host entitlement/tier", async () => {
    const { host } = fakeHost({ entitlement: "active", tier: "pilot" } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const e = await bridge.membership.entitlement();
    expect(e.status).toBe("active");
    expect(e.tier).toBe("pilot");
  });

  // BC-1.3 — the sim grantStatus is an HONEST stand-in derived from the persona/
  // AppState (NEVER a fabricated real 40204 read): granted (32,000-SALT stake + SBT)
  // ONLY for a paid+active sim member; 0 stake / no SBT otherwise. This lets the sim
  // S5 settle from grantStatus (the Rule-1 shape), not a blind timer.
  it("membership.grantStatus reads granted for a paid+active sim member", async () => {
    const { host } = fakeHost({ entitlement: "active", tier: "pilot" } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const g = await bridge.membership.grantStatus("0xabc");
    expect(g.attributedStakeWei).toBe((32000n * 10n ** 18n).toString());
    expect(g.hasSbt).toBe(true);
  });

  it("membership.grantStatus reads NOT-granted (0 / false) for an unpaid sim member", async () => {
    const { host } = fakeHost({ entitlement: "active", tier: "free" } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const g = await bridge.membership.grantStatus("0xabc");
    expect(g.attributedStakeWei).toBe("0");
    expect(g.attributedSharesWei).toBe("0");
    expect(g.hasSbt).toBe(false);
  });

  it("membership.grantStatus reads NOT-granted for a paid but LAPSED sim member", async () => {
    const { host } = fakeHost({ entitlement: "lapsed", tier: "pilot" } as Partial<AppState>);
    const bridge = createSimBridge(host);
    const g = await bridge.membership.grantStatus("0xabc");
    expect(g.hasSbt).toBe(false);
  });
});

// CORE-AI1 (@rule8) — sim chat: the web preview has NO OS keyring and reaches no
// provider, so real inference cannot happen (Rule 1). providerStatus reports
// nothing configured; setProvider/clearProvider/infer throw Unavailable; the
// backend is the honest built-in demo agent (never a gateway it does not call).
describe("sim adapter — chat AI provider domain is honestly unavailable (AI1)", () => {
  it("providerStatus returns an empty list (no keyring on the web shim)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    expect(await bridge.chat.providerStatus()).toEqual([]);
  });

  it("setProvider / clearProvider / infer are honestly Unavailable in sim", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const { isUnavailable } = await import("./types");
    await expect(bridge.chat.setProvider("openai", "https://api.openai.com/v1", "gpt-4o", "sk-x")).rejects.toSatisfy(
      (e: unknown) => isUnavailable(e),
    );
    await expect(bridge.chat.clearProvider("openai")).rejects.toSatisfy((e: unknown) => isUnavailable(e));
    await expect(bridge.chat.infer("openai", "[]", "{}")).rejects.toSatisfy((e: unknown) => isUnavailable(e));
  });

  it("backend is the honest built-in demo agent (Rule 1 — no gateway label)", async () => {
    const { host } = fakeHost();
    const bridge = createSimBridge(host);
    const b = await bridge.chat.backend();
    expect(b.kind).toBe("demo");
    expect(b.label).toBe("built-in demo agent");
    expect(b.label).not.toContain("infer.citrate.ai");
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
