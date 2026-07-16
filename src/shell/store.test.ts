// CORE-A3 A3-03 — entitlement-expiry enforcement. The `expiresAt` claim gates
// whether a signed-in session keeps its tier: a past (or anomalous) expiry
// downgrades to free/lapsed at the DECISION point, not just in the UI. This is
// the frontend half of the entitlement engine; the Rust id_token `exp` guard is
// the hard backstop (oidc::tests).
import { describe, it, expect } from "vitest";
import { isExpiredClaim, isPaidEntitlementActive } from "./store";

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

// CORE-D3.C — the S3 checkout settle DECISION. The onboarding "Pay" step advances
// ONLY when /userinfo shows the REAL grant: a PAID tier (rank past free/public)
// with an ACTIVE entitlement. This is the exact predicate the tauri poll uses,
// so the Rule-1 property ("never settle before the entitlement lands") is decided
// here and unit-tested independently of the popup/poll plumbing.
describe("isPaidEntitlementActive — D3.C checkout settle decision", () => {
  it("free/public tier is NOT paid-active even if entitlement reads active", () => {
    expect(isPaidEntitlementActive({ tier: "free", entitlement: "active" })).toBe(false);
  });

  it("a paid tier (pilot/enterprise) with an ACTIVE entitlement IS the landed grant", () => {
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "active" })).toBe(true);
    expect(isPaidEntitlementActive({ tier: "enterprise", entitlement: "active" })).toBe(true);
  });

  it("a paid tier that is NOT active (lapsed/grace/expiring) has NOT settled", () => {
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "lapsed" })).toBe(false);
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "grace" })).toBe(false);
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "expiring" })).toBe(false);
  });

  it("an unknown tier fails closed (treated as unpaid)", () => {
    expect(isPaidEntitlementActive({ tier: "mystery", entitlement: "active" })).toBe(false);
    expect(isPaidEntitlementActive({ tier: "", entitlement: "active" })).toBe(false);
  });
});
