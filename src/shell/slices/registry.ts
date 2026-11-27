// L0-WP06 (red step): the slice registry's interface (L0 section 3.4). The checks and the
// persistence land in the green commit; this file exists so the red tests compile and fail on
// their assertions.
import type { Slice } from "./createSlice";

export type SliceLane = `L${0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;

export interface SliceRegistration<S extends object> {
  id: string;
  lane: SliceLane;
  slice: Slice<S>;
  persist?: { scope: "device" | "network"; keys: (keyof S)[]; storageKey: `citrate-core.v60.${string}` };
  sealed?: boolean;
}

const registered: SliceRegistration<object>[] = [];

export function registerSlice<S extends object>(r: SliceRegistration<S>): Slice<S> {
  registered.push(r as unknown as SliceRegistration<object>);
  return r.slice;
}

export function registeredSlices(): readonly SliceRegistration<object>[] {
  return registered;
}
