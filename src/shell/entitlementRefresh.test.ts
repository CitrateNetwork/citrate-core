// v0.5.0-rc.5 FIX 1 — the entitlement refreshes after a browser checkout, without a restart.
//
// Reported 2026-10-13 (owner + DGX): server side green (paid 13:32:40, granted 13:32:53,
// SBT balanceOf=1, entitlement commercial), yet the app showed "Membership lapsed" until
// it was restarted. `s.entitlement` is set only by `applyAuthStatus`, and `/userinfo` was
// re-read only on launch, the S2 KYC poll and the S3/S5 onboarding polls. A member who
// paid from Settings (renew) or whose S3 poll had settled on the chain before the claim
// caught up never saw the new claim.
//
// Under test:
//   - a checkout from ANY entry point starts ONE bounded entitlement poll (~10 s, stops on
//     an active entitlement or after ~15 min, cancelled on sign-out);
//   - window focus / visibilitychange → visible re-reads /userinfo, throttled to one per
//     30 s, only while signed in;
//   - chain evidence of a grant in flight with no claim yet shows "activating", never the
//     red lapsed banner, and never "active" without the claim or the full chain proof.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

type GrantShape = { attributedStakeWei: string; hasSbt: boolean };
const NONE: GrantShape = { attributedStakeWei: "0", hasSbt: false };
const FULL: GrantShape = { attributedStakeWei: (32000n * 10n ** 18n).toString(), hasSbt: true };
const SBT_ONLY: GrantShape = { attributedStakeWei: "0", hasSbt: true };
const grantState = { current: NONE as GrantShape };

const NO_TIER = {
  signedIn: true,
  sub: "usr_x",
  tier: null as string | null,
  org: null,
  role: "member",
  kycStatus: null,
  walletAddr: "0xabc",
  expiresAt: null as string | null,
  email: "m@example.com",
};
const COMMERCIAL = { ...NO_TIER, tier: "commercial", expiresAt: "2999-01-01" };
const userinfoState = { current: NO_TIER as typeof NO_TIER };

