// Join-8 follow-up: the Node surface's Activate button names the bond principal AS READ
// (MemberBond.principal via the activation read), never a hard-coded 32,000 (contract §2).
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));

import { Node } from "./Node";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";
import type { ActivationStatusView } from "../bridge/domains";

const store = { walletIsLinked: () => true } as unknown as Store;

function syncedState(principalWei: string | null): AppState {
  const s = freshState("p1");
  s.node = "synced";
  s.hasGrant = true;
  s.activation = principalWei == null ? null : ({ principalWei } as unknown as ActivationStatusView);
  return s;
}

describe("Node · Activate validator label", () => {
  it("shows the principal as read", () => {
    const html = renderToStaticMarkup(<Node store={store} s={syncedState((10000n * 10n ** 18n).toString())} />);
    expect(html).toContain("Activate validator · bond 10,000 SALT");
    expect(html).not.toContain("32,000");
  });

  it("names no amount before the principal is read (unknown is not 32,000)", () => {
    const html = renderToStaticMarkup(<Node store={store} s={syncedState(null)} />);
    expect(html).toContain("Activate validator");
    expect(html).not.toContain("32,000");
  });
});
