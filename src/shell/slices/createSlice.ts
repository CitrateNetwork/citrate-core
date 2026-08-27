// =====================================================================
// citrate-core — CX slice factory (planset citrate-core-social, CX-S0.1)
//
// The per-feature state primitive for CX surfaces. Each CX feature lane (S1..S6)
// owns exactly ONE slice file built with `createSlice`, kept SEPARATE from the
// legacy `class Store` — so a feature lane never edits shared shell state and
// two lanes can never race on `store.ts` / `state.ts` (planset 01_SCOPE §4-§5).
//
// It mirrors the app's existing external-store pattern (`useSyncExternalStore`),
// so slice state is React-reactive the same way `useStore()` is. `get()` returns
// a stable reference between `set()`s, satisfying getSnapshot stability.
// =====================================================================
import { useSyncExternalStore } from "react";

export interface Slice<S extends object> {
  /** Current state (stable reference until the next set). */
  get(): S;
  /** Merge a patch (or a patch computed from current state) and notify subscribers. */
  set(patch: Partial<S> | ((s: S) => Partial<S>)): void;
  /** Subscribe to changes; returns an unsubscribe. */
  subscribe(fn: () => void): () => void;
  /** React hook — re-renders on any change to this slice. */
  use(): S;
}

export function createSlice<S extends object>(initial: S): Slice<S> {
  let state: S = initial;
  const subs = new Set<() => void>();

  const get = (): S => state;

  const set: Slice<S>["set"] = (patch) => {
    const p = typeof patch === "function" ? patch(state) : patch;
    // New object identity so getSnapshot sees a change; no-op patches still
    // allocate but that is cheap and keeps the contract simple + correct.
    state = { ...state, ...p };
    for (const f of subs) f();
  };

  const subscribe = (fn: () => void): (() => void) => {
    subs.add(fn);
    return () => {
      subs.delete(fn);
    };
  };

  const use = (): S => useSyncExternalStore(subscribe, get, get);

  return { get, set, subscribe, use };
}