const checkoutMock = vi.fn(async (_hint?: string) => {});
const grantStatusMock = vi.fn(async (_a: string) => grantState.current);
const userinfoMock = vi.fn(async () => userinfoState.current);
const logoutMock = vi.fn(async () => {});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: {
      checkout: (h?: string) => checkoutMock(h),
      grantStatus: (addr: string) => grantStatusMock(addr),
    },
    auth: {
      status: async () => ({ signedIn: false }),
      userinfo: () => userinfoMock(),
      logout: () => logoutMock(),
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { Store, membershipActivatingFromChain, ENTITLEMENT_POLL_MS, ENTITLEMENT_POLL_MAX_MS, FOCUS_RECHECK_MIN_MS } from "./store";
import { entitlementBanner } from "./entitlementBanner";

async function flush(): Promise<void> {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}

/** A signed-in member whose claim currently reads no tier (free + lapsed). */
function lapsedMember(): Store {
  const store = new Store();
  store.setState({
    signedIn: true,
    walletAddr: "0xabc",
    custodyAddr: "0xabc",
    tier: "free",
    entitlement: "lapsed",
    authExpiresAt: null,
    stage: "done",
  });
  return store;
}

describe("entitlement poll after checkout (FIX 1a)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    checkoutMock.mockClear();
    grantStatusMock.mockClear();
    userinfoMock.mockClear();
    logoutMock.mockClear();
    grantState.current = NONE;
    userinfoState.current = NO_TIER;
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("the cadence outlasts one 10-minute server reconcile", () => {
    expect(ENTITLEMENT_POLL_MS).toBe(10_000);
    expect(ENTITLEMENT_POLL_MAX_MS).toBeGreaterThan(10 * 60_000);
    expect(ENTITLEMENT_POLL_MAX_MS).toBeLessThanOrEqual(15 * 60_000);
  });

  it("renew (Settings) opens checkout AND starts the poll; the banner flips lapsed → active without a restart", async () => {
    const store = lapsedMember();
    expect(entitlementBanner(store.state).tone).toBe("danger");
    await store.renewMembership();
    expect(checkoutMock).toHaveBeenCalledTimes(1);
    expect(store.entitlementPollActive()).toBe(true);

    // Nothing yet: still lapsed after the first tick.
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(userinfoMock).toHaveBeenCalled();
    expect(grantStatusMock).toHaveBeenCalledWith("0xabc");
    expect(store.state.entitlement).toBe("lapsed");

    // The server grants; the next tick folds the claim.
    userinfoState.current = COMMERCIAL;
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.tier).toBe("commercial");
    expect(store.state.entitlement).toBe("active");
    expect(entitlementBanner(store.state).show).toBe(false);

    // The poll stops once active.
    expect(store.entitlementPollActive()).toBe(false);
    const calls = userinfoMock.mock.calls.length;
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS * 5);
    expect(userinfoMock.mock.calls.length).toBe(calls);
  });

  it("onboarding S3 checkout also starts the poll", async () => {
    const store = lapsedMember();
    store.setState({ stage: "s3" });
    store.onS3Pay();
    await flush();
    expect(checkoutMock).toHaveBeenCalledTimes(1);
    expect(store.entitlementPollActive()).toBe(true);
  });

  it("is a single instance: a second checkout does not start a second poll", async () => {
    const store = lapsedMember();
    await store.renewMembership();
    await store.renewMembership();
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    // One tick → one userinfo read, not two.
    expect(userinfoMock).toHaveBeenCalledTimes(1);
  });

  it("stops after the bounded window when the claim never arrives", async () => {
    const store = lapsedMember();
    await store.renewMembership();
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MAX_MS + ENTITLEMENT_POLL_MS);
    expect(store.entitlementPollActive()).toBe(false);
    const calls = userinfoMock.mock.calls.length;
    expect(calls).toBeLessThanOrEqual(Math.ceil(ENTITLEMENT_POLL_MAX_MS / ENTITLEMENT_POLL_MS) + 1);
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS * 10);
    expect(userinfoMock.mock.calls.length).toBe(calls);
    expect(store.state.entitlement).toBe("lapsed"); // never fabricated
  });

  it("is cancelled on sign-out", async () => {
    const store = lapsedMember();
    await store.renewMembership();
    expect(store.entitlementPollActive()).toBe(true);
    await store.authLogout();
    expect(store.entitlementPollActive()).toBe(false);
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS * 3);
    expect(userinfoMock).not.toHaveBeenCalled();
  });

  it("a failed read keeps polling (transient) and never fabricates", async () => {
    const store = lapsedMember();
    userinfoMock.mockRejectedValueOnce(new Error("offline"));
    await store.renewMembership();
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.entitlement).toBe("lapsed");
    expect(store.entitlementPollActive()).toBe(true);
    userinfoState.current = COMMERCIAL;
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.entitlement).toBe("active");
  });
});

describe("focus / visibility re-check (FIX 1b)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    grantStatusMock.mockClear();
    userinfoMock.mockClear();
    grantState.current = NONE;
    userinfoState.current = NO_TIER;
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("window focus folds the fresh claim: banner lapsed → active without a restart", async () => {
    const store = lapsedMember();
    const off = store.installEntitlementFocusRecheck();
    userinfoState.current = COMMERCIAL;
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(userinfoMock).toHaveBeenCalledTimes(1);
    expect(store.state.entitlement).toBe("active");
    expect(entitlementBanner(store.state).show).toBe(false);
    off();
  });

  it("visibilitychange → visible re-checks", async () => {
    const store = lapsedMember();
    const off = store.installEntitlementFocusRecheck();
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(userinfoMock).toHaveBeenCalledTimes(1);
    off();
  });

  it("is throttled to one re-check per 30 s", async () => {
    expect(FOCUS_RECHECK_MIN_MS).toBe(30_000);
    const store = lapsedMember();
    const off = store.installEntitlementFocusRecheck();
    window.dispatchEvent(new Event("focus"));
    await flush();
    window.dispatchEvent(new Event("focus"));
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(userinfoMock).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(FOCUS_RECHECK_MIN_MS);
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(userinfoMock).toHaveBeenCalledTimes(2);
    off();
  });

  it("does nothing while signed out", async () => {
    const store = new Store();
    store.setState({ signedIn: false });
    const off = store.installEntitlementFocusRecheck();
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(userinfoMock).not.toHaveBeenCalled();
    off();
  });

  it("removing the listeners stops re-checks", async () => {
    const store = lapsedMember();
    const off = store.installEntitlementFocusRecheck();
    off();
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(userinfoMock).not.toHaveBeenCalled();
  });
});

