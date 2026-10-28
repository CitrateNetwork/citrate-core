// C1-P — the membership price comes from core-membership's live price route, read in Rust
// (no CORS), and is shown only when it was read. On a 503 or any error the app keeps the
// "See membership pricing" link and shows no number (UI truth contract §5.3).
// Owner decision 2026-10-27 (a, #576): in onboarding the price shows only at checkout
// (MembershipCheckout), after the free-node choice. The Commissary seed tabs that carried a
// "Join · <price>" label were retired on main (K5a: the Store is the signed catalog only).
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { S3, MembershipCheckout } from "../onboarding/Onboarding";
import { Settings } from "./Settings";
import { Commissary } from "./Commissary";
import { membershipPriceLabel, type MembershipPrice } from "../data/membership";
import { store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { bridge } from "../bridge";

const PRICE: MembershipPrice = { currency: "usd", unitAmount: 4800, interval: "year", displayAmount: "$48" };

const stub = {
  walletIsLinked: () => true,
  refreshMembershipPrice: () => {},
  watchMembershipPrice: () => () => {},
  openExternal: async () => {},
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, ...over };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  store.setState({ membershipPrice: null });
  (store as unknown as { lastPriceReadAt: number }).lastPriceReadAt = 0;
});

describe("C1-P — the membership price is read, never assumed", () => {
  it("labels a read price with its interval", () => {
    expect(membershipPriceLabel(PRICE)).toBe("$48 a year");
    expect(membershipPriceLabel({ ...PRICE, interval: "month", displayAmount: "$4", unitAmount: 400 })).toBe("$4 a month");
    expect(membershipPriceLabel(null)).toBeNull();
  });

  it("the store reads the price through the bridge and keeps it", async () => {
    const price = vi.spyOn(bridge.membership, "price").mockResolvedValue(PRICE);
    await store.refreshMembershipPrice();
    expect(price).toHaveBeenCalledTimes(1);
    expect(store.state.membershipPrice).toEqual(PRICE);
    // Cached for a while: a second call does not hit the route again.
    await store.refreshMembershipPrice();
    expect(price).toHaveBeenCalledTimes(1);
  });

  it("a failed read (503) leaves no price, and clears an earlier one", async () => {
    store.setState({ membershipPrice: PRICE });
    vi.spyOn(bridge.membership, "price").mockRejectedValue("membership price unavailable (HTTP 503)");
    await store.refreshMembershipPrice();
    expect(store.state.membershipPrice).toBeNull();
  });

  it("S3 shows the read price, and the link with no number when there is none", () => {
    // The checkout step of S3 (MembershipCheckout) is the one onboarding screen with a price.
    const read = renderToStaticMarkup(<MembershipCheckout store={stub} s={member({ s3: "idle", hasGrant: false, membershipPrice: PRICE })} />);
    expect(read).toContain("$48 a year");
    expect(read).toContain("does not buy SALT");
    const none = renderToStaticMarkup(<MembershipCheckout store={stub} s={member({ s3: "idle", hasGrant: false, membershipPrice: null })} />);
    expect(none).toContain("See membership pricing");
    expect(none).not.toMatch(/\$\s?\d/);
  });

  it("S3's free-node choice shows no price, even when one was read", () => {
    const html = renderToStaticMarkup(<S3 store={stub} s={member({ s3: "idle", hasGrant: false, membershipPrice: PRICE })} />);
    expect(html).toContain("Run a node for free");
    expect(html).not.toContain("$48");
    expect(html).not.toMatch(/\$\s?\d/);
  });

  it("Settings shows the read price on the plan line", () => {
    const html = renderToStaticMarkup(<Settings store={stub} s={member({ sSec: "billing", tier: "pilot", membershipPrice: PRICE })} />);
    expect(html).toContain("$48 a year");
  });

  it("the Store (formerly the Commissary) has no join label and states no price", () => {
    const html = renderToStaticMarkup(<Commissary store={stub} s={member({ tier: "free", membershipPrice: PRICE })} />);
    expect(html).not.toContain("Join ·");
    expect(html).not.toContain("$48");
    expect(html).not.toMatch(/\$\s?\d/);
  });

  it("the web build has no price source and never shows a number", async () => {
    const { createSimBridge } = await import("../bridge/sim");
    await expect(createSimBridge({} as never).membership.price()).rejects.toBeTruthy();
  });
});

// #379 review P2: the price has a lifetime. It is re-read (or cleared) when the lifetime runs
// out, even while a price surface stays open with no navigation.
describe("C1-P — the read price expires", () => {
  const LIFETIME_MS = 10 * 60_000;

  it("a mounted price surface re-reads the price when its lifetime runs out", async () => {
    vi.useFakeTimers();
    const price = vi
      .spyOn(bridge.membership, "price")
      .mockResolvedValueOnce(PRICE)
      .mockResolvedValueOnce({ ...PRICE, unitAmount: 6000, displayAmount: "$60" });
    const stop = store.watchMembershipPrice();
    await vi.advanceTimersByTimeAsync(0);
    expect(store.state.membershipPrice?.displayAmount).toBe("$48");
    await vi.advanceTimersByTimeAsync(LIFETIME_MS + 1);
    expect(price).toHaveBeenCalledTimes(2);
    expect(store.state.membershipPrice?.displayAmount).toBe("$60");
    stop();
  });

  it("a re-read that fails at expiry clears the price instead of keeping the old one", async () => {
    vi.useFakeTimers();
    vi.spyOn(bridge.membership, "price").mockResolvedValueOnce(PRICE).mockRejectedValueOnce("membership price unavailable (HTTP 503)");
    const stop = store.watchMembershipPrice();
    await vi.advanceTimersByTimeAsync(0);
    expect(store.state.membershipPrice).toEqual(PRICE);
    await vi.advanceTimersByTimeAsync(LIFETIME_MS + 1);
    expect(store.state.membershipPrice).toBeNull();
    stop();
  });

  it("with no price surface open, an expired price is cleared and not re-read", async () => {
    vi.useFakeTimers();
    const price = vi.spyOn(bridge.membership, "price").mockResolvedValue(PRICE);
    const stop = store.watchMembershipPrice();
    await vi.advanceTimersByTimeAsync(0);
    stop();
    await vi.advanceTimersByTimeAsync(LIFETIME_MS + 1);
    expect(store.state.membershipPrice).toBeNull();
    expect(price).toHaveBeenCalledTimes(1);
  });
});
