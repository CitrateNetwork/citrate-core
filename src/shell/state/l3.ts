// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L3 (Shell, onboarding, nav, notifications: sign-in identity, membership and tier,
// onboarding steps, coach, telemetry consent, people).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L3 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import { personaFor } from "./persona";
import type { GroupRoleRow } from "../../surfaces/groupsNavigator";
import type { MembershipPrice } from "../../data/membership";
import type { Person } from "../../surfaces/peopleDirectory";
import type { PersistScope } from "../netScope";

export interface L3State {
  tier: string;
  org: string | null;
  /**
   * CORE-A3 entitlement engine — the `citrate_role` claim (member / operator /
   * builder / org-seat). In a Tauri build this comes from the live /userinfo
   * entitlement claim; in web-dev from the sim persona. Gates Settings RBAC.
   */
  citrateRole: string;
  /**
   * ALF cooperative membership flag (ALF-ND). True iff the `citrate_role` claim is
   * `alf_member` — a CLAIM, never tier-derived (DGX_HANDOFF §4.3). Gates the ALF surface + nav.
   */
  alfMember: boolean;
  /**
   * CORE-A3 identity — the REAL signed-in user, folded from the live /userinfo
   * claims (`sub`, `email`, `wallet_address`). In a Tauri build these are the
   * source of truth for every identity surface (Sidebar, Settings account/RBAC,
   * greeting); the sim `persona` is ONLY a web-dev affordance and must never
   * surface once a real user is signed in (Rule 1 — no prototype identity shown
   * to a real account). `signedIn` gates the switch: true ⇒ render auth identity,
   * false ⇒ fall back to the sim persona. `authName`/`authInitials` are derived
   * from the email local-part (the authority issues no display-name claim).
   */
  signedIn: boolean;
  authSub: string | null;
  authEmail: string | null;
  authName: string | null;
  authInitials: string | null;
  /**
   * Q-A.4b item 10 — did `walletAddr` come from a REAL `wallet_address` claim
   * (folded by applyAuthStatus), or is it the deterministic persona `makeAddr`
   * seed? For a SIGNED-IN user this must be true before any identity surface (S4
   * onboarding) renders the address — otherwise the persona hash is a fabricated
   * wallet and the UI shows "—"/pending instead (Rule 1). Not persisted (identity
   * is re-derived from a live session only). Irrelevant in the web-dev/persona
   * path (signedIn=false), where the labelled persona address is the honest preview.
   */
  walletFromClaim: boolean;
  /**
   * BC-6.3 — the REAL entitlement expiry, folded verbatim from the /userinfo
   * `expires_at` claim (bridge `AuthStatus.expiresAt`; an ISO datetime or unix
   * seconds). Settings "Memberships & billing" renders THIS (an honest "—" when
   * null/absent) — never a hardcoded date (Rule 1). `null` until a signed-in
   * session folds a claim that carries an expiry.
   */
  authExpiresAt: string | null;
  entitlement: "active" | "expiring" | "grace" | "lapsed";
  stage: "s0" | "s1" | "s2" | "s3" | "s4" | "s5" | "s6" | "s7" | "done";
  s1: "idle" | "waiting" | "attest" | "done";
  s2: "none" | "pending" | "verified" | "failed" | "review";
  s3: "idle" | "paying" | "settled";
  /**
   * The bounded S3 grant poll ran out of budget while still "paying".
   *
   * The poll used to just stop, leaving a spinner with no exit: `s3 === "paying"`
   * renders only a loader, so a member whose grant had not landed yet had NO way
   * to resume waiting and no explanation. That is a dead end, and it is reachable
   * in NORMAL operation — core-membership's grant reconciler runs on a 10-minute
   * cron with a 6-HOUR backoff after a denial, so a grant can legitimately land
   * long after any client-side budget we are willing to spend. Surfacing this flag
   * lets S3 say so honestly and offer a re-check that costs nothing (it re-reads
   * the chain; it never re-opens checkout, so it can never double-charge).
   */
  s3PollExhausted: boolean;
  /**
   * The grant is delayed on our side (2026-11-12..11-17 outage): S3 is paying, the chain has no
   * grant, and either core-membership reports its grant signer not ready or 10 minutes have passed
   * since checkout opened (`isGrantDelayed`, onboarding/grantDelay.ts). S3 then shows the
   * delayed-grant card instead of "Waiting for checkout…". Session only, never persisted.
   */
  s3GrantDelayed: boolean;
  /**
   * core-membership `/api/health` `chain.signerReadiness` as last read while S3 was paying: true
   * "ready", false a named not-ready state, null not read (or an older deploy without the field).
   * Session only, never persisted.
   */
  s3SignerReady: boolean | null;
  /**
   * core#584: the pre-checkout grant check (onS3Pay's money guard) is running. One attempt at a
   * time: Pay and "Continue free" are disabled until it ends, and Back ends it. Not persisted.
   */
  s3Checking: boolean;
  /**
   * core#584: Settings › Billing "Become a member" is checking chain 40204 for a grant this account
   * already holds, before any checkout. One attempt at a time; the button is disabled meanwhile.
   * Not persisted.
   */
  billingChecking: boolean;
  /**
   * v0.5.0-rc.5 FIX 1c: the on-chain grant is in flight (SBT minted or stake attributed)
   * while the `/userinfo` tier claim has not landed yet. The banner shows a neutral
   * "Membership activating" notice instead of the red lapse. Never unlocks paid
   * features (entitlement stays as the claim says). Not persisted: re-derived from a
   * live chain read every session.
   */
  membershipActivating: boolean;
  /**
   * L3-WP19: when THIS session opened a membership checkout (ms), or null. Opening is all the app
   * can see (the order is the server's), so the banner says "if you completed checkout" and never
   * shows the red lapse to someone who may just have paid. Cleared when the claim reads active and
   * on sign-out. Not persisted: a checkout from an earlier session is `s3: "paying"`.
   */
  membershipCheckoutAt: number | null;
  s5: "idle" | "verifying" | "settling" | "settled";
  s5c: number;
  s5n: number;
  /**
   * BC-1.3 (F1 — Rule 1 display honesty) — the REAL staked principal the settled
   * S5 card renders, as a decimal wei string read from
   * `MembershipStakeVault.attributedStake(member)` via the grant-status read (tauri)
   * or the honest sim `grantStatus` (web-dev). Set ONLY when the grant genuinely
   * lands (both poll paths), never fabricated. `null`/`"0"` until then. The card
   * formats wei→SALT from this — it never shows a hardcoded "32,000".
   */
  s5StakeWei: string | null;
  /**
   * M-2.2/M-2.3 — a plain-language description of the member's bond escrow
   * (`describeBond`), e.g. "Bond locked · not staked · until block 300,000". Derived ONLY
   * from the real grant-status read; `null` until the grant lands. Replaces the
   * hardcoded "mainnet release policy" line, which described a release flag that
   * no longer exists — the member's own lock + KYC gates govern release now.
   */
  s5BondStatus: string | null;
  /** C1-V V5 — `MemberBond.unlockBlock()` from the grant read; null until the grant lands. */
  s5UnlockBlock: number | null;
  kycOutcome: "verified" | "failed" | "review";
  coach: number;
  coachDone: boolean;
  demoOpen: boolean;
  hasGrant: boolean;
  hasSbt: boolean;
  /** C1-P — the membership price read from core-membership; null = not read or unavailable. */
  membershipPrice: MembershipPrice | null;
  telemetry: boolean;
  /** The telemetry disclosure version the member consented to (telemetry-fields.json `version`).
   *  `null` = no consent. 1 = consented before 0.5.2 (before `joinSteps`); see joinFunnel.ts. */
  telemetryConsentVersion: number | null;
  s1c: number;
  /** 0.5.4 — the member collapsed the sidebar to its icons. Off by default. Persisted. */
  sidebarCollapsed: boolean;
  s6ready?: boolean;
  /**
   * CONNECT-S0 — the People directory: everyone you share a group with, derived live from your group
   * rosters + verified faces (never fabricated — Rule 1). `peopleState` reflects the fetch honestly:
   * "loading" before/while reading, "ready" once the real aggregation lands, "unavailable" if groups
   * can't be read (the UI shows an honest empty/offline state, never invented people).
   */
  people: Person[];
  peopleState: "loading" | "ready" | "unavailable";
  /**
   * CONNECT-S4 — the groups & clusters role navigator: every group you're in, badged with your role,
   * derived in the SAME pass as `people` (shares `peopleState`). Manage-capable groups sort first so
   * "where I'm admin" is one glance. Never fabricated (Rule 1) — a group whose roster hasn't loaded
   * carries a null role.
   */
  myGroups: GroupRoleRow[];
  /**
   * Download gate: an access link (`citrate://access?code=…`) from the download email, waiting on
   * the first-run access check (src/onboarding/AccessPassGate.tsx). Not a group invite. Checked in
   * Rust (`access_pass_verify`) only when the person presses Continue. Transient, not persisted.
   */
  pendingAccessLink: string | null;
  /**
   * PHONEPAY-S4 — a pending `citrate://claim` onboarding link (transient, NOT persisted).
   * Set from the emailed claim link; carries only untrusted DISPLAY HINTS (the email the
   * buyer paid with + an order id) so onboarding can (a) tell the user which account to
   * sign in with and (b) flag a wrong-account mismatch after sign-in. It grants NOTHING —
   * admission is the authority re-auth + server-side sub↔order reconciliation (A8). Null
   * when no claim is pending; cleared once onboarding has surfaced it.
   */
  pendingClaim: {
    email: string | null;
    orderHint: string | null;
    raw: string;
  } | null;
}

