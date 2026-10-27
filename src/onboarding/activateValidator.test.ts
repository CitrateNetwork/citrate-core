// Join-8 — the Activate-validator step's pure logic, against UI truth contract v1:
//   §1 Validators / Membership and bond: the gate's conditions and the state words;
//   §2 durations: blocks × MEASURED block time, never 12 s;
//   §5: one grant explanation, "staked" only after activated(), no earning or projections.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import type { ActivationStatusView, ExitOutcomeView } from "../bridge/domains";
import {
  activationGate,
  activationGrantCopy,
  activationPhase,
  fmtBlocksDuration,
  isNetworkSynced,
  keyMismatch,
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
    nodePubkey: "0x" + "07".repeat(32),
    bondPubkey: null,
    registryStatus: 0,
    registryBondedWei: null,
    registryEscrowWei: null,
    exitOutcome: null,
    activationEpoch: 0,
    registryActive: false,
    currentEpoch: 500,
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

  it("requires this node's validator key, and says why it is missing", () => {
    const g = activationGate(ready({ nodeKeyReady: false, nodePubkey: null, validatorPubkey: null, pubkeySource: null, registryStatus: null }), true);
    expect(g.ready).toBe(false);
    const key = g.checks.find((c) => c.key === "key");
    expect(key?.ok).toBe(false);
    expect(key?.detail).toMatch(/no validator key/i);
  });
});

