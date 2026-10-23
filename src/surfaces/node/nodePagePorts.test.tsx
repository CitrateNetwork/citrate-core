// The 0.5.1 Node surface (Node.tsx) is no longer mounted: the K3 Node page (Status, Services,
// Rewards, Advanced) replaced it in the five-place navigation. Its truth tests are moved here and
// run against the page members actually see, so deleting Node.tsx loses no guarantee:
//   - Q-A.4b vitals honesty, SCL-S0.6 blocked start, the mining-gate hold;
//   - C1-V validator words (actEpoch, Exiting/Slashed, unread registry), the activation offer
//     (#348 no re-offer, chain#331 window), the producer sample (#348 round 2, C1-V5 bond key);
//   - #348 rewards: read, shown and claimed through an activated bond whatever the stake, re-read
//     when the bond reconciles; contract §1 Rewards wording; the one grant story (C1-V V5).
// Sync percentages (Q-A.4b "62%") are not ported: contract §1 forbids a sync percentage, and the
// K3 page shows "Syncing · N blocks behind" instead (nodeWords.test.ts).
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));

import { freshState, type AppState, type PeerRow } from "../../shell/state";
import type { Store } from "../../shell/store";
import { CONSENSUS_LAG_NOTE, type MemberBondState } from "../../shell/chainTruth";
import { BETANET_CARRYOVER, GRANT_DISCLAIMER } from "../../data/grantCopy";
import { NodeStatusView } from "./NodeStatus";
import { NodeRewards } from "./NodeRewards";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
const ACT_ON = { isActive: true, status: 1, actEpoch: 499, currentEpoch: 499 };
const ACT_PENDING = { isActive: true, status: 1, actEpoch: 501, currentEpoch: 499 };
const ACT_NONE = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 499 };
const ME = "0x" + "aa".repeat(48);
const OTHER = "0x" + "bb".repeat(48);
const bond = (activated: boolean, unlocked: boolean | null = false): MemberBondState => ({ deployed: true, activated, pubkey: activated ? ME : null, unlocked });

function stubStore(over: Record<string, unknown> = {}): Store {
  return {
    walletIsLinked: () => true,
    refreshEarnings: vi.fn(async () => {}),
    claimRewards: vi.fn(async () => {}),
    activateValidator: vi.fn(async () => true),
    linkWallet: async () => {},
    toast: () => {},
    go: () => {},
    startNode: () => {},
    stopNode: () => {},
    ...over,
  } as unknown as Store;
}

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, stage: "done", node: "synced", peers: 3, syncPct: 100, height: 499_058, hasGrant: true, s5StakeWei: wei(32000), ...over };
}

/** The K3 Node page as a member sees it on a node whose status and public head are fresh. */
function page(s: AppState, store: Store = stubStore()): string {
  const head = s.height > 0 ? s.height : null;
  return (
    renderToStaticMarkup(
      <NodeStatusView store={store} s={s} publicHead={head} publicHeadAgeMs={1_000} publicHeadError={false} statusAgeMs={1_000} compat="compatible" mode="tauri" statusError={null} />,
    ) + renderToStaticMarkup(<NodeRewards store={store} s={s} mode="tauri" />)
  );
}

describe("vitals honesty (Q-A.4b, from Node.tsx)", () => {
  const seeded = (over: Partial<AppState> = {}) => member({ node: "validating", peers: 7, blocksProposed: 412, cpu: 23, ram: 780, ...over });

  it("no fabricated blocks proposed, cpu, ram or peer rows; the real peer count", () => {
    const fakeRows: PeerRow[] = [{ id: "16Uiu2HAdeadbeef", lat: "42 ms", region: "x" } as unknown as PeerRow];
    const html = page(seeded({ peerRows: fakeRows }));
    for (const fake of ["412", "23 %", "780 MB", "16Uiu2HAdeadbeef", "42 ms"]) expect(html).not.toContain(fake);
    expect(html).toContain("7 direct peers");
  });

  it("no cosmetic Pause or Resume; Stop is the real control", () => {
    const html = page(seeded());
    expect(html).not.toContain(">Pause<");
    expect(html).not.toContain(">Resume<");
    expect(html).toContain(">Stop node<");
  });

  it("a bonded validator is not told it is unregistered; an unbonded member is", () => {
    expect(page(seeded({ bondedStake: 32_000, validatorActivation: ACT_ON }))).not.toContain("not a registered validator");
    expect(page(member({ bondedStake: 0 }))).toContain("— · not a registered validator");
  });

  it("a seeded blocks-proposed count never shows as a tally", () => {
    expect(page(seeded({ bondedStake: 32_000, validatorActivation: ACT_ON, blocksProposed: 7 }))).not.toContain(">7<");
  });
});

