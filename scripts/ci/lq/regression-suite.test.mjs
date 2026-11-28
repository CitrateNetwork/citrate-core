// @vitest-environment node
//
// LQ-WP12: the shared regression suite (scripts/ci/lq/regression-suite.mjs, check
// `lq / regression-suite`). Registry, path mapping, the cargo filter set and the vitest file list,
// the verdict over the runners' own output, and the characterization-first rule. The last case
// checks the registries committed in this repo.
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  cargoArgs,
  fsReader,
  gate,
  loadRegistry,
  matchGlob,
  parseCargoLog,
  plan,
  removedSuites,
  surfacesFor,
  verify,
} from "./regression-suite.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.join(here, "..", "..", "..");

// An in-memory tree: { "<path>": "<contents>" }.
function reader(tree) {
  return {
    read: (p) => (p in tree ? tree[p] : null),
    list: (dir) =>
      Object.keys(tree)
        .filter((p) => path.posix.dirname(p) === dir)
        .map((p) => path.posix.basename(p)),
  };
}

const SURFACES = {
  version: 1,
  surfaces: [
    {
      surface: "forced-update-gate",
      owner_lane: "L3",
      paths: ["src/shell/UpdateBanner.tsx", "src-tauri/src/update_policy*.rs"],
      suites: [
        { runner: "vitest", file: "src/shell/updateGate.test.tsx" },
        { runner: "cargo", filter: "update_policy::tests::" },
      ],
    },
  ],
};
const L0 = {
  version: 1,
  suites: [{ id: "SEAM-STORE", lane: "L0", runner: "vitest", files: ["src/shell/seams/store.test.ts"], covers: ["src/shell/store.ts"] }],
};
const L3 = {
  version: 1,
  suites: [
    { id: "L3-GATES", lane: "L3", runner: "vitest", files: ["src/shell/l3/gates.test.tsx"], covers: ["src/App.tsx"], tests: ["gate_one", "gate_two"] },
    { id: "L3-CARGO", lane: "L3", runner: "cargo", package: "citrate-core", filter: "lq_l3_gate::", covers: ["src-tauri/src/gate.rs"] },
  ],
};
function tree(extra = {}) {
  return reader({
    ".agentile/coverage/characterization.json": JSON.stringify(SURFACES),
    "ci/characterization.json": JSON.stringify(L0),
    "ci/characterization.d/l3.json": JSON.stringify(L3),
    "src/shell/updateGate.test.tsx": "x",
    "src/shell/seams/store.test.ts": "x",
    "src/shell/l3/gates.test.tsx": "x",
    ...extra,
  });
}

function vitestReport(files) {
  return {
    testResults: Object.entries(files).map(([name, tests]) => ({
      name: path.join("/w/repo", name),
      status: Object.values(tests).includes("failed") ? "failed" : "passed",
      assertionResults: Object.entries(tests).map(([title, status]) => ({ title, fullName: title, status })),
    })),
  };
}

const GREEN_CARGO = [
  "     Running unittests src/lib.rs (target/debug/deps/citrate_core-1)",
  "test update_policy::tests::a_tampered_minimum_never_gates ... ok",
  "test lq_l3_gate::gate_refuses ... ok",
  "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 3000 filtered out",
].join("\n");
const GREEN_VITEST = vitestReport({
  "src/shell/updateGate.test.tsx": { "gates on a minimum": "passed" },
  "src/shell/seams/store.test.ts": { "store methods": "passed" },
  "src/shell/l3/gates.test.tsx": { gate_one: "passed", gate_two: "passed" },
});

