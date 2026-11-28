// @vitest-environment node
//
// created: 2026-11-28 | branch: feat/v60-lq-wp18-review-record
// author: Larry Klosowski (LQ lane, LQ-WP18; Claude Opus 5.5)
// status: active
//
// LQ-WP18: the review-record check (scripts/ci/lq/review-matrix.mjs, check `lq / review-record`).
// One case per row of the skill matrix (planset lanes/LQ_NON_REGRESSION_GUARD.md, LQ-WP18 step 4),
// the two Gherkin scenarios, the independence rule and the mutation-campaign rule. The CLI cases
// run the script against a scratch git repo, as the workflow does against the checkout.
import { afterEach, describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync, spawnSync } from "node:child_process";
import { evaluate, normalizeSkill, parseReviewRecord, requiredSkills, selfTest } from "./review-matrix.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(here, "review-matrix.mjs");

const block = (o) => [
  `reviewer: ${o.reviewer ?? "reviewer-a"}`,
  `context: ${o.context ?? "ctx-a"}`,
  `skills: ${(o.skills ?? []).join(", ")}`,
  `findings: ${o.findings ?? "0 (none)"}`,
  ...(o.measured === null ? [] : [`measured: ${o.measured ?? "npx vitest run scripts/ci/lq: 1 file, 30 passed"}`]),
  ...(o.mutation ? [`mutation: ${o.mutation}`] : []),
].join("\n");

const body = (...blocks) => `## Summary\n\nchange.\n\n## Review record\n\n${blocks.join("\n\n")}\n\n## Test plan\n\n- [x] ran\n`;

const two = (skillsA, skillsB = ["differential-review"], extra = {}) => body(
  block({ reviewer: "reviewer-a", context: "agent-1111", skills: skillsA, ...extra.a }),
  block({ reviewer: "reviewer-b", context: "agent-2222", skills: skillsB, ...extra.b }),
);

const skillsFor = (files, opts = {}) => requiredSkills(files, opts).map((r) => r.skill);

describe("skill matrix: one case per row", () => {
  it("every PR needs differential-review", () => {
    expect(skillsFor(["README.md"])).toEqual(["differential-review"]);
  });
  it("config, defaults and capability ACLs need sharp-edges and insecure-defaults", () => {
    for (const f of ["src/shell/state.ts", "src-tauri/tauri.conf.json", "src-tauri/capabilities/default.json", "src-tauri/permissions/l1-commands.toml", "release/flags.json"]) {
      expect(skillsFor([f]), f).toEqual(expect.arrayContaining(["sharp-edges", "insecure-defaults"]));
    }
  });
  it("a new dependency or bundled binary needs supply-chain-risk-auditor", () => {
    for (const f of ["Cargo.lock", "src-tauri/Cargo.toml", "package.json", "package-lock.json", "release/components.json", "src-tauri/runtime-deps.sha256"]) {
      expect(skillsFor([f]), f).toContain("supply-chain-risk-auditor");
    }
  });
  it("a workflow change needs agentic-actions-auditor", () => {
    expect(skillsFor([".github/workflows/ci.yml"])).toContain("agentic-actions-auditor");
    expect(skillsFor([".github/actions/setup/action.yml"])).toContain("agentic-actions-auditor");
  });
  it("crypto or key material needs constant-time-analysis and zeroize-audit", () => {
    for (const f of ["kit/src/custody.rs", "kit/src/secret.rs", "kit/src/siwe.rs", "kit/src/eip712.rs", "src-tauri/src/keychain.rs", "src-tauri/src/updater_sign.rs"]) {
      expect(skillsFor([f]), f).toEqual(expect.arrayContaining(["constant-time-analysis", "zeroize-audit"]));
    }
  });
  it("contracts need entry-point-analyzer, building-secure-contracts and spec-to-code-compliance", () => {
    expect(skillsFor(["contracts/src/WorkMarket.sol"])).toEqual(expect.arrayContaining(["entry-point-analyzer", "building-secure-contracts", "spec-to-code-compliance"]));
  });
  it("money path, authz and state machines need property-based-testing and mutation-testing", () => {
    for (const f of ["kit/src/ceremony.rs", "kit/src/payouts_gate.rs", "src-tauri/src/agent_grants.rs", "src/agent/approvalCards.ts", "src-tauri/src/deploy_gate.rs"]) {
      expect(skillsFor([f]), f).toEqual(expect.arrayContaining(["property-based-testing", "mutation-testing"]));
    }
  });
  it("production code needs semgrep and codeql; a test file alone does not", () => {
    expect(skillsFor(["src-tauri/src/model.rs"])).toEqual(expect.arrayContaining(["semgrep", "codeql"]));
    expect(skillsFor(["src/shell/Thing.tsx"])).toEqual(expect.arrayContaining(["semgrep", "codeql"]));
    expect(skillsFor(["src-tauri/src/model_tests.rs"])).not.toContain("semgrep");
    expect(skillsFor(["src/shell/Thing.test.tsx"])).not.toContain("codeql");
  });
  it("a protocol or spec implementation needs spec-to-code-compliance", () => {
    for (const f of ["src-tauri/src/hermes_mcp.rs", "src-tauri/src/lanes/l1/acp_host.rs", "src-tauri/src/oauth_cimd.rs"]) {
      expect(skillsFor([f]), f).toContain("spec-to-code-compliance");
    }
  });
  it("a large surface needs audit-context-building", () => {
    expect(skillsFor(["src/a.ts"], { changedLines: 1500 })).toContain("audit-context-building");
    expect(skillsFor(["src/a.ts"], { changedLines: 100 })).not.toContain("audit-context-building");
  });
  it("a finding fixed needs variant-analysis", () => {
    expect(skillsFor(["src/a.ts"], { title: "fix(l6): refuse a forged callback" })).toContain("variant-analysis");
    expect(skillsFor(["src/a.ts"], { labels: ["finding-fix"] })).toContain("variant-analysis");
    expect(skillsFor(["src/a.ts"], { title: "feat: a thing" })).not.toContain("variant-analysis");
  });
  it("arithmetic with units needs dimensional-analysis", () => {
    for (const f of ["release/budgets.json", "src-tauri/src/daemon_budget.rs", "src-tauri/src/gas_estimate.rs"]) {
      expect(skillsFor([f]), f).toContain("dimensional-analysis");
    }
  });
});

