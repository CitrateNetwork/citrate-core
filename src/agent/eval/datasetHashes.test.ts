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
import { FROZEN_V1_SHA256 } from "./frozenPins.ts";

describe("v1 eval datasets are frozen (A50)", () => {
  for (const [file, sha] of Object.entries(FROZEN_V1_SHA256)) {
    it(`${file} has the pinned sha256`, () => {
      const bytes = readFileSync(resolve(process.cwd(), "src/agent/eval", file));
      expect(createHash("sha256").update(bytes).digest("hex")).toBe(sha);
    });
  }
});
