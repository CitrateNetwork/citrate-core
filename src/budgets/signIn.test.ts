// HUP-S2.3: live sign-in from the managed browser: the pure helpers the main window uses.
import { describe, it, expect } from "vitest";
import { isSignInRequestId, newSignInIds, parseAutoSignedNotice, signedForYouText } from "./signIn";

describe("HUP-S2.3 sign-in requests", () => {
  it("accepts only the ids the sidecar mints", () => {
    expect(isSignInRequestId("signin-3-123456")).toBe(true);
    for (const bad of ["", "signin-", "signin-x", "b7", "signin-1;x", "signin-" + "1".repeat(60), 7, null]) {
      expect(isSignInRequestId(bad)).toBe(false);
    }
  });

  it("hands each request to core once, oldest first, with a bounded memory", () => {
    const a = newSignInIds(["signin-1-1", "signin-2-2", "nope"], new Set());
    expect(a.fresh).toEqual(["signin-1-1", "signin-2-2"]);
    const b = newSignInIds(["signin-1-1", "signin-2-2", "signin-3-3"], a.seen);
    expect(b.fresh).toEqual(["signin-3-3"]);
    const many = newSignInIds(Array.from({ length: 10 }, (_, i) => `signin-${i}-0`), new Set(), 4);
    expect(many.fresh).toHaveLength(10);
    expect(many.seen.size).toBe(4);
    expect(many.seen.has("signin-9-0")).toBe(true);
  });

  it("says where Hermes signed in and what is left", () => {
    expect(signedForYouText({ origin: "https://app.example.org", budgetId: 2, recordId: 9, remaining: 1 })).toBe(
      "Hermes signed you in to https://app.example.org with your wallet, inside the budget you set (1 automatic sign-in left).",
    );
    expect(signedForYouText({ origin: "https://a.example", budgetId: 2, recordId: 9, remaining: 4 })).toContain("4 automatic sign-ins left");
  });

  it("drops a malformed notice", () => {
    expect(parseAutoSignedNotice({ origin: "https://a.example", budgetId: 1, recordId: 2, remaining: 3 })).toEqual({ origin: "https://a.example", budgetId: 1, recordId: 2, remaining: 3 });
    for (const bad of [null, "x", {}, { origin: "", budgetId: 1, recordId: 2, remaining: 3 }, { origin: "https://a", budgetId: -1, recordId: 2, remaining: 3 }, { origin: "https://a", budgetId: 1.5, recordId: 2, remaining: 3 }, { origin: "x".repeat(301), budgetId: 1, recordId: 2, remaining: 3 }]) {
      expect(parseAutoSignedNotice(bad)).toBeNull();
    }
  });
});