describe("review record rules", () => {
  it("Gherkin: a workflow change without the agentic-actions audit fails naming the missing skill", () => {
    const r = evaluate({ files: [".github/workflows/ci.yml"], body: two(["differential-review"]) });
    expect(r.ok).toBe(false);
    expect(r.errors.join("\n")).toMatch(/agentic-actions-auditor/);
  });
  it("the same workflow change passes once one record lists agentic-actions-auditor", () => {
    const r = evaluate({ files: [".github/workflows/ci.yml"], body: two(["differential-review", "agentic-actions-auditor:agentic-actions-auditor"]) });
    expect(r.errors).toEqual([]);
    expect(r.ok).toBe(true);
  });
  it("Gherkin: same context id fails (reviewers not independent)", () => {
    const b = body(block({ context: "agent-1111", skills: ["differential-review"] }), block({ reviewer: "reviewer-b", context: "Agent-1111", skills: ["differential-review"] }));
    const r = evaluate({ files: ["README.md"], body: b });
    expect(r.ok).toBe(false);
    expect(r.errors.join("\n")).toMatch(/reviewers not independent/);
  });
  it("fewer than two reviewer blocks fails", () => {
    const r = evaluate({ files: ["README.md"], body: body(block({ skills: ["differential-review"] })) });
    expect(r.ok).toBe(false);
    expect(r.errors.join("\n")).toMatch(/two independent reviewers/);
  });
  it("no review record section fails", () => {
    const r = evaluate({ files: ["README.md"], body: "## Summary\n\nnothing\n" });
    expect(r.ok).toBe(false);
    expect(r.errors.join("\n")).toMatch(/## Review record/);
  });
  it("a block without measured fails", () => {
    const r = evaluate({ files: ["README.md"], body: two(["differential-review"], ["differential-review"], { b: { measured: null } }) });
    expect(r.ok).toBe(false);
    expect(r.errors.join("\n")).toMatch(/measured/);
  });
  it("template placeholders and HTML comments do not count as a record", () => {
    const tpl = "## Review record\n\n<!--\nreviewer: x\ncontext: y\nskills: differential-review\nfindings: 0\nmeasured: z\n-->\nreviewer: <name>\ncontext: <fresh session id>\nskills: <list>\nfindings: <count>\nmeasured: <commands>\n";
    const r = evaluate({ files: ["README.md"], body: tpl });
    expect(r.ok).toBe(false);
  });
  it("missing mutation campaign on a money path fails", () => {
    const skills = ["differential-review", "property-based-testing", "mutation-testing", "semgrep", "codeql"];
    const r = evaluate({ files: ["kit/src/ceremony.rs"], body: two(skills) });
    expect(r.ok).toBe(false);
    expect(r.errors.join("\n")).toMatch(/mutation campaign/);
  });
  it("a mutation campaign with no survivors on a money path passes; a survivor blocks", () => {
    const skills = ["differential-review", "property-based-testing", "mutation-testing", "semgrep", "codeql"];
    const ok = evaluate({ files: ["kit/src/ceremony.rs"], body: two(skills, ["differential-review"], { a: { mutation: "cargo-mutants: generated 40, killed 40, surviving 0" } }) });
    expect(ok.errors).toEqual([]);
    const bad = evaluate({ files: ["kit/src/ceremony.rs"], body: two(skills, ["differential-review"], { a: { mutation: "cargo-mutants: generated 40, killed 38, surviving 2 (equivalent)" } }) });
    expect(bad.ok).toBe(false);
    expect(bad.errors.join("\n")).toMatch(/surviving/);
  });
  it("skills may be a bulleted list under skills:", () => {
    const b = body(
      "reviewer: a\ncontext: c1\nskills:\n  - differential-review\n  - agentic-actions-auditor\nfindings: 0\nmeasured: ran it",
      "reviewer: b\ncontext: c2\nskills: differential-review\nfindings: 1 low (fixed)\nmeasured: ran it",
    );
    const r = parseReviewRecord(b);
    expect(r.blocks[0].skills).toEqual(["differential-review", "agentic-actions-auditor"]);
    expect(evaluate({ files: [".github/workflows/x.yml"], body: b }).ok).toBe(true);
  });
  it("skill names normalize across plugin prefixes", () => {
    expect(normalizeSkill("static-analysis:codeql")).toBe("codeql");
    expect(normalizeSkill(" Differential-Review ")).toBe("differential-review");
    expect(normalizeSkill("trailofbits/sharp-edges")).toBe("sharp-edges");
  });
  it("control characters in PR text never reach the output", () => {
    const r = evaluate({ files: [".github/workflows/x.yml"], body: two(["differential-review\u001b[31m"]) });
    expect(r.errors.join("\n")).not.toMatch(/[\u0000-\u001f]/);
  });
});

describe("CLI", () => {
  let dir;
  afterEach(() => { if (dir) fs.rmSync(dir, { recursive: true, force: true }); dir = undefined; });
  const git = (...a) => execFileSync("git", a, { cwd: dir, encoding: "utf8" });
  const scratch = () => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "lq-review-"));
    git("init", "-q", "-b", "main");
    git("config", "user.email", "t@t"); git("config", "user.name", "t"); git("config", "commit.gpgsign", "false");
    fs.writeFileSync(path.join(dir, "README.md"), "x\n");
    git("add", "README.md"); git("commit", "-qm", "base");
    git("checkout", "-qb", "feat/x");
    fs.mkdirSync(path.join(dir, ".github/workflows"), { recursive: true });
    fs.writeFileSync(path.join(dir, ".github/workflows/ci.yml"), "name: ci\n");
    git("add", ".github/workflows/ci.yml"); git("commit", "-qm", "head");
  };
  const run = (...a) => spawnSync(process.execPath, [SCRIPT, ...a], { cwd: dir, encoding: "utf8" });

  it("--base prints the required skills for the diff", () => {
    scratch();
    const r = run("--base", "main", "--root", dir);
    expect(r.status).toBe(0);
    expect(r.stdout).toMatch(/differential-review/);
    expect(r.stdout).toMatch(/agentic-actions-auditor/);
  });
  it("check refuses a body without the workflow audit (exit 1) and passes with it (exit 0)", () => {
    scratch();
    const f = path.join(dir, "body.md");
    fs.writeFileSync(f, two(["differential-review"]));
    expect(run("check", "--base", "main", "--root", dir, "--body", f).status).toBe(1);
    fs.writeFileSync(f, two(["differential-review", "agentic-actions-auditor"]));
    const ok = run("check", "--base", "main", "--root", dir, "--body", f);
    expect(ok.stdout).toMatch(/PASS/);
    expect(ok.status).toBe(0);
  });
  it("an unknown flag is a usage error (exit 2)", () => {
    scratch();
    expect(run("check", "--nope", "x").status).toBe(2);
  });
  it("--self-test passes", () => {
    expect(selfTest().failures).toEqual([]);
  });
});
