// Join-8 — the Activate-validator step's pure logic, against UI truth contract v1:
//   §1 Validators / Membership and bond: the gate's conditions and the state words;
//   §2 durations: blocks × MEASURED block time, never 12 s;
//   §5: one grant explanation, "staked" only after activated(), no earning or projections.
import { describe, it, expect } from "vitest";
import type { ActivationStatusView } from "../bridge/domains";
import {
  activationGate,
  activationGrantCopy,
  activationPhase,
  fmtBlocksDuration,
  isNetworkSynced,
  validatorState,
  ACTIVATE_EXPLANATION,
} from "./activateValidator";

const WEI_32K = (32000n * 10n ** 18n).toString();

/** A granted member with a synced node and no validator yet: every gate condition met. */
function ready(over: Partial<ActivationStatusView> = {}): ActivationStatusView {
  return {
    bondAddress: "0x2222222222222222222222222222222222222222",
    bondDeployed: true,
    bondActivated: false,
    principalWei: WEI_32K,
    unlockBlock: 16_200_000,
    unlockTimestamp: 1_822_000_000,
    memberTokenId: "4",
    membershipActive: true,
    validatorPubkey: "0x" + "07".repeat(32),
    pubkeySource: "node",
    nodeKeyReady: true,
    registryStatus: 0,
    activationEpoch: 0,
    registryActive: false,
    epochBlocks: 1000,
    headBlock: 500_000,
    blockTimeMs: 2000,
    localBlock: 499_998,
    localSyncing: false,
    tx: null,
    ...over,
  };
}

describe("Synced (contract §1): eth_syncing false AND within 5 blocks of the public head", () => {
  it("is synced at 5 behind, not at 6", () => {
    expect(isNetworkSynced(ready({ localBlock: 499_995 }))).toBe(true);
    expect(isNetworkSynced(ready({ localBlock: 499_994 }))).toBe(false);
  });
  it("is not synced while eth_syncing returns an object", () => {
    expect(isNetworkSynced(ready({ localSyncing: true }))).toBe(false);
  });
  it("is unknown (null), not false, when a leg was not read", () => {
    expect(isNetworkSynced(ready({ localSyncing: null }))).toBeNull();
    expect(isNetworkSynced(ready({ headBlock: null }))).toBeNull();
    expect(isNetworkSynced(ready({ localBlock: null }))).toBeNull();
  });
});

describe("gating matrix: membership active, bond deployed, node Synced, not already a validator", () => {
  it("opens only when every condition is met", () => {
    const g = activationGate(ready(), true);
    expect(g.ready).toBe(true);
    expect(g.checks.every((c) => c.ok === true)).toBe(true);
  });

  const cases: [string, Partial<ActivationStatusView>, string][] = [
    ["membership lapsed", { membershipActive: false }, "membership"],
    ["membership unknown", { membershipActive: null }, "membership"],
    ["no bond deployed", { bondDeployed: false, bondActivated: null, membershipActive: null }, "bond"],
    ["bond unknown", { bondDeployed: null }, "bond"],
    ["node 6 blocks behind", { localBlock: 499_994 }, "synced"],
    ["node still syncing", { localSyncing: true }, "synced"],
    ["node not answering", { localSyncing: null, localBlock: null }, "synced"],
    ["bond already activated", { bondActivated: true }, "notValidator"],
    ["node key already registered", { registryStatus: 1 }, "notValidator"],
    ["registry status unknown", { registryStatus: null }, "notValidator"],
  ];
  for (const [name, over, key] of cases) {
    it(`blocks when ${name}, and names that reason`, () => {
      const g = activationGate(ready(over), true);
      expect(g.ready).toBe(false);
      const failed = g.checks.find((c) => c.key === key);
      expect(failed?.ok).not.toBe(true);
      expect(failed?.detail.length).toBeGreaterThan(0);
    });
  }

  it("blocks when this device's wallet is not the member wallet (activate is onlyMember)", () => {
    const g = activationGate(ready(), false);
    expect(g.ready).toBe(false);
    expect(g.checks.find((c) => c.key === "wallet")?.ok).toBe(false);
  });

  it("blocks with no read at all, every check unknown rather than failed-as-zero", () => {
    const g = activationGate(null, true);
    expect(g.ready).toBe(false);
    expect(g.checks.filter((c) => c.key !== "wallet").every((c) => c.ok === null)).toBe(true);
  });

  it("does not require the node key to exist yet when the bond is not activated (activation arms first)", () => {
    const g = activationGate(ready({ nodeKeyReady: false, validatorPubkey: null, pubkeySource: null, registryStatus: null }), true);
    expect(g.checks.find((c) => c.key === "notValidator")?.ok).toBe(true);
  });
});

