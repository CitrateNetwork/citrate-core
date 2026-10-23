// K3 Node › Status and the one activation gate (#366 canOfferActivation, DGX N2/N3/N4 on #362).
//
// The Node page offers "Activate validator" only when chainTruth's canOfferActivation allows it
// (registry status, registered stake, MemberBond.activated(), the activation window
// MemberBond.isUnlocked() and actEpoch, all from the store's reads) AND the page itself reads
// "Synced" against a fresh public head (contract §1: production stays unarmed under Sync unknown).
// When it isn't offered, the card says the gate's reason. The validator word and the eligible count
// come from the same store reads, not from props nobody passes.
import { afterEach, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { freshState, type AppState } from "../../shell/state";
import type { Store } from "../../shell/store";
import { CONSENSUS_LAG_NOTE, canOfferActivation, type MemberBondState, type ValidatorActivation } from "../../shell/chainTruth";
import { NodeStatusView } from "./NodeStatus";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function mount(el: React.ReactElement): Promise<HTMLDivElement> {
  act(() => root?.unmount());
  host?.remove();
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(el);
  });
  return host;
}

function stubStore(): Store & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    startNode: () => void calls.push("startNode"),
    stopNode: () => void calls.push("stopNode"),
    go: (r: string) => void calls.push("go:" + r),
    toast: (m: string) => void calls.push("toast:" + m),
    walletIsLinked: () => true,
    linkWallet: async () => void calls.push("linkWallet"),
    activateValidator: async () => {
      calls.push("activateValidator");
      return true;
    },
  } as unknown as Store & { calls: string[] };
}

const live = {
  publicHead: 510_032,
  publicHeadAgeMs: 2_000,
  publicHeadError: false,
  statusAgeMs: 1_000,
  compat: "compatible" as const,
  mode: "tauri" as const,
  statusError: null,
};

const KEY = "0x" + "ab".repeat(48);
const NONE: ValidatorActivation = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 40 };
const PENDING: ValidatorActivation = { isActive: true, status: 1, actEpoch: 42, currentEpoch: 40 };
const ACTIVE: ValidatorActivation = { isActive: true, status: 1, actEpoch: 38, currentEpoch: 40 };
const EXITING: ValidatorActivation = { isActive: false, status: 2, actEpoch: 38, currentEpoch: 40 };
const SLASHED: ValidatorActivation = { isActive: false, status: 3, actEpoch: 38, currentEpoch: 40 };
const LOCKED: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: false };
const UNLOCKED: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: true };
const LOCK_UNREAD: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: null };
const ACTIVATED: MemberBondState = { deployed: true, activated: true, pubkey: KEY, unlocked: false };
const BOND_UNREAD: MemberBondState = { deployed: true, activated: null, pubkey: null, unlocked: null };

/** A member on a synced node: granted, with the given registry and bond reads. */
const member = (patch: Partial<AppState> = {}): AppState => ({
  ...freshState("p1"),
  stage: "done",
  node: "synced",
  height: 510_030,
  peers: 4,
  hasGrant: true,
  bondedStake: 0,
  validatorActivation: NONE,
  memberBond: LOCKED,
  ...patch,
});

const activateButton = (h: HTMLElement) => [...h.querySelectorAll("button")].find((b) => /Activate validator/.test(b.textContent ?? ""));
const gateText = (h: HTMLElement) => h.querySelector('[data-testid="activate-gate"]')?.textContent ?? "";

