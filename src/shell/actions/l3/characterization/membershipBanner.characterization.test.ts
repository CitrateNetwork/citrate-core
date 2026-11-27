// 0.6.0 L3-WP19 characterization: the entitlement banner cases that must survive the free-member
// and post-checkout change (citrate-core 9b1ab14b). A failure here means a member whose membership
// really lapsed, an active member, or the offline grace window now sees something else: update the
// expectation only together with a reviewed, deliberate change of that case.
//
// What WP19 changes on purpose is NOT pinned here (a free member, a checkout waiting on its grant);
// those are the red tests in ../membershipBanner.test.ts.
import { describe, it, expect } from "vitest";
import { entitlementBanner } from "../../../entitlementBanner";
import { freshState, type AppState } from "../../../state";

/** A signed-in member's banner facts on top of a fresh state. */
function facts(over: Partial<AppState>): AppState {
  return { ...freshState("p1"), signedIn: true, ...over };
}

describe("L3-WP19 characterization: the entitlement banner cases that stay", () => {
  it("lapsed_member_with_a_grant_is_red_with_renew", () => {
    const b = entitlementBanner(facts({ tier: "free", entitlement: "lapsed", hasGrant: true, hasSbt: true, s3: "settled" }));
    expect(b.show).toBe(true);
    expect(b.tone).toBe("danger");
    expect(b.text).toMatch(/^Membership lapsed\. Paid features are locked\./);
    expect(b.cta).toBe(true);
  });

  it("expired_claim_is_red_with_renew", () => {
    // A claim whose expiry is in the past (applyAuthStatus folds free + lapsed): a real lapse.
    const b = entitlementBanner(facts({ tier: "free", entitlement: "lapsed", authExpiresAt: "2001-01-01" }));
    expect(b.tone).toBe("danger");
    expect(b.cta).toBe(true);
  });

  it("active_hides_the_banner", () => {
    const b = entitlementBanner(facts({ tier: "commercial", entitlement: "active", hasGrant: true }));
    expect(b.show).toBe(false);
    expect(b.cta).toBe(false);
  });

  it("expiring_is_warn_with_renew", () => {
    const b = entitlementBanner(facts({ tier: "commercial", entitlement: "expiring", hasGrant: true }));
    expect(b.tone).toBe("warn");
    expect(b.cta).toBe(true);
  });

  it("grace_is_warn_without_a_call_to_action", () => {
    const b = entitlementBanner(facts({ tier: "commercial", entitlement: "grace", hasGrant: true }));
    expect(b.tone).toBe("warn");
    expect(b.text).toMatch(/offline/);
    expect(b.cta).toBe(false);
  });

  it("grant_in_flight_on_chain_is_neutral_activating", () => {
    const b = entitlementBanner(facts({ tier: "free", entitlement: "lapsed", membershipActivating: true }));
    expect(b.tone).toBe("neutral");
    expect(b.text).toMatch(/^Membership activating/);
    expect(b.cta).toBe(false);
  });
});