describe("state mapping (contract §1 Validators)", () => {
  it("Not a validator: no bond", () => {
    expect(validatorState(ready({ bondDeployed: false, bondActivated: null })).word).toBe("Not a validator");
  });
  it("Bond locked · not staked: MemberBond.activated() = false", () => {
    expect(validatorState(ready()).word).toBe("Bond locked · not staked");
  });
  // chain#319: isActive is already true AT REGISTRATION, before consensus activation, so the
  // waiting state is keyed on actEpoch vs currentEpoch(), never on isActive = false.
  const registered = { bondActivated: true, pubkeySource: "bond" as const, registryStatus: 1 as const, registryActive: true };
  it("Registered · active from epoch N while actEpoch > currentEpoch, even though isActive is already true", () => {
    const v = validatorState(ready({ ...registered, activationEpoch: 502, currentEpoch: 500 }));
    expect(v.word).toBe("Registered · active from epoch 502");
    expect(v.detail).toContain("502");
  });
  it("Validator active only when isActive AND actEpoch <= currentEpoch", () => {
    expect(validatorState(ready({ ...registered, activationEpoch: 500, currentEpoch: 500 })).word).toBe("Validator active");
    expect(validatorState(ready({ ...registered, activationEpoch: 498, currentEpoch: 500 })).word).toBe("Validator active");
    expect(validatorState(ready({ ...registered, activationEpoch: 501, currentEpoch: 500 })).word).not.toBe("Validator active");
    expect(validatorState(ready({ ...registered, activationEpoch: 500, currentEpoch: 500, registryActive: false })).word).not.toBe("Validator active");
  });
  it("Validator active notes the consensus lag and that Producing blocks is the evidence", () => {
    const d = validatorState(ready({ ...registered, activationEpoch: 500, currentEpoch: 500 })).detail;
    expect(d).toMatch(/up to an epoch/);
    expect(d).toContain("Producing blocks");
  });
  it("an unread epoch leg is unknown (—), never a guessed state", () => {
    expect(validatorState(ready({ ...registered, activationEpoch: 502, currentEpoch: null })).word).toBeNull();
    expect(validatorState(ready({ ...registered, activationEpoch: null, currentEpoch: 500 })).word).toBeNull();
  });
  it("Exiting / Slashed from registry status 2 / 3", () => {
    expect(validatorState(ready({ bondActivated: true, registryStatus: 2 })).word).toBe("Exiting");
    expect(validatorState(ready({ bondActivated: true, registryStatus: 3 })).word).toBe("Slashed");
  });
  // The key stays Exiting after the payout (validatorInfo.status never moves on), and a
  // non-Byzantine slash ALSO leaves it Exiting while it draws bonded, then escrow, then vested
  // rewards (ValidatorRegistry._slash, citrate-chain 847ea34a). So bonded and escrow at 0 say the
  // exit SETTLED; only the registry's logs (exitOutcome) say whether the bond came back.
  const settled = { bondActivated: true, pubkeySource: "bond" as const, registryStatus: 2 as const, registryBondedWei: "0", registryEscrowWei: "0" };
  const WEI_5 = (5n * 10n ** 18n).toString();
  const outcome = (over: Partial<ExitOutcomeView> = {}): ExitOutcomeView => ({
    outcome: "returned",
    exitBlock: 1500,
    escrowedWei: WEI_32K,
    paidOutWei: (32005n * 10n ** 18n).toString(),
    paidOutBlock: 7100,
    slashedWei: null,
    scannedFrom: 1000,
    scannedTo: 7999,
    reason: null,
    ...over,
  });
  const returned = { ...settled, exitOutcome: outcome() };
  it("Bond returned only when the logs prove the payout: escrow >= principal, paid >= escrow, no slash before it", () => {
    const v = validatorState(ready(returned));
    expect(v.word).toBe("Bond returned");
    expect(v.detail).toContain("32,000 SALT");
    expect(v.detail).toContain("Withdrawn");
    expect(v.detail).toContain("7,100");
    expect(activationPhase(ready(returned), true)).toBe("exited");
  });
  it("a settled exit with no outcome read is never Bond returned: Exit settled · outcome not read", () => {
    const st = ready({ ...settled, exitOutcome: null });
    const v = validatorState(st);
    expect(v.word).toBe("Exit settled · outcome not read");
    expect(v.detail).not.toMatch(/paid out|returned/i);
    expect(activationPhase(st, true)).toBe("exitSettled");
  });
  it("an exit drained by a slash reads slashed, with the penalty, never Bond returned (review case)", () => {
    // Escrow drawn to 0 by a slash, then a terminal withdraw of rewards, even one above the principal.
    for (const paid of [WEI_5, (64000n * 10n ** 18n).toString(), null]) {
      const st = ready({ ...settled, exitOutcome: outcome({ outcome: "slashed", slashedWei: WEI_32K, paidOutWei: paid, paidOutBlock: paid ? 7100 : null }) });
      const v = validatorState(st);
      expect(v.word).toBe("Exit settled · slashed");
      expect(v.detail).toContain("32,000 SALT");
      expect(v.detail).toContain("Slashed");
      expect(v.detail).not.toMatch(/returned/i);
      expect(activationPhase(st, true)).toBe("exitSettled");
      expect(activationGrantCopy(st)).not.toMatch(/returned|paid out/i);
    }
  });
  it("a slash before the exit (escrowed less than the principal) says what the exit escrowed", () => {
    const st = ready({ ...settled, exitOutcome: outcome({ outcome: "slashed", escrowedWei: (30400n * 10n ** 18n).toString(), slashedWei: null }) });
    const v = validatorState(st);
    expect(v.word).toBe("Exit settled · slashed");
    expect(v.detail).toContain("30,400 of the bond's 32,000 SALT");
  });
  it("an outcome the logs could not show reads not read, with the reason", () => {
    const st = ready({ ...settled, exitOutcome: outcome({ outcome: "notRead", paidOutWei: null, paidOutBlock: null, reason: "no payout logged in blocks 1,000–7,999" }) });
    const v = validatorState(st);
    expect(v.word).toBe("Exit settled · outcome not read");
    expect(v.detail).toContain("no payout logged");
    expect(activationPhase(st, true)).toBe("exitSettled");
  });
  it("a 'returned' outcome whose own amounts do not prove it is not Bond returned (fail closed)", () => {
    for (const over of [
      { escrowedWei: (31999n * 10n ** 18n).toString() },
      { paidOutWei: (31999n * 10n ** 18n).toString() },
      { escrowedWei: null },
      { paidOutWei: null },
      { paidOutBlock: null },
      { slashedWei: "1" },
      { escrowedWei: "not a number" },
    ]) {
      const st = ready({ ...settled, exitOutcome: outcome(over) });
      expect(validatorState(st).word, JSON.stringify(over)).not.toBe("Bond returned");
      expect(activationPhase(st, true)).toBe("exitSettled");
    }
    expect(validatorState(ready({ ...returned, principalWei: null })).word).not.toBe("Bond returned");
  });
  it("still Exiting while the registry holds the escrow, or while bonded or escrow is unread, whatever the outcome says", () => {
    for (const over of [{ registryEscrowWei: WEI_32K }, { registryEscrowWei: null }, { registryBondedWei: null }, { registryBondedWei: "1" }]) {
      expect(validatorState(ready({ ...returned, ...over })).word).toBe("Exiting");
      expect(activationPhase(ready({ ...returned, ...over }), true)).toBe("exiting");
    }
  });
  it("the grant copy says returned only for a proven return, and never staked once the exit settled", () => {
    expect(activationGrantCopy(ready(returned))).toContain("returned");
    for (const st of [ready(returned), ready({ ...settled, exitOutcome: null }), ready({ ...settled, exitOutcome: outcome({ outcome: "slashed", slashedWei: WEI_32K }) })]) {
      expect(activationGrantCopy(st)).not.toContain("so the grant is staked");
    }
    expect(activationGrantCopy(ready({ ...settled, exitOutcome: null }))).not.toMatch(/returned|paid out/i);
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
    validatorState(ready({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 502 })).detail,
    validatorState(ready({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 499 })).detail,
    validatorState(ready({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 502 })).word ?? "",
  ];
  it.each(texts.map((t, i) => [i, t]))("text %i stays inside the contract", (_i, t) => {
    expect(t).not.toMatch(/\bearn|income|projected|estimate|\bper month\b|\bAPY\b|\byield\b|Validating|Finali[sz]ed|\bprice\b|next epoch/i);
  });
});

describe("step phase: a sent activation is Pending until the chain shows it", () => {
  it("pending while the receipt is absent", () => {
    expect(activationPhase(ready({ tx: { hash: "0xabc", included: false, blockNumber: null, success: null } }), true)).toBe("pending");
  });
  it("failed with the block when the receipt reverted", () => {
    expect(activationPhase(ready({ tx: { hash: "0xabc", included: true, blockNumber: 501, success: false } }), true)).toBe("failed");
  });
  it("registered until actEpoch is reached, active once isActive and actEpoch <= currentEpoch", () => {
    expect(activationPhase(ready({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 502 }), true)).toBe("registered");
    expect(activationPhase(ready({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 500 }), true)).toBe("active");
  });
  it("ready / blocked from the gate, reading with no data", () => {
    expect(activationPhase(ready(), true)).toBe("ready");
    expect(activationPhase(ready({ membershipActive: false }), true)).toBe("blocked");
    expect(activationPhase(null, true)).toBe("reading");
  });
});

describe("the bond's registered key vs this node's key", () => {
  it("warns when the bond was activated with a different key than this node's", () => {
    const w = keyMismatch(ready({ bondActivated: true, bondPubkey: "0x" + "09".repeat(32), nodePubkey: "0x" + "07".repeat(32) }));
    expect(w).toMatch(/different/i);
  });
  it("says nothing when they match, or when either key is unknown", () => {
    expect(keyMismatch(ready({ bondActivated: true, bondPubkey: "0x" + "07".repeat(32) }))).toBeNull();
    expect(keyMismatch(ready({ bondActivated: true, bondPubkey: "0x" + "09".repeat(32), nodePubkey: null }))).toBeNull();
    expect(keyMismatch(ready())).toBeNull();
  });
});

describe("a recorded pending tx outranks the gate", () => {
  it("is pending when the step holds a tx hash the read did not include", () => {
    expect(activationPhase(ready({ tx: null }), true, "0xfeed")).toBe("pending");
  });
  it("is ready with no recorded tx", () => {
    expect(activationPhase(ready({ tx: null }), true, null)).toBe("ready");
  });
});

describe("Synced is a two-sided band: a local node AHEAD of the public head is not Synced", () => {
  // Public 500,000. A local node ahead of it is on a divergent branch or reading a stale public
  // RPC, and must not pass the gate into the staking ceremony.
  it("5 blocks ahead is within the band", () => {
    expect(isNetworkSynced(ready({ headBlock: 500_000, localBlock: 500_005 }))).toBe(true);
  });
  it("6 blocks ahead is not Synced", () => {
    expect(isNetworkSynced(ready({ headBlock: 500_000, localBlock: 500_006 }))).toBe(false);
  });
  it("a wildly divergent local height (10,000 ahead) is not Synced and blocks the gate with its reason", () => {
    const st = ready({ headBlock: 500_000, localBlock: 510_000 });
    expect(isNetworkSynced(st)).toBe(false);
    const g = activationGate(st, true);
    expect(g.ready).toBe(false);
    const synced = g.checks.find((c) => c.key === "synced");
    expect(synced?.ok).toBe(false);
    expect(synced?.detail).toMatch(/10,000 blocks ahead/);
  });
});

describe("TRIPWIRE: sync-gate arithmetic lives in one shared, two-sided helper", () => {
  const root = join(__dirname, "..");
  const files: string[] = [];
  const walk = (d: string) => {
    for (const f of readdirSync(d)) {
      const p = join(d, f);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(ts|tsx)$/.test(f) && !/\.test\.(ts|tsx)$/.test(f)) files.push(p);
    }
  };
  walk(root);
  it("no source subtracts local and head heights except syncGap, and the gate takes its absolute value", () => {
    const heightMath = /(headBlock|localBlock)\s*-\s*[\w.]*(headBlock|localBlock)/;
    const hits: string[] = [];
    for (const f of files) {
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (heightMath.test(line)) hits.push(`${f.slice(root.length + 1)}:${i + 1}`);
        });
    }
    expect(hits).toHaveLength(1); // the one subtraction, inside syncGap
    const src = readFileSync(join(root, "onboarding/activateValidator.ts"), "utf8");
    const helper = src.slice(src.indexOf("export function syncGap"), src.indexOf("export function isNetworkSynced"));
    expect(helper).toMatch(/headBlock|localBlock/);
    const gate = src.slice(src.indexOf("export function isNetworkSynced"));
    expect(gate.slice(0, gate.indexOf("\n}\n"))).toMatch(/Math\.abs\(\s*syncGap\(/);
  });
});

