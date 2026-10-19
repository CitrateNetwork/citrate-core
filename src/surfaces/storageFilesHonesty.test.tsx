// CX-S2.4 — StorageFiles copy honesty (D-22 subsidy framing / Rule 1 / compliance).
//
// The storage surface must never frame storage as the USER earning by storing (D-22), never
// guaranteed, never framed as immediate cash, and it must be honest that the on-chain bond is NOT
// live (S2.2 blocked on the chain CommD fix; IPFSIncentivesV3 dormant, UI truth contract §1). These are
// the red-then-green tripwires: they fail if any forbidden earnings claim renders, and prove the
// honest "forthcoming / pin locally" framing holds. Mirrors the copy-lint patterns in
// scripts/cx-copy-lint.sh so the surface and the CI gate agree.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { StorageFiles } from "./StorageFiles";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

// StorageFiles renders from its slice + `store.toast`; effects don't fire in static markup, so a
// minimal stub is enough.
const stubStore = { toast: () => {} } as unknown as Store;

const html = renderToStaticMarkup(<StorageFiles store={stubStore} s={freshState("p1")} />);

describe("StorageFiles honesty — subsidy-framed, no user-earnings/guaranteed/day-one (D-22)", () => {
  it("never implies the user earns by storing, guarantees, or immediate cash", () => {
    expect(html).not.toMatch(/earn\s+salt\s+by\s+storing/i);
    expect(html).not.toMatch(/guaranteed\s+(salt|rewards?|returns?|income)/i);
    expect(html).not.toMatch(/day.?one\s+(cash|earnings?)/i);
  });

  // K4 (UI truth contract §1): IPFSIncentivesV3 is deployed but dormant and unfunded, so the earlier
  // "staked SALT bond you approve" framing described something that is not live. Pins are local.
  it("says pin bonds are not live yet, in the contract's required words", () => {
    expect(html).toContain("Pin bonds not live yet");
  });

  it("frames pinning as a local pin with no bond and no transaction, never a bond to approve", () => {
    const lower = html.toLowerCase();
    expect(lower).toContain("a local pin, with no bond and no transaction");
    expect(lower).not.toContain("staked salt bond");
    expect(html).not.toContain("Pin · bond");
    expect(lower).not.toMatch(/approve the bond/);
  });
});
