// C1-V — UI truth contract v1.1 (citrate-chain#319, f366a7be): §1 activation epoch, §2 registered
// vs eligible validators, §6 checkpoint availability.
//
//   §1 "Registered · active from epoch N" when validatorInfo(pubkey).actEpoch > currentEpoch().
//      isActive(pubkey) is already true at registration, so it cannot tell pending from active.
//      "Validator active" needs isActive AND actEpoch <= currentEpoch, with a note that consensus
//      can lag by up to an epoch. Never promise "next epoch".
//   §2 activeCount() counts registrations still pending activation: it is "Registered
//      validators", never "active". "Eligible validators (live registry)" is activeSet members with
//      actEpoch <= currentEpoch. No consensus-set number: no RPC exposes the epoch snapshot.
//   §6 (Rust side, node_withdrawn_tests.rs): a null, error or missing block is never a mismatch.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { encodeFunctionData } from "viem";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  // activeCount() as the live registry returns it today (5, one of them possibly pending).
  useReadContract: () => ({ data: 6n }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Dashboard } from "./Dashboard";
import { Node } from "./Node";
import { Onboarding } from "../onboarding/Onboarding";
import { REGISTRY_ABI, countEligible, isValidatorActive, validatorWord, type ChainReads, type ValidatorActivation } from "../shell/chainTruth";
import { mapNodeState, store, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import { bridge } from "../bridge";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
const KEY = ("0x" + "11".repeat(32)) as `0x${string}`;

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshPendingWithdrawals: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, node: "synced", peers: 3, syncPct: 100, height: 499_058, hasGrant: true, s5StakeWei: wei(32000), ...over };
}

const pending: ValidatorActivation = { isActive: true, status: 1, actEpoch: 501, currentEpoch: 499 };
const active: ValidatorActivation = { isActive: true, status: 1, actEpoch: 499, currentEpoch: 499 };

afterEach(() => {
  vi.restoreAllMocks();
});

