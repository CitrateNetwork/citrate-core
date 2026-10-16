// HUP-S5.4 — the pop-out kinds are a closed allowlist, mirrored 1:1 by Rust `popout.rs` and the
// `popout` capability's window list. A label that is not on the list is never treated as a pop-out.
import { describe, it, expect } from "vitest";
import { POPOUT_KINDS, isPopoutKind, popoutLabel, popoutKindFromLabel, POPOUT_TITLES } from "./kinds";

describe("HUP-S5.4 pop-out kinds", () => {
  it("is exactly the five planned kinds (D-36)", () => {
    expect([...POPOUT_KINDS]).toEqual(["browser", "contract", "monitor", "diff", "media"]);
  });

  it("labels are popout-<kind>, never the main window's label", () => {
    for (const k of POPOUT_KINDS) {
      expect(popoutLabel(k)).toBe("popout-" + k);
      expect(popoutLabel(k)).not.toBe("main");
      expect(POPOUT_TITLES[k].length).toBeGreaterThan(0);
    }
  });

  it("round-trips a label back to its kind", () => {
    for (const k of POPOUT_KINDS) expect(popoutKindFromLabel(popoutLabel(k))).toBe(k);
  });

  it("anything else is not a pop-out", () => {
    for (const label of ["main", "", "popout-", "popout-Monitor", "popout-monitor ", "popout-shell", "monitor", "xpopout-monitor", "popout-../main"]) {
      expect(popoutKindFromLabel(label)).toBeNull();
    }
    for (const v of ["shell", "MONITOR", "", null, undefined, 3, {}]) expect(isPopoutKind(v)).toBe(false);
  });
});
