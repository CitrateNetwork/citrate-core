// citrate-core: the route and surface registry (L0-WP08, lanes/L0_INTEGRATION_SEAMS.md 3.5).
import type { ReactNode } from "react";
import type { Store } from "../store";
import type { AppState } from "../state";
import type { PlaceId } from "../places";

export type LaneId = `L${0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;

export interface SurfaceTab {
  place: PlaceId;
  label: string;
  order: number;
}

export interface SurfaceDecl {
  route: string;
  lane: LaneId;
  render: (store: Store, s: AppState) => ReactNode;
  tab?: SurfaceTab;
}

const registered = new Map<string, SurfaceDecl>();

export function registerSurfaces(decls: SurfaceDecl[]): void {
  for (const d of decls) registered.set(d.route, d);
}

export function surfaceForRoute(route: string): SurfaceDecl | undefined {
  return registered.get(route);
}