describe("blocked start and mining hold (SCL-S0.6, mining gate, from Node.tsx)", () => {
  it("names the holder of a refused start and the action", () => {
    const msg = "The chain database is held by process 4242 (/Applications/Citrate Core.app/Contents/MacOS/citrate). Quit that process, then start the node again.";
    const html = page({ ...freshState("p1"), stage: "done", node: "error", nodeBlocked: msg });
    expect(html).toContain("process 4242");
    expect(html).toContain("/Applications/Citrate Core.app/Contents/MacOS/citrate");
    expect(html).toContain("Quit that process");
    expect(page(member({ nodeBlocked: null }))).not.toContain("Quit that process");
  });

  it("shows why block production is held", () => {
    const hold = "Block production is off: 1 other producer(s) are live, at least 2 are needed first.";
    expect(page(member({ miningHold: hold }))).toContain("1 other producer(s) are live");
    expect(page(member({ miningHold: null }))).not.toContain("Block production is off");
  });
});

describe("validator words (C1-V, contract v1.1 §1, from Node.tsx)", () => {
  it("an unread registry is '—', never active", () => {
    const html = page(member({ bondedStake: 32_000, validatorActivation: null }));
    expect(html).toContain("— · registry not read yet");
    expect(html).not.toContain(">Validator active<");
  });

  it("pending shows the real N; active shows the consensus lag note; no election odds", () => {
    const p = page(member({ bondedStake: 32_000, validatorActivation: ACT_PENDING }));
    expect(p).toContain("Registered · active from epoch 501");
    expect(p).not.toContain(">Validator active<");
    expect(p).not.toContain("Election odds");
    const a = page(member({ node: "validating", bondedStake: 32_000, validatorActivation: ACT_ON }));
    expect(a).toContain("Validator active");
    expect(a).toContain(CONSENSUS_LAG_NOTE);
  });

  it("Exiting / Slashed for a member with stake 0, and no activation", () => {
    for (const [status, word] of [
      [2, "Exiting"],
      [3, "Slashed"],
    ] as const) {
      const html = page(member({ bondedStake: 0, validatorActivation: { ...ACT_ON, isActive: false, status }, memberBond: bond(true) }));
      expect(html).toContain(word);
      expect(html).not.toContain("Bond locked · not staked");
      expect(html).not.toContain("Activate validator");
    }
  });
});

describe("activation offer (#348, chain#331, C1-V4, from Node.tsx)", () => {
  it("an activated bond is never re-offered, registry read or not", () => {
    for (const validatorActivation of [ACT_NONE, null]) {
      const html = page(member({ memberBond: bond(true), validatorActivation }));
      expect(html).not.toContain("Activate validator");
      expect(html).not.toContain("Bond locked · not staked");
    }
  });

  it("an unlocked or unread lock: no activation, and the reason", () => {
    for (const [b, why] of [
      [bond(false, true), "Unlocked · not activated"],
      [bond(false, null), "bond lock not read yet"],
    ] as const) {
      const html = page(member({ memberBond: b, validatorActivation: ACT_NONE }));
      expect(html).not.toContain("Activate validator");
      expect(html).toContain(why);
    }
  });

  it("a registered stake that isn't active yet, or unread, is not offered; a bond that reads not activated is", () => {
    for (const validatorActivation of [ACT_PENDING, null]) expect(page(member({ bondedStake: 32_000, validatorActivation }))).not.toContain("Activate validator");
    expect(page(member({ bondedStake: 0, validatorActivation: null, memberBond: bond(false) }))).toContain("Activate validator");
    expect(page(member({ bondedStake: 0, validatorActivation: ACT_NONE, memberBond: bond(false) }))).toContain("Activate validator");
  });

  it("the validator card tells the one grant story to a member with a grant", () => {
    const html = page(member({ node: "synced" }));
    expect(html).toContain(GRANT_DISCLAIMER);
    expect(html).toContain(BETANET_CARRYOVER);
    expect(html).not.toContain("LiquidStakingPool — your full membership grant");
  });
});

