// BC-7.1 (Rule 1 / I-3 honesty at ship) — the prototype/demo persona panel is a
// web-dev-only affordance. In a PACKAGED Tauri build (BRIDGE_MODE === "tauri") it
// must render NOTHING: no floating "Prototype" tag, no persona switcher, no KYC-outcome
// or entitlement-state overrides. This regression test locks that guard so a future
// edit can't accidentally ship the demo controls to a real member.
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// Force packaged-app mode for this file.
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { DemoPanel } from "./Chrome";
import { freshState, type AppState } from "./state";
import type { Store } from "./store";

// In tauri mode DemoPanel short-circuits on `showProto` before touching the store.
const noopStore = {} as unknown as Store;

describe("DemoPanel — hidden in the packaged app (BC-7.1)", () => {
  it("renders nothing in tauri mode even with demoOpen=true", () => {
    const s: AppState = { ...freshState("p1"), demoOpen: true };
    const html = renderToStaticMarkup(<DemoPanel store={noopStore} s={s} />);
    // The empty fragment renders to an empty string — no demo affordance at all.
    expect(html).toBe("");
    expect(html).not.toContain("Prototype");
    expect(html).not.toContain("Persona");
    expect(html).not.toContain("Reset prototype");
  });
});