describe("TRIPWIRE: a bond is said to be returned only from the read that proves it", () => {
  const root = join(__dirname, "..");
  const files: string[] = [];
  const walk = (d: string) => {
    for (const f of readdirSync(d)) {
      const p = join(d, f);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(ts|tsx)$/.test(f) && !/\.test\.(ts|tsx)$/.test(f)) files.push(p);
    }
  };
  walk(root);
  // A payout claim about the bond, grant, principal or stake ("Bond returned", "the exit has paid
  // out", "stake refunded"). Registry zeros alone can be a slash, so a new claim like this
  // anywhere else must go through bondReturned() and its exitOutcome read.
  const claim = /\b(bond|grant|principal|stake|exit)\b[^"'`\n]{0,40}\b(returned|paid out|refunded)\b/i;
  it("no source outside activateValidator.ts claims a bond payout", () => {
    const hits: string[] = [];
    for (const f of files) {
      if (f.endsWith(join("onboarding", "activateValidator.ts"))) continue;
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (claim.test(line) && !/^\s*(\/\/|\*|\/\*)/.test(line)) hits.push(`${f.slice(root.length + 1)}:${i + 1}: ${line.trim()}`);
        });
    }
    expect(hits).toEqual([]);
  });
  it("in activateValidator.ts every payout claim sits behind bondReturned(), and bondReturned reads exitOutcome", () => {
    const src = readFileSync(join(root, "onboarding/activateValidator.ts"), "utf8");
    const fn = src.slice(src.indexOf("function bondReturned("));
    const body = fn.slice(0, fn.indexOf("\n}\n"));
    expect(body).toContain("exitOutcome");
    expect(body).toMatch(/outcome\s*[!=]==\s*"returned"/);
    // Every line that claims a payout is inside a branch guarded by bondReturned(...).
    const lines = src.split("\n");
    const offenders: string[] = [];
    lines.forEach((line, i) => {
      if (!claim.test(line) || /^\s*(\/\/|\*|\/\*)/.test(line) || line.includes("ValidatorWord") || /^\s*\|\s*"/.test(line)) return;
      const ctx = lines.slice(Math.max(0, i - 6), i + 1).join("\n");
      if (!/bondReturned\(/.test(ctx)) offenders.push(`${i + 1}: ${line.trim()}`);
    });
    expect(offenders).toEqual([]);
  });
});
