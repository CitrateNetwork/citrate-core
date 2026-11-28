// L0-WP07b: the CORE-A3 auth actions (sign-in, the claim fold, the entitlement re-checks, sign-out,
// KYC) and the launch-time session resume, moved out of the Store class body into the L3
// membership and onboarding lane. Each Store method of the same name is a one-line delegate to the
// function here, so every call site (`store.authLogin()`) is unchanged. The code is moved as it
// was: `this` became `store`, and the Store's private members are reached through
// storeInternals(store). The pure claim helpers these actions use moved with them from the top of
// store.ts, which re-exports them.
//
// Data sources (Rule 7): the identity authority's claim through bridge.auth (status, refresh,
// login, userinfo, cancelLogin, logout: the Rust OIDC session and the refresh token sealed in the
// custody vault), the live /userinfo re-check (readLiveClaim, actions/l3/membership.ts), and the
// on-chain grant (bridge.membership.grantStatus, chain 40204). Unchanged.

import type { GrantStatus } from "../../../bridge/domains";
import { BRIDGE_MODE } from "../../../bridge/mode";
import { bridge } from "../../../bridge";
import { isGrantOnChain } from "../../grantReads";
import type { AppState } from "../../state";
import type { Store } from "../../store";
import { storeInternals } from "../../storeInternals";
import { readLiveClaim } from "./membership";

/**
 * A3-03 — is an entitlement `expiresAt` claim in the past (or anomalous)?
 * - ABSENT (null/undefined/empty) → NOT expired: the authority legitimately may
 *   not send an expiry; the hard id_token `exp` gate in Rust still bounds the
 *   session.
 * - PRESENT but UNPARSEABLE → treated as EXPIRED (fail-closed): a malformed
 *   expiry on a T1 gating surface is an anomaly, not a licence to keep the tier.
 * - PRESENT + in the past → expired.
 * Accepts an ISO date/datetime OR a unix-epoch numeric string. The authority nests
 * the entitlement expiry as epoch-MILLISECONDS (citrate-identity: `new Date(row).
 * getTime()`), and the Rust seam forwards it verbatim as a digit string — so a bare
 * `Number(x) * 1000` (assuming seconds) mis-scaled a real ms expiry to ~year 58000
 * and NEVER expired. Disambiguate by magnitude: a value already in ms range
 * (>= 1e12, i.e. any time after 2001) is used as-is; a smaller value is seconds.
 */
export function isExpiredClaim(expiresAt: string | null | undefined): boolean {
  if (!expiresAt) return false;
  let ms: number;
  if (/^\d+$/.test(expiresAt)) {
    const n = Number(expiresAt);
    ms = n >= 1e12 ? n : n * 1000; // ms already vs unix-seconds
  } else {
    ms = Date.parse(expiresAt); // ISO date/datetime
  }
  if (!Number.isFinite(ms)) return true; // fail-closed on an unparseable value
  return ms < Date.now();
}

/**
 * Membership-renew SAFEGUARD (Luke, 2026-09-11). A paid member reopened the app and
 * was wrongly shown "renew": the `/userinfo` entitlement re-check on reopen came back
 * THIN for their session (signed in, but no tier claim), so `applyAuthStatus` folded
 * `tier:free + entitlement:lapsed` — even though their on-chain membership SBT and
 * staked grant were fully present. The on-chain SBT is the AUTHORITATIVE membership
 * proof (the `/userinfo` entitlement is a *derived mirror* of it, per
 * citrate-identity/src/entitlements.ts), so a real member must never be pushed to
 * renew because of a transient claim.
 *
 * Returns a patch restoring an ACTIVE paid membership from the on-chain grant, or
 * `null`. Regression guards (all must hold):
 *  - the current entitlement is `lapsed` — nothing to correct otherwise;
 *  - the lapse is NOT from an explicit PAST expiry (`isExpiredClaim(authExpiresAt)`):
 *    a genuinely elapsed year is a REAL lapse and MUST still show renew — this only
 *    rescues the missing/thin-claim case;
 *  - the grant is genuinely on chain AND the SBT is minted (`isGrantOnChain && hasSbt`)
 *    — a live 40204 read, never a client assertion, so a never-paid free account
 *    (no SBT) is never elevated.
 * The restored tier is the paid-member baseline `commercial` (rank > free); a
 * commercial.kyc upgrade still arrives from the next live claim.
 */
