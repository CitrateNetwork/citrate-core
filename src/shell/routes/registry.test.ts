// L0-WP08: the route and surface registry (planset v060, lanes/L0_INTEGRATION_SEAMS.md 3.5).
// A lane adds a screen in its own src/shell/routes/l<N>.tsx; PlaceView.tsx and places.ts stay as
// they are. Registration is process-wide, so every test here registers routes of its own.
import { describe, expect, it } from "vitest";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { RESERVED_ROUTES, registerSurfaces, surfaceForRoute, type SurfaceDecl } from "./registry";
import { CANONICAL_ROUTES, LEGACY_ROUTES, PLACES, ROUTABLE, locate, registerFor, resolveRoute } from "../places";
import { PlaceView, surfaceFor } from "../PlaceView";
import { ROUTES } from "../Sidebar";
import { store } from "../store";
import { freshState } from "../state";
import { Wallet } from "../../surfaces";

const SRC = join(__dirname, "..", "..");

function decl(route: string, lane: string, extra: Partial<SurfaceDecl> = {}): SurfaceDecl {
  return { route, lane: lane as SurfaceDecl["lane"], render: () => createElement("div", { "data-lane-surface": route }), ...extra };
}

/** The labels of PlaceView's place tab strip, in order. */
function placeTabs(markup: string): string[] {
  const m = markup.match(/<div[^>]*data-testid="place-tabs"[^>]*>(.*?)<\/div>/);
  return m ? [...m[1].matchAll(/<button[^>]*>([^<]*)<\/button>/g)].map((x) => x[1]) : [];
}

