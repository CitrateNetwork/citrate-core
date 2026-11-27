// L3-WP19: the banner through the store, from checkout to an active membership.
//
// Why the red label stayed after paying (owner report 2026-11-27):
//   - the post-checkout poll and the focus re-check read /userinfo with the in-memory access
//     token, which the authority expires after an hour; on a node left open longer every read
//     was a 401 the poll swallowed, so a claim that DID land was never folded until a restart;
//   - nothing marked "a checkout was opened", so until the claim or chain evidence arrived the
//     banner stayed the red lapse, and for grants the server refuses (no linked wallet, identity
//     not verified) it never left.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

type GrantShape = { attributedStakeWei: string; hasSbt: boolean };
const NONE: GrantShape = { attributedStakeWei: "0", hasSbt: false };
const FULL: GrantShape = { attributedStakeWei: (32000n * 10n ** 18n).toString(), hasSbt: true };
const grantState = { current: NONE as GrantShape };

const NO_TIER = {
  signedIn: true,
  sub: "usr_x",
  tier: null as string | null,
  org: null,
  role: "member",
  kycStatus: "verified" as string | null,
  walletAddr: "0xabc",
  expiresAt: null as string | null,
  email: "m@example.com",
};
const COMMERCIAL = { ...NO_TIER, tier: "commercial" };
const userinfoState = { current: NO_TIER as typeof NO_TIER };
/** When true, /userinfo answers as the kit does for an expired access token, until a refresh. */
const session = { expired: false };
/** The kit's AuthError::Unauthorized text (kit/src/oidc.rs). */
const UNAUTHORIZED = "auth: session expired or not authorized — sign in again";

const checkoutMock = vi.fn(async (_hint?: string) => {});
const grantStatusMock = vi.fn(async (_a: string) => grantState.current);
const userinfoMock = vi.fn(async () => {
  if (session.expired) throw UNAUTHORIZED;
  return userinfoState.current;
});
const refreshMock = vi.fn(async () => {
  session.expired = false;
  return userinfoState.current;
});

