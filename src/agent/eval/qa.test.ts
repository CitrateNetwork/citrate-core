// @vitest-environment node
// HUP-S3.5 — Citrate QA eval set v1. Three layers:
//   1. the deterministic scorer (key points, citations, abstention) on fixtures;
//   2. the shipped dataset: 150 items, unique ids, every category, ~10 % unanswerable, and every
//      citation resolving in the committed anchor index at the pinned source commits;
//   3. live provenance: when the public source repos are checked out locally (sibling dirs of this
//      repo, or QA_SOURCES_ROOT), the anchor index is re-derived from `git show <commit>:<path>` and
//      every answer key point is found in the text of the section it cites. Skipped, with the
//      reason printed, when the repos or the pinned commits are not available (e.g. in CI).
import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import qaJson from "./qa-v1.json";
import anchorsJson from "./qa-v1.anchors.json";
import {
  QA_CATEGORIES,
  aggregateQa,
  extractAnchors,
  extractCitations,
  extractSection,
  findMissingCitations,
  keyPointCoverage,
  keyPointMatched,
  normalizeText,
  parseQaDataset,
  resolveCitationNodes,
  runQaEval,
  scoreQaItem,
  slugify,
  ungroundedKeyPoints,
  type AnchorIndex,
  type QaDataset,
  type QaItem,
} from "./qa";

const SHA = "a".repeat(40);

const fixtureIndex: AnchorIndex = {
  version: "qa-v1",
  sources: {
    "citrate-docs": {
      repo: "CitrateNetwork/citrate-docs",
      commit: SHA,
      files: { "content/chain/genesis.md": { blob: "b".repeat(40), anchors: ["what-it-is", "reference"] } },
    },
  },
};

const answerable: QaItem = {
  id: "qa-fixture-chain-id",
  question: "What is the Citrate chain id?",
  category: "chain-basics",
  difficulty: "easy",
  answerable: true,
  keyPoints: [["40204"], ["0x9d0c"]],
  citations: [{ source: "citrate-docs", path: "content/chain/genesis.md", anchor: "what-it-is" }],
};

const unanswerable: QaItem = {
  id: "qa-fixture-unans",
  question: "What is SALT trading at on Coinbase today?",
  category: "chain-basics",
  difficulty: "easy",
  answerable: false,
  keyPoints: [],
  citations: [],
};

describe("markdown anchors", () => {
  it("slugifies headings GitHub-style", () => {
    expect(slugify("Step 0: Welcome")).toBe("step-0-welcome");
    expect(slugify("GhostDAG engine, `src/ghostdag.rs`")).toBe("ghostdag-engine-srcghostdagrs");
    expect(slugify("Deterministic Q16.16 compute, `0x010A` to `0x010F`")).toBe(
      "deterministic-q1616-compute-0x010a-to-0x010f",
    );
  });
  it("skips frontmatter and fenced code, de-duplicates repeated headings", () => {
    const md = "---\ntitle: x\n---\n# A\n```\n# not a heading\n```\n## B\n## B\ntext\n";
    expect(extractAnchors(md)).toEqual(["a", "b", "b-1"]);
  });
  it("extracts a section up to the next heading of the same or higher level", () => {
    const md = "## One\nalpha\n### Sub\nbeta\n## Two\ngamma\n";
    const s = extractSection(md, "one") ?? "";
    expect(s).toContain("alpha");
    expect(s).toContain("beta");
    expect(s).not.toContain("gamma");
    expect(extractSection(md, "missing")).toBeNull();
  });
});

