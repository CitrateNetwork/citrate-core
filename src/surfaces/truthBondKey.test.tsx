// C1-V5 — UI truth contract v1.1 §1 Validators (chain#326). The validator key is MemberBond.pubkey()
// once the bond has activated (it survives an exit, unlike pubkeyOfStaker); this node's proposer key
// is used only when no bond has activated. The S7 step's read (#354, validator_activation.rs) already
// keys its registry reads this way; this makes the Node / Dashboard / Wallet words (the store's
// `validatorActivation`) and the "Producing blocks" check follow the same key.
//
// Ported from #366 onto main's bond model: the key is `bondPubkey` from the grant read, beside
// `bondActivated`; there is no separate bond-state object.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: 6n }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Node } from "./Node";
import { validatorWord, type ChainReads, type ValidatorActivation } from "../shell/chainTruth";
import { store, type Store } from "../shell/store";
import { bridge } from "../bridge";
import type { GrantStatus } from "../bridge/domains";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
const NODE_KEY = ("0x" + "11".repeat(32)) as `0x${string}`;
const BOND_KEY = ("0x" + "33".repeat(32)) as `0x${string}`;

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  refreshActivation: async () => true,
  linkWallet: () => {},
  activateValidator: async () => true,
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

const none: ValidatorActivation = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 511 };
const active: ValidatorActivation = { isActive: true, status: 1, actEpoch: 480, currentEpoch: 511 };

function grant(over: Partial<GrantStatus>): GrantStatus {
  return {
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
    bondActivated: true,
    bondPubkey: BOND_KEY,
    bondUnlocked: false,
    ...over,
  };
}

function reads(activation: ValidatorActivation = active) {
  const r = {
    validatorActivation: vi.fn(async () => activation),
    minStakeWei: async () => 32_000n * WEI,
    blockSeconds: async () => 2,
    producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
    validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
  };
  (store as unknown as { chainReads: ChainReads }).chainReads = r as unknown as ChainReads;
  return r;
}

async function poll(g: GrantStatus, activation: ValidatorActivation = active) {
  const r = reads(activation);
  (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
  (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
  (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
  (store as unknown as { validatorPubkey: string | null }).validatorPubkey = null;
  store.setState({ node: "synced", stage: "done", hasGrant: false, bondedStake: 0, validatorActivation: null, validatorPubkey: null, bondActivated: null, bondPubkey: null, bondUnlocked: null });
  vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 511_000, syncPct: 100, miningArmed: true } as never);
  vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
  vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: NODE_KEY });
  vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(g);
  await store.refreshNode();
  return r;
}

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ bondActivated: null, bondPubkey: null, bondUnlocked: null, bondedStake: 0, validatorActivation: null, validatorPubkey: null, hasGrant: false });
});

describe("C1-V5 — the registry key is MemberBond.pubkey() once the bond has activated", () => {
  it("reads the registry for the bond's key, even when the node holds another key", async () => {
    const r = await poll(grant({}));
    expect(r.validatorActivation).toHaveBeenCalledWith(BOND_KEY);
    expect(r.validatorActivation).not.toHaveBeenCalledWith(NODE_KEY);
    expect(store.state.bondPubkey).toBe(BOND_KEY);
  });

  it("an exited bond (stake 0, still activated) is read under the bond's key, so Exiting shows", async () => {
    const exiting: ValidatorActivation = { isActive: false, status: 2, actEpoch: 480, currentEpoch: 511 };
    const r = await poll(grant({ bondedStakeWei: "0", hasValidator: false }), exiting);
    expect(r.validatorActivation).toHaveBeenCalledWith(BOND_KEY);
    expect(validatorWord(store.state.bondedStake, store.state.validatorActivation, store.state.hasGrant, store.state.bondActivated, store.state.bondUnlocked)).toBe("Exiting");
  });

  it("falls back to the node's key when no bond has activated", async () => {
    const r = await poll(grant({ bondDeployed: false, bondActivated: null, bondPubkey: null, bondUnlocked: null, hasValidator: false, bondedStakeWei: "0", hasSbt: false, attributedStakeWei: "0", attributedPrincipalWei: "0" }), none);
    expect(r.validatorActivation).toHaveBeenCalledWith(NODE_KEY);
    expect(store.state.validatorPubkey).toBe(NODE_KEY);
  });

  it("an activated bond whose key could not be read is unread: never the node's key", async () => {
    const r = await poll(grant({ bondPubkey: null }));
    expect(r.validatorActivation).not.toHaveBeenCalled();
    expect(store.state.validatorActivation).toBeNull();
    expect(store.state.validatorPubkey).not.toBe(NODE_KEY);
  });

  it("a bonded validator that is producing under the bond's key shows 'Producing blocks'", async () => {
    await poll(grant({}));
    expect(store.state.validatorPubkey).toBe(BOND_KEY);
    const html = renderToStaticMarkup(<Node store={stub} s={{ ...store.state, producers: { head: 1, window: 100, proposers: [BOND_KEY] }, producersError: null }} />);
    expect(html).toContain("Producing blocks");
  });

  it("a node key that produces does not make the member's bond key 'Producing blocks'", async () => {
    await poll(grant({}));
    const html = renderToStaticMarkup(<Node store={stub} s={{ ...store.state, producers: { head: 1, window: 100, proposers: [NODE_KEY] }, producersError: null }} />);
    expect(html).not.toContain("Producing blocks");
  });

  it("a failed MemberBond.pubkey() read is retried on the next due poll, node stopped or not", async () => {
    const r = await poll(grant({ bondPubkey: null }));
    expect(r.validatorActivation).not.toHaveBeenCalled();
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "stopped", peers: 0, height: 0, syncPct: 0, miningArmed: false } as never);
    const gs = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({}));
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    await store.refreshNode();
    expect(gs).toHaveBeenCalledTimes(1);
    expect(store.state.bondPubkey).toBe(BOND_KEY);
  });
});
