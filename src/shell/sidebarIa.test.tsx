// Five-place navigation (0.5.2) supersedes the HUP-S0.8 sectioned sidebar (D-34). The sidebar is
// Hermes, Groups, Wallet, Node, Store, then Settings. Every surface the old sidebar reached is still
// reachable: as a tab, a sub-tab, or (Community) a routable hidden leaf. The redirect table itself is
// tested in places.test.ts; this file keeps the structural invariants the HUP-S0.8 test held.
import { describe, it, expect } from "vitest";
import { ROUTES, navParent } from "./Sidebar";
import { CANONICAL_ROUTES, HIDDEN_ROUTES, LEGACY_ROUTES, PLACES, locate, registerFor } from "./places";

// Every route id the HUP-S0.8 sidebar knew (its NAVI registry, plus ALF).
const OLD_IDS = [
  "dashboard", "wallet", "node", "storage", "journal", "comms", "commissary", "settings", "models",
  "files", "people", "groups", "cluster", "train", "agent", "connections", "community", "alf",
];

const tabRoutes = PLACES.flatMap((p) => p.tabs.flatMap((t) => [t.route, ...(t.sub ?? []).map((x) => x.route)]));

describe("five-place sidebar", () => {
  it("leads with Hermes (the home: chat + widgets)", () => {
    expect(PLACES[0].id).toBe("hermes");
    expect(PLACES[0].tabs[0].route).toBe("hermes/chat");
    expect(locate("dashboard").place).toBe("hermes");
  });

  it("has five places plus Settings (was 11 top-level items)", () => {
    expect(PLACES.length).toBe(6);
    expect(PLACES[PLACES.length - 1].id).toBe("settings");
  });

  it("places every old surface exactly once: in one tab or sub-tab, or hidden", () => {
    for (const id of OLD_IDS) {
      const leaf = LEGACY_ROUTES[id]?.route;
      expect(leaf, id).toBeDefined();
      if (!leaf) continue;
      const inTabs = tabRoutes.filter((r) => r === leaf).length > 0;
      const hidden = leaf in HIDDEN_ROUTES;
      expect([inTabs, hidden].filter(Boolean).length, id).toBe(1);
    }
  });

  it("no two old surfaces share a home", () => {
    const leaves = OLD_IDS.map((id) => LEGACY_ROUTES[id]?.route);
    expect(new Set(leaves).size).toBe(leaves.length);
  });

  it("every tab with sub-tabs opens on its first sub-tab", () => {
    for (const p of PLACES) for (const t of p.tabs) if (t.sub) expect(t.route, t.label).toBe(t.sub[0].route);
  });

  it("every old id and every canonical route is a routable deep link with a theme register", () => {
    for (const id of [...OLD_IDS, ...CANONICAL_ROUTES]) {
      expect(ROUTES, `route ${id}`).toContain(id);
      expect(["instrument", "charter"], `register ${id}`).toContain(registerFor(id));
    }
  });

  it("an old id highlights the place that now holds it", () => {
    expect(navParent("agent")).toBe("hermes");
    expect(navParent("train")).toBe("groups");
    expect(navParent("files")).toBe("node");
    expect(navParent("models")).toBe("store");
    expect(navParent("connections")).toBe("settings");
  });
});