describe("deterministic scorer", () => {
  it("normalizes digit grouping, case, and code marks but keeps identifier underscores", () => {
    expect(normalizeText("**32,000 SALT** and `eth_call`")).toBe("32000 salt and eth_call");
    expect(keyPointCoverage("Stake 32000 salt.", [["32,000 SALT"]])).toBe(1);
  });
  it("matches key points on token boundaries, so a short number or word is not found inside a longer one", () => {
    // "18" must not be credited by "2018" or "180"; "mod" not by "model"; "3" not by "0x0103".
    expect(keyPointMatched("Shipped in 2018 with 180 peers.", ["18"])).toBe(false);
    expect(keyPointMatched("k is 18.", ["18"])).toBe(true);
    expect(keyPointMatched("The model picks one.", ["mod", "modulo"])).toBe(false);
    expect(keyPointMatched("keccak256(job) mod n", ["mod"])).toBe(true);
    expect(keyPointMatched("precompile 0x0103", ["3", "three"])).toBe(false);
    expect(keyPointMatched("Observe, orient, decide, and act.", ["Act"])).toBe(true);
    expect(keyPointMatched("It acts exactly once.", ["Act"])).toBe(true);
    expect(keyPointMatched("Exactly once.", ["Act"])).toBe(false);
    // punctuation-edged phrasings keep working
    expect(keyPointMatched("a 10% quorum", ["10%"])).toBe(true);
    expect(keyPointMatched("a 110% quorum", ["10%"])).toBe(false);
    expect(keyPointMatched("retries (3) times", ["(3)"])).toBe(true);
  });
  it("extracts source:path#anchor citations and de-duplicates them", () => {
    const refs = extractCitations(
      "See citrate-docs:content/chain/genesis.md#reference and citrate-docs:content/chain/genesis.md#reference, " +
        "plus citrate-chain:README.md.",
    );
    expect(refs).toEqual([
      { source: "citrate-docs", path: "content/chain/genesis.md", anchor: "reference" },
      { source: "citrate-chain", path: "README.md" },
    ]);
  });
  it("passes a covered, correctly cited answer", () => {
    const s = scoreQaItem(
      answerable,
      "Chain id 40204 (0x9d0c). [citrate-docs:content/chain/genesis.md#what-it-is]",
      fixtureIndex,
    );
    expect(s).toMatchObject({ coverage: 1, citationHit: true, citedInvalid: 0, pass: true });
  });
  it("fails a right answer with no citation, and a cited answer that misses key points", () => {
    expect(scoreQaItem(answerable, "It is 40204, hex 0x9d0c.", fixtureIndex).pass).toBe(false);
    const thin = scoreQaItem(answerable, "Not sure. citrate-docs:content/chain/genesis.md#what-it-is", fixtureIndex);
    expect(thin.coverage).toBe(0);
    expect(thin.pass).toBe(false);
  });
  it("counts a citation to a heading that does not exist as invalid and fails the item", () => {
    const s = scoreQaItem(
      answerable,
      "40204 / 0x9d0c citrate-docs:content/chain/genesis.md#what-it-is citrate-docs:content/chain/genesis.md#chain-id",
      fixtureIndex,
    );
    expect(s.citedInvalid).toBe(1);
    expect(s.pass).toBe(false);
    const wrongFile = scoreQaItem(answerable, "40204 0x9d0c citrate-docs:content/chain/nope.md#what-it-is", fixtureIndex);
    expect(wrongFile.citationHit).toBe(false);
  });
  it("accepts structured citations returned out-of-band", () => {
    const s = scoreQaItem(answerable, "40204, i.e. 0x9d0c", fixtureIndex, [
      { source: "citrate-docs", path: "content/chain/genesis.md", anchor: "reference" },
    ]);
    expect(s.pass).toBe(true);
  });
  it("passes an unanswerable item only on abstention without invalid citations", () => {
    expect(scoreQaItem(unanswerable, "That is not documented in the Citrate docs.", fixtureIndex).pass).toBe(true);
    expect(scoreQaItem(unanswerable, "SALT trades at $1.20.", fixtureIndex).pass).toBe(false);
    expect(
      scoreQaItem(unanswerable, "Not documented, see citrate-docs:content/chain/price.md#usd", fixtureIndex).pass,
    ).toBe(false);
  });
  it("credits the 'the documentation does not cover X' abstention the QA prompt asks for", () => {
    // Real answers from the 2026-10-01 Gemma 4 E4B run (eval/results): each is a plain statement
    // that the docs do not cover the topic, which is what QA_SYSTEM_PROMPT instructs. The shared
    // W3.4 refusal markers miss every one of them, so all 15 unanswerable probes scored as failures.
    for (const a of [
      "The Citrate documentation does not contain information regarding the current trading price of SALT on Coinbase.",
      "The documentation does not specify the exact block height at which the Citrate testnet will switch over to mainnet.",
      "The documentation does not cover the memory retention policy for Memrizz.",
      "The bundled Citrate documentation does not specify a recommended AWS EC2 instance type for running a validator.",
      "I do not have documentation detailing the specific type of credential a Citrate passkey is.",
      "The docs don't mention a Hardhat plugin.",
    ]) {
      const s = scoreQaItem(unanswerable, a, fixtureIndex);
      expect(s.abstained, a).toBe(true);
      expect(s.pass, a).toBe(true);
    }
    // Statements about what the documentation DOES say are not abstentions.
    for (const a of [
      "The documentation specifies chain id 40204.",
      "The docs cover this: SALT has 18 decimals.",
      "The documentation does contain the genesis parameters.",
      "Validators do not need documentation to stake.",
      // The match stays inside one sentence: a docs noun in one sentence and a negated verb in the next is not an abstention.
      "The documentation lists two RPC ports. Clients do not specify a port when they use the default.",
    ]) {
      expect(scoreQaItem(unanswerable, a, fixtureIndex).abstained, a).toBe(false);
    }
  });
  it("aggregates rates over the right denominators", () => {
    const a = scoreQaItem(answerable, "40204 0x9d0c citrate-docs:content/chain/genesis.md#reference", fixtureIndex);
    const b = scoreQaItem(unanswerable, "I don't know; it is not documented.", fixtureIndex);
    const c = scoreQaItem({ ...answerable, id: "qa-fixture-2" }, "I could not find that.", fixtureIndex);
    const card = aggregateQa([a, b, c], { datasetVersion: "qa-v1", model: "m", startedAt: "2026-09-30T00:00:00Z" });
    expect(card.n).toBe(3);
    expect(card.passRate).toBeCloseTo(2 / 3);
    expect(card.citationHitRate).toBeCloseTo(1 / 2);
    expect(card.abstentionRate).toBe(1);
    expect(card.falseAbstentionRate).toBeCloseTo(1 / 2);
    expect(card.citationValidity).toBe(1);
    expect(card.failures).toEqual(["qa-fixture-2"]);
    expect(card.byCategory["chain-basics"]).toEqual({ n: 3, passRate: 2 / 3 });
  });
  it("runs a set through an injected ask and aborts (no scorecard) on a transport error", async () => {
    const ds = {
      version: "qa-v1",
      provenance: { author: "a", created: "2026-09-30", purpose: "p", disjointFromTraining: true },
      sources: { "citrate-docs": { repo: "CitrateNetwork/citrate-docs", commit: SHA, visibility: "public" } },
      items: [answerable, unanswerable],
    } as QaDataset;
    const seen: string[] = [];
    const out = await runQaEval(
      ds,
      fixtureIndex,
      {
        ask: async (q) => ({
          text: q.includes("chain id") ? "40204 0x9d0c citrate-docs:content/chain/genesis.md#reference" : "Not documented.",
        }),
        onProgress: (id) => seen.push(id),
      },
      { model: "fixture" },
    );
    expect(out.scorecard.passRate).toBe(1);
    expect(seen).toEqual([answerable.id, unanswerable.id]);
    await expect(
      runQaEval(ds, fixtureIndex, { ask: async () => Promise.reject(new Error("ECONNREFUSED")) }, { model: "x" }),
    ).rejects.toThrow(/ECONNREFUSED/);
  });
});

