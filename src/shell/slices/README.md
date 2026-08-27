# src/shell/slices — CX per-feature state (planset citrate-core-social)

The parallel-safe store convention for CX features (planset 01_SCOPE §4-§5, 02_ARCHITECTURE §3).

The app's original UI state is a single `class Store` (`../store.ts`, `AppState` in `../state.ts`)
— a monolith every feature would otherwise edit. To let CX feature lanes (S1..S6) build without
racing on that shared file, **each CX feature owns exactly one slice file here**, built with
`createSlice`, kept separate from the class Store.

## The rule

- One file per feature: `models.ts`, `storage.ts`, `groups.ts`, `cluster.ts`, `training.ts`,
  `agent.ts`. A lane touches only its own slice (enforced by `scripts/cx-ownership-check.sh`).
- A slice holds that feature's UI state + the actions that call `bridge.<domain>` and fold
  results back with `set(...)`. It never imports another feature's slice.

## The pattern

```ts
// src/shell/slices/models.ts   (owned by lane s1)
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";

const slice = createSlice({ catalog: [], active: null as string | null, status: "idle" });
export const useModels = slice.use;

export const modelsActions = {
  async refresh() {
    const catalog = await bridge.models.catalog();   // frozen interface (S0.2)
    slice.set({ catalog });
  },
};
```

A surface then does `const s = useModels()` and calls `modelsActions.refresh()`. No edit to
`store.ts` / `state.ts` — so no race, and the legacy Store keeps its 312 tests.

`createSlice` mirrors the app's `useSyncExternalStore` reactivity; `get()` is a stable reference
between `set()`s (getSnapshot stability). See `createSlice.test.ts`.
