// Five-place navigation (0.5.2). The app has five places — Hermes, Groups, Wallet, Node, Store —
// plus Settings for account-level things. Each place has tabs; a tab may have sub-tabs. Every leaf
// is a canonical route ("hermes/work", "node/storage/files", …) that the hash router accepts.
//
// Compatibility rule: nothing is deleted. Every route id the app used before (dashboard, agent,
// files, train, …) and every Settings section that moved keeps working as a deep link and opens
// its new home. `resolveRoute` is the single place that rule lives; `store.go` and the hash
// handler both go through it. The full table is docs/nav/FIVE_PLACE_ROUTE_MAP.md.
//
// This module is pure (no store, no bridge, no React) so the router, the sidebar and the tests
// all read the same table.

export type PlaceId = "hermes" | "groups" | "wallet" | "node" | "store" | "settings";

export interface SubTab {
  label: string;
  route: string;
}

export interface PlaceTab {
  label: string;
  /** The tab's default leaf route (its first sub-tab, when it has sub-tabs). */
  route: string;
  sub?: SubTab[];
  /** Shown only to ALF members (the claim gates it, as the old ALF nav item was). */
  alfOnly?: boolean;
}

export interface Place {
  id: PlaceId;
  label: string;
  /** Two SVG path strings for the 24×24 sidebar glyph. */
  icon: [string, string];
  tabs: PlaceTab[];
}

/** Settings sections that live in Hermes › Controls: the Controls panel (`hermes`) and the five that moved. */
export const CONTROLS_SETTINGS = ["hermes", "ai", "escalation", "mcp", "web", "budgets"] as const;
/** The five Settings sections that moved to Hermes › Controls; the tab lists them under the panel. */
export const MOVED_CONTROLS_SETTINGS = ["ai", "escalation", "mcp", "web", "budgets"] as const;
/** The Settings section that moved to Node › Advanced. */
export const NODE_SETTINGS = ["node"] as const;
/** Account-level Settings sections, the only ones Settings keeps. */
export const ACCOUNT_SETTINGS = ["account", "connections", "api", "keys", "billing", "privacy", "app"] as const;

export const PLACES: Place[] = [
  {
    id: "hermes",
    label: "Hermes",
    icon: ["M3 11 L12 3 L21 11 V20 H14 V14 H10 V20 H3 Z", "M0 0"],
    tabs: [
      { label: "Chat", route: "hermes/chat" },
      { label: "Journal", route: "hermes/journal" },
      { label: "Work", route: "hermes/work" },
      { label: "Learn", route: "hermes/learn" },
      { label: "Controls", route: "hermes/controls" },
    ],
  },
  {
    id: "groups",
    label: "Groups",
    icon: ["M8 11 A3 3 0 1 0 8 5 A3 3 0 0 0 8 11 Z M2 20 A6 6 0 0 1 14 20", "M16 11 A3 3 0 0 0 16 5 M18 20 A6 6 0 0 0 15 15"],
    tabs: [
      {
        label: "Chat",
        route: "groups/chat",
        sub: [
          { label: "Groups", route: "groups/chat" },
          { label: "Alerts", route: "groups/chat/alerts" },
        ],
      },
      { label: "Rounds", route: "groups/rounds" },
      { label: "Members", route: "groups/members" },
      { label: "Cluster", route: "groups/cluster" },
      { label: "ALF", route: "groups/alf", alfOnly: true },
    ],
  },
  {
    id: "wallet",
    label: "Wallet",
    icon: ["M3 7 V17 A2 2 0 0 0 5 19 H19 A2 2 0 0 0 21 17 V9 A2 2 0 0 0 19 7 Z M3 7 A2 2 0 0 1 5 5 H16", "M15.5 13 H17.5"],
    tabs: [{ label: "Wallet", route: "wallet" }],
  },
  {
    id: "node",
    label: "Node",
    icon: ["M4 5 H20 V11 H4 Z M4 13 H20 V19 H4 Z", "M7 8 H7.01 M7 16 H7.01"],
    tabs: [
      { label: "Status", route: "node/status" },
      { label: "Services", route: "node/services" },
      {
        label: "Storage",
        route: "node/storage/files",
        sub: [
          { label: "Files", route: "node/storage/files" },
          { label: "Memory", route: "node/storage/memory" },
          { label: "Pins", route: "node/storage/pins" },
        ],
      },
      { label: "Rewards", route: "node/rewards" },
      { label: "Advanced", route: "node/advanced" },
    ],
  },
  {
    id: "store",
    label: "Store",
    icon: ["M21 8 L12 3 L3 8 V16 L12 21 L21 16 Z", "M3 8 L12 13 L21 8 M12 13 V21"],
    tabs: [
      { label: "Apps", route: "store/apps" },
      { label: "Models", route: "store/models" },
    ],
  },
  {
    id: "settings",
    label: "Settings",
    icon: ["M4 7 H20 M4 12 H20 M4 17 H20", "M9 5 V9 M15 10 V14 M8 15 V19"],
    tabs: [
      { label: "Settings", route: "settings" },
      { label: "Connections", route: "settings/connections" },
    ],
  },
];