describe("activating state (FIX 1c)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    grantStatusMock.mockClear();
    userinfoMock.mockClear();
    grantState.current = NONE;
    userinfoState.current = NO_TIER;
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("pure: SBT minted, stake still landing, no claim → activating", () => {
    const st = { entitlement: "lapsed", authExpiresAt: null };
    expect(membershipActivatingFromChain(st, SBT_ONLY)).toBe(true);
    expect(membershipActivatingFromChain(st, { attributedStakeWei: "5", hasSbt: false })).toBe(true);
  });

  it("pure: no chain evidence → not activating (the lapse is real)", () => {
    expect(membershipActivatingFromChain({ entitlement: "lapsed", authExpiresAt: null }, NONE)).toBe(false);
  });

  it("pure: full chain proof → not 'activating' (the safeguard restores active instead)", () => {
    expect(membershipActivatingFromChain({ entitlement: "lapsed", authExpiresAt: null }, FULL)).toBe(false);
  });

  it("pure: a genuinely expired claim is a real lapse, never 'activating'", () => {
    expect(membershipActivatingFromChain({ entitlement: "lapsed", authExpiresAt: "2001-01-01" }, SBT_ONLY)).toBe(false);
  });

  it("pure: an active entitlement is never 'activating'; a malformed stake fails closed", () => {
    expect(membershipActivatingFromChain({ entitlement: "active", authExpiresAt: null }, SBT_ONLY)).toBe(false);
    expect(membershipActivatingFromChain({ entitlement: "lapsed", authExpiresAt: null }, { attributedStakeWei: "x", hasSbt: false })).toBe(false);
  });

  it("poll: SBT on chain, claim absent → neutral activating banner, not red, not active", async () => {
    const store = lapsedMember();
    grantState.current = SBT_ONLY;
    await store.renewMembership();
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.entitlement).toBe("lapsed"); // never "active" without claim or full proof
    expect(store.state.membershipActivating).toBe(true);
    const b = entitlementBanner(store.state);
    expect(b.show).toBe(true);
    expect(b.tone).toBe("neutral");
    expect(b.text).toMatch(/Membership activating/);
    expect(b.cta).toBe(false);
    // Still polling: activating is not the end state.
    expect(store.entitlementPollActive()).toBe(true);

    // The claim lands → active, activating cleared, banner gone.
    userinfoState.current = COMMERCIAL;
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.entitlement).toBe("active");
    expect(store.state.membershipActivating).toBe(false);
    expect(entitlementBanner(store.state).show).toBe(false);
  });

  it("poll: full chain proof with no claim → the existing safeguard restores active", async () => {
    const store = lapsedMember();
    grantState.current = FULL;
    await store.renewMembership();
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.entitlement).toBe("active");
    expect(store.state.tier).toBe("commercial");
    expect(store.state.membershipActivating).toBe(false);
  });

  it("focus with full chain proof does not flip a chain-proven member back to lapsed", async () => {
    const store = lapsedMember();
    grantState.current = FULL;
    const off = store.installEntitlementFocusRecheck();
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(store.state.entitlement).toBe("active");
    off();
  });

  it("banner: a plain lapse stays the red lapsed banner with a Renew CTA", () => {
    const b = entitlementBanner({ entitlement: "lapsed", membershipActivating: false });
    expect(b.tone).toBe("danger");
    expect(b.text).toMatch(/Membership lapsed/);
    expect(b.cta).toBe(true);
  });
});