describe("citations resolve to node ids of the imported graph (g2-knowledge b)", () => {
  const retrieved = [
    { id: "0a1b2c3d4e", cite: "citrate-docs:content/chain/genesis.md#reference" },
    { id: "1f2e3d4c5b", cite: "citrate-docs:content/chain/genesis.md#what-it-is" },
    { id: "2a2a2a2a2a", cite: "citrate-docs:content/chain/staking.md" },
  ];
  it("maps each answer citation to the retrieved nodes it names (file, and section when given)", () => {
    const answer =
      "40204 (citrate-docs:content/chain/genesis.md#reference), see citrate-docs:content/chain/genesis.md and " +
      "citrate-docs:content/chain/unknown.md#x";
    expect(resolveCitationNodes(answer, retrieved)).toEqual([
      { citation: "citrate-docs:content/chain/genesis.md#reference", nodeIds: ["0a1b2c3d4e"] },
      { citation: "citrate-docs:content/chain/genesis.md", nodeIds: ["0a1b2c3d4e", "1f2e3d4c5b"] },
      { citation: "citrate-docs:content/chain/unknown.md#x", nodeIds: [] },
    ]);
  });

  it("stamps per-item node ids and the run's node-citation rate when the provider reports retrieval", async () => {
    const ds = {
      version: "qa-v1",
      provenance: { author: "a", created: "2026-09-30", purpose: "p", disjointFromTraining: true },
      sources: { "citrate-docs": { repo: "CitrateNetwork/citrate-docs", commit: SHA, visibility: "public" } },
      items: [answerable, unanswerable],
    } as QaDataset;
    const out = await runQaEval(
      ds,
      fixtureIndex,
      {
        ask: async (q) =>
          q.includes("chain id")
            ? {
                text: "40204 0x9d0c citrate-docs:content/chain/genesis.md#reference citrate-docs:content/chain/genesis.md#tokenomics",
                retrieved: retrieved.slice(0, 1),
                toolCalls: [{ tenant: "citrate-docs", query: "chain id" }],
              }
            : { text: "Not documented.", retrieved: [], toolCalls: [] },
      },
      { model: "fixture" },
    );
    expect(out.items[0].retrievedNodes).toEqual(["0a1b2c3d4e"]);
    expect(out.items[0].toolCalls).toEqual([{ tenant: "citrate-docs", query: "chain id" }]);
    expect(out.items[0].citedNodes).toEqual([
      { citation: "citrate-docs:content/chain/genesis.md#reference", nodeIds: ["0a1b2c3d4e"] },
      { citation: "citrate-docs:content/chain/genesis.md#tokenomics", nodeIds: [] },
    ]);
    expect(out.scorecard.citationNodeRate).toBe(0.5);
  });

  it("leaves the node-citation rate off a closed-book run", async () => {
    const ds = {
      version: "qa-v1",
      provenance: { author: "a", created: "2026-09-30", purpose: "p", disjointFromTraining: true },
      sources: { "citrate-docs": { repo: "CitrateNetwork/citrate-docs", commit: SHA, visibility: "public" } },
      items: [answerable],
    } as QaDataset;
    const out = await runQaEval(ds, fixtureIndex, { ask: async () => ({ text: "40204" }) }, { model: "fixture" });
    expect("citationNodeRate" in out.scorecard).toBe(false);
    expect("citedNodes" in out.items[0]).toBe(false);
  });
});