export function entitlementSafeguardFromChain(
  st: { tier: string; entitlement: string; authExpiresAt: string | null },
  g: GrantStatus,
): Partial<AppState> | null {
  if (st.entitlement !== "lapsed") return null;
  if (isExpiredClaim(st.authExpiresAt)) return null; // a real expiry — honor the lapse
  if (!isGrantOnChain(g) || !g.hasSbt) return null; // no on-chain membership proof
  return { tier: "commercial", entitlement: "active" };
}

/**
 * v0.5.0-rc.5 FIX 1c: is the member's grant visibly IN FLIGHT on chain while the
 * `/userinfo` tier claim has not caught up?
 *
 * True when the entitlement reads `lapsed` WITHOUT a genuine past expiry, the full
 * on-chain proof (`isGrantOnChain && hasSbt`) is NOT yet there (that case is
 * `entitlementSafeguardFromChain`, which restores `active`), but the chain already
 * shows part of the grant: the member SBT is minted or stake is attributed. The banner
 * then says "activating" instead of the red lapse.
 *
 * Honesty (Rule 1): this never unlocks anything. It only changes the banner from
 * "lapsed" to a neutral notice; `active` still needs the claim or the full chain proof.
 * A malformed stake value fails closed (false).
 */
export function membershipActivatingFromChain(
  st: { entitlement: string; authExpiresAt: string | null },
  g: { attributedStakeWei: string; hasSbt: boolean },
): boolean {
  if (st.entitlement !== "lapsed") return false;
  if (isExpiredClaim(st.authExpiresAt)) return false; // a real expiry is a real lapse
  if (isGrantOnChain(g) && g.hasSbt) return false; // full proof: the safeguard restores active
  if (g.hasSbt) return true;
  try {
    return BigInt(g.attributedStakeWei) > 0n;
  } catch {
    return false;
  }
}

/** FIX 1a: the post-checkout entitlement poll cadence. */
export const ENTITLEMENT_POLL_MS = 10_000;
/**
 * FIX 1a: the poll's bound. Must outlast one 10-minute core-membership grant reconcile
 * (`/api/cron/reconcile-grants`), so a grant the webhook missed is still picked up.
 */
export const ENTITLEMENT_POLL_MAX_MS = 15 * 60_000;
/** FIX 1b: at most one focus/visibility entitlement re-check per 30 s. */
export const FOCUS_RECHECK_MIN_MS = 30_000;

/**
 * Derive a display name + initials from an email local-part. The authority
 * issues NO display-name claim, so `larry@citrate.ai` → { name: "Larry",
 * initials: "LA" } and `ada.lovelace@x.io` → { name: "Ada Lovelace", initials:
 * "AL" }. Used by applyAuthStatus to render a real signed-in user's identity;
 * exported so the derivation is unit-tested independently of the auth plumbing.
 */
export function deriveIdentityFromEmail(email: string): { name: string; initials: string } {
  const local = email.split("@")[0];
  const words = local.split(/[._+-]+/).filter(Boolean);
  const name = words.map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" ") || email;
  const fromWords = words.length > 1 ? words.map((w) => w.charAt(0)).join("") : local.slice(0, 2);
  const initials = (fromWords || local.slice(0, 2)).slice(0, 2).toUpperCase();
  return { name, initials };
}

/**
 * Pull the live claim-derived auth status from the bridge and fold it into the
 * entitlement engine (tier/org/role/entitlement) + the KYC S2 state. In a
 * Tauri build this reads the real /userinfo entitlement claim; in web-dev it
 * reads the sim persona. Never throws — an unavailable/failed read leaves the
 * prior state untouched (honest).
 */
export async function refreshAuth(store: Store): Promise<void> {
  const internals = storeInternals(store);
  try {
    const st = await bridge.auth.status();
    internals.applyAuthStatus(st);
  } catch {
    /* honest no-op: the auth domain reported unavailable / not signed in */
  }
}

/**
 * Re-establish the signed-in session on LAUNCH from the vaulted OIDC refresh
 * token (A3), so a restart keeps the paid tier instead of dropping the member to
 * signed-out → public/free. `bridge.auth.status()` reads only the in-memory
 * session, which is empty on a fresh process; `bridge.auth.refresh()` silently
 * mints a fresh session from the refresh token sealed in the custody vault. That
 * read REQUIRES an unlocked vault (the token is a custody slot), so this must run
 * AFTER `custodyEnsureUnlocked`. A never-signed-in user (no token) or a declined
 * refresh fails gracefully and stays honestly signed out. After a successful
 * refresh we re-check the LIVE entitlement (`authUserinfo`) — the federation RP
 * rule — so the current tier (e.g. a just-raised membership) is folded, not a
 * stale claim.
 */
