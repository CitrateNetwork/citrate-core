// UI truth contract rule 2 (unknown is not zero) for the ONE staked figure,
// ValidatorRegistry.stakeOf (contract §2 "Your stake"). `bondedStake` starts at 0 and is persisted,
// so before a stakeOf read returns this session it is a default or a stale value, not a reading.
// The Wallet's Staked tile showed "0" there (fresh install, or the node stopped: the launch-time
// grant read did not fold the stake and the poll only read it with the node running).
// Until a read returns, the figure is "—" with "not read yet", everywhere it is shown.
import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";

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
import { renderStatus } from "./node/nodePage.fixture";
import { createDemoProvider, type AgentContext, type ToolCall } from "../agent/harness";
import { resolveQuery } from "../widgets/catalog";
import { SALT_BALANCE_CAPTION } from "../widgets/gallery";
import { Dashboard } from "./Dashboard";
import { Onboarding } from "../onboarding/Onboarding";
import { store, type Store } from "../shell/store";
import { freshState, PERSIST_KEYS, type AppState } from "../shell/state";
import { bridge } from "../bridge";
import { STAKE_NOT_READ } from "../shell/chainTruth";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshActivation: async () => true,
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  activateValidator: async () => true,
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, node: "off", hasGrant: true, s5StakeWei: wei(32000), bondActivated: false, ...over };
}

/** The Staked tile on the Wallet overview: [value, caption]. */
function walletTile(s: AppState): [string, string] {
  const html = renderToStaticMarkup(<Wallet store={stub} s={{ ...s, wTab: "overview" }} />);
  const m = /<span class="eyebrow">Staked<\/span><span[^>]*>([^<]*)<\/span><span[^>]*>([^<]*)<\/span>/.exec(html);
  expect(m, "Staked tile").not.toBeNull();
  return [m![1], m![2]];
}

/** The figure on the Wallet staking tab ("N SALT staked"). */
function stakingFigure(s: AppState): string {
  const html = renderToStaticMarkup(<Wallet store={stub} s={{ ...s, wTab: "staking" }} />);
  const m = /<span class="tabular"[^>]*>([^<]*)<\/span><span class="eyebrow"[^>]*>SALT staked<\/span>/.exec(html);
  expect(m, "staking figure").not.toBeNull();
  return m![1];
}

/** The Dashboard's Staked vital: [value, caption]. */
function dashTile(s: AppState): [string, string] {
  const html = renderToStaticMarkup(<Dashboard store={stub} s={s} />);
  const i = html.indexOf(">Staked<");
  expect(i, "Staked vital").toBeGreaterThan(-1);
  const rest = html.slice(i + 8);
  const texts = [...rest.matchAll(/>([^<>]+)</g)].map((m) => m[1].trim()).filter(Boolean);
  return [texts[0], texts[1]];
}

const grant = (bondedSalt: number): GrantStatus => ({
  attributedStakeWei: wei(32000),
  attributedPrincipalWei: wei(32000),
  hasSbt: true,
  bondAddress: "0x" + "b".repeat(40),
  bondDeployed: true,
  bondedStakeWei: wei(bondedSalt),
  hasValidator: bondedSalt > 0,
  bondActivated: bondedSalt > 0,
  unlockBlock: 16_000_000,
  isUnlocked: false,
  isKycVerified: false,
});

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ bondedStake: 0, bondedStakeRead: false, bondActivated: null });
});

