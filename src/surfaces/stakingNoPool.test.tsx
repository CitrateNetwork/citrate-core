// Owner decision D-9 (2026-10-26): the LiquidStakingPool is retired at the r1006 reroll and is not
// in the genesis contract set. The Wallet offers no pool self-stake: no add-stake or withdraw form,
// no pending-withdrawal queue, no self-stake figure, and a staked total that never counts pool
// shares. Where the pool content was, the Staking tab says how staking works now: through the
// member's validator bond (UI truth contract §1 Validators, §2 "Your stake").
//
// Each render carries a value under the key a 0.5.1 save used for the pool self-stake (2,500), so
// a surface that still read it would show it.
import { afterEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";
import { join } from "node:path";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Wallet } from "./Wallet";
import { Dashboard } from "./Dashboard";
import { freshState, type AppState } from "../shell/state";
import { store, type Store } from "../shell/store";
import { bridge } from "../bridge";

const RETIRED: { uiCopy: string[]; persistedKeys: string[] } = JSON.parse(
  readFileSync(join(__dirname, "../../fixtures/retired/liquid-staking-pool.json"), "utf8"),
);
const LEGACY_KEY = RETIRED.persistedKeys[0];

const BOND_LINE = "Staking is through your validator bond";

const stub = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "sub-1", wallet: "0x" + "0".repeat(40), tier: "pilot", role: "member", org: null, real: true }),
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  toast: () => {},
} as unknown as Store;

/** A granted member (33,500 SALT attributed, a deliberately non-32,000 value) whose state still
 *  carries a 0.5.1 pool self-stake of 2,500. */
function member(tab: string, over: Partial<AppState> = {}): AppState {
  const s = { ...freshState("p1"), [LEGACY_KEY]: 2500 } as AppState;
  s.wTab = tab;
  s.signedIn = true;
  s.hasGrant = true;
  s.s5StakeWei = (33500n * 10n ** 18n).toString();
  return { ...s, ...over };
}

afterEach(() => vi.restoreAllMocks());

describe("D-9: the Wallet has no pool self-stake", () => {
  it("the Staking tab has no pool form, queue or figure, and says staking is through the bond", () => {
    const html = renderToStaticMarkup(<Wallet store={stub} s={member("staking")} />);
    for (const t of RETIRED.uiCopy) expect({ copy: t, shown: html.includes(t) }).toEqual({ copy: t, shown: false });
    expect(html).not.toContain("2,500");
    expect(html).not.toContain("<input");
    expect(html).toContain(BOND_LINE);
    // The grant row and the staked figure are the attributed grant alone.
    expect(html).toContain("33,500 SALT");
    expect(html).not.toContain("36,000");
  });

  it("a member without a grant sees the same: no pool form, staking is through the bond", () => {
    const html = renderToStaticMarkup(<Wallet store={stub} s={member("staking", { hasGrant: false, s5StakeWei: "" })} />);
    for (const t of RETIRED.uiCopy) expect(html).not.toContain(t);
    expect(html).not.toContain("<input");
    expect(html).toContain(BOND_LINE);
  });

  it("the Overview staked tile counts the grant only, with no self-stake", () => {
    const html = renderToStaticMarkup(<Wallet store={stub} s={member("overview")} />);
    expect(html).toContain("33,500");
    expect(html).not.toContain("36,000");
    expect(html).not.toContain("2,500");
    expect(html).not.toMatch(/\+ [\d,]+ self/);
  });

  it("the Dashboard staked tile counts no pool shares and does not name the pool", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={{ ...member("overview"), node: "synced", peers: 3, syncPct: 100, height: 499_058 }} />);
    for (const t of RETIRED.uiCopy) expect(html).not.toContain(t);
    expect(html).toContain("33,500");
    expect(html).not.toContain("36,000");
  });

  it("a balances reply that still carries a pool figure never reaches app state", async () => {
    store.setState({ liquid: 0 });
    vi.spyOn(bridge.wallet, "balances").mockResolvedValue({ liquid: 4, claimable: 1, address: "0x" + "1".repeat(40), staked: 2500 } as Awaited<ReturnType<typeof bridge.wallet.balances>>);
    await store.refreshWallet();
    const s = store.getSnapshot() as unknown as Record<string, unknown>;
    expect(s.liquid).toBe(4);
    expect(s[LEGACY_KEY]).toBeUndefined();
  });

  it("the store has no pool actions or pending-withdrawal read", () => {
    const methods = Object.getOwnPropertyNames(Object.getPrototypeOf(store));
    expect(methods.filter((m) => /withdrawal/i.test(m) || /^wallet\w*stake$/i.test(m))).toEqual([]);
    expect(Object.keys(bridge.wallet).filter((m) => /withdrawal/i.test(m) || m === "stake")).toEqual([]);
  });
});
