// CONNECT-S4 — the People surface's groups & clusters role navigator, display honesty.
// The navigator is derived live (store.refreshPeople → s.myGroups); the surface must badge the real
// role, offer "where I'm admin" only when you manage something, and never fabricate a role for a group
// whose roster hasn't loaded (Rule 1).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { People } from "./People";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";
import type { GroupRoleRow } from "./groupsNavigator";

// People only touches `store` in an effect (refreshPeople — not fired by static render) and in click
// handlers. A bare stub covers the render.
const noopStore = {} as unknown as Store;

function stateWith(myGroups: GroupRoleRow[], over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.peopleState = "ready";
  s.people = [];
  s.myGroups = myGroups;
  return { ...s, ...over };
}
const render = (s: AppState) => renderToStaticMarkup(<People store={noopStore} s={s} />);

describe("People — CONNECT-S4 role navigator honesty", () => {
  it("badges my real role per group and summarizes managed vs total", () => {
    const html = render(
      stateWith([
        { id: "g1", name: "Design", kind: "channel", myRole: "owner", iManage: true },
        { id: "g2", name: "Ops", kind: "forum", myRole: "member", iManage: false },
      ]),
    );
    expect(html).toContain("Design");
    expect(html).toContain("Ops");
    expect(html).toContain(">owner<");
    expect(html).toContain(">member<");
    // summary: "1 you run · 2 total"
    expect(html).toContain("1 you run · 2 total");
  });

  it("offers the 'Where I'm admin' filter only when you manage a group", () => {
    // The apostrophe renders HTML-escaped (I&#x27;m); match on the stable prefix.
    const managed = render(stateWith([{ id: "g1", name: "Design", kind: "channel", myRole: "admin", iManage: true }]));
    expect(managed).toContain("Where I");
    expect(managed).toContain("admin</button>");

    const none = render(stateWith([{ id: "g2", name: "Ops", kind: "forum", myRole: "member", iManage: false }]));
    expect(none).not.toContain("Where I");
  });

  it("shows '—' for a group whose role hasn't loaded — never a fabricated role", () => {
    const html = render(stateWith([{ id: "g1", name: "Pending", kind: "channel", myRole: null, iManage: false }]));
    expect(html).toContain(">—<");
    // NEGATIVE CONTROL — no invented role badge for the unknown seat.
    expect(html).not.toContain(">member<");
    expect(html).not.toContain(">owner<");
  });

  it("honest empty state when you're in no groups", () => {
    const html = render(stateWith([]));
    expect(html.toLowerCase()).toContain("not in any groups yet");
  });

  it("each group offers a one-click jump to the group and its cluster", () => {
    const html = render(stateWith([{ id: "g1", name: "Design", kind: "channel", myRole: "owner", iManage: true }]));
    expect(html).toContain(">Open<");
    expect(html).toContain(">Cluster<");
  });
});
