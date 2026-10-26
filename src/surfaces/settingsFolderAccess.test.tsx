// HUP-S2.1 — the Hermes folder-access (Grants) card lives in one section. Since 0.5.2 K2 that is
// Terminal & folders (Hermes › Controls › Details), next to the terminal switch; no other section has it.
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
  it("the Terminal & folders section shows the terminal switch and the Hermes folder-access card", () => {
    const html = render("terminal");
    expect(html).toContain("Hermes · folder access");
    expect(html).toContain('id="hermes-folder-access"');
    expect(html).toContain('id="hermes-terminal"');
  });
  it("other sections do not, including App", () => {
    expect(render("account")).not.toContain("Hermes · folder access");
    expect(render("app")).not.toContain("Hermes · folder access");
    expect(render("app")).not.toContain('id="hermes-terminal"');
  });
  it("the Personas & voice section holds the persona picker; App no longer does", () => {
    expect(render("personas")).toContain("Hermes · persona");
    expect(render("app")).not.toContain("Hermes · persona");
  });
});
