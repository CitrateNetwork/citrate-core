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

/** GROW-S1b — the DGX join-code resolver (citrate-landing #45). Base for mint/resolve/jwks. */
export const JOIN_API = "https://citrate.ai/api/join";
/** The JWT issuer the resolver signs with (must match citrate-landing lib/join.ts JOIN_ISSUER). */
export const JOIN_ISSUER = "https://citrate.ai/join";
/** Opaque short-code shape — MUST match the resolver's alphabet + length (lib/join.ts CODE_RE): 8
 *  chars, unambiguous set (no 0/O/1/l/I). This is how we tell a `/join/<code>` from `/join/<clusterId>`. */
export const CODE_RE = /^[23456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz]{8}$/;

export interface JoinLinkParts {
  /** The cluster/group being joined. Omit for a general network invite (no specific cluster). */
  clusterId?: string;
  /** An opaque short code (`/join/<code>`) to be resolved via the DGX resolver. Mutually exclusive
   *  with the self-contained fields — when set, call `resolveJoinCode` to get the display parts. */
  code?: string;
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
    const by = u.searchParams.get("by");
    const c = u.searchParams.get("c");
    const g = u.searchParams.get("g");
    const ref = u.searchParams.get("ref");
    const first = seg.length ? decodeURIComponent(seg[0]) : "";
    // OPAQUE SHORT CODE: a single `/join/<code>` segment matching the resolver's code shape, with NO
    // self-contained display params → resolve it via `resolveJoinCode`. Otherwise it's a self-contained
    // link and the display params below carry everything.
    if (first && CODE_RE.test(first) && !by && !c && !g && !ref) {
      out.code = first;
      return out;
    }
    if (first) out.clusterId = first;
    if (by) out.inviterHandle = by;
    if (c) out.clusterName = c;
    if (g) out.goal = g;
    if (ref) out.inviter = ref;
  } catch {
    /* not a URL → empty parts (caller shows an honest "that doesn't look like a link") */
  }
  return out;
}

/** GROW-S1b — resolve an opaque short code via the DGX resolver and VERIFY the EdDSA-signed JWT
 *  (`sig`) against the published JWKS before trusting anything. Returns the display parts from the
 *  SIGNED payload (not the unsigned body) so a tampered/forged response is rejected. Throws on a bad
 *  signature, wrong issuer, expiry, or an unreachable/inactive resolver (503) — the caller then shows
 *  an honest error and the self-contained links still work. */
export async function resolveJoinCode(
  code: string,
  jwksOverride?: import("jose").JWTVerifyGetKey,
): Promise<JoinLinkParts> {
  const { jwtVerify, createRemoteJWKSet } = await import("jose");
  const res = await fetch(`${JOIN_API}/${encodeURIComponent(code)}`);
  if (!res.ok) throw new Error(`resolve ${code}: HTTP ${res.status}`);
  const body = (await res.json()) as { ok?: boolean; sig?: string };
  if (!body?.ok || !body.sig) throw new Error("resolve: no signed invite");
  // Production fetches + caches the published JWKS; tests inject a local key set (jose's own JWKS fetch
  // bypasses a global fetch mock). Either way jwtVerify checks the EdDSA sig, issuer, and expiry.
  const jwks = jwksOverride ?? createRemoteJWKSet(new URL(`${JOIN_API}/jwks`));
  const { payload } = await jwtVerify(body.sig, jwks, { issuer: JOIN_ISSUER });
  const p = payload as Record<string, unknown>;
  return {
    clusterId: p.cluster != null ? String(p.cluster) : undefined,
    clusterName: typeof p.clusterName === "string" ? p.clusterName : undefined,
    inviterHandle: typeof p.inviter === "string" ? p.inviter : undefined,
    goal: typeof p.goal === "string" ? p.goal : undefined,
  };
}

/** GROW-S1b — mint a short, opaque, attribution-private link via the resolver. Falls back to the
 *  self-contained `buildJoinLink` if the resolver is unavailable/inactive (503), so sharing always
 *  works; once the resolver is activated, links auto-upgrade to short codes. `inviterAddress` (the
 *  attribution) is sent to the resolver to store PRIVATE — it is never returned by resolve. */
export async function mintJoinLink(p: JoinLinkParts): Promise<string> {
  try {
    const res = await fetch(JOIN_API, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        clusterId: p.clusterId,
        clusterName: p.clusterName,
        inviter: p.inviterHandle,
        inviterAddress: p.inviter,
        goal: p.goal,
      }),
    });
    if (res.ok) {
      const b = (await res.json()) as { ok?: boolean; url?: string };
      if (b?.ok && b.url) return b.url;
    }
  } catch {
    /* resolver unreachable → self-contained fallback below */
  }
  return buildJoinLink(p);
}
