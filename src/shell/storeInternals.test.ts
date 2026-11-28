// L0-WP07: the one view of a Store's private members that lane action modules use.
import { describe, expect, it } from "vitest";
import { Store, store } from "./store";
import { bindStoreInternals, storeInternals } from "./storeInternals";

describe("storeInternals", () => {
  it("storeInternals_refuses_a_store_its_constructor_did_not_build", () => {
    const bare = Object.create(Store.prototype) as Store;
    expect(() => storeInternals(bare)).toThrow("storeInternals: not a constructed Store");
  });

  it("bindStoreInternals_refuses_a_second_view_of_the_same_store", () => {
    const forged = { archiveT: null, threadParents: new Map(), chatSwitchBlocked: () => null };
    expect(() => bindStoreInternals(store, forged)).toThrow("storeInternals: this Store is already bound");
    // The registered view is still the Store's own.
    expect(storeInternals(store)).not.toBe(forged);
  });

  it("the_view_reads_and_writes_the_store_fields_themselves", () => {
    const fields = store as unknown as { _archiveT: ReturnType<typeof setTimeout> | null; threadParents: Map<string, unknown> };
    const view = storeInternals(store);
    const t = setTimeout(() => {}, 60_000);
    try {
      view.archiveT = t;
      expect(fields._archiveT).toBe(t);
      fields._archiveT = null;
      expect(view.archiveT).toBeNull();
      expect(view.threadParents).toBe(fields.threadParents);
    } finally {
      clearTimeout(t);
    }
  });
});
