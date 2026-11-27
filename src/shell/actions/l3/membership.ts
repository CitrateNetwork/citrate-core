// L3-WP19: who the membership banner is talking to, and the live claim read behind it.
//
// Reported 2026-11-27 (owner): every free member saw the red "Membership lapsed ... Renew" banner,
// and a member who then paid kept seeing it. Three causes, one per function here:
//   1. `neverHeldMembership`: a free member's `/userinfo` carries no tier claim, so the entitlement
//      engine folds `tier: free, entitlement: lapsed` for them exactly as for a member whose year
//      ran out. Nothing told the two apart, so every free member was told to renew.
//   2. `checkoutAwaiting`: after checkout opens, nothing said "you just paid" until the claim or
//      chain evidence of the grant arrived. Paid orders whose grant the server refuses never got
//      there: re-driving the 17 stuck orders on 2026-11-27 showed 10 refused because the member
//      never linked a wallet in Core (`grant_denied_no_member_wallet`, `walletLinkNeeded`), and
//      others held until identity verification is done (`denied_backoff`).
//   3. `readLiveClaim`: the post-checkout poll and the focus re-check read `/userinfo` with the
//      in-memory access token, which the authority expires after an hour. On a node left open
//      longer, every read was a 401 that the poll swallowed, so a claim that did land was never
//      folded until the app restarted.
//
// Data sources (Rule 7), every one already folded into AppState:
//   - `entitlement`, `tier`, `authExpiresAt`, `s2`: the live `/userinfo` claim (applyAuthStatus),
//     `s2` from its `kyc_status`;
//   - `hasGrant`, `hasSbt`: the chain 40204 grant read (reconciledGrantPatch);
//   - `walletAddr` (the claim's `wallet_address`) against `custodyAddr` (this device's custody EOA,
//     from the wallet read): the link the grant needs, as store.walletIsLinked() reads it;
//   - `s3`: the persisted membership checkout step ("paying" = a checkout was opened on this
//     device and no grant has settled; "settled" = the grant landed on chain);
//   - `membershipCheckoutAt`: when this session opened checkout (in memory).
// The app cannot read the order itself (core-membership's order route takes only its own client's
// tokens, see src/onboarding/grantDelay.ts), so nothing here ever claims a payment went through.

import type { AuthStatus } from "../../../bridge/types";
import type { AppState } from "../../state";
import type { Store } from "../../store";

/** The AppState fields the membership banner reads. */
export type MembershipFacts = Pick<
  AppState,
  | "entitlement"
  | "membershipActivating"
  | "hasGrant"
  | "hasSbt"
  | "s3"
  | "s2"
  | "authExpiresAt"
  | "membershipCheckoutAt"
  | "walletAddr"
  | "custodyAddr"
>;

/**
 * True when the authority serves THIS device's custody wallet as the member's `wallet_address`.
 * An unread side reads false, never an optimistic yes (store.walletIsLinked delegates here).
 */
export function walletLinked(s: Pick<AppState, "walletAddr" | "custodyAddr">): boolean {
  const claim = (s.walletAddr || "").toLowerCase();
  const custody = (s.custodyAddr || "").toLowerCase();
  return claim !== "" && custody !== "" && claim === custody;
}

/**
 * True only when the link is known to be missing: this device's custody wallet has been read and
 * the claim does not serve it. Before the wallet read lands nothing is known, so the banner does
 * not ask for a link it may not need.
 */
export function walletLinkNeeded(s: Pick<AppState, "walletAddr" | "custodyAddr">): boolean {
  return (s.custodyAddr || "") !== "" && !walletLinked(s);
}

/**
 * True when nothing on this device or in the claim says this account ever held a membership: no
 * grant or SBT read on chain, no settled checkout, and no entitlement expiry in the claim (a free
 * member's claim carries none). A lapsed member always has at least one of these, so they keep the
 * renewal path (the 2026-09-11 safeguard still restores a member whose SBT is on chain).
 */
export function neverHeldMembership(s: Pick<AppState, "hasGrant" | "hasSbt" | "s3" | "authExpiresAt">): boolean {
  return !s.hasGrant && !s.hasSbt && s.s3 !== "settled" && !s.authExpiresAt;
}

/**
 * True when a membership checkout was opened and its grant has not settled: this session opened
 * it (`membershipCheckoutAt`), or an earlier one did (`s3: "paying"` is persisted). Opening is all
 * the app can see; whether the member paid is the server's to say.
 */