describe("dataset validator rejects malformed data", () => {
  const good = {
    version: "qa-v1",
    provenance: { author: "a", created: "2026-09-30", purpose: "p", disjointFromTraining: true },
    sources: { "citrate-docs": { repo: "CitrateNetwork/citrate-docs", commit: SHA, visibility: "public" } },
    items: [answerable],
  };
  it("accepts a well-formed dataset", () => {
    expect(parseQaDataset(good).items).toHaveLength(1);
  });
  it("rejects duplicate ids, unknown categories, and a non-slug anchor", () => {
    expect(() => parseQaDataset({ ...good, items: [answerable, answerable] })).toThrow(/duplicate/);
    expect(() => parseQaDataset({ ...good, items: [{ ...answerable, category: "misc" }] })).toThrow(/category/);
    const badAnchor = { ...answerable, citations: [{ ...answerable.citations[0], anchor: "What It Is" }] };
    expect(() => parseQaDataset({ ...good, items: [badAnchor] })).toThrow(/slug/);
  });
  it("rejects an answerable item without citations and an unanswerable item with key points", () => {
    expect(() => parseQaDataset({ ...good, items: [{ ...answerable, citations: [] }] })).toThrow(/citation/);
    expect(() => parseQaDataset({ ...good, items: [{ ...unanswerable, keyPoints: [["x"]] }] })).toThrow(/unanswerable/);
  });
  it("rejects a short commit, a non-public source, and a missing disjointness flag", () => {
    const short = { ...good, sources: { "citrate-docs": { ...good.sources["citrate-docs"], commit: "abc123" } } };
    expect(() => parseQaDataset(short)).toThrow(/40-hex/);
    const priv = { ...good, sources: { "citrate-docs": { ...good.sources["citrate-docs"], visibility: "private" } } };
    expect(() => parseQaDataset(priv)).toThrow(/public/);
    expect(() =>
      parseQaDataset({ ...good, provenance: { ...good.provenance, disjointFromTraining: false } }),
    ).toThrow(/disjoint/);
  });
  it("findMissingCitations reports an unknown heading and a commit mismatch", () => {
    const ds = parseQaDataset({
      ...good,
      items: [{ ...answerable, citations: [{ ...answerable.citations[0], anchor: "chain-id" }] }],
    });
    expect(findMissingCitations(ds, fixtureIndex)).toEqual([
      "qa-fixture-chain-id: citrate-docs:content/chain/genesis.md#chain-id has no such heading",
    ]);
    const moved = { ...fixtureIndex, sources: { "citrate-docs": { ...fixtureIndex.sources["citrate-docs"], commit: "c".repeat(40) } } };
    expect(findMissingCitations(parseQaDataset(good), moved)[0]).toMatch(/index commit/);
  });
});

