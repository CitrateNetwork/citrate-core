// citrate-core#391 — Node › Rewards (and the Wallet) show node_validator_earnings, which reads
// ValidatorRegistry.rewardsOf(this node's proposer key), while the page captions the figures with
// the member's validator key: MemberBond.pubkey() where the bond defines it (C1-V5). Figures are
// accepted only when the read's proposerPubkey is that key. A mismatch, an unread bond key, an
// unread bond, or a malformed proposerPubkey fails closed: no figures, "—" with the reason.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const validatorEarnings = vi.fn();
const grantStatus = vi.fn();
const agentClaim = vi.fn();
vi.mock("../../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    node: { validatorEarnings: (...a: unknown[]) => validatorEarnings(...a) },
    membership: { grantStatus: () => grantStatus() },
    agent: { claim: (...a: unknown[]) => agentClaim(...a) },
  },
}));
vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Store, earningsKeyCheck } from "../../shell/store";
import { NodeRewards } from "./NodeRewards";
import type { MemberBondState } from "../../shell/chainTruth";

const BOND_KEY = "0x" + "aa".repeat(48);
const NODE_KEY = "0x" + "bb".repeat(48);
const FIGURES = { totalWei: "30000000000000000000", claimableWei: "20000000000000000000" };
const bond = (pubkey: string | null, activated: boolean | null = true): MemberBondState => ({ deployed: true, activated, pubkey, unlocked: false });

async function read(memberBond: MemberBondState | null, proposerPubkey: unknown, bondedStake = 32_000): Promise<Store> {
  const store = new Store();
  store.setState({ bondedStake, memberBond, rewards: null, rewardsError: null });
  validatorEarnings.mockResolvedValue({ ...FIGURES, proposerPubkey });
  await store.refreshEarnings();
  return store;
}

beforeEach(() => {
  validatorEarnings.mockReset();
  grantStatus.mockReset();
  agentClaim.mockReset();
  agentClaim.mockResolvedValue({ kind: "nothing", claimableWei: "0" });
});

describe("#391 — rewards are accepted only for the member's validator key", () => {
  it("a read for another key than MemberBond.pubkey() shows no figures, and says why", async () => {
    const store = await read(bond(BOND_KEY), NODE_KEY);
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("not your bond's validator key");
    expect(store.state.rewardsError).toContain(NODE_KEY.slice(0, 10));
  });

  it("an exited validator (stake 0, bond activated) is checked the same way", async () => {
    const store = await read(bond(BOND_KEY), NODE_KEY, 0);
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("not your bond's validator key");
  });

  it("a deployed bond whose key isn't read yet fails closed", async () => {
    const store = await read(bond(null, null), NODE_KEY);
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("bond's validator key isn't read yet");
  });

  it("an unread bond fails closed", async () => {
    const store = await read(null, NODE_KEY);
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("bond isn't read yet");
  });

  it("a missing or malformed proposerPubkey fails closed", async () => {
    for (const k of ["", null, "0xzz", 42]) {
      const store = await read(bond(BOND_KEY), k);
      expect(store.state.rewards, String(k)).toBeNull();
      expect(store.state.rewardsError, String(k)).toContain("didn't name a readable key");
    }
  });

  it("the matching key is accepted, whatever its letter case", async () => {
    const store = await read(bond(BOND_KEY), BOND_KEY.toUpperCase().replace("0X", "0x"));
    expect(store.state.rewards).toEqual(FIGURES);
    expect(store.state.rewardsError).toBeNull();
  });

  it("with no bond deployed the node's own key is the validator key (C1-V5)", async () => {
    const store = await read({ deployed: false, activated: null, pubkey: null }, NODE_KEY);
    expect(store.state.rewards).toEqual(FIGURES);
  });

  it("the reason never reads as the no-key state (Rewards matches /proposer.?key/ for that)", async () => {
    for (const [b, k] of [[bond(BOND_KEY), NODE_KEY], [bond(null, null), NODE_KEY], [null, NODE_KEY], [bond(BOND_KEY), ""]] as const) {
      const why = earningsKeyCheck(b, k);
      expect(why, JSON.stringify([b, k])).not.toBeNull();
      expect(String(why), JSON.stringify([b, k])).not.toMatch(/proposer.?key/i);
    }
  });

  it("earningsKeyCheck is the pure rule: null only for a matching key", () => {
    expect(earningsKeyCheck(bond(BOND_KEY), BOND_KEY)).toBeNull();
    expect(earningsKeyCheck({ deployed: false, activated: null, pubkey: null }, NODE_KEY)).toBeNull();
    expect(earningsKeyCheck(bond(BOND_KEY), NODE_KEY)).not.toBeNull();
    expect(earningsKeyCheck(bond(null), NODE_KEY)).not.toBeNull();
    expect(earningsKeyCheck(null, NODE_KEY)).not.toBeNull();
  });
});

describe("#391 — Node › Rewards shows the mismatch as unknown, with the reason", () => {
  it("no figures, no Claim, the error state naming both keys", async () => {
    const store = await read(bond(BOND_KEY), NODE_KEY);
    const html = renderToStaticMarkup(createElement(NodeRewards, { store, s: store.state, mode: "tauri" }));
    expect(html).toContain('data-state="error"');
    expect(html).toContain("not your bond&#x27;s validator key");
    expect(html).not.toContain("30.00");
    expect(html).not.toContain("20.00");
    expect(html).not.toMatch(/<button[^>]*>\s*Claim/);
  });
});

