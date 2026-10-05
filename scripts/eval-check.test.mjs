// @vitest-environment node
//
// HUP-S11.2 (US-11.2): the deterministic, no-model eval check (scripts/eval-check.mjs) that the
// `eval-check` pull-request workflow runs. Each check is shown to pass on the committed tree and
// to fail on a tampered copy (a changed fragment, a stale pin, a drifted scorecard, a lock whose
// skill commit disagrees with its source), so the job cannot go green on a tree it should refuse.
import { describe, expect, it as baseIt, beforeAll, afterAll } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  checkDatasetVersions,
  checkDatasets,
  checkPins,
  checkRuntimePin,
  checkScorecard,
  checkSkillsLock,
  listDatasetFiles,
  parsePins,
  renderPins,
  skillSourcePlan,
} from "./eval-check.mjs";
import { FROZEN_V1_SHA256 } from "../src/agent/eval/frozenPins.ts";

// Every case copies the tree or spawns the CLI, which loads the TypeScript eval modules; give them
// room on a loaded machine.
const SLOW_MS = 120_000;
const it = (name, fn) => baseIt(name, fn, SLOW_MS);

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const SCRIPT = path.join(here, "eval-check.mjs");
const EVAL_DIR = path.join(repoRoot, "src", "agent", "eval");

/** A copy of everything the check reads, so a test can tamper with it. */
function copyTree(dst) {
  for (const rel of [
    "src/agent/eval",
    "eval/results",
    "eval/sidecar-runtime.rev",
    "skills.lock",
    ".agentile/skill-intake/intake.json",
  ]) {
    fs.cpSync(path.join(repoRoot, rel), path.join(dst, rel), { recursive: true });
  }
}

function runCli(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8", timeout: 120_000 });
}

let tmp;
beforeAll(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), "eval-check-"));
});
afterAll(() => {
  fs.rmSync(tmp, { recursive: true, force: true });
});

describe("the committed tree passes", () => {
  it("node scripts/eval-check.mjs exits 0 and reports every check", () => {
    // Runs under plain node (no npm install), as the PR job does: this also proves the eval
    // modules the CLIs import resolve without a bundler.
    const r = runCli([]);
    expect(r.stderr + r.stdout).not.toMatch(/FAIL/);
    expect(r.status).toBe(0);
    for (const name of ["datasets", "dataset versions", "dataset pins", "skills.lock", "scorecard", "sidecar runtime pin"]) {
      expect(r.stdout).toContain(`ok   ${name}`);
    }
  });

  it("the dataset loaders accept every committed dataset", async () => {
    expect(await checkDatasets(EVAL_DIR)).toEqual([]);
  });
});

describe("dataset pins (src/agent/eval/datasets.sha256)", () => {
  it("pins every dataset file, the v1 pins equal the frozen A50 hashes", () => {
    const pins = parsePins(fs.readFileSync(path.join(EVAL_DIR, "datasets.sha256"), "utf8"));
    expect([...pins.keys()]).toEqual(listDatasetFiles(EVAL_DIR));
    for (const [file, sha] of Object.entries(FROZEN_V1_SHA256)) expect(pins.get(file)).toBe(sha);
    expect(listDatasetFiles(EVAL_DIR)).toContain("toolcall-v2.d/everyday.json");
  });

  it("renderPins reproduces the committed manifest byte for byte", () => {
    expect(renderPins(EVAL_DIR)).toBe(fs.readFileSync(path.join(EVAL_DIR, "datasets.sha256"), "utf8"));
  });

  it("names a changed, an unpinned and a missing file", () => {
    const root = path.join(tmp, "pins");
    copyTree(root);
    const dir = path.join(root, "src/agent/eval");
    const pinsText = fs.readFileSync(path.join(dir, "datasets.sha256"), "utf8");
    fs.appendFileSync(path.join(dir, "toolcall-v2.d", "everyday.json"), " ");
    fs.writeFileSync(path.join(dir, "toolcall-v2.d", "99-new.json"), "{}");
    fs.rmSync(path.join(dir, "workflow-v1.json"));
    const problems = checkPins(dir, pinsText).join("\n");
    expect(problems).toMatch(/toolcall-v2\.d\/everyday\.json: sha256 .* pinned/);
    expect(problems).toMatch(/toolcall-v2\.d\/99-new\.json: not pinned/);
    expect(problems).toMatch(/workflow-v1\.json: pinned but missing/);
  });

  it("refuses a manifest that moves a frozen v1 pin, even when the file matches it", () => {
    const root = path.join(tmp, "frozen");
    copyTree(root);
    const dir = path.join(root, "src/agent/eval");
    fs.appendFileSync(path.join(dir, "toolcall-v1.json"), "\n");
    const regenerated = renderPins(dir);
    const problems = checkPins(dir, regenerated).join("\n");
    expect(problems).toMatch(/toolcall-v1\.json: frozen \(A50\)/);
  });

  it("rejects a malformed manifest line", () => {
    expect(() => parsePins("abc  toolcall-v1.json\n")).toThrow(/line 1/);
    expect(() => parsePins(`${"a".repeat(64)}  ../escape.json\n`)).toThrow(/line 1/);
  });

  it("--write-pins refuses to rewrite a frozen pin", () => {
    const root = path.join(tmp, "write");
    copyTree(root);
    fs.appendFileSync(path.join(root, "src/agent/eval/injection-v1.json"), "\n");
    const r = runCli(["--root", root, "--write-pins"]);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/injection-v1\.json: frozen/);
  });
});

