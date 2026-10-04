// HUP-S3.1 / US-3.1 AC2: in a retrieval run a citation is valid when it resolves to a node of the
// bundled corpus (repo:path, and the anchor of one of that file's sections). Red-first.
import { describe, it, expect } from "vitest";
import { buildCorpusCitationIndex } from "./corpusCitations";
import { citationValid, scoreQaItem, type AnchorIndex, type QaItem } from "./qa";

const tenant = (nodes: { repo: string; path: string; content: string }[]) =>
  JSON.stringify({
    repo: "citrate-docs",
    exported_at_ms: 1,
    nodes: nodes.map((n) => ({ source_ref: { Artifact: { repo: n.repo, path: n.path, git_sha: "x", byte_start: 0, byte_end: 1 } }, content: n.content })),
    edges: [],
  });

const corpus = buildCorpusCitationIndex([
  tenant([
    { repo: "citrate-docs", path: "content/chain/tutorials/call-citrate-rpc.md", content: "Call the Citrate RPC" },
    { repo: "citrate-docs", path: "content/chain/tutorials/call-citrate-rpc.md", content: "Call the Citrate RPC › Step 1, confirm you are on Citrate\n\nbody" },
    { repo: "citrate-docs", path: "content/chain/genesis.md", content: "Genesis › What it is\n\nchain id 40204" },
  ]),
]);

const emptyIndex: AnchorIndex = { version: "qa-v1", sources: {} };

describe("buildCorpusCitationIndex", () => {
  it("indexes every bundled file with the anchors of its sections", () => {
    expect(corpus.files.get("citrate-docs:content/chain/tutorials/call-citrate-rpc.md")).toEqual(new Set(["step-1-confirm-you-are-on-citrate"]));
    expect(corpus.files.get("citrate-docs:content/chain/genesis.md")).toEqual(new Set(["what-it-is"]));
  });

  it("ignores nodes that are not file passages", () => {
    const idx = buildCorpusCitationIndex([JSON.stringify({ repo: "x", exported_at_ms: 1, nodes: [{ source_ref: { DagNative: { key: "k" } }, content: "a" }], edges: [] })]);
    expect(idx.files.size).toBe(0);
  });
});

describe("citationValid with the corpus", () => {
  it("accepts a bundled file, with or without one of its section anchors", () => {
    expect(citationValid({ source: "citrate-docs", path: "content/chain/tutorials/call-citrate-rpc.md" }, emptyIndex, corpus)).toBe(true);
    expect(citationValid({ source: "citrate-docs", path: "content/chain/tutorials/call-citrate-rpc.md", anchor: "step-1-confirm-you-are-on-citrate" }, emptyIndex, corpus)).toBe(true);
  });
  it("refuses an unknown file or an anchor the file does not have", () => {
    expect(citationValid({ source: "citrate-docs", path: "content/nope.md" }, emptyIndex, corpus)).toBe(false);
    expect(citationValid({ source: "citrate-docs", path: "content/chain/genesis.md", anchor: "made-up" }, emptyIndex, corpus)).toBe(false);
    // Without the corpus (a closed-book run) only the anchor index counts.
    expect(citationValid({ source: "citrate-docs", path: "content/chain/genesis.md" }, emptyIndex)).toBe(false);
  });
});

describe("scoreQaItem with the corpus", () => {
  const item: QaItem = {
    id: "qa-chain-chain-id",
    question: "What is the chain id?",
    category: "chain-basics",
    difficulty: "easy",
    answerable: true,
    keyPoints: [["40204"], ["0x9d0c"]],
    citations: [{ source: "citrate-docs", path: "content/chain/genesis.md", anchor: "what-it-is" }],
  };
  const answer =
    "The chain id is 40204; eth_chainId returns 0x9d0c (citrate-docs:content/chain/genesis.md#what-it-is, " +
    "citrate-docs:content/chain/tutorials/call-citrate-rpc.md#step-1-confirm-you-are-on-citrate).";

  it("passes when every citation resolves to a bundled node", () => {
    const s = scoreQaItem(item, answer, emptyIndex, [], { corpus });
    expect(s.citedInvalid).toBe(0);
    expect(s.citationHit).toBe(true);
    expect(s.pass).toBe(true);
  });
  it("without the corpus the same answer has unresolved citations", () => {
    expect(scoreQaItem(item, answer, emptyIndex).pass).toBe(false);
  });
});
