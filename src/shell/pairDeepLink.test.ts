// HUP-S8.2: a `citrate://pair?…` link opened from a QR or another app lands in the fleet wizard
// with the link filled in. It pairs nothing by itself: the member presses "Pair". Before this, the
// pairing link fell through to the join-invite parser and showed up as a bogus group invite.
import { beforeEach, describe, expect, it } from "vitest";
import { store } from "./store";

const PAIR = "citrate://pair?c=eyJ2IjoxfQ&s=AAAA";

describe("citrate://pair deep link", () => {
  beforeEach(() => {
    store.setState({ pendingInvite: null, pendingPairLink: null, route: "home" });
  });

  it("goes to the Cluster surface with the link pending, and is not read as a group invite", () => {
    store.handleDeepLink(PAIR);
    expect(store.state.pendingPairLink).toBe(PAIR);
    expect(store.state.pendingInvite).toBeNull();
    expect(store.state.route).toBe("cluster");
  });

  it("is cleared once the wizard has taken it", () => {
    store.handleDeepLink(PAIR);
    store.clearPendingPairLink();
    expect(store.state.pendingPairLink).toBeNull();
  });

  it("an oversized pairing link is ignored", () => {
    store.handleDeepLink(`citrate://pair?c=${"A".repeat(3000)}&s=x`);
    expect(store.state.pendingPairLink).toBeNull();
    expect(store.state.pendingInvite).toBeNull();
  });
});