describe("the staked figure is '—' until a stakeOf read returns (unknown is not zero)", () => {
  it("Wallet overview: a fresh install shows '—' and 'not read yet', never 0", () => {
    const [value, caption] = walletTile(member());
    expect(value).toBe("—");
    expect(caption).toContain("not read yet");
    expect(caption).toContain("stakeOf");
  });

  it("Wallet overview: a persisted stake from an earlier session is not shown as a reading", () => {
    const [value, caption] = walletTile(member({ bondedStake: 32000, bondActivated: true }));
    expect(value).toBe("—");
    expect(caption).toContain("not read yet");
  });

  it("Wallet overview: once read, a real 0 is 0 (with the bond word) and a real stake is shown", () => {
    expect(walletTile(member({ bondedStakeRead: true, bondedStake: 0, bondActivated: false, bondUnlocked: false }))).toEqual(["0", "Bond locked · not staked"]);
    expect(walletTile(member({ bondedStakeRead: true, bondedStake: 32000, bondActivated: true }))).toEqual(["32,000", "ValidatorRegistry.stakeOf"]);
  });

  it("after a genesis switch the unread stake says which read it waits for: not read yet on this network (#505)", () => {
    // A stake persisted from the previous network, with the grant read not yet made on the new one.
    const switched = member({ bondedStake: 32000, bondActivated: true, netUnread: ["grant"] });
    expect(walletTile(switched)).toEqual(["—", "not read yet on this network"]);
    expect(dashTile(switched)).toEqual(["—", "not read yet on this network"]);
    // The same unread stake on the same network keeps the stakeOf caption.
    expect(walletTile(member({ bondedStake: 32000, bondActivated: true }))).toEqual(["—", STAKE_NOT_READ]);
    expect(dashTile(member({ bondedStake: 32000, bondActivated: true }))).toEqual(["—", STAKE_NOT_READ]);
  });

  it("Wallet staking tab: the same gate", () => {
    expect(stakingFigure(member())).toBe("—");
    expect(stakingFigure(member({ bondedStake: 32000 }))).toBe("—");
    expect(stakingFigure(member({ bondedStakeRead: true, bondedStake: 0 }))).toBe("0");
    expect(stakingFigure(member({ bondedStakeRead: true, bondedStake: 32000 }))).toBe("32,000");
  });

  it("Dashboard Staked vital: the same gate", () => {
    const [value, caption] = dashTile(member());
    expect(value).toBe("—");
    expect(caption).toContain("not read yet");
    expect(dashTile(member({ bondedStake: 32000, bondActivated: true }))[0]).toBe("—");
    expect(dashTile(member({ bondedStakeRead: true, bondedStake: 32000, bondActivated: true }))[0]).toBe("32,000");
    expect(dashTile(member({ bondedStakeRead: true, bondedStake: 0, bondActivated: false }))[0]).toBe("0");
  });

  it("Onboarding settled card: a persisted stake is not printed as the registry stake before a read", () => {
    const s = member({ stage: "s6", node: "synced", peers: 3, syncPct: 100, height: 546_000, bondedStake: 32000, bondActivated: true });
    const html = renderToStaticMarkup(<Onboarding store={stub} s={s} />);
    expect(html).not.toMatch(/stake 32,000 SALT/);
  });

  it("the agent's wallet context carries no stake until it is read", () => {
    store.setState({ bondedStake: 32000, bondedStakeRead: false });
    expect(store.snapshot().staked).toBeNull();
    store.setState({ bondedStakeRead: true });
    expect(store.snapshot().staked).toBe(32000);
  });

  it("the read flag is never persisted", () => {
    expect(PERSIST_KEYS).not.toContain("bondedStakeRead");
    expect(freshState("p1").bondedStakeRead).toBe(false);
  });
});

describe("the stake is read with the node stopped, and at launch", () => {
  it("the launch-time grant read folds the stake and marks it read", async () => {
    vi.spyOn(store, "identity").mockReturnValue({ wallet: "0x" + "1".repeat(40) } as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant(32000));
    store.setState({ hasGrant: true, s3: "settled", s5: "settled", bondedStake: 0, bondedStakeRead: false });
    await (store as unknown as { reconcileOnboardingFromChain(): Promise<void> }).reconcileOnboardingFromChain();
    expect(store.state.bondedStake).toBe(32000);
    expect(store.state.bondedStakeRead).toBe(true);
  });

  it("a failed launch-time read leaves the stake unread", async () => {
    vi.spyOn(store, "identity").mockReturnValue({ wallet: "0x" + "1".repeat(40) } as never);
    vi.spyOn(bridge.membership, "grantStatus").mockRejectedValue(new Error("rpc down"));
    store.setState({ hasGrant: true, s3: "settled", s5: "settled", bondedStake: 32000, bondedStakeRead: false });
    await (store as unknown as { reconcileOnboardingFromChain(): Promise<void> }).reconcileOnboardingFromChain();
    expect(store.state.bondedStakeRead).toBe(false);
  });

  it("with the node stopped and activated() already read, the poll still reads an unread stake", async () => {
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    store.setState({ node: "off", nodeIntent: "stopped", hasGrant: true, bondedStake: 0, bondedStakeRead: false, bondActivated: false });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant(0));
    await store.refreshNode();
    expect(gs).toHaveBeenCalled();
    expect(store.state.bondedStakeRead).toBe(true);
    expect(store.state.bondedStake).toBe(0);
  });

  it("a malformed stake in the read is not a reading", async () => {
    vi.spyOn(store, "identity").mockReturnValue({ wallet: "0x" + "1".repeat(40) } as never);
    for (const bondedStakeWei of ["", "0x10", "-1", "1e21"]) {
      vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue({ ...grant(0), bondedStakeWei });
      store.setState({ hasGrant: true, s3: "settled", s5: "settled", bondedStake: 7, bondedStakeRead: false });
      await (store as unknown as { reconcileOnboardingFromChain(): Promise<void> }).reconcileOnboardingFromChain();
      expect(store.state.bondedStakeRead, bondedStakeWei).toBe(false);
    }
  });

  it("a failed poll read keeps the stake unread, never a fabricated 0", async () => {
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    store.setState({ node: "off", nodeIntent: "stopped", hasGrant: true, bondedStake: 32000, bondedStakeRead: false, bondActivated: false });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.membership, "grantStatus").mockRejectedValue(new Error("rpc down"));
    await store.refreshNode();
    expect(store.state.bondedStakeRead).toBe(false);
  });
});

