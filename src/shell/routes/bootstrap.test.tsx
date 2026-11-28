// L0-WP08: a lane registration loaded the way the app loads it. PlaceView.tsx imports ./index,
// which imports every l<N>.tsx; here l6.tsx is replaced by a file that registers routes, as L6's
// will, so the whole chain (index, registry, places, PlaceView) runs as it does in the app.
// (1) The SEAM-ROUTES fixtures hold the built-in tables only, so a lane's route never fails them.
// (2) A lane's render may call hooks: moving between a built-in route and a registered one, or
//     between two registered ones, never changes PlaceView's own hook order.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, createElement, isValidElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { BUILTIN_CANONICAL_ROUTES, BUILTIN_ROUTABLE, CANONICAL_ROUTES, ROUTABLE, resolveRoute } from "../places";
import { PlaceView, surfaceFor } from "../PlaceView";
import { registeredRoutes } from "./registry";
import { store } from "../store";
import { freshState } from "../state";
import { SEAMS_BASE, checkSnapshot } from "../../../scripts/seams-snapshot.mjs";

vi.mock("./l6", async () => {
  const { registerSurfaces } = await import("./registry");
  const { createElement: h, useEffect, useRef, useState } = await import("react");
  registerSurfaces([
    // The lane Gherkin: L6 registers marketplace/mcp.
    { route: "marketplace/mcp", lane: "L6", tab: { place: "store", label: "MCP", order: 30 }, render: () => h("div", { "data-lane-surface": "marketplace/mcp" }) },
    // Renders that call hooks directly, with different hook counts.
    {
      route: "store/wp08-hook-a",
      lane: "L6",
      render: () => {
        const [n] = useState(1);
        return h("div", { "data-lane-surface": "store/wp08-hook-a" }, String(n));
      },
    },
    {
      route: "store/wp08-hook-b",
      lane: "L6",
      render: () => {
        const [label] = useState("b");
        const seen = useRef(0);
        useEffect(() => {
          seen.current += 1;
        }, []);
        return h("div", { "data-lane-surface": "store/wp08-hook-b" }, label);
      },
    },
  ]);
  return {};
});

function typeName(t: unknown): string {
  if (typeof t === "string") return t;
  if (typeof t === "function") return t.name || "<anonymous>";
  const o = t as { displayName?: string; type?: unknown; render?: unknown } | null;
  if (o?.displayName) return o.displayName;
  if (o?.type) return `memo(${typeName(o.type)})`;
  if (o?.render) return `forwardRef(${typeName(o.render)})`;
  return String(t);
}

/** As routes.characterization.test.tsx records a surface: its component and its own props. */
function rendered(node: ReactNode): unknown {
  if (!isValidElement(node)) return node === null ? "null" : typeof node;
  const { store: _store, s: _s, ...props } = node.props as Record<string, unknown>;
  return { component: typeName(node.type), props };
}

let mounted: Root | null = null;
afterEach(() => {
  if (mounted) act(() => mounted?.unmount());
  mounted = null;
});

describe("L0-WP08 lane bootstrap", () => {
  it("a_lane_route_loaded_by_the_bootstrap_leaves_the_seam_routes_fixtures_green", () => {
    expect(registeredRoutes()).toEqual(["marketplace/mcp", "store/wp08-hook-a", "store/wp08-hook-b"]);
    expect(ROUTABLE).toContain("marketplace/mcp");
    expect(resolveRoute("#/marketplace/mcp")).toEqual({ route: "marketplace/mcp" });
    // The live tables are the built-in ones plus the registered routes, and nothing else.
    expect([...CANONICAL_ROUTES].sort()).toEqual([...BUILTIN_CANONICAL_ROUTES, ...registeredRoutes()].sort());
    expect([...ROUTABLE].sort()).toEqual([...BUILTIN_ROUTABLE, ...registeredRoutes()].sort());
    // The SEAM-ROUTES computation, as that suite runs it, still matches its fixtures.
    const s = store.state;
    const surfaces: Record<string, unknown> = {};
    for (const r of [...BUILTIN_CANONICAL_ROUTES].sort()) surfaces[r] = rendered(surfaceFor(r, store, s));
    surfaces["<unknown>"] = rendered(surfaceFor("seams/not-a-route", store, s));
    expect(checkSnapshot(`fixtures/seams/routes-surfaces.${SEAMS_BASE}.json`, surfaces)).toEqual([]);
    const resolves: Record<string, unknown> = {};
    for (const r of [...BUILTIN_ROUTABLE].sort()) resolves[r] = resolveRoute(r);
    expect(checkSnapshot(`fixtures/seams/routes-resolve.${SEAMS_BASE}.json`, resolves)).toEqual([]);
  });

  it("navigating_to_a_registered_surface_that_uses_hooks_keeps_the_shell_up", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const errors: unknown[] = [];
    const spy = vi.spyOn(console, "error").mockImplementation((...a: unknown[]) => void errors.push(a));
    const onError = (e: ErrorEvent) => {
      errors.push(e.error);
      e.preventDefault();
    };
    window.addEventListener("error", onError);
    try {
      mounted = createRoot(host);
      const base = { ...freshState("p1"), stage: "done" as const };
      const path: [string, string][] = [
        ["marketplace/mcp", '[data-lane-surface="marketplace/mcp"]'],
        ["store/wp08-hook-a", '[data-lane-surface="store/wp08-hook-a"]'],
        ["store/apps", '[data-surface="store/apps"]'],
        ["store/wp08-hook-b", '[data-lane-surface="store/wp08-hook-b"]'],
        ["store/wp08-hook-a", '[data-lane-surface="store/wp08-hook-a"]'],
        ["store/wp08-hook-b", '[data-lane-surface="store/wp08-hook-b"]'],
        ["marketplace/mcp", '[data-lane-surface="marketplace/mcp"]'],
      ];
      for (const [route, sel] of path) {
        try {
          await act(async () => mounted?.render(createElement(PlaceView, { store, s: { ...base, route } })));
        } catch (e) {
          errors.push(e);
        }
        expect(host.querySelector(sel), `${route} rendered`).not.toBeNull();
      }
      expect(errors.filter((e) => /hook/i.test(String(e)))).toEqual([]);
    } finally {
      window.removeEventListener("error", onError);
      spy.mockRestore();
      host.remove();
    }
  });
});