export function checkoutAwaiting(s: Pick<AppState, "s3" | "membershipCheckoutAt">): boolean {
  return typeof s.membershipCheckoutAt === "number" || s.s3 === "paying";
}

/**
 * A free member on the free plan: the claim reads lapsed only because it carries no tier, nothing
 * shows a past membership, no checkout is waiting, and no grant is visibly in flight.
 */
export function isFreeMember(s: MembershipFacts): boolean {
  return s.entitlement === "lapsed" && !s.membershipActivating && !checkoutAwaiting(s) && neverHeldMembership(s);
}

/** The authority's 401 as the kit reports it (kit/src/oidc.rs `AuthError::Unauthorized`). */
export function isSessionExpiredError(err: unknown): boolean {
  const msg = err instanceof Error ? err.message : String(err);
  return /session expired|not authorized/i.test(msg);
}

/**
 * The session state `readLiveClaim` shares across its callers (one per Store): the refresh in
 * flight, and an epoch that sign-out advances.
 */
export interface ClaimSession {
  refreshing: Promise<AuthStatus> | null;
  /** The epoch the refresh in flight started in: a refresh from before a sign-out is never joined. */
  refreshingEpoch: number;
  epoch: number;
}

export function newClaimSession(): ClaimSession {
  return { refreshing: null, refreshingEpoch: 0, epoch: 0 };
}

/** The read was overtaken by a sign-out: nothing it returned may be folded. */
export const SIGNED_OUT_DURING_READ = "auth: signed out while the claim was being read";

/**
 * The live `/userinfo` claim. When the access token has expired (a 401), renew the session from
 * the vaulted refresh token (the same silent refresh launch uses) and read again.
 *  - One refresh at a time: the poll and the focus re-check can both hit the 401, and the
 *    authority rotates refresh tokens and revokes the grant when a used one is presented again,
 *    so concurrent callers share the refresh in flight.
 *  - A sign-out while the refresh runs wins: the refresh may have written a new refresh token
 *    and session after logout wiped them, so they are wiped again (auth.logout) and the read
 *    rejects. A sign-out during the read rejects too, so no caller re-signs the member in.
 * Any other failure, or a refresh the authority declines, is thrown unchanged: nothing is
 * fabricated, and a different subject is refused inside the refresh itself (A3-04).
 */
export async function readLiveClaim(
  auth: { userinfo(): Promise<AuthStatus>; refresh(): Promise<AuthStatus>; logout(): Promise<void> },
  session: ClaimSession,
): Promise<AuthStatus> {
  const epoch = session.epoch;
  let claim: AuthStatus;
  try {
    claim = await auth.userinfo();
  } catch (err) {
    if (!isSessionExpiredError(err)) throw err;
    if (!session.refreshing || session.refreshingEpoch !== epoch) {
      const refreshing: Promise<AuthStatus> = auth.refresh().finally(() => {
        if (session.refreshing === refreshing) session.refreshing = null;
      });
      session.refreshing = refreshing;
      session.refreshingEpoch = epoch;
    }
    try {
      await session.refreshing;
    } finally {
      // Whether the refresh resolved or failed, it may have stored a rotated token after the
      // sign-out wiped the vault: wipe again, and report the sign-out rather than the result.
      if (session.epoch !== epoch) {
        await auth.logout().catch(() => {});
        throw new Error(SIGNED_OUT_DURING_READ);
      }
    }
    claim = await auth.userinfo();
  }
  if (session.epoch !== epoch) throw new Error(SIGNED_OUT_DURING_READ);
  return claim;
}

/** What the banner's button does. */
export type BannerAction = "renew" | "upgrade" | "link" | "verify";

/**
 * Run the banner's call to action through the seams that already exist:
 *  - renew: the renewal checkout (store.renewMembership), for a member whose membership lapsed;
 *  - upgrade: the guarded checkout for a free member (store.becomeMember: link the wallet first,
 *    never for an account that already holds the grant), as Settings › Billing does;
 *  - link: the wallet link ceremony (store.linkWallet: provisions the device wallet, opens the
 *    personal_sign review; nothing is signed until the member approves, no funds move);
 *  - verify: the identity verification flow (store.onS2Start: opens KYC, polls the claim).
 * After link or verify, keep watching the entitlement so the banner clears once the grant lands.
 */
export async function runBannerAction(store: Store, action: BannerAction): Promise<void> {
  if (action === "renew") return store.renewMembership();
  if (action === "upgrade") return store.becomeMember();
  if (action === "link") await store.linkWallet();
  else store.onS2Start();
  store.startEntitlementPoll();
}
