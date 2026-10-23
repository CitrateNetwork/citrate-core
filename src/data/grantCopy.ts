// =====================================================================
// citrate-core: the ONE grant-custody explanation (C1-V V5)
//
// UI truth contract v1 §5.4 (citrate-chain docs/ui-truth/UI_TRUTH_CONTRACT.md): say exactly what
// the contracts do, use one explanation everywhere (onboarding, Wallet, Node and the Hermes
// context), and say "staked" only after the bond is activated. §5.7: the "not purchased, not an
// investment" line appears wherever the grant is mentioned. §5.6: no promise that betanet SALT
// carries over to mainnet.
//
// What the contracts do: the membership grant is paid into the member's MemberBond clone
// (MembershipStakeVault.bondOf), which locks it until MemberBond.unlockBlock / unlockTimestamp.
// It is staked in the ValidatorRegistry only when the member activates the bond.
// =====================================================================

/** §5.7, verbatim on every surface that mentions the grant. */
export const GRANT_DISCLAIMER = "The grant is not purchased, and it is not an investment.";

/** §5.6, until the C2 genesis spec decides what carries over. */
export const BETANET_CARRYOVER = "Betanet. What carries over to mainnet is set in the genesis spec.";

export interface GrantFacts {
  /** The grant amount as displayed (e.g. "32,000"), or null when it has not been read. */
  amountSalt: string | null;
  /** `MemberBond.unlockBlock()`, or null when it has not been read (before the grant). */
  unlockBlock: number | null;
  /** The bond has been activated: its principal is staked in the ValidatorRegistry. */
  staked: boolean;
  /**
   * The grant is on chain. A granted member whose unlock block has not been read sees "—", never
   * the pre-grant "for about a year" (contract rule 2). Omit before the grant exists.
   */
  granted?: boolean;
}

/** The custody sentence (§5.4): where the grant is and whether it is staked. */
export function grantCustody({ amountSalt, unlockBlock, staked, granted }: GrantFacts): string {
  const grant = amountSalt && amountSalt !== "—" ? `a ${amountSalt} SALT grant` : "a SALT grant";
  const until = unlockBlock != null ? `until block ${unlockBlock.toLocaleString("en-US")}` : granted ? "until block — (not read yet)" : "for about a year";
  return staked
    ? `Your membership comes with ${grant}, locked in your bond ${until}. Your bond is activated, so the grant is staked in the ValidatorRegistry.`
    : `Your membership comes with ${grant}, locked in your bond ${until}. It is not staked until you activate the bond.`;
}

/** The full explanation: custody, the disclaimer, and the betanet line. */
export function grantExplanation(facts: GrantFacts): string {
  return [grantCustody(facts), GRANT_DISCLAIMER, BETANET_CARRYOVER].join(" ");
}
