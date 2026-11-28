// L0-WP02 SEAM-ROUTES: which component each canonical route renders, and where every routable
// string resolves, recorded at a826e441. L0-WP08 puts a route registry in front of PlaceView's
// `surfaceFor` switch; every current route must keep rendering the same component with the same
// props, and every legacy id must keep resolving as before. The fixtures hold the built-in tables
// only (BUILTIN_CANONICAL_ROUTES, BUILTIN_ROUTABLE): a route a lane registers in its
// src/shell/routes/l<N>.tsx is checked to be registered, never snapshotted, so a lane never needs
// an L0 fixture change (src/shell/routes/bootstrap.test.tsx loads one through the bootstrap).
import { describe, expect, it } from "vitest";
import { isValidElement, type ReactNode } from "react";
import { surfaceFor } from "../PlaceView";
import { BUILTIN_CANONICAL_ROUTES, BUILTIN_ROUTABLE, CANONICAL_ROUTES, ROUTABLE, resolveRoute } from "../places";
import { registeredRoutes } from "../routes/registry";
import { store } from "../store";
import { SEAMS_BASE, checkSnapshot, diffFixture } from "../../../scripts/seams-snapshot.mjs";

function typeName(t: unknown): string {
  if (typeof t === "string") return t;
  if (typeof t === "function") return t.name || "<anonymous>";
  const o = t as { displayName?: string; type?: unknown; render?: unknown } | null;
  if (o?.displayName) return o.displayName;
  if (o?.type) return `memo(${typeName(o.type)})`;
  if (o?.render) return `forwardRef(${typeName(o.render)})`;
  return String(t);
}

/** The component and its props other than the store and state every surface receives. */
function rendered(node: ReactNode): unknown {
  if (!isValidElement(node)) return node === null ? "null" : typeof node;
  const { store: _store, s: _s, ...props } = node.props as Record<string, unknown>;
  return { component: typeName(node.type), props };
}

describe("SEAM-ROUTES: route to surface", () => {
  it("every_canonical_route_renders_the_same_component", () => {
    const s = store.state;
    const table: Record<string, unknown> = {};
    for (const r of [...BUILTIN_CANONICAL_ROUTES].sort()) table[r] = rendered(surfaceFor(r, store, s));
    // An unknown route falls through to the default surface.
    table["<unknown>"] = rendered(surfaceFor("seams/not-a-route", store, s));
    expect(BUILTIN_CANONICAL_ROUTES.length).toBeGreaterThan(20);
    // Every live canonical route is built-in or registered by a lane; nothing else gets in.
    expect([...CANONICAL_ROUTES].sort()).toEqual([...BUILTIN_CANONICAL_ROUTES, ...registeredRoutes()].sort());
    const rel = `fixtures/seams/routes-surfaces.${SEAMS_BASE}.json`;
    expect(checkSnapshot(rel, table)).toEqual([]);
    // Mutation: a route that renders another component is named.
    const moved = { ...table, wallet: { component: "Dashboard", props: {} } };
    expect(diffFixture(rel, moved)).toEqual(['~ wallet.component: "Wallet" -> "Dashboard"']);
  });

  it("every_routable_string_resolves_as_before", () => {
    const table: Record<string, unknown> = {};
    for (const r of [...BUILTIN_ROUTABLE].sort()) table[r] = resolveRoute(r);
    expect([...ROUTABLE].sort()).toEqual([...BUILTIN_ROUTABLE, ...registeredRoutes()].sort());
    expect(checkSnapshot(`fixtures/seams/routes-resolve.${SEAMS_BASE}.json`, table)).toEqual([]);
  });
});
