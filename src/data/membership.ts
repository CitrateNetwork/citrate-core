// =====================================================================
// citrate-core: the membership price (C1-V V12)
//
// UI truth contract v1 §5.3: the membership price comes from the membership service, not a
// hard-coded string, and it buys membership, not SALT.
//
// Chain-team ruling (no waiver): core-membership does not serve a public price route yet, so the
// app states NO price. Every surface links to the membership site instead ("See membership
// pricing"), and Stripe checkout shows the exact charge before anything is paid. This module stays
// the single place that will read the price once core-membership ships the route (board item
// owned by DGX); until then `membershipPriceLabel()` returns null and callers show the link.
// =====================================================================

/**
 * The public membership page on citrate.ai, which states the current price. Not
 * membership.citrate.ai: that host describes the membership service and shows no price (#349 review).
 */
export const MEMBERSHIP_PRICING_URL = "https://citrate.ai/membership";

/** The link text every surface shows in place of a price. */
export const SEE_PRICING = "See membership pricing";

/**
 * The price as served by core-membership, or null while no price route exists. Callers render
 * SEE_PRICING linking to MEMBERSHIP_PRICING_URL when this is null; they never supply a number.
 */
export function membershipPriceLabel(): string | null {
  return null;
}

/** §5.3: what membership buys. */
export const MEMBERSHIP_BUYS = "Membership does not buy SALT.";
