// The entitlement banner above every Shell surface, as a pure function of AppState so the
// copy and tone are unit-tested without rendering the Shell (v0.5.0-rc.5 FIX 1).
//
// "activating" (FIX 1c): the member's on-chain grant is in flight (SBT minted or stake
// attributed) but neither the `/userinfo` tier claim nor the full chain proof has landed
// yet. That is not a lapse, so it is shown as a neutral notice with no Renew button. It is
// also not "active": paid features stay locked until the claim or the full on-chain proof
// arrives (Rule 1).

export type BannerTone = "danger" | "warn" | "neutral";

export interface EntitlementBanner {
  show: boolean;
  tone: BannerTone;
  text: string;
  /** Show the Renew call to action. */
  cta: boolean;
}

export function entitlementBanner(s: { entitlement: string; membershipActivating?: boolean }): EntitlementBanner {
  if (s.entitlement === "active") return { show: false, tone: "neutral", text: "", cta: false };
  if (s.entitlement === "expiring") {
    return {
      show: true,
      tone: "warn",
      text: "Membership renews in 14 days, 2026-07-25. Auto-renewal is off.",
      cta: true,
    };
  }
  if (s.entitlement === "grace") {
    return {
      show: true,
      tone: "warn",
      text: "Cannot verify membership, offline. Paid features stay unlocked for 63 h more. Your node, local wallet, and local memory never lock.",
      cta: false,
    };
  }
  if (s.entitlement === "lapsed" && s.membershipActivating) {
    return {
      show: true,
      tone: "neutral",
      text: "Membership activating… Your grant is landing on chain. This updates on its own in a minute or two.",
      cta: false,
    };
  }
  if (s.entitlement === "lapsed") {
    return {
      show: true,
      tone: "danger",
      text: "Membership lapsed. Paid features are locked. Your node, local wallet, and local memory keep working, and your keys are yours.",
      cta: true,
    };
  }
  return { show: true, tone: "neutral", text: "", cta: false };
}

/** Theme tokens for each banner tone (background, border/dot, foreground). */
export const BANNER_TONE_TOKENS: Record<BannerTone, { bg: string; bd: string; fg: string }> = {
  danger: { bg: "var(--danger-bg)", bd: "var(--danger)", fg: "var(--danger)" },
  warn: { bg: "var(--warn-bg)", bd: "var(--warn)", fg: "var(--warn)" },
  neutral: { bg: "var(--info-bg)", bd: "var(--info)", fg: "var(--tx-1)" },
};
