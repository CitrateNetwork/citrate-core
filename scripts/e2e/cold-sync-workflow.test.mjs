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

// A python3 that stands in for the harness: records its arguments, writes FAKE_VERDICT (default a
// PASS) where --out points and exits FAKE_EXIT (default 0). Any other python3 call is the real one.
function fakePython() {
  const bin = path.join(dir, "bin");
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, "python3"), `#!/bin/sh
case "$1" in
  scripts/e2e/cold-sync.py)
    printf '%s\\n' "$@" > "${dir}/harness-args"
    out=""; prev=""
    for a in "$@"; do [ "$prev" = "--out" ] && out="$a"; prev="$a"; done
    mkdir -p "$out"
    if [ -n "\${FAKE_VERDICT:-}" ]; then printf '%s' "$FAKE_VERDICT" > "$out/verdict.json"
    else echo '{"verdict":"PASS","reasons":[],"rules":[]}' > "$out/verdict.json"; fi
    exit "\${FAKE_EXIT:-0}" ;;
  *) exec "${REAL_PY}" "$@" ;;
esac
`, { mode: 0o755 });
  return bin;
}

describe("Cold sync", () => {
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

describe("Paths, Authenticate, Install the node's build deps", () => {
  it("Paths: OUT and the component build root under RUNNER_TEMP", () => {
    const env = path.join(dir, "gh-env");
    const r = runStep("Paths", { RUNNER_TEMP: "/rt", GITHUB_ENV: env }, dir);
    expect(r.status, r.stderr).toBe(0);
    expect(fs.readFileSync(env, "utf8")).toBe("OUT=/rt/cold-sync\nCITRATE_COMPONENT_BUILD_ROOT=/rt/component-build\n");
  });

  it("Authenticate: rewrites github.com fetches with the token, in the global git config only", () => {
    const home = fs.mkdtempSync(path.join(dir, "home-"));
    const r = runStep("Authenticate to citrate-chain (read-only)", { HOME: home, CHAIN_TOKEN: "tok123" }, dir);
    expect(r.status, r.stderr).toBe(0);
    const cfg = fs.readFileSync(path.join(home, ".gitconfig"), "utf8");
    expect(cfg).toContain('[url "https://x-access-token:tok123@github.com/"]');
    expect(cfg).toContain("insteadOf = https://github.com/");
  });

  it("Install the node's build deps: apt-get through sudo", () => {
    const bin = path.join(dir, "sudo-bin");
    fs.mkdirSync(bin);
    fs.writeFileSync(path.join(bin, "sudo"), `#!/bin/sh\nprintf '%s\\n' "$*" >> "${dir}/sudo-log"\n`, { mode: 0o755 });
    const r = runStep("Install the node's build deps", { PATH: `${bin}:${process.env.PATH}` }, dir);
    expect(r.status, r.stderr).toBe(0);
    const log = fs.readFileSync(path.join(dir, "sudo-log"), "utf8").split("\n").filter(Boolean);
    expect(log[0]).toBe("apt-get update");
    expect(log[1]).toMatch(/^apt-get install -y .*\bclang\b.*\bprotobuf-compiler\b/);
  });
});

describe("Build the pinned node sidecar", () => {
  const STEP = "Build the pinned node sidecar (scripts/build-sidecar.sh)";
  // A checkout with the real component-pins.mjs and components.json, a build-sidecar.sh that prints
  // `line`, and a git that records the clone.
  function build(line) {
    const root = path.join(dir, "repo");
    fs.mkdirSync(path.join(root, "scripts"), { recursive: true });
    fs.mkdirSync(path.join(root, "release"), { recursive: true });
    fs.copyFileSync(path.join(repo, "scripts", "component-pins.mjs"), path.join(root, "scripts", "component-pins.mjs"));
    fs.copyFileSync(path.join(repo, "release", "components.json"), path.join(root, "release", "components.json"));
    fs.writeFileSync(path.join(root, "scripts", "build-sidecar.sh"), `#!/bin/sh\necho "building $*"\necho "${line}"\n`, { mode: 0o755 });
    const bin = path.join(dir, "git-bin");
    fs.mkdirSync(bin);
    fs.writeFileSync(path.join(bin, "git"), `#!/bin/sh\nprintf '%s\\n' "$*" >> "${dir}/git-log"\n`, { mode: 0o755 });
    return runStep(STEP, { PATH: `${bin}:${process.env.PATH}`, ...outputs() }, root);
  }
  const ghOutput = () => Object.fromEntries(fs.readFileSync(path.join(dir, "gh-output"), "utf8").split("\n").filter(Boolean).map((l) => l.split(/=(.*)/s).slice(0, 2)));

  it("clones citrate-chain, builds, and outputs the binary and its digest next to the pin", () => {
    const built = path.join(dir, "citrate-x86_64-unknown-linux-gnu");
    fs.writeFileSync(built, "bin");
    const digest = "cd".repeat(32);
    const r = build(`citrate-node x86_64-unknown-linux-gnu ${digest} ${built}`);
    expect(r.status, r.stderr).toBe(0);
    expect(fs.readFileSync(path.join(dir, "git-log"), "utf8")).toMatch(/^clone --quiet https:\/\/github\.com\/CitrateNetwork\/citrate-chain\.git \.\.\/citrate-chain$/m);
    expect(ghOutput()).toEqual({ bin: built, sha256: digest });
    const pin = JSON.parse(fs.readFileSync(path.join(repo, "release", "components.json"), "utf8"))
      .components.find((c) => c.id === "citrate-node").artifacts["x86_64-unknown-linux-gnu"].sha256;
    expect(fs.readFileSync(path.join(dir, "summary"), "utf8")).toContain(`pinned \`${pin}\``);
  });

  it("refuses a build output that is not the node, or names no file", () => {
    expect(build(`mem-mcp x86_64-unknown-linux-gnu ${"cd".repeat(32)} /bin/sh`).status).toBe(1);
    fs.rmSync(path.join(dir, "repo"), { recursive: true }); fs.rmSync(path.join(dir, "git-bin"), { recursive: true });
    const r = build(`citrate-node x86_64-unknown-linux-gnu ${"cd".repeat(32)} ${path.join(dir, "absent")}`);
    expect(r.status).toBe(1);
    expect(r.stdout).toMatch(/::error::unexpected build output/);
  });
});

describe("Negative case (wrong genesis must be refused)", () => {
  const STEP = "Negative case (wrong genesis must be refused)";
  const G = "0x" + "ab".repeat(32);
  function negative(verdict, exit, args = ["--profiles", "src-tauri/network/profiles.json"]) {
    const root = checkout(true);
    fs.mkdirSync(path.join(root, "src-tauri", "addresses"), { recursive: true });
    fs.writeFileSync(path.join(root, "src-tauri", "addresses", "40204.json"), JSON.stringify({ chainId: 40204, genesisHash: G, addresses: { ValidatorRegistry: "0x" + "1".repeat(40) } }));
    fs.writeFileSync(path.join(root, "src-tauri", "network", "profiles.json"), JSON.stringify({
      active: "a", profiles: { a: { genesisHash: G, addressBook: "addresses/40204.json" }, b: { genesisHash: "0x" + "ef".repeat(32), addressBook: "addresses/40204.json" } },
    }));
    fs.mkdirSync(OUT(), { recursive: true });
    fs.writeFileSync(path.join(OUT(), "profile-args"), args.join("\n") + "\n");
    return runStep(STEP, {
      PATH: `${fakePython()}:${process.env.PATH}`, OUT: OUT(), NODE_BIN: "/opt/citrate", ...outputs(),
      FAKE_VERDICT: JSON.stringify(verdict), FAKE_EXIT: String(exit),
    }, root);
  }
  const rules = (st) => Object.entries(st).map(([rule, status]) => ({ rule, status, detail: "" }));
  const REFUSED = { verdict: "FAIL", reasons: ["endpoint_identity: x", "local_genesis: y"], rules: rules({ endpoint_identity: "FAIL", local_genesis: "FAIL", reaches_target: "skip" }) };

  it("flips the profile genesis (and the book's to match), runs the harness on it, and passes on a refusal", () => {
    const r = negative(REFUSED, 1);
    expect(r.status, r.stderr + r.stdout).toBe(0);
    expect(r.stdout).toMatch(/REFUSED as required/);
    const wrong = JSON.parse(fs.readFileSync(path.join(OUT(), "wrong-profile.json"), "utf8"));
    expect(wrong.genesisHash).toBe(G.slice(0, -1) + "0");
    expect(JSON.parse(fs.readFileSync(path.join(OUT(), "wrong-book.json"), "utf8")).genesisHash).toBe(wrong.genesisHash);
    const a = fs.readFileSync(path.join(dir, "harness-args"), "utf8").split("\n");
    expect(a[a.indexOf("--profile") + 1]).toBe(path.join(OUT(), "wrong-profile.json"));
    expect(a[a.indexOf("--node-bin") + 1]).toBe("/opt/citrate");
  });

  it("uses the named profile, and a genesis ending in 0 flips to 1", () => {
    const named = negative(REFUSED, 1, ["--profiles", "src-tauri/network/profiles.json", "--name", "b"]);
    expect(named.status, named.stderr).toBe(0);
    expect(JSON.parse(fs.readFileSync(path.join(OUT(), "wrong-profile.json"), "utf8")).genesisHash).toBe("0x" + "ef".repeat(31) + "e0");
  });

  const notRefused = {
    "a PASS": [{ verdict: "PASS", reasons: [], rules: rules({ endpoint_identity: "ok", local_genesis: "ok", reaches_target: "ok" }) }, 0],
    "an input refusal (exit 2)": [{ ...REFUSED }, 2],
    "a refusal by the endpoint only": [{ verdict: "FAIL", reasons: [], rules: rules({ endpoint_identity: "FAIL", local_genesis: "ok", reaches_target: "skip" }) }, 1],
    "a refusal that still synced": [{ verdict: "FAIL", reasons: [], rules: rules({ endpoint_identity: "FAIL", local_genesis: "FAIL", reaches_target: "ok" }) }, 1],
  };
  for (const [name, [v, code]] of Object.entries(notRefused)) {
    it(`fails the step on ${name}`, () => {
      const r = negative(v, code);
      expect(r.status).toBe(1);
      expect(r.stdout).toMatch(/NOT refused/);
    });
  }
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