describe("#394 — the rewards read names the member's validator key", () => {
  // The registry answers rewardsOf for whichever key is asked; with no key, the node's own key.
  const keyed = () => validatorEarnings.mockImplementation(async (k?: string) => ({ ...FIGURES, proposerPubkey: k ?? NODE_KEY }));

  it("reads rewardsOf(MemberBond.pubkey()) for a deployed bond, so a node with another key still shows the bond's rewards", async () => {
    keyed();
    const store = new Store();
    store.setState({ bondedStake: 0, memberBond: bond(BOND_KEY), rewards: null, rewardsError: null });
    await store.refreshEarnings();
    expect(validatorEarnings).toHaveBeenCalledWith(BOND_KEY);
    expect(store.state.rewards).toEqual(FIGURES);
    expect(store.state.rewardsError).toBeNull();
  });

  it("a deployed bond whose key isn't read yet reads nothing and says why", async () => {
    keyed();
    const store = new Store();
    store.setState({ bondedStake: 32_000, memberBond: bond(null, null), rewards: null, rewardsError: null });
    await store.refreshEarnings();
    expect(validatorEarnings).not.toHaveBeenCalled();
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("bond's validator key isn't read yet");
  });

  it("with no bond deployed it reads the node's own key (no key sent)", async () => {
    keyed();
    const store = new Store();
    store.setState({ bondedStake: 32_000, memberBond: { deployed: false, activated: null, pubkey: null }, rewards: null, rewardsError: null });
    await store.refreshEarnings();
    expect(validatorEarnings).toHaveBeenCalledTimes(1);
    expect(validatorEarnings.mock.calls[0][0]).toBeUndefined();
    expect(store.state.rewards).toEqual(FIGURES);
  });

  const K32 = "0x" + "55".repeat(32);
  const grant = { bondDeployed: true, bondActivated: true, hasValidator: false, bondPubkey: K32 };
  const claimStore = () => {
    const store = new Store();
    (store as unknown as { assertPayoutKyc: () => Promise<boolean> }).assertPayoutKyc = async () => true;
    (store as unknown as { openWalletReview: () => void }).openWalletReview = () => {};
    return store;
  };

  it("Claim's pre-check reads rewardsOf for the bond's key, the key the bond claims for", async () => {
    grantStatus.mockResolvedValue(grant);
    agentClaim.mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await claimStore().claimRewards();
    expect(agentClaim).toHaveBeenCalledWith({ validator: true, pubkey: K32 });
  });

  it("Claim stops with the reason when the bond claims but its key isn't read", async () => {
    grantStatus.mockResolvedValue({ ...grant, bondPubkey: null });
    const store = claimStore();
    await store.claimRewards();
    expect(agentClaim).not.toHaveBeenCalled();
    expect(String(store.state.toast)).toContain("validator key isn't read yet");
  });
});

// Tripwire (STD): every non-test read of node_validator_earnings that uses its figures checks the key
// first, and the figures land in state only through refreshEarnings.
describe("tripwire — every earnings read path checks the key", () => {
  const files = ["src/shell/store.ts", "src/surfaces/node/NodeRewards.tsx", "src/surfaces/Wallet.tsx", "src/surfaces/Dashboard.tsx"];
  const src = (f: string) => readFileSync(join(process.cwd(), f), "utf8");

  it("each bridge.node.validatorEarnings( call either ignores the figures or runs earningsKeyCheck before using them", () => {
    let calls = 0;
    for (const f of files) {
      const s = src(f);
      let at = s.indexOf("bridge.node.validatorEarnings(");
      while (at !== -1) {
        calls++;
        // The enclosing method: from the previous method header to the next one.
        const start = Math.max(s.lastIndexOf("\n  async ", at), s.lastIndexOf("\n  private async ", at));
        const end = s.indexOf("\n  }\n", at);
        const body = s.slice(start, end);
        if (/\.(totalWei|claimableWei)\b/.test(body)) {
          const check = body.indexOf("earningsKeyCheck(");
          expect(check, f + "@" + at).toBeGreaterThan(-1);
          expect(check, f + "@" + at).toBeLessThan(body.indexOf("rewards: { totalWei"));
        }
        at = s.indexOf("bridge.node.validatorEarnings(", at + 1);
      }
    }
    expect(calls).toBeGreaterThan(0);
  });

  it("every node_validator_earnings invoke in the bridge forwards the requested key", () => {
    const b = src("src/bridge/tauri/index.ts");
    const sites = b.split('"node_validator_earnings"').length - 1;
    expect(sites).toBeGreaterThan(0);
    let at = b.indexOf('"node_validator_earnings"');
    while (at !== -1) {
      const call = b.slice(at, b.indexOf(")", at) + 1);
      expect(call, "bridge@" + at).toMatch(/pubkey/);
      at = b.indexOf('"node_validator_earnings"', at + 1);
    }
  });

  it("refreshEarnings passes the bond's key to the read", () => {
    const s = src("src/shell/store.ts");
    const start = s.indexOf("async refreshEarnings(");
    const body = s.slice(start, s.indexOf("\n  }\n", start));
    expect(body).toMatch(/validatorEarnings\(\s*key/);
  });

  it("only refreshEarnings writes reward figures into state", () => {
    const s = src("src/shell/store.ts");
    const writes = s.split("rewards: { totalWei").length - 1;
    expect(writes).toBe(1);
    const at = s.indexOf("rewards: { totalWei");
    expect(s.lastIndexOf("async refreshEarnings(", at)).toBeGreaterThan(s.lastIndexOf("\n  async ", at) - 1);
  });
});
