// @vitest-environment node
//
// HUP-S11.0 / gate g5-size: the installer size gate is wired into .github/workflows/release.yml
// as a MANUAL-ONLY step. A tag push builds and publishes exactly as before; only a manual
// dispatch with `size_gate: true` runs scripts/size-budget.mjs, and then the release is created
// as a draft and published only after the gate passes (an over-budget build stays a draft).
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
const WINDOWS_FILE = path.join(repoRoot, ".github", "workflows", "windows-release.yml");
const INSTALLED_ACCEPTANCE_FILE = path.join(
  repoRoot,
  "src-tauri",
  "windows",
  "tests",
  "test-installed-product.ps1",
);
const src = fs.readFileSync(FILE, "utf8");
const windowsSrc = fs.readFileSync(WINDOWS_FILE, "utf8");
const installedAcceptanceSrc = fs.readFileSync(INSTALLED_ACCEPTANCE_FILE, "utf8");

function pyYaml(file) {
  const probe = spawnSync("python3", ["-c", "import yaml"], { encoding: "utf8" });
  if (probe.error || probe.status !== 0) return null;
  const r = spawnSync(
    "python3",
    [
      "-c",
      "import json,sys,yaml\nd=yaml.safe_load(open(sys.argv[1]))\nif True in d: d['on']=d.pop(True)\nprint(json.dumps(d))",
      file,
    ],
    { encoding: "utf8" },
  );
  if (r.status !== 0) throw new Error(`PyYAML failed to parse release.yml:\n${r.stderr}`);
  return JSON.parse(r.stdout);
}

const wf = pyYaml(FILE);
const windowsWf = pyYaml(WINDOWS_FILE);
const GATE = "${{ github.event_name == 'workflow_dispatch' && inputs.size_gate }}";

function steps() {
  if (!wf) return null;
  return wf.jobs["release-macos-arm"].steps;
}