vi.mock("../../../bridge", () => ({
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
      refresh: () => refreshMock(),
      logout: async () => {},
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));

vi.mock("../../../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { Store, ENTITLEMENT_POLL_MS, ENTITLEMENT_POLL_MAX_MS } from "../../store";
import { entitlementBanner } from "../../entitlementBanner";

/** A signed-in free member (no tier claim: free + lapsed), wallet linked, identity verified. */
function freeMember(): Store {
  const store = new Store();
  store.setState({
    signedIn: true,
    authEmail: "m@example.com",
    walletAddr: "0xabc",
    custodyAddr: "0xabc",
    tier: "free",
    entitlement: "lapsed",
    authExpiresAt: null,
    hasGrant: false,
    hasSbt: false,
    s2: "verified",
    s3: "idle",
    stage: "done",
  });
  return store;
}

/** A member whose membership lapsed: they hold the grant from before. */
function lapsedMember(): Store {
  const store = freeMember();
  store.setState({ hasGrant: true, hasSbt: true, s3: "settled" });
  return store;
}

describe("post-checkout: the banner leaves the lapse and clears once active", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    checkoutMock.mockClear();
    grantStatusMock.mockClear();
    userinfoMock.mockClear();
    refreshMock.mockClear();
    grantState.current = NONE;
    userinfoState.current = NO_TIER;
    session.expired = false;
    // Each test starts from a clean save: an earlier test's s3 "paying" must not leak in.
    localStorage.clear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("a free member's Upgrade opens the guarded checkout; activating until the claim lands, then gone", async () => {
    const store = freeMember();
    expect(entitlementBanner(store.state).action).toBe("upgrade");
    await store.becomeMember();
    expect(checkoutMock).toHaveBeenCalledTimes(1);
    // The checkout step is recorded (persisted), so a restart still knows a checkout was opened.
    expect(store.state.s3).toBe("paying");
    const waiting = entitlementBanner(store.state);
    expect(waiting.tone).toBe("neutral");
    expect(waiting.text).toMatch(/^Membership activating\./);
    expect(waiting.cta).toBe(false);

    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.entitlement).toBe("lapsed"); // never fabricated
    expect(entitlementBanner(store.state).tone).not.toBe("danger");

    userinfoState.current = COMMERCIAL;
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(store.state.entitlement).toBe("active");
    expect(store.state.membershipCheckoutAt).toBeNull();
    expect(entitlementBanner(store.state).show).toBe(false);
    expect(store.entitlementPollActive()).toBe(false);
  });

  it("renewal on a node open past the access token's hour: a 401 renews the session and the claim folds", async () => {
    const store = lapsedMember();
    expect(entitlementBanner(store.state).tone).toBe("danger");
    await store.renewMembership();
    expect(entitlementBanner(store.state).tone).not.toBe("danger");

    // The server grants while the access token has expired.
    session.expired = true;
    userinfoState.current = COMMERCIAL;
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(refreshMock).toHaveBeenCalledTimes(1);
    expect(store.state.entitlement).toBe("active");
    expect(entitlementBanner(store.state).show).toBe(false);
  });

  it("the poll and the focus re-check both hitting the 401 refresh the session once", async () => {
    const store = lapsedMember();
    const off = store.installEntitlementFocusRecheck();
    await store.renewMembership();
    session.expired = true;
    userinfoState.current = COMMERCIAL;
    // The member comes back from the browser just as the poll ticks.
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(refreshMock).toHaveBeenCalledTimes(1);
    expect(store.state.entitlement).toBe("active");
    off();
  });

  it("signing out while a session refresh runs does not sign the member back in", async () => {
    const store = lapsedMember();
    session.expired = true;
    let release: () => void = () => {};
    refreshMock.mockImplementationOnce(async () => {
      await new Promise<void>((r) => (release = r));
      session.expired = false;
      return COMMERCIAL;
    });
    const read = store.refreshEntitlement().catch((e: unknown) => e);
    await vi.waitFor(() => expect(refreshMock).toHaveBeenCalledTimes(1));
    await store.authLogout();
    release();
    await read;
    expect(store.state.signedIn).toBe(false);
    expect(store.state.entitlement).toBe("lapsed");
  });

  it("a read that fails for another reason does not refresh the session and fabricates nothing", async () => {
    const store = lapsedMember();
    await store.renewMembership();
    userinfoMock.mockRejectedValueOnce(new Error("offline"));
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MS);
    expect(refreshMock).not.toHaveBeenCalled();
    expect(store.state.entitlement).toBe("lapsed");
    expect(store.entitlementPollActive()).toBe(true);
  });

  it("a grant that never lands: after the poll's bound the banner is a neutral 'still activating', never red", async () => {
    const store = freeMember();
    await store.becomeMember();
    await vi.advanceTimersByTimeAsync(ENTITLEMENT_POLL_MAX_MS + ENTITLEMENT_POLL_MS);
    expect(store.entitlementPollActive()).toBe(false);
    expect(store.state.entitlement).toBe("lapsed");
    const b = entitlementBanner(store.state);
    expect(b.tone).toBe("neutral");
    expect(b.text).toMatch(/^Still activating\./);
    expect(b.text).toContain("support@citrate.ai");
  });

  it("paid but the wallet is not linked: the banner asks for the link, not a renewal", async () => {
    const store = freeMember();
    // Onboarding opened checkout in an earlier session, and this device's wallet is not linked.
    store.setState({ s3: "paying", custodyAddr: "0xdef" });
    const b = entitlementBanner(store.state);
    expect(b.tone).toBe("brand");
    expect(b.action).toBe("link");
  });

  it("paid but identity not verified: the banner asks for verification, not a renewal", async () => {
    const store = freeMember();
    store.setState({ s3: "paying", s2: "none" });
    const b = entitlementBanner(store.state);
    expect(b.tone).toBe("brand");
    expect(b.action).toBe("verify");
  });

  it("a lapsed member who did not open checkout keeps the red Renew (2026-09-11 safeguard intact)", async () => {
    const store = lapsedMember();
    await store.refreshEntitlement();
    expect(store.state.entitlement).toBe("lapsed");
    expect(entitlementBanner(store.state).action).toBe("renew");
    // A real on-chain SBT and grant still restore an active membership over a thin claim.
    grantState.current = FULL;
    await store.refreshEntitlement();
    expect(store.state.entitlement).toBe("active");
  });

  it("sign-out forgets the checkout this session opened", async () => {
    const store = freeMember();
    await store.becomeMember();
    expect(store.state.membershipCheckoutAt).not.toBeNull();
    await store.authLogout();
    expect(store.state.membershipCheckoutAt).toBeNull();
  });
});
