// HUP-S3.1 / g2-knowledge: the QA eval can answer from the bundled knowledge corpus, retrieved
// through the memory daemon's own search (`memory.search {passages: true}`). Red-first.
import { describe, it, expect } from "vitest";
import { buildRetrievalContext, parsePassages, retrievalUserMessage, searchRequestLine, parseSearchResponse } from "./retrieval";

const RENDER = [
  "freshness: (no watermark)",
  "tenant 'citrate-docs' — 1688 nodes, showing 2:",
  "  0a1b2c3d4e 0.812 [doc] Genesis › What it is",
  "    cite: citrate-docs:content/chain/genesis.md#what-it-is",
  "    > Genesis › What it is",
  "    >",
  "    > The chain id is 40204; eth_chainId returns 0x9d0c.",
  "  1f2e3d4c5b 0.640 [doc] Staking",
  "    cite: citrate-docs:content/chain/staking.md",
  "    > Staking",
  "",
].join("\n");

describe("parsePassages", () => {
  it("reads score, citation and text per hit", () => {
    expect(parsePassages(RENDER)).toEqual([
      { id: "0a1b2c3d4e", score: 0.812, cite: "citrate-docs:content/chain/genesis.md#what-it-is", text: "Genesis › What it is\n\nThe chain id is 40204; eth_chainId returns 0x9d0c." },
      { id: "1f2e3d4c5b", score: 0.64, cite: "citrate-docs:content/chain/staking.md", text: "Staking" },
    ]);
  });

  it("returns nothing for a title-only or empty rendering (an older daemon)", () => {
    expect(parsePassages("tenant 'x' — 0 nodes, showing 0:\n")).toEqual([]);
    expect(parsePassages("tenant 'x' — 3 nodes, showing 1:\n  0a1b2c3d4e 0.5 [doc] title only\n")).toEqual([]);
  });
});

describe("buildRetrievalContext", () => {
  const p = (cite: string, score: number, text: string) => ({ id: cite.slice(-10), cite, score, text });

  it("orders passages by score across tenants and numbers them with the citation to quote", () => {
    const ctx = buildRetrievalContext([[p("a:x.md#one", 0.5, "first")], [p("b:y.md", 0.9, "second")]], 10_000);
    expect(ctx.indexOf("cite as b:y.md")).toBeLessThan(ctx.indexOf("cite as a:x.md#one"));
    expect(ctx).toMatch(/^\[1\] cite as b:y\.md\nsecond/m);
  });

  it("drops duplicate passages and stops at the character budget", () => {
    const big = "z".repeat(800);
    const ctx = buildRetrievalContext([[p("a:1.md", 0.9, big), p("a:1.md", 0.9, big), p("a:2.md", 0.8, big), p("a:3.md", 0.7, big)]], 1700);
    expect((ctx.match(/cite as a:1\.md/g) ?? []).length).toBe(1);
    expect(ctx).toContain("cite as a:2.md");
    expect(ctx).not.toContain("cite as a:3.md");
  });

  it("is empty when nothing was retrieved", () => {
    expect(buildRetrievalContext([[], []], 5000)).toBe("");
  });
});

describe("retrievalUserMessage", () => {
  it("puts the excerpts before the question and says when there are none", () => {
    expect(retrievalUserMessage("What is the chain id?", "[1] cite as a:b.md\ntext")).toBe(
      "Bundled documentation excerpts:\n\n[1] cite as a:b.md\ntext\n\nQuestion: What is the chain id?",
    );
    expect(retrievalUserMessage("Q?", "")).toBe("Bundled documentation excerpts: none found.\n\nQuestion: Q?");
  });
});

describe("daemon wire", () => {
  it("builds one JSON-RPC tools/call line for memory.search with passages", () => {
    const line = searchRequestLine(7, "citrate-docs", "chain id", 5);
    expect(line.endsWith("\n")).toBe(true);
    expect(JSON.parse(line)).toEqual({
      jsonrpc: "2.0",
      id: 7,
      method: "tools/call",
      params: { name: "memory.search", arguments: { repo: "citrate-docs", query: "chain id", budget: 5, passages: true } },
    });
  });

  it("returns the tool text, and throws on a JSON-RPC or tool error", () => {
    const ok = JSON.stringify({ jsonrpc: "2.0", id: 1, result: { content: [{ type: "text", text: RENDER }], isError: false } });
    expect(parseSearchResponse(ok)).toBe(RENDER);
    expect(() => parseSearchResponse(JSON.stringify({ jsonrpc: "2.0", id: 1, error: { code: -32000, message: "boom" } }))).toThrow(/boom/);
    expect(() => parseSearchResponse(JSON.stringify({ jsonrpc: "2.0", id: 1, result: { content: [{ type: "text", text: "denied" }], isError: true } }))).toThrow(/denied/);
    expect(() => parseSearchResponse("not json")).toThrow();
  });
});

describe("memoryResultFromSearchText (the app's MemoryResult, for the eval's tool loop)", () => {
  it("reads the tenant total, ids, kinds, titles, citations and passages", async () => {
    const { memoryResultFromSearchText } = await import("./retrieval");
    expect(memoryResultFromSearchText("citrate-docs", RENDER)).toEqual({
      tenant: "citrate-docs",
      totalInTenant: 1688,
      hits: [
        {
          id: "0a1b2c3d4e",
          kind: "doc",
          title: "Genesis › What it is",
          cite: "citrate-docs:content/chain/genesis.md#what-it-is",
          passage: "Genesis › What it is\n\nThe chain id is 40204; eth_chainId returns 0x9d0c.",
        },
        { id: "1f2e3d4c5b", kind: "doc", title: "Staking", cite: "citrate-docs:content/chain/staking.md", passage: "Staking" },
      ],
    });
  });

  it("keeps a title-only hit (personal notes) without a passage", async () => {
    const { memoryResultFromSearchText } = await import("./retrieval");
    expect(memoryResultFromSearchText("personal", "tenant 'personal' — 3 nodes, showing 1:\n  0a1b2c3d4e 0.5 [Note] my note\n")).toEqual({
      tenant: "personal",
      totalInTenant: 3,
      hits: [{ id: "0a1b2c3d4e", kind: "Note", title: "my note" }],
    });
  });
});

describe("selectRetrievalPassages (what a passages run showed the model)", () => {
  it("returns the passages buildRetrievalContext numbers, in the same order", async () => {
    const { selectRetrievalPassages } = await import("./retrieval");
    const big = "z".repeat(800);
    const p = (cite: string, score: number, text: string) => ({ id: cite.slice(-6), cite, score, text });
    const per = [[p("a:1.md", 0.9, big), p("a:1.md", 0.9, big), p("a:2.md", 0.8, big), p("a:3.md", 0.7, big)]];
    expect(selectRetrievalPassages(per, 1700).map((x) => x.cite)).toEqual(["a:1.md", "a:2.md"]);
  });
});
