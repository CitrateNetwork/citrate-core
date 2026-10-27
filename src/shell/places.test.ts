// Five-place navigation (0.5.2): every route the app used before keeps working as a deep link and
// opens its new home. The expected table below is written out by hand from the plan's
// "Today → New home in 0.5.2" table (docs/nav/FIVE_PLACE_ROUTE_MAP.md), not derived from
// LEGACY_ROUTES, so a wrong entry in the router fails here.
import { beforeEach, describe, expect, it } from "vitest";
import {
  ACCOUNT_SETTINGS,
  CANONICAL_ROUTES,
  CONTROLS_DETAIL_SETTINGS,
  CONTROLS_SETTINGS,
  LEGACY_ROUTES,
  PLACES,
  ROUTABLE,
  locate,
  registerFor,
  resolveRoute,
  visibleTabs,
} from "./places";
import { store } from "./store";

// old route id → [new leaf route, place, tab label, sub-tab label | null]
const OLD_ROUTES: Record<string, [string, string, string | null, string | null]> = {
  dashboard: ["hermes/chat", "hermes", "Chat", null],
  agent: ["hermes/work", "hermes", "Work", null],
  journal: ["hermes/journal", "hermes", "Journal", null],
  groups: ["groups/chat", "groups", "Chat", "Groups"],
  comms: ["groups/chat/alerts", "groups", "Chat", "Alerts"],
  people: ["groups/members", "groups", "Members", null],
  cluster: ["groups/cluster", "groups", "Cluster", null],
  train: ["groups/rounds", "groups", "Rounds", null],
  alf: ["groups/alf", "groups", "ALF", null],
  community: ["groups/community", "groups", null, null],
  wallet: ["wallet", "wallet", "Wallet", null],
  node: ["node/status", "node", "Status", null],
  files: ["node/storage/files", "node", "Storage", "Files"],
  storage: ["node/storage/memory", "node", "Storage", "Memory"],
  // K4 (#368): the Files hub tabs it added on main before the five places landed.
  pins: ["node/storage/pins", "node", "Storage", "Pins"],
  backup: ["node/storage/backup", "node", "Storage", "Backup"],
  commissary: ["store/apps", "store", "Apps", null],
  models: ["store/models", "store", "Models", null],
  settings: ["settings", "settings", "Settings", null],
  connections: ["settings/connections", "settings", "Connections", null],
};

// Settings section → [new leaf route, place, tab label, section the new home opens on]
const SETTINGS_SECTIONS: Record<string, [string, string, string, string | null]> = {
  hermes: ["hermes/controls", "hermes", "Controls", "hermes"],
  ai: ["hermes/controls/details", "hermes", "Controls", "ai"],
  escalation: ["hermes/controls/details", "hermes", "Controls", "escalation"],
  mcp: ["hermes/controls/details", "hermes", "Controls", "mcp"],
  web: ["hermes/controls/details", "hermes", "Controls", "web"],
  terminal: ["hermes/controls/details", "hermes", "Controls", "terminal"],
  budgets: ["hermes/controls/details", "hermes", "Controls", "budgets"],
  personas: ["hermes/controls/details", "hermes", "Controls", "personas"],
  node: ["node/advanced", "node", "Advanced", "node"],
  account: ["settings", "settings", "Settings", "account"],
  connections: ["settings/connections", "settings", "Connections", null],
  api: ["settings", "settings", "Settings", "api"],
  keys: ["settings", "settings", "Settings", "keys"],
  billing: ["settings", "settings", "Settings", "billing"],
  privacy: ["settings", "settings", "Settings", "privacy"],
  app: ["settings", "settings", "Settings", "app"],
};

const placeOf = (id: string) => PLACES.find((p) => p.id === id);
const tabLabel = (place: string, tabRoute: string | null) =>
  tabRoute === null ? null : placeOf(place)?.tabs.find((t) => t.route === tabRoute)?.label ?? "(missing)";
const subLabel = (place: string, tabRoute: string | null, sub: string | null) =>
  sub === null ? null : placeOf(place)?.tabs.find((t) => t.route === tabRoute)?.sub?.find((x) => x.route === sub)?.label ?? "(missing)";

