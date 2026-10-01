// @vitest-environment node
//
// HUP-S11.2: guard rails for .github/workflows/eval.yml.
//
// The eval workflow calls a model endpoint and can take an hour, so it must never run on its
// own: manual dispatch only, no push / pull_request / schedule / workflow_run triggers. These
// tests also pin the hygiene the other workflows follow (actions pinned to a full commit SHA,
// read-only token, dispatch inputs passed through env rather than spliced into shell).
//
// actionlint is not installed on this machine, so validity is checked two ways: a structural
// check in plain JS that runs everywhere, and a full YAML parse through python3 + PyYAML when
// present (skipped, and says so, when it is not).
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const FILE = path.join(repoRoot, ".github", "workflows", "eval.yml");
const src = fs.readFileSync(FILE, "utf8");
const lines = src.split("\n");

/** Top-level keys of the `on:` block, read by indentation (no YAML dependency). */
function triggerKeys() {
  const start = lines.findIndex((l) => /^on:\s*$/.test(l));
  if (start < 0) return null;
  const keys = [];
  for (let i = start + 1; i < lines.length; i++) {
    const l = lines[i];
    if (/^\S/.test(l)) break; // next top-level key
    const m = /^ {2}([A-Za-z_]+):/.exec(l);
    if (m) keys.push(m[1]);
  }
  return keys;
}

function pyYaml() {
  const probe = spawnSync("python3", ["-c", "import yaml"], { encoding: "utf8" });
  if (probe.error || probe.status !== 0) return null;
  const r = spawnSync(
    "python3",
    [
      "-c",
      // PyYAML reads the bare key `on` as boolean True; map it back.
      "import json,sys,yaml\nd=yaml.safe_load(open(sys.argv[1]))\nif True in d: d['on']=d.pop(True)\nprint(json.dumps(d))",
      FILE,
    ],
    { encoding: "utf8" },
  );
  if (r.status !== 0) throw new Error(`PyYAML failed to parse eval.yml:\n${r.stderr}`);
  return JSON.parse(r.stdout);
}

describe("eval.yml triggers", () => {
  it("is manual only: workflow_dispatch and nothing else", () => {
    expect(triggerKeys()).toEqual(["workflow_dispatch"]);
    for (const t of ["push", "pull_request", "pull_request_target", "schedule", "workflow_run", "merge_group"]) {
      expect(src).not.toMatch(new RegExp(`^\\s{2}${t}:`, "m"));
    }
  });
});

describe("eval.yml hygiene", () => {
  it("pins every action to a full commit SHA", () => {
    const uses = lines.filter((l) => /^\s*-?\s*uses:/.test(l));
    expect(uses.length).toBeGreaterThan(0);
    for (const u of uses) expect(u).toMatch(/uses:\s*[\w.-]+\/[\w.-]+@[0-9a-f]{40}\b/);
  });

  it("asks for a read-only token", () => {
    expect(src).toMatch(/^permissions:\n {2}contents: read\s*$/m);
  });

  it("never splices dispatch inputs or secrets into a run: script", () => {
    let inRun = false;
    let runIndent = 0;
    for (const l of lines) {
      const m = /^(\s*)(?:- )?run:\s*\|?\s*(.*)$/.exec(l);
      if (m) {
        inRun = true;
        runIndent = m[1].length;
        expect(m[2]).not.toMatch(/\$\{\{/);
        continue;
      }
      if (inRun) {
        const ind = /^(\s*)/.exec(l)[1].length;
        if (l.trim() !== "" && ind <= runIndent) inRun = false;
        else expect(l, `expression inside run: ${l}`).not.toMatch(/\$\{\{/);
      }
    }
  });

  it("passes the API key by env var NAME to the eval CLIs, never on argv", () => {
    expect(src).toContain("--api-key-env EVAL_API_KEY");
    expect(src).not.toMatch(/--api-key(?!-env)/);
  });

  it("uploads the scorecard as an artifact", () => {
    expect(src).toMatch(/uses:\s*actions\/upload-artifact@/);
    expect(src).toContain("SCORECARD.md");
  });
});

describe("eval.yml parses as YAML", () => {
  const parsed = pyYaml();
  it.skipIf(parsed === null)("PyYAML: valid, manual trigger, one job with steps", () => {
    expect(Object.keys(parsed.on)).toEqual(["workflow_dispatch"]);
    const inputs = parsed.on.workflow_dispatch.inputs;
    expect(inputs.model.required).toBe(true);
    expect(Object.keys(parsed.jobs)).toEqual(["eval"]);
    expect(Array.isArray(parsed.jobs.eval.steps)).toBe(true);
    expect(parsed.permissions).toEqual({ contents: "read" });
  });
  if (parsed === null) {
    it("PyYAML not available: full YAML parse skipped (structural checks above still ran)", () => {
      expect(parsed).toBeNull();
    });
  }
});

describe("no other workflow runs the eval", () => {
  it("only eval.yml invokes the eval CLIs", () => {
    const dir = path.join(repoRoot, ".github", "workflows");
    for (const f of fs.readdirSync(dir)) {
      if (f === "eval.yml") continue;
      const body = fs.readFileSync(path.join(dir, f), "utf8");
      expect(body, f).not.toMatch(/eval-tools\.mjs|eval-qa\.mjs/);
    }
  });
});
