// A granted-but-not-yet-activated member has 32,000 SALT attributed + locked in the vault, while the
// VALIDATOR bond is still 0 (until the activation ceremony). The old surfacing read only the validator
// bond, so the member — and the node's model — saw "0 staked". These tests pin the fix: the attributed
// grant surfaces distinctly from the validator bond, and the model's snapshot no longer reports 0.
import { describe, it, expect } from "vitest";
import { reconciledGrantPatch, Store } from "./store";

const WEI_32K = (32000n * 10n ** 18n).toString();

// Mirrors the GrantStatus DTO for a member whose grant is on chain but not yet validator-activated.
const GRANT = {
  attributedStakeWei: WEI_32K,
  attributedPrincipalWei: WEI_32K,
  bondedStakeWei: "0", // validator bond is 0 until MemberBond.activate
  bondDeployed: true,
  bondAddress: "0xc248a5470b945eaffd34b51377293773cf26adde",
  unlockBlock: 500000,
  isUnlocked: false,
  isKycVerified: false,
  hasSbt: true,
  hasValidator: false,
} as const;

describe("grant stake surfacing — locked 32k is not '0 staked' (Rule 1)", () => {
  it("reconciledGrantPatch surfaces the attributed 32k + the unlock-bearing bond status", () => {
    const p = reconciledGrantPatch(GRANT as never);
    expect(p).not.toBeNull();
    expect(p!.hasGrant).toBe(true);
    expect(p!.s5StakeWei).toBe(WEI_32K);
    expect(p!.s5n).toBe(32000);
    expect(String(p!.s5BondStatus)).toMatch(/Staked · unlocks at block/);
  });

  it("returns null (no fabricated stake) when the grant is not genuinely on chain", () => {
    expect(reconciledGrantPatch({ ...GRANT, hasSbt: false } as never)).toBeNull();
    expect(reconciledGrantPatch({ ...GRANT, attributedStakeWei: "0" } as never)).toBeNull();
  });

  it("snapshot() exposes the locked grant to the model, distinct from the validator bond", () => {
    const store = new Store();
    store.setState({
      hasGrant: true,
      s5StakeWei: WEI_32K,
      s5BondStatus: "Staked · unlocks at block 500,000",
      bondedStake: 0,
      selfStake: 0,
    } as never);
    const snap = store.snapshot() as {
      staked: number;
      membership: { grantStakedSalt: number; validatorBondedSalt: number; locked: boolean; bondStatus: string; note: string };
    };
    expect(snap.staked).toBe(32000); // the model no longer sees 0 for a granted member
    expect(snap.membership.grantStakedSalt).toBe(32000);
    expect(snap.membership.validatorBondedSalt).toBe(0); // honest: not validating yet
    expect(snap.membership.locked).toBe(true);
    expect(snap.membership.bondStatus).toMatch(/unlocks at block/);
    expect(snap.membership.note.toLowerCase()).toContain("locked stake is not spendable");
  });
});
