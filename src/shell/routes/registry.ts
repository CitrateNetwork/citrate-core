// =====================================================================
// citrate-core: the route and surface registry (L0-WP08, lanes/L0_INTEGRATION_SEAMS.md 3.5)
//
// A lane adds a screen by declaring it in its own src/shell/routes/l<N>.tsx, never by editing
// PlaceView.tsx or places.ts. PlaceView's surfaceFor asks this registry first and then falls
// through to its switch; a built-in route can never be registered, so every built-in route renders
// what it rendered before. registerSurfaces checks the whole batch first and registers nothing
// from it when one declaration is refused:
//   - a lane other than L0 to L9, or a render that is not a function;
//   - a route that is not lowercase segments (a-z, 0-9, inner '-') joined by '/';
//   - a route another registration holds (the error names both lanes);
//   - a built-in route: canonical, legacy, a place prefix or a bare place (resolveRoute knows it),
//     so a lane can never replace a built-in surface or redirect, or a RESERVED id below;
//   - a route with no place: the tab's place, or else the route's first segment, must be a place,
//     and a tab may not sit in another place than the one the route's first segment names;
//   - a route under a built-in sub-tab strip ("node/storage/x", "groups/rounds/x"): it would show
//     as a top-level tab outside that strip, so a lane asks L3 to add a sub-tab instead;
//   - a tab with an empty label, a label its place already shows (any case), or an order below 10
//     (built-in tabs are 10, 20, 30, …), so a registered tab never becomes a place's first tab,
//     which a bare place id opens;
//   - a register other than "instrument" or "charter" (charter when none is given).
// A registered surface renders inside the shell, behind every gate App.tsx applies before it
// (sign-in, onboarding, network compatibility). It adds no gate of its own: a surface that needs
// one (a claim, a HIC level) checks it in its render, as built-in surfaces do.
//
// render runs as the body of its own component (RegisteredSurface in PlaceView.tsx), keyed by
// route, so it may call hooks: PlaceView's hook order never depends on which route is open.
//
// Registration runs when src/shell/routes/index.ts loads the lane files, at module load, and a
// refusal throws there, so the app (and every test that imports PlaceView) fails at import. That
// includes a later built-in route in places.ts that collides with one a lane registered: the
// conflict surfaces the first time either side's CI loads the shell. In dev, a hot reload of a lane
// file runs its registerSurfaces again, which refuses its own routes as taken: reload the page.
// =====================================================================
import type { ReactNode } from "react";
import type { Store } from "../store";
import type { AppState } from "../state";
import { PLACES, REGISTERS, extendPlaces, resolveRoute, subStripOf, type PlaceId, type Register } from "../places";