describe("dataset versions", () => {
  it("every dataset file names itself in `version`", () => {
    expect(checkDatasetVersions(EVAL_DIR)).toEqual([]);
  });
  it("flags a file whose version is not its name", () => {
    const root = path.join(tmp, "versions");
    copyTree(root);
    const f = path.join(root, "src/agent/eval/workflow-v1.json");
    const j = JSON.parse(fs.readFileSync(f, "utf8"));
    j.version = "workflow-v2";
    fs.writeFileSync(f, JSON.stringify(j));
    expect(checkDatasetVersions(path.join(root, "src/agent/eval")).join("\n")).toMatch(/workflow-v1\.json: version "workflow-v2"/);
  });
  it("a held-out manifest names its set in `dataset`, not `version` (HUP-S9.3)", () => {
    const root = path.join(tmp, "heldout");
    copyTree(root);
    const f = path.join(root, "src/agent/eval/toolcall-v2.heldout.json");
    const j = JSON.parse(fs.readFileSync(f, "utf8"));
    j.dataset = "injection-v2";
    fs.writeFileSync(f, JSON.stringify(j));
    expect(checkDatasetVersions(path.join(root, "src/agent/eval")).join("\n")).toMatch(
      /toolcall-v2\.heldout\.json: dataset "injection-v2", expected "toolcall-v2"/,
    );
  });
});

describe("datasets are schema-checked", () => {
  it("a v2 fragment naming an unknown tool fails, naming the problem", async () => {
    const root = path.join(tmp, "schema");
    copyTree(root);
    const f = path.join(root, "src/agent/eval/toolcall-v2.d/everyday.json");
    const j = JSON.parse(fs.readFileSync(f, "utf8"));
    j.tasks[0].expect = { tool: "no_such_tool" };
    fs.writeFileSync(f, JSON.stringify(j));
    expect((await checkDatasets(path.join(root, "src/agent/eval"))).join("\n")).toMatch(/toolcall-v2.*no_such_tool/);
  });
  it("a QA citation whose anchor is missing from the index fails", async () => {
    const root = path.join(tmp, "anchors");
    copyTree(root);
    const f = path.join(root, "src/agent/eval/qa-v1.anchors.json");
    const j = JSON.parse(fs.readFileSync(f, "utf8"));
    for (const src of Object.values(j.sources)) for (const file of Object.values(src.files)) file.anchors = [];
    fs.writeFileSync(f, JSON.stringify(j));
    expect((await checkDatasets(path.join(root, "src/agent/eval"))).join("\n")).toMatch(/qa-v1: citations missing from qa-v1\.anchors\.json/);
  });
  it("the CLI exits 1 on that tree", () => {
    const r = runCli(["--root", path.join(tmp, "schema")]);
    expect(r.status).toBe(1);
    expect(r.stdout).toMatch(/FAIL datasets/);
  });
});

