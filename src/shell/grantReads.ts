// L0-WP07b: whether the member's membership grant is on chain (MembershipStakeVault.attributedStake
// and the member SBT, read by bridge.membership.grantStatus), moved verbatim out of store.ts so the
// Store (onboarding, the wallet review gate) and the L3 auth actions (actions/l3/auth.ts) share one
// rule without a value import of store.ts. store.ts re-exports both names, so
// `import { isGrantOnChain } from "./store"` is unchanged. L0 spine.
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