describe("TRIPWIRE: no staked figure is rendered from a default 0 before a read", () => {
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
  // The bug shape: `staked > 0 ? fmtI(staked) : "0"` (a 0 fallback for an unread stake), and any
  // number formatting of the raw `bondedStake` (persisted, defaulted) outside the one gate.
  const zeroFallback = /\bstak\w*\s*>\s*0\s*\?[^:]+:\s*["'`]0["'`]/i;
  const rawFormat = /fmt\w*\(\s*[\w.]*\bbondedStake\b/;
  // A stake value defaulted to 0 (`staked ?? 0`, `stakeRead ?? 0`, `c.staked || 0`).
  const nullishZero = /\bstak\w*\s*(\?\?|\|\|)\s*0\b/i;
  // The raw `bondedStake` handed on as a field value (`validatorBondedSalt: s.bondedStake`) or
  // rendered bare in JSX (`{s.bondedStake}`): either carries an unread default as a reading.
  const rawField = /(:\s*|\{\s*)(s|this\.state|state)\.bondedStake\s*[,}\n]/;
  it("no source formats bondedStake directly, passes it on raw, or defaults a stake to 0", () => {
    const hits: string[] = [];
    for (const f of files) {
      if (f.endsWith(join("shell", "chainTruth.ts"))) continue;
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (zeroFallback.test(line) || rawFormat.test(line) || nullishZero.test(line) || rawField.test(line)) hits.push(`${f.slice(root.length + 1)}:${i + 1}: ${line.trim()}`);
        });
    }
    expect(hits).toEqual([]);
  });
  it("the patterns catch each known shape", () => {
    for (const bad of [
      "const x = fmtI(stakeRead ?? 0);",
      "stakedSalt: c.staked ?? 0,",
      "x = c.staked || 0;",
      "validatorBondedSalt: s.bondedStake,",
      "<span>{s.bondedStake}</span>",
      "staked: this.state.bondedStake }",
      "const s2 = staked > 0 ? fmtI(staked) : \"0\";",
      "fmtI(s.bondedStake)",
    ]) {
      expect([zeroFallback, rawFormat, nullishZero, rawField].some((r) => r.test(bad)), bad).toBe(true);
    }
  });
  it("the Wallet, Dashboard and Node staked figures go through readStakeSalt", () => {
    for (const f of ["surfaces/Wallet.tsx", "surfaces/Dashboard.tsx", "surfaces/Node.tsx"]) {
      expect(readFileSync(join(root, f), "utf8"), f).toMatch(/readStakeSalt\(s\)/);
    }
  });
});

