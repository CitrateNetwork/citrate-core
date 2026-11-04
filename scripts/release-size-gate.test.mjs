// @vitest-environment node
//
// HUP-S11.0 / gate g5-size: the installer size gate is wired into .github/workflows/release.yml
// as a MANUAL-ONLY step. A tag push builds and publishes exactly as before; only a manual
// dispatch with `size_gate: true` runs scripts/size-budget.mjs, and then the release is created
// as a draft and published only after the gate passes (an over-budget build stays a draft). A dry
// run (`dry_run: true`) also runs the gate on its unsigned bundle, and never publishes.
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
const RELEASE_INSTALLER_FILE = path.join(
  repoRoot,
  "src-tauri",
  "windows",
  "tests",
  "test-release-installer.ps1",
);
const INSTALLER_SMOKE_WORKFLOW_FILE = path.join(
  repoRoot,
  ".github",
  "workflows",
  "windows-release-installer-smoke.yml",
);
const src = fs.readFileSync(FILE, "utf8");
const windowsSrc = fs.readFileSync(WINDOWS_FILE, "utf8");
const installedAcceptanceSrc = fs.readFileSync(INSTALLED_ACCEPTANCE_FILE, "utf8");
const releaseInstallerSrc = fs.readFileSync(RELEASE_INSTALLER_FILE, "utf8");
const installerSmokeWorkflowSrc = fs.readFileSync(INSTALLER_SMOKE_WORKFLOW_FILE, "utf8");

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
const GATE = "${{ env.DRY_RUN == 'true' || (github.event_name == 'workflow_dispatch' && inputs.size_gate) }}";
const PUBLISH = "${{ env.DRY_RUN != 'true' && github.event_name == 'workflow_dispatch' && inputs.size_gate }}";

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

  it("runs size-budget.mjs only on a manual dispatch with size_gate or a dry run, after the build, on the build's bundle dir", () => {
    const calls = src.split("\n").filter((l) => !/^\s*#/.test(l) && l.includes("scripts/size-budget.mjs"));
    expect(calls.length).toBe(1);
    expect(calls[0]).toMatch(/--bundle-dir target\/aarch64-apple-darwin\/release\/bundle/);
    expect(calls[0]).toMatch(/--arch aarch64/);
    const s = steps();
    if (!s) return console.warn("PyYAML not available: step order not checked");
    const build = s.findIndex((x) => (x.uses ?? "").startsWith("tauri-apps/tauri-action@"));
    const dryBuild = s.findIndex((x) => (x.run ?? "").includes("npx tauri build"));
    const gate = s.findIndex((x) => (x.run ?? "").includes("scripts/size-budget.mjs"));
    expect(build).toBeGreaterThanOrEqual(0);
    expect(dryBuild).toBeGreaterThanOrEqual(0);
    expect(gate).toBeGreaterThan(build);
    expect(gate).toBeGreaterThan(dryBuild);
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
    // A dry run reaches the gate but never the publish step.
    expect(s[publish].if).toBe(PUBLISH);
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

  it("builds the node through the pinned engine at this release's chain rev", () => {
    expect(windowsSrc).not.toMatch(/build-sidecar\.sh[^\n]*\n(?:[^\n]*\\\n)*[^\n]*--output/);
    if (!windowsWf) return console.warn("PyYAML not available: Windows node build step not parsed");
    const step = windowsWf.jobs.build_windows.steps.find((x) => (x.run ?? "").includes("build-sidecar.sh"));
    expect(step).toBeDefined();
    expect(step.run).toContain("component-pins.mjs field citrate-node source.rev");
    expect(step.run).toContain('[ "$pinned_rev" != "$CHAIN_SHA" ]');
    expect(step.run).toContain("--print-digest");
    expect(step.run).not.toContain("--output");
    expect(step.run).toContain('actual="$(sha256sum "$staged" | cut -d\' \' -f1)"');
    expect(step.run).toContain('install -m 755 "$staged" "src-tauri/binaries/citrate-$TARGET_TRIPLE.exe"');
    const pinFirst = step.run.indexOf("$CHAIN_SHA");
    const build = step.run.indexOf("build-sidecar.sh");
    const install = step.run.indexOf("install -m 755");
    expect(pinFirst).toBeLessThan(build);
    expect(build).toBeLessThan(install);
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

  it("uses exact Known Folder app-data children with fail-closed owned cleanup", () => {
    expect(installedAcceptanceSrc).toContain(
      "[Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)",
    );
    expect(installedAcceptanceSrc).toContain(
      "[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)",
    );
    expect(installedAcceptanceSrc).toContain("$AppIdentifier = 'ai.citrate.core'");
    expect(installedAcceptanceSrc).not.toContain("$ProfileRoot");
    expect(installedAcceptanceSrc).not.toContain("citrate-installed-product-profile-");

    const directPathGuard = installedAcceptanceSrc.slice(
      installedAcceptanceSrc.indexOf("function Assert-DirectKnownFolderAppPath"),
      installedAcceptanceSrc.indexOf("function New-OwnedAppDirectory"),
    );
    expect(directPathGuard).toContain("[string]::IsNullOrWhiteSpace($KnownFolder)");
    expect(directPathGuard).toContain("Test-Path -LiteralPath $CanonicalKnownFolder -PathType Container");
    expect(directPathGuard).toContain("[System.IO.Path]::GetDirectoryName($CanonicalAppDirectory)");
    expect(directPathGuard).toContain("[System.IO.Path]::GetFileName($CanonicalAppDirectory)");
    expect(directPathGuard).toContain("[System.StringComparison]::OrdinalIgnoreCase");
    expect(directPathGuard).toContain("$ActualLeaf -cne $AppIdentifier");

    const collision = installedAcceptanceSrc.indexOf(
      "app data collision; refusing to touch pre-existing user data",
    );
    const roamingCreation = installedAcceptanceSrc.indexOf(
      "New-OwnedAppDirectory -Path $AppDataDirectory",
    );
    const localCreation = installedAcceptanceSrc.indexOf(
      "New-OwnedAppDirectory -Path $LocalAppDataDirectory",
    );
    const firstLaunch = installedAcceptanceSrc.indexOf("$Session1 = Start-InstalledApp");
    expect(collision).toBeGreaterThanOrEqual(0);
    expect(collision).toBeLessThan(roamingCreation);
    expect(collision).toBeLessThan(localCreation);
    expect(localCreation).toBeLessThan(firstLaunch);
    expect(
      installedAcceptanceSrc.indexOf("$AppDataDirectoryOwned = $true", roamingCreation),
    ).toBeGreaterThan(roamingCreation);
    expect(
      installedAcceptanceSrc.indexOf("$LocalAppDataDirectoryOwned = $true", localCreation),
    ).toBeGreaterThan(localCreation);

    const startInstalledApp = installedAcceptanceSrc.slice(
      installedAcceptanceSrc.indexOf("function Start-InstalledApp"),
      installedAcceptanceSrc.indexOf("function Stop-InstalledApp"),
    );
    expect(startInstalledApp).not.toMatch(
      /\.Environment\[['"](?:APPDATA|LOCALAPPDATA)['"]\]\s*=/,
    );

    const cleanupGuard = installedAcceptanceSrc.slice(
      installedAcceptanceSrc.indexOf("function Remove-OwnedKnownFolderAppDirectory"),
      installedAcceptanceSrc.indexOf("function Get-FreeLoopbackPort"),
    );
    expect(cleanupGuard).toContain("Assert-DirectKnownFolderAppPath");
    expect(cleanupGuard).toContain("-PathType Container");
    expect(cleanupGuard).toContain("$Item.PSIsContainer");
    expect(cleanupGuard).toContain("[System.IO.FileAttributes]::ReparsePoint");
    expect(cleanupGuard).toContain("Remove-Item -LiteralPath $CanonicalPath -Recurse");
    expect(installedAcceptanceSrc).toContain("if (-not $Cleanup.owned)");
    expect(installedAcceptanceSrc).toContain("skipped-process-cleanup-failed");
    expect(installedAcceptanceSrc).toContain("unsafe-or-failed");

    const finalProcessStop = installedAcceptanceSrc.lastIndexOf("Stop-InstalledProcessTrees");
    const finalDirectoryRemoval = installedAcceptanceSrc.lastIndexOf(
      "Remove-OwnedKnownFolderAppDirectory",
    );
    expect(finalProcessStop).toBeGreaterThanOrEqual(0);
    expect(finalProcessStop).toBeLessThan(finalDirectoryRemoval);
    expect(installedAcceptanceSrc).toContain("roamingAppDirectory = $null");
    expect(installedAcceptanceSrc).toContain("localAppDirectory = $null");
    expect(installedAcceptanceSrc).toContain("memoryDirectory = $null");
    expect(installedAcceptanceSrc).toContain("cleanup = [ordered]@{");
  });

  it("compiles the sleeper fixture with bounded Windows PowerShell 5.1 before acceptance", () => {
    expect(installerSmokeWorkflowSrc).toMatch(
      /- name: Exercise the exact installer artifact\n {8}shell: pwsh/,
    );
    expect(releaseInstallerSrc).toContain("function New-SleeperFixture");
    expect(releaseInstallerSrc).toContain(
      "'System32\\WindowsPowerShell\\v1.0\\powershell.exe'",
    );
    for (const argument of [
      "'-NoLogo'",
      "'-NoProfile'",
      "'-NonInteractive'",
      "'-ExecutionPolicy', 'Bypass'",
    ]) {
      expect(releaseInstallerSrc).toContain(argument);
    }
    expect(releaseInstallerSrc).toContain("-Scenario 'compile-sleeper-fixture'");
    expect(releaseInstallerSrc).toContain("-TimeoutMilliseconds 60000");
    expect(releaseInstallerSrc).toContain("PSEdition -ceq 'Desktop'");
    expect(releaseInstallerSrc).toContain("$CompilerVersion.Major -eq 5");
    expect(releaseInstallerSrc).toContain("$CompilerVersion.Minor -eq 1");
    expect(releaseInstallerSrc).toContain("sleeper fixture is a regular non-reparse file");
    expect(releaseInstallerSrc).toContain("New-ProcessIdentity -Process $Probe");

    const generatedScriptStart = releaseInstallerSrc.indexOf("$CompileScript = @'");
    const generatedScriptEnd = releaseInstallerSrc.indexOf(
      "\n'@\n    [System.IO.File]::WriteAllText",
      generatedScriptStart,
    );
    const addTypeLocations = [...releaseInstallerSrc.matchAll(/\bAdd-Type\b/g)].map(
      (match) => match.index,
    );
    expect(generatedScriptStart).toBeGreaterThanOrEqual(0);
    expect(generatedScriptEnd).toBeGreaterThan(generatedScriptStart);
    expect(addTypeLocations).toHaveLength(1);
    expect(addTypeLocations[0]).toBeGreaterThan(generatedScriptStart);
    expect(addTypeLocations[0]).toBeLessThan(generatedScriptEnd);

    const fixtureValidation = releaseInstallerSrc.indexOf("$Sleeper = New-SleeperFixture");
    const productAcceptance = releaseInstallerSrc.indexOf("& $ProductAcceptance");
    const sidecarCopy = releaseInstallerSrc.indexOf(
      "Copy-Item -LiteralPath $Sleeper -Destination $Path -Force",
    );
    const sidecarStart = releaseInstallerSrc.indexOf(
      "Start-Process -FilePath $Path -PassThru -WindowStyle Hidden",
    );
    expect(fixtureValidation).toBeGreaterThanOrEqual(0);
    expect(fixtureValidation).toBeLessThan(productAcceptance);
    expect(fixtureValidation).toBeLessThan(sidecarCopy);
    expect(fixtureValidation).toBeLessThan(sidecarStart);
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
