// The federation link builders turn catalog values (bare host / atlas-path /
// absolute) into the exact https URL the shell opens. Getting these wrong sends
// a user to the wrong place, so the mapping is pinned here.
import { describe, it, expect } from "vitest";
import { federationUrl, scanTxUrl, scanAddrUrl, ATLAS_BASE, SCAN_BASE } from "./links";

describe("federationUrl", () => {
  it("prefixes a bare host with https", () => {
    expect(federationUrl("explorer.citrate.ai")).toBe("https://explorer.citrate.ai");
    expect(federationUrl("dataroom.citrate.ai")).toBe("https://dataroom.citrate.ai");
  });
  it("maps an atlas/ path onto the Atlas base", () => {
    expect(federationUrl("atlas/tutorials/first-validator")).toBe(`${ATLAS_BASE}/tutorials/first-validator`);
    expect(federationUrl("atlas/sdk/js")).toBe(`${ATLAS_BASE}/sdk/js`);
    expect(federationUrl("atlas/docs/d1")).toBe(`${ATLAS_BASE}/docs/d1`);
  });
  it("passes an already-absolute https URL through unchanged", () => {
    expect(federationUrl("https://explorer.citrate.ai/tx/0xabc")).toBe("https://explorer.citrate.ai/tx/0xabc");
  });
  it("only ever produces https URLs (the shell rejects non-https)", () => {
    for (const v of ["explorer.citrate.ai", "atlas/docs/d1", "https://x.citrate.ai/y"]) {
      expect(federationUrl(v).startsWith("https://")).toBe(true);
    }
  });
});

describe("scanTxUrl / scanAddrUrl", () => {
  it("builds CitrateScan tx + address links", () => {
    expect(scanTxUrl("0xdeadbeef")).toBe(`${SCAN_BASE}/tx/0xdeadbeef`);
    expect(scanAddrUrl("0xbb3a5102b647606Da86A85BD6B6BA24d01fD138b")).toBe(
      `${SCAN_BASE}/address/0xbb3a5102b647606Da86A85BD6B6BA24d01fD138b`,
    );
  });
});
