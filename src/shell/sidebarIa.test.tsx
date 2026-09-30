// HUP-S0.8 (D-34, owner-approved 2026-09-30) — the consolidated sidebar. Supersedes the CX-S7.1
// "You / Your Groups" IA test: the owner found the sidebar too long (18 items, 6 overlapping social
// entries). Sub-surfaces stay routable (deep links keep working) but live as TABS under a parent.
import { describe, it, expect } from "vitest";
import { NAVI, SECTIONS, NESTED, HUBS, HIDDEN, ROUTES } from "./Sidebar";
import { REGISTER } from "../App";

const naviIds = NAVI.map(([id]) => id);
const topLevel = SECTIONS.flatMap((s) => s.ids);

describe("HUP-S0.8 consolidated sidebar (D-34)", () => {
  it("leads with Hermes (the home: chat + widgets)", () => {
    expect(SECTIONS[0].ids).toEqual(["dashboard"]);
    expect(NAVI.find(([id]) => id === "dashboard")?.[1]).toBe("Hermes");
  });

  it("has at most 11 top-level items (was 18)", () => {
    expect(topLevel.length).toBeLessThanOrEqual(11);
  });

  it("places every surface exactly once: top-level, nested under a top-level parent, or hidden", () => {
    for (const id of naviIds) {
      const places = [topLevel.includes(id), id in NESTED, HIDDEN.includes(id)].filter(Boolean).length;
      expect(places, `surface ${id}`).toBe(1);
    }
    for (const [child, parent] of Object.entries(NESTED)) {
      expect(topLevel, `${child}'s parent ${parent}`).toContain(parent);
    }
  });

  it("folds the social section into one Groups item with tabs", () => {
    for (const id of ["people", "cluster", "train", "comms"]) expect(NESTED[id]).toBe("groups");
    expect(HUBS.groups.map((t) => t.label)).toEqual(["Chat", "Members", "Cluster", "Training", "Alerts"]);
    expect(topLevel).not.toContain("people");
    expect(HIDDEN).toContain("community");
  });

  it("merges memory into Files and Connections into Settings", () => {
    expect(NESTED.storage).toBe("files");
    expect(NESTED.connections).toBe("settings");
  });

  it("every hub's first tab is the parent itself and it lists all of its nested surfaces", () => {
    for (const [parent, tabs] of Object.entries(HUBS)) {
      expect(tabs[0].id).toBe(parent);
      const nested = Object.entries(NESTED).filter(([, p]) => p === parent).map(([c]) => c);
      for (const c of nested) expect(tabs.map((t) => t.id)).toContain(c);
    }
  });

  it("every surface (and ALF) is a routable deep link with a theme register", () => {
    for (const id of [...naviIds, "alf"]) {
      expect(ROUTES, `route ${id}`).toContain(id);
      expect(REGISTER[id], `register ${id}`).toBeDefined();
    }
  });
});