/** Routable leaves that are deliberately not in any tab strip (Rule 1: not wired enough to list). */
export const HIDDEN_ROUTES: Record<string, PlaceId> = {
  "groups/community": "groups",
};

export interface Redirect {
  route: string;
  /** State that comes with the redirect (which Settings section the new home opens on). */
  patch?: { sSec?: string };
}

/** Every pre-0.5.2 route id, and every Settings section as `settings/<id>`, mapped to its new home. */
export const LEGACY_ROUTES: Record<string, Redirect> = {
  dashboard: { route: "hermes/chat" },
  agent: { route: "hermes/work" },
  journal: { route: "hermes/journal" },
  groups: { route: "groups/chat" },
  comms: { route: "groups/chat/alerts" },
  people: { route: "groups/members" },
  cluster: { route: "groups/cluster" },
  train: { route: "groups/rounds" },
  community: { route: "groups/community" },
  alf: { route: "groups/alf" },
  wallet: { route: "wallet" },
  node: { route: "node/status" },
  files: { route: "node/storage/files" },
  storage: { route: "node/storage/memory" },
  commissary: { route: "store/apps" },
  models: { route: "store/models" },
  settings: { route: "settings" },
  connections: { route: "settings/connections" },
  // Earlier name of Node › Rewards (UI truth contract §1: the tab is "Rewards").
  "node/earnings": { route: "node/rewards" },
  // Settings sections that moved to Hermes › Controls. `hermes` is the Controls panel itself.
  "settings/hermes": { route: "hermes/controls", patch: { sSec: "hermes" } },
  "settings/ai": { route: "hermes/controls", patch: { sSec: "ai" } },
  "settings/escalation": { route: "hermes/controls", patch: { sSec: "escalation" } },
  "settings/mcp": { route: "hermes/controls", patch: { sSec: "mcp" } },
  "settings/web": { route: "hermes/controls", patch: { sSec: "web" } },
  "settings/budgets": { route: "hermes/controls", patch: { sSec: "budgets" } },
  // Node configuration moved to Node › Advanced.
  "settings/node": { route: "node/advanced", patch: { sSec: "node" } },
  // Account-level sections stay in Settings.
  "settings/account": { route: "settings", patch: { sSec: "account" } },
  "settings/connections": { route: "settings/connections" },
  "settings/api": { route: "settings", patch: { sSec: "api" } },
  "settings/keys": { route: "settings", patch: { sSec: "keys" } },
  "settings/billing": { route: "settings", patch: { sSec: "billing" } },
  "settings/privacy": { route: "settings", patch: { sSec: "privacy" } },
  "settings/app": { route: "settings", patch: { sSec: "app" } },
};

/** Every canonical leaf route: each tab, each sub-tab, and the hidden leaves. */
export const CANONICAL_ROUTES: string[] = (() => {
  const out: string[] = [];
  const add = (r: string) => {
    if (!out.includes(r)) out.push(r);
  };
  for (const p of PLACES) {
    for (const t of p.tabs) {
      add(t.route);
      for (const st of t.sub ?? []) add(st.route);
    }
  }
  for (const r of Object.keys(HIDDEN_ROUTES)) add(r);
  return out;
})();