describe("producing blocks (contract §1/§2, #348 round 2, C1-V5, from Node.tsx)", () => {
  const v = (over: Partial<AppState>) => member({ bondedStake: 32_000, validatorActivation: ACT_ON, ...over });

  it("seen in the sample: 'Producing blocks', naming the window", () => {
    const html = page(v({ validatorPubkey: OTHER, producers: { head: 1, window: 100, proposers: [OTHER] } }));
    expect(html).toContain(">Producing blocks<");
    expect(html).toContain("sampled over the last 100 blocks");
    expect(html).not.toContain("validator key not read yet");
  });

  it("not seen: 'Not seen producing', never 'not producing'", () => {
    const html = page(v({ validatorPubkey: ME, producers: { head: 1, window: 100, proposers: [OTHER] } }));
    expect(html).toContain("Not seen producing in the last 100 blocks");
    expect(html).not.toContain(">Producing blocks<");
    expect(html.toLowerCase()).not.toContain("not producing");
  });

  it("a short sample is incomplete, and a failed sample is unavailable", () => {
    const short = page(v({ validatorPubkey: ME, producers: { head: 1, window: 37, proposers: [ME] } }));
    expect(short).toContain("— · producer sample incomplete: only 37 of 100 blocks came back");
    expect(short).not.toContain(">Producing blocks<");
    expect(short).not.toContain("last 100 blocks");
    const failed = page(v({ validatorPubkey: ME, producers: null, producersError: "rpc 503" }));
    expect(failed).toContain("— · producer sample unavailable: rpc 503");
  });

  it("no key yet, or not sampled yet, say so", () => {
    expect(page(v({ validatorPubkey: null, producers: null }))).toContain("— · validator key not read yet");
    expect(page(v({ validatorPubkey: ME, producers: null, producersError: null }))).toContain("— · not sampled yet");
  });
});