describe("five places", () => {
  it("are Hermes, Groups, Wallet, Node, Store, then Settings", () => {
    expect(PLACES.map((p) => p.label)).toEqual(["Hermes", "Groups", "Wallet", "Node", "Store", "Settings"]);
  });

  it("Hermes has Chat, Journal, Work, Learn, Controls", () => {
    expect(placeOf("hermes")?.tabs.map((t) => t.label)).toEqual(["Chat", "Journal", "Work", "Learn", "Controls"]);
  });

  it("Groups has Chat, Rounds, Members, Cluster for everyone, and ALF only for ALF members", () => {
    const g = placeOf("groups");
    expect(g).toBeDefined();
    if (!g) return;
    expect(visibleTabs(g, false).map((t) => t.label)).toEqual(["Chat", "Rounds", "Members", "Cluster"]);
    expect(visibleTabs(g, true).map((t) => t.label)).toEqual(["Chat", "Rounds", "Members", "Cluster", "ALF"]);
  });

  it("Node has Status, Services, Storage, Rewards, Advanced, and Storage holds Files, Memory, Pins", () => {
    const n = placeOf("node");
    expect(n?.tabs.map((t) => t.label)).toEqual(["Status", "Services", "Storage", "Rewards", "Advanced"]);
    expect(n?.tabs.find((t) => t.label === "Storage")?.sub?.map((x) => x.label)).toEqual(["Files", "Memory", "Pins", "Backup"]);
  });

  it("Store holds Apps (the Commissary) and Models", () => {
    expect(placeOf("store")?.tabs.map((t) => t.label)).toEqual(["Apps", "Models"]);
  });

  it("Settings keeps only account-level sections", () => {
    expect([...ACCOUNT_SETTINGS].sort()).toEqual(["account", "api", "app", "billing", "connections", "keys", "privacy"]);
    for (const moved of [...CONTROLS_SETTINGS, "node", "hermes"]) expect(ACCOUNT_SETTINGS as readonly string[]).not.toContain(moved);
  });
});

describe("every old route id opens its new home", () => {
  for (const [old, [leaf, place, tab, sub]] of Object.entries(OLD_ROUTES)) {
    it(`${old} → ${leaf}`, () => {
      expect(resolveRoute(old)).toEqual({ route: leaf });
      expect(resolveRoute("#/" + old)?.route).toBe(leaf);
      const loc = locate(old);
      expect(loc.route).toBe(leaf);
      expect(loc.place).toBe(place);
      expect(tabLabel(place, loc.tab)).toBe(tab);
      expect(subLabel(place, loc.tab, loc.sub)).toBe(sub);
      expect(ROUTABLE).toContain(old);
    });
  }

  it("the router knows no old id beyond the ones above", () => {
    // Pre-0.5.2 ids are flat; `settings/<id>` sections and `node/earnings` are tested separately.
    const legacyIds = Object.keys(LEGACY_ROUTES).filter((k) => !k.includes("/"));
    expect(legacyIds.sort()).toEqual(Object.keys(OLD_ROUTES).sort());
  });
});

describe("every Settings section deep link opens its new home", () => {
  for (const [sec, [leaf, place, tab, opens]] of Object.entries(SETTINGS_SECTIONS)) {
    it(`settings/${sec} → ${leaf}${opens ? " (" + opens + ")" : ""}`, () => {
      const r = resolveRoute("settings/" + sec);
      expect(r?.route).toBe(leaf);
      expect(r?.patch?.sSec ?? null).toBe(opens);
      const loc = locate("settings/" + sec);
      expect(loc.place).toBe(place);
      expect(tabLabel(place, loc.tab)).toBe(tab);
    });
  }

  it("AI providers, Escalation and spend, MCP servers, Web search, Terminal, Budgets and Personas land on Hermes › Controls › Details", () => {
    for (const sec of ["models", "ai", "escalation", "mcp", "web", "terminal", "budgets", "personas"]) expect(resolveRoute("settings/" + sec)?.route).toBe("hermes/controls/details");
    expect(resolveRoute("settings/models")).toEqual({ route: "hermes/controls/details", patch: { sSec: "models" } });
  });

  it("the Controls panel section (settings/hermes) lands on Hermes › Controls", () => {
    expect(CONTROLS_SETTINGS as readonly string[]).toContain("hermes");
    expect(resolveRoute("settings/hermes")).toEqual({ route: "hermes/controls", patch: { sSec: "hermes" } });
    expect(locate("#/settings/hermes").place).toBe("hermes");
  });
});

