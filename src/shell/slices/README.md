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

## 0.6.0: the slice registry (L0-WP06)

From 0.6.0 every new piece of lane state is a slice registered in
[`registry.ts`](registry.ts); none goes into `AppState`. `AppState` itself is composed from one
part per lane (`../state/core.ts`, `../state/l1.ts` .. `l9.ts`), which hold the legacy fields only.

```ts
// src/shell/slices/groupHistory.ts   (owned by L4)
import { createSlice } from "./createSlice";
import { registerSlice } from "./registry";

export const groupHistory = registerSlice({
  id: "l4.groupHistory",          // l<N>.<name>, unique
  lane: "L4",
  slice: createSlice({ threads: [] as string[], open: null as string | null }),
  sealed: true,                   // D16: sealed state never uses webview storage
});
```

The rules `registerSlice` enforces (it throws and registers nothing otherwise):

- the id starts with the lane (`l4.` for L4) and is not registered yet;
- `persist.storageKey` is `citrate-core.v60.<name>` (a-z, 0-9, `.`, `_`, `-`) and no other slice
  uses it; `persist.keys` are fields of the slice, and only they are written or restored;
- `sealed: true` with a `persist` block is refused ("sealed slices persist through the sealed
  store only"). Any value other than `false` or absent counts as sealed.

A persisted slice is restored at registration and written on every change. `device` scope uses
the key as given; `network` scope keys the entry by genesis (`<key>@<genesis>`), so a value read
on one network is never restored on another. A saved value replaces a default only when it is of
the same JSON kind (a `null` default takes a string, number or boolean). The registry's storage
entry is listed in `../persistScope.test.ts`.

