// =====================================================================
// citrate-core: the membership price (C1-V V12)
//
// UI truth contract v1 §5.3: "$48 a year" comes from the membership service, not a hard-coded
// string, and it buys membership, not SALT.
//
// SOURCE NOTE (flagged in the C1-V PR): core-membership has no price endpoint yet. The amount it
// charges is its Stripe price (`STRIPE_PRICE_ID`, the $48/yr Pilot price, core-membership
// src/lib/stripe/client.ts), and Stripe checkout shows the member the exact charge before they pay.
// Until core-membership serves the price, this is the single place the app states it, in the same
// style as the chain config (src/chain.ts) and the address book; every surface reads it from here.
// When the service exposes the price, replace this constant with that read.
// =====================================================================

export const MEMBERSHIP_PRICE = {
  usd: 48,
  period: "year",
  source: "core-membership checkout (Stripe STRIPE_PRICE_ID); no price endpoint yet",
} as const;

/** "$48" */
export function membershipPrice(): string {
  return "$" + MEMBERSHIP_PRICE.usd;
}

/** "$48/year" */
export function membershipPricePerYear(): string {
  return membershipPrice() + "/" + MEMBERSHIP_PRICE.period;
}

/** §5.3: what the price buys. */
export const MEMBERSHIP_BUYS = "It buys membership. It does not buy SALT.";
