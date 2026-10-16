// HUP-S2.3: live Sign-In with Ethereum from Hermes's managed browser: types, pure helpers and
// the bridge to core.
//
// A page in the managed browser asks for the member's address or a sign-in signature; the request
// waits in the Hermes sidecar. The main window only NAMES the request (its id); core reads the
// message, the asking page and the session taint itself, attests the page origin with its own read
// of the browser, and decides through the Signature Ceremony (src-tauri/src/web_signin.rs). Inside
// a live budget the sign-in is signed for the member and announced with a "Signed for you" notice;
// otherwise it opens the ordinary approval card. Nothing here signs.
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";
import type { CeremonyView } from "../bridge/types";

/** The Tauri event core emits after each automatic sign-in. */
export const AUTO_SIGNED_EVENT = "web-budget://auto-signed";

/** What core did with one request. */
export type SignInOutcome =
  | { outcome: "auto_signed"; origin: string; remaining: number; recordId: number; budgetId: number; delivered: boolean }
  | { outcome: "pending"; ceremony: CeremonyView; reason: string }
  | { outcome: "address_shared"; origin: string }
  | { outcome: "refused"; reason: string };

/** The "Signed for you" notice (core's event payload). */
export interface AutoSignedNotice {
  origin: string;
  budgetId: number;
  recordId: number;
  remaining: number;
}

export interface SignInApi {
  /** Decide one waiting request by id. */
  request(requestId: string): Promise<SignInOutcome>;
  /** The member approved the sign-in card: core signs and delivers it. True if the page got it. */
  approve(ceremonyId: string, rawAck: boolean): Promise<boolean>;
  /** The member declined the sign-in card. */
  reject(ceremonyId: string): Promise<void>;
}

export const tauriSignIn: SignInApi = {
  request: (requestId) => invoke<SignInOutcome>("web_signing_request", { requestId }),
  approve: (id, rawAck) => invoke<boolean>("web_signing_approve", { id, rawAck }),
  reject: (id) => invoke<void>("web_signing_reject", { id }),
};

/** The sign-in API in the desktop app; in web preview there is no managed browser. */
export const signInApi: { current: SignInApi | null } = {
  current: BRIDGE_MODE === "tauri" ? tauriSignIn : null,
};

const REQUEST_ID = /^signin-[0-9-]{1,56}$/;

/** A request id as the sidecar mints it. */
export function isSignInRequestId(id: unknown): id is string {
  return typeof id === "string" && REQUEST_ID.test(id);
}

/**
 * The ids in `ids` not handled yet, oldest first, and the updated seen set. Each request is
 * handed to core once; the seen set is bounded so a long session does not grow it forever.
 */
export function newSignInIds(ids: readonly string[], seen: ReadonlySet<string>, max = 64): { fresh: string[]; seen: Set<string> } {
  const fresh = ids.filter((id) => isSignInRequestId(id) && !seen.has(id));
  const next = new Set([...seen, ...fresh]);
  while (next.size > max) {
    const first = next.values().next().value;
    if (first === undefined) break;
    next.delete(first);
  }
  return { fresh, seen: next };
}

/** The notice text: what was signed where, and what is left. */
export function signedForYouText(n: AutoSignedNotice): string {
  const left = n.remaining === 1 ? "1 automatic sign-in left" : `${n.remaining} automatic sign-ins left`;
  return `Hermes signed you in to ${n.origin} with your wallet, inside the budget you set (${left}).`;
}

/** Parse core's event payload; anything malformed is dropped. */
export function parseAutoSignedNotice(raw: unknown): AutoSignedNotice | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const ok = (v: unknown) => typeof v === "number" && Number.isInteger(v) && v >= 0;
  if (typeof r.origin !== "string" || r.origin.length === 0 || r.origin.length > 300) return null;
  if (!ok(r.budgetId) || !ok(r.recordId) || !ok(r.remaining)) return null;
  return { origin: r.origin, budgetId: r.budgetId as number, recordId: r.recordId as number, remaining: r.remaining as number };
}
