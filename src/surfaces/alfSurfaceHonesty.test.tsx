// ALF-ND Phase A — the ALF workbench surface + its nav gating.
//   - Compute contribution is an HONEST SEAM (Rule 1): "activating soon", never a fabricated
//     round/unit and never a functional-looking "start round" control.
//   - Ownership points to the web portal (web = trophy case, node = workbench).
//   - The surface + the sidebar entry are GATED on s.alfMember (the alf_member claim); a non-member
//     gets an honest prompt, not a stub.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { ALF } from "./ALF";
import { Sidebar } from "../shell/Sidebar";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

const noopStore = {} as unknown as Store;
const sidebarStore = { identity: () => ({ sub: "", name: "Tester", initials: "TE", role: "member" }) } as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.alfMember = true;
  s.node = "validating";
  s.height = 71600;
  s.peers = 7;
  return { ...s, ...over };
}

describe("ALF surface — honest compute seam (Rule 1)", () => {
  it("shows the workbench with an honest 'activating soon' seam and never fakes a round", () => {
    const html = renderToStaticMarkup(<ALF store={noopStore} s={member()} />);
    expect(html).toContain("Your workbench");
    expect(html).toContain("activating soon");
    expect(html).toContain("fake a round"); // "...won't fake a round."
    // No functional-looking contribution control that does nothing.
    expect(html).not.toMatch(/Start round|Run a round|Run round/i);
  });

  it("points ownership at the web portal (node = workbench, web = trophy case)", () => {
    const html = renderToStaticMarkup(<ALF store={noopStore} s={member()} />);
    expect(html).toContain("alf.citrate.ai/dashboard");
  });

  it("shows the real node status, honestly (syncing % / synced height), never a fabricated block", () => {
    const syncing = renderToStaticMarkup(<ALF store={noopStore} s={member({ node: "syncing", syncPct: 42 })} />);
    expect(syncing).toContain("syncing 42%");
  });

  it("a signed-in non-member gets an honest prompt, not the workbench", () => {
    const html = renderToStaticMarkup(<ALF store={noopStore} s={freshState("p1")} />);
    expect(html).toContain("for ALF members");
    expect(html).not.toContain("Your workbench");
  });
});

describe("Sidebar — ALF nav is gated on the alf_member claim", () => {
  it("shows the ALF entry for an ALF member", () => {
    const html = renderToStaticMarkup(<Sidebar store={sidebarStore} s={member()} />);
    expect(html).toContain(">ALF<");
  });

  it("hides the ALF entry for a non-member", () => {
    const html = renderToStaticMarkup(<Sidebar store={sidebarStore} s={freshState("p1")} />);
    expect(html).not.toContain(">ALF<");
  });
});
