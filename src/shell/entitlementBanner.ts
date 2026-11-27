// The entitlement banner above every Shell surface, as a pure function of AppState so the
// copy and tone are unit-tested without rendering the Shell (v0.5.0-rc.5 FIX 1).
//
// "activating" (FIX 1c): the member's on-chain grant is in flight (SBT minted or stake
// attributed) but neither the `/userinfo` tier claim nor the full chain proof has landed
// yet. That is not a lapse, so it is shown as a neutral notice with no Renew button. It is
// also not "active": paid features stay locked until the claim or the full on-chain proof
// arrives (Rule 1).
//
// L3-WP19 (owner report 2026-11-27): a lapsed entitlement is three different people, and only
// one of them is told to renew (src/shell/actions/l3/membership.ts names the data sources):
//   - a free member who never held a membership: Citrate green, "Upgrade" (the guarded checkout);
//   - someone whose checkout is waiting on its grant: never red. What the grant still needs comes
//     first: link the wallet (the grant pays the linked wallet and is refused without one), then
//     verify identity (grants are held until it is done), else "activating", then a neutral
//     "still activating" with the support address once one reconciler sweep has passed;
//   - a member whose membership ran out: red, "Renew".

import { GRANT_DELAYED_AFTER_MS } from "../onboarding/grantDelay";
import { SUPPORT_EMAIL } from "./UpdateBanner";
import { checkoutAwaiting, isFreeMember, walletLinkNeeded, type BannerAction, type MembershipFacts } from "./actions/l3/membership";

export type BannerTone = "danger" | "warn" | "neutral" | "brand";

export interface EntitlementBanner {
  show: boolean;
  tone: BannerTone;
  text: string;
  /** Show the call to action. */
  cta: boolean;
  /** What the call to action does, or null with no call to action. */
  action: BannerAction | null;
  /** The call to action's label ("" with none). */
  ctaLabel: string;
}

const NONE = { cta: false, action: null, ctaLabel: "" } as const;

export function entitlementBanner(s: MembershipFacts, now: number = Date.now()): EntitlementBanner {
  if (s.entitlement === "active") return { show: false, tone: "neutral", text: "", ...NONE };
  if (s.entitlement === "expiring") {
    return {
      show: true,
      tone: "warn",
      text: "Membership renews in 14 days, 2026-07-25. Auto-renewal is off.",
      cta: true,
      action: "renew",
      ctaLabel: "Renew",
    };
  }
  if (s.entitlement === "grace") {
    return {
      show: true,
      tone: "warn",
      text: "Cannot verify membership, offline. Paid features stay unlocked for 63 h more. Your node, local wallet, and local memory never lock.",
      ...NONE,
    };
  }
  if (s.entitlement === "lapsed" && s.membershipActivating) {
    return {
      show: true,
      tone: "neutral",
      text: "Membership activating… Your grant is landing on chain. This updates on its own in a minute or two.",
      ...NONE,
    };
  }
  if (s.entitlement === "lapsed" && checkoutAwaiting(s)) {
    if (walletLinkNeeded(s)) {
      return {
        show: true,
        tone: "brand",
        text: "If you completed checkout, link your wallet to activate your membership.",
        cta: true,
        action: "link",
        ctaLabel: "Link wallet",
      };
    }
    if (s.s2 === "none" || s.s2 === "failed") {
      return {
        show: true,
        tone: "brand",
        text: "If you completed checkout, verify your identity to activate your membership.",
        cta: true,
        action: "verify",
        ctaLabel: "Verify identity",
      };
    }
    if (s.s2 === "pending" || s.s2 === "review") {
      return {
        show: true,
        tone: "neutral",
        text: "Identity verification in progress. If you completed checkout, your membership activates once it is approved.",
        ...NONE,
      };
    }
    // Verified. A checkout opened in an earlier session (only `s3: "paying"` survives a restart)
    // has already waited longer than one sweep.
    const waited = typeof s.membershipCheckoutAt === "number" ? now - s.membershipCheckoutAt : Infinity;
    if (waited >= GRANT_DELAYED_AFTER_MS) {
      return {
        show: true,
        tone: "neutral",
        text: `Still activating. If you completed checkout, we're on it and it finishes on its own. Questions? Email ${SUPPORT_EMAIL}.`,
        ...NONE,
      };
    }
    return {
      show: true,
      tone: "neutral",
      text: "Membership activating. If you completed checkout, it activates on its own in a few minutes.",
      ...NONE,
    };
  }
  if (isFreeMember(s)) {
    return {
      show: true,
      tone: "brand",
      text: "You're on the free plan. Become a member to unlock paid features.",
      cta: true,
      action: "upgrade",
      ctaLabel: "Upgrade",
    };
  }
  if (s.entitlement === "lapsed") {
    return {
      show: true,
      tone: "danger",
      text: "Membership lapsed. Paid features are locked. Your node, local wallet, and local memory keep working, and your keys are yours.",
      cta: true,
      action: "renew",
      ctaLabel: "Renew",
    };
  }
  return { show: true, tone: "neutral", text: "", ...NONE };
}

/** Theme tokens for each banner tone (background, border/dot, foreground). */
export const BANNER_TONE_TOKENS: Record<BannerTone, { bg: string; bd: string; fg: string }> = {
  danger: { bg: "var(--danger-bg)", bd: "var(--danger)", fg: "var(--danger)" },
  warn: { bg: "var(--warn-bg)", bd: "var(--warn)", fg: "var(--warn)" },
  neutral: { bg: "var(--info-bg)", bd: "var(--info)", fg: "var(--tx-1)" },
  // Citrate green (tokens.css): the wash, the accent, and the AA accent text; themed light and dark.
  brand: { bg: "var(--accent-wash)", bd: "var(--accent)", fg: "var(--accent-text)" },
};
