// L0-WP08: the route and surface registry (planset v060, lanes/L0_INTEGRATION_SEAMS.md 3.5).
// A lane adds a screen in its own src/shell/routes/l<N>.tsx; PlaceView.tsx and places.ts stay as
// they are. Registration is process-wide, so every test here registers routes of its own.
import { describe, expect, it } from "vitest";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { registerSurfaces, surfaceForRoute, type SurfaceDecl } from "./registry";
import { CANONICAL_ROUTES, LEGACY_ROUTES, PLACES, ROUTABLE, locate, resolveRoute } from "../places";
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
    // The Gherkin scenario: L6 registers marketplace/mcp; the member opens #/marketplace/mcp.
    registerSurfaces([decl("marketplace/mcp", "L6", { tab: { place: "store", label: "MCP", order: 30 } })]);
    expect(ROUTABLE).toContain("marketplace/mcp");
    expect(ROUTES).toContain("marketplace/mcp"); // the list App.tsx's hash handler accepts
    expect(CANONICAL_ROUTES).toContain("marketplace/mcp");
    expect(resolveRoute("#/marketplace/mcp")).toEqual({ route: "marketplace/mcp" });
    expect(locate("marketplace/mcp")).toEqual({ place: "store", tab: "marketplace/mcp", sub: null, route: "marketplace/mcp" });
    // A route with no tab is a hidden leaf of the place its first segment names.
    registerSurfaces([decl("groups/wp08-hidden", "L4")]);
    expect(locate("groups/wp08-hidden")).toEqual({ place: "groups", tab: null, sub: null, route: "groups/wp08-hidden" });
    expect(PLACES.flatMap((p) => p.tabs.map((t) => t.route))).not.toContain("groups/wp08-hidden");
  });

  it("legacy_routes_still_resolve", () => {
    registerSurfaces([decl("node/wp08-legacy", "L7"), decl("settings/wp08-legacy", "L3")]);
    for (const [from, to] of Object.entries(LEGACY_ROUTES)) expect(resolveRoute(from), from).toEqual(to);
    // A lane can never take a built-in route: canonical, legacy, a place prefix or a bare place.
    for (const r of ["wallet", "hermes/chat", "dashboard", "settings/ai", "node/storage", "store"]) {
      expect(() => registerSurfaces([decl(r, "L6")]), r).toThrow(/built-in route/);
      expect(surfaceForRoute(r), r).toBeUndefined();
    }
    expect((surfaceFor("wallet", store, store.state) as ReactElement).type).toBe(Wallet);
  });

  it("a_registered_surface_renders_in_place_view", () => {
    registerSurfaces([decl("node/wp08-render", "L7", { tab: { place: "node", label: "Lane test", order: 25 } })]);
    const s = { ...freshState("p1"), stage: "done" as const, route: "node/wp08-render" };
    const html = renderToStaticMarkup(createElement(PlaceView, { store, s }));
    expect(html).toContain('data-lane-surface="node/wp08-render"');
    expect(placeTabs(html)).toEqual(["Status", "Services", "Lane test", "Storage", "Rewards", "Advanced"]);
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
      ["render", { ...decl("store/wp08-render", "L6"), render: "nope" as never }],
    ];
    const before = ROUTABLE.length;
    for (const [why, d] of bad) {
      // The bad one is second, so a refusal also proves the good first one was not kept.
      expect(() => registerSurfaces([decl(`store/wp08-ok-${ROUTABLE.length}`, "L6"), d]), `${why}: ${d.route}`).toThrow();
    }
    expect(ROUTABLE.length).toBe(before);
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
