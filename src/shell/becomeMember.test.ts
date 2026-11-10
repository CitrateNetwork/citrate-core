// Settings › Billing "Become a member ↗" (follow-up to #576). A free user's checkout goes
// through the same guards as onboarding S3 (onS3Pay): link the wallet first, and never open
// checkout for an account that already holds the membership grant on chain. Renewal for a
// lapsed member is unchanged (a lapsed member holds the grant and still renews).
import { describe, it, expect, vi, beforeEach } from "vitest";

type GrantShape = { attributedStakeWei: string; hasSbt: boolean };
const NONE: GrantShape = { attributedStakeWei: "0", hasSbt: false };
const FULL: GrantShape = { attributedStakeWei: (32000n * 10n ** 18n).toString(), hasSbt: true };
const grant = { current: NONE as GrantShape, fail: false };

const checkoutMock = vi.fn(async (_hint?: string) => {});
const grantStatusMock = vi.fn(async (_a: string) => {
  if (grant.fail) throw new Error("rpc down");
  return grant.current;
});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: {
      checkout: (h?: string) => checkoutMock(h),
      grantStatus: (a: string) => grantStatusMock(a),
    },
    auth: { status: async () => ({ signedIn: false }), userinfo: async () => ({ signedIn: true, tier: null }), logout: async () => {} },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";

async function flush(): Promise<void> {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}

function freeUser(linked: boolean): Store {
  const store = new Store();
  store.setState({
    signedIn: true,
    authEmail: "free@example.com",
    walletAddr: "0xabc",
    custodyAddr: linked ? "0xabc" : "0xdef",
    tier: "free",
    entitlement: "active",
    hasGrant: false,
    stage: "done",
  });
  return store;
}

describe("store.becomeMember — the guarded checkout for a free user", () => {
  beforeEach(() => {
    checkoutMock.mockClear();
    grantStatusMock.mockClear();
    grant.current = NONE;
    grant.fail = false;
  });

  it("links the wallet first: an unlinked device opens the link, never checkout", async () => {
    const store = freeUser(false);
    const link = vi.spyOn(store, "linkWallet").mockResolvedValue(undefined);
    await store.becomeMember();
    await flush();
    expect(link).toHaveBeenCalledTimes(1);
    expect(checkoutMock).not.toHaveBeenCalled();
  });

  it("MONEY GUARD: an account that already holds the grant is never sent to checkout", async () => {
    grant.current = FULL;
    const store = freeUser(true);
    await store.becomeMember();
    await flush();
    expect(grantStatusMock).toHaveBeenCalledTimes(1);
    expect(checkoutMock).not.toHaveBeenCalled();
    expect(store.state.hasGrant).toBe(true);
  });

  it("a linked free user with no grant gets the checkout, bound to the signed-in email", async () => {
    const store = freeUser(true);
    await store.becomeMember();
    await flush();
    expect(grantStatusMock).toHaveBeenCalledTimes(1);
    expect(checkoutMock).toHaveBeenCalledTimes(1);
    expect(checkoutMock).toHaveBeenCalledWith("free@example.com");
    expect(store.entitlementPollActive()).toBe(true);
  });

  it("an unreadable chain is not evidence of a grant (same rule as onboarding S3)", async () => {
    grant.fail = true;
    const store = freeUser(true);
    await store.becomeMember();
    await flush();
    expect(checkoutMock).toHaveBeenCalledTimes(1);
  });

  it("renewal for a lapsed member who holds the grant still opens checkout", async () => {
    grant.current = FULL;
    const store = freeUser(true);
    store.setState({ entitlement: "lapsed", hasGrant: true });
    await store.renewMembership();
    await flush();
    expect(checkoutMock).toHaveBeenCalledTimes(1);
  });
});
