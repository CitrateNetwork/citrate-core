// @vitest-environment node
// GROW-S0 — the legacy display-link parser (links already posted keep rendering) and the claim
// link parser. The v2 invite links are covered in inviteLink.test.ts.
import { describe, it, expect } from "vitest";
import { parseJoinLink, shortInviter, parseClaimLink } from "./referral";

describe("parseJoinLink — GROW-S0", () => {
  it("reads a legacy display link (already posted) for its display fields", () => {
    expect(parseJoinLink("https://citrate.ai/join/grp_x?by=dana&c=Aperture&g=inference&ref=0xabc")).toEqual({
      clusterId: "grp_x",
      clusterName: "Aperture",
      goal: "inference",
      inviter: "0xabc",
      inviterHandle: "dana",
    });
  });
  it("parses a bare cluster link with no query", () => {
    expect(parseJoinLink("https://citrate.ai/join/grp_x")).toEqual({ clusterId: "grp_x" });
  });
  it("tolerates a scheme-less link", () => {
    expect(parseJoinLink("citrate.ai/join/grp_x?by=dana").clusterId).toBe("grp_x");
  });
  it("returns empty parts for garbage, never throws", () => {
    expect(parseJoinLink("not a link at all")).toEqual({});
  });
});

describe("shortInviter — GROW-S0", () => {
  it("shortens a long address and defaults to 'someone'", () => {
    expect(shortInviter("0x90e0b7abc0000000000000000000000000000001")).toBe("0x90e0…0001");
    expect(shortInviter("")).toBe("someone");
    expect(shortInviter(null)).toBe("someone");
  });
});

// PHONEPAY-S4 — the claim/onboard deep-link parser. Params are untrusted display HINTS
// (A8): a valid email/order shape is surfaced; anything else is dropped; a non-claim link
// returns null so join/invite handling is untouched.
describe("parseClaimLink — PHONEPAY-S4", () => {
  it("parses a citrate://claim deep-link with email + order hints", () => {
    const p = parseClaimLink("citrate://claim?email=buyer%40corp.com&order=ord_abc123");
    expect(p).not.toBeNull();
    expect(p!.email).toBe("buyer@corp.com");
    expect(p!.order).toBe("ord_abc123");
  });

  it("parses the https fallback and the order_id alias", () => {
    const p = parseClaimLink("https://citrate.ai/claim?email=a%40b.io&order_id=xyz-9");
    expect(p!.email).toBe("a@b.io");
    expect(p!.order).toBe("xyz-9");
  });

  it("returns null for join/invite links so they fall through untouched", () => {
    expect(parseClaimLink("citrate://invite?ref=0x90e0b7")).toBeNull();
    expect(parseClaimLink("https://citrate.ai/join/ABCD2345")).toBeNull();
    expect(parseClaimLink("not a url at all !!")).toBeNull();
  });

  it("drops a malformed email or oversized/garbage order hint (never trusts raw input)", () => {
    const bad = parseClaimLink("citrate://claim?email=notanemail&order=" + "x".repeat(80));
    expect(bad).not.toBeNull(); // still a claim link…
    expect(bad!.email).toBeUndefined(); // …but the bad hints are dropped
    expect(bad!.order).toBeUndefined();
    const inject = parseClaimLink("citrate://claim?order=" + encodeURIComponent("<script>alert(1)</script>"));
    expect(inject!.order).toBeUndefined();
  });

  it("a claim link with no hints is still recognized (empty parts, not null)", () => {
    expect(parseClaimLink("citrate://claim")).toEqual({});
  });
});
