// GROW-S0 — referral link primitive: build/parse round-trips, display-only encoding, dignity (no
// more than a name), general vs cluster invites.
import { describe, it, expect } from "vitest";
import { buildJoinLink, parseJoinLink, shortInviter, JOIN_BASE } from "./referral";

describe("buildJoinLink — GROW-S0", () => {
  it("builds a cluster invite with display params + full-address attribution", () => {
    const link = buildJoinLink({
      clusterId: "grp_aperture",
      clusterName: "Aperture",
      goal: "inference",
      inviter: "0x90e0b7abc0000000000000000000000000000001",
      inviterHandle: "dana",
    });
    expect(link.startsWith(`${JOIN_BASE}/grp_aperture?`)).toBe(true);
    expect(link).toContain("by=dana");
    expect(link).toContain("c=Aperture");
    expect(link).toContain("g=inference");
    // full address rides in ref (attribution), not the display name
    expect(link).toContain("ref=0x90e0b7abc0000000000000000000000000000001");
  });

  it("falls back to a short address for display when no handle", () => {
    const link = buildJoinLink({ clusterId: "g1", inviter: "0x90e0b7abc0000000000000000000000000000001" });
    expect(link).toContain("by=0x90e0"); // shortened for display
    expect(link).toContain("ref=0x90e0b7abc0000000000000000000000000000001"); // full for attribution
  });

  it("builds a general network invite when no cluster", () => {
    const link = buildJoinLink({ inviter: "0xabc0000000000000000000000000000000000001", inviterHandle: "dana" });
    expect(link.startsWith(`${JOIN_BASE}?`)).toBe(true);
    expect(link).not.toContain("/join/"); // no cluster segment
    expect(link).toContain("by=dana");
  });

  it("URL-encodes a cluster name with spaces (no broken link)", () => {
    const link = buildJoinLink({ clusterId: "g1", clusterName: "Data Guild" });
    expect(link).toContain("c=Data+Guild");
    expect(parseJoinLink(link).clusterName).toBe("Data Guild");
  });

  it("emits nothing beyond display + ref (dignity: a link never leaks more than a name)", () => {
    const link = buildJoinLink({ clusterId: "g1", clusterName: "A", goal: "storage", inviter: "0x1", inviterHandle: "d" });
    const params = new URL(link).searchParams;
    expect([...params.keys()].sort()).toEqual(["by", "c", "g", "ref"]);
  });
});

describe("parseJoinLink — GROW-S0", () => {
  it("round-trips a full cluster link", () => {
    const parts = { clusterId: "grp_x", clusterName: "Aperture", goal: "inference", inviter: "0xabc", inviterHandle: "dana" };
    expect(parseJoinLink(buildJoinLink(parts))).toEqual(parts);
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
