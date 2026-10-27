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
import { Dashboard } from "./Dashboard";
import { Onboarding } from "../onboarding/Onboarding";
import { store, type Store } from "../shell/store";
import { freshState, PERSIST_KEYS, type AppState } from "../shell/state";
import { bridge } from "../bridge";
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
    expect(walletTile(member({ bondedStakeRead: true, bondedStake: 0, bondActivated: false }))).toEqual(["0", "Bond locked · not staked"]);
    expect(walletTile(member({ bondedStakeRead: true, bondedStake: 32000, bondActivated: true }))).toEqual(["32,000", "ValidatorRegistry.stakeOf"]);
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
  it("no source formats bondedStake directly or falls back to '0' for a stake", () => {
    const hits: string[] = [];
    for (const f of files) {
      if (f.endsWith(join("shell", "chainTruth.ts"))) continue;
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (zeroFallback.test(line) || rawFormat.test(line)) hits.push(`${f.slice(root.length + 1)}:${i + 1}: ${line.trim()}`);
        });
    }
    expect(hits).toEqual([]);
  });
  it("the Wallet and Dashboard staked figures go through readStakeSalt", () => {
    for (const f of ["surfaces/Wallet.tsx", "surfaces/Dashboard.tsx"]) {
      expect(readFileSync(join(root, f), "utf8"), f).toMatch(/readStakeSalt\(s\)/);
    }
  });
});
