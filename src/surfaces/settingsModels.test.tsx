// D-11 (RC52) — Settings › Models is a Details section of Hermes › Controls, and the older
// "Providers & keys" section (the chat route) says plainly that it is separate from Models.
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings } from "./Settings";
import { freshState } from "../shell/state";
import { CONTROLS_DETAIL_SETTINGS } from "../shell/places";
import type { Store } from "../shell/store";

const stubStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "s", wallet: "", tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

const html = (sSec: string) => {
  const s = freshState("p1");
  s.sSec = sSec;
  return renderToStaticMarkup(<Settings store={stubStore} s={s} sections={CONTROLS_DETAIL_SETTINGS} heading="Details" />);
};

describe("Hermes › Controls › Details › Models", () => {
  it("is listed first and renders the Models section", () => {
    const h = html("models");
    expect(h).toContain(">Models<");
    expect(h.indexOf(">Models<")).toBeLessThan(h.indexOf(">Providers &amp; keys<"));
    expect(h).toContain('data-testid="models-settings"');
  });

  it("Providers & keys says its keys are separate from Models and not counted there", () => {
    const h = html("ai");
    expect(h).toContain('data-testid="legacy-providers-note"');
    expect(h).toContain("not counted against the caps in Models");
    expect(h).not.toContain('data-testid="models-settings"');
  });
});
