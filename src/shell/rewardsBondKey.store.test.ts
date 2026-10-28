// citrate-core#391 / #416 (port of #394 onto main's bond model). The rewards figures on Node and
// Wallet come from node_validator_earnings = ValidatorRegistry.rewardsOf(key). They belong to the
// member's validator key: MemberBond.pubkey() once the bond has activated (C1-V5; kept after an
// exit), this node's own key only when no bond has activated. So the read names that key, and its
// figures are accepted only when the answer's proposerPubkey is that key. An unread bond, an unread
// bond key, another key or a malformed proposerPubkey fails closed: no figures, "—" with the reason.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const validatorEarnings = vi.fn();
const grantStatus = vi.fn();
const agentClaim = vi.fn();
vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    node: { validatorEarnings: (...a: unknown[]) => validatorEarnings(...a) },
    membership: { grantStatus: () => grantStatus() },
    agent: { claim: (...a: unknown[]) => agentClaim(...a) },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Store, earningsKeyCheck } from "./store";
import { Node } from "../surfaces/Node";
import type { AppState } from "./state";

const BOND_KEY = "0x" + "aa".repeat(32);
const NODE_KEY = "0x" + "bb".repeat(32);
const FIGURES = { totalWei: "30000000000000000000", claimableWei: "20000000000000000000" };
type BondReads = Pick<AppState, "hasGrant" | "bondActivated" | "bondPubkey">;
const activated = (pubkey: string | null): BondReads => ({ hasGrant: true, bondActivated: true, bondPubkey: pubkey });
const noBond: BondReads = { hasGrant: false, bondActivated: null, bondPubkey: null };

async function read(bond: BondReads, proposerPubkey: unknown, bondedStake = 32_000): Promise<Store> {
  const store = new Store();
  store.setState({ ...bond, bondedStake, rewards: null, rewardsError: null });
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
  it("an answer for another key than MemberBond.pubkey() shows no figures, and says why", async () => {
    const store = await read(activated(BOND_KEY), NODE_KEY);
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("not your bond's validator key");
    expect(store.state.rewardsError).toContain(NODE_KEY.slice(0, 10));
  });

  it("an exited validator (stake 0, bond activated) is checked the same way", async () => {
    const store = await read(activated(BOND_KEY), NODE_KEY, 0);
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("not your bond's validator key");
  });

  it("a missing or malformed proposerPubkey fails closed", async () => {
    for (const k of ["", null, "0xzz", 42]) {
      const store = await read(activated(BOND_KEY), k);
      expect(store.state.rewards, String(k)).toBeNull();
      expect(store.state.rewardsError, String(k)).toContain("didn't name a readable key");
    }
  });

  // Review B-1 on #556: contract rule 2, unknown is not zero. BigInt("") is 0n, so before #556 an
  // empty figure showed as 0 SALT. Every figure that isn't a decimal wei string is unknown.
  it("a figure that isn't decimal wei is unknown, never 0 SALT, and says why", async () => {
    for (const bad of ["", 0 as unknown as string, 12 as unknown as string, "0x1a", " 5", "-1", "1e18", null as unknown as string]) {
      for (const which of ["totalWei", "claimableWei"] as const) {
        const store = new Store();
        store.setState({ ...activated(BOND_KEY), bondedStake: 32_000, rewards: { totalWei: "9", claimableWei: "9" }, rewardsError: null });
        validatorEarnings.mockResolvedValue({ ...FIGURES, [which]: bad, proposerPubkey: BOND_KEY });
        await store.refreshEarnings();
        expect(store.state.rewards, `${which}=${JSON.stringify(bad)}`).toBeNull();
        expect(store.state.rewardsError, `${which}=${JSON.stringify(bad)}`).toContain("isn't a number");
        const html = renderToStaticMarkup(createElement(Node, { store, s: { ...store.state, nTab: "earn" } }));
        expect(html, `${which}=${JSON.stringify(bad)}`).not.toMatch(/>0\.00</);
      }
    }
  });

  it("the matching key is accepted, whatever its letter case", async () => {
    const store = await read(activated(BOND_KEY), "0x" + BOND_KEY.slice(2).toUpperCase());
    expect(store.state.rewards).toEqual(FIGURES);
    expect(store.state.rewardsError).toBeNull();
  });

  it("the reason never reads as the no-key state", () => {
    for (const [want, k] of [[BOND_KEY, NODE_KEY], [BOND_KEY, ""], [undefined, 42]] as const) {
      const why = earningsKeyCheck(want, k);
      expect(why, JSON.stringify([want, k])).not.toBeNull();
      expect(String(why)).not.toMatch(/proposer.?key/i);
    }
  });

  it("earningsKeyCheck is the pure rule: null only for the expected key, or any readable key when the node's own is expected", () => {
    expect(earningsKeyCheck(BOND_KEY, BOND_KEY)).toBeNull();
    expect(earningsKeyCheck(undefined, NODE_KEY)).toBeNull();
    expect(earningsKeyCheck(BOND_KEY, NODE_KEY)).not.toBeNull();
    expect(earningsKeyCheck(undefined, "0xzz")).not.toBeNull();
  });
});

