// GROW-S0/S1 — invite-link helpers (pure, testable). The webview never builds, resolves or fetches a
// join link itself: every v2 link (`https://citrate.ai/join/<code>#K`) is minted, held and resolved
// in Rust (`bridge.join`), because the part after `#` is a one-time key. This module only tells what
// kind of string the member pasted or the OS handed us, and how a join-service error reads.
//
// Link kinds the app understands:
//   - v2: `https://citrate.ai/join/<code>[#K]`, `https://www.citrate.ai/join/<code>[#K]`,
//     `citrate://join/<code>[#K]`, or a bare `<code>[#K]`. Goes straight to Rust (pending_join_set).
//   - relay: `citrate://invite?g=…&t=…` (the app-only self-admit link). Redeemed as before.
//   - legacy-referral: the older display-only `…/join/<id>?by=&c=&g=` link people already posted.
//     Shown for context only; the app no longer builds that format.

/** The public join base (for display; Rust builds the share links). */
export const JOIN_BASE = "https://citrate.ai/join";

/** Short-code shape, the service's alphabet + length: 8 chars, no 0/O/1/l/I. */
export const CODE_RE = /^[23456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz]{8}$/;
/** The link key: 22 unpadded base64url characters (128 bits). */
const KEY_RE = /^[A-Za-z0-9_-]{22}$/;

/** Invite goals the join service accepts (Rust `JOIN_GOALS`), "general" first for the picker. */
export const JOIN_GOALS = ["general", "storage", "training", "inference", "dapps"] as const;
export type JoinGoalId = (typeof JOIN_GOALS)[number];

export interface JoinLinkParts {
  /** The cluster/group named by a legacy display link. */
  clusterId?: string;
  /** Human cluster name for the banner (display only). */
  clusterName?: string;
  /** The cluster's goal (display only). */
  goal?: string;
  /** The legacy link's referrer address (never displayed; links the app builds no longer carry it). */
  inviter?: string;
  /** The inviter's display handle. */
  inviterHandle?: string;
}

/** A short rendering of an address. */
export function shortInviter(addr: string | null | undefined): string {
  const a = (addr || "").trim();
  if (!a) return "someone";
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a;
}

/** Read a legacy display link for its display fields. Tolerant of a bare base and unknown params. */
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
    if (first) out.clusterId = first;
    if (by) out.inviterHandle = by;
    if (c) out.clusterName = c;
    if (g) out.goal = g;
    if (ref) out.inviter = ref;
  } catch {
    /* not a URL → empty parts */
  }
  return out;
}

export type InviteInput =
  | { kind: "v2"; hasKey: boolean }
  | { kind: "relay" }
  | { kind: "legacy-referral"; parts: JoinLinkParts }
  | { kind: "none" };

/** What a pasted or deep-linked string is. Never returns the code or key: a v2 string is handed to
 *  Rust as-is. A fragment that isn't a well-formed key still counts as v2 so Rust can refuse it with
 *  a clear error. */
export function classifyInviteInput(raw: string): InviteInput {
  const s = (raw || "").trim();
  if (!s || s.length > 2048) return { kind: "none" };
  const hash = s.indexOf("#");
  const pre = hash >= 0 ? s.slice(0, hash) : s;
  const frag = hash >= 0 ? s.slice(hash + 1) : "";
  const v2 = (code: string): InviteInput => (CODE_RE.test(code) ? { kind: "v2", hasKey: KEY_RE.test(frag) } : { kind: "none" });
  if (CODE_RE.test(pre)) return v2(pre);
  if (/^citrate:\/\/invite\?/i.test(s)) return /[?&]t=[^&#]+/.test(s) && /[?&]g=[^&#]+/.test(s) ? { kind: "relay" } : { kind: "none" };
  let u: URL;
  try {
    u = new URL(pre);
  } catch {
    return { kind: "none" };
  }
  const segs = u.pathname.split("/").filter(Boolean);
  const hasDisplayParams = ["by", "c", "g", "ref"].some((k) => u.searchParams.has(k));
  if (u.protocol === "citrate:" && u.host === "join") {
    if (segs.length !== 1) return { kind: "none" };
    if (hasDisplayParams) return { kind: "legacy-referral", parts: parseJoinLink(`https://citrate.ai/join/${segs[0]}${u.search}`) };
    return v2(segs[0]);
  }
  if (u.protocol === "https:" && (u.host === "citrate.ai" || u.host === "www.citrate.ai") && segs[0] === "join") {
    if (hasDisplayParams) return { kind: "legacy-referral", parts: parseJoinLink(pre) };
    if (segs.length !== 2) return { kind: "none" };
    return v2(segs[1]);
  }
  return { kind: "none" };
}

export type JoinErrorKind = "unavailable" | "nameNotAllowed" | "notSignedIn" | "notFound" | "rateLimited" | "other";

/** Classify an error from the Rust join commands by its safe Display text. */
export function joinErrorKind(e: unknown): JoinErrorKind {
  const m = e instanceof Error ? e.message : String(e ?? "");
  if (/temporarily unavailable|could not reach the invite service|timed out after/i.test(m)) return "unavailable";
  if (/isn't allowed on an invite/i.test(m)) return "nameNotAllowed";
  if (/sign in to create invite links|session expired or not authorized/i.test(m)) return "notSignedIn";
  if (/invite not found/i.test(m)) return "notFound";
  if (/too many invite requests/i.test(m)) return "rateLimited";
  return "other";
}

/** The member-facing sentence for a join error. */
export function joinErrorMessage(e: unknown): string {
  switch (joinErrorKind(e)) {
    case "unavailable":
      return "Short invite links are not available right now. Try again later.";
    case "nameNotAllowed": {
      // The rule lives in one place: Rust `JoinError::NameNotAllowed` (#353), which matches the
      // service. Show that text as-is (minus the "join:" prefix) instead of a second copy here.
      const m = (e instanceof Error ? e.message : String(e ?? "")).replace(/^join:\s*/, "").trim();
      return m ? m.charAt(0).toUpperCase() + m.slice(1) : "That group name isn't allowed on an invite.";
    }
    case "notSignedIn":
      return "Sign in to create invite links.";
    case "notFound":
      return "That invite has expired or was revoked. Ask for a new one.";
    case "rateLimited":
      return "Too many invite requests just now. Try again in a little while.";
    default: {
      const m = e instanceof Error ? e.message : String(e ?? "");
      return m.replace(/^join:\s*/, "") || "Something went wrong with that invite.";
    }
  }
}

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
    // Deep-link form is `citrate://claim?…` (host = the verb, like invites.rs's
    // `citrate://invite?…`); the https fallback is `…/claim`.
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