describe("regression-suite", () => {
  it("path to surface mapping", () => {
    expect(matchGlob("src-tauri/src/update_policy*.rs", "src-tauri/src/update_policy.rs")).toBe(true);
    expect(matchGlob("src-tauri/src/update_policy*.rs", "src-tauri/src/update_policy/x.rs")).toBe(false);
    expect(matchGlob("src/onboarding/**", "src/onboarding/steps/S6.tsx")).toBe(true);
    expect(matchGlob("src/onboarding/**", "src/onboardingx.tsx")).toBe(false);
    expect(matchGlob("src/bridge/", "src/bridge/a/b.ts")).toBe(true);
    expect(matchGlob("src/shell/store.ts", "src/shell/store.tsx")).toBe(false);
    const reg = loadRegistry(tree());
    expect(reg.errors).toEqual([]);
    expect(surfacesFor("src/shell/UpdateBanner.tsx", reg.surfaces).map((s) => s.surface)).toEqual(["forced-update-gate"]);
    expect(surfacesFor("src/shell/Other.tsx", reg.surfaces)).toEqual([]);
  });

  it("missing characterization fails", () => {
    const reg = loadRegistry(tree());
    const errs = gate({ changed: ["src-tauri/src/update_policy.rs"], branch: "feat/v60-l3-wp04-update", base: reg });
    expect(errs.join("\n")).toMatch(/register characterization for forced-update-gate first/);
    // A suite in ci/characterization.d at the base whose covers match the path characterizes it.
    const withChar = loadRegistry(
      tree({
        "ci/characterization.d/l3.json": JSON.stringify({
          ...L3,
          suites: [...L3.suites, { id: "L3-UPD", lane: "L3", runner: "cargo", package: "citrate-core", filter: "lq_l3_upd::", covers: ["src-tauri/src/update_policy.rs"] }],
        }),
      }),
    );
    expect(gate({ changed: ["src-tauri/src/update_policy.rs"], branch: "feat/v60-l3-wp04-update", base: withChar })).toEqual([]);
    // Tests and registry files are the characterization PR itself, never a behavior change.
    expect(gate({ changed: ["src-tauri/src/update_policy_tests.rs", "src/shell/UpdateBanner.test.tsx"], branch: "feat/v60-l3-wp00-char", base: reg })).toEqual([]);
    // The rule is for product-lane branches; L0, LQ and release branches only run the suites.
    expect(gate({ changed: ["src-tauri/src/update_policy.rs"], branch: "feat/v60-l0-wp11-seam", base: reg })).toEqual([]);
  });

  it("registered suite failing fails the group", () => {
    const reg = loadRegistry(tree());
    const ok = verify({ suites: reg.suites, cargo: { exit: 0, tests: parseCargoLog(GREEN_CARGO) }, vitest: GREEN_VITEST, root: "/w/repo" });
    expect(ok.filter((r) => !r.ok)).toEqual([]);
    const redCargo = GREEN_CARGO.replace("gate_refuses ... ok", "gate_refuses ... FAILED");
    const bad = verify({ suites: reg.suites, cargo: { exit: 101, tests: parseCargoLog(redCargo) }, vitest: GREEN_VITEST, root: "/w/repo" });
    expect(bad.find((r) => r.id === "L3-CARGO")).toMatchObject({ ok: false });
    expect(bad.find((r) => r.id === "L3-CARGO").reason).toMatch(/failed: lq_l3_gate::gate_refuses/);
    const redVitest = vitestReport({
      "src/shell/updateGate.test.tsx": { "gates on a minimum": "passed" },
      "src/shell/seams/store.test.ts": { "store methods": "failed" },
      "src/shell/l3/gates.test.tsx": { gate_one: "passed", gate_two: "passed" },
    });
    expect(verify({ suites: reg.suites, cargo: { exit: 0, tests: parseCargoLog(GREEN_CARGO) }, vitest: redVitest, root: "/w/repo" }).find((r) => r.id === "SEAM-STORE").ok).toBe(false);
  });

  it("a filter or file that runs nothing, or a listed test that did not pass, fails", () => {
    const reg = loadRegistry(tree());
    const noCargo = verify({ suites: reg.suites, cargo: { exit: 0, tests: parseCargoLog("test result: ok. 0 passed;") }, vitest: GREEN_VITEST, root: "/w/repo" });
    expect(noCargo.find((r) => r.id === "L3-CARGO").reason).toMatch(/selects no test/);
    const missingTest = vitestReport({
      "src/shell/updateGate.test.tsx": { "gates on a minimum": "passed" },
      "src/shell/seams/store.test.ts": { "store methods": "passed" },
      "src/shell/l3/gates.test.tsx": { gate_one: "passed", gate_two: "skipped" },
    });
    expect(verify({ suites: reg.suites, cargo: { exit: 0, tests: parseCargoLog(GREEN_CARGO) }, vitest: missingTest, root: "/w/repo" }).find((r) => r.id === "L3-GATES").reason).toMatch(/gate_two/);
    const noFile = vitestReport({ "src/shell/seams/store.test.ts": { "store methods": "passed" }, "src/shell/l3/gates.test.tsx": { gate_one: "passed", gate_two: "passed" } });
    const r = verify({ suites: reg.suites, cargo: { exit: 0, tests: parseCargoLog(GREEN_CARGO) }, vitest: noFile, root: "/w/repo" });
    expect(r.find((x) => x.id === "LQ:forced-update-gate/vitest").reason).toMatch(/updateGate\.test\.tsx/);
    // No runner output at all is a refusal, never a pass.
    expect(verify({ suites: reg.suites, cargo: null, vitest: null, root: "/w/repo" }).every((x) => !x.ok)).toBe(true);
  });

  it("cargo filter build", () => {
    const reg = loadRegistry(tree());
    const p = plan(reg.suites);
    expect(p.cargo.packages).toEqual(["citrate-core"]);
    expect(p.cargo.filters).toEqual(["lq_l3_gate::", "update_policy::tests::"]);
    expect(p.vitest.files).toEqual(["src/shell/l3/gates.test.tsx", "src/shell/seams/store.test.ts", "src/shell/updateGate.test.tsx"]);
    expect(cargoArgs(p)).toEqual(["test", "--locked", "--offline", "--no-fail-fast", "-p", "citrate-core", "--lib", "--", "lq_l3_gate::", "update_policy::tests::"]);
    expect(parseCargoLog("test a::b ... ok\ntest a::c ... FAILED\ntest a::d ... ignored, slow\n")).toEqual({ "a::b": "ok", "a::c": "FAILED", "a::d": "ignored" });
  });

  it("a suite registered at the merge base cannot disappear without ratchet-adr", () => {
    const base = loadRegistry(tree());
    const head = loadRegistry(tree({ "ci/characterization.d/l3.json": JSON.stringify({ ...L3, suites: [L3.suites[1]] }) }));
    expect(removedSuites(base.suites, head.suites, []).join("\n")).toMatch(/L3-GATES/);
    expect(removedSuites(base.suites, head.suites, ["ratchet-adr"])).toEqual([]);
  });

  it("an invalid registry is refused", () => {
    const dup = loadRegistry(tree({ "ci/characterization.d/l4.json": JSON.stringify({ version: 1, suites: [L0.suites[0]] }) }));
    expect(dup.errors.join("\n")).toMatch(/SEAM-STORE.*registered twice/);
    const missing = loadRegistry(tree({ "src/shell/l3/gates.test.tsx": null }));
    expect(missing.errors.join("\n")).toMatch(/gates\.test\.tsx.*does not exist/);
    const noFilter = loadRegistry(tree({ "ci/characterization.d/l4.json": JSON.stringify({ version: 1, suites: [{ id: "X", runner: "cargo", package: "citrate-core" }] }) }));
    expect(noFilter.errors.join("\n")).toMatch(/X: a cargo suite needs package and filter/);
    expect(loadRegistry(tree({ "ci/characterization.json": "{" })).errors.join("\n")).toMatch(/ci\/characterization\.json/);
  });

  it("the registries in this repo load, and every file they name exists", () => {
    const reg = loadRegistry(fsReader(repo));
    expect(reg.errors).toEqual([]);
    expect(reg.suites.length).toBeGreaterThan(30);
    expect(reg.surfaces.length).toBeGreaterThanOrEqual(10);
    for (const f of plan(reg.suites).vitest.files) expect(fs.existsSync(path.join(repo, f)), f).toBe(true);
  });
});
