// L0-WP07: the one view of a Store's private members that lane action modules use.
import { describe, expect, it } from "vitest";
import { Store, store, type StoreInternals } from "./store";
import { bindStoreInternals, storeInternals } from "./storeInternals";

describe("storeInternals", () => {
  it("storeInternals_refuses_a_store_its_constructor_did_not_build", () => {
    const bare = Object.create(Store.prototype) as Store;
    expect(() => storeInternals(bare)).toThrow("storeInternals: not a constructed Store");
  });

  it("bindStoreInternals_refuses_a_second_view_of_the_same_store", () => {
    // Only the refusal is under test, so the forged view need not be a complete one.
    const forged = { archiveT: null, threadParents: new Map(), chatSwitchBlocked: () => null } as unknown as StoreInternals;
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

  it("the_07b_view_reads_and_writes_the_entitlement_poll_and_billing_fields", () => {
    const fields = store as unknown as {
      entitlementPollTimer: ReturnType<typeof setTimeout> | null;
      entitlementPollDeadline: number;
      lastFocusRecheckAt: number;
      billingAttempt: number;
      claimSession: unknown;
    };
    const view = storeInternals(store);
    const before = { timer: fields.entitlementPollTimer, deadline: fields.entitlementPollDeadline, focus: fields.lastFocusRecheckAt, billing: fields.billingAttempt };
    const t = setTimeout(() => {}, 60_000);
    try {
      view.entitlementPollTimer = t;
      view.entitlementPollDeadline = 1234;
      view.lastFocusRecheckAt = 5678;
      view.billingAttempt = before.billing + 1;
      expect(fields.entitlementPollTimer).toBe(t);
      expect(fields.entitlementPollDeadline).toBe(1234);
      expect(fields.lastFocusRecheckAt).toBe(5678);
      expect(fields.billingAttempt).toBe(before.billing + 1);
      expect(view.claimSession).toBe(fields.claimSession);
    } finally {
      clearTimeout(t);
      fields.entitlementPollTimer = before.timer;
      fields.entitlementPollDeadline = before.deadline;
      fields.lastFocusRecheckAt = before.focus;
      fields.billingAttempt = before.billing;
    }
  });
});
