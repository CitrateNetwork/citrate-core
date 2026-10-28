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

import { Node } from "./Node";
import { Onboarding } from "../onboarding/Onboarding";
import type { Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
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
      const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", bondActivated: true, validatorActivation })} />);
      expect(html).not.toContain("next: activate your validator"); // the S7 offer (Join-8 #354)
      expect(html).not.toContain("Bond locked · not staked");
    });

    it(`the Node page (registry ${validatorActivation ? "status None" : "unread"})`, () => {
      const html = renderToStaticMarkup(<Node store={stub} s={member({ bondActivated: true, validatorActivation })} />);
      expect(html).not.toContain("Activate validator");
      expect(html).not.toContain("Bond locked · not staked");
    });
  }

  // Join-8 (#354): activation is offered by its own step (S7), so S6 points there; no button in S6.
  it("a bond that never activated is still offered activation", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", bondActivated: false, bondUnlocked: false, validatorActivation: null })} />);
    expect(html).toContain("next: activate your validator, after the local model below");
  });
});
