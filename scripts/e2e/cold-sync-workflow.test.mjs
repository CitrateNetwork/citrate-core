// @vitest-environment node
//
// .github/workflows/cold-sync.yml: dry runs of its shell steps, under the shell GitHub uses for
// `shell: bash` (bash --noprofile --norc -eo pipefail), so a dispatch path that cannot work is
// caught before the first dispatch (#523: the default path, no profile_name, failed the resolve
// step). The harness itself is replaced by a recording `python3` on PATH.
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..", "..");
const REAL_PY = spawnSync("sh", ["-c", "command -v python3"], { encoding: "utf8" }).stdout.trim();
const workflow = fs.readFileSync(path.join(repo, ".github", "workflows", "cold-sync.yml"), "utf8");

// The `run: |` block of the step named `name`, dedented.
function stepScript(name) {
  const lines = workflow.split("\n");
  const at = lines.findIndex((l) => l.trim() === `- name: ${name}`);
  expect(at, `step ${name}`).toBeGreaterThan(-1);
  const run = lines.findIndex((l, i) => i > at && /^\s+run: \|\s*$/.test(l));
  const indent = lines[run].match(/^\s*/)[0].length;
  const body = [];
  for (const l of lines.slice(run + 1)) {
    if (l.trim() !== "" && l.match(/^\s*/)[0].length <= indent) break;
    body.push(l);
  }
  const pad = Math.min(...body.filter((l) => l.trim()).map((l) => l.match(/^\s*/)[0].length));
  return body.map((l) => l.slice(pad)).join("\n");
}

let dir;
beforeEach(() => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), "cold-sync-wf-"));
});
afterEach(() => fs.rmSync(dir, { recursive: true, force: true }));

// Steps whose run block a test has executed; the coverage tripwire at the end needs every one.
const exercised = new Set();
function runStep(name, env, cwd) {
  exercised.add(name);
  return bash(stepScript(name), env, cwd);
}
function bash(script, env, cwd) {
  const file = path.join(dir, "step.sh");
  fs.writeFileSync(file, script);
  return spawnSync("bash", ["--noprofile", "--norc", "-eo", "pipefail", file], {
    cwd, env: { PATH: process.env.PATH, ...env }, encoding: "utf8",
  });
}
// A checkout with (or without) src-tauri/network/profiles.json.
function checkout(withProfiles) {
  const root = path.join(dir, "repo");
  fs.mkdirSync(path.join(root, "src-tauri", "network"), { recursive: true });
  if (withProfiles) fs.writeFileSync(path.join(root, "src-tauri", "network", "profiles.json"), "{}");
  return root;
}
const OUT = () => path.join(dir, "out");
const outputs = () => ({ GITHUB_OUTPUT: path.join(dir, "gh-output"), GITHUB_STEP_SUMMARY: path.join(dir, "summary") });
const resolve = (env, withProfiles = true) =>
  runStep("Resolve the profile", { OUT: OUT(), IN_PROFILE_NAME: "", IN_PROFILE_JSON: "", ...outputs(), ...env }, checkout(withProfiles));
const profileArgs = () => fs.readFileSync(path.join(OUT(), "profile-args"), "utf8").split("\n").filter(Boolean);

describe("Resolve the profile", () => {
  it("the default (no profile_name): the profiles.json and its active profile", () => {
    const r = resolve({});
    expect(r.status, r.stderr).toBe(0);
    expect(profileArgs()).toEqual(["--profiles", "src-tauri/network/profiles.json"]);
  });

  it("a profile_name", () => {
    const r = resolve({ IN_PROFILE_NAME: "genesis-2026-12" });
    expect(r.status, r.stderr).toBe(0);
    expect(profileArgs()).toEqual(["--profiles", "src-tauri/network/profiles.json", "--name", "genesis-2026-12"]);
  });

  it("a profile_json, kept as data", () => {
    const json = '{"chainId": 40204, "label": "$(touch pwned) `x`"}';
    const r = resolve({ IN_PROFILE_JSON: json });
    expect(r.status, r.stderr).toBe(0);
    expect(profileArgs()).toEqual(["--profile", path.join(OUT(), "profile.json")]);
    expect(fs.readFileSync(path.join(OUT(), "profile.json"), "utf8")).toBe(json);
    expect(fs.existsSync(path.join(dir, "repo", "pwned"))).toBe(false);
  });

  it("no profiles.json and no profile_json: refused", () => {
    const r = resolve({}, false);
    expect(r.status).toBe(1);
    expect(r.stdout).toMatch(/no src-tauri\/network\/profiles\.json/);
  });
});

describe("Cold sync", () => {
  // A python3 that records its arguments and writes a PASS verdict where --out points.
  function fakePython() {
    const bin = path.join(dir, "bin");
    fs.mkdirSync(bin);
    fs.writeFileSync(path.join(bin, "python3"), `#!/bin/sh
case "$1" in
  scripts/e2e/cold-sync.py)
    printf '%s\\n' "$@" > "${dir}/harness-args"
    out=""; prev=""
    for a in "$@"; do [ "$prev" = "--out" ] && out="$a"; prev="$a"; done
    mkdir -p "$out"; echo '{"verdict":"PASS","reasons":[],"rules":[]}' > "$out/verdict.json"; exit 0 ;;
  *) exec "${REAL_PY}" "$@" ;;
esac
`, { mode: 0o755 });
    return bin;
  }
  const run = (env) => {
    const root = checkout(true);
    fs.mkdirSync(OUT(), { recursive: true });
    fs.writeFileSync(path.join(OUT(), "profile-args"), "--profiles\nsrc-tauri/network/profiles.json\n");
    return runStep("Cold sync", {
      PATH: `${fakePython()}:${process.env.PATH}`, OUT: OUT(), NODE_BIN: "/opt/citrate", NODE_SHA256: "ab".repeat(32),
      IN_TIMEOUT: "60m", IN_EVERY: "1000", IN_BLOCKS: "", ...outputs(), ...env,
    }, root);
  };
  const harnessArgs = () => fs.readFileSync(path.join(dir, "harness-args"), "utf8").split("\n").filter(Boolean);

  it("to tip (blocks empty): no --blocks, and the step passes", () => {
    const r = run({});
    expect(r.status, r.stderr).toBe(0);
    expect(harnessArgs()).not.toContain("--blocks");
    expect(harnessArgs()).toEqual(expect.arrayContaining(["--node-bin", "/opt/citrate", "--expect-sha256", "ab".repeat(32)]));
  });

  it("bounded: --blocks passed through", () => {
    const r = run({ IN_BLOCKS: "5000" });
    expect(r.status, r.stderr).toBe(0);
    const a = harnessArgs();
    expect(a[a.indexOf("--blocks") + 1]).toBe("5000");
  });
});

describe("tripwire: every step with a run block has a dry run in this file", () => {
  // A structural check of shell text can always be dodged (#526 review: `|| false`, a comment, a
  // line continuation, `test`, `:;`, no spaces). Running every step is the check that holds: a
  // step that cannot work under `bash -eo pipefail` fails its own dry run.
  it("every `run:` step was executed by a test above", () => {
    const steps = [...workflow.matchAll(/^\s+- name: (.+)\n(?:\s+(?!- ).*\n)*?\s+run: \|/gm)].map((m) => m[1].trim());
    expect(steps.length).toBeGreaterThanOrEqual(6);
    expect(steps.filter((s) => !exercised.has(s))).toEqual([]);
  });
});
