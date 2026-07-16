// CORE-D3.C — onS3Pay wiring, the Rule-1 property under test.
//
// In the TAURI path, onS3Pay opens the REAL core-membership checkout popup
// (bridge.membership.checkout) then POLLS /userinfo (bridge.auth.userinfo) until
// the entitlement grant lands (a PAID tier + active entitlement). It must:
//   1. open the checkout popup (invoke the real bridge method),
//   2. set s3="paying",
//   3. NOT settle while /userinfo shows public/none (the Rule-1 property — never
//      fake a settled membership; the tauri path advances ONLY on the real grant),
//   4. settle (s3="settled") + fold the entitlement ONLY once /userinfo flips to a
//      paid+active tier.
// The SIM path keeps the prototype's fake settle so web-dev onboarding still walks.
//
// We mock the bridge + BRIDGE_MODE so the Store's tauri branch runs headless, and
// drive the 5s poll cadence with fake timers.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// The evolving /userinfo the mocked bridge returns. Tests flip `current` to model
// the server-side grant landing.
const userinfoState = {
  current: {
    signedIn: true,
    sub: "usr_x",
    tier: "public",
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

const checkoutMock = vi.fn(async () => {});
const userinfoMock = vi.fn(async () => userinfoState.current);
const statusMock = vi.fn(async () => ({ signedIn: false }));

// Mock the bridge module the Store imports. bindSimHost is a no-op here; the
// membership/auth/custody methods are the only ones onS3Pay + start touch.
vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: { checkout: () => checkoutMock() },
    auth: {
      status: () => statusMock(),
      userinfo: () => userinfoMock(),
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));

// Force the tauri branch of onS3Pay.
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { Store } from "./store";

// Flush pending microtasks (awaited promises inside the poll tick).
async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe("onS3Pay (tauri) — opens checkout, polls userinfo, settles ONLY on the real grant", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    checkoutMock.mockClear();
    userinfoMock.mockClear();
    userinfoState.current = { ...userinfoState.current, tier: "public", expiresAt: "2999-01-01" };
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("opens the checkout popup and enters 'paying'", async () => {
    const store = new Store();
    store.onS3Pay();
    expect(store.state.s3).toBe("paying");
    // The REAL checkout popup command was invoked (not a fake settle).
    await flush();
    expect(checkoutMock).toHaveBeenCalledTimes(1);
  });

  it("does NOT settle while /userinfo shows a public/none tier (the Rule-1 property)", async () => {
    userinfoState.current = { ...userinfoState.current, tier: "public" } as never;
    const store = new Store();
    store.onS3Pay();
    expect(store.state.s3).toBe("paying");

    // Drive several poll ticks with the entitlement STILL not granted.
    for (let i = 0; i < 3; i++) {
      await vi.advanceTimersByTimeAsync(5000);
      await flush();
    }
    // userinfo WAS polled, but s3 NEVER advanced — no fabricated settlement.
    expect(userinfoMock).toHaveBeenCalled();
    expect(store.state.s3).toBe("paying");
    expect(store.state.tier).not.toBe("pilot");
  });

  it("settles ONLY once /userinfo flips to a paid+active tier (the real grant landed)", async () => {
    userinfoState.current = { ...userinfoState.current, tier: "public" } as never;
    const store = new Store();
    store.onS3Pay();

    // Two ticks: still public → still paying.
    await vi.advanceTimersByTimeAsync(5000);
    await flush();
    expect(store.state.s3).toBe("paying");

    // The server-side grant lands: /userinfo now returns a paid tier.
    userinfoState.current = { ...userinfoState.current, tier: "pilot" } as never;
    await vi.advanceTimersByTimeAsync(5000);
    await flush();

    // NOW it settles — and the folded entitlement engine shows the paid tier active.
    expect(store.state.s3).toBe("settled");
    expect(store.state.tier).toBe("pilot");
    expect(store.state.entitlement).toBe("active");
  });

  it("a still-lapsed paid tier does NOT settle (fold-in must be active)", async () => {
    // A paid tier whose expiry is in the PAST folds to free/lapsed (A3-03), so it
    // must NOT settle — the grant is not genuinely active.
    userinfoState.current = { ...userinfoState.current, tier: "pilot", expiresAt: "2000-01-01" } as never;
    const store = new Store();
    store.onS3Pay();
    for (let i = 0; i < 3; i++) {
      await vi.advanceTimersByTimeAsync(5000);
      await flush();
    }
    expect(store.state.s3).toBe("paying");
  });
});