export async function resumeSession(store: Store): Promise<void> {
  const internals = storeInternals(store);
  let refreshed = false;
  try {
    const st = await bridge.auth.refresh();
    internals.applyAuthStatus(st);
    refreshed = true;
  } catch {
    // No vaulted refresh token (never signed in) or the refresh was declined —
    // stay signed out honestly rather than fabricate a session.
  }
  // Live /userinfo re-check once the session is back, so the freshest entitlement
  // (tier/expiry) is folded. Skipped when there is no session to re-check.
  // WRAPPED: a rejection here must not skip the chain reconcile below. These are
  // independent — the grant is a permissionless chain read that does not need a
  // live session, and letting an auth hiccup suppress it is exactly how a paid
  // member stays stranded (observed 2026-08-05).
  try {
    if (refreshed) await store.authUserinfo();
    else await store.refreshAuth();
  } catch {
    /* honest no-op: entitlement display may lag; the chain read below still runs */
  }
  // The onboarding step state is LOCAL; the grant is on CHAIN. Re-sync them.
  await internals.reconcileOnboardingFromChain();
}

/** Fold a claim-derived AuthStatus into AppState (the entitlement engine). */
export function applyAuthStatus(
  store: Store,
  st: {
    signedIn: boolean;
    sub?: string | null;
    email?: string | null;
    walletAddr?: string | null;
    tier: string | null;
    org: string | null;
    role: string | null;
    kycStatus: string | null;
    expiresAt?: string | null;
  },
): void {
  if (!st.signedIn) return;
  const patch: Partial<AppState> = {};
  // Fold the REAL identity so no prototype persona ever surfaces to a signed-in
  // user (Rule 1). The authority issues no display-name claim, so derive a
  // display name + initials from the email local part.
  patch.signedIn = true;
  patch.authSub = st.sub ?? null;
  patch.authEmail = st.email ?? null;
  if (st.email) {
    const d = deriveIdentityFromEmail(st.email);
    patch.authName = d.name;
    patch.authInitials = d.initials;
  }
  // Q-A.4b item 10 — fold the REAL wallet only from a genuine wallet_address
  // claim, and RECORD that provenance. A signed-in user with no wallet claim
  // keeps walletFromClaim=false, so the S4 onboarding card shows "—"/pending
  // rather than the fabricated persona `makeAddr` hash (Rule 1).
  if (st.walletAddr) {
    patch.walletAddr = st.walletAddr;
    patch.walletFromClaim = true;
  } else {
    patch.walletFromClaim = false;
  }
  // BC-6.3: fold the REAL entitlement expiry verbatim (the Settings billing card
  // renders THIS, an honest "—" when absent — never a hardcoded date). Folded in
  // every branch below, including the expired one (it explains WHY the tier
  // lapsed). `??` normalizes undefined → null.
  patch.authExpiresAt = st.expiresAt ?? null;
  // A3-03: enforce the entitlement expiry at the DECISION point — a claim whose
  // `expiresAt` is in the past is downgraded to the free tier + lapsed, so no
  // gated surface stays unlocked on a stale claim. A valid future expiry keeps
  // the claimed tier and marks the entitlement active.
  if (isExpiredClaim(st.expiresAt)) {
    patch.tier = "free";
    patch.entitlement = "lapsed";
  } else if (st.tier) {
    patch.tier = st.tier;
    patch.entitlement = "active";
    patch.membershipActivating = false; // the claim landed: nothing left to activate
    patch.membershipCheckoutAt = null; // L3-WP19: and no checkout left waiting
  } else {
    // Signed in but the authority granted NO tier — fail closed to free so an
    // ungranted session can never inherit a prior session's persisted paid
    // tier (tier/entitlement are persisted keys).
    patch.tier = "free";
    patch.entitlement = "lapsed";
  }
  patch.org = st.org;
  // The authority may omit the citrate_role claim for a plain member; a
  // signed-in paid account is a "member" by default (never the persona's role).
  patch.citrateRole = st.role || "member";
  // ALF cooperative membership is a CLAIM (DGX_HANDOFF §4.3): alf_member is a
  // citrate_role minted by the authority, never tier-derived. Gates the ALF surface.
  patch.alfMember = patch.citrateRole === "alf_member";
  // KYC claim → the S2 seam's five states (none/pending/verified/failed/review).
  if (st.kycStatus === "verified") patch.s2 = "verified";
  else if (st.kycStatus === "pending") patch.s2 = "pending";
  else if (st.kycStatus === "failed") patch.s2 = "failed";
  else if (st.kycStatus === "review") patch.s2 = "review";
  store.setState(patch);
}

