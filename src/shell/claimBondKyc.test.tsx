// #362 B-1 (contract v1.2 §1), ported onto main's Node page. The bond's claimRewards() requires
// MemberBond.isKycVerified() (it reverts KycRequired otherwise). So the bond path's Claim is gated on
// that on-chain flag from the grant read, never on the identity provider's kyc_status claim: a member
// verified only in userinfo is not offered Claim, and is told why. The non-bond path keeps the live
// identity check.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { store, type Store } from "./store";
import { freshState, type AppState } from "./state";
import { bridge } from "../bridge";
import { renderRewards } from "../surfaces/node/nodePage.fixture";
import type { ChainReads } from "./chainTruth";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (n: number) => (BigInt(n) * WEI).toString();
const KEY = "0x" + "66".repeat(32);

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  refreshActivation: async () => true,
  claimRewards: async () => {},
  linkWallet: () => {},
  activateValidator: async () => true,
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

// A bonded validator with 20 SALT claimable now, verified in userinfo (s2), on the Earning tab.
const member = (over: Partial<AppState> = {}): AppState => ({
  ...freshState("p1"),
  signedIn: true,
  node: "synced",
  hasGrant: true,
  s2: "verified",
  bondedStake: 32_000,
  bondActivated: true,
  bondPubkey: KEY,
  rewards: { totalWei: wei(30), claimableWei: wei(20) },
  ...over,
});

const grant = (over: Partial<GrantStatus> = {}): GrantStatus => ({
  attributedStakeWei: wei(32000),
  attributedPrincipalWei: wei(32000),
  hasSbt: true,
  bondAddress: "0x" + "b".repeat(40),
  bondDeployed: true,
  bondedStakeWei: wei(32000),
  hasValidator: true,
  bondActivated: true,
  bondPubkey: KEY,
  bondUnlocked: false,
  unlockBlock: 16_000_000,
  isUnlocked: false,
  isKycVerified: false,
  ...over,
});

const claimButton = /<button[^>]*>\s*Claim\s*<\/button>/;

beforeEach(() => {
  vi.spyOn(store, "identity").mockReturnValue({ sub: "sub-1", wallet: "0x" + "1".repeat(40) } as never);
});
afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ bondKycVerified: null, bondActivated: null, bondPubkey: null, bondedStake: 0, hasGrant: false, toast: null });
});

describe("#362 B-1 — Node › Earning: Claim on the bond path needs MemberBond.isKycVerified()", () => {
  it("verified in userinfo but not on chain: no Claim button, and the reason names the on-chain flag", () => {
    const html = renderRewards(stub, member({ bondKycVerified: false }));
    expect(html).not.toMatch(claimButton);
    expect(html).toContain("MemberBond.isKycVerified()");
    expect(html).toMatch(/identity verification on chain/i);
  });

  it("the on-chain flag unread: no Claim button, and it says the flag isn't read yet", () => {
    const html = renderRewards(stub, member({ bondKycVerified: null }));
    expect(html).not.toMatch(claimButton);
    expect(html).toContain("not read yet");
  });

  it("verified on chain: the Claim button is there", () => {
    const html = renderRewards(stub, member({ bondKycVerified: true }));
    expect(html).toMatch(claimButton);
  });
});