describe("release.yml size gate (manual only)", () => {
  it("keeps normal version tag pushes, excludes Windows companion tags, and defaults size_gate off", () => {
    expect(src).toMatch(/^ {2}push:\n {4}tags:\n {6}- "v\*"\n {6}- "!v\*-windows"/m);
    expect(src).toMatch(/^ {6}size_gate:\n(?: {8}.*\n)*? {8}type: boolean\n/m);
    expect(src).toMatch(/^ {6}size_gate:\n(?: {8}.*\n)*? {8}default: false\n/m);
    if (wf) {
      const input = wf.on.workflow_dispatch.inputs.size_gate;
      expect(input.type).toBe("boolean");
      expect(input.default).toBe(false);
      expect(input.required).toBe(false);
      expect(wf.on.push.tags).toEqual(["v*", "!v*-windows"]);
    }
  });

  it("does not persist checkout credentials in the normal release job", () => {
    expect(src).toMatch(
      /- uses: actions\/checkout@[^\n]+\n {8}with:\n {10}persist-credentials: false/,
    );
    const s = steps();
    if (!s) return console.warn("PyYAML not available: checkout credentials not parsed");
    const checkout = s.find((x) => (x.uses ?? "").startsWith("actions/checkout@"));
    expect(checkout).toBeDefined();
    expect(checkout.with["persist-credentials"]).toBe(false);
  });

  it("does not activate the legacy macOS publisher by exporting GITHUB_TOKEN to Tauri", () => {
    const s = steps();
    expect(src).not.toMatch(
      /uses: tauri-apps\/tauri-action@[^\n]+\n {8}env:\n(?: {10}.*\n)*? {10}GITHUB_TOKEN:/,
    );
    if (!s) return console.warn("PyYAML not available: Tauri action environment not parsed");
    const build = s.find((x) => (x.uses ?? "").startsWith("tauri-apps/tauri-action@"));
    expect(build).toBeDefined();
    expect(build.env).not.toHaveProperty("GITHUB_TOKEN");
  });

  it("rejects manual Windows companion tags before checkout or publication", () => {
    const s = steps();
    expect(src).toMatch(/case "\$TAG" in\n {12}\*-windows\)/);
    if (!s) return console.warn("PyYAML not available: manual tag rejection order not checked");
    const reject = s.findIndex((x) => x.name === "Reject Windows companion tags from manual macOS release");
    const checkout = s.findIndex((x) => (x.uses ?? "").startsWith("actions/checkout@"));
    const build = s.findIndex((x) => (x.uses ?? "").startsWith("tauri-apps/tauri-action@"));
    expect(reject).toBe(0);
    expect(reject).toBeLessThan(checkout);
    expect(reject).toBeLessThan(build);
    expect(s[reject].if).toBe("${{ github.event_name == 'workflow_dispatch' }}");
    expect(s[reject].env.TAG).toBe("${{ inputs.tag }}");
    expect(s[reject].run).not.toMatch(/\$\{\{/);
  });

  it("runs size-budget.mjs only on a manual dispatch with size_gate, after the build, on the build's bundle dir", () => {
    const calls = src.split("\n").filter((l) => !/^\s*#/.test(l) && l.includes("scripts/size-budget.mjs"));
    expect(calls.length).toBe(1);
    expect(calls[0]).toMatch(/--bundle-dir target\/aarch64-apple-darwin\/release\/bundle/);
    expect(calls[0]).toMatch(/--arch aarch64/);
    const s = steps();
    if (!s) return console.warn("PyYAML not available: step order not checked");
    const build = s.findIndex((x) => (x.uses ?? "").startsWith("tauri-apps/tauri-action@"));
    const gate = s.findIndex((x) => (x.run ?? "").includes("scripts/size-budget.mjs"));
    expect(build).toBeGreaterThanOrEqual(0);
    expect(gate).toBeGreaterThan(build);
    expect(s[gate].if).toBe(GATE);
  });

  it("drafts the release when the gate runs and publishes the draft only after it passes", () => {
    const s = steps();
    if (!s) return console.warn("PyYAML not available: draft wiring not checked");
    const build = s.find((x) => (x.uses ?? "").startsWith("tauri-apps/tauri-action@"));
    expect(build.with.releaseDraft).toBe("${{ github.event_name == 'workflow_dispatch' && inputs.size_gate == true }}");
    const gate = s.findIndex((x) => (x.run ?? "").includes("scripts/size-budget.mjs"));
    const publish = s.findIndex((x) => /gh release edit "\$TAG" --draft=false/.test(x.run ?? ""));
    expect(publish).toBe(gate + 1);
    expect(s[publish].if).toBe(GATE);
    // The tag comes in through env, never spliced into the shell line.
    expect(s[publish].env.TAG).toBe("${{ github.event.inputs.tag }}");
    expect(s[publish].run).not.toMatch(/\$\{\{/);
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

describe("windows release staging controls", () => {
  it("forces and verifies Rust 1.96.0 before every dependency build", () => {
    expect(windowsSrc).toMatch(/^ {4}env:\n {6}RUSTUP_TOOLCHAIN: 1\.96\.0$/m);
    if (!windowsWf) return console.warn("PyYAML not available: Windows Rust controls not parsed");
    const job = windowsWf.jobs.build_windows;
    expect(job.env.RUSTUP_TOOLCHAIN).toBe("1.96.0");
    const setup = job.steps.findIndex((x) => (x.uses ?? "").startsWith("dtolnay/rust-toolchain@"));
    const verify = job.steps.findIndex((x) => x.name === "Verify the enforced Rust toolchain");
    const builds = job.steps
      .map((x, index) => ({ index, run: x.run ?? "" }))
      .filter(({ run }) => run.includes("cargo build") || run.includes("build-sidecar.sh"));
    expect(setup).toBeGreaterThanOrEqual(0);
    expect(verify).toBe(setup + 1);
    expect(builds.length).toBeGreaterThan(0);
    expect(builds.every(({ index }) => index > verify)).toBe(true);
    expect(job.steps[verify].run).toMatch(/rustc -V/);
    expect(job.steps[verify].run).toMatch(/cargo -V/);
    expect(job.steps[verify].run).toMatch(/rustc 1\\\.96\\\.0/);
    expect(job.steps[verify].run).toMatch(/cargo 1\\\.96\\\.0/);
  });

  it("stages the unique BGE model directory directly without nesting", () => {
    expect(windowsSrc).toContain(`find "$bge_extract" -type f -name config.json -print0`);
    expect(windowsSrc).toContain(`if (( \${#bge_configs[@]} != 1 )); then`);
    expect(windowsSrc).toContain(`for file in config.json tokenizer.json model.safetensors; do`);
    expect(windowsSrc).toContain(`cp -R "$bge_source/". "$bge_destination/"`);
    expect(windowsSrc).not.toContain(`cp -R "$bge_extract/"* src-tauri/models/bge-base-en-v1.5/`);
    if (!windowsWf) return console.warn("PyYAML not available: Windows BGE staging not parsed");
    const stage = windowsWf.jobs.build_windows.steps.find((x) =>
      (x.run ?? "").includes('bge_configs < <('),
    );
    expect(stage).toBeDefined();
    expect(stage.run).toContain('bge_source="$(dirname "${bge_configs[0]}")"');
    expect(stage.run).toContain('rm -rf "$bge_destination"');
    expect(stage.run).toContain('cp -R "$bge_source/". "$bge_destination/"');
  });

  it("observes production memory and node autostart without direct lifecycle commands", () => {
    expect(installedAcceptanceSrc).not.toContain("memory_import_knowledge");
    expect(installedAcceptanceSrc).not.toMatch(/-Command 'node_(?:start|stop)'/);
    expect(installedAcceptanceSrc).toContain("function Get-ImportProcesses");
    expect(installedAcceptanceSrc).toContain("import-corpus");
    expect(installedAcceptanceSrc).toContain("Wait-ImportResumeCompletion");
    expect(installedAcceptanceSrc).toContain("Wait-MemoryDaemonProcess");
    expect(installedAcceptanceSrc).toContain("-Command 'node_status'");
    expect(installedAcceptanceSrc).toContain("gracefulShutdownProcessCount");
    expect(installedAcceptanceSrc).toContain("gracefulShutdownMemoryProcessCount");
  });

  it("binds consensus RPC evidence to one stable installed node listener", () => {
    expect(installedAcceptanceSrc).toContain("function Get-NodeRpcListenerIdentity");
    expect(installedAcceptanceSrc).toContain(
      "Get-NetTCPConnection -State Listen -LocalPort 8545",
    );
    expect(installedAcceptanceSrc).toContain("Get-CimInstance -ClassName Win32_Process");
    expect(installedAcceptanceSrc).toContain("[System.StringComparison]::OrdinalIgnoreCase");
    expect(installedAcceptanceSrc).toContain("creationTimeUtcTicks");
    expect(installedAcceptanceSrc).toContain("rpcListener = [pscustomobject]");

    const before = installedAcceptanceSrc.indexOf(
      "$ListenerBefore = Get-NodeRpcListenerIdentity",
    );
    const firstRpc = installedAcceptanceSrc.indexOf("$LocalGenesis = Invoke-JsonRpc");
    const lastRpc = installedAcceptanceSrc.indexOf("$Remote2001 = Invoke-JsonRpc");
    const after = installedAcceptanceSrc.indexOf(
      "$ListenerAfter = Get-NodeRpcListenerIdentity",
    );
    expect(before).toBeGreaterThanOrEqual(0);
    expect(before).toBeLessThan(firstRpc);
    expect(after).toBeGreaterThan(lastRpc);

    expect(windowsSrc).toContain("$node.rpcListener.before");
    expect(windowsSrc).toContain("$node.rpcListener.after");
    expect(windowsSrc).toContain("[int]$listenerBefore.localPort -ne 8545");
    expect(windowsSrc).toContain(
      "[System.IO.Path]::GetFileName([string]$listenerBefore.executablePath) -ine 'citrate.exe'",
    );
  });

  it("records owner-authorized GUI waivers in provenance and release handoff", () => {
    const rows = [
      "Passkey: WAIVED (owner 2026-10-17)",
      "Membership-Activation: WAIVED (owner 2026-10-17)",
      "Visual-Resize: WAIVED (owner 2026-10-17)",
      "Toolchain-Panel: WAIVED (owner 2026-10-17)",
      "Certification: not yet certified",
    ];
    for (const row of rows) {
      expect(windowsSrc.split(row)).toHaveLength(3);
    }
    expect(windowsSrc).toContain("Authenticode: NotSigned");
    expect(windowsSrc).not.toContain("Certification: not certified");
  });
});
