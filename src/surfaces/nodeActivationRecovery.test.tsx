// #406 — Node offers the same Retry / Forget as S7 for a recorded activation that reverted or has
// gone unseen past 10 minutes, instead of a fresh Activate button that only says "already sent".
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));

import { Node } from "./Node";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";
import type { ActivationStatusView } from "../bridge/domains";

function mkStore(canForget: boolean): Store {
  return { walletIsLinked: () => true, canForgetActivationTx: () => canForget, refreshActivation: async () => true } as unknown as Store;
}
function synced(tx: ActivationStatusView["tx"], hash: string | null): AppState {
  const s = freshState("p1");
  s.node = "synced";
  s.hasGrant = true;
  s.s7TxHash = hash;
  s.s7TxAt = Date.now() - 11 * 60_000;
  s.activation = { bondActivated: false, bondUnlocked: false, bondDeployed: true, membershipActive: true, tx, recordedTx: null } as unknown as ActivationStatusView;
  return s;
}

describe("Node · recorded activation recovery (#406)", () => {
  it("a reverted recorded activation offers Try again, not a fresh Activate", () => {
    const html = renderToStaticMarkup(<Node store={mkStore(false)} s={synced({ hash: "0xdead", included: true, blockNumber: 501, success: false }, "0xdead")} />);
    expect(html).toContain('data-testid="node-activation-failed"');
    expect(html).toContain("Try again");
    expect(html).not.toContain("Activate validator · bond");
  });

  it("an activation unseen past 10 minutes offers Forget", () => {
    const html = renderToStaticMarkup(<Node store={mkStore(true)} s={synced({ hash: "0xfeed", included: false, blockNumber: null, success: null }, "0xfeed")} />);
    expect(html).toContain('data-testid="node-activation-pending"');
    expect(html).toContain('data-testid="node-activation-forget"');
    expect(html).not.toContain("Activate validator · bond");
  });

  it("a recent pending activation shows Pending with no Forget and no Activate", () => {
    const html = renderToStaticMarkup(<Node store={mkStore(false)} s={synced({ hash: "0xfeed", included: false, blockNumber: null, success: null }, "0xfeed")} />);
    expect(html).toContain('data-testid="node-activation-pending"');
    expect(html).not.toContain('data-testid="node-activation-forget"');
    expect(html).not.toContain("Activate validator · bond");
  });

  it("with nothing recorded, Node still offers Activate", () => {
    const html = renderToStaticMarkup(<Node store={mkStore(false)} s={synced(null, null)} />);
    expect(html).toContain("Activate validator");
    expect(html).not.toContain('data-testid="node-activation-pending"');
  });
});
