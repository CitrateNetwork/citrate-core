// @vitest-environment node
//
// HUP-S11.2: scorecard markdown renderer (scripts/eval-scorecard.mjs).
//
// Fixture scorecards in the exact shapes scripts/eval-tools.mjs and scripts/eval-qa.mjs write,
// the committed real results in eval/results/, and the CLI contract: no scorecards in, nothing
// written and exit 2 (Rule 1: never an invented or empty scorecard).
import { describe, expect, it, beforeAll, afterAll } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { classifyScorecard, loadScorecards, renderScorecardMarkdown, splitFrontmatter } from "./eval-scorecard.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const SCRIPT = path.join(here, "eval-scorecard.mjs");

const TOOLS = {
  model: "m-small",
  datasetVersion: "toolcall-v1+injection-v1",
  n: 80,
  nToolcall: 57,
  nInjection: 23,
  validToolCallRate: 0.95,
  correctToolRate: 0.9,
  argsOkRate: 0.875,
  injectionResistRate: 1,
  failures: ["tc-a", "tc-b"],
  failureReasons: { "tc-a": ["expected x, got y"], "tc-b": ["args: q|r does not match"] },
  startedAt: "2026-10-01T03:37:18.027Z",
  finishedAt: "2026-10-01T03:42:18.097Z",
  scoring: "deterministic",
  tier: "T1",
};

const QA = {
  scorecard: {
    datasetVersion: "qa-v1",
    model: "m-small",
    tier: "T0",
    startedAt: "2026-10-01T05:00:00.000Z",
    n: 40,
    passRate: 0.75,
    keyPointCoverage: 0.8125,
    citationHitRate: 0.7,
    citationValidity: null,
    abstentionRate: 1,
    falseAbstentionRate: 0.05,
    byCategory: {},
    failures: ["qa-1"],
    failureReasons: { "qa-1": ["missing key point"] },
  },
  items: [],
};

let tmp;

function runCli(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
}

beforeAll(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), "eval-scorecard-"));
  const dir = path.join(tmp, "results");
  fs.mkdirSync(dir);
  fs.writeFileSync(path.join(dir, "2026-10-01-m-small.json"), JSON.stringify(TOOLS));
  fs.writeFileSync(path.join(dir, "2026-10-01-qa-m-small.json"), JSON.stringify(QA));
  fs.writeFileSync(path.join(dir, "notes.json"), JSON.stringify({ hello: 1 }));
  fs.writeFileSync(path.join(dir, "README.md"), "# not a scorecard\n");
  fs.mkdirSync(path.join(tmp, "empty"));
});

afterAll(() => {
  fs.rmSync(tmp, { recursive: true, force: true });
});

describe("classifyScorecard", () => {
  it("recognises tool-call and QA scorecards and nothing else", () => {
    expect(classifyScorecard(TOOLS)?.kind).toBe("tools");
    expect(classifyScorecard(QA)?.kind).toBe("qa");
    expect(classifyScorecard({ hello: 1 })).toBeNull();
    expect(classifyScorecard([])).toBeNull();
    expect(classifyScorecard(null)).toBeNull();
    expect(classifyScorecard({ ...TOOLS, validToolCallRate: "1" })).toBeNull();
  });
});

describe("loadScorecards", () => {
  it("reads every json in the dir, skips non-scorecards by name", () => {
    const s = loadScorecards(path.join(tmp, "results"));
    expect(s.tools.map((t) => t.file)).toEqual(["2026-10-01-m-small.json"]);
    expect(s.qa.map((t) => t.file)).toEqual(["2026-10-01-qa-m-small.json"]);
    expect(s.skipped).toEqual(["notes.json"]);
  });
});

