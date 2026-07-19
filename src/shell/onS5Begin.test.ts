// BC-1.3 — onS5Begin wiring, the Rule-1 property under test.
//
// S5 (grant + stake ceremony) must settle ONLY when the 32,000-SALT membership
// grant is GENUINELY on-chain. In the TAURI path, onS5Begin runs the verify
// animation, then POLLS the REAL on-chain grant status
// (bridge.membership.grantStatus(identity.wallet)) + the /userinfo entitlement,
// and advances to "settled" ONLY when grant + SBT + entitlement are all real:
//   NEGATIVE CONTROL: grantStatus returns attributedStake=0, hasSbt=false →
//     S5 must NEVER reach "settled" (stays "settling") and hasGrant/hasSbt stay
//     false — the guard that proves S5 no longer fabricates settlement.
//   POSITIVE: grantStatus returns granted (stake >= requirement) + sbt=1 AND the
//     entitlement is paid+active → S5 reaches "settled" with hasGrant/hasSbt true.
//
// We mock the bridge + BRIDGE_MODE so the Store's tauri branch runs headless, and
// drive the poll cadence with fake timers (mirrors onS3Pay.test.ts).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const REQUIREMENT_WEI = (32000n * 10n ** 18n).toString();

// The evolving on-chain grant the mocked bridge returns. Tests flip `current` to
// model the grant landing on-chain.
const grantState = {
  current: { attributedStakeWei: "0", attributedSharesWei: "0", hasSbt: false } as {
    attributedStakeWei: string;
    attributedSharesWei: string;
    hasSbt: boolean;
  },
};

// The /userinfo the mocked bridge returns — a paid+active member (the entitlement
// leg). The negative control keeps this paid+active to PROVE the guard is the chain
// read, not the entitlement (S5 must still not settle without the real grant).
const userinfoState = {
  current: {
    signedIn: true,
    sub: "usr_x",
    tier: "pilot",
    org: null,
    role: "member",
    kycStatus: "verified",
    walletAddr: "0xabc",
    expiresAt: "2999-01-01",
    email: "d@example.com",
  } as {
    signedIn: boolean;
    tier: string | null;
    org: string | null;
    role: string | null;
    kycStatus: string | null;
    walletAddr: string | null;
    expiresAt: string | null;
    email: string | null;
    sub: string;
  },
};

const grantStatusMock = vi.fn(async () => grantState.current);
const userinfoMock = vi.fn(async () => userinfoState.current);
const statusMock = vi.fn(async () => ({ signedIn: false }));

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: { grantStatus: (addr: string) => grantStatusMock(addr) },
    auth: {
      status: () => statusMock(),
      userinfo: () => userinfoMock(),
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { Store } from "./store";

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

// Sign the store in with a paid+active claim + a wallet address (so the poll has a
// member address to read and the entitlement leg is satisfied).
async function signInPaid(store: Store): Promise<void> {
  // applyAuthStatus is private; drive it via the real authUserinfo path which the
  // store calls during the poll anyway. Seed the wallet + tier by folding userinfo.
  await store.authUserinfo();
  await flush();
}

describe("onS5Begin (tauri) — settles S5 ONLY on the REAL on-chain grant", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    grantStatusMock.mockClear();
    userinfoMock.mockClear();
    grantState.current = { attributedStakeWei: "0", attributedSharesWei: "0", hasSbt: false };
    userinfoState.current = { ...userinfoState.current, tier: "pilot", expiresAt: "2999-01-01" };
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("NEGATIVE CONTROL: with grantStatus 0/false, S5 never settles and hasGrant/hasSbt stay false", async () => {
    const store = new Store();
    await signInPaid(store);
    // Grant is NOT on-chain (0 stake, no SBT) — even though the entitlement is
    // paid+active. S5 must not settle.
    grantState.current = { attributedStakeWei: "0", attributedSharesWei: "0", hasSbt: false };

    store.onS5Begin();
    // Run through the verify animation into settling.
    await vi.advanceTimersByTimeAsync(3400);
    await flush();
    expect(store.state.s5).toBe("settling");

    // Drive several poll ticks with the grant STILL not on-chain.
    for (let i = 0; i < 6; i++) {
      await vi.advanceTimersByTimeAsync(5000);
      await flush();
    }
    // The chain WAS read, but S5 NEVER advanced — no fabricated settlement.
    expect(grantStatusMock).toHaveBeenCalled();
    expect(store.state.s5).toBe("settling");
    expect(store.state.hasGrant).toBe(false);
    expect(store.state.hasSbt).toBe(false);
  });

  it("POSITIVE: grantStatus granted + sbt + paid entitlement → S5 settles with hasGrant/hasSbt true", async () => {
    const store = new Store();
    await signInPaid(store);
    grantState.current = { attributedStakeWei: "0", attributedSharesWei: "0", hasSbt: false };

    store.onS5Begin();
    await vi.advanceTimersByTimeAsync(3400);
    await flush();

    // One tick: still not on-chain → still settling.
    await vi.advanceTimersByTimeAsync(5000);
    await flush();
    expect(store.state.s5).toBe("settling");
    expect(store.state.hasGrant).toBe(false);

    // The grant lands on-chain: attributedStake >= requirement + SBT minted.
    grantState.current = { attributedStakeWei: REQUIREMENT_WEI, attributedSharesWei: REQUIREMENT_WEI, hasSbt: true };
    await vi.advanceTimersByTimeAsync(5000);
    await flush();

    // NOW it settles — hasGrant/hasSbt set ONLY from the real read.
    expect(store.state.s5).toBe("settled");
    expect(store.state.hasGrant).toBe(true);
    expect(store.state.hasSbt).toBe(true);
  });

  it("does NOT settle if the grant is on-chain but the entitlement is NOT paid+active", async () => {
    const store = new Store();
    // Entitlement leg fails: a free tier (grantStatus derives NOT paid → but even if
    // the chain grant were present, the entitlement leg must gate settlement).
    userinfoState.current = { ...userinfoState.current, tier: "public" } as never;
    await signInPaid(store);
    // Chain grant IS present.
    grantState.current = { attributedStakeWei: REQUIREMENT_WEI, attributedSharesWei: REQUIREMENT_WEI, hasSbt: true };

    store.onS5Begin();
    await vi.advanceTimersByTimeAsync(3400);
    await flush();
    for (let i = 0; i < 4; i++) {
      await vi.advanceTimersByTimeAsync(5000);
      await flush();
    }
    // The entitlement leg is not paid+active → S5 stays settling (never fabricated).
    expect(store.state.s5).toBe("settling");
  });
});