export function freshL3(pid: string): L3State {
  const P = personaFor(pid);
  return {
    tier: P.fresh ? "free" : P.tier,
    org: P.org,
    citrateRole: P.role,
    alfMember: false, // set true only by applyAuthStatus when the alf_member claim is present
    // Real identity is empty until a live sign-in folds /userinfo claims in
    // (applyAuthStatus). Until then the sim persona is the display fallback.
    signedIn: false,
    authSub: null,
    authEmail: null,
    authName: null,
    authInitials: null,
    walletFromClaim: false,
    authExpiresAt: null,
    entitlement: "active",
    stage: P.fresh ? "s0" : "done",
    s1: "idle",
    s2: "none",
    s3: "idle",
    s3PollExhausted: false,
    s3GrantDelayed: false,
    s3SignerReady: null,
    s3Checking: false,
    billingChecking: false,
    membershipActivating: false,
    membershipCheckoutAt: null,
    s5: "idle",
    s5c: 0,
    s5n: 0,
    s5StakeWei: null,
    s5BondStatus: null,
    s5UnlockBlock: null,
    kycOutcome: "verified",
    coach: -1,
    coachDone: !P.fresh,
    demoOpen: false,
    hasGrant: false,
    hasSbt: false,
    membershipPrice: null,
    telemetry: false,
    telemetryConsentVersion: null,
    s1c: 0,
    sidebarCollapsed: false,
    people: [],
    peopleState: "loading",
    myGroups: [],
    pendingAccessLink: null,
    pendingClaim: null,
  };
}

const D = "device" as const;
const N = "network" as const;
export const L3_PERSIST_SCOPE = {
  tier: D, org: D, citrateRole: D, entitlement: D, stage: D, s2: D, s3: D,
  // The grant and the SBT are chain reads (reconciledGrantPatch), so they are the network's.
  s5: N, s5n: N, hasGrant: N, hasSbt: N,
  // NOTE: the real-identity fields (signedIn/authSub/authEmail/authName/
  // authInitials/authExpiresAt) are DELIBERATELY NOT persisted — HIPAA sign-out-by-default.
  // Every launch starts signed-out; identity is re-derived only from a live
  // authority session (refreshAuth → auth.status), never from disk. No account
  // PII is written to localStorage.
  telemetry: D, telemetryConsentVersion: D, coachDone: D,
  s5StakeWei: N, s5BondStatus: N,
  kycOutcome: D,
  // 0.5.4: the member's layout choice (collapsed sidebar).
  sidebarCollapsed: D,
} satisfies Partial<Record<keyof L3State, PersistScope>>;
