// L0-WP07: how a lane action module (src/shell/actions/l<N>/*.ts) reaches the Store's private
// members. The Store registers one narrow view of them (StoreInternals, declared in store.ts) when it
// is constructed; a lane module reads it with storeInternals(store), never by casting the Store to
// `any`. This file imports only types from store.ts, so a lane module that imports it has no value
// import cycle with the Store.
import type { Store, StoreInternals } from "./store";

const INTERNALS = new WeakMap<Store, StoreInternals>();

/** Called once, by the Store constructor. A second registration for the same Store is refused, so
 *  no module can swap in a different view of an existing Store. */
export function bindStoreInternals(store: Store, internals: StoreInternals): void {
  if (INTERNALS.has(store)) throw new Error("storeInternals: this Store is already bound");
  INTERNALS.set(store, internals);
}

/** The private members a lane action module may use, for a Store built by its constructor. */
export function storeInternals(store: Store): StoreInternals {
  const internals = INTERNALS.get(store);
  if (!internals) throw new Error("storeInternals: not a constructed Store");
  return internals;
}
