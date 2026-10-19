// Join-8 — the S7 "Activate validator" screen renders the contract's words for each state.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { S7 } from "./Onboarding";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";
import type { ActivationStatusView } from "../bridge/domains";

const noopStore = { walletIsLinked: () => true, refreshActivation: async () => {} } as unknown as Store;
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
    registryStatus: 0,
    activationEpoch: 0,
    registryActive: false,
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

  it("Staked · active from next epoch, with the epoch wait in measured time", () => {
    const html = view(st({ bondActivated: true, pubkeySource: "bond", registryStatus: 1, activationEpoch: 501, registryActive: false }));
    expect(html).toContain("Staked · active from next epoch");
    expect(html).not.toContain("Validator active");
    expect(html).toContain("~33 min");
  });

  it("Validator active only from isActive = true", () => {
    const html = view(st({ bondActivated: true, pubkeySource: "bond", registryStatus: 1, registryActive: true }));
    expect(html).toContain("Validator active");
    expect(html).not.toContain("Staked · active from next epoch");
  });

  it("a sent activation is Pending; a reverted one is an error with its block", () => {
    expect(view(st({ tx: { hash: "0xabc", included: false, blockNumber: null, success: null } }), { s7TxHash: "0xabc" })).toContain("Pending");
    const failed = view(st({ tx: { hash: "0xabc", included: true, blockNumber: 501, success: false } }), { s7TxHash: "0xabc" });
    expect(failed).toContain("reverted");
    expect(failed).toContain("501");
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
    for (const a of [st(), st({ bondActivated: true, registryStatus: 1 }), st({ bondActivated: true, registryStatus: 1, registryActive: true }), null]) {
      expect(view(a)).not.toMatch(/\bearn|projected|estimate|per month|Validating/i);
    }
  });
});