/** A bare place id ("hermes", "store") or a tab with sub-tabs ("node/storage") opens its first leaf. */
const PREFIX_ROUTES: Record<string, string> = (() => {
  const out: Record<string, string> = {};
  for (const p of PLACES) {
    out[p.id] = p.tabs[0].route;
    for (const t of p.tabs) {
      if (!t.sub) continue;
      const prefix = t.sub[0].route.split("/").slice(0, -1).join("/");
      if (prefix && !(prefix in out)) out[prefix] = t.route;
    }
  }
  return out;
})();

/**
 * Where a route opens. A canonical leaf opens itself; an old route id or a moved Settings section
 * opens its new home; a bare place id opens its first tab. Unknown input returns null (the caller
 * decides; the hash router ignores it, as it ignored unknown ids before).
 */
export function resolveRoute(input: string): Redirect | null {
  const r = (input || "").replace(/^#?\/?/, "").replace(/\/+$/, "");
  if (!r) return null;
  if (CANONICAL_ROUTES.includes(r)) return { route: r };
  if (r in LEGACY_ROUTES) return LEGACY_ROUTES[r];
  if (r in PREFIX_ROUTES) return { route: PREFIX_ROUTES[r] };
  return null;
}

/** Every string the hash router accepts. */
export const ROUTABLE: string[] = [...CANONICAL_ROUTES, ...Object.keys(LEGACY_ROUTES), ...Object.keys(PREFIX_ROUTES)];

export interface Location {
  place: PlaceId;
  /** The active tab's default route (null for a hidden leaf). */
  tab: string | null;
  /** The active sub-tab route, when the tab has sub-tabs. */
  sub: string | null;
  /** The canonical leaf being shown. */
  route: string;
}

/** Locate any route (canonical, old or unknown) in the five places. Unknown falls back to Hermes › Chat. */
export function locate(input: string): Location {
  const leaf = resolveRoute(input)?.route ?? "hermes/chat";
  for (const p of PLACES) {
    for (const t of p.tabs) {
      if (t.route === leaf) return { place: p.id, tab: t.route, sub: t.sub ? leaf : null, route: leaf };
      for (const st of t.sub ?? []) {
        if (st.route === leaf) return { place: p.id, tab: t.route, sub: st.route, route: leaf };
      }
    }
  }
  const hidden = HIDDEN_ROUTES[leaf];
  if (hidden) return { place: hidden, tab: null, sub: null, route: leaf };
  return { place: "hermes", tab: "hermes/chat", sub: null, route: "hermes/chat" };
}

/** The tabs a member sees on a place (ALF only for members). */
export function visibleTabs(place: Place, alfMember: boolean): PlaceTab[] {
  return place.tabs.filter((t) => !t.alfOnly || alfMember);
}

/** The Settings section a place shows: the requested one when it belongs there, else the first. */
export function sectionFor(allowed: readonly string[], requested: string): string {
  return allowed.includes(requested) ? requested : allowed[0];
}

// Instrument = dark live telemetry; Charter = light civic/document (from the design's REG map).
const REGISTER_BY_ROUTE: Record<string, "instrument" | "charter"> = {
  "hermes/chat": "instrument",
  "hermes/journal": "charter",
  "hermes/work": "charter",
  "hermes/learn": "charter",
  "hermes/controls": "charter",
  "groups/chat": "charter",
  "groups/chat/alerts": "charter",
  "groups/rounds": "instrument",
  "groups/members": "charter",
  "groups/cluster": "instrument",
  "groups/alf": "charter",
  "groups/community": "charter",
  wallet: "instrument",
  "node/status": "instrument",
  "node/services": "instrument",
  "node/storage/files": "instrument",
  "node/storage/memory": "instrument",
  "node/storage/pins": "instrument",
  "node/rewards": "instrument",
  "node/advanced": "charter",
  "store/apps": "charter",
  "store/models": "instrument",
  settings: "charter",
  "settings/connections": "charter",
};

export function registerFor(route: string): "instrument" | "charter" {
  return REGISTER_BY_ROUTE[locate(route).route] ?? "charter";
}
