// 0.6.0 L3-WP00 characterization: the navigation table (places.ts) as it is on citrate-core
// a826e441, pinned before L3-WP09 (four places: Agent, Groups, Node, Marketplace, plus Settings)
// and L3-WP12 (delete Train, People, Comms) change it. A failure here means the route table
// changed: update the expectation only together with that reviewed change, and keep every old
// route resolving (places.ts compatibility rule).
import { describe, it, expect } from "vitest";
import { CANONICAL_ROUTES, HIDDEN_ROUTES, LEGACY_ROUTES, PLACES, ROUTABLE, locate, resolveRoute } from "../../../places";

/** Today's canonical leaves, in table order. The 0.6.0 brief counted 34; the table has 33. */
const CANONICAL_TODAY = [
  "hermes/chat",
  "hermes/journal",
  "hermes/work",
  "hermes/learn",
  "hermes/controls",
  "hermes/controls/details",
  "hermes/controls/records",
  "groups/chat",
  "groups/chat/alerts",
  "groups/rounds",
  "groups/rounds/live",
  "groups/rounds/open",
  "groups/rounds/datasets",
  "groups/rounds/adapters",
  "groups/rounds/payouts",
  "groups/members",
  "groups/cluster",
  "groups/alf",
  "wallet",
  "node/status",
  "node/services",
  "node/storage/files",
  "node/storage/memory",
  "node/storage/pins",
  "node/storage/backup",
  "node/rewards",
  "node/advanced",
  "store/apps",
  "store/models",
  "settings",
  "settings/connections",
  "settings/work",
  "groups/community",
];

describe("L3-WP00 characterization: places and routes", () => {
  it("six_places_in_order", () => {
    expect(PLACES.map((p) => p.id)).toEqual(["hermes", "groups", "wallet", "node", "store", "settings"]);
    expect(PLACES.map((p) => p.label)).toEqual(["Hermes", "Groups", "Wallet", "Node", "Store", "Settings"]);
    // Each place opens on its first tab.
    expect(PLACES.map((p) => resolveRoute(p.id)?.route)).toEqual(["hermes/chat", "groups/chat", "wallet", "node/status", "store/apps", "settings"]);
  });

  it("canonical_route_count", () => {
    expect(CANONICAL_ROUTES.length).toBe(33);
    expect(CANONICAL_ROUTES).toEqual(CANONICAL_TODAY);
    expect(HIDDEN_ROUTES).toEqual({ "groups/community": "groups" });
    // Every canonical leaf opens itself.
    for (const r of CANONICAL_ROUTES) expect(resolveRoute(r), r).toEqual({ route: r });
  });

  it("every_legacy_route_resolves", () => {
    expect(Object.keys(LEGACY_ROUTES).length).toBe(47);
    for (const [from, to] of Object.entries(LEGACY_ROUTES)) {
      // The redirect lands on a canonical leaf, with the Settings section it names.
      expect(CANONICAL_ROUTES, from).toContain(to.route);
      expect(resolveRoute(from), from).toEqual(to);
      expect(resolveRoute("#/" + from), from).toEqual(to);
      expect(locate(from).route, from).toBe(to.route);
    }
    // The three surfaces 0.6.0 deletes still resolve today.
    expect(resolveRoute("train")).toEqual({ route: "groups/rounds" });
    expect(resolveRoute("people")).toEqual({ route: "groups/members" });
    expect(resolveRoute("comms")).toEqual({ route: "groups/chat/alerts" });
    for (const r of ROUTABLE) expect(resolveRoute(r), r).not.toBeNull();
  });

  it("unknown_route_falls_back_to_hermes_chat", () => {
    for (const r of ["", "nope", "hermes/nope", "#/unknown/leaf"]) {
      expect(resolveRoute(r), r).toBeNull();
      expect(locate(r), r).toEqual({ place: "hermes", tab: "hermes/chat", sub: null, route: "hermes/chat" });
    }
  });
});
