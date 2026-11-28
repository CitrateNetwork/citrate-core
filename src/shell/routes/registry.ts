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
//     so a lane can never replace a built-in surface or redirect;
//   - a route with no place: the tab's place, or else the route's first segment, must be a place,
//     and a tab may not sit in another place than the one the route's first segment names;
//   - a tab with an empty label or an order below 10 (built-in tabs are 10, 20, 30, …), so a
//     registered tab never becomes a place's first tab, which a bare place id opens.
// A registered surface renders inside the shell, behind every gate App.tsx applies before it
// (sign-in, onboarding, network compatibility). It adds no gate of its own: a surface that needs
// one (a claim, a HIC level) checks it in its render, as built-in surfaces do.
// =====================================================================
import type { ReactNode } from "react";
import type { Store } from "../store";
import type { AppState } from "../state";
import { PLACES, extendPlaces, resolveRoute, type PlaceId } from "../places";

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
  render: (store: Store, s: AppState) => ReactNode;
  tab?: SurfaceTab;
}

const LANE_RE = /^L[0-9]$/;
const SEGMENT = "[a-z0-9]+(?:-[a-z0-9]+)*";
const ROUTE_RE = new RegExp(`^${SEGMENT}(?:/${SEGMENT})*$`);
const FIRST_TAB_ORDER = 10;

const registered = new Map<string, SurfaceDecl>();

const isPlace = (id: unknown): id is PlaceId => PLACES.some((p) => p.id === id);

/** The place a declaration's route belongs to, or a refusal. */
function placeOf(d: SurfaceDecl): PlaceId {
  const first = d.route.split("/")[0];
  if (!d.tab) {
    if (!isPlace(first)) throw new Error(`route "${d.route}": names no place (start it with a place id or give it a tab)`);
    return first;
  }
  const { place, label, order } = d.tab;
  if (!isPlace(place)) throw new Error(`route "${d.route}": tab place "${String(place)}" is not a place`);
  if (isPlace(first) && first !== place) throw new Error(`route "${d.route}": is under ${first} but its tab is in ${place}`);
  if (typeof label !== "string" || label.trim() === "") throw new Error(`route "${d.route}": tab needs a label`);
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
  if (resolveRoute(d.route) !== null) throw new Error(`route "${d.route}" is a built-in route (places.ts); ${d.lane} cannot take it`);
  if (typeof d.render !== "function") throw new Error(`route "${d.route}": render must be a function`);
  return placeOf(d);
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
    registered.set(d.route, Object.freeze({ route: d.route, lane: d.lane, render: d.render, tab }));
    extendPlaces(d.route, places[i], tab && { label: tab.label, order: tab.order });
  });
}

/** The registered declaration for a route; undefined for a built-in or unknown route. */
export function surfaceForRoute(route: string): SurfaceDecl | undefined {
  return registered.get(route);
}
