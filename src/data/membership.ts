// =====================================================================
// citrate-core: the membership price (C1-V V12)
//
// UI truth contract v1 §5.3: the membership price comes from the membership service, not a
// hard-coded string, and it buys membership, not SALT.
//
// C1-P: core-membership now serves the price at `{coreMembershipUrl}/api/membership/price`. The app
// reads it in Rust (`membership_price`; the route has no CORS) into AppState.membershipPrice and
// shows it only when it was read and its displayAmount matches its unitAmount. On a 503 or any
// error every surface keeps the "See membership pricing" link and shows no number.
// =====================================================================

/**
 * The public membership page on citrate.ai, which states the current price. Not
 * membership.citrate.ai: that host describes the membership service and shows no price (#349 review).
 */
export const MEMBERSHIP_PRICING_URL = "https://citrate.ai/membership";

/** The link text every surface shows in place of a price. */
export const SEE_PRICING = "See membership pricing";

/** The price as core-membership served it (amounts in minor units). */
export interface MembershipPrice {
  currency: string;
  unitAmount: number;
  interval: "year" | "month" | string;
  displayAmount: string;
}

/**
 * "$48 a year" from a read price, or null when none was read. Callers render SEE_PRICING linking
 * to MEMBERSHIP_PRICING_URL when this is null; they never supply a number.
 */
export function membershipPriceLabel(price: MembershipPrice | null | undefined): string | null {
  if (!price || !price.displayAmount || !(price.unitAmount > 0)) return null;
  const per = price.interval === "year" ? "a year" : price.interval === "month" ? "a month" : null;
  return per ? price.displayAmount + " " + per : null;
}

/** §5.3: what membership buys. */
export const MEMBERSHIP_BUYS = "Membership does not buy SALT.";