/** Run the real loopback-PKCE sign-in (Tauri), then fold in the claim. */
export async function authLogin(store: Store): Promise<void> {
  const internals = storeInternals(store);
  const st = await bridge.auth.login();
  internals.applyAuthStatus(st);
  // The login result may carry only id_token claims (no email/wallet). Fold
  // the live /userinfo immediately so the real identity (email/wallet/tier)
  // is complete on the first render — never a persona fallback for a signed-in
  // user. Best-effort: keep the login-folded state if /userinfo is unavailable.
  try {
    await store.authUserinfo();
  } catch {
    /* honest no-op — the login-folded claim stands */
  }
  store.save();
}

/**
 * rc.5 FIX 2: abort the in-flight browser sign-in (Cancel while "Waiting for your
 * browser"). The pending `authLogin` then rejects with "sign-in cancelled". Never
 * throws: a failed cancel leaves the 300 s callback timeout as the backstop.
 */
export async function cancelSignIn(_store: Store): Promise<void> {
  try {
    await bridge.auth.cancelLogin();
  } catch {
    /* honest no-op: the Rust wait still ends at its timeout */
  }
}

/** Live /userinfo entitlement re-check, folded into the entitlement engine. */
export async function authUserinfo(store: Store): Promise<void> {
  const internals = storeInternals(store);
  const st = await bridge.auth.userinfo();
  internals.applyAuthStatus(st);
}

/**
 * v0.5.0-rc.5 FIX 1: re-read the entitlement from BOTH sources and fold them in one
 * synchronous step (no lapsed flash between the claim and the chain read):
 *  - the live `/userinfo` claim (`applyAuthStatus`);
 *  - while the claim does not read active, the on-chain grant (Tauri, with a wallet):
 *    the full proof restores `active` via `entitlementSafeguardFromChain` (the same
 *    rule the launch reconcile uses); partial evidence marks "activating".
 * A chain read failure folds the claim alone (honest). Throws only if `/userinfo`
 * itself fails, so callers decide whether that is fatal.
 */
export async function refreshEntitlement(store: Store): Promise<void> {
  const internals = storeInternals(store);
  // L3-WP19: a 401 (the hour-long access token expired on a node left open) renews the session
  // from the vaulted refresh token and reads again, instead of failing every poll until restart.
  const st = await readLiveClaim(bridge.auth, internals.claimSession);
  let grant: GrantStatus | null = null;
  const member = st.walletAddr || store.state.walletAddr;
  const claimActive = !!st.tier && !isExpiredClaim(st.expiresAt);
  if (BRIDGE_MODE === "tauri" && member && !claimActive) {
    try {
      grant = await bridge.membership.grantStatus(member);
    } catch {
      grant = null; // transient RPC error: fold the claim alone, never fabricate
    }
  }
  // L3-WP19: signed out while the chain was read: fold nothing (never re-sign the member in).
  if (!store.state.signedIn) return;
  internals.applyAuthStatus(st);
  if (!grant) return;
  const guard = entitlementSafeguardFromChain(store.state, grant);
  if (guard) {
    store.setState({ ...guard, membershipActivating: false });
    store.save();
    return;
  }
  const activating = membershipActivatingFromChain(store.state, grant);
  if (activating !== store.state.membershipActivating) store.setState({ membershipActivating: activating });
}

/**
 * FIX 1a: after a membership checkout opens (from ANY entry point: onboarding S3,
 * Settings renew, the lapsed banner), poll the entitlement every ~10 s until it reads
 * active, for at most ~15 min. Single instance: a second checkout only extends the
 * window. Cancelled on sign-out. Tauri only (the sim has no server to wait for).
 */
export function startEntitlementPoll(store: Store): void {
  const internals = storeInternals(store);
  if (BRIDGE_MODE !== "tauri") return;
  internals.entitlementPollDeadline = Date.now() + ENTITLEMENT_POLL_MAX_MS;
  if (internals.entitlementPollTimer) return;
  internals.entitlementPollTimer = setTimeout(() => void internals.entitlementPollTick(), ENTITLEMENT_POLL_MS);
}

/** FIX 1a: is the post-checkout entitlement poll running? */
export function entitlementPollActive(store: Store): boolean {
  const internals = storeInternals(store);
  return internals.entitlementPollTimer !== null;
}

