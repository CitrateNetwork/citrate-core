// L3-WP19 part B: Settings › Billing reads the same membership derivation as the banner
// (src/shell/actions/l3/membership.ts, landed in L0-WP01c #821): a free member whose claim
// folds as lapsed is offered the guarded checkout, not "Renew", and is not told their
// membership lapsed. Moved from #819's membershipBanner.test.tsx when the WP was split.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { freshState, type AppState } from "../../state";
import { Settings } from "../../../surfaces/Settings";
import type { Store } from "../../store";

/** A signed-in free member as the entitlement engine folds them: no tier claim, so free + lapsed. */
function freeMember(over: Partial<AppState> = {}): AppState {
  return {
    ...freshState("p1"),
    signedIn: true,
    stage: "done",
    tier: "free",
    entitlement: "lapsed",
    authExpiresAt: null,
    hasGrant: false,
    hasSbt: false,
    s3: "idle",
    s2: "none",
    walletAddr: "0xabc",
    custodyAddr: "0xabc",
    ...over,
  } as AppState;
}

function settingsText(s: AppState): string {
  const store = new Proxy({} as Record<string, unknown>, {
    get: (_t, k: string) =>
      k === "identity"
        ? () => ({ name: "M", initials: "M", email: "m@example.com", sub: "s", wallet: "0xabc", tier: "free", role: "member", org: null, real: true })
        : () => undefined,
  }) as unknown as Store;
  return renderToStaticMarkup(<Settings store={store} s={{ ...s, sSec: "billing" }} />).replace(/<[^>]*>/g, " ");
}

describe("Settings › Billing for a free member whose claim has no tier", () => {
  it("offers 'Become a member ↗', not 'Renew', and says no membership rather than lapsed", () => {
    const t = settingsText(freeMember());
    expect(t).toContain("Become a member ↗");
    expect(t).not.toMatch(/Renew/);
    expect(t).toContain("no active membership");
    expect(t).not.toMatch(/lapsed/);
  });

  it("a lapsed member with a grant keeps 'Renew ↗'", () => {
    const t = settingsText(freeMember({ hasGrant: true }));
    expect(t).toContain("Renew ↗");
  });
});
