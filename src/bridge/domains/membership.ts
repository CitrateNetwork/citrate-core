// citrate-core: bridge domain `membership`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

/**
 * BC-1.3 — the REAL on-chain membership grant status the S5 (membership grant)
 * ceremony settles from. Every field is a live 40204 read (Rule 1): the vault's
 * `attributedStake`/`attributedShares(member)` (wei as decimal strings, since a
 * 32,000-SALT grant exceeds JS number range) and `CitrateMemberSBT.balanceOf`
 * (`hasSbt`). A fresh/never-granted member reads 0 / 0 / false.
 */
export interface GrantStatus {
  attributedStakeWei: string;
  /**
   * `MembershipStakeVault.attributedPrincipal(member)`. M-2 replaced
   * `attributedShares` (a pool share count) with raw bonded principal —
   * there are no shares under bonding.
   */
  attributedPrincipalWei: string;
  hasSbt: boolean;
  /**
   * `MembershipStakeVault.bondOf(member)` — the member's MemberBond escrow.
   * ALWAYS present (CREATE2-deterministic), so it resolves even for a member who
   * has never been granted. `bondDeployed` is what says it is real.
   */
  bondAddress: string;
  bondDeployed: boolean;
  /**
   * `ValidatorRegistry.stakeOf(pubkeyOfStaker(bond))` — the principal actually
   * bonded in the registry. Zero until the member runs the activation ceremony,
   * so this is NOT the settle signal; attribution is.
   */
  bondedStakeWei: string;
  /**
   * `pubkeyOfStaker(bondOf(member)) != 0` — the member has ACTIVATED a validator.
   * NOTE the staker is the member's bond clone, not the member.
   */
  hasValidator: boolean;
  /**
   * `MemberBond.activated()`: the bond bonded its principal into the registry. It stays true after
   * a full exit or a slash (when `hasValidator` reads false), and the bond can still claim that
   * key's matured rewards. null = no bond, or the read failed. Optional for older backends.
   */
  bondActivated?: boolean | null;
  /**
   * C1-V5 — `MemberBond.pubkey()`: the validator key the bond activated with, kept after an exit
   * (unlike `pubkeyOfStaker`). null = not activated, no bond, or the read failed. Optional for
   * older backends (absent = unread).
   */
  bondPubkey?: string | null;
  /**
   * `MemberBond.isUnlocked()` AS READ (chain#331: `activate` reverts once true). null = no bond or
   * the read did not decode; absent = an older backend. Both are unread, never "locked".
   */
  bondUnlocked?: boolean | null;
  /** `MemberBond.unlockBlock()` — height leg of the 1-year lock; null pre-bond. */
  unlockBlock: number | null;
  /** `MemberBond.isUnlocked()` — either leg elapsed. Eligibility, not a payout. */
  isUnlocked: boolean;
  /** `MemberBond.isKycVerified()` — gates money OUT only, never participation. */
  isKycVerified: boolean;
}


/** The qualifying fields the Enterprise contact form collects. `org` + `email` are required; the rest
 *  help sales prep the call. No card/money fields — this is a lead, not a purchase. */
export interface EnterpriseLeadInput {
  org: string;
  email: string;
  contact?: string;
  seats?: string;
  workload?: string;
  timeline?: string;
  notes?: string;
}

/** core-membership's report of its grant signer (`membership.signerHealth`). */
export interface MembershipSignerHealth {
  signerReady: boolean | null;
  signerState: string | null;
}

export interface MembershipDomain {
  /**
   * BC-1.3 (@rule8) — read the member's REAL on-chain grant status from 40204
   * (`MembershipStakeVault.attributedStake`/`attributedShares` + `CitrateMemberSBT
   * .balanceOf`). `memberAddress` is the AA smart-wallet the grant targets (the
   * OIDC `wallet_address` claim), NOT the custody EOA. A PURE READ — it signs
   * nothing. S5 settles the grant leg ONLY from this real read; a never-granted
   * member honestly reads 0 stake / no SBT (no fabricated settlement, Rule 1). The
   * Tauri impl invokes `membership_grant_status`; the sim impl derives from the
   * persona/AppState (granted only for a paid sim member — never a real chain read).
   */
  grantStatus(memberAddress: string): Promise<GrantStatus>;
  /**
   * C1-P — the membership price from core-membership's public price route
   * (`{coreMembershipUrl}/api/membership/price`), read in Rust (the route sends no CORS headers).
   * Rejects on a 503, any non-200, or a body whose displayAmount disagrees with unitAmount; the
   * caller then shows the "See membership pricing" link and no number. The sim impl always
   * rejects: the web build has no price source.
   */
  price(): Promise<import("../../data/membership").MembershipPrice>;
  /**
   * The treasury grant signer's readiness as core-membership reports it on its PUBLIC
   * `{coreMembershipUrl}/api/health` (`chain.signerReadiness`, core-membership#86), read in Rust
   * (the route sends no CORS headers). `signerReady` is true for "ready", false for a named
   * not-ready state (unreachable, wrong_contracts, wrong_signer, underfunded, unconfigured,
   * unknown), and null when the field is absent (an older deploy) or is the dev-only "local-env".
   * Rejects on a transport error or a body that is not core-membership's health. A pure read: no
   * auth, no signing. S3 uses it only to say a grant is delayed; it never settles anything. The sim
   * impl always rejects: the web build has no source for it.
   */
  signerHealth(): Promise<MembershipSignerHealth>;
  /**
   * CORE-D3.C — open the REAL core-membership checkout in an in-app popup
   * (`{coreMembershipUrl}/checkout`). Resolves once the popup is OPENED — it does
   * NOT report payment success. The money + entitlement grant happen server-side
   * (core-membership → droplet); the caller then polls `auth.userinfo()` until the
   * entitlement goes active (the same "open a flow then poll userinfo" pattern as
   * `kycStart`/`pollKyc`). The Tauri impl invokes `membership_checkout`; the sim
   * impl is guarded out of packaged builds (web-dev keeps its fake settle).
   */
  checkout(loginHint?: string): Promise<void>;
  /**
   * Enterprise · Contact us — POST a QUALIFIED sales lead to core-membership's `/api/enterprise/lead`
   * (NOT the money path: never grants, charges, or signs). Contact PII is validated + field-encrypted
   * server-side. Resolves on success; REJECTS with an honest message on a validation/availability
   * failure (Rule 1 — never a fabricated "received"). The Tauri impl invokes `membership_enterprise_lead`.
   */
  enterpriseLead(lead: EnterpriseLeadInput): Promise<void>;
  /**
   * BC-5.3 (T1 identity READ) — read the member's AUTHORITATIVE wholly-on-chain
   * SBT emblem for their OIDC `sub`. The post-reroll CitrateMemberSBT generates the
   * art on-chain: `tokenURI` returns a `data:application/json;base64,...` whose
   * `image` is a `data:image/svg+xml;base64,...`. This resolves the tokenId via
   * `isSubBound`/`tokenIdForSub(keccak256(sub))`, reads `tokenURI`, and returns the
   * decoded `image` data-URI — or `null` HONESTLY when the member has no SBT (the
   * caller then shows the local `sbtArt.ts` emblem as a labelled offline fallback,
   * never a fabricated on-chain mark — Rule 1). A PURE READ; signs nothing. The
   * Tauri impl invokes `sbt_token_uri`; the sim impl returns null (no real chain in
   * the web preview — the local fallback renders, honestly labelled).
   */
  sbtArt(sub: string): Promise<string | null>;
}
