// GROW-S0 — the referral / cluster-invite link primitive (pure, testable).
//
// One shareable web link — `citrate.ai/join/<clusterId>?…` — that anyone can open in a normal browser
// (the cold-start entry point, unlike a `citrate://` deep-link blob that dies without the app). The
// link carries only DISPLAY data the inviter authorized (their chosen handle or a short address, the
// cluster name, the cluster goal) so the GROW-S1 web page can render a personalized CTA — "Dana
// invited you to Aperture" — with NO backend call. It is NOT the authorization: the actual join still
// routes through the server-blind relay and is ceremony-signed (the display params are advisory only,
// never trusted for admission — anti-spoofing).
//
// PROVISIONAL FORMAT (v1): self-contained query params so S0/S1 work with no join service. The GROW-S1b
// resolver will additionally issue short signed codes (`/join/<shortcode>`) and do referral attribution
// + rate-limiting server-side; this module is the client contract we reconcile with it. Keeping the
// payload to non-PII display fields is the dignity rule (D-7): a link never leaks more than a name.

/** The public join base. A real https URL (works in any browser), not a `citrate://` scheme. */
export const JOIN_BASE = "https://citrate.ai/join";

export interface JoinLinkParts {
  /** The cluster/group being joined. Omit for a general network invite (no specific cluster). */
  clusterId?: string;
  /** Human cluster name for the CTA (display only). */
  clusterName?: string;
  /** The cluster's goal/theme (storage | training | inference | dapps | …) for the CTA (display only). */
  goal?: string;
  /** The inviter's address — referral attribution (rewards). Display uses `by`, not this. */
  inviter?: string;
  /** The inviter's chosen display handle (verified @handle) if any; else a short address is shown. */
  inviterHandle?: string;
}

/** A short, non-lossy-for-display rendering of an address (the FULL address rides in `ref`). */
export function shortInviter(addr: string | null | undefined): string {
  const a = (addr || "").trim();
  if (!a) return "someone";
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a;
}

/** Build the shareable join link. Display params are URL-encoded; only fields that are present are
 *  added (a general invite has no cluster). `by` is the display name (handle or short address). */
export function buildJoinLink(p: JoinLinkParts): string {
  const path = p.clusterId ? `${JOIN_BASE}/${encodeURIComponent(p.clusterId)}` : JOIN_BASE;
  const q = new URLSearchParams();
  const by = p.inviterHandle?.trim() || (p.inviter ? shortInviter(p.inviter) : "");
  if (by) q.set("by", by);
  if (p.clusterName) q.set("c", p.clusterName);
  if (p.goal) q.set("g", p.goal);
  if (p.inviter) q.set("ref", p.inviter); // full address — attribution, not display
  const qs = q.toString();
  return qs ? `${path}?${qs}` : path;
}

/** Parse a join link back to its parts (the app's redeem side + tests). Tolerant of a bare base,
 *  a `citrate.ai/join/<id>` with no query, and extra unknown params. */
export function parseJoinLink(url: string): JoinLinkParts {
  const out: JoinLinkParts = {};
  try {
    const u = new URL(url.includes("://") ? url : `https://${url}`);
    const seg = u.pathname.replace(/^\/join\/?/, "").split("/").filter(Boolean);
    if (seg.length) out.clusterId = decodeURIComponent(seg[0]);
    const by = u.searchParams.get("by");
    const c = u.searchParams.get("c");
    const g = u.searchParams.get("g");
    const ref = u.searchParams.get("ref");
    if (by) out.inviterHandle = by;
    if (c) out.clusterName = c;
    if (g) out.goal = g;
    if (ref) out.inviter = ref;
  } catch {
    /* not a URL → empty parts (caller shows an honest "that doesn't look like a link") */
  }
  return out;
}
