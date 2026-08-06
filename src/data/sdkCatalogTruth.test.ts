// ─────────────────────────────────────────────────────────────────────────────
// The Commissary's SDK rows must name packages that EXIST and are OURS.
//
// The app ships its own copy of the catalog (`seed.ts`) rather than reading the
// signed manifest, and it drifted: it told members to run
//
//   npm install @citrate/sdk                          <- NOT OURS, and it SUCCEEDS
//   pip install citrate-ai                            <- 404
//   npm install @citrate/marketplace --registry=ghcr  <- 404, and ours is on npm
//
// The first is the dangerous one: `@citrate/sdk` is owned by `cnidarian-foundation`
// and its 0.5.1 (Apr 2026) outranks our real 0.2.0, so a member comparing versions
// installs a stranger's package. Ownership verified 2026-08-02 via `npm owner ls`
// (core-membership `superseded.ts`).
//
// These assertions are the canonical strings from core-membership
// `src/lib/commissary/catalog.ts`, copied verbatim. If that file changes, this
// fails — which is the point: the copy must not drift again.
// ─────────────────────────────────────────────────────────────────────────────
import { describe, it, expect } from "vitest";
import { CATALOG } from "./seed";

const CANONICAL: Record<string, string> = {
  "citrate-sdk-ts": "npm i @citratelabs/sdk@0.2.0",
  "citrate-sdk-py": "pip install citrate-labs-sdk==0.6.0",
  "citrate-sdk-marketplace": "npm i @citratelabs/marketplace-sdk@0.1.0",
};

describe("Commissary SDK rows mirror the canonical catalog", () => {
  it("every SDK id and install command matches core-membership verbatim", () => {
    const got = Object.fromEntries(CATALOG.sdks.map((s) => [s.id, s.install]));
    expect(got).toEqual(CANONICAL);
  });

  it("never names a scope or package that is not ours", () => {
    const forbidden = [
      "@citrate/sdk", // cnidarian-foundation's, not ours
      "@citrate/marketplace", // does not exist
      "citrate-ai", // does not exist on PyPI
    ];
    for (const s of CATALOG.sdks) {
      for (const bad of forbidden) {
        expect(s.install).not.toContain(bad);
      }
    }
  });

  it("pins an exact version — a floating install stops matching the published integrity hash", () => {
    for (const s of CATALOG.sdks) {
      expect(s.install).toMatch(/(@\d+\.\d+\.\d+|==\d+\.\d+\.\d+)/);
    }
  });

  it("does not send members to a registry we do not publish to", () => {
    for (const s of CATALOG.sdks) {
      expect(s.install).not.toContain("--registry=ghcr");
      expect(["npm", "PyPI"]).toContain(s.registry);
    }
  });
});
