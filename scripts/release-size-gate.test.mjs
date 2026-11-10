// @vitest-environment node
//
// HUP-S11.0 / gate g5-size: the installer size gate is wired into .github/workflows/release.yml
// as a MANUAL-ONLY step. A tag push builds exactly as before; only a manual dispatch with
// `size_gate: true` runs scripts/size-budget.mjs, after the build and before the workflow artifact
// upload, so an over-budget build uploads nothing. release.yml publishes nothing either way
// (053-UPDATER decision 2: no draft, no GitHub release). A dry run (`dry_run: true`) also runs the
// gate on its unsigned bundle.
//
// Structural checks in plain JS (no YAML dependency), plus a full parse through python3 + PyYAML
// when present (skipped, and says so, when it is not).
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const FILE = path.join(repoRoot, ".github", "workflows", "release.yml");
const src = fs.readFileSync(FILE, "utf8");

function pyYaml() {
  const probe = spawnSync("python3", ["-c", "import yaml"], { encoding: "utf8" });
  if (probe.error || probe.status !== 0) return null;
  const r = spawnSync(
    "python3",
    [
      "-c",
      "import json,sys,yaml\nd=yaml.safe_load(open(sys.argv[1]))\nif True in d: d['on']=d.pop(True)\nprint(json.dumps(d))",
      FILE,
    ],
    { encoding: "utf8" },
  );
  if (r.status !== 0) throw new Error(`PyYAML failed to parse release.yml:\n${r.stderr}`);
  return JSON.parse(r.stdout);
}

const wf = pyYaml();
const GATE = "${{ env.DRY_RUN == 'true' || (github.event_name == 'workflow_dispatch' && inputs.size_gate) }}";

function steps() {
  if (!wf) return null;
  return wf.jobs["release-macos-arm"].steps;
}

describe("release.yml size gate (manual only)", () => {
  it("keeps the tag-push trigger and adds a boolean size_gate dispatch input, default false", () => {
    expect(src).toMatch(/^ {2}push:\n {4}tags: \["v\*"\]/m);
    expect(src).toMatch(/^ {6}size_gate:\n(?: {8}.*\n)*? {8}type: boolean\n/m);
    expect(src).toMatch(/^ {6}size_gate:\n(?: {8}.*\n)*? {8}default: false\n/m);
    if (wf) {
      const input = wf.on.workflow_dispatch.inputs.size_gate;
      expect(input.type).toBe("boolean");
      expect(input.default).toBe(false);
      expect(input.required).toBe(false);
      expect(wf.on.push.tags).toEqual(["v*"]);
    }
  });

  it("runs size-budget.mjs only on a manual dispatch with size_gate or a dry run, after the build, on the build's bundle dir", () => {
    const calls = src.split("\n").filter((l) => !/^\s*#/.test(l) && l.includes("scripts/size-budget.mjs"));
    expect(calls.length).toBe(1);
    expect(calls[0]).toMatch(/--bundle-dir target\/aarch64-apple-darwin\/release\/bundle/);
    expect(calls[0]).toMatch(/--arch aarch64/);
    const s = steps();
    if (!s) return console.warn("PyYAML not available: step order not checked");
    const build = s.findIndex((x) => x.name === "Build · sign · notarize (macOS arm64)");
    const dryBuild = s.findIndex((x) => x.name === "Build the bundle unsigned (dry run)");
    const gate = s.findIndex((x) => (x.run ?? "").includes("scripts/size-budget.mjs"));
    expect(build).toBeGreaterThanOrEqual(0);
    expect(dryBuild).toBeGreaterThanOrEqual(0);
    expect(gate).toBeGreaterThan(build);
    expect(gate).toBeGreaterThan(dryBuild);
    expect(s[gate].if).toBe(GATE);
  });

  it("runs before the artifact upload, so an over-budget build uploads nothing, and drafts no release", () => {
    const s = steps();
    if (!s) return console.warn("PyYAML not available: upload order not checked");
    const gate = s.findIndex((x) => (x.run ?? "").includes("scripts/size-budget.mjs"));
    const upload = s.findIndex((x) => (x.uses ?? "").startsWith("actions/upload-artifact@"));
    expect(upload).toBeGreaterThan(gate);
    expect(s[upload].if).toBe("${{ env.DRY_RUN != 'true' }}");
    expect(s.some((x) => (x.uses ?? "").startsWith("tauri-apps/tauri-action@"))).toBe(false);
    expect(s.some((x) => /gh release (create|edit|upload)/.test(x.run ?? ""))).toBe(false);
    expect(s[gate].run).not.toMatch(/\$\{\{/);
  });

  it("is not run by any other workflow", () => {
    const dir = path.join(repoRoot, ".github", "workflows");
    for (const f of fs.readdirSync(dir)) {
      if (f === "release.yml") continue;
      expect(fs.readFileSync(path.join(dir, f), "utf8"), f).not.toMatch(/size-budget\.mjs/);
    }
  });
});
