// HUP-S3.1: Hermes answers from the bundled knowledge corpus. The memory_search tool picks the
// tenant, asks the daemon for passages on knowledge tenants, and hands the model each passage with
// the citation it must quote. Red-first.
import { describe, expect, it } from "vitest";
import { KNOWLEDGE_TENANTS, formatMemoryHits, memorySearchTarget } from "./knowledgeSearch";
import type { MemoryResult } from "../bridge/domains";

describe("memorySearchTarget", () => {
  it("routes knowledge tenants to passages and personal notes to titles", () => {
    expect(KNOWLEDGE_TENANTS).toEqual(["citrate-docs", "methodology", "refs", "skills"]);
    for (const t of KNOWLEDGE_TENANTS) expect(memorySearchTarget(t)).toEqual({ tenant: t, passages: true });
    expect(memorySearchTarget("personal")).toEqual({ tenant: "personal", passages: false });
  });

  it("defaults anything else to the Citrate docs", () => {
    for (const t of [undefined, "", "chain-state", "federation", 7, "CITRATE-DOCS"]) {
      expect(memorySearchTarget(t)).toEqual({ tenant: "citrate-docs", passages: true });
    }
  });
});

describe("formatMemoryHits", () => {
  const base: MemoryResult = { tenant: "citrate-docs", totalInTenant: 1688, hits: [] };

  it("is honest about an empty result", () => {
    expect(formatMemoryHits(base)).toMatch(/No results in the citrate-docs memory \(1688 nodes total\)/);
  });

  it("gives each passage with the citation to quote", () => {
    const out = formatMemoryHits({
      ...base,
      hits: [
        {
          id: "0a1b2c3d4e",
          kind: "doc",
          title: "Genesis › What it is",
          cite: "citrate-docs:content/chain/genesis.md#what-it-is",
          passage: "Genesis › What it is\n\nThe chain id is 40204.",
        },
        { id: "1f2e3d4c5b", kind: "doc", title: "Staking" },
      ],
    });
    expect(out).toContain("[1] cite as citrate-docs:content/chain/genesis.md#what-it-is");
    expect(out).toContain("The chain id is 40204.");
    // A hit without a passage keeps the title-only line.
    expect(out).toContain("[2] Staking");
  });

  it("keeps the title-only rendering for results without passages", () => {
    const out = formatMemoryHits({ ...base, hits: [{ id: "a", kind: "Rationale", title: "prefers reduced telemetry" }] });
    expect(out).toBe("[1] prefers reduced telemetry");
  });
});