describe("skills.lock", () => {
  const lockText = fs.readFileSync(path.join(repoRoot, "skills.lock"), "utf8");
  const intake = JSON.parse(fs.readFileSync(path.join(repoRoot, ".agentile/skill-intake/intake.json"), "utf8"));

  it("the committed lock is well formed and agrees with the intake", () => {
    expect(checkSkillsLock(lockText, intake)).toEqual([]);
  });

  it("flags a skill pinned to a different commit than its source", () => {
    const bad = lockText.replace(
      /(\[\[skill\]\]\nname = "[^"]+"\nsource = "trailofbits"\ncommit = ")a56045e9ae00b3506cacefea0f672aab0a1a6e3c/,
      "$10000000000000000000000000000000000000000",
    );
    expect(bad).not.toBe(lockText);
    expect(checkSkillsLock(bad, intake).join("\n")).toMatch(/commit .* is not its source's/);
  });

  it("flags a source commit that the intake does not record", () => {
    const moved = structuredClone(intake);
    moved.sources[0].commit = "1".repeat(40);
    expect(checkSkillsLock(lockText, moved).join("\n")).toMatch(/source trailofbits/);
  });

  it("plans one shallow checkout per source repo at the pinned commit", () => {
    const plan = skillSourcePlan(lockText);
    expect(plan).toContainEqual({
      dir: ".claude/plugins/marketplaces/trailofbits",
      url: "https://github.com/trailofbits/skills",
      commit: "a56045e9ae00b3506cacefea0f672aab0a1a6e3c",
    });
    // hermes-skills and hermes-optional share one checkout of the repo named in "(via ...)".
    const hermes = plan.filter((p) => p.url === "https://github.com/CitrateNetwork/testing-hermes-design");
    expect(hermes).toEqual([
      { dir: "testing-hermes-design", url: "https://github.com/CitrateNetwork/testing-hermes-design", commit: "5445e42b87b9918d5b1bfa9f4eadd8e4bb10ff37" },
    ]);
    for (const p of plan) {
      expect(p.url).toMatch(/^https:\/\/github\.com\/[\w.-]+\/[\w.-]+$/);
      expect(p.commit).toMatch(/^[0-9a-f]{40}$/);
      expect(p.dir.split("/")).not.toContain("..");
    }
  });
});

describe("scorecard", () => {
  it("the committed SCORECARD.md matches its JSON and every tier renders", () => {
    expect(checkScorecard(path.join(repoRoot, "eval", "results"))).toEqual([]);
  });

  it("flags a drifted SCORECARD.md and an unreadable scorecard JSON", () => {
    const root = path.join(tmp, "card");
    copyTree(root);
    const dir = path.join(root, "eval/results");
    const md = path.join(dir, "SCORECARD.md");
    fs.writeFileSync(md, fs.readFileSync(md, "utf8").replace("100.0%", "99.0%"));
    fs.writeFileSync(path.join(dir, "2026-01-01-broken.json"), "{ not json");
    const p = checkScorecard(dir).join("\n");
    expect(p).toMatch(/SCORECARD\.md does not match/);
    expect(p).toMatch(/2026-01-01-broken\.json/);
  });
});

describe("sidecar runtime pin (eval/sidecar-runtime.rev)", () => {
  it("names the runtime repo and a full commit", () => {
    expect(checkRuntimePin(fs.readFileSync(path.join(repoRoot, "eval", "sidecar-runtime.rev"), "utf8"))).toEqual([]);
  });
  it("refuses a branch name, a short sha or another repo", () => {
    for (const bad of [
      "repo = CitrateNetwork/citrate-agent-runtime\nrev = main\n",
      "repo = CitrateNetwork/citrate-agent-runtime\nrev = 45fceda\n",
      `repo = evil/citrate-agent-runtime\nrev = ${"a".repeat(40)}\n`,
    ]) {
      expect(checkRuntimePin(bad).length, bad).toBeGreaterThan(0);
    }
  });
});

describe("CLI usage", () => {
  it("exits 2 on an unknown flag", () => {
    expect(runCli(["--nope"]).status).toBe(2);
  });
});
