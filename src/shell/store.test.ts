// CORE-A3 A3-03 — entitlement-expiry enforcement. The `expiresAt` claim gates
// whether a signed-in session keeps its tier: a past (or anomalous) expiry
// downgrades to free/lapsed at the DECISION point, not just in the UI. This is
// the frontend half of the entitlement engine; the Rust id_token `exp` guard is
// the hard backstop (oidc::tests).
import { describe, it, expect } from "vitest";
import { isExpiredClaim, isPaidEntitlementActive, deriveIdentityFromEmail, mapNodeState, store } from "./store";
import { PERSIST_KEYS } from "./state";

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

// CORE-A3 identity — the authority issues no display-name claim, so name +
// initials are derived from the email local-part. Unit-tested independently of
// the auth plumbing (applyAuthStatus uses this exact helper).
describe("deriveIdentityFromEmail — display name/initials from an email", () => {
  it("single-word local part → capitalized name + first two letters", () => {
    expect(deriveIdentityFromEmail("larry@citrate.ai")).toEqual({ name: "Larry", initials: "LA" });
  });
  it("separators (. _ + -) split into title-cased words + first-letter initials", () => {
    expect(deriveIdentityFromEmail("ada.lovelace@x.io")).toEqual({ name: "Ada Lovelace", initials: "AL" });
    expect(deriveIdentityFromEmail("jean-luc_picard@x.io").name).toBe("Jean Luc Picard");
    expect(deriveIdentityFromEmail("jean-luc_picard@x.io").initials).toBe("JL");
  });
});

// CORE-A3 identity resolver — the single seam that stops prototype persona
// identity (Dana Okafor et al.) from ever showing to a real signed-in account.
describe("store.identity() — real signed-in user vs sim persona", () => {
  it("falls back to the sim persona when NOT signed in", () => {
    store.setState({ signedIn: false, authEmail: null, persona: "p1" });
    const id = store.identity();
    expect(id.real).toBe(false);
    expect(id.name).toBe("Dana Okafor"); // PERSONAS.p1
  });
  it("returns the REAL user when signed in with an email claim", () => {
    store.setState({
      signedIn: true,
      authEmail: "larry@citrate.ai",
      authName: "Larry",
      authInitials: "LA",
      authSub: "4925a564",
      walletAddr: "0xbb3a",
      tier: "pilot",
      citrateRole: "member",
      org: null,
    });
    const id = store.identity();
    expect(id.real).toBe(true);
    expect(id.email).toBe("larry@citrate.ai");
    expect(id.name).toBe("Larry");
    expect(id.role).toBe("member");
    expect(id.sub).toBe("4925a564");
  });
  it("signed in but /userinfo not yet folded (no email) → neutral REAL placeholder, NEVER the persona", () => {
    store.setState({ signedIn: true, authEmail: null, authName: null, authInitials: null, persona: "p1" });
    const id = store.identity();
    expect(id.real).toBe(true);
    expect(id.name).not.toBe("Dana Okafor"); // must not leak the sim persona
    expect(id.name).toBe("Member");
  });
});

// HIPAA sign-out-by-default — the signed-in session + account PII are NEVER
// written to disk; every launch starts signed-out and re-derives identity only
// from a live authority session. This test guards that invariant against a
// regression that would re-add the fields to the persisted set.
describe("HIPAA sign-out-by-default — no session/PII persisted", () => {
  it("PERSIST_KEYS excludes every real-identity field", () => {
    for (const k of ["signedIn", "authSub", "authEmail", "authName", "authInitials"] as const) {
      expect(PERSIST_KEYS).not.toContain(k);
    }
  });
});

// CORE Phase 1 — the REAL node poller maps supervisor state (node.rs map_state)
// onto the app's node lifecycle enum. This replaces the sim tick() node state in
// a Tauri build so height/peers/sync are live 40204 truth, not fabricated.
describe("mapNodeState — supervisor state → app node lifecycle", () => {
  it("stopped/unknown → off", () => {
    expect(mapNodeState("stopped", 0, 0)).toBe("off");
    expect(mapNodeState("whatever", 100, 99999)).toBe("off");
  });
  it("starting/restarting → prov; failed → error", () => {
    expect(mapNodeState("starting", 0, 0)).toBe("prov");
    expect(mapNodeState("restarting", 0, 0)).toBe("prov");
    expect(mapNodeState("failed", 0, 0)).toBe("error");
  });
  it("running while not fully synced → syncing", () => {
    expect(mapNodeState("running", 42, 32000)).toBe("syncing");
  });
  it("running + synced + staked at/above threshold → validating", () => {
    expect(mapNodeState("running", 100, 32000)).toBe("validating");
  });
  it("running + synced but under-staked → synced (not validating)", () => {
    expect(mapNodeState("running", 100, 31999)).toBe("synced");
    expect(mapNodeState("running", 100, 0)).toBe("synced");
  });
});
