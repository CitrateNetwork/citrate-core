// CX-S7.1 — the grandma-proof IA (gS-ia): assert the sidebar is organized around You + Your Groups,
// and that every navigable surface lives in exactly one section (no orphaned/duplicated surface).
import { describe, it, expect } from "vitest";
import { NAVI, SECTIONS } from "./Sidebar";

describe("Sidebar IA — organized around You + Your Groups (gS-ia)", () => {
  it("leads with You then Your Groups", () => {
    expect(SECTIONS[0].title).toBe("You");
    expect(SECTIONS[1].title).toBe("Your Groups");
  });

  it("puts the Group front and center in Your Groups", () => {
    const yourGroups = SECTIONS.find((s) => s.title === "Your Groups");
    expect(yourGroups?.ids).toContain("groups");
    expect(yourGroups?.ids).toContain("cluster");
    expect(yourGroups?.ids).toContain("train");
  });

  it("covers every NAVI surface exactly once across the sections (no orphan, no dupe)", () => {
    const naviIds = NAVI.map(([id]) => id).sort();
    const sectionIds = SECTIONS.flatMap((s) => s.ids).sort();
    // every section id is a real surface
    for (const id of sectionIds) expect(naviIds).toContain(id);
    // every surface is placed exactly once
    expect(sectionIds).toEqual(naviIds);
    expect(new Set(sectionIds).size).toBe(sectionIds.length);
  });
});