describe("§1 — activation epoch", () => {
  it("reads currentEpoch() by its selector 0x76671808", () => {
    expect(encodeFunctionData({ abi: REGISTRY_ABI, functionName: "currentEpoch" })).toBe("0x76671808");
    expect(encodeFunctionData({ abi: REGISTRY_ABI, functionName: "validatorInfo", args: [KEY] }).slice(0, 10)).toBe("0x00408838");
  });

  it("is 'Registered · active from epoch N' while actEpoch > currentEpoch, even though isActive is true", () => {
    expect(isValidatorActive(pending)).toBe(false);
    expect(validatorWord(32_000, pending, true)).toBe("Registered · active from epoch 501");
  });

  it("is 'Validator active' only when isActive and actEpoch <= currentEpoch", () => {
    expect(isValidatorActive(active)).toBe(true);
    expect(validatorWord(32_000, active, true)).toBe("Validator active");
    expect(validatorWord(32_000, { ...active, isActive: false }, true)).toBeNull();
    expect(validatorWord(32_000, null, true)).toBeNull();
  });

  it("uses the registry status words for exiting and slashed validators", () => {
    expect(validatorWord(32_000, { ...active, status: 2 }, true)).toBe("Exiting");
    expect(validatorWord(32_000, { ...active, status: 3 }, true)).toBe("Slashed");
  });

  it("mapNodeState is 'validating' only for an activated validator", () => {
    expect(mapNodeState("running", 100, active)).toBe("validating");
    expect(mapNodeState("running", 100, pending)).toBe("synced");
    expect(mapNodeState("running", 100, null)).toBe("synced");
    expect(mapNodeState("running", 40, active)).toBe("syncing");
  });

  it("the poll reads validatorInfo + currentEpoch and keeps a pending validator 'synced'", async () => {
    const reads = {
      validatorActivation: vi.fn(async () => pending),
      minStakeWei: async () => 32_000n * WEI,
      blockSeconds: async () => 2,
      producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
      validatorCounts: async () => ({ registered: 6, eligible: 5, pending: 1 }),
    } as unknown as ChainReads;
    (store as unknown as { chainReads: ChainReads }).chainReads = reads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    (store as unknown as { validatorPubkey: string | null }).validatorPubkey = null;
    store.setState({ node: "synced", hasGrant: false, bondedStake: 0, selfStake: 0, validatorActivation: null });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 499_058, syncPct: 100, miningArmed: true });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: KEY });
    const g: GrantStatus = {
      attributedStakeWei: wei(32000),
      attributedPrincipalWei: wei(32000),
      hasSbt: true,
      bondAddress: "0x" + "b".repeat(40),
      bondDeployed: true,
      bondedStakeWei: wei(32000),
      hasValidator: true,
      unlockBlock: 16_000_000,
      isUnlocked: false,
      isKycVerified: false,
    };
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(g);
    await store.refreshNode();
    expect((reads as unknown as { validatorActivation: ReturnType<typeof vi.fn> }).validatorActivation).toHaveBeenCalledWith(KEY);
    expect(store.state.validatorActivation).toEqual(pending);
    expect(store.state.node).toBe("synced");
  });

  it("Node shows the pending word with the real N, and the active word with the lag note", () => {
    const p = renderToStaticMarkup(<Node store={stub} s={member({ bondedStake: 32_000, validatorActivation: pending })} />);
    expect(p).toContain("Registered · active from epoch 501");
    expect(p).not.toContain(">Validator active<");
    const a = renderToStaticMarkup(<Node store={stub} s={member({ node: "validating", bondedStake: 32_000, validatorActivation: active })} />);
    expect(a).toContain("Validator active");
    expect(a).toContain("consensus can lag by up to an epoch");
  });

  it("onboarding S6 shows a pending validator as registered, not as an unstaked bond", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", node: "synced", bondedStake: 32_000, validatorActivation: pending })} />);
    expect(html).toContain("Registered · active from epoch 501");
    expect(html).not.toContain("Bond locked · not staked");
    expect(html).not.toContain("Stake my bond");
    const a = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", node: "validating", bondedStake: 32_000, validatorActivation: active })} />);
    expect(a).toContain("Validator active");
    expect(a).not.toContain("ValidatorRegistry.isActive) ·");
  });

  it("no source promises 'next epoch'", () => {
    const files: string[] = [];
    const walk = (d: string) => {
      for (const n of readdirSync(d)) {
        const p = join(d, n);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.(ts|tsx)$/.test(n) && !/\.test\.tsx?$/.test(n)) files.push(p);
      }
    };
    walk("src");
    const hits = files.filter((f) => /next epoch/i.test(readFileSync(f, "utf8")));
    expect(hits).toEqual([]);
  });
});

describe("§2 — registered vs eligible validators", () => {
  it("counts eligible members of the live active set by actEpoch <= currentEpoch", () => {
    expect(countEligible([499, 400, 501, 0, 499, 500], 499)).toEqual({ registered: 6, eligible: 4, pending: 2 });
    expect(countEligible([], 499)).toEqual({ registered: 0, eligible: 0, pending: 0 });
  });

  it("the Dashboard labels activeCount() 'Registered validators', never active", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={member({ validatorCounts: { registered: 6, eligible: 5, pending: 1 } })} />);
    expect(html).toContain("Registered validators");
    expect(html).not.toMatch(/>Validators</);
    expect(html).not.toContain("active · ");
    expect(html).toContain("5 eligible (live registry)");
  });

  it("shows no consensus-set number", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={member({ validatorCounts: { registered: 6, eligible: 5, pending: 1 } })} />);
    expect(html).toContain("unavailable until a snapshot RPC exists");
  });

  it("an unread eligible count is '—', never the registered count", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={member({ validatorCounts: null })} />);
    expect(html).toContain("eligible: —");
  });
});
