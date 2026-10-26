// Join-8 — the S7 "Activate validator" screen renders the contract's words for each state.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { S7 } from "./Onboarding";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";
import type { ActivationStatusView } from "../bridge/domains";

const noopStore = { walletIsLinked: () => true, refreshActivation: async () => {}, canForgetActivationTx: () => false } as unknown as Store;
const WEI_32K = (32000n * 10n ** 18n).toString();

function st(over: Partial<ActivationStatusView> = {}): ActivationStatusView {
  return {
    bondAddress: "0x2222222222222222222222222222222222222222",
    bondDeployed: true,
    bondActivated: false,
    principalWei: WEI_32K,
    unlockBlock: 16_200_000,
    unlockTimestamp: 1_822_000_000,
    memberTokenId: "4",
    membershipActive: true,
    validatorPubkey: "0x" + "07".repeat(32),
    pubkeySource: "node",
    nodeKeyReady: true,
    nodePubkey: "0x" + "07".repeat(32),
    bondPubkey: null,
    registryStatus: 0,
    registryBondedWei: null,
    registryEscrowWei: null,
    activationEpoch: 0,
    registryActive: false,
    currentEpoch: 500,
    epochBlocks: 1000,
    headBlock: 500_000,
    blockTimeMs: 2000,
    localBlock: 499_999,
    localSyncing: false,
    tx: null,
    ...over,
  };
}

function view(a: ActivationStatusView | null, over: Partial<AppState> = {}): string {
  const s = freshState("p1");
  s.stage = "s7";
  s.activation = a;
  Object.assign(s, over);
  return renderToStaticMarkup(<S7 store={noopStore} s={s} />);
}

describe("S7 · Activate validator", () => {
  it("explains what it does and offers the ceremony when the gate is met", () => {
    const html = view(st());
    expect(html).toContain("Activate validator");
    expect(html).toContain("Bond locked · not staked");
    expect(html).toContain("Signature Ceremony");
    expect(html).toContain('data-testid="s7-activate"');
    expect(html).toContain("not purchased");
  });

  it("lists each unmet condition with its reason and does not offer activation", () => {
    const html = view(st({ membershipActive: false, localSyncing: true }));
    expect(html).not.toContain('data-testid="s7-activate"');
    expect(html).toContain("Membership lapsed");
    expect(html).toContain("not Synced");
  });

  it("Registered · active from epoch N (isActive already true), with the wait in measured time", () => {
    // Head 500,000 = epoch 500; actEpoch 501 starts at block 501,000: 1,000 blocks at 2 s.
    const html = view(st({ bondActivated: true, pubkeySource: "bond", bondPubkey: "0x" + "07".repeat(32), registryStatus: 1, activationEpoch: 501, registryActive: true }));
    expect(html).toContain("Registered · active from epoch 501");
    expect(html).not.toContain("Validator active");
    expect(html).not.toMatch(/next epoch/i);
    expect(html).toContain("~33 min");
  });

  it("Validator active only from isActive AND actEpoch <= currentEpoch", () => {
    const html = view(st({ bondActivated: true, pubkeySource: "bond", bondPubkey: "0x" + "07".repeat(32), registryStatus: 1, registryActive: true, activationEpoch: 500 }));
    expect(html).toContain("Validator active");
    expect(html).toContain("Producing blocks");
  });

  it("hides Activate when this node has no validator key, and says why", () => {
    const html = view(st({ nodeKeyReady: false, nodePubkey: null, validatorPubkey: null, pubkeySource: null, registryStatus: null }), { miningHold: "Block production is off: 0 other producer(s) are live, at least 2 are needed first." });
    expect(html).not.toContain('data-testid="s7-activate"');
    expect(html).toContain('data-testid="s7-key"');
    expect(html).toContain("at least 2 are needed first");
  });

  it("warns when the bond's registered key is not this node's key", () => {
    const html = view(st({ bondActivated: true, pubkeySource: "bond", bondPubkey: "0x" + "09".repeat(32), registryStatus: 1, registryActive: true, activationEpoch: 500 }));
    expect(html).toContain('data-testid="s7-key-mismatch"');
  });

  it("a sent activation is Pending; a reverted one is an error with its block", () => {
    expect(view(st({ tx: { hash: "0xabc", included: false, blockNumber: null, success: null } }), { s7TxHash: "0xabc" })).toContain("Pending");
    const failed = view(st({ tx: { hash: "0xabc", included: true, blockNumber: 501, success: false } }), { s7TxHash: "0xabc" });
    expect(failed).toContain("reverted");
    expect(failed).toContain("501");
  });

  it("a recorded tx unseen for a while can be forgotten; a recent one cannot", () => {
    const pendingTx = { hash: "0xabc", included: false, blockNumber: null, success: null };
    const store = { walletIsLinked: () => true, refreshActivation: async () => {}, canForgetActivationTx: () => true } as unknown as Store;
    const s = freshState("p1");
    s.stage = "s7";
    s.activation = st({ tx: pendingTx });
    s.s7TxHash = "0xabc";
    expect(renderToStaticMarkup(<S7 store={store} s={s} />)).toContain('data-testid="s7-forget"');
    // Truthful: "not seen yet" (it may simply not be mined), never "the network has no record".
    const html = renderToStaticMarkup(<S7 store={store} s={s} />);
    expect(html).toContain("not seen on the network yet after 10 minutes");
    expect(html).not.toMatch(/no record/i);
    const recent = { ...store, canForgetActivationTx: () => false } as unknown as Store;
    expect(renderToStaticMarkup(<S7 store={recent} s={s} />)).not.toContain('data-testid="s7-forget"');
  });

  it("offers Explore free in every state before activation", () => {
    expect(view(st())).toContain('data-testid="s7-skip"');
    expect(view(st({ membershipActive: false }))).toContain('data-testid="s7-skip"');
    expect(view(null)).toContain('data-testid="s7-skip"');
  });

  it("before the first read shows — rather than a state", () => {
    const html = view(null);
    expect(html).toContain("—");
    expect(html).not.toContain("Bond locked · not staked");
  });

  it("never says earning, projections, or Validating", () => {
    for (const a of [st(), st({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 502 }), st({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 500 }), null]) {
      expect(view(a)).not.toMatch(/\bearn|projected|estimate|per month|Validating|next epoch/i);
    }
  });
});
