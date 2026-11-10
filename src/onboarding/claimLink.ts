// PHONEPAY-S4 — the claim/onboard deep-link. A buyer who paid on the web/phone gets an
// email with a `citrate://claim?email=…&order=…` link (and an `https://citrate.ai/claim?…`
// fallback for a browser). Opening it lands them in app onboarding bound to the RIGHT
// identity. CRITICAL (adversarial A8): every field here is an untrusted DISPLAY HINT —
// it pre-fills which account to sign in with and lets the app flag a wrong-account
// mismatch. NOTHING in this link grants anything; a forged/edited link admits no one.
// The sole gate is the authority re-auth (OIDC) + the server-side reconciler matching
// sub↔order. A leaked/forwarded link is therefore not a bearer credential.

export interface ClaimLinkParts {
  /** The email the buyer paid with — a hint for "sign in with THIS account" + mismatch detection. */
  email?: string;
  /** An opaque order id hint (display/support only — never trusted for admission). */
  order?: string;
}

/** Parse a `citrate://claim?…` (or `https://citrate.ai/claim?…`) onboarding link. Returns
 *  null when the url is not claim-shaped (so join/invite links fall through untouched).
 *  Both fields are validated to a safe shape and treated as HINTS ONLY (see the note above). */
export function parseClaimLink(url: string): ClaimLinkParts | null {
  try {
    const u = new URL(url.includes("://") ? url : `https://${url}`);
    // Deep-link form is `citrate://claim?…` (host = the verb, as in `citrate://invite?…`); the
    // https fallback is `…/claim`.
    const verb = (u.host || u.hostname || "").toLowerCase();
    const firstSeg = u.pathname.replace(/^\/+/, "").split("/")[0]?.toLowerCase() ?? "";
    const isClaim = verb === "claim" || firstSeg === "claim";
    if (!isClaim) return null;
    const out: ClaimLinkParts = {};
    const email = u.searchParams.get("email")?.trim();
    // A validated email shape — a malformed hint is simply dropped, never shown raw.
    if (email && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) out.email = email;
    const order = (u.searchParams.get("order") ?? u.searchParams.get("order_id"))?.trim();
    // A conservative id charset — no spaces/markup can ride in as a "hint".
    if (order && /^[A-Za-z0-9_-]{1,64}$/.test(order)) out.order = order;
    return out;
  } catch {
    return null;
  }
}