export type LaneId = `L${0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;

export interface SurfaceTab {
  place: PlaceId;
  label: string;
  /** Position in the place's strip; at least 10. Built-in tabs are 10, 20, 30, … */
  order: number;
}

export interface SurfaceDecl {
  route: string;
  lane: LaneId;
  /** Runs as a component body (hooks allowed); returns the surface. */
  render: (store: Store, s: AppState) => ReactNode;
  tab?: SurfaceTab;
  /** The shell's register on this route: dark live telemetry or light document. Default charter. */
  register?: Register;
}

const LANE_RE = /^L[0-9]$/;
const SEGMENT = "[a-z0-9]+(?:-[a-z0-9]+)*";
const ROUTE_RE = new RegExp(`^${SEGMENT}(?:/${SEGMENT})*$`);
const FIRST_TAB_ORDER = 10;

/** Ids the app navigates to by name (store.go) that no place resolves: a claim link lands at
 *  sign-in on "onboarding". A lane may not take one, or it would capture that navigation. */
export const RESERVED_ROUTES: readonly string[] = Object.freeze(["onboarding"]);

const registered = new Map<string, SurfaceDecl>();

const isPlace = (id: unknown): id is PlaceId => PLACES.some((p) => p.id === id);

/** The place a declaration's route belongs to, or a refusal. */
function placeOf(d: SurfaceDecl, batch: Map<string, SurfaceDecl>): PlaceId {
  const first = d.route.split("/")[0];
  if (!d.tab) {
    if (!isPlace(first)) throw new Error(`route "${d.route}": names no place (start it with a place id or give it a tab)`);
    return first;
  }
  const { place, label, order } = d.tab;
  if (!isPlace(place)) throw new Error(`route "${d.route}": tab place "${String(place)}" is not a place`);
  if (isPlace(first) && first !== place) throw new Error(`route "${d.route}": is under ${first} but its tab is in ${place}`);
  if (typeof label !== "string" || label.trim() === "") throw new Error(`route "${d.route}": tab needs a label`);
  const same = (x: string) => x.trim().toLowerCase() === label.trim().toLowerCase();
  const shown = PLACES.find((p) => p.id === place)?.tabs.map((t) => t.label) ?? [];
  for (const b of batch.values()) if (b.tab?.place === place) shown.push(b.tab.label);
  const taken = shown.find(same);
  if (taken !== undefined) throw new Error(`route "${d.route}": ${place} already has a tab "${taken}"`);
  if (typeof order !== "number" || !Number.isFinite(order) || order < FIRST_TAB_ORDER) {
    throw new Error(`route "${d.route}": tab order must be a number of at least ${FIRST_TAB_ORDER}`);
  }
  return place;
}

function validate(d: SurfaceDecl, batch: Map<string, SurfaceDecl>): PlaceId {
  const route = String(d?.route);
  if (typeof d?.lane !== "string" || !LANE_RE.test(d.lane)) throw new Error(`route "${route}": lane must be L0 to L9`);
  if (typeof d.route !== "string" || !ROUTE_RE.test(d.route)) {
    throw new Error(`route "${route}": must be lowercase segments (a-z, 0-9, -) joined by /`);
  }
  const held = registered.get(d.route) ?? batch.get(d.route);
  if (held) throw new Error(`route "${d.route}" is registered by ${held.lane}; ${d.lane} cannot register it again`);
  if (resolveRoute(d.route) !== null || RESERVED_ROUTES.includes(d.route)) {
    throw new Error(`route "${d.route}" is a built-in route (places.ts) or reserved; ${d.lane} cannot take it`);
  }
  const strip = subStripOf(d.route);
  if (strip) throw new Error(`route "${d.route}": is under the built-in sub-tab strip "${strip}"; ${d.lane} cannot add to it`);
  if (typeof d.render !== "function") throw new Error(`route "${d.route}": render must be a function`);
  if (d.register !== undefined && !REGISTERS.includes(d.register)) {
    throw new Error(`route "${d.route}": register must be ${REGISTERS.join(" or ")}`);
  }
  return placeOf(d, batch);
}

/** Register a lane's surfaces. Throws, registering none of them, when one breaks a rule above. */
export function registerSurfaces(decls: SurfaceDecl[]): void {
  if (!Array.isArray(decls)) throw new Error("registerSurfaces takes an array of declarations");
  const batch = new Map<string, SurfaceDecl>();
  const places: PlaceId[] = [];
  for (const d of decls) {
    places.push(validate(d, batch));
    batch.set(d.route, d);
  }
  decls.forEach((d, i) => {
    const tab = d.tab ? Object.freeze({ place: d.tab.place, label: d.tab.label, order: d.tab.order }) : undefined;
    const register = d.register ?? "charter";
    registered.set(d.route, Object.freeze({ route: d.route, lane: d.lane, render: d.render, tab, register }));
    extendPlaces(d.route, places[i], tab && { label: tab.label, order: tab.order }, register);
  });
}

/** Every registered route, in registration order. */
export function registeredRoutes(): string[] {
  return [...registered.keys()];
}

/** The registered declaration for a route; undefined for a built-in or unknown route. */
export function surfaceForRoute(route: string): SurfaceDecl | undefined {
  return registered.get(route);
}