describe("L0-WP08 route registry", () => {
  it("duplicate_route_throws_with_both_lanes", () => {
    registerSurfaces([decl("store/wp08-dup", "L6")]);
    expect(() => registerSurfaces([decl("store/wp08-dup", "L1")])).toThrow(/registered by L6; L1 cannot/);
    expect(surfaceForRoute("store/wp08-dup")?.lane).toBe("L6");
    // Inside one batch too, and the batch registers nothing.
    expect(() => registerSurfaces([decl("store/wp08-a", "L4"), decl("store/wp08-a", "L5")])).toThrow(/registered by L4; L5 cannot/);
    expect(surfaceForRoute("store/wp08-a")).toBeUndefined();
    expect(ROUTABLE).not.toContain("store/wp08-a");
  });

  it("registered_route_is_routable", () => {
    // The Gherkin scenario's shape: a lane registers marketplace/<x>; the member opens #/marketplace/<x>.
    // Every route here carries wp08, so it never collides with one a real lane file registers
    // (this suite loads every routes/l<N>.tsx through PlaceView; bootstrap.test.tsx has the Gherkin's).
    registerSurfaces([decl("marketplace/wp08-mcp", "L6", { tab: { place: "store", label: "WP08 MCP", order: 30 } })]);
    expect(ROUTABLE).toContain("marketplace/wp08-mcp");
    expect(ROUTES).toContain("marketplace/wp08-mcp"); // the list App.tsx's hash handler accepts
    expect(CANONICAL_ROUTES).toContain("marketplace/wp08-mcp");
    expect(resolveRoute("#/marketplace/wp08-mcp")).toEqual({ route: "marketplace/wp08-mcp" });
    expect(locate("marketplace/wp08-mcp")).toEqual({ place: "store", tab: "marketplace/wp08-mcp", sub: null, route: "marketplace/wp08-mcp" });
    // A route with no tab is a hidden leaf of the place its first segment names.
    registerSurfaces([decl("groups/wp08-hidden", "L4")]);
    expect(locate("groups/wp08-hidden")).toEqual({ place: "groups", tab: null, sub: null, route: "groups/wp08-hidden" });
    expect(PLACES.flatMap((p) => p.tabs.map((t) => t.route))).not.toContain("groups/wp08-hidden");
  });

  it("legacy_routes_still_resolve", () => {
    registerSurfaces([decl("node/wp08-legacy", "L7"), decl("settings/wp08-legacy", "L3")]);
    for (const [from, to] of Object.entries(LEGACY_ROUTES)) expect(resolveRoute(from), from).toEqual(to);
    // A lane can never take a built-in route: canonical, legacy, a place prefix or a bare place.
    for (const r of ["wallet", "hermes/chat", "dashboard", "settings/ai", "node/storage", "store", "onboarding"]) {
      const tab = { place: "store" as const, label: "X", order: 30 };
      expect(() => registerSurfaces([decl(r, "L6", r.includes("/") ? {} : { tab })]), r).toThrow(/built-in route/);
      expect(surfaceForRoute(r), r).toBeUndefined();
    }
    expect((surfaceFor("wallet", store, store.state) as ReactElement).type).toBe(Wallet);
  });

  it("a_registered_surface_renders_in_place_view", () => {
    registerSurfaces([decl("node/wp08-render", "L7", { tab: { place: "node", label: "WP08 lane test", order: 25 } })]);
    const s = { ...freshState("p1"), stage: "done" as const, route: "node/wp08-render" };
    const html = renderToStaticMarkup(createElement(PlaceView, { store, s }));
    expect(html).toContain('data-lane-surface="node/wp08-render"');
    // Order 25 sits between the second (20) and third (30) built-in tabs; a tab a real lane file
    // registers may sit anywhere, so only the built-in tabs and this one are compared.
    const mine = ["Status", "Services", "WP08 lane test", "Storage", "Rewards", "Advanced"];
    expect(placeTabs(html).filter((l) => mine.includes(l))).toEqual(mine);
  });

  it("refuses_a_malformed_declaration_and_registers_nothing", () => {
    const bad: [string, SurfaceDecl][] = [
      ["lane", decl("store/wp08-bad1", "L10")],
      ["lane", decl("store/wp08-bad2", "l6")],
      ["route", decl("Store/Wp08", "L6")],
      ["route", decl("#/store/wp08", "L6")],
      ["route", decl("store/wp08/", "L6")],
      ["route", decl("store/../wallet", "L6")],
      ["route", decl("", "L6")],
      ["place", decl("nowhere/wp08", "L6")],
      ["place", decl("nowhere/wp08b", "L6", { tab: { place: "nowhere" as never, label: "X", order: 30 } })],
      ["place", decl("node/wp08-wrong", "L6", { tab: { place: "store", label: "X", order: 30 } })],
      ["tab", decl("store/wp08-first", "L6", { tab: { place: "store", label: "X", order: 5 } })],
      ["tab", decl("store/wp08-nan", "L6", { tab: { place: "store", label: "X", order: Number.NaN } })],
      ["tab", decl("store/wp08-label", "L6", { tab: { place: "store", label: " ", order: 30 } })],
      ["tab", decl("store/wp08-same", "L6", { tab: { place: "store", label: " apps ", order: 30 } })],
      ["render", { ...decl("store/wp08-render", "L6"), render: "nope" as never }],
    ];
    const before = ROUTABLE.length;
    for (const [why, d] of bad) {
      // The bad one is second, so a refusal also proves the good first one was not kept.
      expect(() => registerSurfaces([decl(`store/wp08-ok-${ROUTABLE.length}`, "L6"), d]), `${why}: ${d.route}`).toThrow();
    }
    // Two tabs of one batch with one label in one place.
    const twin = (route: string, label: string, order: number) => decl(route, "L6", { tab: { place: "store", label, order } });
    expect(() => registerSurfaces([twin("store/wp08-t1", "WP08 twin", 30), twin("store/wp08-t2", "wp08 twin", 40)])).toThrow(/already has a tab "WP08 twin"/);
    expect(ROUTABLE.length).toBe(before);
  });

  it("every_named_navigation_target_is_built_in_or_reserved", () => {
    // A route the app navigates to by name that no place resolves could be taken by a lane's
    // registration and capture that navigation. Each one resolves, or is reserved in registry.ts.
    // Names come from: go("<id>") in any quote style (store.go, this.go, a go prop), a *_ROUTE
    // constant, a `route: "<id>"` field (coach ctas, alerts, controls links) outside the bridge,
    // and the app_navigate tool's allowlist in store.ts.
    const targets = new Set<string>();
    const lit = /(["'`])([^"'`$]+)\1/.source;
    const patterns = [new RegExp(/\bgo\(\s*/.source + lit, "g"), new RegExp(/\b[A-Z][A-Z0-9_]*_ROUTE\s*=\s*/.source + lit, "g")];
    const field = new RegExp(/\broute:\s*/.source + lit, "g");
    for (const f of readdirSync(SRC, { recursive: true }) as string[]) {
      if (!/\.tsx?$/.test(f) || /\.test\.tsx?$/.test(f)) continue;
      const rel = f.split("\\").join("/");
      const text = readFileSync(join(SRC, f), "utf8");
      for (const re of patterns) for (const m of text.matchAll(re)) targets.add(m[2]);
      // The bridge's `route:` fields are policy routing (ask_first, …), not navigation; places.ts is the table itself.
      if (!rel.startsWith("bridge/") && rel !== "shell/places.ts") for (const m of text.matchAll(field)) targets.add(m[2]);
    }
    const nav = readFileSync(join(SRC, "shell/store.ts"), "utf8").match(/\[([^\]]*)\]\.indexOf\(args\.route\)/);
    expect(nav, "app_navigate allowlist").not.toBeNull();
    for (const m of (nav?.[1] ?? "").matchAll(/["']([^"']+)["']/g)) targets.add(m[1]);
    // The scan reaches every form named above (a single-quoted cta, both route constants, the allowlist).
    for (const t of ["groups", "groups/cluster", "settings/work", "storage", "commissary", "groups/members"]) expect(targets, t).toContain(t);
    expect(targets.size).toBeGreaterThan(20);
    const loose = [...targets].filter((t) => resolveRoute(t) === null && !RESERVED_ROUTES.includes(t)).sort();
    expect(loose).toEqual([]);
    expect(resolveRoute("onboarding")).toBeNull(); // still not a place route, so still reserved
  });

  it("refuses_a_route_under_a_built_in_sub_tab_strip", () => {
    // A registered route cannot join a built-in sub-tab strip; it would show as a top-level tab.
    for (const r of ["groups/rounds/wp08", "node/storage/wp08", "hermes/controls/wp08", "groups/chat/wp08", "node/storage/files/wp08"]) {
      const place = r.split("/")[0] as "groups";
      expect(() => registerSurfaces([decl(r, "L4", { tab: { place, label: "X", order: 30 } })]), r).toThrow(/sub-tab strip/);
      expect(() => registerSurfaces([decl(r, "L4")]), r).toThrow(/sub-tab strip/);
      expect(ROUTABLE, r).not.toContain(r);
    }
    // A sibling that only shares the first letters is not under the strip.
    registerSurfaces([decl("node/storagex-wp08", "L7")]);
    expect(surfaceForRoute("node/storagex-wp08")?.lane).toBe("L7");
  });

  it("a_registered_route_declares_its_register", () => {
    // Instrument (dark telemetry) or charter (light document); charter when the lane says nothing.
    registerSurfaces([decl("node/wp08-dark", "L7", { register: "instrument" }), decl("node/wp08-light", "L7")]);
    expect(registerFor("node/wp08-dark")).toBe("instrument");
    expect(registerFor("node/wp08-light")).toBe("charter");
    expect(registerFor("wallet")).toBe("instrument"); // a built-in keeps its own
    expect(() => registerSurfaces([decl("node/wp08-dim", "L7", { register: "dim" as never })])).toThrow(/register/);
    expect(ROUTABLE).not.toContain("node/wp08-dim");
  });

  it("lane_route_files_declare_only_their_own_lane", () => {
    // SurfaceDecl.lane names the lane that owns the file declaring it: l6.tsx declares only L6.
    for (let n = 0; n <= 9; n++) {
      const text = readFileSync(join(SRC, `shell/routes/l${n}.tsx`), "utf8");
      const lanes = [...text.matchAll(/\blane:\s*["'`]([^"'`]*)["'`]/g)].map((m) => m[1]);
      expect(lanes.filter((l) => l !== `L${n}`), `l${n}.tsx`).toEqual([]);
      // A lane id from a variable or an expression could not be checked here.
      expect(text, `l${n}.tsx`).not.toMatch(/\blane:\s*[^"'`\s]/);
    }
  });

  it("lane_route_files_exist_and_never_import_place_view", () => {
    // routes/index.ts loads every lane file; PlaceView.tsx imports it, so a lane file importing
    // PlaceView.tsx would close an import cycle. Only the registry may extend places.ts.
    const index = readFileSync(join(SRC, "shell/routes/index.ts"), "utf8");
    for (let n = 0; n <= 9; n++) {
      const text = readFileSync(join(SRC, `shell/routes/l${n}.tsx`), "utf8");
      expect(text, `l${n}.tsx`).not.toMatch(/from\s+["']\.\.\/PlaceView["']/);
      expect(index, `index.ts loads l${n}`).toMatch(new RegExp(`import\\s+["']\\./l${n}["'];`));
    }
    const callers = (readdirSync(SRC, { recursive: true }) as string[])
      .filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f))
      .filter((f) => /\bextendPlaces\b/.test(readFileSync(join(SRC, f), "utf8")))
      .map((f) => f.split("\\").join("/"))
      .sort();
    expect(callers).toEqual(["shell/places.ts", "shell/routes/registry.ts"]);
  });
});
