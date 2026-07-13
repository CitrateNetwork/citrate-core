// CORE-A3 A3-03 — entitlement-expiry enforcement. The `expiresAt` claim gates
// whether a signed-in session keeps its tier: a past (or anomalous) expiry
// downgrades to free/lapsed at the DECISION point, not just in the UI. This is
// the frontend half of the entitlement engine; the Rust id_token `exp` guard is
// the hard backstop (oidc::tests).
import { describe, it, expect } from "vitest";
import { isExpiredClaim } from "./store";

describe("isExpiredClaim — A3-03 entitlement-expiry enforcement", () => {
  it("absent/empty expiry is NOT expired (authority may omit it)", () => {
    expect(isExpiredClaim(null)).toBe(false);
    expect(isExpiredClaim(undefined)).toBe(false);
    expect(isExpiredClaim("")).toBe(false);
  });

  it("a past ISO date is expired", () => {
    expect(isExpiredClaim("2000-01-01")).toBe(true);
    expect(isExpiredClaim("2020-06-15T12:00:00Z")).toBe(true);
  });

  it("a future ISO date is NOT expired", () => {
    expect(isExpiredClaim("2999-01-01")).toBe(false);
    expect(isExpiredClaim("2100-12-31T23:59:59Z")).toBe(false);
  });

  it("a past unix-seconds string is expired; a future one is not", () => {
    expect(isExpiredClaim(String(Math.floor(Date.now() / 1000) - 3600))).toBe(true);
    expect(isExpiredClaim(String(Math.floor(Date.now() / 1000) + 3600))).toBe(false);
  });

  it("an unparseable expiry FAILS CLOSED (treated as expired — T1 gating)", () => {
    expect(isExpiredClaim("not-a-date")).toBe(true);
    expect(isExpiredClaim("2026-13-45")).toBe(true);
  });
});
