// #348 DGX review — UI truth contract §1 Validators (citrate-chain main, lines 58-59):
//   "Registered · active from epoch N" while validatorInfo(pubkey).actEpoch (= N) > currentEpoch().
//      isActive(pubkey) is already true at registration, so it cannot tell pending from active.
//      Show the real N; never promise "next epoch".
//   "Validator active" only when isActive AND actEpoch <= currentEpoch, with a note that consensus
//      can lag by up to an epoch. "—" when the registry reads are missing.
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
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { renderStatus } from "./node/nodePage.fixture";
import { Onboarding } from "../onboarding/Onboarding";
import { REGISTRY_ABI, isValidatorActive, validatorWord, type ChainReads, type ValidatorActivation } from "../shell/chainTruth";
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

describe("#348 — §1 activation epoch", () => {
  it("reads currentEpoch() by its selector 0x76671808", () => {
    expect(encodeFunctionData({ abi: REGISTRY_ABI, functionName: "currentEpoch" })).toBe("0x76671808");
    expect(encodeFunctionData({ abi: REGISTRY_ABI, functionName: "validatorInfo", args: [KEY] }).slice(0, 10)).toBe("0x00408838");
  });

  it("is 'Registered · active from epoch N' while actEpoch > currentEpoch, even though isActive is true", () => {
    expect(isValidatorActive(pending)).toBe(false);
    expect(validatorWord(32_000, pending, true)).toBe("Registered · active from epoch 501");
  });

  it("shows the real N at the most common pending case, actEpoch = currentEpoch + 1", () => {
    const next: ValidatorActivation = { isActive: true, status: 1, actEpoch: 500, currentEpoch: 499 };
    expect(validatorWord(32_000, next, true)).toBe("Registered · active from epoch 500");
    expect(mapNodeState("running", 100, next)).toBe("synced");
  });

  it("an unread registry is '—' on Node and S6, never active", () => {
    const n = renderStatus(stub, member({ bondedStake: 32_000, validatorActivation: null }));
    expect(n).toContain("— · registry not read yet");
    expect(n).not.toContain(">Validator active<");
    const o = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", node: "synced", bondedStake: 32_000, validatorActivation: null })} />);
    expect(o).toContain("— · registry not read yet");
    expect(mapNodeState("running", 100, null)).toBe("synced");
  });

  it("is 'Validator active' only when isActive and actEpoch <= currentEpoch", () => {
    expect(isValidatorActive(active)).toBe(true);
    expect(validatorWord(32_000, active, true)).toBe("Validator active");
    expect(validatorWord(32_000, { ...active, isActive: false }, true)).toBeNull();
    expect(validatorWord(32_000, null, true)).toBeNull();
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
    } as unknown as ChainReads;
    (store as unknown as { chainReads: ChainReads }).chainReads = reads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    (store as unknown as { validatorPubkey: string | null }).validatorPubkey = null;
    store.setState({ node: "synced", hasGrant: false, bondedStake: 0, validatorActivation: null });
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
    const p = renderStatus(stub, member({ bondedStake: 32_000, validatorActivation: pending }));
    expect(p).toContain("Registered · active from epoch 501");
    expect(p).not.toContain(">Validator active<");
    const a = renderStatus(stub, member({ node: "validating", bondedStake: 32_000, validatorActivation: active }));
    expect(a).toContain("Validator active");
    expect(a).toContain("consensus can lag by up to an epoch");
  });

  it("onboarding S6 shows a pending validator as registered, not as an unstaked bond", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", node: "synced", bondedStake: 32_000, validatorActivation: pending })} />);
    expect(html).toContain("Registered · active from epoch 501");
    expect(html).not.toContain("Bond locked · not staked");
    expect(html).not.toContain("next: activate your validator"); // the S7 offer (Join-8 #354)
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
