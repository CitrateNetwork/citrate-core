// L3-WP19: the pure membership facts the banner and Settings › Billing share, the live claim read
// that survives an expired access token, and the banner's call to action.
import { describe, it, expect, vi } from "vitest";
import {
  checkoutAwaiting,
  isFreeMember,
  isSessionExpiredError,
  neverHeldMembership,
  newClaimSession,
  readLiveClaim,
  SIGNED_OUT_DURING_READ,
  runBannerAction,
  walletLinked,
  walletLinkNeeded,
} from "./membership";
import type { AuthStatus } from "../../../bridge/types";
import type { Store } from "../../store";

const NEVER = { hasGrant: false, hasSbt: false, s3: "idle" as const, authExpiresAt: null };

describe("neverHeldMembership", () => {
  it("is true only with no grant, no SBT, no settled checkout and no claim expiry", () => {
    expect(neverHeldMembership(NEVER)).toBe(true);
    expect(neverHeldMembership({ ...NEVER, hasGrant: true })).toBe(false);
    expect(neverHeldMembership({ ...NEVER, hasSbt: true })).toBe(false);
    expect(neverHeldMembership({ ...NEVER, s3: "settled" })).toBe(false);
    expect(neverHeldMembership({ ...NEVER, authExpiresAt: "2001-01-01" })).toBe(false);
    // A checkout in flight is not a past membership.
    expect(neverHeldMembership({ ...NEVER, s3: "paying" })).toBe(true);
  });
});

describe("checkoutAwaiting / isFreeMember", () => {
  const base = { ...NEVER, entitlement: "lapsed" as const, membershipActivating: false, s2: "none" as const, membershipCheckoutAt: null, walletAddr: "", custodyAddr: "" };

  it("a checkout this session or an earlier one (s3 paying) is awaiting", () => {
    expect(checkoutAwaiting({ s3: "idle", membershipCheckoutAt: null })).toBe(false);
    expect(checkoutAwaiting({ s3: "idle", membershipCheckoutAt: 1 })).toBe(true);
    expect(checkoutAwaiting({ s3: "paying", membershipCheckoutAt: null })).toBe(true);
  });

  it("a free member is lapsed with no history, no checkout and nothing in flight", () => {
    expect(isFreeMember(base)).toBe(true);
    expect(isFreeMember({ ...base, entitlement: "active" })).toBe(false);
    expect(isFreeMember({ ...base, membershipActivating: true })).toBe(false);
    expect(isFreeMember({ ...base, s3: "paying" })).toBe(false);
    expect(isFreeMember({ ...base, hasSbt: true })).toBe(false);
  });
});

describe("walletLinked / walletLinkNeeded (store.walletIsLinked reads the same)", () => {
  it("linked only when both sides are read and equal, case-insensitively", () => {
    expect(walletLinked({ walletAddr: "0xAbC", custodyAddr: "0xabc" })).toBe(true);
    expect(walletLinked({ walletAddr: "0xabc", custodyAddr: "0xdef" })).toBe(false);
    expect(walletLinked({ walletAddr: "", custodyAddr: "0xabc" })).toBe(false);
    expect(walletLinked({ walletAddr: "0xabc", custodyAddr: "" })).toBe(false);
  });

  it("a link is needed only once this device's wallet is read", () => {
    expect(walletLinkNeeded({ walletAddr: "0xabc", custodyAddr: "0xdef" })).toBe(true);
    expect(walletLinkNeeded({ walletAddr: "", custodyAddr: "0xdef" })).toBe(true);
    expect(walletLinkNeeded({ walletAddr: "0xabc", custodyAddr: "" })).toBe(false);
    expect(walletLinkNeeded({ walletAddr: "0xabc", custodyAddr: "0xabc" })).toBe(false);
  });
});

const CLAIM: AuthStatus = { signedIn: true, sub: "s", tier: "commercial", org: null, role: null, kycStatus: null, walletAddr: null, expiresAt: null, email: null };

