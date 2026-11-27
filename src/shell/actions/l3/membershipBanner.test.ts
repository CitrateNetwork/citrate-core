// L3-WP19 (owner report 2026-11-27): "All free members get a big red label that tells them to
// renew, then when they do renew the red label in the node doesn't leave ... new users should be
// told to upgrade and it shouldn't be red or orange, it should be Citrate green."
//
// The banner as a pure function of AppState, for each person a lapsed entitlement can be:
//   - a free member who never held a membership: Citrate green, "Upgrade" (the guarded checkout);
//   - someone whose checkout waits on its grant: link the wallet first, then verify identity, then
//     activating, then "still activating" with the support address; never red or orange;
//   - a member whose membership ran out: red, "Renew" (pinned in L3-MEMBANNER).
import { describe, it, expect } from "vitest";
import { entitlementBanner, BANNER_TONE_TOKENS } from "../../entitlementBanner";
import { freshState, type AppState } from "../../state";

const NOW = Date.parse("2026-11-27T12:00:00Z");
const MIN = 60_000;

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
    membershipActivating: false,
    ...over,
  } as AppState;
}

/** The same member a moment after opening checkout in this session. */
function justCheckedOut(over: Partial<AppState> = {}): AppState {
  return freeMember({ s3: "paying", membershipCheckoutAt: NOW - MIN, ...over } as Partial<AppState>);
}

/** No em-dash, no token or reward wording, no promise of future benefits. */
function assertCopyHygiene(text: string): void {
  expect(text).not.toMatch(/[—–]/);
  expect(text).not.toMatch(/\bSALT\b|token|reward|earn|airdrop|yield|profit/i);
  expect(text).not.toMatch(/will (get|receive)|future benefit/i);
}

describe("free member (never held a membership)", () => {
  it("is told to upgrade in Citrate green, not to renew in red", () => {
    const b = entitlementBanner(freeMember(), NOW);
    expect(b.show).toBe(true);
    expect(b.tone).toBe("brand");
    expect(b.text).toBe("You're on the free plan. Become a member to unlock paid features.");
    expect(b.cta).toBe(true);
    expect(b.action).toBe("upgrade");
    expect(b.ctaLabel).toBe("Upgrade");
    expect(b.text).not.toMatch(/lapsed|renew/i);
    assertCopyHygiene(b.text);
  });

  it("brand is the Citrate green accent tokens, never the danger or warn colors", () => {
    expect(BANNER_TONE_TOKENS.brand).toEqual({ bg: "var(--accent-wash)", bd: "var(--accent)", fg: "var(--accent-text)" });
    expect(JSON.stringify(BANNER_TONE_TOKENS.brand)).not.toMatch(/danger|warn/);
  });

  it("any past-membership evidence keeps the renewal path", () => {
    for (const over of [{ hasGrant: true }, { hasSbt: true }, { s3: "settled" }, { authExpiresAt: "2001-01-01" }] as Partial<AppState>[]) {
      const b = entitlementBanner(freeMember(over), NOW);
      expect(b.tone, JSON.stringify(over)).toBe("danger");
      expect(b.action, JSON.stringify(over)).toBe("renew");
    }
  });
});

describe("a checkout waiting on its grant (never red or orange)", () => {
  it("the wallet is not linked yet: link it first (the grant pays the linked wallet)", () => {
    const b = entitlementBanner(justCheckedOut({ custodyAddr: "0xdef" }), NOW);
    expect(b.tone).toBe("brand");
    expect(b.text).toBe("If you completed checkout, link your wallet to activate your membership.");
    expect(b.action).toBe("link");
    expect(b.ctaLabel).toBe("Link wallet");
    assertCopyHygiene(b.text);
  });

  it("a wallet not read yet is not called unlinked (no link prompt on an unknown)", () => {
    const b = entitlementBanner(justCheckedOut({ custodyAddr: "" }), NOW);
    expect(b.action).not.toBe("link");
  });

  it("linked but identity not verified: verify identity", () => {
    for (const s2 of ["none", "failed"] as const) {
      const b = entitlementBanner(justCheckedOut({ s2 }), NOW);
      expect(b.tone, s2).toBe("brand");
      expect(b.text, s2).toBe("If you completed checkout, verify your identity to activate your membership.");
      expect(b.action, s2).toBe("verify");
      expect(b.ctaLabel, s2).toBe("Verify identity");
    }
  });

  it("identity verification in progress: neutral, no button", () => {
    for (const s2 of ["pending", "review"] as const) {
      const b = entitlementBanner(justCheckedOut({ s2 }), NOW);
      expect(b.tone, s2).toBe("neutral");
      expect(b.text, s2).toMatch(/^Identity verification in progress\./);
      expect(b.cta, s2).toBe(false);
    }
  });

  it("linked and verified, just opened: neutral activating", () => {
    const b = entitlementBanner(justCheckedOut({ s2: "verified" }), NOW);
    expect(b.tone).toBe("neutral");
    expect(b.text).toBe("Membership activating. If you completed checkout, it activates on its own in a few minutes.");
    expect(b.cta).toBe(false);
    assertCopyHygiene(b.text);
  });

  it("after one reconciler sweep: still activating, with the support address, not red", () => {
    const b = entitlementBanner(justCheckedOut({ s2: "verified", membershipCheckoutAt: NOW - 11 * MIN } as Partial<AppState>), NOW);
    expect(b.tone).toBe("neutral");
    expect(b.text).toMatch(/^Still activating\./);
    expect(b.text).toContain("support@citrate.ai");
    expect(b.cta).toBe(false);
    assertCopyHygiene(b.text);
  });

  it("a checkout opened in an earlier session (only s3 paying survives) counts as a long wait", () => {
    const b = entitlementBanner(freeMember({ s3: "paying", s2: "verified" }), NOW);
    expect(b.text).toMatch(/^Still activating\./);
  });

  it("a lapsed member who just opened renewal is not shown the red lapse", () => {
    const b = entitlementBanner(justCheckedOut({ hasGrant: true, hasSbt: true, s3: "settled", s2: "verified" }), NOW);
    expect(b.tone).toBe("neutral");
    expect(b.action).toBeNull();
  });

  it("no combination of link, identity and wait is ever danger or warn", () => {
    for (const custodyAddr of ["0xabc", "0xdef", ""])
      for (const s2 of ["none", "pending", "verified", "failed", "review"] as const)
        for (const at of [NOW, NOW - 5 * MIN, NOW - 60 * MIN, null])
          for (const s3 of ["paying", "idle"] as const) {
            if (s3 === "idle" && at === null) continue; // not a checkout
            const b = entitlementBanner(freeMember({ custodyAddr, s2, s3, membershipCheckoutAt: at } as Partial<AppState>), NOW);
            const key = JSON.stringify({ custodyAddr, s2, at, s3 });
            expect(b.show, key).toBe(true);
            expect(["brand", "neutral"], key).toContain(b.tone);
            expect(b.action === "renew" || b.action === "upgrade", key).toBe(false);
            assertCopyHygiene(b.text);
          }
  });
});