describe("Hermes › Controls has three depths (design slice 1)", () => {
  it("Switches, Details and Records are sub-tabs of Controls, Switches first", () => {
    const controls = placeOf("hermes")?.tabs.find((t) => t.label === "Controls");
    expect(controls?.route).toBe("hermes/controls");
    expect(controls?.sub?.map((x) => [x.label, x.route])).toEqual([
      ["1 · Switches", "hermes/controls"],
      ["2 · Details", "hermes/controls/details"],
      ["3 · Records", "hermes/controls/records"],
    ]);
  });

  it("each depth is a canonical route on Hermes › Controls", () => {
    for (const [route, sub] of [
      ["hermes/controls", "1 · Switches"],
      ["hermes/controls/details", "2 · Details"],
      ["hermes/controls/records", "3 · Records"],
    ] as const) {
      expect(resolveRoute(route), route).toEqual({ route });
      const loc = locate(route);
      expect(loc.place).toBe("hermes");
      expect(tabLabel("hermes", loc.tab)).toBe("Controls");
      expect(subLabel("hermes", loc.tab, loc.sub)).toBe(sub);
      expect(registerFor(route)).toBe("charter");
    }
  });

  it("the design's depth-two ids open Details on their section", () => {
    const want: Record<string, string> = { models: "models", providers: "ai", escalation: "escalation", mcp: "mcp", web: "web", terminal: "terminal", budgets: "budgets", personas: "personas" };
    for (const [id, sSec] of Object.entries(want)) {
      expect(resolveRoute("hermes/controls/" + id), id).toEqual({ route: "hermes/controls/details", patch: { sSec } });
    }
  });

  it("the Details sections are the moved Settings sections plus Terminal and Personas, none account-level", () => {
    expect([...CONTROLS_DETAIL_SETTINGS]).toEqual(["models", "ai", "escalation", "mcp", "web", "terminal", "budgets", "personas"]);
    for (const sec of CONTROLS_DETAIL_SETTINGS) expect(ACCOUNT_SETTINGS as readonly string[]).not.toContain(sec);
  });
});

describe("Node › Rewards (UI truth contract §1)", () => {
  it("the tab is Rewards at node/rewards, and the earlier node/earnings id redirects to it", () => {
    const node = PLACES.find((p) => p.id === "node");
    expect(node?.tabs.find((t) => t.route === "node/rewards")?.label).toBe("Rewards");
    expect(node?.tabs.some((t) => /earning/i.test(t.label) || t.route === "node/earnings")).toBe(false);
    expect(resolveRoute("node/earnings")).toEqual({ route: "node/rewards" });
    expect(locate("node/earnings").route).toBe("node/rewards");
    expect(ROUTABLE).toContain("node/earnings");
  });
});

describe("canonical routes", () => {
  it("each canonical leaf resolves to itself and has a theme register", () => {
    for (const r of CANONICAL_ROUTES) {
      expect(resolveRoute(r), r).toEqual({ route: r });
      expect(locate(r).route, r).toBe(r);
      expect(["instrument", "charter"], r).toContain(registerFor(r));
    }
  });

  it("a bare place id or a tab with sub-tabs opens the first leaf", () => {
    expect(resolveRoute("hermes")?.route).toBe("hermes/chat");
    expect(resolveRoute("store")?.route).toBe("store/apps");
    expect(resolveRoute("node/storage")?.route).toBe("node/storage/files");
    expect(resolveRoute("groups")?.route).toBe("groups/chat");
  });

  it("an unknown route is not routable and shows Hermes › Chat", () => {
    expect(resolveRoute("bank")).toBeNull();
    expect(resolveRoute("")).toBeNull();
    expect(ROUTABLE).not.toContain("bank");
    expect(locate("bank").route).toBe("hermes/chat");
  });

  it("an old id keeps its old theme register", () => {
    expect(registerFor("dashboard")).toBe("instrument");
    expect(registerFor("journal")).toBe("charter");
    expect(registerFor("cluster")).toBe("instrument");
    expect(registerFor("commissary")).toBe("charter");
  });
});

describe("store.go follows the redirects", () => {
  beforeEach(() => {
    store.setState({ route: "hermes/chat", sSec: "account" });
  });

  for (const [old, [leaf]] of Object.entries(OLD_ROUTES)) {
    it(`go("${old}") lands on ${leaf}`, () => {
      store.go(old);
      expect(store.state.route).toBe(leaf);
      expect(location.hash).toBe("#/" + leaf);
    });
  }

  for (const [sec, [leaf, , , opens]] of Object.entries(SETTINGS_SECTIONS)) {
    it(`go("settings/${sec}") lands on ${leaf}`, () => {
      store.go("settings/" + sec);
      expect(store.state.route).toBe(leaf);
      if (opens) expect(store.state.sSec).toBe(opens);
    });
  }

  it("going to plain Settings keeps the last section the member had open", () => {
    store.setState({ sSec: "keys" });
    store.go("settings");
    expect(store.state.route).toBe("settings");
    expect(store.state.sSec).toBe("keys");
  });
});