describe("review round 2 (#555): every other place the stake reaches", () => {
  const ctxBase: AgentContext = { height: 546_000, peers: 3, nodeState: "synced", staked: null, liquid: 12.5, claimable: null, rewardsTotal: null, walletAddr: "0xabc0000000000000000000000000000000000001", tier: "pilot" };
  async function ask(ctx: AgentContext, q: string): Promise<string> {
    vi.useFakeTimers();
    let text = "";
    const done = createDemoProvider(() => ctx).send({
      messages: [{ role: "user", content: q }],
      callbacks: { onStatus: () => {}, onToken: (t: string) => (text += t), onToolCall: async (_c: ToolCall) => "ok" },
    });
    await vi.runAllTimersAsync();
    await done;
    vi.useRealTimers();
    return text;
  }

  function nodeStake(s: AppState): string {
    const html = renderStatus(stub, s);
    const m = /data-testid="your-stake"[^>]*>([^<]*)<\/span>/.exec(html);
    expect(m, "Your stake row").not.toBeNull();
    return m![1];
  }

  it("Node 'Your stake': not read yet, a read 0, and a read stake", () => {
    expect(nodeStake(member({ node: "synced", bondedStake: 32000, bondActivated: true }))).toBe("— · not read yet");
    expect(nodeStake(member({ node: "synced", bondedStakeRead: true, bondedStake: 0 }))).toBe("—");
    expect(nodeStake(member({ node: "synced", bondedStakeRead: true, bondedStake: 32000, bondActivated: true }))).toBe("32,000 SALT");
  });

  it("the agent snapshot's membership block carries no unread stake (validatorBondedSalt, locked)", () => {
    store.setState({ hasGrant: true, s5StakeWei: wei(32000), bondedStake: 32000, bondedStakeRead: false });
    const m = (store.snapshot() as unknown as { membership: Record<string, unknown> }).membership;
    expect(m.validatorBondedSalt).toBeNull();
    expect(m.locked).toBeNull();
    expect(String(m.validatorBonded)).toContain("not read yet");
    store.setState({ bondedStake: 0, bondedStakeRead: false });
    const m0 = (store.snapshot() as unknown as { membership: Record<string, unknown> }).membership;
    expect(m0.validatorBondedSalt).toBeNull();
    expect(m0.locked).toBeNull();
    store.setState({ bondedStake: 0, bondedStakeRead: true });
    const read = (store.snapshot() as unknown as { membership: Record<string, unknown> }).membership;
    expect(read.validatorBondedSalt).toBe(0);
    expect(read.locked).toBe(true);
    store.setState({ hasGrant: false, s5StakeWei: null });
  });

  it("the agent snapshot carries the grant from MemberBond.principal(), else the vault's attribution, else null", () => {
    store.setState({ hasGrant: true, s5StakeWei: wei(32000), activation: null });
    expect(store.snapshot().grantSalt).toBe(32000);
    store.setState({ activation: { principalWei: wei(31000) } as never });
    expect(store.snapshot().grantSalt).toBe(31000);
    store.setState({ hasGrant: false, s5StakeWei: null, activation: null });
    expect(store.snapshot().grantSalt).toBeNull();
  });

  it("Hermes 'staking position': the grant from the grant read, the stake only from a stakeOf read", async () => {
    // Locked, not activated: grant 32,000, a real stakeOf read of 0. Never "grant is 0 SALT".
    const locked = await ask({ ...ctxBase, grantSalt: 32000, staked: 0 }, "what's my staking position?");
    expect(locked).toContain("**32,000 SALT**");
    expect(locked).not.toMatch(/grant is \*\*0/);
    expect(locked).toMatch(/not staked until/);
    // Stake unread: the grant is still stated; the stake says not read yet.
    const unread = await ask({ ...ctxBase, grantSalt: 32000, staked: null }, "what's my staking position?");
    expect(unread).toContain("**32,000 SALT**");
    expect(unread).toContain("not read yet");
    expect(unread).not.toMatch(/grant is \*\*— /);
    // Activated: the stake is the stakeOf figure.
    const active = await ask({ ...ctxBase, grantSalt: 32000, staked: 32000 }, "what's my staking position?");
    expect(active).toMatch(/\*\*32,000 SALT\*\* staked in the ValidatorRegistry/);
    // Grant not read: says so, never 0.
    const none = await ask({ ...ctxBase, grantSalt: null, staked: null }, "what's my staking position?");
    expect(none).toMatch(/grant is not read yet/i);
    expect(none).not.toContain("0 SALT");
  });

  it("Hermes 'balance': an unread stake is '— (not read yet)', never 0 SALT", async () => {
    const bal = await ask({ ...ctxBase, staked: null }, "what's my wallet balance?");
    expect(bal).toContain("Staked in the ValidatorRegistry: **— (not read yet)**");
    expect(bal).not.toMatch(/Staked in the ValidatorRegistry: \*\*0/);
    const read = await ask({ ...ctxBase, staked: 0 }, "what's my wallet balance?");
    expect(read).toContain("Staked in the ValidatorRegistry: **0 SALT**");
  });

  it("the wallet.summary widget query passes an unread stake as null, and the SALT widget shows —", () => {
    const sources = (staked: number | null) => ({
      context: () => ({ height: 1, peers: 1, nodeState: "synced", staked, liquid: 1, claimable: null }),
      model: () => ({ label: "m", id: null }),
      daemons: () => ({ allPaused: false, total: 0, running: 0, paused: 0, budgetUsedUp: 0 }),
    });
    const unread = resolveQuery("wallet.summary", sources(null)) as Record<string, unknown>;
    expect(unread.stakedSalt).toBeNull();
    const caption = new Function("d", `return ${SALT_BALANCE_CAPTION};`) as (d: unknown) => string;
    expect(caption(unread)).toMatch(/^staked — · /);
    expect(caption(resolveQuery("wallet.summary", sources(0)))).toMatch(/^staked 0 · /);
  });

  it("a malformed stake in the poll's read is not a reading either", async () => {
    for (const bondedStakeWei of ["", "0x10", "-1", "1e21"]) {
      (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
      store.setState({ node: "off", nodeIntent: "stopped", hasGrant: true, bondedStake: 7, bondedStakeRead: false, bondActivated: false });
      vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false });
      vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
      vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue({ ...grant(0), bondedStakeWei });
      await store.refreshNode();
      expect(store.state.bondedStakeRead, bondedStakeWei).toBe(false);
      expect(store.state.bondedStake, bondedStakeWei).toBe(7);
    }
  });
});
