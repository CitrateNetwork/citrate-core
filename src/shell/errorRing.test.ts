// Telemetry WP-T.2 — error-ring tests (cap, newest-last, ignore-empty, clear).
import { describe, it, expect, beforeEach } from "vitest";
import { recordUiError, getUiErrors, clearUiErrors } from "./errorRing";

describe("errorRing", () => {
  beforeEach(() => clearUiErrors());

  it("records and returns errors oldest→newest (a copy)", () => {
    recordUiError("first");
    recordUiError("second");
    expect(getUiErrors()).toEqual(["first", "second"]);
    getUiErrors().push("mutation"); // returned array is a copy
    expect(getUiErrors()).toEqual(["first", "second"]);
  });

  it("ignores empty/whitespace and caps at 20 (drops oldest)", () => {
    recordUiError("   ");
    expect(getUiErrors()).toEqual([]);
    for (let i = 0; i < 25; i++) recordUiError("e" + i);
    const out = getUiErrors();
    expect(out.length).toBe(20);
    expect(out[0]).toBe("e5"); // e0..e4 dropped
    expect(out[out.length - 1]).toBe("e24");
  });

  it("clear empties the ring", () => {
    recordUiError("x");
    clearUiErrors();
    expect(getUiErrors()).toEqual([]);
  });
});
