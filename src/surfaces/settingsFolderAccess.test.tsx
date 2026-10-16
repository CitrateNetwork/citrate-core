// HUP-S2.1 — Settings > App carries the Hermes folder-access (Grants) card; no other section does.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings } from "./Settings";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

const stubStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "sub-1", wallet: "0x0", tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

function render(sSec: AppState["sSec"]): string {
  const s = freshState("p1");
  s.sSec = sSec;
  return renderToStaticMarkup(<Settings store={stubStore} s={s} />);
}

describe("Settings folder access", () => {
  it("the App section shows the Hermes folder-access card", () => {
    const html = render("app");
    expect(html).toContain("Hermes · folder access");
    expect(html).toContain('id="hermes-folder-access"');
  });
  it("other sections do not", () => {
    expect(render("account")).not.toContain("Hermes · folder access");
  });
});
