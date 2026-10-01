// @vitest-environment node
// HUP-S7.7 — the paraconsensus + precompile literacy QA set (qa-literacy-v1). Same format, scorer
// and provenance rules as qa-v1 (HUP-S3.5); this file checks the literacy set itself:
//   1. the validator accepts a named pack version (`qa-<pack>-vN`) and still refuses junk;
//   2. the shipped set: 30 items, ids disjoint from qa-v1, about 10 % unanswerable, public sources
//      pinned by full commit, every citation resolving in its committed anchor index;
//   3. live provenance when the source repos are checked out (QA_SOURCES_ROOT or sibling dirs):
//      blob ids and headings re-derived from `git show`, and every key point found in the section
//      it cites. Skipped, with the reason printed, when the repos are not available (e.g. in CI).
import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import litJson from "./qa-literacy-v1.json";
import litAnchorsJson from "./qa-literacy-v1.anchors.json";
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

describe("dataset version names", () => {
  const base = parseQaDataset(qaV1Json);
  const withVersion = (version: string) => ({ ...base, version, items: base.items.slice(0, 1) });

  it("accepts qa-vN and a named pack qa-<pack>-vN", () => {
    expect(parseQaDataset(withVersion("qa-v1")).version).toBe("qa-v1");
    expect(parseQaDataset(withVersion("qa-literacy-v1")).version).toBe("qa-literacy-v1");
    expect(parseQaDataset(withVersion("qa-chain-lit-v12")).version).toBe("qa-chain-lit-v12");
  });
  it("rejects malformed versions", () => {
    for (const v of ["literacy-v1", "qa-Literacy-v1", "qa--v1", "qa-literacy-", "qa-literacy-v", "qa-v1.json", "qa-literacy-v1-"]) {
      expect(() => parseQaDataset(withVersion(v)), v).toThrow(/version/);
    }
  });
});

describe("qa-literacy-v1.json", () => {
  const ds = parseQaDataset(litJson);
  const index = litAnchorsJson as AnchorIndex;

  it("is versioned, carries provenance, and pins public CitrateNetwork sources by full commit", () => {
    expect(ds.version).toBe("qa-literacy-v1");
    expect(ds.provenance.author).toMatch(/Larry Klosowski/);
    expect(ds.provenance.disjointFromTraining).toBe(true);
    for (const s of Object.values(ds.sources)) {
      expect(s.repo).toMatch(/^CitrateNetwork\//);
      expect(s.visibility).toBe("public");
    }
  });
  it("has exactly 30 items with unique ids, none shared with qa-v1", () => {
    expect(ds.items).toHaveLength(30);
    const ids = ds.items.map((i) => i.id);
    expect(new Set(ids).size).toBe(30);
    const v1 = new Set(parseQaDataset(qaV1Json).items.map((i) => i.id));
    expect(ids.filter((id) => v1.has(id))).toEqual([]);
    for (const id of ids) expect(id).toMatch(/^qa-lit-/);
  });
  it("is mostly paraconsensus and precompile questions, at every difficulty", () => {
    const para = ds.items.filter((i) => i.category === "paraconsensus-fl").length;
    const pre = ds.items.filter((i) => i.category === "devtools-contracts" || i.category === "ai-inference").length;
    expect(para).toBeGreaterThanOrEqual(10);
    expect(pre).toBeGreaterThanOrEqual(10);
    for (const d of ["easy", "medium", "hard"]) expect(ds.items.some((i) => i.difficulty === d)).toBe(true);
  });
  it("has 3 unanswerable probes (10 %)", () => {
    expect(ds.items.filter((i) => !i.answerable)).toHaveLength(3);
  });
  it("every citation path + anchor exists in the anchor index at the pinned commit", () => {
    expect(findMissingCitations(ds, index)).toEqual([]);
  });
  it("the anchor index lists only files the dataset cites (no stale entries)", () => {
    const cited = new Set(ds.items.flatMap((i) => i.citations.map((c) => `${c.source}:${c.path}`)));
    const indexed = Object.entries(index.sources).flatMap(([s, v]) => Object.keys(v.files).map((p) => `${s}:${p}`));
    expect(indexed.filter((k) => !cited.has(k))).toEqual([]);
    expect(index.version).toBe(ds.version);
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

const ds = parseQaDataset(litJson);
const unavailable = Object.entries(ds.sources)
  .filter(([name, s]) => {
    const dir = join(sourcesRoot, name);
    return !existsSync(dir) || git(dir, ["cat-file", "-e", `${s.commit}^{commit}`]) === null;
  })
  .map(([name]) => name);
const live = unavailable.length === 0;
if (!live) {
  console.info(
    `qa-literacy-v1 live provenance skipped: source repo(s) ${unavailable.join(", ")} not found at the pinned commit ` +
      `under ${sourcesRoot} (set QA_SOURCES_ROOT). The committed anchor index is still checked above.`,
  );
}

describe.skipIf(!live)("qa-literacy-v1 live provenance against the pinned source commits", () => {
  const index = litAnchorsJson as AnchorIndex;
  const cache = new Map<string, string>();
  const show = (source: string, path: string): string => {
    const key = `${source}:${path}`;
    const hit = cache.get(key);
    if (hit !== undefined) return hit;
    const text = git(join(sourcesRoot, source), ["show", `${ds.sources[source].commit}:${path}`]) ?? "";
    cache.set(key, text);
    return text;
  };

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
      for (const it of ds.items.filter((i) => i.answerable)) {
        const text = it.citations.map((c) => extractSection(show(c.source, c.path), c.anchor) ?? "").join("\n");
        const missing = ungroundedKeyPoints(it, text);
        if (missing.length) problems.push(`${it.id}: ${missing.map((k) => k.join("|")).join("; ")}`);
      }
      expect(problems).toEqual([]);
    },
    LIVE_TIMEOUT_MS,
  );
});
