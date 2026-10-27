// @vitest-environment node
//
// fxEARN tripwire (review P1-1 on #556). `bondedStake` drives the grant copy ("Your bond is
// activated, so the grant is staked"), the activation offer and the rewards read, and it is
// persisted. Its only live source is the grant read: `stakeOf(pubkeyOfStaker(MemberBond))`
// (`GrantStatus.bondedStakeWei`). A second source (a wallet's own registry stake was one) made a
// member with an unactivated bond read as staked. This fails CI if another writer appears.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const store = readFileSync(join(__dirname, "store.ts"), "utf8");

describe("tripwire — bondedStake has one live source, the bond's registry stake", () => {
  it("the poll's `bonded` is assigned only from the state and from g.bondedStakeWei", () => {
    const writes = [...store.matchAll(/\bbonded\s*=(?!=)\s*([^;\n]+)/g)].map((m) => m[1].trim());
    expect(writes).toEqual(["this.state.bondedStake", "g.bondedStakeWei ? Number(BigInt(g.bondedStakeWei)) / 1e18 : 0"]);
  });

  it("bondedStake is written only by the poll patch and the sim-only preview", () => {
    const direct = [...store.matchAll(/\bbondedStake\s*=(?!=)\s*([^;\n]+)/g)].map((m) => m[1].trim());
    expect(direct).toEqual(["simBond"]);
    const keyed = [...store.matchAll(/\bbondedStake:\s*([^,\n}]+)/g)].map((m) => m[1].trim());
    expect(keyed).toEqual(["bonded"]);
  });

  it("the registry read for the validator words carries no stake or staker", () => {
    const truth = readFileSync(join(__dirname, "chainTruth.ts"), "utf8");
    const iface = truth.slice(truth.indexOf("export interface ValidatorActivation"), truth.indexOf("}", truth.indexOf("export interface ValidatorActivation")));
    expect(iface).not.toMatch(/staker|bonded/i);
  });
});
