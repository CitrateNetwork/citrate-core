// @vitest-environment node
// HUP-S7.7 / US-9.2 AC1 — qa-literacy-v2: the 30 qa-literacy-v1 items unchanged, plus the items v1
// was missing for US-9.2 AC1 ("explains FOUR, the knowledge/truth orders, the classifier, and the
// aggregation, with citations"). This file checks:
//   1. v2 is v1 plus new items: every v1 item byte-for-byte equal and in the same order;
//   2. shape: 41 items, unique qa-lit- ids, none shared with qa-v1, 4 unanswerable probes;
//   3. coverage: each US-9.2 AC1 topic has answerable, cited items (the map below);
//   4. citations resolve in the committed anchor index, which lists only cited files;
//   5. live provenance when the source repos are checked out (QA_SOURCES_ROOT or sibling dirs).
import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import v1Json from "./qa-literacy-v1.json";
import v2Json from "./qa-literacy-v2.json";
import v2AnchorsJson from "./qa-literacy-v2.anchors.json";
import qaV1Json from "./qa-v1.json";
import {
  extractAnchors,
  extractSection,
  findMissingCitations,
  parseQaDataset,
  ungroundedKeyPoints,
  type AnchorIndex,
} from "./qa";

const LIVE_TIMEOUT_MS = 60_000;
const v1 = parseQaDataset(v1Json);
const v2 = parseQaDataset(v2Json);
const index = v2AnchorsJson as AnchorIndex;

/** US-9.2 AC1 topic -> the v2 items that score it. Each must be answerable and cited. */
const AC1_COVERAGE: Record<string, string[]> = {
  "FOUR (the four values)": ["qa-lit-four-both-neither", "qa-lit-four-true-false", "qa-lit-mean-collapses"],
  "knowledge order": ["qa-lit-knowledge-order"],
  "truth order": ["qa-lit-truth-order"],
  classifier: ["qa-lit-classifier-rules", "qa-lit-trust-weights"],
  "aggregation (off-chain)": ["qa-lit-aggregation-steps", "qa-lit-dual-output", "qa-lit-reduction", "qa-lit-router-contested"],
  "aggregation (0x0110)": ["qa-lit-0110-why-integer", "qa-lit-0110-inputs", "qa-lit-output-size"],
};

describe("qa-literacy-v2.json", () => {
  it("is versioned and keeps the v1 provenance rules", () => {
    expect(v2.version).toBe("qa-literacy-v2");
    expect(v2.provenance.author).toMatch(/Larry Klosowski/);
    expect(v2.provenance.disjointFromTraining).toBe(true);
    expect(v2.sources).toEqual(v1.sources);
    for (const s of Object.values(v2.sources)) expect(s.visibility).toBe("public");
  });
  it("starts with every v1 item unchanged, in v1 order", () => {
    expect(v2.items.slice(0, v1.items.length)).toEqual(v1.items);
  });
  it("has 41 items with unique qa-lit- ids, none shared with qa-v1, and 4 unanswerable probes", () => {
    expect(v2.items).toHaveLength(41);
    const ids = v2.items.map((i) => i.id);
    expect(new Set(ids).size).toBe(41);
    for (const id of ids) expect(id).toMatch(/^qa-lit-/);
    const qa = new Set(parseQaDataset(qaV1Json).items.map((i) => i.id));
    expect(ids.filter((id) => qa.has(id))).toEqual([]);
    expect(v2.items.filter((i) => !i.answerable)).toHaveLength(4);
  });
  it("covers every US-9.2 AC1 topic with answerable, cited items", () => {
    const byId = new Map(v2.items.map((i) => [i.id, i]));
    for (const [topic, ids] of Object.entries(AC1_COVERAGE)) {
      expect(ids.length, topic).toBeGreaterThan(0);
      for (const id of ids) {
        const item = byId.get(id);
        expect(item, `${topic}: ${id}`).toBeDefined();
        expect(item?.answerable, id).toBe(true);
        expect(item?.citations.length, id).toBeGreaterThan(0);
      }
    }
  });
  it("every new item is a paraconsensus item cited to the public docs", () => {
    for (const i of v2.items.slice(v1.items.length)) {
      expect(i.category, i.id).toBe("paraconsensus-fl");
      for (const c of i.citations) expect(c.source, i.id).toBe("citrate-docs");
    }
  });
  it("every citation resolves in the anchor index, which lists only cited files", () => {
    expect(findMissingCitations(v2, index)).toEqual([]);
    expect(index.version).toBe(v2.version);
    const cited = new Set(v2.items.flatMap((i) => i.citations.map((c) => `${c.source}:${c.path}`)));
    const indexed = Object.entries(index.sources).flatMap(([s, v]) => Object.keys(v.files).map((p) => `${s}:${p}`));
    expect(indexed.filter((k) => !cited.has(k))).toEqual([]);
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

const unavailable = Object.entries(v2.sources)
  .filter(([name, s]) => {
    const dir = join(sourcesRoot, name);
    return !existsSync(dir) || git(dir, ["cat-file", "-e", `${s.commit}^{commit}`]) === null;
  })
  .map(([name]) => name);
const live = unavailable.length === 0;
if (!live) {
  console.info(
    `qa-literacy-v2 live provenance skipped: source repo(s) ${unavailable.join(", ")} not found at the pinned commit ` +
      `under ${sourcesRoot} (set QA_SOURCES_ROOT). The committed anchor index is still checked above.`,
  );
}

describe.skipIf(!live)("qa-literacy-v2 live provenance against the pinned source commits", () => {
  const show = (source: string, path: string): string =>
    git(join(sourcesRoot, source), ["show", `${v2.sources[source].commit}:${path}`]) ?? "";

  it(
    "the committed anchor index matches the files at the pinned commits (blob + anchors)",
    () => {
      for (const [source, v] of Object.entries(index.sources)) {
        for (const [path, f] of Object.entries(v.files)) {
          const blob = (git(join(sourcesRoot, source), ["rev-parse", `${v.commit}:${path}`]) ?? "").trim();
          expect(`${source}:${path} ${blob}`).toBe(`${source}:${path} ${f.blob}`);
          expect(extractAnchors(show(source, path))).toEqual(f.anchors);
        }
      }
    },
    LIVE_TIMEOUT_MS,
  );
  it(
    "every key point of every answerable item appears in the text of the section(s) it cites",
    () => {
      const problems: string[] = [];
      for (const it of v2.items.filter((i) => i.answerable)) {
        const text = it.citations.map((c) => extractSection(show(c.source, c.path), c.anchor) ?? "").join("\n");
        const missing = ungroundedKeyPoints(it, text);
        if (missing.length) problems.push(`${it.id}: ${missing.map((k) => k.join("|")).join("; ")}`);
      }
      expect(problems).toEqual([]);
    },
    LIVE_TIMEOUT_MS,
  );
});