describe("readLiveClaim", () => {
  it("recognizes the kit's 401 text, as a string or an Error", () => {
    expect(isSessionExpiredError("auth: session expired or not authorized — sign in again")).toBe(true);
    expect(isSessionExpiredError(new Error("auth: session expired or not authorized — sign in again"))).toBe(true);
    expect(isSessionExpiredError(new Error("offline"))).toBe(false);
    expect(isSessionExpiredError("auth: network error")).toBe(false);
  });

  it("returns the claim without touching the session when the read works", async () => {
    const auth = { userinfo: vi.fn(async () => CLAIM), refresh: vi.fn(async () => CLAIM), logout: vi.fn(async () => {}) };
    await expect(readLiveClaim(auth, newClaimSession())).resolves.toEqual(CLAIM);
    expect(auth.refresh).not.toHaveBeenCalled();
  });

  it("on a 401 refreshes once and reads again", async () => {
    const userinfo = vi.fn<() => Promise<AuthStatus>>().mockRejectedValueOnce("auth: session expired or not authorized — sign in again").mockResolvedValueOnce(CLAIM);
    const auth = { userinfo, refresh: vi.fn(async () => CLAIM), logout: vi.fn(async () => {}) };
    await expect(readLiveClaim(auth, newClaimSession())).resolves.toEqual(CLAIM);
    expect(auth.refresh).toHaveBeenCalledTimes(1);
    expect(userinfo).toHaveBeenCalledTimes(2);
  });

  it("any other failure, or a declined refresh, is thrown unchanged", async () => {
    const offline = { userinfo: vi.fn(async (): Promise<AuthStatus> => { throw new Error("offline"); }), refresh: vi.fn(async () => CLAIM), logout: vi.fn(async () => {}) };
    await expect(readLiveClaim(offline, newClaimSession())).rejects.toThrow("offline");
    expect(offline.refresh).not.toHaveBeenCalled();
    const declined = {
      userinfo: vi.fn(async (): Promise<AuthStatus> => { throw "auth: session expired or not authorized — sign in again"; }),
      refresh: vi.fn(async (): Promise<AuthStatus> => { throw new Error("auth: token exchange failed"); }),
      logout: vi.fn(async () => {}),
    };
    const session = newClaimSession();
    await expect(readLiveClaim(declined, session)).rejects.toThrow("token exchange failed");
    expect(declined.userinfo).toHaveBeenCalledTimes(1);
    expect(session.refreshing).toBeNull(); // a failed refresh does not stick
  });

  it("concurrent callers that both hit the 401 share ONE refresh (rotated tokens cannot be replayed)", async () => {
    let expired = true;
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const auth = {
      userinfo: vi.fn(async (): Promise<AuthStatus> => {
        if (expired) throw "auth: session expired or not authorized — sign in again";
        return CLAIM;
      }),
      refresh: vi.fn(async () => {
        await gate;
        expired = false;
        return CLAIM;
      }),
      logout: vi.fn(async () => {}),
    };
    const session = newClaimSession();
    const a = readLiveClaim(auth, session);
    const b = readLiveClaim(auth, session);
    await Promise.resolve();
    release();
    await expect(Promise.all([a, b])).resolves.toEqual([CLAIM, CLAIM]);
    expect(auth.refresh).toHaveBeenCalledTimes(1);
  });

  it("a sign-out during the refresh wins: the session is wiped again and nothing is returned", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const auth = {
      userinfo: vi.fn(async (): Promise<AuthStatus> => { throw "auth: session expired or not authorized — sign in again"; }),
      refresh: vi.fn(async () => {
        await gate;
        return CLAIM;
      }),
      logout: vi.fn(async () => {}),
    };
    const session = newClaimSession();
    const read = readLiveClaim(auth, session);
    await Promise.resolve();
    session.epoch++; // authLogout
    release();
    await expect(read).rejects.toThrow(SIGNED_OUT_DURING_READ);
    expect(auth.logout).toHaveBeenCalledTimes(1);
    expect(auth.userinfo).toHaveBeenCalledTimes(1);
  });

  it("a refresh that fails after a sign-out still wipes the session again", async () => {
    let fail: (e: unknown) => void = () => {};
    const gate = new Promise<never>((_r, j) => (fail = j));
    const auth = {
      userinfo: vi.fn(async (): Promise<AuthStatus> => { throw "auth: session expired or not authorized — sign in again"; }),
      refresh: vi.fn(async (): Promise<AuthStatus> => gate),
      logout: vi.fn(async () => {}),
    };
    const session = newClaimSession();
    const read = readLiveClaim(auth, session);
    await vi.waitFor(() => expect(auth.refresh).toHaveBeenCalledTimes(1));
    session.epoch++; // authLogout
    fail(new Error("auth: network error"));
    await expect(read).rejects.toThrow(SIGNED_OUT_DURING_READ);
    expect(auth.logout).toHaveBeenCalledTimes(1);
  });

  it("a refresh from before a sign-out is never joined by a read after it", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    let expired = true;
    const auth = {
      userinfo: vi.fn(async (): Promise<AuthStatus> => {
        if (expired) throw "auth: session expired or not authorized — sign in again";
        return CLAIM;
      }),
      refresh: vi.fn(async () => {
        await gate;
        expired = false;
        return CLAIM;
      }),
      logout: vi.fn(async () => {}),
    };
    const session = newClaimSession();
    const stale = readLiveClaim(auth, session).catch((e: unknown) => e);
    await vi.waitFor(() => expect(auth.refresh).toHaveBeenCalledTimes(1));
    session.epoch++; // sign-out, then a new session
    const fresh = readLiveClaim(auth, session);
    await vi.waitFor(() => expect(auth.refresh).toHaveBeenCalledTimes(2));
    release();
    await expect(fresh).resolves.toEqual(CLAIM);
    expect(await stale).toBeInstanceOf(Error);
  });

  it("a sign-out during a plain read rejects it too", async () => {
    const session = newClaimSession();
    const auth = {
      userinfo: vi.fn(async () => {
        session.epoch++;
        return CLAIM;
      }),
      refresh: vi.fn(async () => CLAIM),
      logout: vi.fn(async () => {}),
    };
    await expect(readLiveClaim(auth, session)).rejects.toThrow(SIGNED_OUT_DURING_READ);
  });
});

describe("runBannerAction runs the existing seams", () => {
  function spyStore() {
    const calls: string[] = [];
    const store = {
      renewMembership: vi.fn(async () => void calls.push("renew")),
      becomeMember: vi.fn(async () => void calls.push("becomeMember")),
      linkWallet: vi.fn(async () => void calls.push("linkWallet")),
      onS2Start: vi.fn(() => void calls.push("onS2Start")),
      startEntitlementPoll: vi.fn(() => void calls.push("poll")),
    };
    return { store: store as unknown as Store, calls };
  }

  it("renew renews; upgrade is the guarded checkout", async () => {
    const a = spyStore();
    await runBannerAction(a.store, "renew");
    expect(a.calls).toEqual(["renew"]);
    const b = spyStore();
    await runBannerAction(b.store, "upgrade");
    expect(b.calls).toEqual(["becomeMember"]);
  });

  it("link and verify open their flows, then keep watching the entitlement", async () => {
    const a = spyStore();
    await runBannerAction(a.store, "link");
    expect(a.calls).toEqual(["linkWallet", "poll"]);
    const b = spyStore();
    await runBannerAction(b.store, "verify");
    expect(b.calls).toEqual(["onS2Start", "poll"]);
  });
});