describe("Node › Status — Activate validator goes through canOfferActivation", () => {
  it("offered for a granted bond that is not activated, not staked and still inside its lock", async () => {
    const store = stubStore();
    const h = await mount(<NodeStatusView store={store} s={member()} {...live} />);
    const b = activateButton(h);
    expect(b).toBeDefined();
    expect(b?.textContent).not.toMatch(/32,000|SALT/);
    await act(async () => {
      b?.click();
    });
    expect(store.calls).toContain("activateValidator");
  });

  it("matches canOfferActivation exactly over the registry, stake, bond and lock reads", async () => {
    const activations = [null, NONE, PENDING, ACTIVE, EXITING, SLASHED];
    const bonds = [null, LOCKED, UNLOCKED, LOCK_UNREAD, ACTIVATED, BOND_UNREAD, { deployed: false, activated: null, pubkey: null }];
    for (const validatorActivation of activations) {
      for (const memberBond of bonds) {
        for (const bondedStake of [0, 32_000]) {
          for (const hasGrant of [true, false]) {
            const s = member({ validatorActivation, memberBond, bondedStake, hasGrant });
            const h = await mount(<NodeStatusView store={stubStore()} s={s} {...live} />);
            const want = canOfferActivation(bondedStake, validatorActivation, hasGrant, memberBond);
            const label = JSON.stringify({ validatorActivation, memberBond, bondedStake, hasGrant });
            expect(activateButton(h) != null, label).toBe(want);
            if (!want) expect(gateText(h), label).not.toBe("");
          }
        }
      }
    }
  });

  it("an unlocked bond: no Activate, and the reason is the closed activation window", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={member({ memberBond: UNLOCKED })} {...live} />);
    expect(activateButton(h)).toBeUndefined();
    expect(gateText(h)).toContain("activation window closed");
  });

  it("an unread lock fails closed: no Activate, 'bond lock not read yet'", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={member({ memberBond: LOCK_UNREAD })} {...live} />);
    expect(activateButton(h)).toBeUndefined();
    expect(gateText(h)).toContain("bond lock not read yet");
  });

  it("an unread registry or bond: no Activate, and the gate's '—' reason", async () => {
    const unreadRegistry = await mount(<NodeStatusView store={stubStore()} s={member({ validatorActivation: null, bondedStake: 32_000 })} {...live} />);
    expect(activateButton(unreadRegistry)).toBeUndefined();
    expect(gateText(unreadRegistry)).toContain("registry not read yet");
    const unreadBond = await mount(<NodeStatusView store={stubStore()} s={member({ memberBond: BOND_UNREAD })} {...live} />);
    expect(activateButton(unreadBond)).toBeUndefined();
    expect(gateText(unreadBond)).toMatch(/^— · /);
  });

  it("Exiting, Slashed, pending and active keys: no Activate, and the validator word is the reason", async () => {
    for (const [validatorActivation, word] of [
      [EXITING, "Exiting"],
      [SLASHED, "Slashed"],
      [PENDING, "Registered · active from epoch 42"],
      [ACTIVE, "Validator active"],
    ] as const) {
      const h = await mount(<NodeStatusView store={stubStore()} s={member({ validatorActivation, memberBond: ACTIVATED, bondedStake: 32_000 })} {...live} />);
      expect(activateButton(h), word).toBeUndefined();
      expect(gateText(h), word).toContain(word);
    }
  });

  it("N3: a registered key whose bond reads not activated is not told its bond is already activated", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={member({ validatorActivation: PENDING, memberBond: LOCKED })} {...live} />);
    expect(activateButton(h)).toBeUndefined();
    expect(gateText(h)).not.toMatch(/already activated/i);
    expect(gateText(h)).toContain("Registered · active from epoch 42");
  });

  it("N4: an offerable bond is not offered unless the page reads Synced, and the card says so", async () => {
    for (const over of [
      { publicHead: null, publicHeadAgeMs: null },
      { publicHeadAgeMs: 61_000 },
      { publicHeadError: true },
      { publicHead: 510_100 },
    ]) {
      const h = await mount(<NodeStatusView store={stubStore()} s={member()} {...live} {...over} />);
      expect(activateButton(h), JSON.stringify(over)).toBeUndefined();
      expect(gateText(h), JSON.stringify(over)).toContain("Activation opens once this node reads Synced");
    }
  });

  it("the store's own state must read synced too (arming re-checks the tip in Rust)", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={member({ node: "syncing" })} {...live} />);
    expect(activateButton(h)).toBeUndefined();
  });

  it("the validator word in the sentence and row comes from the store's registry reads", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={member({ validatorActivation: ACTIVE, memberBond: ACTIVATED, bondedStake: 32_000 })} {...live} />);
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced · Validator active");
    expect(h.querySelector('[data-testid="validator-word"]')?.textContent).toBe("Validator active" + CONSENSUS_LAG_NOTE);
    const unread = await mount(<NodeStatusView store={stubStore()} s={member({ validatorActivation: null, bondedStake: 32_000 })} {...live} />);
    expect(unread.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced");
    expect(unread.querySelector('[data-testid="validator-word"]')?.textContent).toBe("— · registry not read yet");
  });

  it("Eligible validators is the store's registry count, '—' until read", async () => {
    const read = await mount(<NodeStatusView store={stubStore()} s={member({ validatorCounts: { registered: 9, eligible: 7, pending: 2 } })} {...live} />);
    expect(read.querySelector('[data-testid="eligible-validators"]')?.textContent).toBe("7");
    const unread = await mount(<NodeStatusView store={stubStore()} s={member({ validatorCounts: null })} {...live} />);
    expect(unread.querySelector('[data-testid="eligible-validators"]')?.textContent ?? "").toMatch(/^—/);
  });
});

// ---------------------------------------------------------------------------------------------
// Tripwire (STD): no activation path on the Node page bypasses the one gate.
describe("tripwire — the Node page has one activation gate", () => {
  const dir = join(process.cwd(), "src/surfaces/node");
  const prod = readdirSync(dir).filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f));
  const src = (f: string) => readFileSync(join(dir, f), "utf8");

  it("every Node page file that calls activateValidator decides first with activationOffer", () => {
    for (const f of prod) {
      const body = src(f);
      if (!body.includes("activateValidator(")) continue;
      expect(body, f).toContain("activationOffer(");
      expect(body.indexOf("activationOffer("), f).toBeLessThan(body.indexOf("activateValidator("));
      // One call site, behind the offer's early return.
      expect(body.split("activateValidator(").length - 1, f).toBe(1);
      expect(body.indexOf("if (!offer.offer)"), f).toBeGreaterThan(-1);
      expect(body.indexOf("activateValidator("), f).toBeGreaterThan(body.indexOf("if (!offer.offer)"));
    }
  });

  it("activationOffer offers only inside canOfferActivation", () => {
    const words = src("nodeWords.ts");
    const start = words.indexOf("export function activationOffer(");
    expect(start).toBeGreaterThan(-1);
    const body = words.slice(start, words.indexOf("\n}\n", start));
    const gate = body.indexOf("if (canOfferActivation(");
    const after = body.indexOf("const word = validatorWord(");
    expect(gate).toBeGreaterThan(-1);
    expect(body.match(/offer: true/g)?.length).toBe(1);
    expect(body.indexOf("offer: true")).toBeGreaterThan(gate);
    expect(body.indexOf("offer: true")).toBeLessThan(after);
  });

  it("no Node page file arms mining, registers or activates a validator directly", () => {
    for (const f of prod) {
      expect(src(f), f).not.toMatch(/armMining\(|registerValidator\(|\bactivate\(/);
    }
  });

  it("there is no second, page-local activation gate", () => {
    for (const f of prod) {
      expect(src(f), f).not.toMatch(/\bactivateGate\b|registryStatus|bondActivated/);
    }
  });
});
