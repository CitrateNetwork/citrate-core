// L0-WP07b: whether the member's membership grant is on chain (MembershipStakeVault.attributedStake
// and the member SBT, read by bridge.membership.grantStatus), moved verbatim out of store.ts so the
// Store (onboarding, the wallet review gate) and the L3 auth actions (actions/l3/auth.ts) share one
// rule without a value import of store.ts. L0-WP07f adds describeBond, the bond words that both the
// onboarding grant step (actions/l3/onboarding.ts) and the wallet review gate show. store.ts
// re-exports every name, so `import { isGrantOnChain } from "./store"` is unchanged. L0 spine.
//
// Data source (Rule 7): a GrantStatus read from chain 40204 (bridge.membership.grantStatus); these
// only interpret it and read nothing themselves.

/**
 * BC-1.3 — the validator stake requirement in wei (== 32,000 SALT == 32000e18).
 * A granted member's `MembershipStakeVault.attributedStake(member)` reads at least
 * this. Kept as a BigInt so the comparison never loses precision (a 32k-SALT grant
 * far exceeds JS's safe integer range).
 */
export const VALIDATOR_STAKE_REQUIREMENT_WEI = 32000n * 10n ** 18n;

/**
 * BC-1.3 — is the 32,000-SALT membership grant GENUINELY on-chain?
 *
 * The S5 grant leg settles ONLY when the member holds the SBT AND at least the
 * validator stake requirement is attributable to them on chain (Rule 1 — read from
 * chain, never a fabricated settlement).
 *
 * THE VAULT IS THE ANSWER AGAIN (M-2, citrate-chain #141). This gate has been
 * widened twice, both times because the model moved the principal somewhere the
 * gate could not see:
 *   - ADR 2026-07-27 funded the member's OWN EOA, so a just-granted member held
 *     the 32k as NATIVE balance and the vault read 0 — added `nativeBalanceWei`;
 *   - after they self-bonded it moved to the registry — added `bondedStakeWei`.
 * Settling on the vault alone left a correctly-granted member polling "still
 * settling" forever (the 2026-07-28 stall): working, indistinguishable from broken.
 *
 * Under M-2 the principal has exactly ONE home again — the member's MemberBond
 * escrow — and the vault attributes it at GRANT time and keeps attributing it. So
 * `attributedStake` is durable across the whole lifecycle and both extra legs go.
 *
 * They would in fact be WRONG to keep. The registry leg stays 0 from the grant
 * until the member runs the activation ceremony, which can be days; the native leg
 * never fills at all, because M-2 sends the member only ~0.05 SALT of gas. Keeping
 * either would be reading a number that no longer carries the meaning it did.
 *
 * `hasSbt` remains a hard precondition.
 *
 * Wei values are decimal strings parsed as BigInt so a value beyond JS number range
 * is exact. A malformed value fails closed — never a fabricated grant.
 */
export function isGrantOnChain(g: { attributedStakeWei: string; hasSbt: boolean }): boolean {
  if (!g.hasSbt) return false;
  let staked: bigint;
  try {
    staked = BigInt(g.attributedStakeWei);
  } catch {
    return false; // fail-closed on an unparseable value — never fabricate a grant
  }
  return staked >= VALIDATOR_STAKE_REQUIREMENT_WEI;
}

/**
 * How the dashboard describes a member's bond (M-2.2 / M-2.3). Pure, so the
 * wording is testable without a render.
 *
 * Rule 1: every branch names a real on-chain fact. "Staked" is never claimed for a
 * member whose escrow does not exist, and an unlock is never advertised as
 * withdrawable while KYC is outstanding — the chain enforces both gates, and the UI
 * must not imply otherwise.
 */
export function describeBond(g: {
  bondDeployed: boolean;
  unlockBlock: number | null;
  isUnlocked: boolean;
  isKycVerified: boolean;
  hasValidator: boolean;
  bondActivated?: boolean | null;
}): string {
  // C1-V V5 — the UI truth contract §1 membership/bond words: "staked" only once the bond is
  // activated (`hasValidator`, the bond clone is a registry staker); "Locked until … (block N)" from
  // MemberBond.unlockBlock (the app does not read unlockTimestamp yet, so only the block shows);
  // "Unlocked · verify identity to withdraw" / "Unlocked" from isUnlocked + isKycVerified.
  if (!g.bondDeployed) return "No bond yet";
  // #349 review: the bond's activation state is MemberBond.activated(); a registry binding
  // (hasValidator) implies it. After an exit or a slash hasValidator reads false while the bond
  // stays activated, and an unread flag is not evidence either way.
  const activated = g.hasValidator ? true : (g.bondActivated ?? null);
  if (!g.isUnlocked) {
    const at = g.unlockBlock === null ? "—" : g.unlockBlock.toLocaleString("en-US");
    if (g.hasValidator) return `Staked · locked until block ${at}`;
    // "not staked" needs activated() = false (#411's fix); otherwise the plain §1 lock word.
    return activated === false ? `Bond locked · not staked · until block ${at}` : `Locked until block ${at}`;
  }
  // Unlocked. A never-activated bond can't be released or withdrawn (requestRelease /
  // withdrawToMember revert NotActivated()), so it gets no identity / withdraw wording (v1.2).
  if (activated === false) return "Unlocked · not activated";
  if (activated === null) return "Unlocked · bond state not read yet";
  // Owner decision A.5: eligible, never automatic. A.6: KYC supersedes the lock, so an
  // unverified member is NOT told their funds are available.
  if (!g.isKycVerified) return "Unlocked · verify identity to withdraw";
  return "Unlocked";
}