describe("renderScorecardMarkdown", () => {
  const md = () =>
    renderScorecardMarkdown(loadScorecards(path.join(tmp, "results")), {
      created: "2026-10-01",
      branch: "hup/test",
      source: "results",
    });

  it("starts with Rule-12 frontmatter", () => {
    const { front } = splitFrontmatter(md());
    expect(front).toContain("created: 2026-10-01");
    expect(front).toContain("branch: hup/test");
    expect(front).toMatch(/author: .+/);
    expect(front).toMatch(/status: generated/);
  });

  it("renders the tool-call row with percentages from the scorecard", () => {
    const m = md();
    expect(m).toContain(
      "| 2026-10-01-m-small.json | m-small | T1 | toolcall-v1+injection-v1 | 80 | 95.0% | 90.0% | 87.5% | 100.0% | 2 | met |",
    );
  });

  it("renders the QA row, with n/a for a null rate", () => {
    expect(md()).toContain(
      "| 2026-10-01-qa-m-small.json | m-small | T0 | qa-v1 | 40 | 75.0% | 81.3% | 70.0% | n/a | 100.0% | 5.0% | n/a | 1 |",
    );
  });

  it("marks a QA run that answered from the retrieved corpus (HUP-S3.1)", () => {
    const rag = { ...QA, scorecard: { ...QA.scorecard, retrieval: { mode: "memory.search passages", tenants: ["citrate-docs", "methodology"], k: 5 } } };
    fs.writeFileSync(path.join(tmp, "results", "2026-10-01-qa-rag-m-small.json"), JSON.stringify(rag));
    try {
      expect(md()).toContain("| 2026-10-01-qa-rag-m-small.json | m-small | T0 | qa-v1 + corpus (citrate-docs+methodology, k=5) | 40 |");
      // The closed-book row is unchanged.
      expect(md()).toContain("| 2026-10-01-qa-m-small.json | m-small | T0 | qa-v1 | 40 |");
    } finally {
      fs.rmSync(path.join(tmp, "results", "2026-10-01-qa-rag-m-small.json"));
    }
  });

  it("marks a run that answered through the app's memory_search tool, with its node-citation rate (g2-knowledge)", () => {
    const tool = {
      ...QA,
      scorecard: {
        ...QA.scorecard,
        citationNodeRate: 0.875,
        retrieval: { mode: "memory_search tool", tenants: ["citrate-docs", "methodology", "refs", "skills"], k: 5, maxTurns: 6 },
      },
    };
    fs.writeFileSync(path.join(tmp, "results", "2026-10-01-qa-tool-m-small.json"), JSON.stringify(tool));
    try {
      expect(md()).toContain("| 2026-10-01-qa-tool-m-small.json | m-small | T0 | qa-v1 + corpus (memory_search tool, k=5) | 40 |");
      expect(md()).toMatch(/\| 2026-10-01-qa-tool-m-small\.json \|.*\| 87\.5% \| 1 \|$/m);
      // A run without retrieval shows n/a in the node column.
      expect(md()).toContain("| 2026-10-01-qa-m-small.json | m-small | T0 | qa-v1 | 40 | 75.0% | 81.3% | 70.0% | n/a | 100.0% | 5.0% | n/a | 1 |");
    } finally {
      fs.rmSync(path.join(tmp, "results", "2026-10-01-qa-tool-m-small.json"));
    }
  });

  it("marks the g1 valid-tool-call bar per tier: met / not met on T1+, T0 has its own bar", () => {
    const low = { tools: [{ file: "x.json", sc: { ...TOOLS, validToolCallRate: 0.89 } }], qa: [], skipped: [] };
    expect(renderScorecardMarkdown(low, { created: "2026-10-01", branch: "b", source: "s" })).toContain("| not met |");
    const edge = { tools: [{ file: "x.json", sc: { ...TOOLS, validToolCallRate: 0.9 } }], qa: [], skipped: [] };
    expect(renderScorecardMarkdown(edge, { created: "2026-10-01", branch: "b", source: "s" })).toContain("| met |");
    const t0 = { tools: [{ file: "x.json", sc: { ...TOOLS, tier: "T0" } }], qa: [], skipped: [] };
    expect(renderScorecardMarkdown(t0, { created: "2026-10-01", branch: "b", source: "s" })).toContain(
      "| T0 bar |",
    );
    const none = { tools: [{ file: "x.json", sc: { ...TOOLS, tier: undefined } }], qa: [], skipped: [] };
    expect(renderScorecardMarkdown(none, { created: "2026-10-01", branch: "b", source: "s" })).toContain(
      "| no tier |",
    );
  });

  it("lists failure reasons and escapes pipes so the table stays intact", () => {
    const m = md();
    expect(m).toContain("`tc-a`: expected x, got y");
    expect(m).toContain("`tc-b`: args: q\\|r does not match");
    expect(m).toContain("`qa-1`: missing key point");
  });

  it("says what the scorecard does not measure", () => {
    expect(md()).toMatch(/step success/i);
  });
});

describe("CLI", () => {
  it("writes the markdown to --out and exits 0", () => {
    const out = path.join(tmp, "SCORECARD.md");
    const r = runCli(["--in", path.join(tmp, "results"), "--out", out, "--date", "2026-10-01", "--branch", "b"]);
    expect(r.status).toBe(0);
    expect(fs.readFileSync(out, "utf8")).toContain("m-small");
    expect(r.stderr).toContain("skipped notes.json");
  });

  it("exits 2 and writes nothing when the dir has no scorecards", () => {
    const out = path.join(tmp, "none.md");
    const r = runCli(["--in", path.join(tmp, "empty"), "--out", out]);
    expect(r.status).toBe(2);
    expect(fs.existsSync(out)).toBe(false);
  });

  it("exits 2 on an unknown flag or a bad --date", () => {
    expect(runCli(["--nope"]).status).toBe(2);
    expect(runCli(["--in", path.join(tmp, "results"), "--date", "yesterday"]).status).toBe(2);
  });
});

describe("committed eval/results/SCORECARD.md", () => {
  it("matches a fresh render of eval/results (regenerate with node scripts/eval-scorecard.mjs)", () => {
    const file = path.join(repoRoot, "eval", "results", "SCORECARD.md");
    const committed = fs.readFileSync(file, "utf8");
    const { front, body } = splitFrontmatter(committed);
    const created = /created: (\S+)/.exec(front)?.[1];
    const branch = /branch: (\S+)/.exec(front)?.[1];
    expect(created).toBeTruthy();
    const fresh = renderScorecardMarkdown(loadScorecards(path.join(repoRoot, "eval", "results")), {
      created,
      branch,
      source: "eval/results",
    });
    expect(splitFrontmatter(fresh).body).toBe(body);
  });
});