describe("#362 B-1 — store.claimRewards gates the bond path on the chain's flag, not userinfo", () => {
  it("a bond whose isKycVerified() reads false builds no claim, even when userinfo says verified", async () => {
    const userinfo = vi.spyOn(store, "assertPayoutKyc").mockResolvedValue(true);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ isKycVerified: false }));
    const claim = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await store.claimRewards();
    expect(claim).not.toHaveBeenCalled();
    expect(String(store.getSnapshot().toast)).toContain("MemberBond.isKycVerified()");
    expect(userinfo).not.toHaveBeenCalled();
  });

  it("a bond verified on chain claims without the userinfo check", async () => {
    const userinfo = vi.spyOn(store, "assertPayoutKyc").mockResolvedValue(false);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ isKycVerified: true }));
    const claim = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await store.claimRewards();
    expect(claim).toHaveBeenCalledWith({ validator: true, pubkey: KEY });
    expect(userinfo).not.toHaveBeenCalled();
  });

  it("the non-bond path keeps the live identity check", async () => {
    const userinfo = vi.spyOn(store, "assertPayoutKyc").mockResolvedValue(false);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ bondDeployed: false, hasValidator: false, bondActivated: null, bondPubkey: null }));
    const claim = vi.spyOn(bridge.agent, "claim");
    await store.claimRewards();
    expect(userinfo).toHaveBeenCalled();
    expect(claim).not.toHaveBeenCalled();
  });

  it("the claim folds the fresh flag, so the Node page follows what was just read", async () => {
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ isKycVerified: true }));
    vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await store.claimRewards();
    expect(store.state.bondKycVerified).toBe(true);
  });
});

describe("#362 B-1 — the grant read carries the flag, and the poll re-reads it while not true", () => {
  it("an activated bond that isn't verified on chain is re-read on the next due poll, node stopped or not", async () => {
    const reads = {
      validatorActivation: vi.fn(async () => ({ isActive: true, status: 1, actEpoch: 1, currentEpoch: 2 })),
      validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
      minStakeWei: async () => 32_000n * WEI,
      blockSeconds: async () => 2,
      producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
    } as unknown as ChainReads;
    (store as unknown as { chainReads: ChainReads }).chainReads = reads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    store.setState({ node: "off", stage: "done", hasGrant: true, bondedStake: 32_000, bondActivated: true, bondPubkey: KEY, bondUnlocked: false, bondKycVerified: false });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false } as never);
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ isKycVerified: true }));
    await store.refreshNode();
    expect(gs).toHaveBeenCalledTimes(1);
    expect(store.state.bondKycVerified).toBe(true);
  });
});

// Tripwire (STD): no Claim is offered or built from a userinfo claim alone on the bond path.
describe("tripwire — the bond path's Claim follows MemberBond.isKycVerified(), never userinfo alone", () => {
  const src = (f: string) => readFileSync(f, "utf8");

  it("every surface that offers Claim (calls store.claimRewards) gates it with bondClaimBlocked", () => {
    const offenders: string[] = [];
    // K3: every surface file, the Node page's tabs in surfaces/node included, so a new tab can't slip past.
    const surfaces = (readdirSync("src/surfaces", { recursive: true }) as string[]).filter((f) => /\.tsx$/.test(f) && !/\.(test|fixture)\.tsx$/.test(f)).map((f) => "src/surfaces/" + f);
    expect(surfaces).toContain("src/surfaces/node/NodeRewards.tsx");
    for (const f of surfaces) {
      const s = src(f);
      if (/store\.claimRewards\(/.test(s) && !/bondClaimBlocked\(/.test(s)) offenders.push(f);
    }
    expect(offenders).toEqual([]);
  });

  it("claimRewards checks the grant read's isKycVerified on the bond path before building a claim, and calls the userinfo check only off it", () => {
    const s = src("src/shell/store.ts");
    const start = s.indexOf("  async claimRewards(): Promise<void> {");
    const body = s.slice(start, s.indexOf("\n  }\n", start));
    const kyc = body.indexOf("isKycVerified");
    const build = body.indexOf("bridge.agent.claim(");
    expect(kyc).toBeGreaterThan(-1);
    expect(kyc).toBeLessThan(build);
    // The userinfo check is on the non-bond branch only: it never runs before the path is known.
    const userinfo = body.indexOf("assertPayoutKyc(");
    expect(userinfo).toBeGreaterThan(body.indexOf("grantStatus("));
    expect(body.slice(Math.max(0, userinfo - 40), userinfo)).toMatch(/else\s+if\s*\(\s*!\s*\(await\s+this\.$/);
  });
});
