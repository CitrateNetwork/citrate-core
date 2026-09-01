// @vitest-environment node
// GROW-S0 — referral link primitive: build/parse round-trips, display-only encoding, dignity (no
// more than a name), general vs cluster invites.
import { describe, it, expect, vi } from "vitest";
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

// ===========================================================================
// GROW-S1b — opaque short-code detection + the EdDSA-signed resolve (verify + reject forgery)
// ===========================================================================
import { resolveJoinCode, CODE_RE, JOIN_ISSUER } from "./referral";
import { SignJWT, generateKeyPair, exportJWK, createLocalJWKSet } from "jose";

const CODE = "Ab3Kp9Rs"; // 8 chars from the resolver alphabet

describe("parseJoinLink — GROW-S1b short-code detection", () => {
  it("treats an 8-char code with NO display params as a code link", () => {
    const p = parseJoinLink("https://citrate.ai/join/Ab3Kp9Rs");
    expect(p.code).toBe("Ab3Kp9Rs");
    expect(p.clusterId).toBeUndefined();
    expect(CODE_RE.test("Ab3Kp9Rs")).toBe(true);
  });
  it("a self-contained link (has display params) is NOT a code, even if the seg is 8 chars", () => {
    const p = parseJoinLink("https://citrate.ai/join/Ab3Kp9Rs?by=dana&ref=0xabc");
    expect(p.code).toBeUndefined();
    expect(p.clusterId).toBe("Ab3Kp9Rs");
    expect(p.inviterHandle).toBe("dana");
  });
  it("a normal (non-code-shaped) clusterId stays self-contained", () => {
    expect(parseJoinLink("https://citrate.ai/join/grp_aperture").code).toBeUndefined();
    expect(parseJoinLink("https://citrate.ai/join/grp_aperture").clusterId).toBe("grp_aperture");
  });
  it("detects a code on the citrate:// scheme too", () => {
    expect(parseJoinLink("citrate://join/Ab3Kp9Rs").code).toBe("Ab3Kp9Rs");
  });
});

describe("resolveJoinCode — GROW-S1b verify the EdDSA-signed resolve", () => {
  async function localJwks(publicKey: CryptoKey) {
    const pub = { ...(await exportJWK(publicKey)), use: "sig", alg: "EdDSA", kid: "join-1" };
    return createLocalJWKSet({ keys: [pub] });
  }
  // Mock ONLY the code fetch (GET /api/join/<code>); the JWKS is injected locally (jose's own JWKS
  // fetch bypasses a global fetch mock).
  function mockCodeFetch(sig: string, status = 200, body?: unknown) {
    global.fetch = vi.fn(async () =>
      new Response(JSON.stringify(body ?? { ok: true, cluster: "grp_x", clusterName: "Aperture", inviter: "dana", goal: "inference", sig }), { status }),
    ) as unknown as typeof fetch;
  }

  it("returns the SIGNED display parts when the signature verifies", async () => {
    const { publicKey, privateKey } = await generateKeyPair("EdDSA", { extractable: true });
    const sig = await new SignJWT({ cluster: "grp_x", clusterName: "Aperture", inviter: "dana", goal: "inference" })
      .setProtectedHeader({ alg: "EdDSA", typ: "JWT", kid: "join-1" })
      .setIssuer(JOIN_ISSUER).setIssuedAt().setExpirationTime("30d").sign(privateKey);
    mockCodeFetch(sig);
    expect(await resolveJoinCode(CODE, await localJwks(publicKey))).toEqual({ clusterId: "grp_x", clusterName: "Aperture", inviterHandle: "dana", goal: "inference" });
  });

  it("REJECTS a JWT signed by a different key (forged invite)", async () => {
    const attacker = await generateKeyPair("EdDSA", { extractable: true });
    const legit = await generateKeyPair("EdDSA", { extractable: true });
    const forged = await new SignJWT({ cluster: "grp_evil", inviter: "totally-legit" })
      .setProtectedHeader({ alg: "EdDSA", typ: "JWT", kid: "join-1" })
      .setIssuer(JOIN_ISSUER).setIssuedAt().setExpirationTime("30d").sign(attacker.privateKey);
    mockCodeFetch(forged);
    await expect(resolveJoinCode(CODE, await localJwks(legit.publicKey))).rejects.toThrow();
  });

  it("REJECTS a wrong issuer", async () => {
    const { publicKey, privateKey } = await generateKeyPair("EdDSA", { extractable: true });
    const sig = await new SignJWT({ cluster: "grp_x" })
      .setProtectedHeader({ alg: "EdDSA", typ: "JWT", kid: "join-1" })
      .setIssuer("https://evil.example/join").setIssuedAt().setExpirationTime("30d").sign(privateKey);
    mockCodeFetch(sig);
    await expect(resolveJoinCode(CODE, await localJwks(publicKey))).rejects.toThrow();
  });

  it("rejects a 503 (resolver inactive) so self-contained links stay the fallback", async () => {
    const { publicKey } = await generateKeyPair("EdDSA", { extractable: true });
    global.fetch = vi.fn(async () => new Response(JSON.stringify({ ok: false }), { status: 503 })) as unknown as typeof fetch;
    await expect(resolveJoinCode(CODE, await localJwks(publicKey))).rejects.toThrow();
  });
});
