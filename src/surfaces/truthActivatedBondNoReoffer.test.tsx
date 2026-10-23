// #348/#349 Codex P2: after a full exit or a slash, the registry stake reads 0 but the bond has
// already activated (MemberBond.activated() = true; it can't activate again). Such a bond must
// never be offered "Stake my bond" / "Activate validator", whatever the registry read.
import { describe, it, expect, vi } from "vitest";
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

import { Onboarding } from "../onboarding/Onboarding";
import type { Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshPendingWithdrawals: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  refreshProducers: () => {},
  linkWallet: () => {},
  activateValidator: async () => true,
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

const member = (over: Partial<AppState> = {}): AppState => ({
  ...freshState("p1"),
  signedIn: true,
  node: "synced",
  peers: 3,
  syncPct: 100,
  height: 546_000,
  hasGrant: true,
  bondedStake: 0,
  ...over,
});

describe("an already-activated bond with stake 0 is never offered activation again", () => {
  for (const validatorActivation of [null, { isActive: false, status: 0, actEpoch: 0, currentEpoch: 546 }]) {
    it(`onboarding S6 (registry ${validatorActivation ? "status None" : "unread"})`, () => {
      const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", memberBond: { deployed: true, activated: true, pubkey: "0x" + "44".repeat(32), unlocked: false }, validatorActivation })} />);
      expect(html).not.toContain("Stake my bond");
      expect(html).not.toContain("Bond locked · not staked");
    });
  }

  it("a bond that never activated is still offered activation", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", memberBond: { deployed: true, activated: false, pubkey: null, unlocked: false }, validatorActivation: null })} />);
    expect(html).toContain("Stake my bond");
  });
});
