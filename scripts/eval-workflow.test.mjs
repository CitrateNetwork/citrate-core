// @vitest-environment node
//
// HUP-S11.2: guard rails for .github/workflows/eval.yml (manual model runs) and
// .github/workflows/eval-check.yml (the no-model pull-request check).
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
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadScorecards } from "./eval-scorecard.mjs";

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
    expect(src).toContain("SCORECARD-$EVAL_TIER.md");
  });
});

describe("eval.yml parses as YAML", () => {
  const parsed = pyYaml();
  it.skipIf(parsed === null)("PyYAML: valid, manual trigger, one job with steps", () => {
    expect(Object.keys(parsed.on)).toEqual(["workflow_dispatch"]);
    const inputs = parsed.on.workflow_dispatch.inputs;
    expect(inputs.model.required).toBe(true);
    // HUP-S11.2: v2 is the default dataset generation; the sidecar suite is selectable.
    expect(inputs.datasets.default).toBe("v2");
    expect(inputs.datasets.options).toEqual(["v2", "v1"]);
    expect(inputs.suites.options).toEqual(["all", "tools", "qa", "sidecar"]);
    expect(inputs.tier.options).toEqual(["none", "T0", "T1", "T2"]);
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
  it("only eval.yml invokes the eval CLIs that call a model", () => {
    const dir = path.join(repoRoot, ".github", "workflows");
    for (const f of fs.readdirSync(dir)) {
      if (f === "eval.yml") continue;
      const body = fs.readFileSync(path.join(dir, f), "utf8");
      expect(body, f).not.toMatch(/eval-tools\.mjs|eval-qa\.mjs|eval-sidecar\.mjs/);
    }
  });
});

describe("eval.yml suites (HUP-S11.2)", () => {
  it("scores the v2 datasets by default and passes the choice through env", () => {
    expect(src).toContain("EVAL_DATASETS: ${{ inputs.datasets }}");
    expect(src).toContain('--datasets "$EVAL_DATASETS"');
  });

  it("runs the sidecar suite on binaries built from the pinned runtime commit, never an input", () => {
    expect(src).toContain("node scripts/eval-sidecar.mjs");
    expect(src).toContain("eval/sidecar-runtime.rev");
    expect(src).toMatch(/repository: \$\{\{ steps\.runtime\.outputs\.repo \}\}/);
    expect(src).toMatch(/ref: \$\{\{ steps\.runtime\.outputs\.rev \}\}/);
    expect(src).not.toMatch(/ref: \$\{\{ inputs\./);
    expect(src).toContain("cargo build --locked -p agent-sidecar -p citrate-agent-mcp-host");
    expect(src).toContain('--runtime-rev "$RUNTIME_REV"');
    // A remote endpoint is passed with --allow-remote to every model-calling CLI.
    for (const cli of ["eval-tools.mjs", "eval-qa.mjs", "eval-sidecar.mjs"]) expect(src).toContain(`node scripts/${cli}`);
    expect(src.match(/--allow-remote/g)?.length).toBeGreaterThanOrEqual(3);
  });

  it("refuses a non-https endpoint for the sidecar suite before the runtime is built", () => {
    // The sidecar refuses plain http off loopback; fail in seconds, not after a long Rust build.
    const guard = src.indexOf('[[ "$EVAL_BASE_URL" != https://* ]]');
    expect(guard).toBeGreaterThan(-1);
    expect(guard).toBeLessThan(src.indexOf("name: Check out citrate-agent-runtime at the pinned commit"));
    expect(guard).toBeGreaterThan(src.indexOf("name: Read the pinned runtime commit"));
  });

  it("checks datasets and pins before any model call, and gates the later suites on it", () => {
    const check = src.indexOf("node scripts/eval-check.mjs");
    expect(check).toBeGreaterThan(-1);
    expect(check).toBeLessThan(src.indexOf("node scripts/eval-tools.mjs"));
    expect(src).toMatch(/steps\.check\.outcome == 'success' && \(inputs\.suites == 'all' \|\| inputs\.suites == 'qa'\)/);
    expect(src).toMatch(/steps\.check\.outcome == 'success' && \(inputs\.suites == 'all' \|\| inputs\.suites == 'sidecar'\)/);
  });

  it("reads per-tier secrets with a single-secret fallback", () => {
    expect(src).toContain("secrets[format('EVAL_BASE_URL_{0}', inputs.tier)] || secrets.EVAL_BASE_URL");
    expect(src).toContain("secrets[format('EVAL_API_KEY_{0}', inputs.tier)] || secrets.EVAL_API_KEY");
  });

  it("renders and uploads one SCORECARD-<tier>.md per run", () => {
    expect(src).toContain('--tier "$EVAL_TIER"');
    expect(src).toContain("SCORECARD-$EVAL_TIER.md");
    expect(src).toContain("name: eval-scorecard-${{ inputs.tier }}-${{ github.run_id }}");
  });
});

// ── eval-check.yml: the deterministic pull-request job ─────────────────────────────────────────

const CHECK_FILE = path.join(repoRoot, ".github", "workflows", "eval-check.yml");
const checkSrc = fs.readFileSync(CHECK_FILE, "utf8");
const checkLines = checkSrc.split("\n");

function stepScript(srcLines, name) {
  const i = srcLines.findIndex((l) => l.includes(`- name: ${name}`));
  expect(i, name).toBeGreaterThan(-1);
  const r = srcLines.findIndex((l, j) => j > i && /^\s+run: \|\s*$/.test(l));
  const ind = /^(\s*)/.exec(srcLines[r + 1])[1].length;
  const body = [];
  for (let j = r + 1; j < srcLines.length; j++) {
    const l = srcLines[j];
    if (l.trim() !== "" && /^(\s*)/.exec(l)[1].length < ind) break;
    body.push(l.slice(ind));
  }
  return body.join("\n");
}

describe("eval-check.yml (HUP-S11.2 PR job)", () => {
  it("runs on pull requests into main and release branches, plus manual dispatch", () => {
    const start = checkLines.findIndex((l) => /^on:\s*$/.test(l));
    const keys = [];
    for (let i = start + 1; i < checkLines.length && !/^\S/.test(checkLines[i]); i++) {
      const m = /^ {2}([A-Za-z_]+):/.exec(checkLines[i]);
      if (m) keys.push(m[1]);
    }
    expect(keys).toEqual(["pull_request", "workflow_dispatch"]);
    expect(checkSrc).toContain('branches: [main, "release/**"]');
    expect(checkSrc).not.toMatch(/pull_request_target/);
  });

  it("calls no model, reads no secret and asks for a read-only token", () => {
    expect(checkSrc).not.toMatch(/secrets\./);
    expect(checkSrc).not.toMatch(/eval-tools\.mjs|eval-qa\.mjs|eval-sidecar\.mjs|EVAL_BASE_URL/);
    expect(checkSrc).toMatch(/^permissions:\n {2}contents: read\s*$/m);
    expect(checkSrc).not.toMatch(/npm (ci|install)/);
  });

  it("pins every action to a full commit SHA and checks out without persisted credentials", () => {
    const uses = checkLines.filter((l) => /^\s*-?\s*uses:/.test(l));
    expect(uses.length).toBeGreaterThan(0);
    for (const u of uses) expect(u).toMatch(/uses:\s*[\w.-]+\/[\w.-]+@[0-9a-f]{40}\b/);
    expect(checkSrc).toContain("persist-credentials: false");
  });

  it("runs the full check against the fetched skill sources", () => {
    expect(checkSrc).toContain('node scripts/eval-check.mjs --fetch-skill-sources "$RUNNER_TEMP/skill-sources"');
    expect(checkSrc).toContain('node scripts/eval-check.mjs --skills-sources "$RUNNER_TEMP/skill-sources"');
  });

  it("the per-tier render step renders every tier with results from the committed tree", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "eval-check-render-"));
    const summary = path.join(dir, "summary.md");
    fs.writeFileSync(summary, "");
    const r = spawnSync("bash", ["-e", "-c", stepScript(checkLines, "Render SCORECARD-<tier>.md from the committed results")], {
      cwd: repoRoot,
      encoding: "utf8",
      env: { PATH: process.env.PATH, RUNNER_TEMP: dir, GITHUB_STEP_SUMMARY: summary, GITHUB_REF_NAME: "test" },
      timeout: 120_000,
    });
    const files = fs.existsSync(path.join(dir, "scorecards")) ? fs.readdirSync(path.join(dir, "scorecards")).sort() : [];
    const text = fs.readFileSync(summary, "utf8");
    fs.rmSync(dir, { recursive: true, force: true });
    expect(r.status, r.stderr).toBe(0);
    // One file per tier that has results (T2 has none until DGX runs it), none for an empty tier.
    const cards = loadScorecards(path.join(repoRoot, "eval", "results"));
    const tiers = new Set([...cards.tools, ...cards.qa, ...cards.sidecar].map((e) => e.sc.tier ?? "none"));
    expect(files).toEqual([...tiers].map((t) => `SCORECARD-${t}.md`).sort());
    expect(files).toContain("SCORECARD-T0.md");
    expect(text).toContain("Tier: **T0** only.");
  }, 120_000);
});

describe("eval.yml endpoint check", () => {
  // The "Resolve endpoint" step writes the URL into $GITHUB_ENV, one NAME=value per line, so a
  // value with a line break would set extra variables for every later step. Run the step's own
  // script under bash with a temp GITHUB_ENV and check what it accepts.
  function resolveScript() {
    const i = lines.findIndex((l) => /- name: Resolve endpoint/.test(l));
    expect(i).toBeGreaterThan(-1);
    const r = lines.findIndex((l, j) => j > i && /^\s+run: \|\s*$/.test(l));
    const ind = /^(\s*)/.exec(lines[r + 1])[1].length;
    const body = [];
    for (let j = r + 1; j < lines.length; j++) {
      const l = lines[j];
      if (l.trim() !== "" && /^(\s*)/.exec(l)[1].length < ind) break;
      body.push(l.slice(ind));
    }
    return body.join("\n");
  }

  function runResolve(url) {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "eval-resolve-"));
    const envFile = path.join(dir, "env");
    fs.writeFileSync(envFile, "");
    const r = spawnSync("bash", ["-e", "-c", resolveScript()], {
      cwd: dir,
      encoding: "utf8",
      env: { PATH: process.env.PATH, GITHUB_ENV: envFile, EVAL_OUT: "out", INPUT_BASE_URL: url, SECRET_BASE_URL: "" },
    });
    const written = fs.readFileSync(envFile, "utf8");
    fs.rmSync(dir, { recursive: true, force: true });
    return { status: r.status, written };
  }

  it("accepts one http(s) URL and writes exactly one line", () => {
    const r = runResolve("https://eval.example/v1");
    expect(r.status).toBe(0);
    expect(r.written).toBe("EVAL_BASE_URL=https://eval.example/v1\n");
  });

  it("rejects a value with a line break, so nothing extra reaches GITHUB_ENV", () => {
    for (const bad of ["https://eval.example/v1\nNODE_OPTIONS=x", "https://eval.example/v1\r\nX=1", "not a url", ""]) {
      const r = runResolve(bad);
      expect(r.status, JSON.stringify(bad)).toBe(2);
      expect(r.written, JSON.stringify(bad)).toBe("");
    }
  });
});