describe("state mapping (contract §1 Validators)", () => {
  it("Not a validator: no bond", () => {
    expect(validatorState(ready({ bondDeployed: false, bondActivated: null })).word).toBe("Not a validator");
  });
  it("Bond locked · not staked: MemberBond.activated() = false", () => {
    expect(validatorState(ready()).word).toBe("Bond locked · not staked");
  });
  it("Staked · active from next epoch: registered, isActive = false", () => {
    expect(validatorState(ready({ bondActivated: true, pubkeySource: "bond", registryStatus: 1, registryActive: false, activationEpoch: 502 })).word).toBe(
      "Staked · active from next epoch",
    );
  });
  it("Validator active ONLY when ValidatorRegistry.isActive = true", () => {
    const st = ready({ bondActivated: true, pubkeySource: "bond", registryStatus: 1, registryActive: true });
    expect(validatorState(st).word).toBe("Validator active");
    expect(validatorState({ ...st, registryActive: null }).word).toBeNull();
    expect(validatorState({ ...st, registryActive: false }).word).not.toBe("Validator active");
  });
  it("Exiting / Slashed from registry status 2 / 3", () => {
    expect(validatorState(ready({ bondActivated: true, registryStatus: 2 })).word).toBe("Exiting");
    expect(validatorState(ready({ bondActivated: true, registryStatus: 3 })).word).toBe("Slashed");
  });
  it("unknown reads map to no word (shown as —), never a guessed state", () => {
    expect(validatorState(null).word).toBeNull();
    expect(validatorState(ready({ bondDeployed: null })).word).toBeNull();
    expect(validatorState(ready({ bondActivated: true, registryStatus: null })).word).toBeNull();
  });
});

describe("durations use the measured block time, never 12 s", () => {
  it("1,000 blocks at a measured 2 s is ~33 min", () => {
    expect(fmtBlocksDuration(1000, 2000)).toBe("1,000 blocks (~33 min)");
  });
  it("50,400 blocks at 2 s is ~28 h, not ~7 days", () => {
    expect(fmtBlocksDuration(50_400, 2000)).toBe("50,400 blocks (~28 h)");
  });
  it("with no measurement it shows blocks only and says so", () => {
    const s = fmtBlocksDuration(1000, null);
    expect(s).toBe("1,000 blocks (time not measured yet)");
  });
});

describe("one grant explanation (contract §5.4, §5.6, §5.7)", () => {
  it("says locked until date and block, with the disclaimer and the betanet line", () => {
    const s = activationGrantCopy(ready());
    expect(s).toContain("Your membership comes with a 32,000 SALT grant, locked in your bond until");
    expect(s).toContain("block 16,200,000");
    expect(s).toContain("2027");
    expect(s).toContain("not purchased");
    expect(s).toContain("not an investment");
    expect(s).toContain("What carries over to mainnet is set in the genesis spec.");
  });
  it('says "staked" only after activated()', () => {
    expect(activationGrantCopy(ready())).not.toMatch(/\bis staked\b/);
    expect(activationGrantCopy(ready({ bondActivated: true }))).toMatch(/staked in the ValidatorRegistry/);
  });
  it("reads the principal; an unread principal is —, never a hard-coded 32,000", () => {
    expect(activationGrantCopy(ready({ principalWei: null }))).toContain("a — SALT grant");
  });
});

describe("no earning, projections or banned validator words in the step's copy", () => {
  const texts = [
    ACTIVATE_EXPLANATION,
    activationGrantCopy(ready()),
    activationGrantCopy(ready({ bondActivated: true })),
    ...activationGate(null, false).checks.map((c) => c.detail),
    ...activationGate(ready({ membershipActive: false, bondDeployed: false, localSyncing: true, registryStatus: 1 }), false).checks.map((c) => c.detail),
    validatorState(ready({ bondActivated: true, registryStatus: 1, registryActive: false, activationEpoch: 502 })).detail,
    validatorState(ready({ bondActivated: true, registryStatus: 1, registryActive: true })).detail,
  ];
  it.each(texts.map((t, i) => [i, t]))("text %i stays inside the contract", (_i, t) => {
    expect(t).not.toMatch(/\bearn|income|projected|estimate|\bper month\b|\bAPY\b|\byield\b|Validating|Finali[sz]ed|\bprice\b/i);
  });
});

describe("step phase: a sent activation is Pending until the chain shows it", () => {
  it("pending while the receipt is absent", () => {
    expect(activationPhase(ready({ tx: { hash: "0xabc", included: false, blockNumber: null, success: null } }), true)).toBe("pending");
  });
  it("failed with the block when the receipt reverted", () => {
    expect(activationPhase(ready({ tx: { hash: "0xabc", included: true, blockNumber: 501, success: false } }), true)).toBe("failed");
  });
  it("staked once registered, active once isActive", () => {
    expect(activationPhase(ready({ bondActivated: true, registryStatus: 1, registryActive: false }), true)).toBe("staked");
    expect(activationPhase(ready({ bondActivated: true, registryStatus: 1, registryActive: true }), true)).toBe("active");
  });
  it("ready / blocked from the gate, reading with no data", () => {
    expect(activationPhase(ready(), true)).toBe("ready");
    expect(activationPhase(ready({ membershipActive: false }), true)).toBe("blocked");
    expect(activationPhase(null, true)).toBe("reading");
  });
});