describe("Node › Rewards from the store's rewardsOf read (#348, contract §1 Rewards, from Node.tsx)", () => {
  it("names rewardsOf, splits claimable and maturing, and has no off-chain estimates", () => {
    const html = page(member({ bondedStake: 32_000, validatorActivation: ACT_ON, rewards: { totalWei: wei(30), claimableWei: wei(20) } }));
    expect(html).not.toContain("off-chain estimate");
    expect(html).not.toContain("ContributionAccounting.claimable");
    for (const t of ["Validator rewards", "Claimable now", "Maturing", "ValidatorRegistry.rewardsOf", "30.00", "20.00", "10.00"]) expect(html).toContain(t);
  });

  it("an exited validator's claimable rewards show through the bond, and Claim is enabled", async () => {
    const store = stubStore();
    // v1.2 §1: the bond's claimRewards() needs MemberBond.isKycVerified() on chain (true here).
    const s = member({ bondedStake: 0, memberBond: { ...bond(true), kycVerified: true }, validatorActivation: { ...ACT_ON, isActive: false, status: 2 }, rewards: { totalWei: wei(30), claimableWei: wei(20) } });
    const html = renderToStaticMarkup(<NodeRewards store={store} s={s} mode="tauri" />);
    expect(html).toContain("20.00");
    expect(html).toMatch(/<button[^>]*>\s*Claim/);
    expect(html).not.toMatch(/<button[^>]*disabled=""[^>]*>\s*Claim/);
  });

  it("Claim stays disabled under the 5 SALT dust threshold, and says how far", () => {
    const html = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, rewards: { totalWei: wei(3), claimableWei: wei(3) } })} mode="tauri" />);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>\s*Claim/);
    expect(html).toContain("2.00 SALT to go");
  });

  it("an activated bond whose rewards aren't read yet says 'not read yet', not 'Not a validator'", () => {
    const html = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 0, memberBond: bond(true), rewards: null })} mode="tauri" />);
    expect(html).toContain("not read yet");
    expect(html).not.toContain("Not a validator");
  });

  it("a member who is not a validator sees the word, never a figure", () => {
    const html = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ hasGrant: false, bondedStake: 0, validatorActivation: ACT_NONE, memberBond: null, rewards: null, claimable: 9.41 })} mode="tauri" />);
    expect(html).not.toContain("9.41");
    expect(html).toContain("Not a validator");
  });

  it("an unread validator state is '—' with the reason, not 'Not a validator'", () => {
    const html = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 0, validatorActivation: null, memberBond: null, rewards: null })} mode="tauri" />);
    expect(html).not.toContain("Not a validator");
    expect(html).toContain("— · registry not read yet");
  });

  it("no seeded ack age or slashing-protection claim", () => {
    const html = page(member({ bondedStake: 32_000, validatorActivation: ACT_ON }));
    expect(html).not.toMatch(/last ack \d+s ago/);
    expect(html).not.toContain("slashing protection");
    expect(html).not.toContain("NematocystSlashing");
  });

  it("a read failure is the error card, and the missing proposer key its own state", () => {
    const err = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, rewards: null, rewardsError: "rpc 503" })} mode="tauri" />);
    expect(err).toContain("Couldn&#x27;t read rewards");
    expect(err).toContain("rpc 503");
    const key = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, rewards: null, rewardsError: "proposer key not minted yet" })} mode="tauri" />);
    expect(key).toContain("Rewards need this node");
  });
});

describe("Node › Rewards reads through the store (#348 round 2)", () => {
  let root: Root | null = null;
  afterEach(() => {
    act(() => root?.unmount());
    root = null;
  });

  it("reads on open, and again when the bond reconciles to activated while the tab stays open", () => {
    const store = stubStore();
    const calls = store.refreshEarnings as unknown as ReturnType<typeof vi.fn>;
    root = createRoot(document.createElement("div"));
    act(() => root?.render(<NodeRewards store={store} s={member({ bondedStake: 0, memberBond: null })} mode="tauri" />));
    const before = calls.mock.calls.length;
    expect(before).toBeGreaterThanOrEqual(1);
    act(() => root?.render(<NodeRewards store={store} s={member({ bondedStake: 0, memberBond: bond(true) })} mode="tauri" />));
    expect(calls.mock.calls.length).toBeGreaterThan(before);
  });

  it("Claim runs the store's claim (the ceremony), never a local balance change", () => {
    const store = stubStore();
    const host = document.createElement("div");
    root = createRoot(host);
    act(() => root?.render(<NodeRewards store={store} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, rewards: { totalWei: wei(30), claimableWei: wei(20) } })} mode="tauri" />));
    const b = [...host.querySelectorAll("button")].find((x) => x.textContent?.trim() === "Claim");
    act(() => b?.click());
    expect(store.claimRewards).toHaveBeenCalledTimes(1);
  });
});

describe("tripwire — the 0.5.1 Node surface is gone", () => {
  it("nothing imports surfaces/Node", async () => {
    const { readdirSync, readFileSync, statSync } = await import("node:fs");
    const { join } = await import("node:path");
    const walk = (d: string): string[] => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]));
    const files = walk(join(process.cwd(), "src")).filter((f) => /\.tsx?$/.test(f));
    expect(files.some((f) => /src\/surfaces\/Node\.tsx$/.test(f))).toBe(false);
    for (const f of files) expect(readFileSync(f, "utf8"), f).not.toMatch(/from "(\.\.?\/)+(surfaces\/)?Node"|export \{ Node \}/);
  });
});
