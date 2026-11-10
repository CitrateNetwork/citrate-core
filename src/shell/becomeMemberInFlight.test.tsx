// Settings › Billing "Become a member ↗" makes one checkout attempt at a time (core#584, the
// becomeMember half). Before checkout opens, becomeMember reads the chain for a grant this account
// already holds; that read takes time. While it runs:
//   1. a second click (double click) must not start a second attempt: at most one checkout opens;
//   2. signing out ends the attempt: when the read resolves, no checkout opens for a session that
//      is gone;
//   3. the button is disabled.
// The guards from #579 are unchanged: link first; a held grant never goes to checkout; an
// unreadable chain still opens checkout.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";

type GrantShape = { attributedStakeWei: string; hasSbt: boolean };
const NONE: GrantShape = { attributedStakeWei: "0", hasSbt: false };
const FULL: GrantShape = { attributedStakeWei: (32000n * 10n ** 18n).toString(), hasSbt: true };

const m = vi.hoisted(() => {
  type Pending = { resolve: (g: unknown) => void; reject: (e: unknown) => void };
  const pending: Pending[] = [];
  return {
    pending,
    checkout: vi.fn(async (_hint?: string) => {}),
    grantStatus: vi.fn(
      (_a: string) =>
        new Promise((resolve, reject) => {
          pending.push({ resolve, reject });
        }),
    ),
  };
});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: {
      checkout: (h?: string) => m.checkout(h),
      grantStatus: (a: string) => m.grantStatus(a),
    },
    auth: { status: async () => ({ signedIn: false }), userinfo: async () => ({ signedIn: true, tier: null }), logout: async () => {} },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";
import { Settings } from "../surfaces/Settings";
import { freshState, type AppState } from "./state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function flush(): Promise<void> {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}

async function settle(grant: GrantShape | Error = NONE): Promise<void> {
  for (const p of m.pending.splice(0)) {
    if (grant instanceof Error) p.reject(grant);
    else p.resolve(grant);
  }
  await flush();
}

function freeUser(): Store {
  const store = new Store();
  store.setState({
    signedIn: true,
    authEmail: "free@example.com",
    walletAddr: "0xabc",
    custodyAddr: "0xabc",
    tier: "free",
    entitlement: "active",
    hasGrant: false,
    stage: "done",
  });
  return store;
}

beforeEach(() => {
  m.pending.splice(0);
  m.checkout.mockClear();
  m.grantStatus.mockClear();
});

describe("store.becomeMember: one attempt at a time while the held-grant check runs", () => {
  it("a double click during the check opens at most one checkout", async () => {
    const store = freeUser();
    void store.becomeMember();
    void store.becomeMember();
    await settle();
    expect(m.grantStatus).toHaveBeenCalledTimes(1);
    expect(m.checkout).toHaveBeenCalledTimes(1);
    expect(m.checkout).toHaveBeenCalledWith("free@example.com");
  });

  it("the check is visible while it runs and ends when the read resolves", async () => {
    const store = freeUser();
    void store.becomeMember();
    await flush();
    expect(store.state.billingChecking).toBe(true);
    await settle();
    expect(store.state.billingChecking).toBe(false);
  });

  it("signing out during the check: no checkout opens when the read resolves", async () => {
    const store = freeUser();
    void store.becomeMember();
    await store.authLogout();
    await settle();
    expect(m.checkout).not.toHaveBeenCalled();
    expect(store.state.billingChecking).toBe(false);
  });

  it("a new attempt after the first one finished is allowed (the guard is per attempt)", async () => {
    const store = freeUser();
    void store.becomeMember();
    await settle();
    void store.becomeMember();
    await settle();
    expect(m.checkout).toHaveBeenCalledTimes(2);
  });

  it("unchanged: a held grant never goes to checkout, and the check ends", async () => {
    const store = freeUser();
    void store.becomeMember();
    void store.becomeMember();
    await settle(FULL);
    expect(m.checkout).not.toHaveBeenCalled();
    expect(store.state.hasGrant).toBe(true);
    expect(store.state.billingChecking).toBe(false);
  });

  it("unchanged: an unreadable chain still opens checkout once, and the check ends", async () => {
    const store = freeUser();
    void store.becomeMember();
    void store.becomeMember();
    await settle(new Error("rpc down"));
    expect(m.checkout).toHaveBeenCalledTimes(1);
    expect(store.state.billingChecking).toBe(false);
  });

  it("unchanged: an unlinked device links first, with no read and no check", async () => {
    const store = freeUser();
    store.setState({ custodyAddr: "0xdef" });
    const link = vi.spyOn(store, "linkWallet").mockResolvedValue(undefined);
    await store.becomeMember();
    await flush();
    expect(link).toHaveBeenCalledTimes(1);
    expect(m.grantStatus).not.toHaveBeenCalled();
    expect(store.state.billingChecking).toBe(false);
  });
});

describe("Settings › Billing: 'Become a member ↗' is disabled during the check", () => {
  function billing(patch: Partial<AppState>): AppState {
    const s = freshState("p1");
    s.sSec = "billing";
    s.signedIn = true;
    return Object.assign(s, { tier: "free", entitlement: "active", hasGrant: false, hasSbt: false }, patch);
  }

  async function becomeButton(s: AppState, store: Store) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => root.render(<Settings store={store} s={s} />));
    const btn = Array.from(host.querySelectorAll("button")).find((b) => /Become a member/.test(b.textContent ?? ""));
    return { btn, root, host };
  }

  it("disabled while billingChecking, and a click does nothing", async () => {
    const store = freeUser();
    const spy = vi.spyOn(store, "becomeMember").mockResolvedValue();
    const { btn, root, host } = await becomeButton(billing({ billingChecking: true }), store);
    expect(btn).toBeTruthy();
    expect(btn!.disabled).toBe(true);
    await act(async () => btn!.click());
    expect(spy).not.toHaveBeenCalled();
    root.unmount();
    host.remove();
  });

  it("enabled when no check runs", async () => {
    const store = freeUser();
    const { btn, root, host } = await becomeButton(billing({}), store);
    expect(btn!.disabled).toBe(false);
    root.unmount();
    host.remove();
  });
});