describe("#394 — the rewards read names the member's validator key", () => {
  // The registry answers rewardsOf for whichever key is asked; with no key, the node's own key.
  const keyed = () => validatorEarnings.mockImplementation(async (k?: string) => ({ ...FIGURES, proposerPubkey: k ?? NODE_KEY }));

  it("reads rewardsOf(MemberBond.pubkey()) for an activated bond, so a node with another key shows the bond's rewards", async () => {
    keyed();
    const store = new Store();
    store.setState({ ...activated(BOND_KEY), bondedStake: 0, rewards: null, rewardsError: null });
    await store.refreshEarnings();
    expect(validatorEarnings).toHaveBeenCalledWith(BOND_KEY);
    expect(store.state.rewards).toEqual(FIGURES);
    expect(store.state.rewardsError).toBeNull();
  });

  it("an activated bond whose key isn't read yet reads nothing and says why", async () => {
    keyed();
    const store = new Store();
    store.setState({ ...activated(null), bondedStake: 32_000, rewards: null, rewardsError: null });
    await store.refreshEarnings();
    expect(validatorEarnings).not.toHaveBeenCalled();
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain("bond's validator key isn't read yet");
  });

  it("a grant whose bond state isn't read yet reads nothing and says why", async () => {
    keyed();
    const store = new Store();
    store.setState({ hasGrant: true, bondActivated: null, bondPubkey: null, bondedStake: 32_000, rewards: null, rewardsError: null });
    await store.refreshEarnings();
    expect(validatorEarnings).not.toHaveBeenCalled();
    expect(store.state.rewardsError).toContain("bond isn't read yet");
  });

  it("with no bond it reads the node's own key (no key sent)", async () => {
    keyed();
    const store = new Store();
    store.setState({ ...noBond, bondedStake: 32_000, rewards: null, rewardsError: null });
    await store.refreshEarnings();
    expect(validatorEarnings).toHaveBeenCalledTimes(1);
    expect(validatorEarnings.mock.calls[0][0]).toBeUndefined();
    expect(store.state.rewards).toEqual(FIGURES);
  });

  const grant = { bondDeployed: true, bondActivated: true, hasValidator: false, bondPubkey: BOND_KEY, isKycVerified: true };
  const claimStore = () => {
    const store = new Store();
    (store as unknown as { assertPayoutKyc: () => Promise<boolean> }).assertPayoutKyc = async () => true;
    (store as unknown as { openWalletReview: () => void }).openWalletReview = () => {};
    return store;
  };

  it("Claim's pre-check reads rewardsOf for the bond's key, the key the bond claims for", async () => {
    grantStatus.mockResolvedValue(grant);
    await claimStore().claimRewards();
    expect(agentClaim).toHaveBeenCalledWith({ validator: true, pubkey: BOND_KEY });
  });

  it("Claim stops with the reason when the bond claims but its key isn't read", async () => {
    grantStatus.mockResolvedValue({ ...grant, bondPubkey: null });
    const store = claimStore();
    await store.claimRewards();
    expect(agentClaim).not.toHaveBeenCalled();
    expect(String(store.state.toast)).toContain("validator key isn't read yet");
  });
});

describe("#391 — Node › Earning shows a mismatch as unknown, with the reason", () => {
  it("no figures, and the reason naming both keys", async () => {
    const store = await read(activated(BOND_KEY), NODE_KEY);
    const html = renderToStaticMarkup(createElement(Node, { store, s: { ...store.state, nTab: "earn" } }));
    expect(html).toContain("not your bond&#x27;s validator key");
    expect(html).not.toContain("30.00");
    expect(html).not.toContain("20.00");
  });
});

// Tripwire (STD): every non-test read of node_validator_earnings that uses its figures checks the
// key first, the bridge forwards the key, and the figures land in state only through refreshEarnings.
describe("tripwire — every earnings read path checks the key", () => {
  const files = ["src/shell/store.ts", "src/surfaces/Node.tsx", "src/surfaces/Wallet.tsx", "src/surfaces/Dashboard.tsx"];
  const src = (f: string) => readFileSync(join(process.cwd(), f), "utf8");

  it("each bridge.node.validatorEarnings( call either ignores the figures or runs earningsKeyCheck before using them", () => {
    let calls = 0;
    for (const f of files) {
      const s = src(f);
      let at = s.indexOf("bridge.node.validatorEarnings(");
      while (at !== -1) {
        calls++;
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
    let at = b.indexOf('"node_validator_earnings"');
    expect(at).toBeGreaterThan(-1);
    while (at !== -1) {
      const call = b.slice(at, b.indexOf(")", at) + 1);
      expect(call, "bridge@" + at).toMatch(/pubkey/);
      at = b.indexOf('"node_validator_earnings"', at + 1);
    }
  });

  it("refreshEarnings passes the member's key to the read", () => {
    const s = src("src/shell/store.ts");
    const start = s.indexOf("async refreshEarnings(");
    const body = s.slice(start, s.indexOf("\n  }\n", start));
    expect(body).toMatch(/validatorEarnings\(\s*key/);
  });

  it("only refreshEarnings writes reward figures into state", () => {
    const s = src("src/shell/store.ts");
    expect(s.split("rewards: { totalWei").length - 1).toBe(1);
    const at = s.indexOf("rewards: { totalWei");
    expect(s.lastIndexOf("async refreshEarnings(", at)).toBeGreaterThan(s.lastIndexOf("\n  async ", at) - 1);
  });
});