describe("qa-v1.json", () => {
  const ds = parseQaDataset(qaJson);
  const index = anchorsJson as AnchorIndex;

  it("is versioned, carries provenance, and pins public sources by full commit", () => {
    expect(ds.version).toBe("qa-v1");
    expect(ds.provenance.author).toMatch(/Larry Klosowski/);
    expect(Object.keys(ds.sources).length).toBeGreaterThanOrEqual(2);
    for (const s of Object.values(ds.sources)) expect(s.repo).toMatch(/^CitrateNetwork\//);
  });
  it("has exactly 150 items with unique ids", () => {
    expect(ds.items).toHaveLength(150);
    expect(new Set(ds.items.map((i) => i.id)).size).toBe(150);
  });
  it("covers all ten categories, and every difficulty", () => {
    for (const c of QA_CATEGORIES) expect(ds.items.filter((i) => i.category === c).length).toBeGreaterThanOrEqual(10);
    for (const d of ["easy", "medium", "hard"]) expect(ds.items.some((i) => i.difficulty === d)).toBe(true);
  });
  it("has about 10 % unanswerable items (hallucination probes)", () => {
    const n = ds.items.filter((i) => !i.answerable).length;
    expect(n).toBeGreaterThanOrEqual(13);
    expect(n).toBeLessThanOrEqual(17);
  });
  it("every citation path + anchor exists in the anchor index at the pinned commit", () => {
    expect(findMissingCitations(ds, index)).toEqual([]);
  });
  it("the anchor index lists only files the dataset cites (no stale entries)", () => {
    const cited = new Set(ds.items.flatMap((i) => i.citations.map((c) => `${c.source}:${c.path}`)));
    const indexed = Object.entries(index.sources).flatMap(([s, v]) => Object.keys(v.files).map((p) => `${s}:${p}`));
    expect(indexed.filter((k) => !cited.has(k))).toEqual([]);
  });
  it("never cites private or security material", () => {
    for (const i of ds.items) for (const c of i.citations) expect(c.source).not.toMatch(/security|agentile-archive/);
  });
});

// ---------------------------------------------------------------- live provenance (local repos)

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const sourcesRoot = process.env.QA_SOURCES_ROOT ?? resolve(repoRoot, "..");

function git(dir: string, args: string[]): string | null {
  try {
    return execFileSync("git", ["-C", dir, ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], maxBuffer: 1 << 26 });
  } catch {
    return null;
  }
}

const ds = parseQaDataset(qaJson);
const unavailable = Object.entries(ds.sources)
  .filter(([name, s]) => {
    const dir = join(sourcesRoot, name);
    return !existsSync(dir) || git(dir, ["cat-file", "-e", `${s.commit}^{commit}`]) === null;
  })
  .map(([name]) => name);
const live = unavailable.length === 0;
if (!live) {
  console.info(
    `qa-v1 live provenance skipped: source repo(s) ${unavailable.join(", ")} not found at the pinned commit under ` +
      `${sourcesRoot} (set QA_SOURCES_ROOT). The committed anchor index is still checked above.`,
  );
}

describe.skipIf(!live)("qa-v1 live provenance against the pinned source commits", () => {
  const index = anchorsJson as AnchorIndex;
  const cache = new Map<string, string>();
  const show = (source: string, path: string): string => {
    const key = `${source}:${path}`;
    const hit = cache.get(key);
    if (hit !== undefined) return hit;
    const text = git(join(sourcesRoot, source), ["show", `${ds.sources[source].commit}:${path}`]) ?? "";
    cache.set(key, text);
    return text;
  };

  it("the committed anchor index matches the files at the pinned commits (blob + anchors)", () => {
    for (const [source, v] of Object.entries(index.sources)) {
      for (const [path, f] of Object.entries(v.files)) {
        const blob = (git(join(sourcesRoot, source), ["rev-parse", `${v.commit}:${path}`]) ?? "").trim();
        expect(`${source}:${path} ${blob}`).toBe(`${source}:${path} ${f.blob}`);
        expect(extractAnchors(show(source, path))).toEqual(f.anchors);
      }
    }
    // 44 `git show` + `git rev-parse` pairs: past the 5 s default under a full parallel run.
  }, 60_000);
  it("every key point of every answerable item appears in the text of the section(s) it cites", () => {
    const problems: string[] = [];
    for (const it of ds.items.filter((i) => i.answerable)) {
      const text = it.citations.map((c) => extractSection(show(c.source, c.path), c.anchor) ?? "").join("\n");
      const missing = ungroundedKeyPoints(it, text);
      if (missing.length) problems.push(`${it.id}: ${missing.map((k) => k.join("|")).join("; ")}`);
    }
    expect(problems).toEqual([]);
  }, 60_000);
});