/** FIX 1a: cancel the post-checkout entitlement poll (sign-out). */
export function stopEntitlementPoll(store: Store): void {
  const internals = storeInternals(store);
  if (internals.entitlementPollTimer) clearTimeout(internals.entitlementPollTimer);
  internals.entitlementPollTimer = null;
}

export async function entitlementPollTick(store: Store): Promise<void> {
  const internals = storeInternals(store);
  if (!store.state.signedIn) {
    store.stopEntitlementPoll();
    return;
  }
  try {
    await store.refreshEntitlement();
  } catch {
    /* transient: keep polling until the bound; never fabricate */
  }
  // Cancelled (sign-out) while the read was in flight.
  if (internals.entitlementPollTimer === null || !store.state.signedIn) {
    internals.entitlementPollTimer = null;
    return;
  }
  const done = store.state.entitlement === "active" && !store.state.membershipActivating;
  if (done || Date.now() >= internals.entitlementPollDeadline) {
    internals.entitlementPollTimer = null;
    return;
  }
  internals.entitlementPollTimer = setTimeout(() => void internals.entitlementPollTick(), ENTITLEMENT_POLL_MS);
}

/**
 * FIX 1b: re-check the entitlement when the window regains focus or becomes visible
 * (the member coming back from the browser checkout). Throttled to one re-check per
 * 30 s and only while signed in. Returns a function that removes the listeners.
 */
export function installEntitlementFocusRecheck(store: Store): () => void {
  const internals = storeInternals(store);
  if (typeof window === "undefined" || typeof document === "undefined") return () => {};
  const onFocus = () => void internals.recheckEntitlementOnFocus();
  const onVisibility = () => {
    if (document.visibilityState === "visible") void internals.recheckEntitlementOnFocus();
  };
  window.addEventListener("focus", onFocus);
  document.addEventListener("visibilitychange", onVisibility);
  return () => {
    window.removeEventListener("focus", onFocus);
    document.removeEventListener("visibilitychange", onVisibility);
  };
}

export async function recheckEntitlementOnFocus(store: Store): Promise<void> {
  const internals = storeInternals(store);
  if (!store.state.signedIn) return;
  const now = Date.now();
  if (now - internals.lastFocusRecheckAt < FOCUS_RECHECK_MIN_MS) return;
  internals.lastFocusRecheckAt = now;
  try {
    await store.refreshEntitlement();
  } catch {
    /* honest no-op: the last folded entitlement stands */
  }
}

/** Sign out: revoke + clear the vault slot + wipe the session, then reset. */
export async function authLogout(store: Store): Promise<void> {
  const internals = storeInternals(store);
  store.stopEntitlementPoll();
  // L3-WP19: a claim read or session refresh still in flight must not sign the member back in.
  internals.claimSession.epoch++;
  // core#584: a Billing checkout attempt still checking for a held grant opens nothing after sign-out.
  internals.billingAttempt++;
  if (store.state.billingChecking) store.setState({ billingChecking: false });
  await bridge.auth.logout();
  store.setState({
    tier: "free",
    // Coherent free/no-membership state — matches the expired-claim path
    // (tier:free + entitlement:lapsed); leaving a prior "active" would render a
    // false membership badge on a signed-out/free account (Rule 1).
    entitlement: "lapsed",
    citrateRole: "member",
    membershipActivating: false,
    membershipCheckoutAt: null,
    alfMember: false, // clear the ALF flag on sign-out — a signed-out account is not an ALF member (Rule 1)
    org: null,
    signedIn: false,
    authSub: null,
    authEmail: null,
    authName: null,
    authInitials: null,
    walletFromClaim: false,
  });
  store.save();
}

/** Open KYC in the browser; S2 status then arrives via userinfo polling. */
export async function kycStart(store: Store): Promise<void> {
  // Same hazard as "Manage account", but on the compliance path: `kyc_start`
  // opened a BARE authority URL, and the authority resolves identity from the
  // BROWSER's cookie. A member whose browser held a different Citrate session
  // was sent to verify THAT account — the exact failure seen 2026-08-06, where a
  // test member's KYC opened the admin's already-verified account.
  //
  // Route it through the identity-aware opener so we at least hint the intended
  // account and say plainly whose page this is. A server-side authenticated
  // hand-off is the real fix (see handoffs/IDENTITY_ACCOUNT_HANDOFF_2026-08-06).
  await store.openAuthorityPage("https://auth.citrate.ai/kyc/start", "identity verification");
}
