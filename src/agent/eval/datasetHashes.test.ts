// @vitest-environment node
//
// A50 — v1 eval datasets are FROZEN at the exact bytes every v1 scorecard was produced on
// (toolcall-v1: 57 tasks, injection-v1: 23 cases; commit 8e28ed3, HUP-S1.7/S1.10). They were once
// appended to in place, which made later runs incomparable (n=80 scorecards vs an 88-item set).
// This pin makes that impossible: changing a v1 file fails here. Add items as a new fragment file
// under toolcall-v2.d/ or injection-v2.d/ instead (src/agent/eval/fragments.ts).
import { describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const V1_SHA256: Record<string, string> = {
  "toolcall-v1.json": "0bb4205ef14cd1b1701cf7b2e0392e15c858a8bf9b020ab8c2bc49ebbdbb0bdf",
  "injection-v1.json": "9c2148ade8e50d106c4c8b14c94522d43fed1931390c5dfb45f43d9f9ff8f1e2",
  // HUP-S7.7 / US-9.2 AC1 (2026-10-04): frozen once qa-literacy-v2 added the missing paraconsensus items.
  "qa-literacy-v1.json": "f21c1df28df6983a72f5862b1299334271ce7577c94efa786590178d289294d6",
};

describe("v1 eval datasets are frozen (A50)", () => {
  for (const [file, sha] of Object.entries(V1_SHA256)) {
    it(`${file} has the pinned sha256`, () => {
      const bytes = readFileSync(resolve(process.cwd(), "src/agent/eval", file));
      expect(createHash("sha256").update(bytes).digest("hex")).toBe(sha);
    });
  }
});
